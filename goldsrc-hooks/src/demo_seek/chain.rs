//! A seek made while another is still on its way, and the kill feed after any
//! stepped seek (#1392's review).
//!
//! A rewind (`rewind.rs`) or a forward stepped seek moves the clock 5 s a
//! frame, so for a second or two the clock is nowhere near where the seek is
//! heading. Two quick presses of the in-game Playback tab's -10
//! (`ds_demo_seek_by -10`) at 23:00 in a POV demo found the clock near 10:00
//! on the second press and went to 9:50. So a relative seek counts from the
//! target of the seek in progress ([`counted_from`]): the second -10 goes to
//! 22:40, as two presses with the demo playing would.
//!
//! The steps replay every `DeathMsg` on the way within a second or two of real
//! time, forward ones as much as a rewind's: a forward stepped seek on
//! `solifotw-pb2` (PRE, 878.7 -> 978.6 s) landed with three kills from before
//! the last notice lifetime still in the feed. So every stepped seek empties
//! the feed one notice lifetime before its target, as a rewind does
//! ([`stepped_seek`]), and old kills never show after a jump. A seek that
//! replaces one on its way steps even when it is short ([`steps`]), so the
//! kills the old one's steps replayed go too. A rewind whose start frame the
//! player has not sent yet passes that wait on, or the new seek's first step
//! would be a backward jump that replays nothing.
//!
//! A `ds_demo_seek_skip_between` seek still lands at once: it skips what lies
//! between on purpose.

use super::Pending;
use super::rewind::feed_clear_at;

/// A seek still on its way, as the seek replacing it sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct InFlight {
    /// Where it is heading.
    pub(super) target: f64,
    /// The rewind's `m_LastFrameTime` before it, until its start frame is sent.
    pub(super) rewind_from: Option<f64>,
}

/// The seek in `pending`, unless there is none or it has landed: the clock
/// is at a landed seek's target already.
pub(super) fn in_flight(pending: Option<&Pending>) -> Option<InFlight> {
    let seek = pending.filter(|seek| !seek.landed)?;
    Some(InFlight {
        target: seek.target,
        rewind_from: seek.rewind_from,
    })
}

/// The clock a relative seek counts from: the target of a seek on its way,
/// else `now`.
pub(super) fn counted_from(now: f64, chain: Option<InFlight>) -> f64 {
    chain.map_or(now, |seek| seek.target)
}

/// Whether a seek from `now` to `to` that does not go back steps: one longer
/// than a step, or any that replaces a seek on its way, so the kill feed is
/// emptied of what that one's steps replayed.
pub(super) fn steps(now: f64, to: f64, skip_between: bool, chain: Option<InFlight>) -> bool {
    super::stepped(now, to, skip_between) || (!skip_between && chain.is_some())
}

/// A forward stepped seek to `to` from `now`: it empties the kill feed one
/// notice lifetime (`lifetime`) before `to`, and waits for the start frame of
/// a rewind it replaces.
pub(super) fn stepped_seek(
    to: f64,
    then: Option<String>,
    now: f64,
    chain: Option<InFlight>,
    lifetime: Option<f64>,
) -> Pending {
    let mut seek = Pending::new(to, then, now);
    seek.feed_at = Some(feed_clear_at(to, lifetime));
    seek.rewind_from = chain.and_then(|old| old.rewind_from);
    seek
}

