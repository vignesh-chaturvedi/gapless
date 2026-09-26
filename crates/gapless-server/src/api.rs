//! HTTP and WebSocket routes. Documented in docs/api.md.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::broadcast::error::RecvError;
use tower_http::cors::CorsLayer;

use crate::app::App;

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/state", get(state))
        .route("/api/tape", get(tape))
        .route("/api/incidents", get(incidents))
        .route("/api/incidents/{id}", get(incident))
        .route("/api/chaos/kill", post(kill))
        .route("/api/chaos/cut", post(cut))
        .route("/api/chaos/slow", post(slow))
        .route("/api/chaos/patch", post(patch))
        .route("/ws", get(ws))
        .layer(CorsLayer::permissive())
        .with_state(app)
}

type AppState = State<Arc<App>>;

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({ "error": message.into() }))).into_response()
}

async fn health(State(app): AppState) -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "mode": app.mode }))
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

fn outage_label(what: &str, hold_secs: Option<u64>) -> String {
    match hold_secs.filter(|s| *s > 0) {
        Some(s) => format!("{what}, then offline for {s}s"),
        None => what.to_owned(),
    }
}

/// Kill our stream through Solami's account API. Offline, this falls back to a client cut.
async fn kill(State(app): AppState, body: Option<Json<Outage>>) -> Response {
    let Json(outage) = body.unwrap_or_default();
    let conn_id = app
        .shared
        .read()
        .expect("shared lock")
        .snapshot
        .conn_id
        .clone();
    let Some(account) = app.account.clone() else {
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
            app.chaos.lock().expect("chaos lock").pending_label = None;
            error(StatusCode::BAD_GATEWAY, e.to_string())
        }
    }
}

/// Drop the connection from our side.
async fn cut(State(app): AppState, body: Option<Json<Outage>>) -> Response {
    let Json(outage) = body.unwrap_or_default();
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
