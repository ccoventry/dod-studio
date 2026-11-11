//! Keeps the HLTV gun down through a sprint gait "flicker" (issue #704).
//!
//! A sprinting player who switches strafe keys (forward to left, left to
//! back) keeps the sprint key held, so his own view keeps the gun lowered.
//! The server picks the gait from his speed, though, and the speed dips for
//! a server frame or two as the direction swings round: the gait leaves
//! `dod_sprint` for 0.02-0.13 s and the copied gun pops up and drops again.
//! A real stop (the sprint key let go) slows him down along the same
//! heading. So when the sprint gait ends while his heading has just turned,
//! the gun stays down a little longer, and comes up then only if the sprint
//! gait has not come back.
//!
//! The heading comes from the watched player's newest snapshots
//! (`curstate.origin` at `curstate.msg_time`), not the drawn position, which
//! lags by the interpolation time. Measured on `wsod25_grp2_h1_hltv` against
//! `wsod25_grp2_h1_m00cat` (1500-1680 and 1830-1980 on the demo clock): all
//! seven flickers turned 3-27 degrees over the last 0.05 s; 39 of 46 real
//! stops turned under 3. Holding 0.15 s from 8 degrees halved the broken-up
//! POV sprint spells on the first stretch and kept the agreement with POV
//! (96.0% to 96.1%, and 96.2% to 96.3%) and the sprint-end timing.
//!
//! Runs on the engine's thread only (`anim_fix::apply`), so the state is a
//! thread-local, not a lock.

use std::cell::RefCell;

/// How far back the two headings are taken: the newer from `WINDOW` ago to
/// now, the older from twice that to `WINDOW`.
const WINDOW: f64 = 0.05;
/// A turn at least this sharp, as the sprint gait ends, holds the gun down.
const TURN_DEGREES: f64 = 8.0;
/// How long it is held. The flickers measured lasted up to 0.13 s.
const HOLD_SECONDS: f64 = 0.15;
/// Snapshots kept: a few tenths of a second at any update rate a server sends.
const KEPT: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Snapshot {
    time: f64,
    x: f32,
    y: f32,
}

#[derive(Debug, Default)]
pub(super) struct SprintTurn {
    snapshots: [Option<Snapshot>; KEPT],
    /// Where the next snapshot goes in the ring.
    next: usize,
    sprinting: bool,
    /// Demo client time the hold runs until, if one is running.
    hold_until: Option<f64>,
}

impl SprintTurn {
    /// One frame's look at the watched player's newest snapshot. The same
    /// `time` twice is the same snapshot and is skipped.
    pub(super) fn snapshot(&mut self, time: f64, x: f32, y: f32) {
        let newest = self.newest();
        if newest.is_some_and(|s| s.time == time) {
            return;
        }
        // A clock that went backwards is a seek or a new demo.
        if newest.is_some_and(|s| time < s.time) {
            self.reset();
        }
        self.snapshots[self.next] = Some(Snapshot { time, x, y });
        self.next = (self.next + 1) % KEPT;
    }

    fn newest(&self) -> Option<Snapshot> {
        self.snapshots[(self.next + KEPT - 1) % KEPT]
    }

    /// The newest snapshot at or before `time`.
    fn at(&self, time: f64) -> Option<Snapshot> {
        self.snapshots
            .iter()
            .flatten()
            .filter(|s| s.time <= time)
            .max_by(|a, b| a.time.total_cmp(&b.time))
            .copied()
    }

    /// How far the heading turned, in degrees, between the movement over
    /// `WINDOW` before the newest snapshot and the `WINDOW` before that.
    /// `None` without enough snapshots, or when he isn't moving.
    fn turn_degrees(&self) -> Option<f64> {
        let now = self.newest()?;
        let mid = self.at(now.time - WINDOW)?;
        let old = self.at(now.time - 2.0 * WINDOW)?;
        if mid.time >= now.time || old.time >= mid.time {
            return None;
        }
        let heading = |a: Snapshot, b: Snapshot| -> Option<f64> {
            let (dx, dy) = (f64::from(b.x - a.x), f64::from(b.y - a.y));
            (dx != 0.0 || dy != 0.0).then(|| dy.atan2(dx).to_degrees())
        };
        let turn = heading(mid, now)? - heading(old, mid)?;
        Some(((turn + 180.0).rem_euclid(360.0) - 180.0).abs())
    }

    /// Whether the gun is down for sprinting: while the gait is
    /// `dod_sprint`, and for [`HOLD_SECONDS`] after it ends on a turn.
    /// Safe to call more than once a frame.
    pub(super) fn lowered(&mut self, sprint_gait: bool, now: f64) -> bool {
        if sprint_gait {
            self.sprinting = true;
            self.hold_until = None;
            return true;
        }
        if std::mem::take(&mut self.sprinting) {
            self.hold_until = self
                .turn_degrees()
                .is_some_and(|turn| turn >= TURN_DEGREES)
                .then_some(now + HOLD_SECONDS);
        }
        match self.hold_until {
            Some(until) if now < until && now >= until - HOLD_SECONDS => true,
            _ => {
                self.hold_until = None;
                false
            }
        }
    }

