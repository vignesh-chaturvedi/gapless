use std::collections::VecDeque;
use std::sync::{Arc, Mutex, RwLock};

use gapless::{AccountApi, Control};
use tokio::sync::broadcast;

use crate::dto::{IncidentDto, LogDto, SlotCell, Snapshot, TxDto, WsMessage};
use crate::store::Store;

pub const RECENT_TXS: usize = 200;
pub const RECENT_LOG: usize = 150;
pub const TAPE_SLOTS: usize = 600;

/// State shared between the engine (writer), the HTTP handlers and WebSocket clients.
pub struct App {
    pub mode: &'static str,
    pub program: String,
    pub control: Control,
    /// Solami's account API. `None` offline.
    pub account: Option<AccountApi>,
    pub store: Store,
    pub hub: broadcast::Sender<Arc<str>>,
    pub shared: RwLock<Shared>,
    pub chaos: Mutex<Chaos>,
}

/// What a newly connected console needs, kept current by the engine.
pub struct Shared {
    pub snapshot: Snapshot,
    pub tape: Vec<SlotCell>,
    pub incidents: Vec<IncidentDto>,
    pub txs: VecDeque<TxDto>,
    pub log: VecDeque<LogDto>,
}

/// Settings the chaos panel changed, and how the next incident was caused.
#[derive(Debug)]
pub struct Chaos {
    pub handoff_patch: bool,
    pub throttle_ms: Option<u64>,
    pub hold_secs: Option<u64>,
    /// Consumed by the next incident that opens.
    pub pending_label: Option<String>,
}

impl App {
    pub fn broadcast(&self, message: &WsMessage<'_>) {
        if self.hub.receiver_count() == 0 {
            return;
        }
        match serde_json::to_string(message) {
            Ok(json) => {
                let _ = self.hub.send(Arc::from(json));
            }
            Err(e) => tracing::warn!("serializing a WebSocket message: {e}"),
        }
    }

    /// The first message a WebSocket client receives.
    pub fn hello(&self) -> String {
        let shared = self.shared.read().expect("shared lock");
        let txs: Vec<TxDto> = shared.txs.iter().cloned().collect();
        let log: Vec<LogDto> = shared.log.iter().cloned().collect();
        serde_json::to_string(&WsMessage::Hello {
            snapshot: &shared.snapshot,
            tape: &shared.tape,
            incidents: &shared.incidents,
            txs: &txs,
            log: &log,
        })
        .unwrap_or_else(|_| "{}".into())
    }
}
