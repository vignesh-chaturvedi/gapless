use std::collections::{HashMap, HashSet};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::{Stream, StreamExt};
use solami::geyser::subscribe_update::UpdateOneof;
use solami::geyser::{
    SlotStatus, SubscribeRequest, SubscribeUpdate, SubscribeUpdateTransactionInfo,
};
use solami::{SubscribeRequestFilterSlots, SubscriptionBuilder, TxFilter};
use tokio::sync::mpsc;
use tokio::time::{Instant, MissedTickBehavior, interval_at, sleep, sleep_until};
use tokio_stream::wrappers::ReceiverStream;

use crate::Error;
use crate::account::{AccountApi, ClosedConnection};
use crate::backoff::Backoff;
use crate::classify::DisconnectReason;
use crate::config::{Builder, Config};
use crate::cursor::SlotCursor;
use crate::dedup::DedupWindow;
use crate::event::{
    Disconnect, Event, Incident, Metrics, Origin, ReplayStep, Signature, SlotRange, State,
    Transaction,
};
use crate::source::{SolamiSource, Source, UpdateStream};

/// Slots the dedup window keeps beyond the replay horizon.
const DEDUP_MARGIN: u64 = 512;

/// A configured subscription, ready to [`start`](Gapless::start).
pub struct Gapless {
    config: Config,
    source: Arc<dyn Source>,
    account: Option<AccountApi>,
}

impl Gapless {
    pub fn builder(api_key: impl Into<String>) -> Builder {
        Builder::new(api_key)
    }

    pub fn new(config: Config) -> Result<Self, Error> {
        config.validate()?;
        let source = SolamiSource::new(&config.grpc_url, &config.api_key, config.compression)?;
        let account = config
            .account_api
            .as_ref()
            .map(|base| AccountApi::new(base.clone(), config.api_key.clone()));
        Ok(Self {
            config,
            source: Arc::new(source),
            account,
        })
    }

    /// Run against another source, such as a test double or recorded fixtures. The account API
    /// stays off unless added with [`Gapless::with_account`].
    pub fn with_source(config: Config, source: impl Source) -> Self {
        Self {
            config,
            source: Arc::new(source),
            account: None,
        }
    }

    pub fn with_account(mut self, account: Option<AccountApi>) -> Self {
        self.account = account;
        self
    }

    /// Spawn the supervisor on the current tokio runtime.
    pub fn start(self) -> (Events, Control) {
        let (events_tx, events_rx) = mpsc::channel(self.config.event_buffer);
        let (control_tx, control_rx) = mpsc::unbounded_channel();
        tokio::spawn(Supervisor::new(self, events_tx, control_rx).run());
        (
            Events {
                inner: ReceiverStream::new(events_rx),
            },
            Control { tx: control_tx },
        )
    }
}

/// Everything the supervisor reports. Ends after [`State::Stopped`], or if the supervisor
/// can't continue.
pub struct Events {
    inner: ReceiverStream<Event>,
}

impl Stream for Events {
    type Item = Event;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Event>> {
        Pin::new(&mut self.inner).poll_next(cx)
    }
}

enum Command {
    Cut,
    Throttle(Option<Duration>),
    Hold(Option<Duration>),
    HandoffPatch(bool),
    Stop,
}

/// Steers a running supervisor. Dropping every handle leaves it running.
#[derive(Clone)]
pub struct Control {
    tx: mpsc::UnboundedSender<Command>,
}

impl Control {
    /// Drop the current connection from our side; Gapless recovers as from any other disconnect.
    pub fn cut(&self) {
        let _ = self.tx.send(Command::Cut);
    }

    /// Sleep this long after every update, making the consumer slow on purpose. `None` restores
    /// full speed.
    pub fn throttle(&self, per_update: Option<Duration>) {
        let _ = self.tx.send(Command::Throttle(per_update));
    }

    /// Stay disconnected at least this long after a drop that starts an outage, as if the
    /// consumer itself had been down. Replay steps still retry straight away. `None` goes back to
    /// reconnecting as soon as backoff allows.
    pub fn hold(&self, offline_for: Option<Duration>) {
        let _ = self.tx.send(Command::Hold(offline_for));
    }

