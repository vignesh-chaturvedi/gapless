//! A deterministic chain that plays Solami's part: every slot carries `TXS_PER_SLOT`
//! transactions followed by its `SlotProcessed` update, `from_slot` replays history, and each
//! connection can be scripted to fail.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use futures::stream::{self, StreamExt};
use gapless::solami::geyser::subscribe_update::UpdateOneof;
use gapless::solami::geyser::{
    SlotStatus, SubscribeRequest, SubscribeUpdate, SubscribeUpdateSlot, SubscribeUpdateTransaction,
    SubscribeUpdateTransactionInfo,
};
use gapless::{Source, UpdateStream};
use tonic::Status;

pub const TXS_PER_SLOT: u64 = 4;
/// Four transactions, then SlotProcessed, SlotConfirmed(slot - 1) and SlotFinalized(slot - 32).
pub const UPDATES_PER_SLOT: usize = 7;

pub fn sig(slot: u64, index: u64) -> [u8; 64] {
    let mut s = [0u8; 64];
    s[..8].copy_from_slice(&slot.to_le_bytes());
    s[8..16].copy_from_slice(&index.to_le_bytes());
    s[63] = 1;
    s
}

#[derive(Clone)]
pub enum Ending {
    /// `Cancelled: stream terminated by user`, as after a kill through the account API.
    Killed,
    /// Clean end-of-stream, which is how Solami's backpressure close reaches the client.
    Eof,
    /// Stop sending anything, without closing.
    Hang,
}

/// What one connection does.
#[derive(Clone)]
pub struct Script {
    pub end_after: Option<usize>,
    pub ending: Ending,
    /// Slots the chain advances after this connection ends, before anyone reconnects.
    pub outage_after: u64,
    pub reject: Option<Status>,
    pub duplicate_every: Option<usize>,
    /// On a `from_slot` subscription, drop this many transactions from the start of the first
    /// slot past the tip: the slot executing when Solami switches the stream to the live feed.
    pub handoff_loss: usize,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            end_after: None,
            ending: Ending::Eof,
            outage_after: 0,
            reject: None,
            duplicate_every: None,
            handoff_loss: 0,
        }
    }
}

impl Script {
    pub fn cut(after: usize, ending: Ending, outage_after: u64) -> Self {
        Self {
            end_after: Some(after),
            ending,
            outage_after,
            ..Self::default()
        }
    }

    pub fn reject(status: Status) -> Self {
        Self {
            reject: Some(status),
            ..Self::default()
        }
    }
}

struct State {
    tip: u64,
    horizon: u64,
    scripts: VecDeque<Script>,
    from_slots: Vec<Option<u64>>,
    duplicates_sent: u64,
}

#[derive(Clone)]
pub struct FakeChain(Arc<Mutex<State>>);

impl FakeChain {
    pub fn new(tip: u64, horizon: u64, scripts: Vec<Script>) -> Self {
        Self(Arc::new(Mutex::new(State {
            tip,
            horizon,
            scripts: scripts.into(),
            from_slots: Vec::new(),
            duplicates_sent: 0,
        })))
    }

    /// `from_slot` of every subscribe request, in order.
    pub fn requested_from_slots(&self) -> Vec<Option<u64>> {
        self.0.lock().unwrap().from_slots.clone()
    }

    pub fn duplicates_sent(&self) -> u64 {
        self.0.lock().unwrap().duplicates_sent
    }
}

fn update(kind: UpdateOneof) -> SubscribeUpdate {
    SubscribeUpdate {
        filters: vec!["gapless".into()],
        update_oneof: Some(kind),
        created_at: None,
    }
}

fn tx(slot: u64, index: u64) -> SubscribeUpdate {
    update(UpdateOneof::Transaction(SubscribeUpdateTransaction {
        transaction: Some(SubscribeUpdateTransactionInfo {
            signature: sig(slot, index).to_vec(),
            is_vote: false,
            transaction: None,
            meta: None,
            index,
        }),
        slot,
    }))
}

