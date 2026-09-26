//! The engine: consumes Gapless events and turns them into the console's state. That covers the
//! slot tape, the indexer, the transaction feed and the incident lifecycle (opened, replayed,
//! patched, verified). It also runs rolling verification over the live stream.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use futures::StreamExt;
use gapless::fixture::EMULATED_BUFFER;
use gapless::{
    Event, Events, LiveConnection, Origin, Signature, SlotRange, SlotStatus, State, Transaction,
};
use gapless_verify::{Report, Verdict};
use tokio::sync::mpsc;
use tokio::time::{MissedTickBehavior, interval, sleep};

use crate::app::{App, RECENT_LOG, RECENT_TXS, TAPE_SLOTS};
use crate::dto::{
    ControlsDto, IncidentDto, LogDto, MetricsDto, PatchDto, Reason, SlotCell, Snapshot,
    SolamiConnDto, StateDto, StepDto, TxDto, VerificationDto, WsMessage, now_ms, unix_ms,
};
use crate::indexer::{Activity, Decoder, Indexer};
use crate::ledger::Ledger;
use crate::truth::Truth;

/// Slots after a replay target that incident verification covers: where Solami's switch to
/// the live feed can drop transactions.
pub const HANDOFF_SLOTS: u64 = 64;
const ROLLING_MAX_SLOTS: u64 = 400;
const TX_SAMPLE: usize = 40;
/// Dropped duplicates shown in the feed per batch; the metrics count all of them.
const DUPLICATE_SAMPLE: usize = 12;
const INCIDENTS_SHOWN: usize = 20;
const FINALIZATION_TIMEOUT: Duration = Duration::from_secs(240);

enum Job {
    Tips {
        processed: Option<u64>,
        finalized: Option<u64>,
    },
    Connections(Vec<LiveConnection>),
    IncidentFinalized {
        incident: u64,
        range: SlotRange,
    },
    IncidentVerified {
        incident: u64,
        result: Result<Report, String>,
    },
    RollingVerified {
        range: SlotRange,
        result: Result<Report, String>,
    },
}

struct Tracked {
    dto: IncidentDto,
    /// Replay targets, one per step.
    targets: Vec<u64>,
    recovered: bool,
    verification_scheduled: bool,
    verified: bool,
}

pub struct Engine {
    app: Arc<App>,
    truth: Arc<dyn Truth>,
    jobs: mpsc::UnboundedSender<Job>,
    session: i64,
    started_at: u64,
    decoder: Decoder,
    indexer: Indexer,
    ledger: Ledger,
    /// Every signature delivered recently, with its slot, for verification.
    delivered: HashMap<Signature, u64>,
    symbols: HashMap<String, String>,
    state: StateDto,
    state_since: u64,
    conn_id: Option<String>,
    metrics: MetricsDto,
    tip: Option<u64>,
    finalized: Option<u64>,
    solami: Option<SolamiConnDto>,
    tracked: HashMap<u64, Tracked>,
    first_complete: Option<u64>,
    highest_complete: Option<u64>,
    verified_through: Option<u64>,
    /// Rolling verification stays below this while an incident is unverified.
    blocked_from: Option<u64>,
    rolling_running: bool,
    batch_slots: Vec<SlotCell>,
    batch_txs: Vec<TxDto>,
    batch_duplicates: usize,
    batch_tx_count: u32,
    batch_log: Vec<LogDto>,
}

pub async fn run(app: Arc<App>, truth: Arc<dyn Truth>, mut events: Events, session: i64) {
    let (jobs, mut jobs_rx) = mpsc::unbounded_channel();
    spawn_tip_poller(truth.clone(), jobs.clone());
    if let Some(account) = app.account.clone() {
        spawn_connection_poller(account, jobs.clone());
    }
    let mut engine = Engine::new(app, truth, jobs, session);
    engine.log(
        "info",
        format!(
            "following {} ({} mode)",
            engine.app.program, engine.app.mode
        ),
        None,
    );

    let mut flush = interval(Duration::from_millis(100));
    let mut tick = interval(Duration::from_secs(1));
    let mut rolling = interval(Duration::from_secs(10));
    for t in [&mut flush, &mut tick, &mut rolling] {
        t.set_missed_tick_behavior(MissedTickBehavior::Skip);
    }
    loop {
        tokio::select! {
            event = events.next() => match event {
                Some(event) => engine.on_event(event).await,
                None => {
                    engine.log("error", "the stream supervisor stopped".into(), None);
                    engine.flush();
                    engine.tick();
                    return;
                }
            },
            Some(job) = jobs_rx.recv() => engine.on_job(job).await,
            _ = flush.tick() => engine.flush(),
            _ = tick.tick() => engine.tick(),
            _ = rolling.tick() => engine.start_rolling(),
        }
    }
}

