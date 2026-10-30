//! The set of entities each snapshot holds, carried forward the way the engine
//! does, for the probes that rewrite packet entities (`packet_entity_trim`,
//! `entity_count_fix`).
//!
//! A packet's `entity_count` is the size of the snapshot that results from
//! applying it, not the number of entities it lists. A full packet's snapshot is
//! the entities it lists; a delta packet starts from the snapshot its
//! `delta_sequence` names, adds or updates what it lists and drops what it marks
//! removed. Snapshots are kept by the low byte of the frame's
//! `incoming_sequence`, the byte `delta_sequence` holds.
//!
//! Measured exact: on `wsod25_grp3_h1_hltv.dem` the carried size equals the
//! declared count in all 61,753 packets.

use std::collections::{BTreeSet, HashMap};

#[derive(Default)]
pub struct Carry {
    ring: HashMap<u8, BTreeSet<u16>>,
    last: BTreeSet<u16>,
    /// Delta packets whose base snapshot was never seen; they start from the
    /// latest snapshot instead.
    pub missing_base: usize,
}

impl Carry {
    pub fn full(&mut self, seq: i32, listed: impl Iterator<Item = u16>) -> &BTreeSet<u16> {
        self.store(seq, listed.collect())
    }

    /// `entries` are (entity index, removed) in the order the packet lists them.
    pub fn delta(&mut self, seq: i32, from: u8, entries: &[(u16, bool)]) -> &BTreeSet<u16> {
        let mut set = match self.ring.get(&from) {
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
        self.store(seq, set)
    }

    fn store(&mut self, seq: i32, set: BTreeSet<u16>) -> &BTreeSet<u16> {
        self.ring.insert((seq & 0xFF) as u8, set.clone());
        self.last = set;
        &self.last
    }
}
