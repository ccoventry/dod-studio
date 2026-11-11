//! The text the repair (#15) shows, for the `--repair` command line and for
//! Studio's Demo Auditor, which passes these through. One place per line, per
//! CLAUDE.md's strings rule (#237).

use std::fmt::Display;

use analysis::integrity::clock;
use analysis::repair::{Hole, Refusal, RepairReport};

pub fn cannot_read(path: &str, err: impl Display) -> String {
    format!("{path}: cannot read the file: {err}")
}

pub fn cannot_repair(path: &str, err: impl Display) -> String {
    format!("{path}: cannot be repaired: {err}")
}

pub fn cannot_write(path: &str, err: impl Display) -> String {
    format!("cannot write {path}: {err}")
}

pub fn no_free_name(original: &str) -> String {
    format!("{original}: every name for its repair is taken")
}

pub fn not_damaged(path: &str) -> String {
    format!("{path}: not damaged, nothing written")
}

pub fn repaired(path: &str, out: &str) -> String {
    format!("{path}: repaired, written to {out}")
}

pub fn refused(path: &str) -> String {
    format!("{path}: repaired, but the result failed a check, so nothing was written:")
}

/// One hole, as "0:39 to 1:09 into the recording, 30 s lost".
pub fn hole_line(number: usize, hole: &Hole) -> String {
    format!(
        "  gap {number}: {} to {} into the recording, {:.0} s lost",
        clock(hole.before_secs),
        clock(hole.after_secs),
        hole.lost_secs
    )
}

/// What was kept and what the checks found, in one line.
pub fn summary_line(report: &RepairReport) -> String {
    format!(
        "  kept {:.1}% of the recording's bytes, {:.0} s lost in all; checks: reads to the end, no entity update thrown away, every entity count agrees",
        report.percent_kept(),
        report.seconds_lost()
    )
}

/// After the last hole the recording never resumes.
pub const ENDS_AT_LAST_GAP: &str =
    "  the recording does not resume after the last gap, so the repair ends there";

/// Why a repair was not written, in words.
pub fn refusal(reason: &Refusal) -> String {
    match reason {
        Refusal::DoesNotParse { reason } => format!("the repaired demo does not read: {reason}"),
        Refusal::FramesLost { kept, parsed } => {
            format!("only {parsed} of {kept} frames read back")
        }
        Refusal::PacketsDiscarded { count } => {
            format!("the game would throw away {count} entity updates")
        }
        Refusal::CountsDisagree { count } => {
            format!("{count} entity counts disagree with what the game holds")
        }
        Refusal::JoinWithoutSnapshot { join } => {
            format!(
                "gap {} has no entity update after it to rebuild the world from",
                join + 1
            )
        }
    }
}

pub fn repair_totals(demos: usize, repaired: usize, not_damaged: usize, failed: usize) -> String {
    format!("{demos} demos: {repaired} repaired, {not_damaged} not damaged, {failed} not repaired")
}

pub fn no_such_path(path: &str) -> String {
    format!("No such file or folder: {path}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hole_reads_as_minutes_and_seconds_with_the_seconds_lost() {
        let hole = Hole {
            before_secs: 39.2,
            after_secs: 68.6,
            lost_secs: 29.4,
            bytes_skipped: 1,
        };
        assert_eq!(
            hole_line(1, &hole),
            "  gap 1: 0:39 to 1:08 into the recording, 29 s lost"
        );
    }
}
