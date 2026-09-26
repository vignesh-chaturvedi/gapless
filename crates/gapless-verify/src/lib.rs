//! Proof for a Gapless stream: rebuild what the chain says a filter should have delivered for a
//! slot range, and compare it with what the stream actually delivered.
//!
//! The expected set comes from Solami's `getTransactionsForAddress`, filtered by slot range and
//! status. A few slots are also checked independently against full blocks from `getBlock`.
//! Only finalized slots are verified: Solami returns recent confirmed blocks with an empty
//! transaction list until they're indexed.

mod block;
mod diff;
mod history;
mod report;
mod rpc;

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use gapless::{Signature, SlotRange};

pub use block::{BlockMatch, VOTE_PROGRAM, matching};
pub use diff::{Diff, SlotCount, diff};
pub use history::{Landed, expected};
pub use report::{MissingTx, Moved, Repaired, Report, SlotRow, SpotCheck, TxRef, Verdict};
pub use rpc::{Rpc, RpcStats};

pub const DEFAULT_RPC_URL: &str = "https://rpc.solami.dev/sol";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("rate limited by the RPC")]
    RateLimited,
    #[error("RPC transport: {0}")]
    Transport(String),
    #[error("{method}: RPC error {code}: {message}")]
    Rpc {
        method: String,
        code: i64,
        message: String,
    },
    #[error("no block ({code}): {message}")]
    NoBlock { code: i64, message: String },
    #[error("slot {needed} isn't finalized yet (finalized tip {finalized})")]
    NotFinalized { needed: u64, finalized: u64 },
    #[error("unexpected RPC response: {0}")]
    Unexpected(String),
    #[error("{0}")]
    Unsupported(String),
}

/// The most missing transactions [`Verifier::repair`] fetches for one report.
pub const REPAIR_MAX: usize = 500;

#[derive(Clone, Debug)]
pub struct Options {
    /// Slots per `getTransactionsForAddress` request chunk.
    pub chunk_slots: u64,
    /// Chunks fetched at once.
    pub concurrency: usize,
    /// Evenly spaced slots to check against full blocks.
    pub spot_checks: usize,
    /// Extra block checks for slots with differences, to explain them.
    pub explain_checks: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            chunk_slots: 16,
            concurrency: 8,
            spot_checks: 3,
            explain_checks: 5,
        }
    }
}

/// The expected side of a verification: which transactions should have arrived, and which
/// slots have blocks.
pub struct GroundTruth<'a> {
    pub source: &'static str,
    pub addresses: &'a [String],
    pub finalized_tip: u64,
    pub blocks: &'a [u64],
    pub expected: &'a HashMap<Signature, Landed>,
}

/// Compare `delivered` (every signature the stream delivered, with its slot) against `truth`
/// for `range`, and assemble the report. [`Verifier::verify`] uses this with RPC data; offline
/// mode uses it with a fixture.
#[allow(clippy::too_many_arguments)]
pub fn build_report(
    range: SlotRange,
    truth: GroundTruth<'_>,
    delivered: &HashMap<Signature, u64>,
    spot_checks: Vec<SpotCheck>,
    positions: &HashMap<Signature, u64>,
    rpc: RpcStats,
    elapsed_ms: u64,
) -> Report {
    let canonical: HashSet<u64> = truth.blocks.iter().copied().collect();
    let d = diff::diff(range, truth.expected, &canonical, delivered);
    let verdict = if !d.missing.is_empty() {
        Verdict::Incomplete {
            missing: d.missing.len() as u64,
        }
    } else if !d.unexplained.is_empty() {
        Verdict::Inconclusive {
            reason: format!(
                "{} delivered transactions aren't in the expected set",
                d.unexplained.len()
            ),
        }
    } else if let Some(bad) = spot_checks.iter().find(|c| !c.agree) {
        Verdict::Inconclusive {
            reason: format!(
                "getBlock and getTransactionsForAddress disagree on slot {}",
                bad.slot
            ),
        }
    } else {
        Verdict::Complete
    };

    let per_slot = (range.first..=range.last)
        .map(|slot| {
            let count = d.per_slot.get(&slot).copied().unwrap_or_default();
            SlotRow {
                slot,
                has_block: canonical.contains(&slot),
                expected: count.expected,
                delivered: count.delivered,
                matched: count.matched,
            }
        })
        .collect();
    let tx = |(s, slot): &(Signature, u64)| TxRef {
        signature: s.to_string(),
        slot: *slot,
    };
    Report {
        range,
        finalized_tip: truth.finalized_tip,
        source: truth.source,
        addresses: truth.addresses.to_vec(),
        slots_with_blocks: truth.blocks.len() as u64,
        skipped_slots: range.len() - truth.blocks.len() as u64,
        expected: truth.expected.len() as u64,
        delivered: delivered
            .values()
            .filter(|slot| range.contains(**slot))
            .count() as u64,
        matched: d.matched,
        missing: d
            .missing
            .iter()
            .map(|(s, l)| MissingTx {
                signature: s.to_string(),
                slot: l.slot,
                position: positions.get(s).copied(),
                cause: None,
                incident: None,
            })
            .collect(),
        repaired: Vec::new(),
        orphaned: d.orphaned.iter().map(tx).collect(),
        landed_elsewhere: d
            .landed_elsewhere
            .iter()
            .map(|(s, delivered_slot, l)| Moved {
                signature: s.to_string(),
                delivered_slot: *delivered_slot,
                landed_slot: l.slot,
            })
            .collect(),
        unexplained: d.unexplained.iter().map(tx).collect(),
        spot_checks,
        verdict,
        per_slot,
        rpc,
        elapsed_ms,
    }
}

