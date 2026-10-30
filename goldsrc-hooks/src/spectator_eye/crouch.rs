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
/// The drawn origin this close to the last snapshot before a flip has
/// reached the step.
const REACHED_UNITS: f32 = 1.0;
/// A step not crossed after this long is taken as crossed: the interpolation
/// is at most 0.1 s behind, so only a missed snapshot gets here.
const STEP_GIVE_UP_SECONDS: f64 = 0.35;
/// A frame further than this from the last, either way, is a seek: the
/// camera settles where the game puts it, with no ease across it.
const SEEK_SECONDS: f64 = 0.25;
/// The drawn origin moving further than this in one frame is a teleport (a
/// respawn), not a crouch.
const TELEPORT_UNITS: f32 = 64.0;
/// Two snapshots further apart than this don't say what the motion between
/// them was.
const SNAPSHOT_GAP_SECONDS: f32 = 0.1;
/// Look-ahead (#1154): a ground crouch known from the demo file starts its
/// ease this long before its flip arrives, so it ends where POV's does. The
/// flip comes 0.40 s after the key; 0.41 against arrival scored best over
/// 267 crouches of 5 players (0.38-0.43 within 7 points of it).
const LEAD_SECONDS: f64 = 0.41;
/// How far ahead of the newest snapshot the caller looks for a flip.
pub(crate) const LOOK_AHEAD_SECONDS: f32 = 0.6;
/// A flip looked ahead to that hasn't arrived this long after it was due
/// isn't coming (the file and the playback disagree): the camera goes back up.
const LATE_SECONDS: f64 = 0.15;
/// A looked-ahead flip still at least this far off that the file no longer
/// puts within this of where it did isn't coming: the clock difference
/// `crate::lookahead` reads every 0.1 s moved under it (a seek), and the
/// camera goes back up at once rather than wait out [`LATE_SECONDS`].
/// Re-reading the difference moves a flip by about 0.01 s.
const WITHDRAWN_SECONDS: f64 = 0.1;
/// A looked-ahead ease first seen more than this after it should have
/// started is left to the flip: starting it there would drop the camera
/// part of the way at once. A flip enters the look-ahead window at least
/// 0.19 s before its ease starts, so only a timeline landing, a seek or a
/// hitch gets here.
const MISSED_START_SECONDS: f64 = 0.1;

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
    /// When his next ground crouch's flip will arrive, on this clock
    /// (`time`), when the demo file says one comes within
    /// [`LOOK_AHEAD_SECONDS`] of the newest snapshot (`crate::lookahead`);
    /// `None` without a timeline.
    pub upcoming_down: Option<f64>,
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
    /// Whether the drawn origin has reached `from_z`. Until it does it is
    /// still in an earlier snapshot interval and shows none of the step.
    reached: bool,
    /// The drawn origin's height above `from_z` on the last frame, to see
    /// it pass `from_z` between two frames.
    last_offset: Option<f32>,
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
    /// When this ease started watching (a new player or a seek): a
    /// look-ahead ease that would have begun before it isn't started.
    started_at: f64,
    /// The flip a look-ahead ease is running for, until it arrives.
    pre_eased: Option<f64>,
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
            started_at: f64::NEG_INFINITY,
            pre_eased: None,
        }
    }

    /// Forgets everything: the camera goes where the game puts it for
    /// `small_hull`, at once.
    pub(crate) fn settle(&mut self, small_hull: bool) {
        let (target, started_at) = (self.target, self.started_at);
        *self = Self::new();
        self.target = target;
        self.started_at = started_at;
        self.settled_shown = if small_hull { HULL_STEP } else { 0.0 };
        self.depth = if small_hull { FULL_DEPTH } else { 0.0 };
    }

    /// The height to add to the drawn origin this frame. Call once per
    /// rendered frame; a second call for the same `time` changes nothing.
    pub(crate) fn eye_height(&mut self, frame: &Frame) -> f32 {
        let now = if self.starts_over(frame) {
            self.settle(frame.snapshot.small_hull);
            self.target = frame.target;
            self.snapshots = [Some(frame.previous), Some(frame.snapshot)];
            self.started_at = frame.time;
            frame.time
        } else {
            // An HLTV demo's clock steps back a few milliseconds several
            // times a second above its packet rate (#927); the ease holds
            // still through those rather than run backwards.
            let now = frame.time.max(self.last_time);
            self.note_snapshot(frame.previous, now);
            self.note_snapshot(frame.snapshot, now);
            now
        };
        self.last_time = now;
        self.last_drawn_z = frame.drawn_z;
        self.look_ahead(frame, now);
        self.depth = self.depth_at(now);
        STAND_EYE + self.shown(frame.drawn_z, now) - self.depth
    }

    /// A new player, a seek or a teleport: nothing before it says anything
    /// about the camera now.
    fn starts_over(&self, frame: &Frame) -> bool {
        let gap = frame.time - self.last_time;
        let newest = self.snapshots[1];
        frame.target != self.target
            || !(-SEEK_SECONDS..=SEEK_SECONDS).contains(&gap)
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
            reached: false,
            last_offset: None,
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
        self.pre_eased = None;
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

    /// Starts the ease into a ground crouch the demo file says is coming, so
    /// it ends as the flip arrives (POV's ease ends at the flip), and backs
    /// out of one whose flip didn't come or that the file stops promising.
    /// Only from standing still, with nothing in flight, and only for an
    /// ease this view was there to see start: after a seek into the middle
    /// of one, or a timeline that lands mid-crouch, the flip is waited for.
    fn look_ahead(&mut self, frame: &Frame, now: f64) {
        if let Some(flip) = self.pre_eased {
            let withdrawn = now < flip - WITHDRAWN_SECONDS
                && frame
                    .upcoming_down
                    .is_none_or(|next| (next - flip).abs() > WITHDRAWN_SECONDS);
            if withdrawn || now > flip + LATE_SECONDS {
                self.pre_eased = None;
                self.depth = self.depth_at(now);
                self.ease = Ease::Up {
                    since: now,
                    from: self.depth,
                };
            }
            return;
        }
        let Some(flip) = frame.upcoming_down else {
            return;
        };
        let standing =
            !frame.snapshot.small_hull && self.steps[0].is_none() && self.depth_at(now) <= 0.0;
        let start = flip - LEAD_SECONDS;
        let on_time = (start..=start + MISSED_START_SECONDS).contains(&now);
        if standing && on_time && start >= self.started_at {
            self.pre_eased = Some(flip);
            self.ease = Ease::Down {
                since: start,
                phase: 0.0,
            };
        }
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
    fn shown(&mut self, drawn_z: f32, now: f64) -> f32 {
        loop {
            let Some(step) = self.steps[0].as_mut() else {
                return self.settled_shown;
            };
            // Climbing (or dropping) into the flip, the drawn origin can be
            // between the two snapshots' heights while still in an earlier
            // interval: a crouch at the top of a stair climb read 16 of 18
            // crossed before this. It has reached the step once it is at
            // `from_z` or has passed it.
            let timed_out = now - step.arrived >= STEP_GIVE_UP_SECONDS;
            let offset = drawn_z - step.from_z;
            step.reached |= offset.abs() <= REACHED_UNITS
                || step.last_offset.is_some_and(|last| last * offset < 0.0);
            step.last_offset = Some(offset);
            if !step.reached && !timed_out {
                return self.settled_shown;
            }
            let span = step.to_z - step.from_z;
            let along = if span.abs() > f32::EPSILON {
                ((drawn_z - step.from_z) / span).clamp(0.0, 1.0)
            } else {
                1.0
            };
            step.crossed = step.crossed.max(along);
            let into_small_hull = step.into_small_hull;
            let crossed = step.crossed;
            if crossed < 1.0 && !timed_out {
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
        /// What the demo file says about ground crouches: their flips'
        /// `msg_time`s (look-ahead, #1154). Empty: no timeline.
        timeline: Vec<f32>,
    }

    const SNAP: f32 = 1.0 / 30.0;
    const INTERP: f32 = 0.1;

    impl Sim {
        fn new() -> Self {
            Self {
                ease: CrouchEase::new(),
                snapshots: vec![],
                timeline: vec![],
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
            // Snapshots arrive at their own msg_time here.
            let upcoming_down = self
                .timeline
                .iter()
                .copied()
                .find(|f| *f > snapshot.msg_time && *f <= snapshot.msg_time + LOOK_AHEAD_SECONDS)
                .map(f64::from);
            let frame = Frame {
                time: f64::from(t),
                target: 3,
                drawn_z,
                snapshot,
                previous,
                upcoming_down,
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
    fn a_crouch_at_the_top_of_a_climb_waits_for_the_drawn_origin() {
        // Rising 16 units a snapshot up to z 180, then crouching there: the
        // drawn origin is still well below 180 when the flip arrives.
        let mut sim = Sim::new();
        for i in 0..90 {
            let t = i as f32 * SNAP;
            let (z, small) = match i {
                0..30 => (100.0, false),
                30..35 => (100.0 + 16.0 * (i - 29) as f32, false),
                35 => (180.0, false),
                _ => (162.0, true),
            };
            sim.snap(t, z, small);
        }
        let frames = run(&mut sim, 0.8, 2.5, 300.0);
        // The climb moves the drawn origin 1.6 units a frame at 300 fps; the
        // ease far less. The hull's 18-unit step must not show as a jump.
        for w in frames.windows(2) {
            assert!((w[1].1 - w[0].1).abs() < 2.0, "{w:?}");
        }
        let last = frames.last().unwrap().1;
        assert!((last - (162.0 + 18.0 - 64.0)).abs() < 1e-3, "{last}");
    }

    #[test]
    fn a_clock_stepping_back_a_moment_keeps_the_ease_going() {
        let mut sim = crouch(1.0, 10.0, 3.0);
        run(&mut sim, 0.9, 1.15, 120.0);
        let before = sim.camera(1.15);
        // HLTV playback's clock steps back a millisecond or two (#927).
        let back = sim.camera(1.1485);
        assert!((back - before).abs() < 0.3, "{before} then {back}");
        assert!(back > 37.0, "settled at crouch height: {back}");
    }

    #[test]
    fn a_seek_back_settles_without_an_ease() {
        let mut sim = crouch(1.0, 10.0, 6.0);
        run(&mut sim, 0.5, 5.0, 60.0);
        // Back to just after the flip, before the ease would be done.
        let mut ease_from_scratch = crouch(1.0, 10.0, 6.0);
        let settled = ease_from_scratch.camera(1.1);
        assert!((sim.camera(1.1) - settled).abs() < 1e-3);
        assert!((settled - 36.0).abs() < 1e-3);
    }

    /// A backward seek in an HLTV demo rewinds to the start and steps
    /// forward 5 s a frame (#1137); landing just after a flip, the camera is
    /// where the game puts it, not part-way down an ease.
    #[test]
    fn stepping_forward_in_five_second_steps_lands_at_the_games_height() {
        let mut sim = crouch(11.0, 30.0, 14.0);
        for t in [0.0, 5.0, 10.0] {
            sim.camera(t);
        }
        let landing = 11.1;
        let cam = sim.camera(landing);
        let game = sim.drawn(landing) + 18.0 - 64.0;
        assert!(
            (cam - game).abs() < 1e-3,
            "camera {cam} vs the game's {game}"
        );
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
            upcoming_down: None,
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

    /// The crouch from `crouch(down, ..)` as the demo file has it: the flip
    /// is the first small-hull snapshot.
    fn with_timeline(mut sim: Sim, down: f32) -> Sim {
        let flip = sim
            .snapshots
            .iter()
            .find(|s| s.msg_time >= down)
            .unwrap()
            .msg_time;
        sim.timeline = vec![flip];
        sim
    }

    #[test]
    fn looking_ahead_the_ease_ends_as_the_flip_arrives() {
        for fps in [60.0, 120.0, 300.0] {
            let mut sim = with_timeline(crouch(1.0, 10.0, 3.0), 1.0);
            let flip = sim.timeline[0];
            let frames = run(&mut sim, 0.2, 2.0, fps);
            let start = f64::from(flip) - LEAD_SECONDS;
            for (t, cam) in frames {
                // POV's curve, ending at the flip (within a frame either way).
                let want = 58.0 - 22.0 * smoothstep((f64::from(t) - start) / DUCK_SECONDS) as f32;
                assert!(
                    (cam - want).abs() < 22.0 * 1.5 / 0.4 / fps + 0.6,
                    "{fps} fps at {t:.3}: camera {cam:.2}, POV's curve {want:.2}"
                );
            }
        }
    }

    #[test]
    fn looking_ahead_doesnt_start_an_ease_it_missed_the_start_of() {
        // Starts watching 0.2 s before the flip, half-way through where the
        // look-ahead ease would be: it waits for the flip, as without one.
        let mut sim = with_timeline(crouch(1.0, 10.0, 3.0), 1.0);
        let flip = sim.timeline[0];
        let frames = run(&mut sim, flip - 0.2, flip - 0.01, 120.0);
        assert!(
            frames.iter().all(|(_, cam)| (cam - 58.0).abs() < 1e-3),
            "{frames:?}"
        );
    }

    #[test]
    fn a_looked_ahead_flip_that_never_comes_lets_the_camera_back_up() {
        // The file says he crouches at 1.0; the playback has him stand.
        let mut sim = crouch(100.0, 100.0, 3.0);
        sim.timeline = vec![1.0];
        let frames = run(&mut sim, 0.2, 2.0, 120.0);
        let lowest = frames.iter().map(|f| f.1).fold(f32::MAX, f32::min);
        assert!(lowest < 45.0, "the ease never started: {lowest}");
        let (_, end) = frames.last().unwrap();
        assert!((end - 58.0).abs() < 1e-3, "still down at the end: {end}");
    }

    #[test]
    fn a_looked_ahead_flip_the_file_stops_promising_lets_the_camera_up_at_once() {
        // Half-way down into a crouch due at 1.0, a seek moves the clock
        // difference and the file has no crouch coming any more: the camera
        // is back up in POV's stand-up time, not after the flip was due.
        let mut sim = crouch(100.0, 100.0, 3.0);
        sim.timeline = vec![1.0];
        let before = run(&mut sim, 0.2, 0.75, 120.0);
        let (_, at_seek) = before.last().unwrap();
        assert!(*at_seek < 52.0, "the ease never started: {at_seek}");
        sim.timeline.clear();
        let after = run(&mut sim, 0.76, 0.9, 120.0);
        let (_, end) = after.last().unwrap();
        assert!((end - 58.0).abs() < 1e-3, "still down at 0.9: {end}");
    }

    #[test]
    fn a_looked_ahead_ease_first_seen_late_waits_for_the_flip() {
        // The timeline lands 0.16 s after this crouch's ease should have
        // started: starting it there would drop the camera 8 units at once.
        let mut sim = crouch(1.0, 10.0, 3.0);
        let flip = sim
            .snapshots
            .iter()
            .find(|s| s.small_hull)
            .unwrap()
            .msg_time;
        let start = flip - LEAD_SECONDS as f32;
        run(&mut sim, 0.2, start + 0.15, 120.0);
        sim.timeline = vec![flip];
        let frames = run(&mut sim, start + 0.16, flip - 0.01, 120.0);
        assert!(
            frames.iter().all(|(_, cam)| (cam - 58.0).abs() < 1e-3),
            "{frames:?}"
        );
    }
}