fn slot(slot: u64, status: SlotStatus) -> SubscribeUpdate {
    update(UpdateOneof::Slot(SubscribeUpdateSlot {
        slot,
        parent: slot.checked_sub(1),
        status: status as i32,
        dead_error: None,
    }))
}

fn slot_updates(s: u64) -> Vec<SubscribeUpdate> {
    let mut out: Vec<_> = (0..TXS_PER_SLOT).map(|i| tx(s, i)).collect();
    out.push(slot(s, SlotStatus::SlotProcessed));
    out.push(slot(s.saturating_sub(1), SlotStatus::SlotConfirmed));
    out.push(slot(s.saturating_sub(32), SlotStatus::SlotFinalized));
    out
}

impl Source for FakeChain {
    fn subscribe(
        &self,
        request: SubscribeRequest,
    ) -> BoxFuture<'static, Result<UpdateStream, Status>> {
        let chain = self.0.clone();
        Box::pin(async move {
            let (start, script, handoff_slot) = {
                let mut s = chain.lock().unwrap();
                let script = s.scripts.pop_front().unwrap_or_default();
                s.from_slots.push(request.from_slot);
                if let Some(status) = script.reject.clone() {
                    return Err(status);
                }
                let first_available = s.tip.saturating_sub(s.horizon);
                if let Some(from) = request.from_slot
                    && from < first_available
                {
                    return Err(Status::invalid_argument(
                        "from_slot is older than the replay horizon",
                    ));
                }
                let handoff_slot = request.from_slot.map(|_| s.tip + 1);
                (request.from_slot.unwrap_or(s.tip + 1), script, handoff_slot)
            };

            let producer = chain.clone();
            let loss = script.handoff_loss;
            let mut lost = 0usize;
            let mut txs_sent = 0usize;
            let dup_every = script.duplicate_every;
            let counter = chain.clone();
            let updates = stream::iter(start..)
                .flat_map(move |s| {
                    {
                        let mut state = producer.lock().unwrap();
                        state.tip = state.tip.max(s);
                    }
                    stream::iter(slot_updates(s))
                })
                .filter(move |u| {
                    let drop = match (&u.update_oneof, handoff_slot) {
                        (Some(UpdateOneof::Transaction(t)), Some(h))
                            if t.slot == h && lost < loss =>
                        {
                            lost += 1;
                            true
                        }
                        _ => false,
                    };
                    futures::future::ready(!drop)
                })
                .flat_map(move |u| {
                    let is_tx = matches!(u.update_oneof, Some(UpdateOneof::Transaction(_)));
                    if is_tx {
                        txs_sent += 1;
                    }
                    if is_tx && dup_every.is_some_and(|k| txs_sent.is_multiple_of(k)) {
                        counter.lock().unwrap().duplicates_sent += 1;
                        stream::iter(vec![u.clone(), u])
                    } else {
                        stream::iter(vec![u])
                    }
                })
                .map(Ok::<_, Status>);

            let Some(n) = script.end_after else {
                return Ok(Box::pin(updates) as UpdateStream);
            };
            let outage = script.outage_after;
            let ending = script.ending.clone();
            let ended = chain.clone();
            let tail = stream::once(async move {
                ended.lock().unwrap().tip += outage;
                match ending {
                    Ending::Killed => Some(Err(Status::cancelled("stream terminated by user"))),
                    Ending::Eof | Ending::Hang => None,
                }
            })
            .filter_map(|item| async move { item });
            let hang = matches!(script.ending, Ending::Hang);
            let head = updates.take(n).chain(tail);
            if hang {
                Ok(Box::pin(head.chain(stream::pending())) as UpdateStream)
            } else {
                Ok(Box::pin(head) as UpdateStream)
            }
        })
    }

    fn tip(&self) -> BoxFuture<'static, Result<u64, Status>> {
        let tip = self.0.lock().unwrap().tip;
        Box::pin(async move { Ok(tip) })
    }

    fn first_available(&self) -> BoxFuture<'static, Result<Option<u64>, Status>> {
        let s = self.0.lock().unwrap();
        let first = s.tip.saturating_sub(s.horizon);
        Box::pin(async move { Ok(Some(first)) })
    }
}
