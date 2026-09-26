use std::collections::{BTreeMap, HashMap, HashSet};

use gapless::{Signature, SlotRange};
use serde::Serialize;

use crate::history::Landed;

/// Per-slot counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SlotCount {
    pub expected: u32,
    pub delivered: u32,
    pub matched: u32,
}

/// What the stream delivered, compared with what the chain says it should have.
#[derive(Debug, Default)]
pub struct Diff {
    pub matched: u64,
    /// In the chain and the filter, never delivered.
    pub missing: Vec<(Signature, Landed)>,
    /// Delivered in a slot with no finalized block: a fork that died.
    pub orphaned: Vec<(Signature, u64)>,
    /// Delivered from one slot (a fork) and landed in another. Counted as matched.
    pub landed_elsewhere: Vec<(Signature, u64, Landed)>,
    /// Delivered in a finalized slot of the range, but not in the expected set.
    pub unexplained: Vec<(Signature, u64)>,
    pub per_slot: BTreeMap<u64, SlotCount>,
}

/// `delivered` maps every signature the stream delivered (in any slot) to the slot it came in.
pub fn diff(
    range: SlotRange,
    expected: &HashMap<Signature, Landed>,
    canonical: &HashSet<u64>,
    delivered: &HashMap<Signature, u64>,
) -> Diff {
    let mut d = Diff::default();
    for (signature, landed) in expected {
        let row = d.per_slot.entry(landed.slot).or_default();
        row.expected += 1;
        match delivered.get(signature) {
            Some(&slot) => {
                d.matched += 1;
                row.matched += 1;
                if slot != landed.slot {
                    d.landed_elsewhere.push((*signature, slot, *landed));
                }
            }
            None => d.missing.push((*signature, *landed)),
        }
    }
    for (signature, &slot) in delivered {
        if !range.contains(slot) {
            continue;
        }
        d.per_slot.entry(slot).or_default().delivered += 1;
        if expected.contains_key(signature) {
            continue;
        }
        if canonical.contains(&slot) {
            d.unexplained.push((*signature, slot));
        } else {
            d.orphaned.push((*signature, slot));
        }
    }
    d.missing.sort_by_key(|(s, l)| (l.slot, l.index, s.0));
    d.orphaned.sort_by_key(|(s, slot)| (*slot, s.0));
    d.landed_elsewhere.sort_by_key(|(s, slot, _)| (*slot, s.0));
    d.unexplained.sort_by_key(|(s, slot)| (*slot, s.0));
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(n: u8) -> Signature {
        Signature([n; 64])
    }

    fn landed(slot: u64) -> Landed {
        Landed { slot, index: 0 }
    }

    #[test]
    fn classifies_every_difference() {
        let range = SlotRange::new(100, 110);
        let expected = HashMap::from([
            (sig(1), landed(100)), // delivered where it landed
            (sig(2), landed(101)), // never delivered
            (sig(3), landed(102)), // delivered from fork slot 103, landed in 102
        ]);
        let canonical = HashSet::from([100, 101, 102, 104]);
        let delivered = HashMap::from([
            (sig(1), 100),
            (sig(3), 103),
            (sig(4), 103), // slot 103 never finalized: orphaned
            (sig(5), 104), // finalized slot, not expected: unexplained
            (sig(6), 200), // outside the range: ignored
        ]);
        let d = diff(range, &expected, &canonical, &delivered);
        assert_eq!(d.matched, 2);
        assert_eq!(d.missing, vec![(sig(2), landed(101))]);
        assert_eq!(d.landed_elsewhere, vec![(sig(3), 103, landed(102))]);
        assert_eq!(d.orphaned, vec![(sig(4), 103)]);
        assert_eq!(d.unexplained, vec![(sig(5), 104)]);
        assert_eq!(
            d.per_slot[&101],
            SlotCount {
                expected: 1,
                delivered: 0,
                matched: 0
            }
        );
        assert_eq!(
            d.per_slot[&103],
            SlotCount {
                expected: 0,
                delivered: 2,
                matched: 0
            }
        );
    }

    #[test]
    fn a_perfect_stream_has_no_differences() {
        let range = SlotRange::new(1, 3);
        let expected: HashMap<_, _> = (1..=3).map(|n| (sig(n), landed(n as u64))).collect();
        let delivered: HashMap<_, _> = (1..=3).map(|n| (sig(n), n as u64)).collect();
        let d = diff(range, &expected, &HashSet::from([1, 2, 3]), &delivered);
        assert_eq!(d.matched, 3);
        assert!(d.missing.is_empty() && d.orphaned.is_empty() && d.unexplained.is_empty());
    }
}
