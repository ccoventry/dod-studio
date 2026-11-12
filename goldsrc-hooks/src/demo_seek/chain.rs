//! A seek made while another is still on its way (#1392's review): what the
//! new one takes over from the one it replaces.
//!
//! A rewind (`rewind.rs`) or a forward stepped seek moves the clock 5 s a
//! frame, so for a second or two the clock is nowhere near where the seek is
//! heading. Two quick presses of the in-game Playback tab's -10
//! (`ds_demo_seek_by -10`) at 23:00 in a POV demo found the clock near 10:00
//! on the second press and went to 9:50. So a relative seek counts from the
//! target of the seek in progress ([`counted_from`]): the second -10 goes to
//! 22:40, as two presses with the demo playing would.
//!
//! The seek that replaces a rewind is usually a forward one from the rewind's
//! step clock (22:40 is ahead of 10:00), which would put the last kills its
//! steps replayed in the feed, however old. So it empties the kill feed the
//! way the rewind would have ([`take_over`]), one notice lifetime before its
//! own target, and a short one steps too so the feed is emptied on the way:
//! old kills never show after a jump. A rewind whose start frame the player
//! has not sent yet passes that wait on too, or the new seek's first step
//! would be a backward jump that replays nothing.
//!
//! A `ds_demo_seek_skip_between` seek still lands at once: it skips what lies
//! between on purpose.

use super::Pending;

/// A seek still on its way, as the seek replacing it sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct InFlight {
    /// Where it is heading.
    pub(super) target: f64,
    /// How long before `target` it empties the kill feed, when it does (a
    /// rewind, or a seek that took over from one).
    pub(super) feed_lead: Option<f64>,
    /// The rewind's `m_LastFrameTime` before it, until its start frame is sent.
    pub(super) rewind_from: Option<f64>,
}

/// The seek in `pending`, unless there is none or it has landed: the clock
/// is at a landed seek's target already.
pub(super) fn in_flight(pending: Option<&Pending>) -> Option<InFlight> {
    let seek = pending.filter(|seek| !seek.landed)?;
    Some(InFlight {
        target: seek.target,
        feed_lead: seek.feed_at.map(|at| seek.target - at),
        rewind_from: seek.rewind_from,
    })
}

/// The clock a relative seek counts from: the target of a seek on its way,
/// else `now`.
pub(super) fn counted_from(now: f64, chain: Option<InFlight>) -> f64 {
    chain.map_or(now, |seek| seek.target)
}

/// Whether a seek that does not go back steps all the same, to empty the
/// kill feed on the way as the seek it replaces would have.
pub(super) fn takes_over_feed_clear(chain: Option<InFlight>, skip_between: bool) -> bool {
    !skip_between && chain.is_some_and(|seek| seek.feed_lead.is_some())
}

/// `seek`, a stepped seek starting now, with what it takes over from the
/// seek it replaces: its kill-feed clear, moved to the new target, and its
/// wait for a rewind's start frame.
pub(super) fn take_over(mut seek: Pending, chain: Option<InFlight>, skip_between: bool) -> Pending {
    if let Some(old) = chain
        && takes_over_feed_clear(chain, skip_between)
    {
        seek.feed_at = old.feed_lead.map(|lead| seek.target - lead);
        seek.rewind_from = old.rewind_from;
    }
    seek
}

/// The words a seek's console lines add, as it starts and as it lands, when
/// it empties the kill feed (a rewind, or a seek that took over from one).
pub(super) fn feed_note(seek: &Pending) -> &'static str {
    if seek.feed_at.is_some() {
        ", the kill feed emptied on the way"
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::super::rewind::{feed_clear_at, rewind_step, rewinds};
    use super::super::{Step, landing, stepped};
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
        assert!(stepped(600.0, to, false));
        // A third press: from 22:40, the second seek's own target.
        let second = take_over(
            Pending::new(to, None, 600.0),
            in_flight(Some(&first)),
            false,
        );
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
        let forward = Pending::new(900.0, None, 300.0);
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

    /// Review's Next twice: the second seek lands ahead of the rewind's step
    /// clock and empties the feed one notice lifetime before its own target.
    #[test]
    fn a_seek_that_replaces_a_rewind_empties_the_kill_feed_as_it_would() {
        let chain = in_flight(Some(&rewind(1370.0, 566.49, None)));
        assert!(takes_over_feed_clear(chain, false));
        let next = take_over(Pending::new(1500.0, None, 700.0), chain, false);
        assert_eq!(next.feed_at, Some(1490.0));
        assert_eq!(rewind_step(700.0, 1500.0, next.feed_at), Step::To(705.0));
        assert_eq!(rewind_step(1490.0, 1500.0, next.feed_at), Step::ClearFeed);
        assert!(!feed_note(&next).is_empty());
        // And the one after that, since this one empties the feed too.
        let again = take_over(
            Pending::new(1600.0, None, 800.0),
            in_flight(Some(&next)),
            false,
        );
        assert_eq!(again.feed_at, Some(1590.0));
    }

    /// A short hop past the step clock steps all the same: past the clear
    /// point already, it empties the feed at once, then lands.
    #[test]
    fn a_short_seek_after_a_rewind_still_empties_the_feed() {
        let chain = in_flight(Some(&rewind(1370.0, 566.49, None)));
        assert!(!stepped(1368.0, 1371.0, false));
        let hop = take_over(Pending::new(1371.0, None, 1368.0), chain, false);
        assert_eq!(rewind_step(1368.0, 1371.0, hop.feed_at), Step::ClearFeed);
        assert_eq!(rewind_step(1368.0, 1371.0, None), Step::Land(1371.0));
    }

    /// A rewind whose start frame is not out yet passes that wait on.
    #[test]
    fn a_seek_that_replaces_a_rewind_waits_for_its_start_frame() {
        let chain = in_flight(Some(&rewind(1370.0, 566.49, Some(1380.0))));
        let next = take_over(Pending::new(1360.0, None, 566.49), chain, false);
        assert_eq!(next.rewind_from, Some(1380.0));
    }

    /// A forward stepped seek never emptied the feed, and a skip lands at
    /// once on purpose: neither passes a clear on.
    #[test]
    fn only_a_seek_that_empties_the_feed_passes_it_on() {
        let forward = in_flight(Some(&Pending::new(900.0, None, 300.0)));
        assert!(!takes_over_feed_clear(forward, false));
        let next = take_over(Pending::new(960.0, None, 400.0), forward, false);
        assert_eq!(next.feed_at, None);
        assert!(feed_note(&next).is_empty());
        let rewinding = in_flight(Some(&rewind(1370.0, 566.49, None)));
        assert!(!takes_over_feed_clear(rewinding, true));
        assert!(!takes_over_feed_clear(None, false));
    }
}
