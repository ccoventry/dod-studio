//! Three gun-lowering states the body and gait sequences miss or get wrong,
//! read from the watched player's newest snapshots instead (issue #704).
//!
//! - **A jump with duck held at take-off.** POV lowers the gun for it like
//!   any jump, but in 11 of 21 such jumps (5 players) the HLTV body never
//!   plays `jump`: it stays `crouch_*_aim` on gait `dod_crawl`. The origin
//!   still says it: a jump leaves the ground at about 260 u/s, so two
//!   snapshot intervals in a row climbing at 150-350 u/s while crouched is a
//!   take-off, and the gun stays down until he lands: the first interval
//!   that stops falling, or 0.1 s with no change in height (a ledge caught
//!   on the way up). A stair step is one interval's jump (and faster than
//!   350 u/s over a short interval), and a ladder's dismount climbs as fast as
//!   a jump, so nothing within 0.5 s of `movetype` 5 counts.
//! - **The end of a crawl.** POV raises the gun the frame the move keys come
//!   up; the server keeps gait `prone_forward` up to 0.6 s longer. The origin
//!   stops at once, so a crawl ends after 0.1 s without it moving. A crawl
//!   blocked against a wall still creeps 2-4 u/s, which counts as moving.
//! - **Sprint let go after running off an edge.** The server keeps gait
//!   `dod_sprint` through a fall however he moves, but POV raises the gun the
//!   moment the sprint key comes up, and in 233 of 319 sprinting walk-offs
//!   (5 players) it came up in the air; only 32 held it to the ground. The
//!   speed doesn't show it (letting go doesn't slow him in the air); the body
//!   does: it leaves `sprint_*` in 84% of the released falls (median 0.13 s
//!   after the key) and in none of the held ones. So in a fall that is not a
//!   jump (two snapshot intervals falling at least `FELL`, no `jump` body, no
//!   crouched take-off), a body that is not `sprint_*` means he let go, until
//!   the fall ends. Against the drawn POV gun on those falls' frames the
//!   HLTV gun agreed 64.6% of the time on the gait alone, 70.9% with this
//!   (93.4% on the held falls, from 98.4%); on moving ground frames, where it
//!   rarely applies, 97.1% either way.
//!
//! Scored offline against the drawn POV gun of 5 players
//! (`wsod25_grp2_h1_*` against `wsod25_grp2_h1_hltv`): duck-jump take-offs
//! the HLTV gun missed went from 14 of 80 to 0, landings from 13 of 93 to 2,
//! with no new HLTV-only spells. The crawl end in the `dod_thunder2`
//! side-by-side went from 0.48 s late to 0.07 s.
//!
//! Positions are `curstate.origin` at `curstate.msg_time`, as
//! `sprint_turn` reads them, not the drawn position, which lags by the
//! interpolation time. Every time here is on that snapshot clock, never the
//! client clock: after a `ds_demo_seek_to` the two differ by minutes (an
//! in-game run read every take-off as 300 s level and dropped it at once).
//! HLTV sends every player in every packet, so the snapshot clock keeps
//! moving while he stands still. Runs on the engine's thread only
//! (`anim_fix::apply`), so the state is a thread-local, not a lock.

use std::cell::RefCell;

/// Climbing at least this fast, in u/s, over two snapshot intervals in a
/// row is a take-off. A jump starts at about 260 and loses 800 u/s each
/// second.
const RISE: f32 = 150.0;
/// And no faster: a stair step climbs 18 units in one short interval.
const RISE_MAX: f32 = 350.0;
/// Falling at least this fast, in u/s, is the way down; the first interval
/// after it that falls slower is the landing.
const FELL: f32 = 50.0;
/// No change in height for this long, in seconds, is a landing too: a ledge
/// caught on the way up. The top of a jump stays level for under 0.07 s.
const LEVEL_SECONDS: f64 = 0.1;
/// No jump lasts longer than this; a missed landing ends here.
const AIR_SECONDS: f64 = 2.0;
/// A take-off this soon after a ladder is the climb carrying him off it.
const LADDER_SECONDS: f64 = 0.5;
/// No movement for this long, in seconds, ends a crawl.
const STILL_SECONDS: f64 = 0.1;
/// Smaller changes are not movement. Positions arrive in 1/32 units.
const MOVED: f32 = 0.01;
/// A longer gap between snapshots starts afresh: no speed is read across it.
const GAP_SECONDS: f64 = 0.5;
/// `MOVETYPE_FLY`: on a ladder.
const MOVETYPE_FLY: i32 = 5;
/// Look-ahead (#1154): how far past the newest snapshot the demo file is
/// asked whether the body leaves `sprint_*`. The body shows a let-go a
/// median 0.13 s after the key (p90 0.30 s). Against the drawn POV gun on
/// the walk-off frames of 5 players: 71.5% without, 79.1% at 0.10 s, 83.3%
/// at 0.20 s (held-to-the-ground falls 93.4% -> 92.1%), 83.4% at 0.30 s
/// (held 86.5%).
pub(super) const LET_GO_LOOK_AHEAD_SECONDS: f32 = 0.20;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Snapshot {
    time: f64,
    origin: [f32; 3],
}

