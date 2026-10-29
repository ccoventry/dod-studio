//! The in-eye camera's height through a crouch, eased on the demo clock the
//! way the player's own view eases it (issue #1144).
//!
//! ## What the two views do
//!
//! Surveyed on 11 POV demos from 6 players against their halves' HLTV demos
//! (`wsod25_grp2_h1_*`, `wsod25_grp4_h1_*`) and 12 more POVs from 12 other
//! players (`solifotw-*`), with `analysis/examples/crouch_eye_probe.rs`:
//!
//! - **Going down**, POV eases the camera the whole 22 units (58 to 36 above
//!   the feet) over 0.4 s from the key, while the player is still in the
//!   standing hull: `22 x smoothstep(t / 0.4)`, half-way at 0.20 s in all 18
//!   players. The server flips `usehull` when that ease ends, and the flip is
//!   the first thing an HLTV demo shows. From it the game's in-eye camera
//!   drops 4 at once and 18 more as the interpolated origin falls into the
//!   small hull: about 0.1 s, four times faster than POV.
//! - **Getting up**, POV rises in 0.11 s (5 units at once, then about
//!   160 u/s), and the game's in-eye camera in about the same time.
//! - **In the air** (most of the flips: crouch-jumps) the origin doesn't
//!   move at the flip and both views drop 4 at once. Left as the game does it.
//!
//! An HLTV demo has nothing earlier than the flip to start from, so the ease
//! starts there: the same speed as POV, finishing 0.4 s after POV does.
//!
//! ## How
//!
//! The game's camera is the interpolated origin plus 18 or 22. This keeps a
//! "depth" into the crouch, 0 standing to 22 crouched, eased on the demo
//! clock, and the part of the hull's 18-unit origin step the interpolated
//! origin already shows, `shown`. The height to add to that origin is then
//! `22 + shown - depth`: 22 standing, 18 crouched, and in between a camera
//! that moves only as `depth` does, whatever the interpolation is doing.
//! The same value goes in both of the game's heights, so it doesn't matter
//! which branch the latest snapshot's `usehull` picks.
//!
//! `shown` is read off the origin itself: across the step, the interpolated
//! origin goes from the last standing snapshot's height to the first
//! crouched one's, so how far along it is says how much of the step it
//! shows. That holds at any `ex_interp` and any update rate.

/// The standing eye, above the origin.
const STAND_EYE: f32 = 22.0;
/// How much the camera drops into a crouch: the hull's 18-unit origin step
/// plus the 4 between the two eyes.
const FULL_DEPTH: f32 = 22.0;
/// The hull's origin step: the small hull's origin is this much lower.
const HULL_STEP: f32 = 18.0;
/// POV's duck time: the camera reaches crouch height this long after the key.
const DUCK_SECONDS: f64 = 0.4;
/// Getting up, POV's camera rises this much on the first frame...
const STAND_FIRST_FRAME: f32 = 5.0;
/// ...and the rest at this rate.
const STAND_UNITS_PER_SECOND: f32 = 160.0;
/// An origin step at a flip within this of 18 units, against the motion
/// before it, is the hull changing on the ground. In the survey the on-ground
/// flips were 16-19 and the in-air ones within about 10 of 0.
const STEP_TOLERANCE: f32 = 3.5;
/// A step not crossed after this long is taken as crossed: the interpolation
/// is at most 0.1 s behind, so only a missed snapshot gets here.
const STEP_GIVE_UP_SECONDS: f64 = 0.35;
/// A frame further than this from the last, or behind it, is a seek: the
/// camera settles where the game puts it, with no ease across it.
const SEEK_SECONDS: f64 = 0.25;
/// The drawn origin moving further than this in one frame is a teleport (a
/// respawn), not a crouch.
const TELEPORT_UNITS: f32 = 64.0;
/// Two snapshots further apart than this don't say what the motion between
/// them was.
const SNAPSHOT_GAP_SECONDS: f32 = 0.1;

/// One frame's look at the watched player.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Frame {
    /// `ref_params_s::time`, the demo clock.
    pub time: f64,
    /// Entity index of the player whose eyes the camera is in.
    pub target: i32,
    /// The interpolated origin's height, where the game draws him this frame.
    pub drawn_z: f32,
    /// The newest snapshot of him.
    pub snapshot: Snapshot,
    /// The newest snapshot's predecessor, so a frame that two snapshots
    /// arrived in still sees the flip between them.
    pub previous: Snapshot,
}