    /// Turn the replay-to-live handoff patch on or off for recoveries from now on.
    pub fn handoff_patch(&self, on: bool) {
        let _ = self.tx.send(Command::HandoffPatch(on));
    }

    /// Stop. The event stream ends with [`State::Stopped`].
    pub fn stop(&self) {
        let _ = self.tx.send(Command::Stop);
    }
}

enum Ended {
    Disconnected(DisconnectReason, String),
    Stop,
    ConsumerGone,
}

/// Set while a reconnect is replaying history up to `target`.
struct Replay {
    incident: u64,
    target: u64,
}

#[derive(Default)]
struct Stats {
    delivered: u64,
    duplicates: u64,
    reconnects: u64,
    incidents: u64,
    window_updates: u64,
    window_txs: u64,
    latency_ms: Option<f64>,
}

/// Which connection is current, and Solami's id for it once known.
#[derive(Default)]
struct Conn {
    generation: u64,
    id: Option<String>,
    /// Unix seconds when we subscribed.
    connected_at: u64,
    /// Live connections on the account just before we subscribed.
    before: HashSet<String>,
}

/// Enough to find a closed connection's row in Solami's history.
struct HistoryLookup {
    account: AccountApi,
    conn_id: Option<String>,
    connected_at: u64,
    before: HashSet<String>,
}

/// A handoff patch waiting for the stream to move far enough past the replay target.
struct PendingPatch {
    incident: u64,
    slots: SlotRange,
    start_after: u64,
}

enum PatchMsg {
    Tx {
        incident: u64,
        slot: u64,
        info: Box<SubscribeUpdateTransactionInfo>,
    },
    Done {
        incident: u64,
        slots: SlotRange,
        error: Option<String>,
    },
}

/// A disconnect reason learned after the fact.
struct Resolution {
    incident: u64,
    step: u32,
    reason: DisconnectReason,
}

/// Our row in the connection history: by id when we know it, otherwise the new connection that
/// started closest to when we subscribed. A replay step can overflow Solami's buffer within ~2 s
/// and leave the live list before we identify it, because we keep draining for a while after.
fn match_history<'a>(
    rows: &'a [ClosedConnection],
    conn_id: Option<&str>,
    connected_at: u64,
    before: &HashSet<String>,
) -> Option<&'a ClosedConnection> {
    match conn_id {
        Some(id) => rows.iter().find(|row| row.conn_id == id),
        None => rows
            .iter()
            .filter(|row| !before.contains(&row.conn_id))
            .filter(|row| row.started_at + 2 >= connected_at && row.started_at <= connected_at + 15)
            .min_by_key(|row| row.started_at.abs_diff(connected_at)),
    }
}

struct Supervisor {
    config: Config,
    source: Arc<dyn Source>,
    account: Option<AccountApi>,
    events: mpsc::Sender<Event>,
    control: mpsc::UnboundedReceiver<Command>,
    control_open: bool,
    cursor: SlotCursor,
    dedup: DedupWindow,
    backoff: Backoff,
    throttle: Option<Duration>,
    hold: Option<Duration>,
    incident: Option<Incident>,
    incidents_opened: u64,
    replay: Option<Replay>,
    tip: Arc<AtomicU64>,
    conn: Arc<Mutex<Conn>>,
    resolved_tx: mpsc::UnboundedSender<Resolution>,
    resolved_rx: mpsc::UnboundedReceiver<Resolution>,
    pending_patch: Option<PendingPatch>,
    patch_tx: mpsc::UnboundedSender<PatchMsg>,
    patch_rx: mpsc::UnboundedReceiver<PatchMsg>,
    patched: HashMap<u64, u64>,
    stats: Stats,
    window_start: Instant,
}

