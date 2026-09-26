//! Recorded mainnet streams, and a [`Source`] that plays one back.
//!
//! A capture is a sequence of frames: `[u64 LE receive-time nanos][length-delimited
//! SubscribeUpdate]`, optionally zstd-compressed. [`FixtureSource`] replays it in real time and
//! loops forever. Each loop shifts slot numbers and signatures, so the playback looks like one
//! continuous chain and dedup never mistakes a later loop for the earlier one. `from_slot`
//! replays history from any slot the playback has already passed, as fast as the consumer reads.

use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use futures::future::BoxFuture;
use futures::stream;
use prost::Message;
use solami::geyser::subscribe_update::UpdateOneof;
use solami::geyser::{SlotStatus, SubscribeRequest, SubscribeUpdate};
use tokio::time::Instant;
use tonic::Status;

use crate::Error;
use crate::config::REPLAY_HORIZON;
use crate::event::{Signature, SlotRange};
use crate::source::{Source, UpdateStream};

const ZSTD_MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];

/// Solami's default per-stream send buffer, in messages. [`FixtureSource`] emulates it: once a
/// stream has caught up with the live edge, frames that are due but not yet read count as
/// pending, and a stream that falls this far behind is closed for backpressure, as Solami does.
pub const EMULATED_BUFFER: u64 = 8_192;
/// A stream within this many frames of the live edge has caught up.
const CAUGHT_UP: u64 = 64;
/// Slots past the tip at resume time where the emulated handoff loss hits (mainnet: 7 and 8).
const HANDOFF_OFFSET: u64 = 7;

/// One recorded update.
#[derive(Clone, Debug)]
pub struct Frame {
    /// Time since the first frame.
    pub offset: Duration,
    pub update: SubscribeUpdate,
}

/// A loaded capture.
#[derive(Debug)]
pub struct Fixture {
    frames: Vec<Frame>,
    /// For each frame, the highest transaction or `SlotProcessed` slot seen so far.
    position: Vec<u64>,
    first_slot: u64,
    last_slot: u64,
    duration: Duration,
}