/// What one snapshot says about the player.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Snapshot {
    /// `entity_state_t::msg_time`.
    pub msg_time: f32,
    /// `entity_state_t::origin[2]`.
    pub z: f32,
    /// `usehull == 1`.
    pub small_hull: bool,
}

/// The hull's origin step at one flip, until the drawn origin has crossed it.
#[derive(Clone, Copy, Debug)]
struct Step {
    /// The last snapshot's height before the flip and the first after it.
    from_z: f32,
    to_z: f32,
    into_small_hull: bool,
    /// When the flip arrived, on the demo clock.
    arrived: f64,
    /// How far across the drawn origin has got, 0 to 1; never goes back.
    crossed: f32,
}

/// Where `depth` is heading.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Ease {
    /// Not moving.
    Still,
    /// Down along POV's curve, `phase` seconds into it at `since`.
    Down { since: f64, phase: f64 },
    /// Up at POV's rate from `from` at `since`.
    Up { since: f64, from: f32 },
}

/// Steps waiting to be crossed: two flips inside one interpolation window
/// is the most a player can do (a crouch tapped and let go).
const STEPS_KEPT: usize = 4;

#[derive(Debug)]
pub(crate) struct CrouchEase {
    target: i32,
    last_time: f64,
    last_drawn_z: f32,
    /// The two newest snapshots seen, newest last.
    snapshots: [Option<Snapshot>; 2],
    steps: [Option<Step>; STEPS_KEPT],
    /// The part of the step shown by everything already crossed: 0 or 18.
    settled_shown: f32,
    depth: f32,
    ease: Ease,
}

impl Default for CrouchEase {
    fn default() -> Self {
        Self::new()
    }
}

