use std::collections::BTreeSet;

use solami::geyser::{CommitmentLevel, SlotStatus};

/// Complete slots older than this (relative to the highest) are forgotten.
const RETAIN_COMPLETE: u64 = 4_096;

/// Tracks which slots have fully arrived, and where to resume after a disconnect.
///
/// At `processed` commitment, Solami sends a slot's `SlotProcessed` update only after all of
/// that slot's transactions. Phase 0 saw no exceptions across 2,230 slots. So a slot is
/// complete once that update (or `SlotDead`) arrives. Resuming from the lowest incomplete slot
/// never skips the rest of a slot that was cut off mid-stream.
#[derive(Clone, Debug)]
pub struct SlotCursor {
    complete_on: SlotStatus,
    /// Slots that delivered transactions but haven't completed.
    open: BTreeSet<u64>,
    complete: BTreeSet<u64>,
    highest_complete: Option<u64>,
    finalized: Option<u64>,
    late: u64,
    orphaned: u64,
}

impl SlotCursor {
    pub fn new(commitment: CommitmentLevel) -> Self {
        let complete_on = match commitment {
            CommitmentLevel::Processed => SlotStatus::SlotProcessed,
            CommitmentLevel::Confirmed => SlotStatus::SlotConfirmed,
            CommitmentLevel::Finalized => SlotStatus::SlotFinalized,
        };
        Self {
            complete_on,
            open: BTreeSet::new(),
            complete: BTreeSet::new(),
            highest_complete: None,
            finalized: None,
            late: 0,
            orphaned: 0,
        }
    }

    pub fn on_transaction(&mut self, slot: u64) {
        if self.complete.contains(&slot) {
            self.late += 1;
        } else {
            self.open.insert(slot);
        }
    }

    pub fn on_slot(&mut self, slot: u64, status: SlotStatus) {
        if status == self.complete_on || status == SlotStatus::SlotDead {
            self.open.remove(&slot);
            self.complete.insert(slot);
            self.highest_complete = Some(self.highest_complete.map_or(slot, |h| h.max(slot)));
            self.prune();
        }
        if status == SlotStatus::SlotFinalized {
            self.finalized = Some(self.finalized.map_or(slot, |f| f.max(slot)));
            // An open slot at or below the finalized tip never completed, so it was on a fork
            // that died. Don't let it drag the resume point back.
            let stale: Vec<u64> = self.open.range(..=slot).copied().collect();
            self.orphaned += stale.len() as u64;
            for s in stale {
                self.open.remove(&s);
            }
        }
    }

    /// Where a replay should start: the lowest incomplete slot, otherwise the slot after the
    /// highest complete one. `None` until anything has been seen.
    pub fn resume_from(&self) -> Option<u64> {
        self.open
            .first()
            .copied()
            .or(self.highest_complete.map(|s| s + 1))
    }

    pub fn highest_complete(&self) -> Option<u64> {
        self.highest_complete
    }

    pub fn is_complete(&self, slot: u64) -> bool {
        self.complete.contains(&slot)
    }

    /// Transactions that arrived after their slot had completed. Expected to stay at 0.
    pub fn late(&self) -> u64 {
        self.late
    }

    /// Open slots abandoned because the chain finalized past them.
    pub fn orphaned(&self) -> u64 {
        self.orphaned
    }

    fn prune(&mut self) {
        if let Some(high) = self.highest_complete {
            let floor = high.saturating_sub(RETAIN_COMPLETE);
            self.complete = self.complete.split_off(&floor);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cursor() -> SlotCursor {
        SlotCursor::new(CommitmentLevel::Processed)
    }

    #[test]
    fn nothing_seen_means_no_resume_point() {
        assert_eq!(cursor().resume_from(), None);
    }

    #[test]
    fn resumes_after_the_highest_complete_slot() {
        let mut c = cursor();
        c.on_transaction(10);
        c.on_slot(10, SlotStatus::SlotProcessed);
        c.on_slot(11, SlotStatus::SlotProcessed);
        assert_eq!(c.resume_from(), Some(12));
        assert_eq!(c.highest_complete(), Some(11));
    }

    #[test]
    fn resumes_from_a_slot_cut_off_mid_stream() {
        let mut c = cursor();
        c.on_transaction(10);
        c.on_slot(10, SlotStatus::SlotProcessed);
        c.on_transaction(11);
        assert_eq!(
            c.resume_from(),
            Some(11),
            "slot 11 had transactions but never completed"
        );
    }

    #[test]
    fn lowest_open_slot_wins_over_later_complete_ones() {
        let mut c = cursor();
        c.on_transaction(20);
        c.on_transaction(21);
        c.on_slot(21, SlotStatus::SlotProcessed);
        assert_eq!(c.resume_from(), Some(20));
    }

    #[test]
    fn finalization_abandons_forked_open_slots() {
        let mut c = cursor();
        c.on_transaction(30); // on a fork that dies
        c.on_transaction(31);
        c.on_slot(31, SlotStatus::SlotProcessed);
        c.on_slot(31, SlotStatus::SlotFinalized);
        assert_eq!(c.resume_from(), Some(32));
        assert_eq!(c.orphaned(), 1);
    }

    #[test]
    fn dead_slots_complete() {
        let mut c = cursor();
        c.on_transaction(40);
        c.on_slot(40, SlotStatus::SlotDead);
        assert_eq!(c.resume_from(), Some(41));
    }

    #[test]
    fn counts_transactions_after_completion_as_late() {
        let mut c = cursor();
        c.on_slot(50, SlotStatus::SlotProcessed);
        c.on_transaction(50);
        assert_eq!(c.late(), 1);
        assert_eq!(c.resume_from(), Some(51));
    }

    #[test]
    fn confirmed_commitment_completes_on_confirmed() {
        let mut c = SlotCursor::new(CommitmentLevel::Confirmed);
        c.on_transaction(60);
        c.on_slot(60, SlotStatus::SlotProcessed);
        assert_eq!(c.resume_from(), Some(60));
        c.on_slot(60, SlotStatus::SlotConfirmed);
        assert_eq!(c.resume_from(), Some(61));
    }

    #[test]
    fn prunes_old_complete_slots() {
        let mut c = cursor();
        c.on_slot(1, SlotStatus::SlotProcessed);
        c.on_slot(1 + RETAIN_COMPLETE + 10, SlotStatus::SlotProcessed);
        assert!(!c.is_complete(1));
    }
}
