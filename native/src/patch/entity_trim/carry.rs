//! The set of entities each snapshot holds, carried forward the way the engine
//! does.
//!
//! A packet's `entity_count` is the size of the snapshot that results from
//! applying it, not the number of entities it lists. A full packet's snapshot
//! is the entities it lists; a delta packet starts from the snapshot its
//! `delta_sequence` names, adds or updates what it lists and drops what it
//! marks removed. Like `hw.dll`, this keeps the last 64 snapshots by
//! `incoming_sequence & 63` (`CL_UPDATE_MASK`), and a delta packet whose base
//! is 63 or more sequences back is thrown away (`CL_FlushEntityPacket`),
//! leaving an empty frame (`docs/goldsrc_demo_salvage.md`, "The flush
//! predicate"). After a jump in `incoming_sequence` a slot can hold a
//! different snapshot than the one with the same low byte: keying by the whole
//! byte disagreed with the engine on #15's `m3_h1-1` (PR #1184).
//!
//! Measured exact: on `wsod25_grp3_h1_hltv.dem` the carried size equals the
//! declared count in all 61,753 packets (#207).

use std::collections::{BTreeSet, HashMap};

/// `hw.dll`'s `CL_UPDATE_MASK` (`CL_UPDATE_BACKUP` 64 - 1).
const CL_UPDATE_MASK: i32 = 63;

#[derive(Default)]
pub(super) struct Carry {
    ring: HashMap<i32, BTreeSet<u16>>,
    last: BTreeSet<u16>,
}

impl Carry {
    /// Stores a full packet's snapshot, which is what it lists.
    pub(super) fn full(&mut self, seq: i32, listed: impl Iterator<Item = u16>) -> &BTreeSet<u16> {
        self.store(seq, listed.collect())
    }

    /// Applies a delta packet to the snapshot it names and stores the result.
    /// `entries` are (entity index, removed) in the order the packet lists
    /// them. `None` when the engine throws the packet away (its base is too
    /// old); a base never seen starts from the latest snapshot.
    pub(super) fn delta(
        &mut self,
        seq: i32,
        from: u8,
        entries: &[(u16, bool)],
    ) -> Option<&BTreeSet<u16>> {
        if (seq - from as i32) & 0xFF >= CL_UPDATE_MASK {
            self.store(seq, BTreeSet::new());
            return None;
        }
        let mut set = match self.ring.get(&(from as i32 & CL_UPDATE_MASK)) {
            Some(base) => base.clone(),
            None => self.last.clone(),
        };
        for &(index, removed) in entries {
            if removed {
                set.remove(&index);
            } else {
                set.insert(index);
            }
        }
        Some(self.store(seq, set))
    }

    fn store(&mut self, seq: i32, set: BTreeSet<u16>) -> &BTreeSet<u16> {
        self.ring.insert(seq & CL_UPDATE_MASK, set.clone());
        self.last = set;
        &self.last
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn indices(set: &BTreeSet<u16>) -> Vec<u16> {
        set.iter().copied().collect()
    }

    #[test]
    fn a_delta_adds_and_removes_from_the_snapshot_it_names() {
        let mut carry = Carry::default();
        carry.full(10, [1, 2, 3].into_iter());
        carry.full(11, [7].into_iter());
        let set = carry.delta(12, 10, &[(2, true), (5, false)]).unwrap();
        assert_eq!(indices(set), vec![1, 3, 5]);
    }

    #[test]
    fn a_later_snapshot_in_the_same_slot_replaces_the_earlier() {
        let mut carry = Carry::default();
        carry.full(3, [1].into_iter());
        // Sequence 67 shares slot 3 with sequence 3, and replaces it.
        carry.full(67, [4, 5].into_iter());
        let set = carry.delta(70, 67, &[]).unwrap();
        assert_eq!(indices(set), vec![4, 5]);
    }

    #[test]
    fn a_delta_from_63_or_more_back_is_thrown_away_and_leaves_an_empty_frame() {
        let mut carry = Carry::default();
        carry.full(0, [1, 2].into_iter());
        assert!(carry.delta(63, 0, &[(9, false)]).is_none());
        let next = carry.delta(64, 63, &[]).unwrap();
        assert!(next.is_empty());
    }
}