impl Supervisor {
    fn new(
        gapless: Gapless,
        events: mpsc::Sender<Event>,
        control: mpsc::UnboundedReceiver<Command>,
    ) -> Self {
        let Gapless {
            config,
            source,
            account,
        } = gapless;
        let (resolved_tx, resolved_rx) = mpsc::unbounded_channel();
        let (patch_tx, patch_rx) = mpsc::unbounded_channel();
        Self {
            cursor: SlotCursor::new(config.commitment),
            dedup: DedupWindow::new(config.replay_horizon + DEDUP_MARGIN),
            backoff: Backoff::new(config.backoff.clone()),
            config,
            source,
            account,
            events,
            control,
            control_open: true,
            throttle: None,
            hold: None,
            incident: None,
            incidents_opened: 0,
            replay: None,
            tip: Arc::new(AtomicU64::new(0)),
            conn: Arc::new(Mutex::new(Conn::default())),
            resolved_tx,
            resolved_rx,
            pending_patch: None,
            patch_tx,
            patch_rx,
            patched: HashMap::new(),
            stats: Stats::default(),
            window_start: Instant::now(),
        }
    }

    async fn run(mut self) {
        self.spawn_tip_poller();
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            if !self.emit(Event::State(State::Connecting { attempt })).await {
                return;
            }
            let generation = self.next_generation();
            let snapshot = live_connection_ids(self.account.clone());
            let (before, from_slot) = tokio::join!(snapshot, self.prepare_resume());
            let Ok(from_slot) = from_slot else { return };
            {
                let mut conn = self.conn.lock().expect("conn lock");
                conn.connected_at = unix_now();
                conn.before = before.clone().unwrap_or_default();
            }

            let ended = match self.source.subscribe(self.request(from_slot)).await {
                Ok(stream) => {
                    self.spawn_identify(generation, before);
                    self.consume(stream).await
                }
                Err(status) => Ended::Disconnected(
                    DisconnectReason::from_status(&status),
                    status.message().to_owned(),
                ),
            };
            let (reason, detail) = match ended {
                Ended::ConsumerGone => return,
                Ended::Stop => return self.stop("stopped by the application").await,
                Ended::Disconnected(reason, detail) => (reason, detail),
            };
            let lookup = self.history_lookup();
            self.stats.reconnects += 1;
            let Some((incident, step)) = self.on_disconnect(reason.clone(), detail.clone()).await
            else {
                return;
            };
            if reason == DisconnectReason::ServerClosed
                && let Some(lookup) = lookup
            {
                self.spawn_resolve(lookup, incident, step);
            }
            if reason.is_fatal() {
                return self.stop(&format!("{reason}: {detail}")).await;
            }
            let backoff = self.backoff.next_delay(&reason, Instant::now());
            // A hold simulates the consumer being down, so it applies to the drop that starts an
            // outage, not to replay steps: those should retry straight away or the gap only grows.
            let opening = self.incident.as_ref().is_some_and(|i| i.steps.is_empty());
            let delay = match self.hold {
                Some(hold) if opening => hold.max(backoff),
                _ => backoff,
            };
            let backoff = State::Backoff {
                delay,
                attempt: self.backoff.attempt(),
            };
            if !self.emit(Event::State(backoff)).await {
                return;
            }
            if !self.pause(delay).await {
                return self.stop("stopped by the application").await;
            }
        }
    }

    /// Choose `from_slot` for the next connect. While recovering, that's the cursor's resume
    /// point, clamped to what Solami can still replay. `Err` means the consumer went away.
    async fn prepare_resume(&mut self) -> Result<Option<u64>, ()> {
        if self.incident.is_none() {
            return Ok(None);
        }
        let Some(resume) = self.cursor.resume_from() else {
            return Ok(None);
        };
        let (tip, first_available) = tokio::join!(self.source.tip(), self.source.first_available());
        let tip = tip.ok();
        if let Some(t) = tip {
            self.tip.fetch_max(t, Ordering::Relaxed);
        }

        let incident = self.incident.as_mut().expect("checked above");
        let mut from = resume;
        if let Ok(Some(first)) = first_available
            && from < first
        {
            let lost = SlotRange {
                first: from,
                last: first - 1,
            };
            incident.unrecoverable = Some(match incident.unrecoverable {
                Some(prev) => SlotRange {
                    first: prev.first.min(lost.first),
                    last: prev.last.max(lost.last),
                },
                None => lost,
            });
            from = first;
        }
        let target = tip.unwrap_or(from).max(from);
        if incident.gap.is_none() {
            incident.gap = tip.filter(|t| *t >= resume).map(|t| SlotRange {
                first: resume,
                last: t,
            });
        }
        incident.steps.push(ReplayStep {
            attempt: incident.steps.len() as u32 + 1,
            from_slot: from,
            target_slot: target,
            started_at: SystemTime::now(),
            ended: None,
            transactions: 0,
        });
        let id = incident.id;
        self.replay = Some(Replay {
            incident: id,
            target,
        });
        let state = State::Replaying {
            incident: id,
            from_slot: from,
            target_slot: target,
        };
        if !self.emit(Event::State(state)).await {
            return Err(());
        }
        Ok(Some(from))
    }

    fn request(&self, from_slot: Option<u64>) -> SubscribeRequest {
        let c = &self.config;
        let filter = TxFilter {
            vote: Some(false),
            failed: if c.include_failed { None } else { Some(false) },
            account_include: c.account_include.clone(),
            account_exclude: c.account_exclude.clone(),
            account_required: c.account_required.clone(),
            signature: None,
        };
        let slots = SubscribeRequestFilterSlots {
            filter_by_commitment: Some(false),
            interslot_updates: Some(false),
        };
        let mut builder = SubscriptionBuilder::new()
            .commitment(c.commitment)
            .transactions("gapless", filter)
            .slots("gapless-slots", slots);
        if let Some(slot) = from_slot {
            builder = builder.from_slot(slot);
        }
        builder.build()
    }

    async fn consume(&mut self, mut stream: UpdateStream) -> Ended {
        let second = Duration::from_secs(1);
        let mut metrics = interval_at(Instant::now() + second, second);
        metrics.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut last_update = Instant::now();
        let mut first = true;
        loop {
            let stall_at = last_update + self.config.stall_timeout;
            tokio::select! {
                command = self.control.recv(), if self.control_open => match command {
                    Some(Command::Cut) => {
                        return Ended::Disconnected(DisconnectReason::Cut, "connection cut by the client".into());
                    }
                    Some(Command::Throttle(per_update)) => self.throttle = per_update,
                    Some(Command::Hold(offline_for)) => self.hold = offline_for,
                    Some(Command::HandoffPatch(on)) => self.config.handoff_patch = on,
                    Some(Command::Stop) => return Ended::Stop,
                    None => self.control_open = false,
                },
                Some(resolution) = self.resolved_rx.recv() => {
                    if !self.apply_resolution(resolution).await {
                        return Ended::ConsumerGone;
                    }
                }
                Some(msg) = self.patch_rx.recv() => {
                    if !self.on_patch(msg).await {
                        return Ended::ConsumerGone;
                    }
                }
                _ = metrics.tick() => {
                    if !self.emit_metrics().await {
                        return Ended::ConsumerGone;
                    }
                }
                _ = sleep_until(stall_at) => {
                    let detail = format!("no updates for {}s", self.config.stall_timeout.as_secs());
                    return Ended::Disconnected(DisconnectReason::Stalled, detail);
                }
                item = stream.next() => {
                    let update = match item {
                        None => return Ended::Disconnected(DisconnectReason::ServerClosed, "stream ended without a status".into()),
                        Some(Err(status)) => {
                            return Ended::Disconnected(DisconnectReason::from_status(&status), status.message().to_owned());
                        }
                        Some(Ok(update)) => update,
                    };
                    last_update = Instant::now();
                    if first {
                        first = false;
                        if self.replay.is_none() && !self.finish_recovery().await {
                            return Ended::ConsumerGone;
                        }
                    }
                    if !self.handle(update).await {
                        return Ended::ConsumerGone;
                    }
                    if let Some(per_update) = self.throttle {
                        sleep(per_update).await;
                    }
                }
            }
        }
    }

    async fn handle(&mut self, update: SubscribeUpdate) -> bool {
        self.stats.window_updates += 1;
        let created_at = update
            .created_at
            .as_ref()
            .and_then(|t| to_system_time(t.seconds, t.nanos));
        match update.update_oneof {
            Some(UpdateOneof::Transaction(tx)) => {
                self.on_transaction(tx.slot, tx.transaction, created_at)
                    .await
            }
            Some(UpdateOneof::Slot(s)) => self.on_slot(s.slot, s.status()).await,
            _ => true,
        }
    }

    fn origin(&self, slot: u64) -> Origin {
        match &self.replay {
            Some(r) if slot <= r.target => Origin::Replay {
                incident: r.incident,
            },
            _ => Origin::Live,
        }
    }

    async fn on_transaction(
        &mut self,
        slot: u64,
        info: Option<SubscribeUpdateTransactionInfo>,
        created_at: Option<SystemTime>,
    ) -> bool {
        let Some(info) = info else { return true };
        let Some(signature) = Signature::from_bytes(&info.signature) else {
            return true;
        };
        self.cursor.on_transaction(slot);
        let origin = self.origin(slot);
        if !self.dedup.insert(signature, slot) {
            self.stats.duplicates += 1;
            if let Some(incident) = self.incident.as_mut() {
                incident.duplicates += 1;
            }
            return true;
        }
        self.stats.delivered += 1;
        self.stats.window_txs += 1;
        let received_at = SystemTime::now();
        match origin {
            Origin::Replay { .. } => {
                if let Some(incident) = self.incident.as_mut() {
                    incident.replayed += 1;
                    if let Some(step) = incident.steps.last_mut() {
                        step.transactions += 1;
                    }
                }
            }
            Origin::Live => {
                if let Some(ms) = created_at
                    .and_then(|c| received_at.duration_since(c).ok())
                    .map(|d| d.as_secs_f64() * 1e3)
                {
                    self.stats.latency_ms = Some(
                        self.stats
                            .latency_ms
                            .map_or(ms, |prev| prev * 0.9 + ms * 0.1),
                    );
                }
            }
        }
        let tx = Transaction {
            slot,
            signature,
            index: info.index,
            origin,
            received_at,
            created_at,
            info,
        };
        self.emit(Event::Transaction(Box::new(tx))).await
    }

    async fn on_slot(&mut self, slot: u64, status: SlotStatus) -> bool {
        self.cursor.on_slot(slot, status);
        let origin = self.origin(slot);
        if origin == Origin::Live {
            self.tip.fetch_max(slot, Ordering::Relaxed);
        }
        if !self
            .emit(Event::Slot {
                slot,
                status,
                origin,
            })
            .await
        {
            return false;
        }
        let caught_up = self.replay.as_ref().is_some_and(|r| {
            self.cursor
                .highest_complete()
                .is_some_and(|h| h >= r.target)
        });
        if caught_up {
            return self.finish_recovery().await;
        }
        let due = self.pending_patch.as_ref().is_some_and(|p| {
            self.cursor
                .highest_complete()
                .is_some_and(|h| h >= p.start_after)
        });
        if due && let Some(patch) = self.pending_patch.take() {
            self.spawn_patch(patch);
        }
        true
    }

    /// Re-read the slots around a replay-to-live handoff on a second, short subscription and
    /// forward their transactions; dedup lets only the ones the resumed stream dropped through.
    fn spawn_patch(&self, patch: PendingPatch) {
        let source = self.source.clone();
        let request = self.request(Some(patch.slots.first));
        let complete_on = self.cursor.completes_on();
        let tx = self.patch_tx.clone();
        let PendingPatch {
            incident, slots, ..
        } = patch;
        tokio::spawn(async move {
            let run = async {
                let mut stream = source.subscribe(request).await?;
                while let Some(item) = stream.next().await {
                    match item?.update_oneof {
                        Some(UpdateOneof::Transaction(t)) if slots.contains(t.slot) => {
                            let Some(info) = t.transaction else { continue };
                            let msg = PatchMsg::Tx {
                                incident,
                                slot: t.slot,
                                info: Box::new(info),
                            };
                            if tx.send(msg).is_err() {
                                return Ok(());
                            }
                        }
                        Some(UpdateOneof::Slot(s))
                            if s.status() == complete_on && s.slot >= slots.last =>
                        {
                            return Ok(());
                        }
                        _ => {}
                    }
                }
                Err(tonic::Status::unavailable("patch stream ended early"))
            };
            let error = match tokio::time::timeout(Duration::from_secs(30), run).await {
                Ok(Ok(())) => None,
                Ok(Err(status)) => Some(status.message().to_owned()),
                Err(_) => Some("timed out".to_owned()),
            };
            let _ = tx.send(PatchMsg::Done {
                incident,
                slots,
                error,
            });
        });
    }

    async fn on_patch(&mut self, msg: PatchMsg) -> bool {
        match msg {
            PatchMsg::Tx {
                incident,
                slot,
                info,
            } => {
                let info = *info;
                let Some(signature) = Signature::from_bytes(&info.signature) else {
                    return true;
                };
                if !self.dedup.insert(signature, slot) {
                    return true;
                }
                self.stats.delivered += 1;
                *self.patched.entry(incident).or_default() += 1;
                let tx = Transaction {
                    slot,
                    signature,
                    index: info.index,
                    origin: Origin::Replay { incident },
                    received_at: SystemTime::now(),
                    created_at: None,
                    info,
                };
                self.emit(Event::Transaction(Box::new(tx))).await
            }
            PatchMsg::Done {
                incident,
                slots,
                error,
            } => {
                let recovered = self.patched.remove(&incident).unwrap_or(0);
                self.emit(Event::HandoffPatched {
                    incident,
                    slots,
                    recovered,
                    error,
                })
                .await
            }
        }
    }

    /// Close the open incident (if any) and report that we're live.
    async fn finish_recovery(&mut self) -> bool {
        if let Some(replay) = self.replay.take()
            && self.config.handoff_patch
        {
            let c = &self.config;
            self.pending_patch = Some(PendingPatch {
                incident: replay.incident,
                slots: SlotRange::new(replay.target, replay.target + c.handoff_patch_slots),
                start_after: replay.target + c.handoff_patch_after,
            });
        }
        self.backoff.reset();
        if let Some(mut incident) = self.incident.take() {
            incident.recovered_at = Some(SystemTime::now());
            if !self.emit(Event::Recovered(incident)).await {
                return false;
            }
        }
        self.emit(Event::State(State::Live)).await
    }

    /// Record a drop, opening an incident if none is open. Returns the incident and step, or
    /// `None` if the consumer went away.
    async fn on_disconnect(
        &mut self,
        reason: DisconnectReason,
        detail: String,
    ) -> Option<(u64, u32)> {
        let at = SystemTime::now();
        let last_complete_slot = self.cursor.highest_complete();
        let resume_from = self.cursor.resume_from();
        self.replay = None;
        self.conn.lock().expect("conn lock").id = None;
        let (incident, step) = match self.incident.as_mut() {
            Some(incident) => {
                if let Some(step) = incident.steps.last_mut()
                    && step.ended.is_none()
                {
                    step.ended = Some((reason.clone(), detail.clone()));
                }
                (incident.id, incident.steps.len() as u32)
            }
            None => {
                self.incidents_opened += 1;
                self.stats.incidents += 1;
                let id = self.incidents_opened;
                self.incident = Some(Incident {
                    id,
                    reason: reason.clone(),
                    detail: detail.clone(),
                    started_at: at,
                    last_complete_slot,
                    gap: None,
                    unrecoverable: None,
                    steps: Vec::new(),
                    replayed: 0,
                    duplicates: 0,
                    recovered_at: None,
                });
                (id, 0)
            }
        };
        let disconnect = Disconnect {
            incident,
            step,
            reason,
            detail,
            at,
            last_complete_slot,
            resume_from,
        };
        self.emit(Event::Disconnected(disconnect))
            .await
            .then_some((incident, step))
    }

    /// What to look for in Solami's connection history for the connection that just ended.
    fn history_lookup(&self) -> Option<HistoryLookup> {
        let account = self.account.clone()?;
        let conn = self.conn.lock().expect("conn lock");
        Some(HistoryLookup {
            account,
            conn_id: conn.id.clone(),
            connected_at: conn.connected_at,
            before: conn.before.clone(),
        })
    }

    /// A clean end-of-stream can hide backpressure. Solami writes the connection's history row
    /// ~10 s after it ends, so look it up in the background instead of delaying the reconnect.
    fn spawn_resolve(&self, lookup: HistoryLookup, incident: u64, step: u32) {
        let resolved = self.resolved_tx.clone();
        tokio::spawn(async move {
            for _ in 0..20 {
                sleep(Duration::from_secs(1)).await;
                if resolved.is_closed() {
                    return;
                }
                let Ok(rows) = lookup.account.history().await else {
                    continue;
                };
                let reason = match_history(
                    &rows,
                    lookup.conn_id.as_deref(),
                    lookup.connected_at,
                    &lookup.before,
                )
                .and_then(|row| row.termination_reason.as_deref())
                .map(DisconnectReason::from_history);
                if let Some(reason) = reason {
                    if reason != DisconnectReason::Other("client_disconnect".into()) {
                        let _ = resolved.send(Resolution {
                            incident,
                            step,
                            reason,
                        });
                    }
                    return;
                }
            }
        });
    }

    /// Apply a reason learned from the history to the open incident, and tell the consumer.
    async fn apply_resolution(&mut self, r: Resolution) -> bool {
        if let Some(incident) = self.incident.as_mut()
            && incident.id == r.incident
        {
            if r.step == 0 {
                incident.reason = r.reason.clone();
            } else if let Some((reason, _)) = incident
                .steps
                .get_mut(r.step as usize - 1)
                .and_then(|step| step.ended.as_mut())
            {
                *reason = r.reason.clone();
            }
        }
        self.emit(Event::ReasonResolved {
            incident: r.incident,
            step: r.step,
            reason: r.reason,
        })
        .await
    }

    /// Sleep before reconnecting, still listening for commands. `false` means stop.
    async fn pause(&mut self, delay: Duration) -> bool {
        let until = Instant::now() + delay;
        loop {
            tokio::select! {
                _ = sleep_until(until) => return true,
                command = self.control.recv(), if self.control_open => match command {
                    Some(Command::Stop) => return false,
                    Some(Command::Throttle(per_update)) => self.throttle = per_update,
                    Some(Command::Hold(offline_for)) => self.hold = offline_for,
                    Some(Command::HandoffPatch(on)) => self.config.handoff_patch = on,
                    Some(Command::Cut) => {}
                    None => self.control_open = false,
                },
                Some(resolution) = self.resolved_rx.recv() => {
                    if !self.apply_resolution(resolution).await {
                        return false;
                    }
                }
                Some(msg) = self.patch_rx.recv() => {
                    if !self.on_patch(msg).await {
                        return false;
                    }
                }
            }
        }
    }

    async fn stop(&mut self, reason: &str) {
        let _ = self
            .emit(Event::State(State::Stopped {
                reason: reason.to_owned(),
            }))
            .await;
    }

    async fn emit_metrics(&mut self) -> bool {
        let elapsed = self.window_start.elapsed().as_secs_f64().max(1e-3);
        let tip = Some(self.tip.load(Ordering::Relaxed)).filter(|t| *t > 0);
        let highest_complete = self.cursor.highest_complete();
        let metrics = Metrics {
            updates_per_sec: self.stats.window_updates as f64 / elapsed,
            transactions_per_sec: self.stats.window_txs as f64 / elapsed,
            highest_complete,
            tip,
            lag_slots: tip.zip(highest_complete).map(|(t, h)| t.saturating_sub(h)),
            delivered: self.stats.delivered,
            duplicates: self.stats.duplicates,
            reconnects: self.stats.reconnects,
            incidents: self.stats.incidents,
            latency_ms: self.stats.latency_ms,
            dedup_entries: self.dedup.len(),
        };
        self.stats.window_updates = 0;
        self.stats.window_txs = 0;
        self.window_start = Instant::now();
        self.emit(Event::Metrics(metrics)).await
    }

    async fn emit(&self, event: Event) -> bool {
        self.events.send(event).await.is_ok()
    }

    fn next_generation(&self) -> u64 {
        let mut conn = self.conn.lock().expect("conn lock");
        conn.generation += 1;
        conn.id = None;
        conn.generation
    }

    /// Keep the tip fresh from Solami's `GetSlot`, so lag is honest even when we fall behind.
    fn spawn_tip_poller(&self) {
        let source = self.source.clone();
        let tip = self.tip.clone();
        let events = self.events.downgrade();
        let every = self.config.tip_interval;
        tokio::spawn(async move {
            while events.upgrade().is_some_and(|tx| !tx.is_closed()) {
                if let Ok(t) = source.tip().await {
                    tip.fetch_max(t, Ordering::Relaxed);
                }
                sleep(every).await;
            }
        });
    }

    /// Solami doesn't return a connection id on subscribe, so find ours as the new entry in the
    /// live-connection list.
    fn spawn_identify(&self, generation: u64, before: Option<HashSet<String>>) {
        let (Some(account), Some(before)) = (self.account.clone(), before) else {
            return;
        };
        let conn = self.conn.clone();
        let events = self.events.downgrade();
        let opened = conn.lock().expect("conn lock").connected_at;
        tokio::spawn(async move {
            for wait_ms in [300, 700, 1_500, 3_000] {
                sleep(Duration::from_millis(wait_ms)).await;
                if conn.lock().expect("conn lock").generation != generation {
                    return;
                }
                let Ok(live) = account.live().await else {
                    continue;
                };
                let Some(ours) = live
                    .into_iter()
                    .filter(|c| !before.contains(&c.conn_id))
                    .min_by_key(|c| c.started_at.abs_diff(opened))
                else {
                    continue;
                };
                {
                    let mut current = conn.lock().expect("conn lock");
                    if current.generation != generation {
                        return;
                    }
                    current.id = Some(ours.conn_id.clone());
                }
                if let Some(tx) = events.upgrade() {
                    let _ = tx
                        .send(Event::ConnectionIdentified {
                            conn_id: ours.conn_id,
                        })
                        .await;
                }
                return;
            }
        });
    }
}

