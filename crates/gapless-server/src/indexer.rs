//! The sample consumer: a tiny Pump.fun indexer. It decodes the Anchor events Pump.fun emits
//! through a self-CPI (`TradeEvent`, `CreateEvent`, `CompleteEvent`) and keeps per-minute
//! counts. Minutes come from the event's own timestamp, so replayed trades land in the minute
//! they happened. Because Gapless delivers each transaction exactly once, these counts never
//! double across an outage.

use std::collections::{BTreeMap, HashSet, VecDeque};

use gapless::solami::geyser::SubscribeUpdateTransactionInfo;
use serde::Serialize;
use sha2::{Digest, Sha256};

/// Anchor's `EVENT_IX_TAG`, as it appears at the start of an event self-CPI's data.
const EVENT_TAG: [u8; 8] = [0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];
const MINUTES_KEPT: usize = 60;
const LAUNCHES_KEPT: usize = 20;

fn event_discriminator(name: &str) -> [u8; 8] {
    Sha256::digest(format!("event:{name}").as_bytes())[..8]
        .try_into()
        .expect("8 bytes")
}

#[derive(Clone, Debug, PartialEq)]
pub enum Activity {
    Trade {
        mint: [u8; 32],
        lamports: u64,
        is_buy: bool,
        user: [u8; 32],
        timestamp: i64,
    },
    Create {
        mint: [u8; 32],
        name: String,
        symbol: String,
        user: [u8; 32],
        timestamp: Option<i64>,
    },
    Complete {
        mint: [u8; 32],
        timestamp: Option<i64>,
    },
}

pub struct Decoder {
    program: [u8; 32],
    trade: [u8; 8],
    create: [u8; 8],
    complete: [u8; 8],
}

impl Decoder {
    pub fn new(program: &str) -> Self {
        let bytes = bs58::decode(program).into_vec().unwrap_or_default();
        let program = bytes.try_into().unwrap_or([0; 32]);
        Self {
            program,
            trade: event_discriminator("TradeEvent"),
            create: event_discriminator("CreateEvent"),
            complete: event_discriminator("CompleteEvent"),
        }
    }

    /// Every Pump.fun event the transaction emitted.
    pub fn activities(&self, info: &SubscribeUpdateTransactionInfo) -> Vec<Activity> {
        let (Some(tx), Some(meta)) = (&info.transaction, &info.meta) else {
            return Vec::new();
        };
        let Some(message) = &tx.message else {
            return Vec::new();
        };
        let keys: Vec<&[u8]> = message
            .account_keys
            .iter()
            .chain(&meta.loaded_writable_addresses)
            .chain(&meta.loaded_readonly_addresses)
            .map(Vec::as_slice)
            .collect();
        let mut out = Vec::new();
        for group in &meta.inner_instructions {
            for ix in &group.instructions {
                let own = keys
                    .get(ix.program_id_index as usize)
                    .is_some_and(|k| *k == self.program);
                if own
                    && ix.data.len() >= 16
                    && ix.data[..8] == EVENT_TAG
                    && let Some(activity) = self.decode(&ix.data[8..16], &ix.data[16..])
                {
                    out.push(activity);
                }
            }
        }
        out
    }