    /// He let go of sprint (`snapshot_motion::let_go_of_sprint`): the sprint
    /// is over, with no hold, even while the gait still reads sprint.
    pub(super) fn let_go(&mut self) {
        self.sprinting = false;
        self.hold_until = None;
    }

    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
}

thread_local! {
    static STATE: RefCell<SprintTurn> = RefCell::new(SprintTurn::default());
}

/// [`SprintTurn::snapshot`] on the engine thread's state.
pub(super) fn note_snapshot(time: f64, x: f32, y: f32) {
    STATE.with(|s| s.borrow_mut().snapshot(time, x, y));
}

/// [`SprintTurn::lowered`] on the engine thread's state.
pub(super) fn lowered(sprint_gait: bool, now: f64) -> bool {
    STATE.with(|s| s.borrow_mut().lowered(sprint_gait, now))
}

/// [`SprintTurn::let_go`] on the engine thread's state.
pub(super) fn let_go() {
    STATE.with(|s| s.borrow_mut().let_go());
}

/// The camera moved to another player.
pub(super) fn forget() {
    STATE.with(|s| s.borrow_mut().reset());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Snapshots every 0.025 s from `t0`, moving at 300 u/s along `heading`
    /// degrees until `turn_at`, then along `heading + turn`.
    fn run(t0: f64, frames: usize, heading: f64, turn_at: f64, turn: f64) -> SprintTurn {
        let mut s = SprintTurn::default();
        let (mut x, mut y) = (0.0f64, 0.0f64);
        for i in 0..frames {
            let t = t0 + i as f64 * 0.025;
            let h = if t >= turn_at {
                heading + turn
            } else {
                heading
            }
            .to_radians();
            x += 300.0 * 0.025 * h.cos();
            y += 300.0 * 0.025 * h.sin();
            s.snapshot(t, x as f32, y as f32);
        }
        s
    }

    #[test]
    fn a_straight_line_has_no_turn() {
        let s = run(10.0, 10, 30.0, 99.0, 0.0);
        assert!(s.turn_degrees().unwrap() < 0.5);
    }

    #[test]
    fn a_strafe_switch_reads_as_a_turn() {
        let s = run(10.0, 10, 0.0, 10.18, 45.0);
        assert!(s.turn_degrees().unwrap() > TURN_DEGREES);
    }

    #[test]
    fn the_turn_wraps_round_the_back() {
        let s = run(10.0, 10, 175.0, 10.18, 20.0);
        let turn = s.turn_degrees().unwrap();
        assert!((turn - 20.0).abs() < 8.0, "{turn}");
    }

    #[test]
    fn standing_still_has_no_heading() {
        let mut s = SprintTurn::default();
        for i in 0..6 {
            s.snapshot(10.0 + f64::from(i) * 0.025, 5.0, 5.0);
        }
        assert_eq!(s.turn_degrees(), None);
    }

    #[test]
    fn the_gun_follows_the_sprint_gait() {
        let mut s = run(10.0, 10, 0.0, 99.0, 0.0);
        assert!(s.lowered(true, 10.2));
        assert!(s.lowered(true, 10.21));
    }

    #[test]
    fn a_straight_stop_raises_it_at_once() {
        let mut s = run(10.0, 10, 0.0, 99.0, 0.0);
        assert!(s.lowered(true, 10.2));
        assert!(!s.lowered(false, 10.23));
    }

    #[test]
    fn a_turn_holds_it_through_a_flicker() {
        let mut s = run(10.0, 10, 0.0, 10.18, 60.0);
        assert!(s.lowered(true, 10.2));
        assert!(s.lowered(false, 10.23));
        // Twice in one frame gives the same answer.
        assert!(s.lowered(false, 10.23));
        assert!(s.lowered(false, 10.3));
        assert!(s.lowered(true, 10.32));
    }

    #[test]
    fn a_turn_without_the_sprint_coming_back_raises_it_after_the_hold() {
        let mut s = run(10.0, 10, 0.0, 10.18, 60.0);
        assert!(s.lowered(true, 10.2));
        assert!(s.lowered(false, 10.23));
        assert!(!s.lowered(false, 10.23 + HOLD_SECONDS + 0.01));
        // And it stays up.
        assert!(!s.lowered(false, 10.5));
    }

    #[test]
    fn a_clock_going_backwards_drops_the_hold_and_the_history() {
        let mut s = run(10.0, 10, 0.0, 10.18, 60.0);
        assert!(s.lowered(true, 10.2));
        assert!(s.lowered(false, 10.23));
        assert!(!s.lowered(false, 5.0));
        s.snapshot(5.0, 1.0, 1.0);
        assert_eq!(s.turn_degrees(), None);
    }

    #[test]
    fn without_snapshots_a_stop_raises_it_at_once() {
        let mut s = SprintTurn::default();
        assert!(s.lowered(true, 1.0));
        assert!(!s.lowered(false, 1.01));
    }
}