pub struct Verifier {
    rpc: Rpc,
    addresses: Vec<String>,
    include_failed: bool,
    options: Options,
}

impl Verifier {
    pub fn new(rpc_url: &str, api_key: &str, addresses: Vec<String>, include_failed: bool) -> Self {
        Self {
            rpc: Rpc::new(rpc_url, api_key),
            addresses,
            include_failed,
            options: Options::default(),
        }
    }

    /// A verifier that applies the same filter as a Gapless stream.
    pub fn for_stream(config: &gapless::Config, rpc_url: &str) -> Result<Self, Error> {
        if !config.account_required.is_empty() || !config.account_exclude.is_empty() {
            return Err(Error::Unsupported(
                "verification supports account_include filters only".into(),
            ));
        }
        Ok(Self::new(
            rpc_url,
            &config.api_key,
            config.account_include.clone(),
            config.include_failed,
        ))
    }

    pub fn with_options(mut self, options: Options) -> Self {
        self.options = options;
        self
    }

    pub fn rpc(&self) -> &Rpc {
        &self.rpc
    }

    pub async fn finalized_tip(&self) -> Result<u64, Error> {
        self.rpc.slot("finalized").await
    }

    /// Wait until `slot` is finalized. Returns the finalized tip.
    pub async fn wait_until_finalized(&self, slot: u64, timeout: Duration) -> Result<u64, Error> {
        let started = Instant::now();
        loop {
            let finalized = self.finalized_tip().await?;
            if finalized >= slot {
                return Ok(finalized);
            }
            if started.elapsed() > timeout {
                return Err(Error::NotFinalized {
                    needed: slot,
                    finalized,
                });
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    /// Compare what a stream delivered with what it should have, for `range`. `delivered` maps
    /// every signature the stream delivered (in any slot) to the slot it came in.
    pub async fn verify(
        &self,
        range: SlotRange,
        delivered: &HashMap<Signature, u64>,
    ) -> Result<Report, Error> {
        let started = Instant::now();
        let usage_before = self.rpc.stats();
        let finalized_tip = self.finalized_tip().await?;
        if range.last > finalized_tip {
            return Err(Error::NotFinalized {
                needed: range.last,
                finalized: finalized_tip,
            });
        }
        let (expected, blocks) = tokio::try_join!(
            history::expected(
                &self.rpc,
                &self.addresses,
                range,
                self.include_failed,
                self.options.chunk_slots,
                self.options.concurrency,
            ),
            self.rpc.finalized_blocks(range.first, range.last),
        )?;
        let canonical: HashSet<u64> = blocks.iter().copied().collect();
        let d = diff::diff(range, &expected, &canonical, delivered);

        let slots = self.spot_slots(&blocks, &d);
        let checked = futures::future::try_join_all(
            slots.iter().map(|&slot| self.check_slot(slot, &expected)),
        )
        .await?;
        let positions: HashMap<Signature, u64> = checked
            .iter()
            .flat_map(|(_, p)| p.iter().map(|(s, i)| (*s, *i)))
            .collect();
        let spot_checks: Vec<SpotCheck> = checked.into_iter().map(|(check, _)| check).collect();

        Ok(build_report(
            range,
            GroundTruth {
                source: "getTransactionsForAddress",
                addresses: &self.addresses,
                finalized_tip,
                blocks: &blocks,
                expected: &expected,
            },
            delivered,
            spot_checks,
            &positions,
            self.rpc.stats().since(usage_before),
            started.elapsed().as_millis() as u64,
        ))
    }

    /// Check the expected-set source itself: full blocks versus `getTransactionsForAddress`
    /// for every slot in `range`.
    pub async fn cross_check(&self, range: SlotRange) -> Result<Vec<SpotCheck>, Error> {
        let expected = history::expected(
            &self.rpc,
            &self.addresses,
            range,
            self.include_failed,
            self.options.chunk_slots,
            self.options.concurrency,
        )
        .await?;
        let slots: Vec<u64> = (range.first..=range.last).collect();
        let checked = futures::future::try_join_all(
            slots.iter().map(|&slot| self.check_slot(slot, &expected)),
        )
        .await?;
        Ok(checked.into_iter().map(|(check, _)| check).collect())
    }

    /// Fetch missing transactions with `getTransaction`, the way a consumer would backfill them:
    /// up to [`REPAIR_MAX`], a few at a time. If every missing one turns up, the verdict becomes
    /// [`Verdict::Repaired`]; a larger loss stays [`Verdict::Incomplete`] and is left to replay.
    pub async fn repair(&self, report: &mut Report) -> Result<(), Error> {
        use futures::{StreamExt, TryStreamExt};
        let signatures: Vec<String> = report
            .missing
            .iter()
            .take(REPAIR_MAX)
            .map(|m| m.signature.clone())
            .collect();
        let rpc = &self.rpc;
        let fetched: Vec<_> = futures::stream::iter(signatures)
            .map(|signature| async move { rpc.transaction(&signature).await })
            .buffered(self.options.concurrency.max(1) * 2)
            .try_collect()
            .await?;
        report.repaired = report
            .missing
            .iter()
            .zip(fetched)
            .filter_map(|(m, tx)| {
                let tx = tx?;
                Some(Repaired {
                    signature: m.signature.clone(),
                    slot: tx["slot"].as_u64().unwrap_or(m.slot),
                    block_time: tx["blockTime"].as_i64(),
                    fee_payer: tx["transaction"]["message"]["accountKeys"][0]
                        .as_str()
                        .map(str::to_owned),
                    transaction: tx,
                })
            })
            .collect();
        if !report.missing.is_empty() && report.repaired.len() == report.missing.len() {
            report.verdict = Verdict::Repaired {
                repaired: report.repaired.len() as u64,
            };
        }
        Ok(())
    }

    /// Evenly spaced block slots, plus slots with differences to explain.
    fn spot_slots(&self, blocks: &[u64], d: &Diff) -> Vec<u64> {
        let mut slots = Vec::new();
        let k = self.options.spot_checks.min(blocks.len());
        for i in 0..k {
            let at = if k == 1 {
                blocks.len() / 2
            } else {
                i * (blocks.len() - 1) / (k - 1)
            };
            slots.push(blocks[at]);
        }
        let differing = d
            .missing
            .iter()
            .map(|(_, l)| l.slot)
            .chain(d.unexplained.iter().map(|(_, slot)| *slot));
        for slot in differing {
            if slots.len() >= k + self.options.explain_checks {
                break;
            }
            if !slots.contains(&slot) {
                slots.push(slot);
            }
        }
        slots
    }

    /// Check one slot against its full block. Also returns the block position of every match.
    async fn check_slot(
        &self,
        slot: u64,
        expected: &HashMap<Signature, Landed>,
    ) -> Result<(SpotCheck, HashMap<Signature, u64>), Error> {
        let from_history: HashSet<Signature> = expected
            .iter()
            .filter(|(_, l)| l.slot == slot)
            .map(|(s, _)| *s)
            .collect();
        let Some(block) = self.rpc.block(slot).await? else {
            let check = SpotCheck {
                slot,
                has_block: false,
                from_block: 0,
                from_history: from_history.len() as u32,
                via_lookup_table: 0,
                agree: from_history.is_empty(),
                only_in_block: Vec::new(),
                only_in_history: from_history.iter().map(ToString::to_string).collect(),
            };
            return Ok((check, HashMap::new()));
        };
        let matches = block::matching(&block, &self.addresses, self.include_failed)?;
        let from_block: HashSet<Signature> = matches.iter().map(|m| m.signature).collect();
        let mut only_in_block: Vec<String> = from_block
            .difference(&from_history)
            .map(ToString::to_string)
            .collect();
        let mut only_in_history: Vec<String> = from_history
            .difference(&from_block)
            .map(ToString::to_string)
            .collect();
        only_in_block.sort();
        only_in_history.sort();
        let positions = matches.iter().map(|m| (m.signature, m.index)).collect();
        let check = SpotCheck {
            slot,
            has_block: true,
            from_block: from_block.len() as u32,
            from_history: from_history.len() as u32,
            via_lookup_table: matches.iter().filter(|m| m.via_lookup_table).count() as u32,
            agree: only_in_block.is_empty() && only_in_history.is_empty(),
            only_in_block,
            only_in_history,
        };
        Ok((check, positions))
    }
}