#[derive(Debug, Default)]
pub(super) struct SnapshotMotion {
    last: Option<Snapshot>,
    /// Vertical speed over the newest interval.
    climb: Option<f32>,
    /// How many intervals in a row have climbed at jump speed. A take-off is
    /// the moment it reaches two, so a lift that keeps climbing past the
    /// `AIR_SECONDS` limit doesn't take off again at once.
    rising_run: u32,
    /// When the origin last changed, and when its height last did.
    moved_at: Option<f64>,
    height_changed_at: Option<f64>,
    ladder_at: Option<f64>,
    /// When the take-off was seen, while he is in the air after one.
    airborne_since: Option<f64>,
    falling: bool,
    /// How many intervals in a row have fallen at least `FELL`.
    falling_run: u32,
    fall: Fall,
    /// The demo file says the body leaves `sprint_*` within
    /// [`LET_GO_LOOK_AHEAD_SECONDS`] of the newest snapshot.
    leaving_sprint: bool,
}

/// What kind of fall he is in, by the snapshots.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
enum Fall {
    #[default]
    None,
    /// Off an edge, no jump: `let_go` once the body has left `sprint_*`.
    WalkOff { let_go: bool },
    /// The way down from a jump, which lowers the gun on its own.
    Jump,
}

impl SnapshotMotion {
    /// One frame's look at the watched player's newest snapshot, with the
    /// body sequence's label and his movetype. The same `time` twice is the
    /// same snapshot and is skipped.
    pub(super) fn snapshot(&mut self, time: f64, origin: [f32; 3], body: &str, movetype: i32) {
        if let Some(last) = self.last {
            if time == last.time {
                return;
            }
            // A clock that went backwards is a seek or a new demo; a long gap
            // is a seek forward, or frames where nobody was watched.
            if time < last.time || time - last.time > GAP_SECONDS {
                self.reset();
            }
        }
        match self.last {
            Some(last) => {
                let moved = |axis: usize| (origin[axis] - last.origin[axis]).abs() > MOVED;
                if (0..3).any(moved) {
                    self.moved_at = Some(time);
                }
                if moved(2) {
                    self.height_changed_at = Some(time);
                }
                self.climb = Some((origin[2] - last.origin[2]) / (time - last.time) as f32);
            }
            None => {
                self.moved_at = Some(time);
                self.height_changed_at = Some(time);
            }
        }
        self.last = Some(Snapshot { time, origin });
        if movetype == MOVETYPE_FLY {
            self.ladder_at = Some(time);
        }
        self.rising_run = match self.climb {
            Some(climb) if (RISE..=RISE_MAX).contains(&climb) => self.rising_run.saturating_add(1),
            _ => 0,
        };
        match (self.airborne_since, self.climb) {
            (None, Some(_)) => {
                let off_a_ladder = self.ladder_at.is_some_and(|at| time - at < LADDER_SECONDS);
                if body.starts_with("crouch_") && !off_a_ladder && self.rising_run == 2 {
                    self.airborne_since = Some(time);
                    self.falling = false;
                }
            }
            (Some(since), Some(climb)) => {
                let level = self
                    .height_changed_at
                    .is_some_and(|at| time - at >= LEVEL_SECONDS);
                if climb <= -FELL {
                    self.falling = true;
                } else if self.falling || level || time - since > AIR_SECONDS {
                    self.airborne_since = None;
                }
            }
            _ => {}
        }
        self.note_fall(body);
    }

