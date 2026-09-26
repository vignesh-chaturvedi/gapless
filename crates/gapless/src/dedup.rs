use std::collections::{BTreeMap, HashMap};

use crate::event::Signature;

/// Remembers delivered signatures for `horizon` slots behind the highest slot seen, which covers
/// everything a `from_slot` replay can send again.
#[derive(Debug)]
pub struct DedupWindow {
    seen: HashMap<Signature, u64>,
    by_slot: BTreeMap<u64, Vec<Signature>>,
    horizon: u64,
    highest: u64,
}

impl DedupWindow {
    pub fn new(horizon: u64) -> Self {
        Self {
            seen: HashMap::new(),
            by_slot: BTreeMap::new(),
            horizon,
            highest: 0,
        }
    }

    /// Records the signature and returns `true` the first time it is offered.
    pub fn insert(&mut self, signature: Signature, slot: u64) -> bool {
        if self.seen.contains_key(&signature) {
            return false;
        }
        self.seen.insert(signature, slot);
        self.by_slot.entry(slot).or_default().push(signature);
        if slot > self.highest {
            self.highest = slot;
            self.evict();
        }
        true
    }

    pub fn len(&self) -> usize {
        self.seen.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }

    fn evict(&mut self) {
        let floor = self.highest.saturating_sub(self.horizon);
        while let Some(entry) = self.by_slot.first_entry() {
            if *entry.key() >= floor {
                break;
            }
            for signature in entry.remove() {
                self.seen.remove(&signature);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(n: u8) -> Signature {
        Signature([n; 64])
    }

    #[test]
    fn first_offer_is_new_and_repeats_are_not() {
        let mut d = DedupWindow::new(100);
        assert!(d.insert(sig(1), 10));
        assert!(!d.insert(sig(1), 10));
        assert!(
            !d.insert(sig(1), 11),
            "a repeat in a later slot is still a repeat"
        );
        assert_eq!(d.len(), 1);
    }

    #[test]
    fn forgets_slots_beyond_the_horizon() {
        let mut d = DedupWindow::new(100);
        d.insert(sig(1), 10);
        d.insert(sig(2), 111);
        assert_eq!(
            d.len(),
            1,
            "slot 10 fell out of a 100-slot window ending at 111"
        );
        assert!(d.insert(sig(1), 112));
    }

    #[test]
    fn keeps_slots_inside_the_horizon() {
        let mut d = DedupWindow::new(100);
        d.insert(sig(1), 11);
        d.insert(sig(2), 111);
        assert!(!d.insert(sig(1), 111));
    }
}