/// The words a seek's landing line adds when it emptied the kill feed.
pub(super) fn feed_note(seek: &Pending) -> &'static str {
    if seek.feed_at.is_some() {
        ", the kill feed emptied on the way"
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::super::rewind::{rewind_step, rewinds};
    use super::super::{Step, landing};
    use super::*;

    /// A rewind to `target` from `base`, the way `player::seek` starts one.
    fn rewind(target: f64, base: f64, from: Option<f64>) -> Pending {
        let mut seek = Pending::new(target, None, base);
        seek.rewind_from = from;
        seek.feed_at = Some(feed_clear_at(target, Some(10.0)));
        seek
    }

    /// The target a seek by `by` gets with `pending` on its way and the
    /// clock at `now`, in a demo running 4.1 to 2000 s.
    fn relative_target(now: f64, pending: Option<&Pending>, by: f64) -> f64 {
        landing(counted_from(now, in_flight(pending)), by, true, 4.1, 2000.0)
    }

    /// The review's case: -10 twice at 23:00 in a POV demo. The first
    /// rewinds toward 22:50 from the recording's start; the second, with
    /// the steps near 10:00, goes to 22:40, not 9:50.
    #[test]
    fn a_second_relative_seek_counts_from_the_rewinds_target() {
        let first = rewind(1370.0, 566.49, None);
        let to = relative_target(600.0, Some(&first), -10.0);
        assert!((to - 1360.0).abs() < 1e-9, "{to}");
        assert!(!rewinds(600.0, to, false), "ahead of the step clock");
        let chain = in_flight(Some(&first));
        assert!(steps(600.0, to, false, chain));
        // A third press: from 22:40, the second seek's own target.
        let second = stepped_seek(to, None, 600.0, chain, Some(10.0));
        let third = relative_target(700.0, Some(&second), -10.0);
        assert!((third - 1350.0).abs() < 1e-9, "{third}");
    }

    /// An HLTV demo rewinds from the clock's start, and a forward stepped
    /// seek steps too: either way a relative seek counts from the target,
    /// back or forward.
    #[test]
    fn relative_seeks_chain_from_the_target_both_ways() {
        let hltv_rewind = rewind(900.0, 30.0, Some(1000.0));
        let back = relative_target(35.0, Some(&hltv_rewind), -10.0);
        assert!((back - 890.0).abs() < 1e-9, "{back}");
        let forward = stepped_seek(900.0, None, 300.0, None, Some(10.0));
        let on = relative_target(400.0, Some(&forward), 60.0);
        assert!((on - 960.0).abs() < 1e-9, "{on}");
        let back = relative_target(400.0, Some(&forward), -10.0);
        assert!((back - 890.0).abs() < 1e-9, "{back}");
        // Kept inside the demo all the same.
        assert_eq!(relative_target(400.0, Some(&forward), 5000.0), 2000.0);
    }

    /// Nothing on its way, or a seek that has landed: from the clock.
    #[test]
    fn with_no_seek_on_its_way_a_relative_seek_counts_from_the_clock() {
        assert_eq!(relative_target(1380.0, None, -10.0), 1370.0);
        let mut landed = Pending::new(1370.0, Some("spec_mode 4".into()), 1370.0);
        landed.landed = true;
        assert_eq!(in_flight(Some(&landed)), None);
        assert_eq!(relative_target(1370.02, Some(&landed), -10.0), 1360.02);
    }

    /// Review's Next twice, or any forward stepped seek: it empties the feed
    /// one notice lifetime before its own target, then steps on.
    #[test]
    fn a_stepped_seek_empties_the_kill_feed_one_notice_lifetime_before_landing() {
        let chain = in_flight(Some(&rewind(1370.0, 566.49, None)));
        let next = stepped_seek(1500.0, None, 700.0, chain, Some(10.0));
        assert_eq!(next.feed_at, Some(1490.0));
        assert_eq!(rewind_step(700.0, 1500.0, next.feed_at), Step::To(705.0));
        assert_eq!(rewind_step(1490.0, 1500.0, next.feed_at), Step::ClearFeed);
        assert!(!feed_note(&next).is_empty());
        // The PRE pb2 forward seek that kept three old kills.
        let alone = stepped_seek(978.61, None, 878.7, None, Some(10.0));
        assert!((alone.feed_at.unwrap() - 968.61).abs() < 1e-9);
        assert_eq!(alone.rewind_from, None);
    }

    /// A short hop past the step clock steps when it replaces a seek on its
    /// way: past the clear point already, it empties the feed at once, then
    /// lands. With nothing on its way it lands at once, as it always has.
    #[test]
    fn a_short_seek_that_replaces_another_still_empties_the_feed() {
        let chain = in_flight(Some(&rewind(1370.0, 566.49, None)));
        assert!(steps(1368.0, 1371.0, false, chain));
        assert!(!steps(1368.0, 1371.0, false, None));
        let hop = stepped_seek(1371.0, None, 1368.0, chain, Some(10.0));
        assert_eq!(rewind_step(1368.0, 1371.0, hop.feed_at), Step::ClearFeed);
        assert_eq!(rewind_step(1368.0, 1371.0, None), Step::Land(1371.0));
    }

    /// A rewind whose start frame is not out yet passes that wait on.
    #[test]
    fn a_seek_that_replaces_a_rewind_waits_for_its_start_frame() {
        let chain = in_flight(Some(&rewind(1370.0, 566.49, Some(1380.0))));
        let next = stepped_seek(1360.0, None, 566.49, chain, Some(10.0));
        assert_eq!(next.rewind_from, Some(1380.0));
    }

    /// A skip lands at once on purpose, chained or not.
    #[test]
    fn a_skip_never_steps() {
        let chain = in_flight(Some(&rewind(1370.0, 566.49, None)));
        assert!(!steps(600.0, 1500.0, true, chain));
        assert!(!steps(600.0, 1500.0, true, None));
        assert!(steps(600.0, 1500.0, false, None));
    }
}
