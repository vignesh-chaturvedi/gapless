//! HTTP and WebSocket routes. Documented in docs/api.md.

use std::path::Path as FsPath;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::broadcast::error::RecvError;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

use crate::app::App;
use crate::dto::now_ms;

/// Public demo: one kill or cut per this long, site-wide.
pub const PUBLIC_COOLDOWN_SECS: u64 = 300;
/// Public demo: the longest a visitor can keep the stream offline.
pub const PUBLIC_MAX_HOLD_SECS: u64 = 30;

/// The API, plus the built console when `console` points at it (`console/dist`). Unknown paths
/// fall back to its `index.html`, so deep links like `/incidents/3` load the app.
pub fn router(app: Arc<App>, console: Option<&FsPath>) -> Router {
    let api = Router::new()
        .route("/api/health", get(health))
        .route("/api/state", get(state))
        .route("/api/tape", get(tape))
        .route("/api/incidents", get(incidents))
        .route("/api/incidents/{id}", get(incident))
        .route("/api/chaos/kill", post(kill))
        .route("/api/chaos/cut", post(cut))
        .route("/api/chaos/slow", post(slow))
        .route("/api/chaos/patch", post(patch))
        .route("/api/{*rest}", any(no_such_route))
        .route("/ws", get(ws))
        .layer(CorsLayer::permissive())
        .with_state(app);
    match console {
        Some(dir) => api
            .fallback_service(ServeDir::new(dir).fallback(ServeFile::new(dir.join("index.html")))),
        None => api,
    }
}

type AppState = State<Arc<App>>;

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({ "error": message.into() }))).into_response()
}

async fn no_such_route() -> Response {
    error(StatusCode::NOT_FOUND, "no such API route; see docs/api.md")
}

async fn health(State(app): AppState) -> Json<serde_json::Value> {
    let sizes = app.shared.read().expect("shared lock").sizes.clone();
    Json(json!({ "ok": true, "mode": app.mode, "sizes": sizes }))
}

async fn state(State(app): AppState) -> Response {
    let shared = app.shared.read().expect("shared lock");
    Json(&shared.snapshot).into_response()
}

async fn tape(State(app): AppState) -> Response {
    let shared = app.shared.read().expect("shared lock");
    Json(&shared.tape).into_response()
}

#[derive(Deserialize)]
struct Limit {
    limit: Option<usize>,
}