impl Fixture {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)
            .map_err(|e| Error::Config(format!("reading {}: {e}", path.display())))?;
        Self::parse(&bytes)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let raw;
        let mut rest: &[u8] = if bytes.starts_with(&ZSTD_MAGIC) {
            raw = zstd::decode_all(bytes).map_err(|e| Error::Config(format!("zstd: {e}")))?;
            &raw
        } else {
            bytes
        };
        let mut frames = Vec::new();
        let mut t0 = None;
        while !rest.is_empty() {
            if rest.len() < 8 {
                return Err(Error::Config("truncated fixture frame".into()));
            }
            let nanos = u64::from_le_bytes(rest[..8].try_into().expect("8 bytes"));
            rest = &rest[8..];
            let update = SubscribeUpdate::decode_length_delimited(&mut rest)
                .map_err(|e| Error::Config(format!("fixture frame: {e}")))?;
            if matches!(update.update_oneof, Some(UpdateOneof::Ping(_)) | None) {
                continue;
            }
            let start = *t0.get_or_insert(nanos);
            frames.push(Frame {
                offset: Duration::from_nanos(nanos.saturating_sub(start)),
                update,
            });
        }
        Self::from_frames(frames)
    }

    pub fn from_frames(frames: Vec<Frame>) -> Result<Self, Error> {
        let mut position = Vec::with_capacity(frames.len());
        let (mut first_slot, mut last_slot, mut high) = (u64::MAX, 0u64, 0u64);
        for frame in &frames {
            if let Some(slot) = position_slot(&frame.update) {
                first_slot = first_slot.min(slot);
                last_slot = last_slot.max(slot);
                high = high.max(slot);
            }
            position.push(high);
        }
        if frames.is_empty() || first_slot == u64::MAX {
            return Err(Error::Config(
                "the fixture has no transactions or slots".into(),
            ));
        }
        // Frames before the first slot update carry position 0; give them the first slot.
        for p in position.iter_mut() {
            if *p == 0 {
                *p = first_slot;
            }
        }
        let duration = frames.last().expect("non-empty").offset + Duration::from_millis(400);
        Ok(Self {
            frames,
            position,
            first_slot,
            last_slot,
            duration,
        })
    }

    /// Write frames in the capture format, zstd-compressed at `level` (0 for raw).
    pub fn write<'a>(
        path: impl AsRef<Path>,
        frames: impl IntoIterator<Item = (u64, &'a SubscribeUpdate)>,
        level: i32,
    ) -> Result<(), Error> {
        let mut raw = Vec::new();
        for (nanos, update) in frames {
            raw.extend_from_slice(&nanos.to_le_bytes());
            update
                .encode_length_delimited(&mut raw)
                .map_err(|e| Error::Config(format!("encode: {e}")))?;
        }
        let bytes = if level > 0 {
            zstd::encode_all(raw.as_slice(), level)
                .map_err(|e| Error::Config(format!("zstd: {e}")))?
        } else {
            raw
        };
        let mut file = std::fs::File::create(path.as_ref())
            .map_err(|e| Error::Config(format!("create {}: {e}", path.as_ref().display())))?;
        file.write_all(&bytes)
            .map_err(|e| Error::Config(format!("write: {e}")))
    }

    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }

    pub fn duration(&self) -> Duration {
        self.duration
    }

    pub fn slots(&self) -> SlotRange {
        SlotRange::new(self.first_slot, self.last_slot)
    }

    /// Slot shift applied on each loop.
    fn span(&self) -> u64 {
        self.last_slot - self.first_slot + 1
    }

    /// Frame `index` as it plays in loop `k`.
    pub fn frame_in_loop(&self, index: usize, k: u64) -> SubscribeUpdate {
        rebase(&self.frames[index].update, k * self.span(), k)
    }

    /// Transactions the playback carries for `range` across every loop that overlaps it, as
    /// (signature, slot, index). This is the offline ground truth.
    pub fn transactions_in(&self, range: SlotRange) -> Vec<(Signature, u64, u64)> {
        let mut out = Vec::new();
        for k in self.loops_covering(range) {
            for index in 0..self.frames.len() {
                if let Some(UpdateOneof::Transaction(tx)) =
                    self.frame_in_loop(index, k).update_oneof
                    && range.contains(tx.slot)
                    && let Some(info) = tx.transaction
                    && let Some(signature) = Signature::from_bytes(&info.signature)
                {
                    out.push((signature, tx.slot, info.index));
                }
            }
        }
        out
    }

    /// Slots in `range` that the playback completes (the offline stand-in for "has a block").
    pub fn slots_in(&self, range: SlotRange) -> Vec<u64> {
        let mut out = Vec::new();
        for k in self.loops_covering(range) {
            for frame in &self.frames {
                if let Some(UpdateOneof::Slot(s)) = &frame.update.update_oneof
                    && s.status() == SlotStatus::SlotProcessed
                {
                    let slot = s.slot + k * self.span();
                    if range.contains(slot) {
                        out.push(slot);
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    fn loops_covering(&self, range: SlotRange) -> std::ops::RangeInclusive<u64> {
        let k = |slot: u64| slot.saturating_sub(self.first_slot) / self.span();
        k(range.first)..=k(range.last)
    }

    /// The frame to start from so a replay begins at `slot`.
    fn locate(&self, slot: u64) -> (u64, usize) {
        let k = slot.saturating_sub(self.first_slot) / self.span();
        let local = slot - k * self.span();
        let index = self.position.partition_point(|&p| p < local);
        if index >= self.frames.len() {
            (k + 1, 0)
        } else {
            (k, index)
        }
    }
}

fn is_transaction(update: &SubscribeUpdate) -> bool {
    matches!(update.update_oneof, Some(UpdateOneof::Transaction(_)))
}

/// Where a resumed stream meets the live feed: the next slot with transactions, and how many of
/// them (the first half) to lose.
fn handoff_cut(fixture: &Fixture, from: usize) -> Option<(u64, usize)> {
    let first =
        (from..fixture.frames.len()).find(|&i| is_transaction(&fixture.frames[i].update))?;
    let slot = fixture.position[first];
    let txs = (first..fixture.frames.len())
        .take_while(|&i| fixture.position[i] == slot)
        .filter(|&i| is_transaction(&fixture.frames[i].update))
        .count();
    Some((slot, txs.div_ceil(2)))
}

/// The slot a frame belongs to for positioning: a transaction's slot, or a `SlotProcessed` slot.
fn position_slot(update: &SubscribeUpdate) -> Option<u64> {
    match &update.update_oneof {
        Some(UpdateOneof::Transaction(tx)) => Some(tx.slot),
        Some(UpdateOneof::Slot(s)) if s.status() == SlotStatus::SlotProcessed => Some(s.slot),
        _ => None,
    }
}

/// Shift slots by `offset`; on loops after the first, also make signatures unique.
fn rebase(update: &SubscribeUpdate, offset: u64, k: u64) -> SubscribeUpdate {
    let mut update = update.clone();
    update.created_at = None;
    if k == 0 {
        return update;
    }
    match &mut update.update_oneof {
        Some(UpdateOneof::Transaction(tx)) => {
            tx.slot += offset;
            if let Some(info) = &mut tx.transaction {
                relabel(&mut info.signature, k);
                if let Some(first) = info
                    .transaction
                    .as_mut()
                    .and_then(|t| t.signatures.first_mut())
                {
                    relabel(first, k);
                }
            }
        }
        Some(UpdateOneof::Slot(s)) => {
            s.slot += offset;
            s.parent = s.parent.map(|p| p + offset);
        }
        _ => {}
    }
    update
}

fn relabel(signature: &mut [u8], k: u64) {
    for (byte, key) in signature.iter_mut().zip(k.to_le_bytes()) {
        *byte ^= key;
    }
}

/// Plays a [`Fixture`] as if it were Solami: live frames arrive at their recorded pace, and
/// `from_slot` replays already-played history as fast as it's read.
#[derive(Clone)]
pub struct FixtureSource {
    fixture: Arc<Fixture>,
    started: Instant,
    /// Wall-clock time of `started`, for [`FixtureSource::slot_time`].
    started_at: SystemTime,
    pending: Arc<AtomicU64>,
    horizon: u64,
    handoff_loss: bool,
}

/// One subscription's read position.
struct Playback {
    source: FixtureSource,
    k: u64,
    index: usize,
    /// Reached the live edge (and started reporting into the buffer).
    caught_up: bool,
    handoff: Handoff,
    closed: bool,
}

/// Where a resumed stream will lose transactions, as Solami's does.
enum Handoff {
    None,
    /// Cut the first slot with transactions at or after this (absolute) slot.
    At(u64),
    /// Dropping from this slot: (loop, local slot, transactions still to drop).
    Cutting(u64, u64, usize),
}

impl Playback {
    fn advance(&mut self) {
        self.index += 1;
        if self.index == self.source.fixture.frames.len() {
            self.index = 0;
            self.k += 1;
        }
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        // Only a stream that reached the live edge reports into the buffer.
        if self.caught_up {
            self.source.pending.store(0, Ordering::Relaxed);
        }
    }
}

impl FixtureSource {
    pub fn new(fixture: Fixture) -> Self {
        Self {
            fixture: Arc::new(fixture),
            started: Instant::now(),
            started_at: SystemTime::now(),
            pending: Arc::new(AtomicU64::new(0)),
            horizon: REPLAY_HORIZON,
            handoff_loss: true,
        }
    }

    /// Emulate Solami's handoff loss (on by default): a resumed stream loses the first half of
    /// the transactions in the slot where it switches to the live feed, 7 slots past the tip at
    /// the time of the resume, though the slot still completes. Phase 2 found this on mainnet
    /// (at the target + 7 and + 8); the handoff patch recovers it.
    pub fn with_handoff_loss(mut self, on: bool) -> Self {
        self.handoff_loss = on;
        self
    }

    /// Replay only this many slots back (Solami's is 3,000), so an outage past the horizon can
    /// be tried in a minute instead of thirteen.
    pub fn with_horizon(mut self, slots: u64) -> Self {
        self.horizon = slots;
        self
    }

    /// When the playback produced `slot`: the fixture's stand-in for block time. A recording's
    /// own event timestamps repeat on every loop, so consumers that bucket by time use this.
    pub fn slot_time(&self, slot: u64) -> SystemTime {
        let (k, index) = self.fixture.locate(slot);
        let offset = self
            .fixture
            .frames
            .get(index)
            .map_or(Duration::ZERO, |f| f.offset);
        self.started_at + self.fixture.duration * k as u32 + offset
    }

    /// Messages waiting in the emulated send buffer of the stream at the live edge.
    pub fn buffer_pending(&self) -> u64 {
        self.pending.load(Ordering::Relaxed)
    }

    pub fn fixture(&self) -> &Arc<Fixture> {
        &self.fixture
    }

    /// Playback position now: (loop, next frame index).
    fn now(&self) -> (u64, usize) {
        let elapsed = self.started.elapsed();
        let d = self.fixture.duration;
        let k = (elapsed.as_nanos() / d.as_nanos()) as u64;
        let within = elapsed - d * k as u32;
        let index = self.fixture.frames.partition_point(|f| f.offset <= within);
        (k, index)
    }

    /// The playback's current tip: the last completed slot it has played.
    pub fn current_tip(&self) -> u64 {
        let (k, index) = self.now();
        let fixture = &self.fixture;
        match index.checked_sub(1) {
            Some(i) => fixture.position[i] + k * fixture.span(),
            None if k > 0 => fixture.last_slot + (k - 1) * fixture.span(),
            None => fixture.first_slot,
        }
    }
}

impl Source for FixtureSource {
    fn subscribe(
        &self,
        request: SubscribeRequest,
    ) -> BoxFuture<'static, Result<UpdateStream, Status>> {
        let this = self.clone();
        Box::pin(async move {
            let (now_k, now_index) = this.now();
            let (k, index) = match request.from_slot {
                Some(slot) => {
                    let first = this.current_tip().saturating_sub(this.horizon);
                    if slot < first {
                        return Err(Status::out_of_range(format!(
                            "broadcast from {slot} is not available, last available: {first}"
                        )));
                    }
                    let located = this.fixture.locate(slot);
                    // Never start in the future.
                    if (located.0, located.1) > (now_k, now_index) {
                        (now_k, now_index)
                    } else {
                        located
                    }
                }
                None => (now_k, now_index),
            };
            let resumed = request.from_slot.is_some() && (k, index) < (now_k, now_index);
            let handoff = if resumed && this.handoff_loss {
                Handoff::At(this.current_tip() + HANDOFF_OFFSET)
            } else {
                Handoff::None
            };
            let playback = Playback {
                source: this,
                k,
                index,
                caught_up: false,
                handoff,
                closed: false,
            };
            let stream = stream::unfold(playback, |mut p| async move {
                if p.closed {
                    return None;
                }
                let fixture = p.source.fixture.clone();
                let len = fixture.frames.len() as u64;
                loop {
                    let due = fixture.duration * p.k as u32 + fixture.frames[p.index].offset;
                    let elapsed = p.source.started.elapsed();
                    if due > elapsed {
                        tokio::time::sleep(due - elapsed).await;
                    }
                    // Frames already due that this consumer hasn't read: Solami's `buffer_pending`.
                    let (now_k, now_index) = p.source.now();
                    let backlog =
                        (now_k * len + now_index as u64).saturating_sub(p.k * len + p.index as u64);
                    if backlog <= CAUGHT_UP {
                        p.caught_up = true;
                    }
                    if p.caught_up {
                        p.source.pending.store(backlog, Ordering::Relaxed);
                        if backlog > EMULATED_BUFFER {
                            p.closed = true;
                            let status = Status::resource_exhausted(
                                "stream backpressure: client too slow, please reconnect",
                            );
                            return Some((Err(status), p));
                        }
                    }
                    let position = fixture.position[p.index];
                    let is_tx = is_transaction(&fixture.frames[p.index].update);
                    if let Handoff::At(at) = p.handoff
                        && is_tx
                        && position + p.k * fixture.span() >= at
                    {
                        p.handoff = handoff_cut(&fixture, p.index)
                            .map_or(Handoff::None, |(slot, n)| Handoff::Cutting(p.k, slot, n));
                    }
                    if let Handoff::Cutting(k, slot, left) = &mut p.handoff {
                        if p.k != *k || position > *slot || *left == 0 {
                            p.handoff = Handoff::None;
                        } else if position == *slot && is_tx {
                            *left -= 1;
                            p.advance();
                            continue;
                        }
                    }
                    let update = fixture.frame_in_loop(p.index, p.k);
                    p.advance();
                    return Some((Ok(update), p));
                }
            });
            Ok(Box::pin(stream) as UpdateStream)
        })
    }

    fn tip(&self) -> BoxFuture<'static, Result<u64, Status>> {
        let tip = self.current_tip();
        Box::pin(async move { Ok(tip) })
    }

    fn first_available(&self) -> BoxFuture<'static, Result<Option<u64>, Status>> {
        let first = self.current_tip().saturating_sub(self.horizon);
        Box::pin(async move { Ok(Some(first)) })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use futures::StreamExt;
    use solami::geyser::{
        SubscribeUpdateSlot, SubscribeUpdateTransaction, SubscribeUpdateTransactionInfo,
    };

    use super::*;

    fn tx(slot: u64, n: u8) -> SubscribeUpdate {
        SubscribeUpdate {
            filters: vec![],
            created_at: None,
            update_oneof: Some(UpdateOneof::Transaction(SubscribeUpdateTransaction {
                slot,
                transaction: Some(SubscribeUpdateTransactionInfo {
                    signature: vec![n; 64],
                    is_vote: false,
                    transaction: None,
                    meta: None,
                    index: n as u64,
                }),
            })),
        }
    }

    fn processed(slot: u64) -> SubscribeUpdate {
        SubscribeUpdate {
            filters: vec![],
            created_at: None,
            update_oneof: Some(UpdateOneof::Slot(SubscribeUpdateSlot {
                slot,
                parent: Some(slot - 1),
                status: SlotStatus::SlotProcessed as i32,
                dead_error: None,
            })),
        }
    }

    /// Slots 100..=104, one transaction each, 400 ms apart.
    fn fixture() -> Fixture {
        let mut frames = Vec::new();
        for (i, slot) in (100..=104).enumerate() {
            let offset = Duration::from_millis(400 * i as u64);
            frames.push(Frame {
                offset,
                update: tx(slot, i as u8 + 1),
            });
            frames.push(Frame {
                offset,
                update: processed(slot),
            });
        }
        Fixture::from_frames(frames).unwrap()
    }

    #[test]
    fn round_trips_through_the_file_format() {
        let dir = std::env::temp_dir().join(format!("gapless-fixture-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("f.bin.zst");
        let f = fixture();
        let frames: Vec<_> = f
            .frames()
            .iter()
            .map(|fr| (fr.offset.as_nanos() as u64, &fr.update))
            .collect();
        Fixture::write(&path, frames, 3).unwrap();
        let loaded = Fixture::load(&path).unwrap();
        assert_eq!(loaded.frames().len(), f.frames().len());
        assert_eq!(loaded.slots(), SlotRange::new(100, 104));
    }

    #[test]
    fn later_loops_shift_slots_and_signatures() {
        let f = fixture();
        let first = f.transactions_in(SlotRange::new(100, 104));
        let second = f.transactions_in(SlotRange::new(105, 109));
        assert_eq!(first.len(), 5);
        assert_eq!(second.len(), 5);
        assert_eq!(second[0].1, 105);
        assert_ne!(first[0].0, second[0].0, "each loop gets its own signatures");
        assert_eq!(
            f.slots_in(SlotRange::new(103, 106)),
            vec![103, 104, 105, 106]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn plays_live_then_replays_history_fast() {
        let source = FixtureSource::new(fixture());
        tokio::time::advance(Duration::from_millis(1_300)).await;
        assert_eq!(source.current_tip(), 103);

        let request = SubscribeRequest {
            from_slot: Some(101),
            ..Default::default()
        };
        let mut stream = source.subscribe(request).await.unwrap();
        let mut slots = Vec::new();
        for _ in 0..6 {
            if let Some(UpdateOneof::Transaction(t)) =
                stream.next().await.unwrap().unwrap().update_oneof
            {
                slots.push(t.slot);
            }
        }
        assert_eq!(
            slots,
            vec![101, 102, 103],
            "history arrives without waiting"
        );
    }
    #[tokio::test(start_paused = true)]
    async fn a_resumed_stream_loses_part_of_the_slot_where_it_meets_live() {
        // Slots 100..300, four transactions and a SlotProcessed each, 10 ms apart.
        let mut frames = Vec::new();
        for (i, slot) in (100..300u64).enumerate() {
            let offset = Duration::from_millis(10 * i as u64);
            for n in 0..4 {
                frames.push(Frame {
                    offset,
                    update: tx(slot, (slot * 4 + n) as u8),
                });
            }
            frames.push(Frame {
                offset,
                update: processed(slot),
            });
        }
        let source = FixtureSource::new(Fixture::from_frames(frames).unwrap());
        tokio::time::advance(Duration::from_millis(1_500)).await;

        let request = SubscribeRequest {
            from_slot: Some(110),
            ..Default::default()
        };
        let mut stream = source.subscribe(request).await.unwrap();
        let mut txs: HashMap<u64, usize> = HashMap::new();
        let mut completed = Vec::new();
        while completed.last().is_none_or(|s| *s < 280) {
            match stream.next().await.unwrap().unwrap().update_oneof {
                Some(UpdateOneof::Transaction(t)) => *txs.entry(t.slot).or_default() += 1,
                Some(UpdateOneof::Slot(s)) => completed.push(s.slot),
                _ => {}
            }
        }
        let short: Vec<_> = (110..=280)
            .filter(|s| txs.get(s).copied().unwrap_or(0) < 4)
            .collect();
        assert_eq!(
            short.len(),
            1,
            "exactly one slot loses transactions: {short:?}"
        );
        assert_eq!(
            txs.get(&short[0]).copied().unwrap_or(0),
            2,
            "the first half of it"
        );
        assert!(completed.contains(&short[0]), "and it still completes");
        assert_eq!(short[0], 257, "7 slots past the tip at the resume (250)");
    }

    #[tokio::test(start_paused = true)]
    async fn a_slow_reader_fills_the_emulated_buffer_and_is_closed() {
        let frames = (0..10_000u64)
            .map(|i| Frame {
                offset: Duration::from_millis(i),
                update: processed(100 + i),
            })
            .collect();
        let source = FixtureSource::new(Fixture::from_frames(frames).unwrap());
        let mut stream = source.subscribe(SubscribeRequest::default()).await.unwrap();
        stream.next().await.unwrap().unwrap();
        assert!(source.buffer_pending() <= CAUGHT_UP);

        // Stop reading for 5 s: 5,000 frames come due.
        tokio::time::advance(Duration::from_millis(5_000)).await;
        stream.next().await.unwrap().unwrap();
        let pending = source.buffer_pending();
        assert!((4_900..=5_100).contains(&pending), "{pending} pending");

        // Past 8,192 the stream is closed for backpressure, as Solami does.
        tokio::time::advance(Duration::from_millis(4_000)).await;
        let error = stream.next().await.unwrap().unwrap_err();
        assert_eq!(error.code(), tonic::Code::ResourceExhausted);
        assert!(stream.next().await.is_none());
        drop(stream);
        assert_eq!(source.buffer_pending(), 0);
    }
}
