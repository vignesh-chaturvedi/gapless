//! The listing watchdog against a mock of Solami's account API: when our stream leaves the
//! live-connection list, Gapless ends it at once instead of draining whatever the client still has
//! buffered, and learns the real reason from the connection history.

#[allow(dead_code)]
mod fake;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::extract::State;
use axum::routing::get;
use fake::{Ending, FakeChain, Script, UPDATES_PER_SLOT};
use futures::StreamExt;
use gapless::{AccountApi, Builder, DisconnectReason, Event, Gapless};
use serde_json::{Value, json};

#[derive(Default)]
struct Account {
    listings: AtomicUsize,
    /// Solami has closed our stream: it's gone from the live list and in the history.
    closed: AtomicBool,
    started_at: AtomicUsize,
}

async fn live(State(account): State<Arc<Account>>) -> axum::Json<Value> {
    // The first listing is the snapshot taken just before subscribing.
    let n = account.listings.fetch_add(1, Ordering::SeqCst);
    if n == 0 || account.closed.load(Ordering::SeqCst) {
        return axum::Json(json!([]));
    }
    let started_at = account.started_at.load(Ordering::SeqCst);
    axum::Json(
        json!([{ "conn_id": "ours", "started_at": started_at, "buffer_size": 8192, "buffer_pending": 8000 }]),
    )
}

async fn history(State(account): State<Arc<Account>>) -> axum::Json<Value> {
    if !account.closed.load(Ordering::SeqCst) {
        return axum::Json(json!([]));
    }
    let started_at = account.started_at.load(Ordering::SeqCst);
    axum::Json(
        json!([{ "conn_id": "ours", "started_at": started_at, "termination_reason": "backpressure" }]),
    )
}

#[tokio::test]
async fn a_stream_solami_stopped_listing_is_ended_without_draining() {
    let account = Arc::new(Account::default());
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    account.started_at.store(now as usize, Ordering::SeqCst);
    let app = Router::new()
        .route("/auth/connections/grpc", get(live))
        .route("/auth/connections/grpc/history", get(history))
        .with_state(account.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    // The stream delivers 10 slots, then goes quiet without closing: as if the rest were stuck
    // behind a slow consumer. The stall timeout is far away, so only the watchdog can end it.
    let chain = FakeChain::new(
        1_000,
        3_000,
        vec![Script::cut(10 * UPDATES_PER_SLOT, Ending::Hang, 0)],
    );
    let config = Builder::new("test-key")
        .program("Prog1111111111111111111111111111111111111")
        .account_api(Some(url.clone()))
        .stall_timeout(Duration::from_secs(60))
        .watch_interval(Some(Duration::from_millis(100)))
        .into_config()
        .unwrap();
    let (mut events, control) = Gapless::with_source(config, chain)
        .with_account(Some(AccountApi::new(url, "test-key")))
        .start();

    let run = async {
        let (mut identified, mut disconnect, mut resolved) = (None, None, None);
        while let Some(event) = events.next().await {
            match event {
                Event::ConnectionIdentified { conn_id } => {
                    identified = Some(conn_id);
                    account.closed.store(true, Ordering::SeqCst);
                }
                Event::Disconnected(d) if disconnect.is_none() => disconnect = Some(d),
                Event::ReasonResolved { reason, .. } => {
                    resolved = Some(reason);
                    break;
                }
                _ => {}
            }
        }
        (identified, disconnect, resolved)
    };
    let (identified, disconnect, resolved) = tokio::time::timeout(Duration::from_secs(20), run)
        .await
        .expect("the watchdog ended the stream well before the stall timeout");
    control.stop();

    assert_eq!(identified.as_deref(), Some("ours"));
    let d = disconnect.expect("a disconnect");
    assert_eq!(d.reason, DisconnectReason::ServerClosed);
    assert!(d.detail.contains("no longer lists"), "{}", d.detail);
    assert_eq!(d.last_complete_slot, Some(1_010));
    assert_eq!(
        resolved,
        Some(DisconnectReason::Backpressure),
        "history names the real reason"
    );
}