    /// Follows a fall: it starts on the second interval in a row falling at
    /// least `FELL`, and ends on the first that falls slower, the same
    /// landing the jump above uses. A `jump` body or a crouched take-off at
    /// any point makes it a jump's way down for the rest of it, so a jump's
    /// landing frame never reads as a sprint let go.
    fn note_fall(&mut self, body: &str) {
        self.falling_run = match self.climb {
            Some(climb) if climb <= -FELL => self.falling_run.saturating_add(1),
            _ => 0,
        };
        let jumping = body == "jump" || self.airborne_since.is_some();
        let sprint_body = body.starts_with("sprint_") && !self.leaving_sprint;
        self.fall = match self.fall {
            _ if self.falling_run == 0 => Fall::None,
            _ if jumping => Fall::Jump,
            Fall::None if self.falling_run >= 2 => Fall::WalkOff {
                let_go: !sprint_body,
            },
            Fall::WalkOff { let_go } => Fall::WalkOff {
                let_go: let_go || !sprint_body,
            },
            other => other,
        };
    }

    /// In a fall off an edge (not a jump) after the body left `sprint_*`:
    /// he let go of sprint, and the sprint gait the server keeps through the
    /// fall says nothing.
    pub(super) fn let_go_of_sprint(&self) -> bool {
        matches!(self.fall, Fall::WalkOff { let_go: true })
    }

    /// In the air after a crouched take-off the body didn't show as `jump`.
    pub(super) fn in_the_air(&self) -> bool {
        self.airborne_since.is_some()
    }

    /// Whether the origin has stood still for [`STILL_SECONDS`] of
    /// snapshots. False with no snapshot yet, which leaves the crawl to the
    /// gait.
    pub(super) fn still(&self) -> bool {
        match (self.last, self.moved_at) {
            (Some(last), Some(at)) => last.time - at >= STILL_SECONDS,
            _ => false,
        }
    }

    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
}

thread_local! {
    static STATE: RefCell<SnapshotMotion> = RefCell::new(SnapshotMotion::default());
}

/// [`SnapshotMotion::snapshot`] on the engine thread's state.
pub(super) fn note_snapshot(time: f64, origin: [f32; 3], body: &str, movetype: i32) {
    STATE.with(|s| s.borrow_mut().snapshot(time, origin, body, movetype));
}

/// Whether the demo file says the body is about to leave `sprint_*`; read
/// by the next [`note_snapshot`]. False without a timeline.
pub(super) fn note_body_ahead(leaving_sprint: bool) {
    STATE.with(|s| s.borrow_mut().leaving_sprint = leaving_sprint);
}

/// [`SnapshotMotion::in_the_air`] on the engine thread's state.
pub(super) fn in_the_air() -> bool {
    STATE.with(|s| s.borrow().in_the_air())
}

/// [`SnapshotMotion::let_go_of_sprint`] on the engine thread's state.
pub(super) fn let_go_of_sprint() -> bool {
    STATE.with(|s| s.borrow().let_go_of_sprint())
}

/// [`SnapshotMotion::still`] on the engine thread's state.
pub(super) fn still() -> bool {
    STATE.with(|s| s.borrow().still())
}

/// The camera moved to another player.
pub(super) fn forget() {
    STATE.with(|s| s.borrow_mut().reset());
}

#[cfg(test)]
mod tests {
    use super::*;

    const CROUCHED: &str = "crouch_bolt_aim";
    const WALK: i32 = 3;
    const DT: f64 = 0.035;

    /// Four level snapshots ending at `t0`, `DT` apart.
    fn standing(s: &mut SnapshotMotion, t0: f64, body: &str) {
        for i in 0..4 {
            s.snapshot(t0 - f64::from(4 - i) * DT, [0.0; 3], body, WALK);
        }
    }

    /// Snapshots every `DT` from `t0` of a jump from height 0: up at 262 u/s
    /// under 800 u/s² gravity. Calls `each` after every snapshot in the air
    /// and returns the time it is back on 0.
    fn jump(
        s: &mut SnapshotMotion,
        t0: f64,
        body: &str,
        mut each: impl FnMut(&SnapshotMotion),
    ) -> f64 {
        standing(s, t0, body);
        let mut t = t0;
        loop {
            let a = (t - t0) as f32;
            let z = 262.0 * a - 400.0 * a * a;
            if z <= 0.0 && t > t0 {
                return t;
            }
            s.snapshot(t, [a * 200.0, 0.0, z.max(0.0)], body, WALK);
            each(s);
            t += DT;
        }
    }

