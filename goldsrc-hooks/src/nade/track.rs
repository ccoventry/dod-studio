//! Each thrown grenade's real path, built from one sample per server frame:
//! where it was, how fast it was going, and whether it has blown up. Pure, so
//! the bounce and explosion rules are tested without a game.

/// The most points one throw keeps. A five-second fuse at 100 fps with a
/// point every couple of units stays far below this.
const MAX_POINTS: usize = 2048;
/// A new point once the grenade has moved this far from the last one.
const MIN_STEP: f32 = 2.0;
/// Below this change in velocity, beyond what gravity explains, nothing hit.
const BOUNCE_MIN_DV: f32 = 40.0;
/// Two bounces closer together than this are one.
const BOUNCE_GAP: f32 = 0.05;

/// One server frame's look at a live grenade.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    /// The edict slot and the game object behind it: a slot reused for a new
    /// grenade is a new throw.
    pub id: (i32, usize),
    pub origin: [f32; 3],
    pub velocity: [f32; 3],
    /// `sv_gravity` times the grenade's own gravity (0.5 for DoD's).
    pub gravity: f32,
    pub on_ground: bool,
    /// Blown up: the game hides it before removing it.
    pub exploded: bool,
}

#[derive(Clone, Debug)]
pub struct Throw {
    pub id: (i32, usize),
    /// Which colour, counting throws since the map started.
    pub number: u32,
    pub points: Vec<[f32; 3]>,
    pub bounces: Vec<[f32; 3]>,
    /// Where it blew up, once it has.
    pub end: Option<[f32; 3]>,
    pub thrown_at: f32,
    pub ended_at: Option<f32>,
    last_velocity: [f32; 3],
    last_time: f32,
    last_bounce: f32,
    rolling: bool,
}

impl Throw {
    pub fn flight_time(&self) -> Option<f32> {
        Some(self.ended_at? - self.thrown_at)
    }

    /// How far it travelled along its path.
    pub fn distance(&self) -> f32 {
        self.points.windows(2).map(|w| dist(w[0], w[1])).sum()
    }

    fn push_point(&mut self, p: [f32; 3]) {
        if self.points.len() >= MAX_POINTS {
            return;
        }
        if self
            .points
            .last()
            .is_none_or(|&last| dist(last, p) >= MIN_STEP)
        {
            self.points.push(p);
        }
    }
}

/// Whether the change from `before` to `after` over `dt` seconds is more than
/// gravity: something the grenade hit.
pub fn is_bounce(before: [f32; 3], after: [f32; 3], dt: f32, gravity: f32) -> bool {
    if dt <= 0.0 {
        return false;
    }
    let expected = [before[0], before[1], before[2] - gravity * dt];
    let dv = dist(expected, after);
    dv > BOUNCE_MIN_DV.max(0.1 * len(before))
}

/// What happened to a throw this frame, for the summary line.
#[derive(Debug, PartialEq)]
pub enum Event {
    Thrown(u32),
    Landed(u32),
}

#[derive(Default)]
pub struct Tracker {
    pub throws: Vec<Throw>,
    next_number: u32,
    /// Grenades that blew up but are still in the entity list (hidden, about
    /// to be removed), so they never start a second throw, even after their
    /// throw itself has been dropped.
    ended: Vec<(i32, usize)>,
}

impl Tracker {
    pub const fn new() -> Self {
        Tracker {
            throws: Vec::new(),
            next_number: 0,
            ended: Vec::new(),
        }
    }

    /// Feeds one server frame: every live grenade, at server time `now`.
    /// Grenades missing from `live` are gone, and a throw still in the air
    /// ends where it was last seen. `keep` is how many throws stay.
    pub fn update(&mut self, now: f32, live: &[Sample], keep: usize) -> Vec<Event> {
        let mut events = Vec::new();
        self.ended.retain(|id| live.iter().any(|s| s.id == *id));
        for s in live {
            if self.ended.contains(&s.id) {
                continue;
            }
            let index = match self
                .throws
                .iter()
                .position(|t| t.id == s.id && t.end.is_none())
            {
                Some(i) => i,
                None => {
                    let number = self.next_number;
                    self.next_number += 1;
                    self.throws.push(Throw {
                        id: s.id,
                        number,
                        points: vec![s.origin],
                        bounces: Vec::new(),
                        end: None,
                        thrown_at: now,
                        ended_at: None,
                        last_velocity: s.velocity,
                        last_time: now,
                        last_bounce: f32::NEG_INFINITY,
                        rolling: s.on_ground,
                    });
                    events.push(Event::Thrown(number));
                    if !s.exploded {
                        continue;
                    }
                    self.throws.len() - 1
                }
            };
            let t = &mut self.throws[index];
            if s.exploded {
                self.ended.push(s.id);
                t.end = Some(s.origin);
                t.ended_at = Some(now);
                t.push_point(s.origin);
                events.push(Event::Landed(t.number));
                continue;
            }
            let dt = now - t.last_time;
            if !t.rolling
                && is_bounce(t.last_velocity, s.velocity, dt, s.gravity)
                && now - t.last_bounce > BOUNCE_GAP
            {
                t.bounces.push(s.origin);
                t.last_bounce = now;
            }
            t.rolling = s.on_ground;
            t.push_point(s.origin);
            t.last_velocity = s.velocity;
            t.last_time = now;
        }

        for t in self.throws.iter_mut().filter(|t| t.end.is_none()) {
            if !live.iter().any(|s| s.id == t.id) {
                t.end = t.points.last().copied();
                t.ended_at = Some(now);
                events.push(Event::Landed(t.number));
            }
        }

        // Drop the oldest finished throws beyond `keep`; never one in flight.
        while self.throws.iter().filter(|t| t.end.is_some()).count() > keep {
            let Some(i) = self.throws.iter().position(|t| t.end.is_some()) else {
                break;
            };
            self.throws.remove(i);
        }
        events
    }

