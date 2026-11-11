//! The set of entities each snapshot holds, carried forward the way the engine
//! does (#15). Used by the repair's entity-count step and by the
//! `packet_entity_trim` probe.
//!
//! A packet's `entity_count` is the size of the snapshot that results from
//! applying it, not the number of entities it lists. A full packet's snapshot is
//! the entities it lists; a delta packet starts from the snapshot its
//! `delta_sequence` names, adds or updates what it lists and drops what it marks
//! removed. Like `hw.dll`, it keeps the last 64 snapshots by
//! `incoming_sequence & 63` (`CL_UPDATE_MASK`), and a delta packet whose base is
//! 63 or more sequences back is thrown away (`CL_FlushEntityPacket`), leaving an
//! empty frame (`docs/goldsrc_demo_salvage.md`, "The flush predicate"). After a
//! jump in `incoming_sequence` (a repaired demo's join), a slot can hold a
//! different snapshot than the one with the same low byte: keying by the whole
//! byte disagreed with the engine there and missed `m3_h1-1`'s overrun at 850 s.
//!
//! Measured exact: on `wsod25_grp3_h1_hltv.dem` the carried size equals the
//! declared count in all 61,753 packets.

use std::collections::{BTreeSet, HashMap};

/// `hw.dll`'s `CL_UPDATE_MASK` (`CL_UPDATE_BACKUP` 64 - 1).
const CL_UPDATE_MASK: i32 = 63;

/// The engine's ring of recent snapshots' entity sets.
#[derive(Default)]
pub struct Carry {
    ring: HashMap<i32, BTreeSet<u16>>,
    last: BTreeSet<u16>,
    /// Delta packets the engine throws away (base too old).
    pub flushed: usize,
    /// Delta packets whose base snapshot was never seen; they start from the
    /// latest snapshot instead.
    pub missing_base: usize,
}

impl Carry {
    /// A full packet at sequence `seq` listing `listed`: its snapshot.
    pub fn full(&mut self, seq: i32, listed: impl Iterator<Item = u16>) -> &BTreeSet<u16> {
        self.store(seq, listed.collect())
    }

    /// A delta packet at sequence `seq` from `from`: its snapshot, or `None`
    /// when the engine throws the packet away. `entries` are (entity index,
    /// removed) in the order the packet lists them.
    pub fn delta(&mut self, seq: i32, from: u8, entries: &[(u16, bool)]) -> Option<&BTreeSet<u16>> {
        if (seq - from as i32) & 0xFF >= CL_UPDATE_MASK {
            self.flushed += 1;
            self.store(seq, BTreeSet::new());
            return None;
        }
        let mut set = match self.ring.get(&(from as i32 & CL_UPDATE_MASK)) {
            Some(b) => b.clone(),
            None => {
                self.missing_base += 1;
                self.last.clone()
            }
        };
        for &(i, remove) in entries {
            if remove {
                set.remove(&i);
            } else {
                set.insert(i);
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

    #[test]
    fn a_delta_carries_its_base_snapshot_forward() {
        let mut carry = Carry::default();
        carry.full(10, [1, 2, 3].into_iter());
        let set = carry.delta(11, 10, &[(4, false), (2, true)]).cloned();
        assert_eq!(set, Some(BTreeSet::from([1, 3, 4])));
    }

    #[test]
    fn a_delta_63_sequences_past_its_base_is_thrown_away() {
        let mut carry = Carry::default();
        carry.full(10, [1].into_iter());
        assert!(carry.delta(73, 10, &[]).is_none());
        assert_eq!(carry.flushed, 1);
    }
}