    #[test]
    fn a_crouched_jump_is_in_the_air_from_take_off_until_it_lands() {
        let mut s = SnapshotMotion::default();
        let mut seen_at = None;
        let mut came_down = false;
        let landing = jump(&mut s, 10.0, CROUCHED, |s| {
            if s.in_the_air() {
                seen_at.get_or_insert(s.last.unwrap().time);
            } else if seen_at.is_some() {
                came_down = true;
            }
        });
        // Seen on the second climbing interval, and held through the top of
        // the jump, where the height stays level for a snapshot or two.
        assert!(seen_at.is_some_and(|t| t - 10.0 < 0.1), "{seen_at:?}");
        assert!(!came_down);
        // The snapshot on the ground still ends a fall; the next one is level.
        s.snapshot(landing, [130.0, 0.0, 0.0], CROUCHED, WALK);
        assert!(s.in_the_air());
        s.snapshot(landing + DT, [130.0, 0.0, 0.0], CROUCHED, WALK);
        assert!(!s.in_the_air());
        s.snapshot(landing + 2.0 * DT, [130.0, 0.0, 0.0], CROUCHED, WALK);
        assert!(!s.in_the_air());
    }

    #[test]
    fn a_ledge_caught_on_the_way_up_is_a_landing() {
        let mut s = SnapshotMotion::default();
        standing(&mut s, 10.0, CROUCHED);
        for (i, z) in [9.0, 17.0, 25.0, 32.0].into_iter().enumerate() {
            s.snapshot(10.0 + i as f64 * DT, [0.0, 0.0, z], CROUCHED, WALK);
        }
        assert!(s.in_the_air());
        // Level from 10.105: still in the air a snapshot later, down once
        // it has been level for LEVEL_SECONDS.
        s.snapshot(10.0 + 4.0 * DT, [0.0, 0.0, 32.0], CROUCHED, WALK);
        assert!(s.in_the_air());
        for i in 5..8 {
            s.snapshot(10.0 + f64::from(i) * DT, [0.0, 0.0, 32.0], CROUCHED, WALK);
        }
        assert!(!s.in_the_air());
    }

    #[test]
    fn standing_jumps_are_left_to_the_body_sequence() {
        let mut s = SnapshotMotion::default();
        jump(&mut s, 10.0, "stand_bolt_aim", |s| assert!(!s.in_the_air()));
    }

    #[test]
    fn a_ladder_dismount_is_not_a_jump() {
        let mut s = SnapshotMotion::default();
        // Climbing at 320 u/s, then off the top, still crouched and rising.
        for i in 0..10 {
            let movetype = if i < 6 { MOVETYPE_FLY } else { WALK };
            s.snapshot(
                10.0 + f64::from(i) * DT,
                [0.0, 0.0, 11.2 * i as f32],
                CROUCHED,
                movetype,
            );
            assert!(!s.in_the_air(), "snapshot {i}");
        }
    }

    #[test]
    fn stairs_are_not_a_jump() {
        let mut s = SnapshotMotion::default();
        // 18-unit steps on most snapshots, 25 ms apart: faster than any jump
        // climbs.
        let mut z = 0.0;
        for i in 0..12 {
            if i % 3 != 2 {
                z += 18.0;
            }
            s.snapshot(10.0 + f64::from(i) * 0.025, [0.0, 0.0, z], CROUCHED, WALK);
            assert!(!s.in_the_air(), "step {i}");
        }
    }

    #[test]
    fn a_crawl_ends_a_tenth_of_a_second_after_he_stops() {
        let mut s = SnapshotMotion::default();
        // Nothing seen yet: the gait decides.
        assert!(!s.still());
        let prone = "prone_rifle_aim";
        for i in 0..10 {
            // 3 u/s, blocked against a wall: still moving.
            s.snapshot(
                10.0 + f64::from(i) * DT,
                [0.105 * i as f32, 0.0, 0.0],
                prone,
                WALK,
            );
            assert!(!s.still(), "snapshot {i}");
        }
        // Stopped at 10.315.
        s.snapshot(10.0 + 10.0 * DT, [0.945, 0.0, 0.0], prone, WALK);
        s.snapshot(10.0 + 11.0 * DT, [0.945, 0.0, 0.0], prone, WALK);
        assert!(!s.still());
        s.snapshot(10.0 + 12.0 * DT, [0.945, 0.0, 0.0], prone, WALK);
        assert!(s.still());
    }