fn spawn_tip_poller(truth: Arc<dyn Truth>, jobs: mpsc::UnboundedSender<Job>) {
    tokio::spawn(async move {
        loop {
            let (processed, finalized) = tokio::join!(truth.tip(), truth.finalized_tip());
            let job = Job::Tips {
                processed: processed.ok(),
                finalized: finalized.ok(),
            };
            if jobs.send(job).is_err() {
                return;
            }
            sleep(Duration::from_secs(1)).await;
        }
    });
}

fn spawn_connection_poller(account: gapless::AccountApi, jobs: mpsc::UnboundedSender<Job>) {
    tokio::spawn(async move {
        loop {
            if let Ok(list) = account.live().await
                && jobs.send(Job::Connections(list)).is_err()
            {
                return;
            }
            sleep(Duration::from_secs(1)).await;
        }
    });
}

impl Engine {
    fn new(
        app: Arc<App>,
        truth: Arc<dyn Truth>,
        jobs: mpsc::UnboundedSender<Job>,
        session: i64,
    ) -> Self {
        let decoder = Decoder::new(&app.program);
        Self {
            app,
            truth,
            jobs,
            session,
            started_at: now_ms(),
            decoder,
            indexer: Indexer::default(),
            ledger: Ledger::default(),
            delivered: HashMap::new(),
            symbols: HashMap::new(),
            state: StateDto::Connecting { attempt: 1 },
            state_since: now_ms(),
            conn_id: None,
            metrics: MetricsDto::default(),
            tip: None,
            finalized: None,
            solami: None,
            tracked: HashMap::new(),
            first_complete: None,
            highest_complete: None,
            verified_through: None,
            blocked_from: None,
            rolling_running: false,
            batch_slots: Vec::new(),
            batch_txs: Vec::new(),
            batch_duplicates: 0,
            batch_tx_count: 0,
            batch_log: Vec::new(),
        }
    }

    fn log(&mut self, level: &'static str, text: String, incident: Option<i64>) {
        match level {
            "error" | "warn" => tracing::warn!("{text}"),
            _ => tracing::info!("{text}"),
        }
        self.batch_log.push(LogDto {
            at: now_ms(),
            level,
            text,
            incident,
        });
    }

    /// The incident a replayed update belongs to, as its store id.
    fn incident_id(&self, origin: Origin) -> Option<u64> {
        match origin {
            Origin::Live => None,
            Origin::Replay { incident } => self.tracked.get(&incident).map(|t| t.dto.id as u64),
        }
    }