    fn decode(&self, discriminator: &[u8], p: &[u8]) -> Option<Activity> {
        if discriminator == self.trade {
            // mint, sol_amount, token_amount, is_buy, user, timestamp, ...
            Some(Activity::Trade {
                mint: p.get(0..32)?.try_into().ok()?,
                lamports: u64::from_le_bytes(p.get(32..40)?.try_into().ok()?),
                is_buy: *p.get(48)? == 1,
                user: p.get(49..81)?.try_into().ok()?,
                timestamp: i64::from_le_bytes(p.get(81..89)?.try_into().ok()?),
            })
        } else if discriminator == self.create {
            // name, symbol, uri (borsh strings), mint, bonding_curve, user, creator, timestamp, ...
            let mut at = 0;
            let mut string = || -> Option<String> {
                let len = u32::from_le_bytes(p.get(at..at + 4)?.try_into().ok()?) as usize;
                let s = String::from_utf8_lossy(p.get(at + 4..at + 4 + len)?).into_owned();
                at += 4 + len;
                Some(s)
            };
            let (name, symbol, _uri) = (string()?, string()?, string()?);
            let mint = p.get(at..at + 32)?.try_into().ok()?;
            let user = p.get(at + 64..at + 96)?.try_into().ok()?;
            let timestamp = p
                .get(at + 128..at + 136)
                .and_then(|b| b.try_into().ok())
                .map(i64::from_le_bytes);
            Some(Activity::Create {
                mint,
                name,
                symbol,
                user,
                timestamp,
            })
        } else if discriminator == self.complete {
            // user, mint, bonding_curve, timestamp
            Some(Activity::Complete {
                mint: p.get(32..64)?.try_into().ok()?,
                timestamp: p
                    .get(96..104)
                    .and_then(|b| b.try_into().ok())
                    .map(i64::from_le_bytes),
            })
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Minute {
    /// Unix minute.
    pub minute: u64,
    pub txs: u64,
    /// Of `txs`, how many arrived through a replay or the handoff patch: the ones a consumer
    /// without Gapless would have lost.
    pub replayed: u64,
    pub trades: u64,
    pub buys: u64,
    pub sells: u64,
    pub buy_sol: f64,
    pub sell_sol: f64,
    pub traders: u64,
    pub launches: u64,
    pub graduations: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Launch {
    pub mint: String,
    pub name: String,
    pub symbol: String,
    pub at: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexerDto {
    pub txs: u64,
    pub replayed: u64,
    pub trades: u64,
    pub buys: u64,
    pub sells: u64,
    pub buy_sol: f64,
    pub sell_sol: f64,
    /// Distinct traders over the kept minutes.
    pub unique_traders: u64,
    pub launches: u64,
    pub graduations: u64,
    pub minutes: Vec<Minute>,
    pub recent_launches: Vec<Launch>,
}

#[derive(Default)]
struct Bucket {
    minute: Minute,
    traders: HashSet<[u8; 32]>,
}

/// Per-minute Pump.fun activity.
#[derive(Default)]
pub struct Indexer {
    buckets: BTreeMap<u64, Bucket>,
    launches: VecDeque<Launch>,
    totals: Minute,
}

impl Indexer {
    /// Count one delivered transaction at its events' own timestamp. `clock` is used when no
    /// event carries one; `clock_only` ignores event timestamps (a looping fixture repeats them).
    /// `replayed` marks a transaction recovered after an outage.
    pub fn record(
        &mut self,
        activities: &[Activity],
        clock: i64,
        clock_only: bool,
        replayed: bool,
    ) {
        let stamped = activities.iter().find_map(|a| match a {
            Activity::Trade { timestamp, .. } => Some(*timestamp),
            Activity::Create { timestamp, .. } | Activity::Complete { timestamp, .. } => *timestamp,
        });
        let when = if clock_only {
            clock
        } else {
            stamped.unwrap_or(clock)
        };
        let minute = (when.max(0) / 60) as u64;
        let bucket = self.buckets.entry(minute).or_default();
        bucket.minute.minute = minute;
        bucket.minute.txs += 1;
        self.totals.txs += 1;
        if replayed {
            bucket.minute.replayed += 1;
            self.totals.replayed += 1;
        }
        for activity in activities {
            match activity {
                Activity::Trade {
                    lamports,
                    is_buy,
                    user,
                    ..
                } => {
                    let sol = *lamports as f64 / 1e9;
                    for m in [&mut bucket.minute, &mut self.totals] {
                        m.trades += 1;
                        if *is_buy {
                            m.buys += 1;
                            m.buy_sol += sol;
                        } else {
                            m.sells += 1;
                            m.sell_sol += sol;
                        }
                    }
                    bucket.traders.insert(*user);
                }
                Activity::Create {
                    mint, name, symbol, ..
                } => {
                    bucket.minute.launches += 1;
                    self.totals.launches += 1;
                    let address = bs58::encode(mint).into_string();
                    self.launches.retain(|l| l.mint != address);
                    self.launches.push_front(Launch {
                        mint: address,
                        name: name.clone(),
                        symbol: symbol.clone(),
                        at: (when.max(0) as u64) * 1000,
                    });
                    self.launches.truncate(LAUNCHES_KEPT);
                }
                Activity::Complete { .. } => {
                    bucket.minute.graduations += 1;
                    self.totals.graduations += 1;
                }
            }
        }
        bucket.minute.traders = bucket.traders.len() as u64;
        while self.buckets.len() > MINUTES_KEPT {
            self.buckets.pop_first();
        }
    }

    pub fn summary(&self) -> IndexerDto {
        let unique: HashSet<&[u8; 32]> = self
            .buckets
            .values()
            .flat_map(|b| b.traders.iter())
            .collect();
        IndexerDto {
            txs: self.totals.txs,
            replayed: self.totals.replayed,
            trades: self.totals.trades,
            buys: self.totals.buys,
            sells: self.totals.sells,
            buy_sol: self.totals.buy_sol,
            sell_sol: self.totals.sell_sol,
            unique_traders: unique.len() as u64,
            launches: self.totals.launches,
            graduations: self.totals.graduations,
            minutes: self
                .buckets
                .values()
                .rev()
                .take(30)
                .map(|b| b.minute.clone())
                .rev()
                .collect(),
            recent_launches: self.launches.iter().cloned().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use gapless::fixture::Fixture;
    use gapless::solami::geyser::subscribe_update::UpdateOneof;

    use super::*;

    const PUMP_FUN: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

    #[test]
    fn decodes_real_pump_fun_events_from_the_fixture() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/pumpfun-150s.bin.zst"
        );
        let fixture = Fixture::load(path).unwrap();
        let decoder = Decoder::new(PUMP_FUN);
        let mut indexer = Indexer::default();
        for frame in fixture.frames() {
            if let Some(UpdateOneof::Transaction(tx)) = &frame.update.update_oneof
                && let Some(info) = &tx.transaction
            {
                // Transactions without events fall back to a receive time in the same period.
                indexer.record(&decoder.activities(info), 1_790_381_400, false, false);
            }
        }
        let s = indexer.summary();
        assert!(s.txs > 5_000, "{} transactions", s.txs);
        assert!(
            s.trades > s.txs / 2,
            "most transactions trade: {} of {}",
            s.trades,
            s.txs
        );
        assert_eq!(s.buys + s.sells, s.trades);
        assert!(s.buy_sol > 0.0 && s.sell_sol > 0.0);
        assert!(s.launches > 20, "{} launches", s.launches);
        assert!(s.recent_launches.iter().all(|l| !l.symbol.is_empty()));
        assert!(
            s.minutes.iter().all(|m| m.minute > 29_000_000),
            "minutes come from event timestamps"
        );
    }

    #[test]
    fn a_looping_fixture_can_bucket_by_its_own_clock() {
        let launch = Activity::Create {
            mint: [7; 32],
            name: "Loop".into(),
            symbol: "LOOP".into(),
            user: [1; 32],
            timestamp: Some(60),
        };
        let mut indexer = Indexer::default();
        indexer.record(std::slice::from_ref(&launch), 600, true, false);
        indexer.record(std::slice::from_ref(&launch), 720, true, false);
        let s = indexer.summary();
        assert_eq!(s.launches, 2, "both launches count");
        assert_eq!(s.recent_launches.len(), 1, "one mint is listed once");
        assert_eq!(s.recent_launches[0].at, 720_000, "at the playback clock");
        assert_eq!(
            s.minutes.iter().map(|m| m.minute).collect::<Vec<_>>(),
            vec![10, 12],
            "the recorded timestamp is ignored"
        );
    }

    #[test]
    fn transactions_without_events_still_count() {
        let mut indexer = Indexer::default();
        indexer.record(&[], 120, false, false);
        indexer.record(&[], 121, false, true);
        let s = indexer.summary();
        assert_eq!((s.txs, s.trades, s.replayed), (2, 0, 1));
        assert_eq!((s.minutes[0].minute, s.minutes[0].replayed), (2, 1));
    }
}