async fn live_connection_ids(account: Option<AccountApi>) -> Option<HashSet<String>> {
    let live = account?.live().await.ok()?;
    Some(live.into_iter().map(|c| c.conn_id).collect())
}

fn to_system_time(seconds: i64, nanos: i32) -> Option<SystemTime> {
    let secs = u64::try_from(seconds).ok()?;
    Some(UNIX_EPOCH + Duration::new(secs, nanos.max(0) as u32))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(conn_id: &str, started_at: u64) -> ClosedConnection {
        ClosedConnection {
            conn_id: conn_id.into(),
            started_at,
            ended_at: Some(started_at + 2),
            region: None,
            bytes_streamed: 0,
            termination_reason: Some("backpressure".into()),
        }
    }

    #[test]
    fn history_match_prefers_the_known_id() {
        let rows = [row("a", 100), row("b", 1_000)];
        let before = HashSet::new();
        assert_eq!(
            match_history(&rows, Some("b"), 100, &before)
                .unwrap()
                .conn_id,
            "b"
        );
    }

    #[test]
    fn history_match_falls_back_to_start_time() {
        let rows = [
            row("old", 900),
            row("other", 1_004),
            row("ours", 1_003),
            row("later", 1_030),
        ];
        let before = HashSet::from(["other".to_owned()]);
        assert_eq!(
            match_history(&rows, None, 1_000, &before).unwrap().conn_id,
            "ours"
        );
        assert!(match_history(&rows[..1], None, 1_000, &before).is_none());
    }
}