    #[test]
    fn a_clock_going_backwards_forgets_the_jump() {
        let mut s = SnapshotMotion::default();
        standing(&mut s, 10.0, CROUCHED);
        for (i, z) in [9.0, 17.0, 25.0].into_iter().enumerate() {
            s.snapshot(10.0 + i as f64 * DT, [0.0, 0.0, z], CROUCHED, WALK);
        }
        assert!(s.in_the_air());
        s.snapshot(5.0, [0.0; 3], CROUCHED, WALK);
        assert!(!s.in_the_air());
    }

    #[test]
    fn a_lift_climbing_for_ever_lowers_it_once_at_most() {
        let mut s = SnapshotMotion::default();
        standing(&mut s, 10.0, CROUCHED);
        let mut t = 10.0;
        let mut z = 0.0;
        while t < 13.0 {
            z += 260.0 * DT as f32;
            s.snapshot(t, [0.0, 0.0, z], CROUCHED, WALK);
            if t > 10.0 + DT + AIR_SECONDS + DT {
                assert!(!s.in_the_air(), "taken off again at {t}");
            }
            t += DT;
        }
    }

    #[test]
    fn a_long_gap_starts_afresh() {
        let mut s = SnapshotMotion::default();
        let prone = "prone_rifle_aim";
        for i in 0..5 {
            s.snapshot(
                10.0 + f64::from(i) * DT,
                [3.0 * i as f32, 0.0, 0.0],
                prone,
                WALK,
            );
        }
        // Seconds later, somewhere else: nothing is read across the gap.
        s.snapshot(20.0, [500.0, 0.0, -300.0], CROUCHED, WALK);
        assert_eq!(s.climb, None);
        assert!(!s.still());
        assert!(!s.in_the_air());
    }

    #[test]
    fn a_missed_landing_gives_up_after_two_seconds() {
        let mut s = SnapshotMotion::default();
        standing(&mut s, 10.0, CROUCHED);
        for (i, z) in [9.0, 17.0, 25.0].into_iter().enumerate() {
            s.snapshot(10.0 + i as f64 * DT, [0.0, 0.0, z], CROUCHED, WALK);
        }
        // Still climbing slowly for ever (never falls, never level).
        let mut t = 10.0 + 3.0 * DT;
        let mut z = 25.0;
        while t < 10.0 + DT + AIR_SECONDS {
            z += 1.0;
            s.snapshot(t, [0.0, 0.0, z], CROUCHED, WALK);
            assert!(s.in_the_air(), "{t}");
            t += DT;
        }
        s.snapshot(t + DT, [0.0, 0.0, z + 1.0], CROUCHED, WALK);
        assert!(!s.in_the_air());
    }

    const SPRINTING: &str = "sprint_bolt_aim";
    const STANDING: &str = "stand_bolt_aim";

    /// Sprints level at 270 u/s up to `t0`, runs off an edge there and falls
    /// 100 units under 800 u/s² gravity, then stands on the floor below for
    /// four snapshots. `body` names the body at each snapshot's time;
    /// `each` sees every snapshot with its time. Returns the landing time.
    fn walk_off(
        s: &mut SnapshotMotion,
        t0: f64,
        body: impl Fn(f64) -> &'static str,
        mut each: impl FnMut(&SnapshotMotion, f64),
    ) -> f64 {
        let mut t = t0 - 4.0 * DT;
        let mut z: f32;
        loop {
            let a = (t - t0).max(0.0) as f32;
            z = (-400.0 * a * a).max(-100.0);
            s.snapshot(t, [(t - t0) as f32 * 270.0, 0.0, z], body(t), WALK);
            each(s, t);
            if z <= -100.0 {
                break;
            }
            t += DT;
        }
        let landing = t;
        for _ in 0..4 {
            t += DT;
            s.snapshot(t, [(t - t0) as f32 * 270.0, 0.0, z], body(t), WALK);
            each(s, t);
        }
        landing
    }

    #[test]
    fn sprint_held_off_an_edge_is_never_let_go() {
        let mut s = SnapshotMotion::default();
        walk_off(
            &mut s,
            10.0,
            |_| SPRINTING,
            |s, t| {
                assert!(!s.let_go_of_sprint(), "{t}");
            },
        );
    }

