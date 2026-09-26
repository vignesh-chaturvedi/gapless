//! The slot tape: what happened to each recent slot. Received live or replayed, complete, how
//! many transactions, and what verification found.

use std::collections::BTreeMap;

use gapless_verify::Report;

use crate::dto::{SlotCell, now_ms};

const KEEP: usize = 3_000;

#[derive(Default)]
pub struct Ledger {
    cells: BTreeMap<u64, SlotCell>,
}

impl Ledger {
    fn cell(&mut self, slot: u64) -> &mut SlotCell {
        self.cells.entry(slot).or_insert_with(|| SlotCell {
            slot,
            origin: "live",
            complete: false,
            txs: 0,
            verified: None,
            missing: 0,
            incident: None,
            at: now_ms(),
        })
    }

    pub fn on_transaction(&mut self, slot: u64, incident: Option<u64>) {
        let cell = self.cell(slot);
        cell.txs += 1;
        if incident.is_some() {
            cell.origin = "replay";
            cell.incident = incident;
        }
    }

    /// A slot completed. Returns its cell for the feed.
    pub fn on_complete(&mut self, slot: u64, incident: Option<u64>) -> SlotCell {
        let cell = self.cell(slot);
        cell.complete = true;
        if incident.is_some() {
            cell.origin = "replay";
            cell.incident = incident;
        }
        cell.at = now_ms();
        let out = cell.clone();
        while self.cells.len() > KEEP {
            self.cells.pop_first();
        }
        out
    }

    /// Record a verification report. Returns the changed cells.
    pub fn on_verified(&mut self, report: &Report) -> Vec<SlotCell> {
        let mut missing: BTreeMap<u64, u32> = BTreeMap::new();
        for m in &report.missing {
            *missing.entry(m.slot).or_default() += 1;
        }
        let repaired = !report.repaired.is_empty() && report.repaired.len() == report.missing.len();
        let mut changed = Vec::new();
        for row in &report.per_slot {
            let Some(cell) = self.cells.get_mut(&row.slot) else {
                continue;
            };
            let n = missing.get(&row.slot).copied().unwrap_or(0);
            cell.missing = n;
            cell.verified = Some(match (n, repaired) {
                (0, _) => "ok",
                (_, true) => "repaired",
                _ => "missing",
            });
            changed.push(cell.clone());
        }
        changed
    }

    pub fn recent(&self, limit: usize) -> Vec<SlotCell> {
        let skip = self.cells.len().saturating_sub(limit);
        self.cells.values().skip(skip).cloned().collect()
    }
}