    async fn on_event(&mut self, event: Event) {
        match event {
            Event::Transaction(tx) => self.on_transaction(*tx),
            Event::Duplicate {
                signature, slot, ..
            } => {
                if self.batch_duplicates < DUPLICATE_SAMPLE {
                    self.batch_duplicates += 1;
                    self.batch_txs.push(TxDto {
                        sig: signature.to_string(),
                        slot,
                        origin: "duplicate",
                        kind: "other",
                        sol: None,
                        mint: None,
                        symbol: None,
                        user: None,
                        at: now_ms(),
                    });
                }
            }
            Event::Slot {
                slot,
                status: SlotStatus::SlotProcessed,
                origin,
            } => {
                let incident = self.incident_id(origin);
                let cell = self.ledger.on_complete(slot, incident);
                self.batch_slots.push(cell);
                self.highest_complete = Some(self.highest_complete.map_or(slot, |h| h.max(slot)));
                self.first_complete.get_or_insert(slot);
            }
            Event::Slot { .. } => {}
            Event::State(state) => self.on_state(state),
            Event::ConnectionIdentified { conn_id } => {
                self.log(
                    "info",
                    format!("Solami identified our stream as {conn_id}"),
                    None,
                );
                self.conn_id = Some(conn_id);
            }
            Event::Disconnected(d) => self.on_disconnect(d).await,
            Event::ReasonResolved {
                incident,
                step,
                reason,
            } => {
                let Some(t) = self.tracked.get_mut(&incident) else {
                    return;
                };
                if step == 0 {
                    t.dto.reason = Reason::from(&reason);
                } else if let Some(s) = t.dto.steps.get_mut(step as usize - 1) {
                    s.ended = Some(Reason::from(&reason));
                }
                let id = t.dto.id;
                self.log(
                    "info",
                    format!("Solami's connection history: {reason}"),
                    Some(id),
                );
                self.publish(incident).await;
            }
            Event::Recovered(incident) => self.on_recovered(incident).await,
            Event::HandoffPatched {
                incident,
                slots,
                recovered,
                error,
            } => {
                let Some(t) = self.tracked.get_mut(&incident) else {
                    return;
                };
                t.dto.patch = Some(PatchDto {
                    slots,
                    recovered,
                    error: error.clone(),
                });
                let id = t.dto.id;
                let text = match &error {
                    None if recovered > 0 => format!(
                        "Handoff patch re-read slots {}–{} and recovered {} Solami dropped at the switch to live",
                        grouped(slots.first),
                        grouped(slots.last),
                        plural(recovered, "transaction")
                    ),
                    None => format!(
                        "Handoff patch re-read slots {}–{}: nothing was dropped",
                        grouped(slots.first),
                        grouped(slots.last)
                    ),
                    Some(e) => format!("Handoff patch failed: {e}"),
                };
                self.log(
                    if error.is_some() { "warn" } else { "success" },
                    text,
                    Some(id),
                );
                self.publish(incident).await;
                self.schedule_verification(incident).await;
            }
            Event::Metrics(m) => {
                self.metrics = MetricsDto::from(&m);
                if let Some(t) = m.tip {
                    self.tip = Some(self.tip.map_or(t, |p| p.max(t)));
                }
            }
        }
    }

    fn on_transaction(&mut self, tx: Transaction) {
        let incident = match tx.origin {
            Origin::Live => None,
            Origin::Replay { incident } => Some(incident),
        };
        let origin = match incident.and_then(|i| self.tracked.get(&i)) {
            None => "live",
            Some(t) if t.recovered => "patch",
            Some(_) => "replay",
        };
        self.delivered.insert(tx.signature, tx.slot);
        let activities = self.decoder.activities(&tx.info);
        // Offline, the playback's own clock stands in for block time.
        let playback = self.app.fixture.as_ref().map(|f| f.slot_time(tx.slot));
        let clock = playback
            .unwrap_or(tx.received_at)
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        self.indexer
            .record(&activities, clock, playback.is_some(), origin != "live");
        self.ledger
            .on_transaction(tx.slot, self.incident_id(tx.origin));
        self.batch_tx_count += 1;

        let mut dto = TxDto {
            sig: tx.signature.to_string(),
            slot: tx.slot,
            origin,
            kind: "other",
            sol: None,
            mint: None,
            symbol: None,
            user: None,
            at: now_ms(),
        };
        match activities.first() {
            Some(Activity::Trade {
                mint,
                lamports,
                is_buy,
                user,
                ..
            }) => {
                let mint = bs58::encode(mint).into_string();
                dto.kind = if *is_buy { "buy" } else { "sell" };
                dto.sol = Some(*lamports as f64 / 1e9);
                dto.symbol = self.symbols.get(&mint).cloned();
                dto.mint = Some(mint);
                dto.user = Some(bs58::encode(user).into_string());
            }
            Some(Activity::Create {
                mint, symbol, user, ..
            }) => {
                let mint = bs58::encode(mint).into_string();
                if self.symbols.len() > 5_000 {
                    self.symbols.clear();
                }
                self.symbols.insert(mint.clone(), symbol.clone());
                dto.kind = "create";
                dto.symbol = Some(symbol.clone());
                dto.mint = Some(mint);
                dto.user = Some(bs58::encode(user).into_string());
            }
            Some(Activity::Complete { mint, .. }) => {
                let mint = bs58::encode(mint).into_string();
                dto.kind = "complete";
                dto.symbol = self.symbols.get(&mint).cloned();
                dto.mint = Some(mint);
            }
            None => {}
        }
        if self.batch_txs.len() < TX_SAMPLE {
            self.batch_txs.push(dto);
        }
    }