    #[test]
    fn sprint_is_let_go_in_a_fall_when_the_body_leaves_sprint() {
        let mut s = SnapshotMotion::default();
        // The body leaves sprint 0.2 s into the fall.
        let body = |t: f64| if t < 10.2 { SPRINTING } else { STANDING };
        let mut let_go = Vec::new();
        let landing = walk_off(&mut s, 10.0, body, |s, t| {
            if s.let_go_of_sprint() {
                let_go.push(t);
            }
        });
        // From the body change until the fall ends, the snapshot after the
        // one on the floor.
        assert!(
            let_go.first().is_some_and(|t| (t - 10.2).abs() < DT),
            "{let_go:?}"
        );
        assert!(
            let_go.last().is_some_and(|t| *t < landing + DT),
            "{let_go:?}"
        );
        // On the floor, a sprint body or not, it is over.
        assert!(!s.let_go_of_sprint());
    }

    #[test]
    fn looking_ahead_the_let_go_shows_before_the_body_change() {
        // The body leaves sprint 0.2 s into the fall, and the demo file
        // says so LET_GO_LOOK_AHEAD_SECONDS before that snapshot arrives.
        let mut s = SnapshotMotion::default();
        let change = 10.2;
        let mut first = None;
        let mut t = 10.0 - 4.0 * DT;
        while t < 10.5 {
            let body = if t < change { SPRINTING } else { STANDING };
            s.leaving_sprint = body == SPRINTING
                && (t..=t + f64::from(LET_GO_LOOK_AHEAD_SECONDS)).contains(&change);
            let a = (t - 10.0).max(0.0) as f32;
            s.snapshot(
                t,
                [(t - 10.0) as f32 * 270.0, 0.0, -400.0 * a * a],
                body,
                WALK,
            );
            if s.let_go_of_sprint() {
                first.get_or_insert(t);
            }
            t += DT;
        }
        let lead = f64::from(LET_GO_LOOK_AHEAD_SECONDS);
        assert!(
            first.is_some_and(|t| t < change && t >= change - lead - DT),
            "{first:?}"
        );
    }

    #[test]
    fn looking_ahead_doesnt_let_go_on_the_ground() {
        // Sprinting level, the body about to change: no fall, no let-go.
        let mut s = SnapshotMotion {
            leaving_sprint: true,
            ..Default::default()
        };
        standing(&mut s, 10.0, SPRINTING);
        assert!(!s.let_go_of_sprint());
    }

    #[test]
    fn sprint_let_go_before_the_edge_shows_once_the_fall_is_seen() {
        let mut s = SnapshotMotion::default();
        let mut first = None;
        walk_off(
            &mut s,
            10.0,
            |_| STANDING,
            |s, t| {
                if s.let_go_of_sprint() {
                    first.get_or_insert(t);
                }
            },
        );
        // The second interval falling at least FELL.
        assert!(first.is_some_and(|t| t > 10.0 && t < 10.15), "{first:?}");
    }

    #[test]
    fn a_landing_ends_the_let_go_and_the_next_fall_starts_afresh() {
        let mut s = SnapshotMotion::default();
        let body = |t: f64| if t < 10.2 { SPRINTING } else { STANDING };
        walk_off(&mut s, 10.0, body, |_, _| {});
        assert!(!s.let_go_of_sprint());
        // Sprinting again, off the next edge, holding it this time.
        walk_off(
            &mut s,
            11.0,
            |_| SPRINTING,
            |s, t| {
                assert!(!s.let_go_of_sprint(), "{t}");
            },
        );
    }

    #[test]
    fn a_jump_is_never_a_sprint_let_go() {
        // Standing jump: the body plays `jump` until the landing snapshot.
        let mut s = SnapshotMotion::default();
        let mut landed = false;
        let landing = jump(&mut s, 10.0, "jump", |s| assert!(!s.let_go_of_sprint()));
        for i in 0..4 {
            s.snapshot(
                landing + f64::from(i) * DT,
                [130.0, 0.0, 0.0],
                STANDING,
                WALK,
            );
            assert!(!s.let_go_of_sprint(), "after landing {i}");
            landed = true;
        }
        assert!(landed);
        // Crouched take-off, which the body never shows as a jump.
        let mut s = SnapshotMotion::default();
        jump(&mut s, 10.0, CROUCHED, |s| assert!(!s.let_go_of_sprint()));
    }
}
