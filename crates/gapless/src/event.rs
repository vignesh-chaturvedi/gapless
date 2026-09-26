use std::fmt;
use std::time::{Duration, SystemTime};

use solami::geyser::{SlotStatus, SubscribeUpdateTransactionInfo};

use crate::classify::DisconnectReason;

/// A transaction signature. Displays as base58.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Signature(pub [u8; 64]);

impl Signature {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        bytes.try_into().ok().map(Self)
    }
}

impl fmt::Display for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&bs58::encode(self.0).into_string())
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// Whether an update arrived as it happened or was replayed to close a gap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Live,
    Replay { incident: u64 },
}

/// A transaction delivered for the first time.
#[derive(Clone, Debug)]
pub struct Transaction {
    pub slot: u64,
    pub signature: Signature,
    /// Position within the slot.
    pub index: u64,
    pub origin: Origin,
    pub received_at: SystemTime,
    /// When Solami stamped the update.
    pub created_at: Option<SystemTime>,
    /// The full transaction and its status meta.
    pub info: SubscribeUpdateTransactionInfo,
}

/// An inclusive range of slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SlotRange {
    pub first: u64,
    pub last: u64,
}

impl SlotRange {
    pub fn new(first: u64, last: u64) -> Self {
        Self { first, last }
    }

    pub fn contains(&self, slot: u64) -> bool {
        (self.first..=self.last).contains(&slot)
    }

    pub fn len(&self) -> u64 {
        self.last.saturating_sub(self.first) + 1
    }

    pub fn is_empty(&self) -> bool {
        self.last < self.first
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Connecting {
        attempt: u32,
    },
    /// Replaying from `from_slot` until the stream reaches `target_slot`, the tip when this step began.
    Replaying {
        incident: u64,
        from_slot: u64,
        target_slot: u64,
    },
    Live,
    Backoff {
        delay: Duration,
        attempt: u32,
    },
    Stopped {
        reason: String,
    },
}

/// The stream dropped. `step` is 0 for the outage itself and N for the Nth replay attempt.
#[derive(Clone, Debug)]
pub struct Disconnect {
    pub incident: u64,
    pub step: u32,
    pub reason: DisconnectReason,
    /// The server's message, or ours.
    pub detail: String,
    pub at: SystemTime,
    pub last_complete_slot: Option<u64>,
    pub resume_from: Option<u64>,
}

/// One reconnect with `from_slot` while recovering from an incident.
#[derive(Clone, Debug)]
pub struct ReplayStep {
    pub attempt: u32,
    pub from_slot: u64,
    pub target_slot: u64,
    pub started_at: SystemTime,
    /// Why this step's stream ended before reaching the target, if it did.
    pub ended: Option<(DisconnectReason, String)>,
    pub transactions: u64,
}

/// An outage, from the first disconnect until the stream caught back up to the tip.
#[derive(Clone, Debug)]
pub struct Incident {
    pub id: u64,
    pub reason: DisconnectReason,
    pub detail: String,
    pub started_at: SystemTime,
    pub last_complete_slot: Option<u64>,
    /// Slots missed while disconnected: from the resume point to the tip at the first reconnect.
    pub gap: Option<SlotRange>,
    /// Slots that had already fallen out of Solami's replay horizon.
    pub unrecoverable: Option<SlotRange>,
    pub steps: Vec<ReplayStep>,
    /// Transactions delivered from replay.
    pub replayed: u64,
    /// Transactions dropped because they had already been delivered.
    pub duplicates: u64,
    pub recovered_at: Option<SystemTime>,
}

impl Incident {
    pub fn duration(&self) -> Option<Duration> {
        self.recovered_at?.duration_since(self.started_at).ok()
    }
}

/// A once-a-second snapshot while a stream is open.
#[derive(Clone, Debug, Default)]
pub struct Metrics {
    pub updates_per_sec: f64,
    pub transactions_per_sec: f64,
    pub highest_complete: Option<u64>,
    /// Latest tip at `processed`, from Solami's `GetSlot` and the stream itself.
    pub tip: Option<u64>,
    pub lag_slots: Option<u64>,
    pub delivered: u64,
    pub duplicates: u64,
    pub reconnects: u64,
    pub incidents: u64,
    /// Smoothed delay between Solami stamping a live update and us receiving it.
    pub latency_ms: Option<f64>,
    pub dedup_entries: usize,
}

#[derive(Clone, Debug)]
pub enum Event {
    Transaction(Box<Transaction>),
    Slot {
        slot: u64,
        status: SlotStatus,
        origin: Origin,
    },
    State(State),
    /// Solami's id for our current stream, found through the account API.
    ConnectionIdentified {
        conn_id: String,
    },
    Disconnected(Disconnect),
    /// Solami's connection history explained a disconnect that arrived as a bare end-of-stream.
    /// It can land after the incident's [`Event::Recovered`], so consumers should patch the
    /// incident by id.
    ReasonResolved {
        incident: u64,
        step: u32,
        reason: DisconnectReason,
    },
    Recovered(Incident),
    /// The handoff patch re-read `slots` after an incident's replay and delivered `recovered`
    /// transactions the resumed stream had dropped.
    HandoffPatched {
        incident: u64,
        slots: SlotRange,
        recovered: u64,
        error: Option<String>,
    },
    Metrics(Metrics),
}