async fn incidents(State(app): AppState, Query(q): Query<Limit>) -> Response {
    match app.store.list(q.limit.unwrap_or(50).min(500)).await {
        Ok(list) => Json(list).into_response(),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn incident(State(app): AppState, Path(id): Path<i64>) -> Response {
    match app.store.get(id).await {
        Ok(Some(incident)) => Json(incident).into_response(),
        Ok(None) => error(StatusCode::NOT_FOUND, format!("no incident {id}")),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Outage {
    /// Stay offline this long after the drop, so there's a real gap to replay.
    hold_secs: Option<u64>,
}

fn set_hold(app: &App, hold_secs: Option<u64>, label: String) {
    let mut chaos = app.chaos.lock().expect("chaos lock");
    chaos.hold_secs = hold_secs.filter(|s| *s > 0);
    chaos.pending_label = Some(label);
    app.control.hold(chaos.hold_secs.map(Duration::from_secs));
}

/// Public demo: claim the next outage, or say how long until one is allowed. Claiming and
/// checking happen under one lock, so two visitors can't both get through.
fn claim_outage(app: &App) -> Option<Response> {
    if !app.public {
        return None;
    }
    let mut chaos = app.chaos.lock().expect("chaos lock");
    let now = now_ms();
    match outage_wait(chaos.next_outage_at, now) {
        0 => {
            chaos.next_outage_at = Some(now + PUBLIC_COOLDOWN_SECS * 1000);
            None
        }
        wait => Some(error(
            StatusCode::TOO_MANY_REQUESTS,
            format!(
                "this public demo allows one outage every {} minutes; the next is in {}s",
                PUBLIC_COOLDOWN_SECS / 60,
                wait.div_ceil(1000)
            ),
        )),
    }
}

/// Milliseconds until the next outage is allowed (0: now).
fn outage_wait(next_at: Option<u64>, now: u64) -> u64 {
    next_at.map_or(0, |at| at.saturating_sub(now))
}

/// Public demo: holds are capped so a visitor can't keep the stream down.
fn capped_hold(app: &App, hold_secs: Option<u64>) -> Option<u64> {
    if app.public {
        hold_secs.map(|s| s.min(PUBLIC_MAX_HOLD_SECS))
    } else {
        hold_secs
    }
}

fn public_off(app: &App, what: &str) -> Option<Response> {
    app.public.then(|| {
        error(
            StatusCode::FORBIDDEN,
            format!("{what} is off on this public demo; run Gapless locally to try it"),
        )
    })
}

fn outage_label(what: &str, hold_secs: Option<u64>) -> String {
    match hold_secs.filter(|s| *s > 0) {
        Some(s) => format!("{what}, then offline for {s}s"),
        None => what.to_owned(),
    }
}

/// Kill our stream through Solami's account API. Offline, this falls back to a client cut.
async fn kill(State(app): AppState, body: Option<Json<Outage>>) -> Response {
    let Json(mut outage) = body.unwrap_or_default();
    outage.hold_secs = capped_hold(&app, outage.hold_secs);
    let conn_id = app
        .shared
        .read()
        .expect("shared lock")
        .snapshot
        .conn_id
        .clone();
    let Some(account) = app.account.clone() else {
        if let Some(response) = claim_outage(&app) {
            return response;
        }
        set_hold(
            &app,
            outage.hold_secs,
            outage_label("cut (offline mode has no account API)", outage.hold_secs),
        );
        app.control.cut();
        return Json(json!({ "ok": true, "method": "cut" })).into_response();
    };
    let Some(conn_id) = conn_id else {
        return error(
            StatusCode::CONFLICT,
            "our Solami connection isn't identified yet",
        );
    };
    if let Some(response) = claim_outage(&app) {
        return response;
    }
    set_hold(
        &app,
        outage.hold_secs,
        outage_label("killed through Solami's account API", outage.hold_secs),
    );
    match account.kill(&conn_id).await {
        Ok(killed) => {
            Json(json!({ "ok": true, "method": "kill", "connId": conn_id, "killed": killed }))
                .into_response()
        }
        Err(e) => {
            let mut chaos = app.chaos.lock().expect("chaos lock");
            chaos.pending_label = None;
            if app.public {
                chaos.next_outage_at = None; // nothing happened, so it doesn't use up the turn
            }
            drop(chaos);
            error(StatusCode::BAD_GATEWAY, e.to_string())
        }
    }
}

/// Drop the connection from our side.
async fn cut(State(app): AppState, body: Option<Json<Outage>>) -> Response {
    let Json(mut outage) = body.unwrap_or_default();
    outage.hold_secs = capped_hold(&app, outage.hold_secs);
    if let Some(response) = claim_outage(&app) {
        return response;
    }
    set_hold(
        &app,
        outage.hold_secs,
        outage_label("cut by the client", outage.hold_secs),
    );
    app.control.cut();
    Json(json!({ "ok": true, "method": "cut" })).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Slow {
    /// Sleep this long after each update; `null` or 0 restores full speed.
    per_update_ms: Option<u64>,
}

/// Make the consumer slow on purpose, until Solami's buffer fills.
async fn slow(State(app): AppState, Json(slow): Json<Slow>) -> Response {
    if let Some(response) = public_off(&app, "the slow consumer") {
        return response;
    }
    let per_update = slow.per_update_ms.filter(|ms| *ms > 0);
    app.chaos.lock().expect("chaos lock").throttle_ms = per_update;
    if per_update.is_some() {
        app.chaos.lock().expect("chaos lock").pending_label = Some("slow consumer".into());
    }
    app.control.throttle(per_update.map(Duration::from_millis));
    Json(json!({ "ok": true, "perUpdateMs": per_update })).into_response()
}

#[derive(Deserialize)]
struct Patch {
    enabled: bool,
}

/// Turn the replay-to-live handoff patch on or off.
async fn patch(State(app): AppState, Json(patch): Json<Patch>) -> Response {
    if let Some(response) = public_off(&app, "the handoff patch switch") {
        return response;
    }
    app.chaos.lock().expect("chaos lock").handoff_patch = patch.enabled;
    app.control.handoff_patch(patch.enabled);
    Json(json!({ "ok": true, "handoffPatch": patch.enabled })).into_response()
}

async fn ws(State(app): AppState, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| client(app, socket))
}

/// Send a hello, then everything the engine broadcasts. A client that falls behind gets a
/// fresh hello instead of the messages it missed.
async fn client(app: Arc<App>, mut socket: WebSocket) {
    let mut rx = app.hub.subscribe();
    if socket
        .send(Message::Text(app.hello().into()))
        .await
        .is_err()
    {
        return;
    }
    loop {
        tokio::select! {
            message = rx.recv() => match message {
                Ok(json) => {
                    if socket.send(Message::Text(json.as_ref().into())).await.is_err() {
                        return;
                    }
                }
                Err(RecvError::Lagged(_)) => {
                    if socket.send(Message::Text(app.hello().into())).await.is_err() {
                        return;
                    }
                }
                Err(RecvError::Closed) => return,
            },
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                Some(Ok(_)) => {}
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::outage_wait;

    #[test]
    fn public_outages_wait_out_the_cooldown() {
        assert_eq!(outage_wait(None, 1_000), 0);
        assert_eq!(outage_wait(Some(5_000), 1_000), 4_000);
        assert_eq!(outage_wait(Some(5_000), 5_000), 0);
        assert_eq!(outage_wait(Some(5_000), 9_000), 0);
    }
}
