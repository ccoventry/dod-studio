//! Text for demos damaged partway (#15) where `native` meets one: a split
//! can't read past the damage, so it points at the repair instead.

/// Why a damaged demo can't be split, and what to do: repair it first, then
/// split the repaired copy. `at_clock` is where the recording breaks, as
/// `m:ss` into it.
pub fn split_needs_repair(at_clock: &str) -> String {
    format!(
        "the demo is damaged {at_clock} into the recording, so it can't be split. \
         Repair it first (its Repair button, or the Demo Auditor's Damaged Demos tab), \
         then split the repaired copy"
    )
}

/// A demo the split could not read, for any other reason.
pub fn split_could_not_read(err: impl std::fmt::Display) -> String {
    format!("could not read the demo: {err}")
}
