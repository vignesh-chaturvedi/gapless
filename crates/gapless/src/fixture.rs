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
use std::time::Duration;

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
}

impl FixtureSource {
    pub fn new(fixture: Fixture) -> Self {
        Self {
            fixture: Arc::new(fixture),
            started: Instant::now(),
        }
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
                    let first = this.current_tip().saturating_sub(REPLAY_HORIZON);
                    if slot < first {
                        return Err(Status::invalid_argument(
                            "from_slot is older than the replay horizon",
                        ));
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
            let stream = stream::unfold((this, k, index), |(this, mut k, mut index)| async move {
                let fixture = this.fixture.clone();
                let due = fixture.duration * k as u32 + fixture.frames[index].offset;
                let elapsed = this.started.elapsed();
                if due > elapsed {
                    tokio::time::sleep(due - elapsed).await;
                }
                let update = fixture.frame_in_loop(index, k);
                index += 1;
                if index == fixture.frames.len() {
                    index = 0;
                    k += 1;
                }
                Some((Ok(update), (this, k, index)))
            });
            Ok(Box::pin(stream) as UpdateStream)
        })
    }

    fn tip(&self) -> BoxFuture<'static, Result<u64, Status>> {
        let tip = self.current_tip();
        Box::pin(async move { Ok(tip) })
    }

    fn first_available(&self) -> BoxFuture<'static, Result<Option<u64>, Status>> {
        let first = self.current_tip().saturating_sub(REPLAY_HORIZON);
        Box::pin(async move { Ok(Some(first)) })
    }
}

#[cfg(test)]
mod tests {
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
}