    pub fn clear(&mut self) {
        self.throws.clear();
    }

    /// A new map: forget everything, and start the colours again.
    pub fn reset(&mut self) {
        self.throws.clear();
        self.ended.clear();
        self.next_number = 0;
    }

    pub fn last_landing(&self) -> Option<[f32; 3]> {
        self.throws.iter().rev().find_map(|t| t.end)
    }
}

pub fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    len([a[0] - b[0], a[1] - b[1], a[2] - b[2]])
}

fn len(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: f32 = 400.0; // sv_gravity 800 times the grenade's 0.5

    fn sample(origin: [f32; 3], velocity: [f32; 3]) -> Sample {
        Sample {
            id: (40, 0xabc),
            origin,
            velocity,
            gravity: G,
            on_ground: false,
            exploded: false,
        }
    }

    #[test]
    fn falling_is_not_a_bounce_but_hitting_a_wall_is() {
        let dt = 0.01;
        assert!(!is_bounce([500.0, 0.0, 200.0], [500.0, 0.0, 196.0], dt, G));
        assert!(is_bounce([500.0, 0.0, 200.0], [-250.0, 0.0, 196.0], dt, G));
        // The floor: down becomes up.
        assert!(is_bounce([300.0, 0.0, -300.0], [240.0, 0.0, 150.0], dt, G));
    }

    #[test]
    fn a_throw_records_its_path_bounces_and_where_it_blew_up() {
        let mut tr = Tracker::default();
        let mut events = tr.update(0.0, &[sample([0.0, 0.0, 64.0], [600.0, 0.0, 200.0])], 5);
        assert_eq!(events, vec![Event::Thrown(0)]);
        let mut t = 0.0;
        let mut p = [0.0, 0.0, 64.0];
        let mut v = [600.0f32, 0.0, 200.0];
        for _ in 0..50 {
            t += 0.01;
            v[2] -= G * 0.01;
            p = [p[0] + v[0] * 0.01, p[1], p[2] + v[2] * 0.01];
            events = tr.update(t, &[sample(p, v)], 5);
            assert!(events.is_empty());
        }
        // Hits a wall: x reverses.
        t += 0.01;
        v = [-300.0, 0.0, v[2]];
        tr.update(t, &[sample(p, v)], 5);
        assert_eq!(tr.throws[0].bounces, vec![p]);

        t += 0.5;
        let mut gone = sample(p, [0.0; 3]);
        gone.exploded = true;
        events = tr.update(t, &[gone], 5);
        assert_eq!(events, vec![Event::Landed(0)]);
        let throw = &tr.throws[0];
        assert_eq!(throw.end, Some(p));
        assert!((throw.flight_time().unwrap() - 1.01).abs() < 1e-3);
        assert!(throw.distance() > 290.0);
        // A hidden grenade still in the list doesn't start a new throw.
        assert!(tr.update(t + 0.1, &[gone], 5).is_empty());
        assert_eq!(tr.throws.len(), 1);
    }

    #[test]
    fn a_grenade_that_vanishes_ends_where_it_was_last_seen() {
        let mut tr = Tracker::default();
        tr.update(0.0, &[sample([0.0, 0.0, 0.0], [100.0, 0.0, 0.0])], 5);
        tr.update(0.1, &[sample([10.0, 0.0, 0.0], [100.0, 0.0, 0.0])], 5);
        assert_eq!(tr.update(0.2, &[], 5), vec![Event::Landed(0)]);
        assert_eq!(tr.last_landing(), Some([10.0, 0.0, 0.0]));
    }

    #[test]
    fn rolling_along_the_ground_adds_no_bounces() {
        let mut tr = Tracker::default();
        let mut s = sample([0.0; 3], [300.0, 0.0, 0.0]);
        s.on_ground = true;
        tr.update(0.0, &[s], 5);
        s.velocity = [150.0, 0.0, 0.0]; // friction, a big slowdown
        s.origin = [3.0, 0.0, 0.0];
        tr.update(0.01, &[s], 5);
        assert!(tr.throws[0].bounces.is_empty());
    }

    #[test]
    fn keeps_only_the_newest_finished_throws() {
        let mut tr = Tracker::default();
        for n in 0..4 {
            let mut s = sample([0.0; 3], [0.0; 3]);
            s.id = (40 + n, 1);
            tr.update(n as f32, &[s], 2);
            tr.update(n as f32 + 0.5, &[], 2);
        }
        let numbers: Vec<u32> = tr.throws.iter().map(|t| t.number).collect();
        assert_eq!(numbers, vec![2, 3]);
        tr.reset();
        tr.update(9.0, &[sample([0.0; 3], [0.0; 3])], 2);
        assert_eq!(tr.throws[0].number, 0);
    }
}
