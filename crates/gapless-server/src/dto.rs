//! The JSON the console reads: REST responses and WebSocket messages. Field names are
//! camelCase; see docs/api.md.

use gapless::{DisconnectReason, Incident, Metrics, SlotRange, State};
use gapless_verify::Report;
use serde::Serialize;

pub fn unix_ms(t: std::time::SystemTime) -> u64 {
    t.duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn now_ms() -> u64 {
    unix_ms(std::time::SystemTime::now())
}

#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum StateDto {
    Connecting {
        attempt: u32,
    },
    Replaying {
        incident: u64,
        from_slot: u64,
        target_slot: u64,
    },
    Live,
    Backoff {
        delay_ms: u64,
        attempt: u32,
    },
    Stopped {
        reason: String,
    },
}

impl From<&State> for StateDto {
    fn from(s: &State) -> Self {
        match s {
            State::Connecting { attempt } => Self::Connecting { attempt: *attempt },
            State::Replaying {
                incident,
                from_slot,
                target_slot,
            } => Self::Replaying {
                incident: *incident,
                from_slot: *from_slot,
                target_slot: *target_slot,
            },
            State::Live => Self::Live,
            State::Backoff { delay, attempt } => Self::Backoff {
                delay_ms: delay.as_millis() as u64,
                attempt: *attempt,
            },
            State::Stopped { reason } => Self::Stopped {
                reason: reason.clone(),
            },
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsDto {
    pub updates_per_sec: f64,
    pub tx_per_sec: f64,
    pub highest_complete: Option<u64>,
    pub tip: Option<u64>,
    pub lag_slots: Option<u64>,
    pub delivered: u64,
    pub duplicates: u64,
    pub reconnects: u64,
    pub incidents: u64,
    pub latency_ms: Option<f64>,
    pub dedup_entries: usize,
}

impl From<&Metrics> for MetricsDto {
    fn from(m: &Metrics) -> Self {
        Self {
            updates_per_sec: m.updates_per_sec,
            tx_per_sec: m.transactions_per_sec,
            highest_complete: m.highest_complete,
            tip: m.tip,
            lag_slots: m.lag_slots,
            delivered: m.delivered,
            duplicates: m.duplicates,
            reconnects: m.reconnects,
            incidents: m.incidents,
            latency_ms: m.latency_ms,
            dedup_entries: m.dedup_entries,
        }
    }
}

/// Our stream as Solami's account API reports it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SolamiConnDto {
    pub conn_id: String,
    pub region: Option<String>,
    pub bytes_streamed: u64,
    pub throughput_bps: u64,
    pub buffer_size: u64,
    pub buffer_pending: u64,
    pub is_paygo: bool,
    /// gRPC streams open on the account right now (ours, plus a handoff patch or other clients).
    pub live_streams: usize,
    pub sampled_at: u64,
    /// Offline, the fixture source emulates Solami's buffer; nothing here came from Solami.
    pub emulated: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reason {
    pub code: String,
    pub text: String,
}

impl From<&DisconnectReason> for Reason {
    fn from(r: &DisconnectReason) -> Self {
        Self {
            code: r.label().to_owned(),
            text: r.to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepDto {
    pub attempt: u32,
    pub from_slot: u64,
    pub target_slot: u64,
    pub started_at: u64,
    pub ended: Option<Reason>,
    pub ended_detail: Option<String>,
    pub transactions: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchDto {
    pub slots: SlotRange,
    pub recovered: u64,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationDto {
    /// `pending` (waiting for finalization), `running`, `done` or `failed`.
    pub status: &'static str,
    pub range: Option<SlotRange>,
    pub error: Option<String>,
    pub report: Option<Report>,
}

/// An outage, from the first disconnect through replay, handoff patch and verification.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IncidentDto {
    pub id: i64,
    pub session_incident: u64,
    pub status: &'static str,
    pub reason: Reason,
    pub detail: String,
    pub opened_at: u64,
    pub recovered_at: Option<u64>,
    pub duration_ms: Option<u64>,
    pub last_complete_slot: Option<u64>,
    pub resume_from: Option<u64>,
    pub gap: Option<SlotRange>,
    pub unrecoverable: Option<SlotRange>,
    pub steps: Vec<StepDto>,
    pub replayed: u64,
    pub duplicates: u64,
    /// What a consumer without dedup would have counted twice.
    pub naive_double_counts: u64,
    pub patch: Option<PatchDto>,
    pub verification: Option<VerificationDto>,
    /// How the outage was caused, when the console did it on purpose.
    pub chaos: Option<String>,
}

impl IncidentDto {
    pub fn apply(&mut self, incident: &Incident) {
        self.reason = Reason::from(&incident.reason);
        self.detail = incident.detail.clone();
        self.gap = incident.gap;
        self.unrecoverable = incident.unrecoverable;
        self.replayed = incident.replayed;
        self.duplicates = incident.duplicates;
        self.naive_double_counts = incident.duplicates;
        self.recovered_at = incident.recovered_at.map(unix_ms);
        self.duration_ms = incident.duration().map(|d| d.as_millis() as u64);
        self.steps = incident
            .steps
            .iter()
            .map(|s| StepDto {
                attempt: s.attempt,
                from_slot: s.from_slot,
                target_slot: s.target_slot,
                started_at: unix_ms(s.started_at),
                ended: s.ended.as_ref().map(|(r, _)| Reason::from(r)),
                ended_detail: s.ended.as_ref().map(|(_, d)| d.clone()),
                transactions: s.transactions,
            })
            .collect();
        if self.recovered_at.is_some() && self.status == "open" {
            self.status = "recovered";
        }
    }
}

/// One slot on the tape.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotCell {
    pub slot: u64,
    /// `live` or `replay`.
    pub origin: &'static str,
    pub complete: bool,
    pub txs: u32,
    /// `ok`, `missing` or `repaired`, once verified.
    pub verified: Option<&'static str>,
    pub missing: u32,
    pub incident: Option<u64>,
    pub at: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TxDto {
    pub sig: String,
    pub slot: u64,
    /// `live`, `replay`, `patch`, or `duplicate` for a re-sent transaction Gapless dropped.
    pub origin: &'static str,
    /// `buy`, `sell`, `create`, `complete` or `other`.
    pub kind: &'static str,
    pub sol: Option<f64>,
    pub mint: Option<String>,
    pub symbol: Option<String>,
    pub user: Option<String>,
    pub at: u64,
}

/// A line for the console's activity log.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogDto {
    pub at: u64,
    /// `info`, `warn`, `error` or `success`.
    pub level: &'static str,
    pub text: String,
    pub incident: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlsDto {
    pub handoff_patch: bool,
    pub throttle_ms: Option<u64>,
    pub hold_secs: Option<u64>,
    pub can_kill: bool,
}

/// Everything the console needs to draw its first frame.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub mode: &'static str,
    pub program: String,
    pub started_at: u64,
    pub state: StateDto,
    /// When the stream entered `state`, for countdowns and "replaying for" copy.
    pub state_since: u64,
    pub conn_id: Option<String>,
    pub metrics: MetricsDto,
    pub tip: Option<u64>,
    /// The finalized tip, which incident verification waits for.
    pub finalized: Option<u64>,
    pub solami: Option<SolamiConnDto>,
    pub verified_through: Option<u64>,
    pub controls: ControlsDto,
    pub indexer: crate::indexer::IndexerDto,
    pub open_incident: Option<IncidentDto>,
}

/// WebSocket messages, tagged by `type`.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsMessage<'a> {
    Hello {
        snapshot: &'a Snapshot,
        tape: &'a [SlotCell],
        incidents: &'a [IncidentDto],
        txs: &'a [TxDto],
        log: &'a [LogDto],
    },
    /// Sent every 100 ms when something changed.
    Batch {
        slots: &'a [SlotCell],
        txs: &'a [TxDto],
        #[serde(rename = "txCount")]
        tx_count: u32,
        log: &'a [LogDto],
    },
    /// Once a second.
    Tick {
        snapshot: &'a Snapshot,
    },
    Incident {
        incident: &'a IncidentDto,
    },
    Verified {
        range: SlotRange,
        slots: &'a [SlotCell],
    },
}