/// `3x^2 - 2x^3`, POV's duck curve, for `x` in 0..=1.
fn smoothstep(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// The `x` in 0..=1 where [`smoothstep`] is `y`.
fn smoothstep_inverse(y: f64) -> f64 {
    let y = y.clamp(0.0, 1.0);
    0.5 - ((1.0 - 2.0 * y).asin() / 3.0).sin()
}

impl CrouchEase {
    pub(crate) const fn new() -> Self {
        Self {
            target: 0,
            last_time: f64::NEG_INFINITY,
            last_drawn_z: 0.0,
            snapshots: [None; 2],
            steps: [None; STEPS_KEPT],
            settled_shown: 0.0,
            depth: 0.0,
            ease: Ease::Still,
        }
    }

    /// Forgets everything: the camera goes where the game puts it for
    /// `small_hull`, at once.
    pub(crate) fn settle(&mut self, small_hull: bool) {
        let target = self.target;
        *self = Self::new();
        self.target = target;
        self.settled_shown = if small_hull { HULL_STEP } else { 0.0 };
        self.depth = if small_hull { FULL_DEPTH } else { 0.0 };
    }

    /// The height to add to the drawn origin this frame. Call once per
    /// rendered frame; a second call for the same `time` changes nothing.
    pub(crate) fn eye_height(&mut self, frame: &Frame) -> f32 {
        if self.starts_over(frame) {
            self.settle(frame.snapshot.small_hull);
            self.target = frame.target;
            self.snapshots = [Some(frame.previous), Some(frame.snapshot)];
        } else {
            self.note_snapshot(frame.previous, frame.time);
            self.note_snapshot(frame.snapshot, frame.time);
        }
        self.last_time = frame.time;
        self.last_drawn_z = frame.drawn_z;
        self.depth = self.depth_at(frame.time);
        STAND_EYE + self.shown(frame) - self.depth
    }

    /// A new player, a seek or a teleport: nothing before it says anything
    /// about the camera now.
    fn starts_over(&self, frame: &Frame) -> bool {
        let gap = frame.time - self.last_time;
        let newest = self.snapshots[1];
        frame.target != self.target
            || !(0.0..=SEEK_SECONDS).contains(&gap)
            || newest.is_some_and(|s| frame.snapshot.msg_time < s.msg_time)
            || (frame.drawn_z - self.last_drawn_z).abs() > TELEPORT_UNITS
    }

    fn note_snapshot(&mut self, snapshot: Snapshot, now: f64) {
        let [older, newest] = self.snapshots;
        let Some(newest) = newest else {
            self.snapshots = [None, Some(snapshot)];
            return;
        };
        if snapshot.msg_time <= newest.msg_time {
            return;
        }
        self.snapshots = [Some(newest), Some(snapshot)];
        if snapshot.small_hull == newest.small_hull {
            return;
        }
        let step = snapshot.z - newest.z;
        // The motion the player already had, so a slope or a fall isn't
        // read as part of the step.
        let motion = older
            .filter(|o| newest.msg_time - o.msg_time <= SNAPSHOT_GAP_SECONDS)
            .map_or(0.0, |o| newest.z - o.z);
        let expected = if snapshot.small_hull {
            -HULL_STEP
        } else {
            HULL_STEP
        };
        let on_ground = snapshot.msg_time - newest.msg_time <= SNAPSHOT_GAP_SECONDS
            && ((step - motion) - expected).abs() <= STEP_TOLERANCE;
        if !on_ground {
            // In the air (or after a gap): the game's own heights are right.
            self.settle(snapshot.small_hull);
            self.snapshots = [Some(newest), Some(snapshot)];
            return;
        }
        let new_step = Step {
            from_z: newest.z,
            to_z: snapshot.z,
            into_small_hull: snapshot.small_hull,
            arrived: now,
            crossed: 0.0,
        };
        match self.steps.iter_mut().find(|s| s.is_none()) {
            Some(free) => *free = Some(new_step),
            // More flips than a player can make in one window: something
            // else is going on, so let the game have it.
            None => {
                self.settle(snapshot.small_hull);
                self.snapshots = [Some(newest), Some(snapshot)];
                return;
            }
        }
        self.depth = self.depth_at(now);
        self.ease = if snapshot.small_hull {
            Ease::Down {
                since: now,
                phase: smoothstep_inverse(f64::from(self.depth / FULL_DEPTH)) * DUCK_SECONDS,
            }
        } else {
            Ease::Up {
                since: now,
                from: self.depth,
            }
        };
    }

    fn depth_at(&self, now: f64) -> f32 {
        match self.ease {
            Ease::Still => self.depth,
            Ease::Down { since, phase } => {
                let x = (phase + (now - since).max(0.0)) / DUCK_SECONDS;
                (smoothstep(x) * f64::from(FULL_DEPTH)) as f32
            }
            Ease::Up { since, from } => {
                let risen =
                    STAND_FIRST_FRAME + STAND_UNITS_PER_SECOND * (now - since).max(0.0) as f32;
                // POV's own rise, from full depth, minus what this one has
                // left to rise: a partial crouch comes up along the same line.
                let pov_left = (FULL_DEPTH - risen).max(0.0);
                pov_left.min(from)
            }
        }
    }

    /// How much of the hull's step the drawn origin shows: 0 standing, 18
    /// crouched. Crosses the waiting steps in order, as the interpolation
    /// does.
    fn shown(&mut self, frame: &Frame) -> f32 {
        loop {
            let Some(step) = self.steps[0].as_mut() else {
                return self.settled_shown;
            };
            let span = step.to_z - step.from_z;
            let along = if span.abs() > f32::EPSILON {
                ((frame.drawn_z - step.from_z) / span).clamp(0.0, 1.0)
            } else {
                1.0
            };
            step.crossed = step.crossed.max(along);
            let into_small_hull = step.into_small_hull;
            let crossed = step.crossed;
            if crossed < 1.0 && frame.time - step.arrived < STEP_GIVE_UP_SECONDS {
                return if into_small_hull {
                    HULL_STEP * crossed
                } else {
                    HULL_STEP * (1.0 - crossed)
                };
            }
            self.settled_shown = if into_small_hull { HULL_STEP } else { 0.0 };
            self.steps.rotate_left(1);
            self.steps[STEPS_KEPT - 1] = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A player standing on flat ground at z 100 (standing origin), 30
    /// snapshots a second, drawn 0.1 s behind the newest snapshot.
    struct Sim {
        ease: CrouchEase,
        /// (msg_time, z, small_hull)
        snapshots: Vec<Snapshot>,
    }

    const SNAP: f32 = 1.0 / 30.0;
    const INTERP: f32 = 0.1;

    impl Sim {
        fn new() -> Self {
            Self {
                ease: CrouchEase::new(),
                snapshots: vec![],
            }
        }

        fn snap(&mut self, t: f32, z: f32, small_hull: bool) {
            self.snapshots.push(Snapshot {
                msg_time: t,
                z,
                small_hull,
            });
        }

        /// The game's drawn origin at `t`: lerp at `t - INTERP`.
        fn drawn(&self, t: f32) -> f32 {
            let r = t - INTERP;
            let i = self
                .snapshots
                .iter()
                .rposition(|s| s.msg_time <= r)
                .unwrap_or(0);
            let a = self.snapshots[i];
            let Some(b) = self.snapshots.get(i + 1) else {
                return a.z;
            };
            let f = ((r - a.msg_time) / (b.msg_time - a.msg_time)).clamp(0.0, 1.0);
            a.z + (b.z - a.z) * f
        }

        /// Camera height above the ground (standing origin 100 = feet 64).
        fn camera(&mut self, t: f32) -> f32 {
            let n = self
                .snapshots
                .iter()
                .rposition(|s| s.msg_time <= t)
                .unwrap_or(0);
            let snapshot = self.snapshots[n];
            let previous = self.snapshots[n.saturating_sub(1)];
            let drawn_z = self.drawn(t);
            let frame = Frame {
                time: f64::from(t),
                target: 3,
                drawn_z,
                snapshot,
                previous,
            };
            drawn_z + self.ease.eye_height(&frame) - 64.0
        }
    }

    /// Snapshots every 1/30 s from 0 to `end`, crouched (origin 82) from
    /// `down` to `up`.
    fn crouch(down: f32, up: f32, end: f32) -> Sim {
        let mut sim = Sim::new();
        let mut i = 0;
        loop {
            let t = i as f32 * SNAP;
            if t > end {
                break;
            }
            let small = t >= down && t < up;
            sim.snap(t, if small { 82.0 } else { 100.0 }, small);
            i += 1;
        }
        sim
    }

    fn run(sim: &mut Sim, from: f32, to: f32, fps: f32) -> Vec<(f32, f32)> {
        let mut out = vec![];
        let mut t = from;
        while t <= to {
            out.push((t, sim.camera(t)));
            t += 1.0 / fps;
        }
        out
    }

    #[test]
    fn the_smoothstep_inverse_undoes_it() {
        for x in [0.0, 0.1, 0.25, 0.5, 0.8, 1.0] {
            assert!((smoothstep_inverse(smoothstep(x)) - x).abs() < 1e-6, "{x}");
        }
    }

    #[test]
    fn standing_and_crouched_rest_at_the_games_own_heights() {
        let mut sim = crouch(1.0, 10.0, 3.0);
        // 58 = 36 below the standing origin... feet 64, eye 22 above 100.
        assert!((sim.camera(0.5) - 58.0).abs() < 1e-3);
        run(&mut sim, 0.5, 2.0, 120.0);
        assert!((sim.camera(2.0) - 36.0).abs() < 1e-3, "{}", sim.camera(2.0));
    }

    #[test]
    fn going_down_follows_povs_curve_from_the_flip() {
        let down = 1.0;
        for fps in [60.0, 120.0, 300.0] {
            let mut sim = crouch(down, 10.0, 3.0);
            let frames = run(&mut sim, 0.5, 2.0, fps);
            // The flip arrives with the first crouched snapshot at or after 1.0.
            // The ease starts on the first frame that sees the flip.
            let flip = (down / SNAP).ceil() * SNAP;
            let seen = frames.iter().find(|(t, _)| *t >= flip).unwrap().0;
            for (t, cam) in frames {
                let since = f64::from(t - seen);
                let want = 58.0 - 22.0 * smoothstep(since / DUCK_SECONDS) as f32;
                assert!(
                    (cam - want).abs() < 0.6,
                    "{fps} fps at {t:.3}: camera {cam:.2}, POV's curve {want:.2}"
                );
            }
        }
    }

    #[test]
    fn half_way_down_takes_the_same_time_at_any_frame_rate() {
        let mut halves = vec![];
        for fps in [60.0, 120.0, 300.0] {
            let mut sim = crouch(1.0, 10.0, 3.0);
            let frames = run(&mut sim, 0.9, 2.0, fps);
            let half = frames.iter().find(|(_, c)| *c <= 47.0).unwrap().0;
            halves.push(half);
        }
        for h in &halves {
            // Two 60 fps frames: the frame that sees the flip, and the frame
            // that finds the camera past half-way.
            assert!((h - halves[0]).abs() <= 2.0 / 60.0 + 1e-4, "{halves:?}");
        }
    }

    #[test]
    fn getting_up_rises_at_povs_rate() {
        let mut sim = crouch(1.0, 2.0, 3.0);
        let frames = run(&mut sim, 1.0, 2.6, 300.0);
        let flip = (2.0 / SNAP).ceil() * SNAP;
        let up = frames
            .iter()
            .find(|(t, c)| *t >= flip && *c >= 57.9)
            .unwrap()
            .0;
        // POV: 17 to go after the first frame, at 160 u/s, about 0.11 s.
        assert!((up - flip - 0.106).abs() < 0.01, "{}", up - flip);
        // And never below crouch height or above standing on the way.
        assert!(frames.iter().all(|(_, c)| (35.9..=58.1).contains(c)));
    }

    #[test]
    fn a_crouch_let_go_half_way_down_comes_straight_back_up() {
        // Down at 1.0, up at 1.2: the ease is about half-way.
        let mut sim = crouch(1.0, 1.2, 3.0);
        let frames = run(&mut sim, 0.9, 2.0, 300.0);
        let lowest = frames.iter().map(|(_, c)| *c).fold(f32::MAX, f32::min);
        assert!(lowest > 44.0 && lowest < 52.0, "lowest {lowest}");
        // No jump between frames anywhere: at most POV's own first-frame rise.
        for w in frames.windows(2) {
            assert!((w[1].1 - w[0].1).abs() <= 5.01, "{w:?}");
        }
        assert!((frames.last().unwrap().1 - 58.0).abs() < 1e-3);
    }

    #[test]
    fn a_crouch_in_the_air_is_left_to_the_game() {
        // A jump: the origin rises and falls smoothly, and the hull flips
        // with no step in it.
        let mut sim = Sim::new();
        for i in 0..90 {
            let t = i as f32 * SNAP;
            let z = 100.0 + (60.0 * (t - 1.0) - 400.0 * (t - 1.0) * (t - 1.0)).max(0.0);
            sim.snap(t, z, t >= 1.05);
        }
        for (t, cam) in run(&mut sim, 0.5, 2.5, 120.0) {
            let drawn = sim.drawn(t);
            let snapshot_small = t >= (1.05 / SNAP).ceil() * SNAP;
            let game = drawn + if snapshot_small { 18.0 } else { 22.0 } - 64.0;
            assert!(
                (cam - game).abs() < 1e-3,
                "{t:.3}: {cam} vs the game's {game}"
            );
        }
    }

    #[test]
    fn a_seek_settles_without_an_ease() {
        let mut sim = crouch(1.0, 10.0, 6.0);
        run(&mut sim, 0.5, 1.1, 120.0);
        // Jump ahead to 5.0, crouched: straight to crouch height.
        assert!((sim.camera(5.0) - 36.0).abs() < 1e-3);
    }

    #[test]
    fn a_new_player_starts_where_the_game_has_him() {
        let mut sim = crouch(1.0, 10.0, 3.0);
        run(&mut sim, 0.5, 1.15, 120.0);
        let mut frame = Frame {
            time: 1.16,
            target: 7,
            drawn_z: 82.0,
            snapshot: Snapshot {
                msg_time: 1.15,
                z: 82.0,
                small_hull: true,
            },
            previous: Snapshot {
                msg_time: 1.12,
                z: 82.0,
                small_hull: true,
            },
        };
        assert_eq!(sim.ease.eye_height(&frame), 18.0);
        frame.time += 0.01;
        assert_eq!(sim.ease.eye_height(&frame), 18.0);
    }

    #[test]
    fn walking_down_a_slope_crouched_keeps_the_resting_height() {
        // Crouched and moving down 3 units a snapshot.
        let mut sim = Sim::new();
        for i in 0..60 {
            let t = i as f32 * SNAP;
            sim.snap(t, 82.0 - 3.0 * i as f32, true);
        }
        for (t, _) in run(&mut sim, 0.5, 1.8, 120.0) {
            let frame_h = sim.camera(t) - (sim.drawn(t) - 64.0);
            assert!((frame_h - 18.0).abs() < 1e-3, "{t}: {frame_h}");
        }
    }
}