    fn on_state(&mut self, state: State) {
        match &state {
            State::Replaying {
                incident,
                from_slot,
                target_slot,
            } => {
                if let Some(t) = self.tracked.get_mut(incident) {
                    t.targets.push(*target_slot);
                    t.dto.steps.push(StepDto {
                        attempt: t.dto.steps.len() as u32 + 1,
                        from_slot: *from_slot,
                        target_slot: *target_slot,
                        started_at: now_ms(),
                        ended: None,
                        ended_detail: None,
                        transactions: 0,
                    });
                    let id = t.dto.id;
                    self.log(
                        "info",
                        format!(
                            "Replaying from slot {} to the tip at {} ({})",
                            grouped(*from_slot),
                            grouped(*target_slot),
                            plural(target_slot.saturating_sub(*from_slot) + 1, "slot")
                        ),
                        Some(id),
                    );
                }
            }
            State::Backoff { delay, .. } if delay.as_secs() >= 5 => {
                self.log(
                    "info",
                    format!("Staying offline for {}s", delay.as_secs()),
                    None,
                );
            }
            State::Stopped { reason } => self.log("error", format!("Stopped: {reason}"), None),
            _ => {}
        }
        self.state = StateDto::from(&state);
        self.state_since = now_ms();
    }

    async fn on_disconnect(&mut self, d: gapless::Disconnect) {
        if let Some(t) = self.tracked.get_mut(&d.incident) {
            if let Some(step) = d
                .step
                .checked_sub(1)
                .and_then(|i| t.dto.steps.get_mut(i as usize))
            {
                step.ended = Some(Reason::from(&d.reason));
                step.ended_detail = Some(d.detail.clone());
            }
            let id = t.dto.id;
            self.log(
                "warn",
                format!("Replay step {} ended: {} ({})", d.step, d.reason, d.detail),
                Some(id),
            );
            self.publish(d.incident).await;
            return;
        }
        let (chaos, throttled) = {
            let mut chaos = self.app.chaos.lock().expect("chaos lock");
            (
                chaos.pending_label.take(),
                chaos.throttle_ms.take().is_some(),
            )
        };
        // A slow consumer can't replay faster than the chain moves, so recover at full speed.
        if throttled {
            self.app.control.throttle(None);
        }
        let opened_at = unix_ms(d.at);
        let id = match self
            .app
            .store
            .open_incident(self.session, d.incident, opened_at)
            .await
        {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!("storing an incident: {e}");
                -(d.incident as i64)
            }
        };
        let dto = IncidentDto {
            id,
            session_incident: d.incident,
            status: "open",
            reason: Reason::from(&d.reason),
            detail: d.detail.clone(),
            opened_at,
            recovered_at: None,
            duration_ms: None,
            last_complete_slot: d.last_complete_slot,
            resume_from: d.resume_from,
            gap: None,
            unrecoverable: None,
            steps: Vec::new(),
            replayed: 0,
            duplicates: 0,
            naive_double_counts: 0,
            patch: None,
            verification: None,
            chaos,
        };
        self.tracked.insert(
            d.incident,
            Tracked {
                dto,
                targets: Vec::new(),
                recovered: false,
                verification_scheduled: false,
                verified: false,
            },
        );
        let from = d
            .resume_from
            .or(self.highest_complete.map(|h| h + 1))
            .unwrap_or(0);
        self.blocked_from = Some(self.blocked_from.map_or(from, |b| b.min(from)));
        // Skip the detail when it only repeats the reason ("cut by the client (connection cut by the client)").
        let text = if d
            .detail
            .to_lowercase()
            .contains(&d.reason.to_string().to_lowercase())
        {
            format!("Disconnected: {}", d.detail)
        } else {
            format!("Disconnected: {} ({})", d.reason, d.detail)
        };
        self.log("error", text, Some(id));
        if throttled {
            self.log(
                "info",
                "Restored full consumer speed so the replay can catch up".into(),
                Some(id),
            );
        }
        self.publish(d.incident).await;
    }

    async fn on_recovered(&mut self, incident: gapless::Incident) {
        let Some(t) = self.tracked.get_mut(&incident.id) else {
            return;
        };
        t.dto.apply(&incident);
        t.recovered = true;
        t.targets = incident.steps.iter().map(|s| s.target_slot).collect();
        let id = t.dto.id;
        let gap = incident.gap.map_or("no slots missed".into(), |g| {
            format!("a gap of {}", plural(g.len(), "slot"))
        });
        self.log(
            "success",
            format!(
                "Recovered after {:.1}s: {gap}, {} replayed in {}, {} dropped",
                incident.duration().unwrap_or_default().as_secs_f64(),
                grouped(incident.replayed),
                plural(incident.steps.len() as u64, "step"),
                plural(incident.duplicates, "duplicate")
            ),
            Some(id),
        );
        let (patch_on, held) = {
            let mut chaos = self.app.chaos.lock().expect("chaos lock");
            (chaos.handoff_patch, chaos.hold_secs.take().is_some())
        };
        if held {
            self.app.control.hold(None);
        }
        self.publish(incident.id).await;
        if !patch_on || incident.steps.is_empty() {
            self.schedule_verification(incident.id).await;
        }
    }

    /// Wait for the incident's slots (the gap plus the handoff slots after the last replay
    /// target) to finalize, then verify them.
    async fn schedule_verification(&mut self, incident: u64) {
        let Some(t) = self.tracked.get_mut(&incident) else {
            return;
        };
        if t.verification_scheduled {
            return;
        }
        t.verification_scheduled = true;
        let Some(gap) = t.dto.gap else {
            t.dto.verification = Some(VerificationDto {
                status: "skipped",
                range: None,
                error: None,
                report: None,
            });
            t.verified = true;
            self.unblock();
            self.publish(incident).await;
            return;
        };
        let target = t.targets.last().copied().unwrap_or(gap.last);
        let range = SlotRange::new(gap.first, target + HANDOFF_SLOTS);
        t.dto.verification = Some(VerificationDto {
            status: "pending",
            range: Some(range),
            error: None,
            report: None,
        });
        self.publish(incident).await;

        let (truth, jobs) = (self.truth.clone(), self.jobs.clone());
        tokio::spawn(async move {
            let started = tokio::time::Instant::now();
            loop {
                if let Ok(finalized) = truth.finalized_tip().await
                    && finalized >= range.last
                {
                    let _ = jobs.send(Job::IncidentFinalized { incident, range });
                    return;
                }
                if started.elapsed() > FINALIZATION_TIMEOUT {
                    let error = format!("slot {} didn't finalize in time", range.last);
                    let _ = jobs.send(Job::IncidentVerified {
                        incident,
                        result: Err(error),
                    });
                    return;
                }
                sleep(Duration::from_secs(2)).await;
            }
        });
    }

    async fn on_job(&mut self, job: Job) {
        match job {
            Job::Tips {
                processed,
                finalized,
            } => {
                if let Some(p) = processed {
                    self.tip = Some(self.tip.map_or(p, |t| t.max(p)));
                }
                if finalized.is_some() {
                    self.finalized = finalized;
                }
            }
            Job::Connections(list) => {
                let count = list.len();
                self.solami = self.conn_id.as_ref().and_then(|id| {
                    list.into_iter()
                        .find(|c| &c.conn_id == id)
                        .map(|c| SolamiConnDto {
                            conn_id: c.conn_id,
                            region: c.region,
                            bytes_streamed: c.bytes_streamed,
                            throughput_bps: c.throughput_bps,
                            buffer_size: c.buffer_size,
                            buffer_pending: c.buffer_pending,
                            is_paygo: c.is_paygo,
                            live_streams: count,
                            sampled_at: now_ms(),
                            emulated: false,
                        })
                });
            }
            Job::IncidentFinalized { incident, range } => {
                let Some(t) = self.tracked.get_mut(&incident) else {
                    return;
                };
                t.dto.verification = Some(VerificationDto {
                    status: "running",
                    range: Some(range),
                    error: None,
                    report: None,
                });
                let windows: Vec<(u64, SlotRange)> = t
                    .targets
                    .iter()
                    .map(|&target| (incident, SlotRange::new(target, target + HANDOFF_SLOTS)))
                    .collect();
                self.publish(incident).await;
                let delivered = self.delivered.clone();
                let (truth, jobs) = (self.truth.clone(), self.jobs.clone());
                tokio::spawn(async move {
                    let result = async {
                        let mut report = truth.verify(range, &delivered, true).await?;
                        report.explain_handoffs(&windows);
                        if !report.missing.is_empty() {
                            truth.repair(&mut report).await?;
                        }
                        anyhow::Ok(report)
                    }
                    .await
                    .map_err(|e| e.to_string());
                    let _ = jobs.send(Job::IncidentVerified { incident, result });
                });
            }
            Job::IncidentVerified { incident, result } => {
                self.on_incident_verified(incident, result).await
            }
            Job::RollingVerified { range, result } => {
                self.rolling_running = false;
                match result {
                    Ok(report) => {
                        let cells = self.ledger.on_verified(&report);
                        self.app.broadcast(&WsMessage::Verified {
                            range,
                            slots: &cells,
                        });
                        self.verified_through = Some(range.last);
                        if !report.missing.is_empty() {
                            self.log(
                                "warn",
                                format!(
                                    "Rolling verification of slots {}–{}: {} missing{}",
                                    grouped(range.first),
                                    grouped(range.last),
                                    plural(report.missing.len() as u64, "transaction"),
                                    if report.repaired.is_empty() {
                                        ""
                                    } else {
                                        ", fetched from RPC"
                                    }
                                ),
                                None,
                            );
                        }
                    }
                    Err(e) => self.log("warn", format!("Rolling verification failed: {e}"), None),
                }
            }
        }
    }

    async fn on_incident_verified(&mut self, incident: u64, result: Result<Report, String>) {
        let cells = match &result {
            Ok(report) => self.ledger.on_verified(report),
            Err(_) => Vec::new(),
        };
        let Some(t) = self.tracked.get_mut(&incident) else {
            return;
        };
        let range = t.dto.verification.as_ref().and_then(|v| v.range);
        let id = t.dto.id;
        t.verified = true;
        let text = match &result {
            Ok(report) => {
                let summary = match &report.verdict {
                    Verdict::Complete => format!(
                        "complete, {} of {} expected transactions delivered",
                        grouped(report.matched),
                        grouped(report.expected)
                    ),
                    Verdict::Repaired { repaired } => {
                        format!("{repaired} missing, all fetched from RPC")
                    }
                    Verdict::Incomplete { missing } => {
                        format!("incomplete, {} missing", grouped(*missing))
                    }
                    Verdict::Inconclusive { reason } => format!("inconclusive ({reason})"),
                };
                t.dto.status = "verified";
                t.dto.verification = Some(VerificationDto {
                    status: "done",
                    range,
                    error: None,
                    report: Some(report.clone()),
                });
                format!(
                    "Verified slots {}: {summary}",
                    range.map_or(String::new(), |r| format!(
                        "{}–{}",
                        grouped(r.first),
                        grouped(r.last)
                    ))
                )
            }
            Err(e) => {
                t.dto.verification = Some(VerificationDto {
                    status: "failed",
                    range,
                    error: Some(e.clone()),
                    report: None,
                });
                format!("Verification failed: {e}")
            }
        };
        let complete = matches!(&result, Ok(r) if r.verdict == Verdict::Complete);
        self.log(if complete { "success" } else { "warn" }, text, Some(id));
        if let Some(range) = range {
            self.app.broadcast(&WsMessage::Verified {
                range,
                slots: &cells,
            });
        }
        self.unblock();
        self.publish(incident).await;
    }

    /// Let rolling verification continue past incidents that are verified.
    fn unblock(&mut self) {
        self.blocked_from = self
            .tracked
            .values()
            .filter(|t| !t.verified)
            .filter_map(|t| t.dto.resume_from.or(t.dto.gap.map(|g| g.first)))
            .min();
    }

    fn start_rolling(&mut self) {
        if self.rolling_running {
            return;
        }
        let (Some(first), Some(finalized), Some(high)) =
            (self.first_complete, self.finalized, self.highest_complete)
        else {
            return;
        };
        let from = self.verified_through.map_or(first + 2, |v| v + 1);
        let mut to = finalized.min(high).min(from + ROLLING_MAX_SLOTS - 1);
        if let Some(blocked) = self.blocked_from {
            to = to.min(blocked.saturating_sub(1));
        }
        if to < from {
            return;
        }
        self.rolling_running = true;
        let range = SlotRange::new(from, to);
        let delivered = self.delivered.clone();
        let (truth, jobs) = (self.truth.clone(), self.jobs.clone());
        tokio::spawn(async move {
            let result = async {
                let mut report = truth.verify(range, &delivered, false).await?;
                if !report.missing.is_empty() {
                    truth.repair(&mut report).await?;
                }
                anyhow::Ok(report)
            }
            .await
            .map_err(|e| e.to_string());
            let _ = jobs.send(Job::RollingVerified { range, result });
        });
    }

    /// Save an incident, push it to consoles and keep the recent list current.
    async fn publish(&mut self, incident: u64) {
        let Some(t) = self.tracked.get(&incident) else {
            return;
        };
        let dto = t.dto.clone();
        if dto.id > 0
            && let Err(e) = self.app.store.save(&dto).await
        {
            tracing::warn!("saving incident {}: {e}", dto.id);
        }
        self.app.broadcast(&WsMessage::Incident { incident: &dto });
        let mut shared = self.app.shared.write().expect("shared lock");
        shared.incidents.retain(|i| i.id != dto.id);
        shared.incidents.insert(0, dto);
        shared.incidents.truncate(INCIDENTS_SHOWN);
    }

    fn flush(&mut self) {
        if self.batch_slots.is_empty()
            && self.batch_txs.is_empty()
            && self.batch_log.is_empty()
            && self.batch_tx_count == 0
        {
            return;
        }
        self.app.broadcast(&WsMessage::Batch {
            slots: &self.batch_slots,
            txs: &self.batch_txs,
            tx_count: self.batch_tx_count,
            log: &self.batch_log,
        });
        let mut shared = self.app.shared.write().expect("shared lock");
        extend_capped(&mut shared.txs, self.batch_txs.drain(..), RECENT_TXS);
        extend_capped(&mut shared.log, self.batch_log.drain(..), RECENT_LOG);
        self.batch_slots.clear();
        self.batch_duplicates = 0;
        self.batch_tx_count = 0;
    }

    fn tick(&mut self) {
        // Forget delivered signatures well behind anything still to be verified.
        if let Some(keep) = self
            .blocked_from
            .or(self.verified_through)
            .map(|s| s.saturating_sub(4_000))
        {
            self.delivered.retain(|_, slot| *slot >= keep);
        }
        if let Some(fixture) = &self.app.fixture {
            self.solami = Some(SolamiConnDto {
                conn_id: "fixture".into(),
                region: None,
                bytes_streamed: 0,
                throughput_bps: 0,
                buffer_size: EMULATED_BUFFER,
                buffer_pending: fixture.buffer_pending(),
                is_paygo: false,
                live_streams: 1,
                sampled_at: now_ms(),
                emulated: true,
            });
        }
        let snapshot = self.snapshot();
        self.app.broadcast(&WsMessage::Tick {
            snapshot: &snapshot,
        });
        let tape = self.ledger.recent(TAPE_SLOTS);
        let mut shared = self.app.shared.write().expect("shared lock");
        shared.snapshot = snapshot;
        shared.tape = tape;
    }

    fn snapshot(&self) -> Snapshot {
        let chaos = self.app.chaos.lock().expect("chaos lock");
        let open_incident = self
            .tracked
            .values()
            .filter(|t| !t.verified)
            .max_by_key(|t| t.dto.opened_at)
            .map(|t| t.dto.clone());
        Snapshot {
            mode: self.app.mode,
            program: self.app.program.clone(),
            started_at: self.started_at,
            state: self.state.clone(),
            state_since: self.state_since,
            conn_id: self.conn_id.clone(),
            metrics: self.metrics.clone(),
            tip: self.tip,
            finalized: self.finalized,
            solami: self.solami.clone(),
            verified_through: self.verified_through,
            controls: ControlsDto {
                handoff_patch: chaos.handoff_patch,
                throttle_ms: chaos.throttle_ms,
                hold_secs: chaos.hold_secs,
                can_kill: self.app.account.is_some() && self.conn_id.is_some(),
            },
            indexer: self.indexer.summary(),
            open_incident,
        }
    }
}

fn extend_capped<T>(ring: &mut VecDeque<T>, items: impl Iterator<Item = T>, cap: usize) {
    ring.extend(items);
    while ring.len() > cap {
        ring.pop_front();
    }
}

/// 450510210 → "450,510,210", for log lines people read.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "1 step", "3 steps".
fn plural(n: u64, word: &str) -> String {
    format!("{} {word}{}", grouped(n), if n == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_and_pluralises() {
        assert_eq!(grouped(450_510_210), "450,510,210");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000), "1,000");
        assert_eq!(plural(1, "step"), "1 step");
        assert_eq!(plural(4_436, "transaction"), "4,436 transactions");
    }
}
