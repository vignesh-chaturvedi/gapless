use std::fmt::Write;

use gapless::SlotRange;
use serde::Serialize;
use serde_json::json;

use crate::rpc::RpcStats;

#[derive(Clone, Debug, Serialize)]
pub struct TxRef {
    pub signature: String,
    pub slot: u64,
}

/// An expected transaction the stream never delivered.
#[derive(Clone, Debug, Serialize)]
pub struct MissingTx {
    pub signature: String,
    pub slot: u64,
    /// Position in the block, when a block check covered its slot.
    pub position: Option<u64>,
    /// What most likely caused it, when known (for example `replay_handoff`).
    pub cause: Option<String>,
    pub incident: Option<u64>,
}

/// A missing transaction fetched from RPC after the fact.
#[derive(Clone, Debug, Serialize)]
pub struct Repaired {
    pub signature: String,
    pub slot: u64,
    pub block_time: Option<i64>,
    pub fee_payer: Option<String>,
    /// The full transaction as `getTransaction` returned it.
    #[serde(skip)]
    pub transaction: serde_json::Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct Moved {
    pub signature: String,
    pub delivered_slot: u64,
    pub landed_slot: u64,
}

/// `getBlock` versus `getTransactionsForAddress` for one slot: two independent expected sets.
#[derive(Clone, Debug, Serialize)]
pub struct SpotCheck {
    pub slot: u64,
    pub has_block: bool,
    pub from_block: u32,
    pub from_history: u32,
    /// Matches in the block that only reference the address through a lookup table.
    pub via_lookup_table: u32,
    pub agree: bool,
    pub only_in_block: Vec<String>,
    pub only_in_history: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SlotRow {
    pub slot: u64,
    pub has_block: bool,
    pub expected: u32,
    pub delivered: u32,
    pub matched: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Verdict {
    /// Every expected transaction was delivered, and every delivered one is accounted for.
    Complete,
    Incomplete {
        missing: u64,
    },
    /// The stream missed some transactions, and every one was fetched from RPC afterwards.
    Repaired {
        repaired: u64,
    },
    /// Something doesn't add up, and the report says what.
    Inconclusive {
        reason: String,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub range: SlotRange,
    pub finalized_tip: u64,
    pub source: &'static str,
    pub addresses: Vec<String>,
    pub slots_with_blocks: u64,
    pub skipped_slots: u64,
    pub expected: u64,
    /// Delivered transactions whose slot is in the range.
    pub delivered: u64,
    pub matched: u64,
    pub missing: Vec<MissingTx>,
    /// Missing transactions fetched from RPC by [`crate::Verifier::repair`].
    pub repaired: Vec<Repaired>,
    pub orphaned: Vec<TxRef>,
    pub landed_elsewhere: Vec<Moved>,
    pub unexplained: Vec<TxRef>,
    pub spot_checks: Vec<SpotCheck>,
    pub verdict: Verdict,
    pub per_slot: Vec<SlotRow>,
    pub rpc: RpcStats,
    pub elapsed_ms: u64,
}

impl Report {
    /// Everything except timing and RPC usage, for checking that two runs agree.
    pub fn fingerprint(&self) -> String {
        let checks: Vec<_> = self
            .spot_checks
            .iter()
            .map(|c| (c.slot, c.from_block, c.from_history, c.agree))
            .collect();
        json!({
            "range": self.range,
            "expected": self.expected,
            "delivered": self.delivered,
            "matched": self.matched,
            "missing": self.missing,
            "orphaned": self.orphaned,
            "landed_elsewhere": self.landed_elsewhere,
            "unexplained": self.unexplained,
            "spot_checks": checks,
            "verdict": self.verdict,
            "per_slot": self.per_slot,
        })
        .to_string()
    }

    /// Label missing transactions that fall just after an incident's replay target: where Solami
    /// switches a resumed stream from replayed history to the live feed. `windows` pairs an
    /// incident id with the slots to blame on its handoff.
    pub fn explain_handoffs(&mut self, windows: &[(u64, SlotRange)]) {
        for m in &mut self.missing {
            if let Some((incident, _)) = windows.iter().find(|(_, w)| w.contains(m.slot)) {
                m.cause = Some("replay_handoff".into());
                m.incident = Some(*incident);
            }
        }
    }

    /// A few lines for a terminal.
    pub fn summary(&self) -> String {
        let mut s = String::new();
        let agree = self.spot_checks.iter().filter(|c| c.agree).count();
        let lookups: u32 = self.spot_checks.iter().map(|c| c.via_lookup_table).sum();
        let _ = writeln!(
            s,
            "slots {}..={} ({} slots: {} with blocks, {} skipped)",
            self.range.first,
            self.range.last,
            self.range.len(),
            self.slots_with_blocks,
            self.skipped_slots
        );
        let _ = writeln!(s, "  expected ({})  {}", self.source, self.expected);
        let _ = writeln!(
            s,
            "  delivered by the stream               {}",
            self.delivered
        );
        let _ = writeln!(
            s,
            "  matched                               {}",
            self.matched
        );
        let _ = writeln!(
            s,
            "  missing                               {}",
            self.missing.len()
        );
        for m in self.missing.iter().take(8) {
            let _ = writeln!(
                s,
                "    slot {} position {}  {}{}",
                m.slot,
                m.position.map_or("?".into(), |p| p.to_string()),
                m.signature,
                m.cause
                    .as_ref()
                    .map(|c| format!("  [{c}]"))
                    .unwrap_or_default()
            );
        }
        if !self.repaired.is_empty() {
            let _ = writeln!(
                s,
                "  repaired via getTransaction           {}",
                self.repaired.len()
            );
        }
        let _ = writeln!(
            s,
            "  orphaned (delivered from dead forks)  {}",
            self.orphaned.len()
        );
        let _ = writeln!(
            s,
            "  landed in a different slot            {}",
            self.landed_elsewhere.len()
        );
        let _ = writeln!(
            s,
            "  unexplained                           {}",
            self.unexplained.len()
        );
        let _ = writeln!(
            s,
            "  getBlock spot checks                  {agree}/{} agree ({lookups} matches via lookup tables)",
            self.spot_checks.len()
        );
        let _ = writeln!(
            s,
            "  rpc                                   {} calls, {:.1} MB, {} retries, {} ms",
            self.rpc.calls,
            self.rpc.bytes as f64 / 1e6,
            self.rpc.retries,
            self.elapsed_ms
        );
        let verdict = match &self.verdict {
            Verdict::Complete => "COMPLETE".to_owned(),
            Verdict::Incomplete { missing } => format!("INCOMPLETE: {missing} missing"),
            Verdict::Repaired { repaired } => {
                format!("REPAIRED: the stream missed {repaired}, all fetched from RPC")
            }
            Verdict::Inconclusive { reason } => format!("INCONCLUSIVE: {reason}"),
        };
        let _ = writeln!(s, "  verdict: {verdict}");
        s
    }
}
