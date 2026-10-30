//! The per-player state changes a demo holds, read straight out of the
//! `.dem` ahead of playback (issue #1154).
//!
//! One pass over the file, a frame at a time through `dem-patch`'s own
//! frame parser ([`Frames`]), keeping only what the look-ahead uses and
//! dropping each frame as soon as it is read: the memory it needs is about
//! 1 MB of the file and the result (about 155 KB for a 33-minute HLTV half),
//! not the file or the whole parsed demo.
//!
//! Times are `svc_time`, the server clock every network frame opens with,
//! which is the demo player's world clock (`crate::demo_seek::clock`). The
//! game's own clock is not: `crate::lookahead` maps between the two.

use std::io::{Read, Seek};

use dem::types::{Aux, Delta, EngineMessage, FrameData, MessageData, NetMessage};

use super::frames::Frames;

/// The players' entity slots.
const PLAYERS: usize = 32;
/// The hull's origin step: the small hull's origin is this much lower.
const HULL_STEP: f32 = 18.0;
/// An origin step within this of 18 units at a flip, against the motion
/// before it, is the hull changing on the ground (`spectator_eye::crouch`
/// uses the same test on the snapshots it sees).
const STEP_TOLERANCE: f32 = 3.5;
/// Two snapshots further apart than this don't say what the motion between
/// them was.
const SNAPSHOT_GAP_SECONDS: f32 = 0.1;
/// `svc_time` going back more than this is a new map, not the HLTV clock's
/// millisecond step-backs (#927).
const TURNED_BACK_SECONDS: f32 = 1.0;

/// What the look-ahead knows about the whole demo.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Timeline {
    /// For each player slot (index 0 is entity 1), the `svc_time` of every
    /// snapshot that first shows him in the small hull after a crouch on
    /// the ground, in order.
    pub crouch_downs: Vec<Vec<f32>>,
    /// For each player slot, every change of his body `sequence`:
    /// (`svc_time`, new sequence index), in order. Indices, not labels: the
    /// hook names them from the model it draws him with.
    pub body_changes: Vec<Vec<(f32, u16)>>,
    /// Network frames read.
    pub frames: usize,
    /// The `svc_time` span read, first to last.
    pub span: (f32, f32),
}

impl Timeline {
    /// The first ground crouch of entity `entity` whose flip comes after
    /// `after` and no later than `until`, both `svc_time`.
    pub fn next_crouch_down(&self, entity: i32, after: f32, until: f32) -> Option<f32> {
        let slot = usize::try_from(entity).ok()?.checked_sub(1)?;
        let times = self.crouch_downs.get(slot)?;
        let i = times.partition_point(|t| *t <= after);
        times.get(i).copied().filter(|t| *t <= until)
    }

    /// Entity `entity`'s body changes after `after` and no later than
    /// `until`, both `svc_time`.
    pub fn body_changes(&self, entity: i32, after: f32, until: f32) -> &[(f32, u16)] {
        let Some(changes) = usize::try_from(entity)
            .ok()
            .and_then(|e| e.checked_sub(1))
            .and_then(|slot| self.body_changes.get(slot))
        else {
            return &[];
        };
        let from = changes.partition_point(|c| c.0 <= after);
        let to = changes.partition_point(|c| c.0 <= until);
        &changes[from..to.max(from)]
    }

    /// The bytes the timeline holds.
    pub fn bytes(&self) -> usize {
        let downs: usize = self
            .crouch_downs
            .iter()
            .map(|v| v.capacity() * std::mem::size_of::<f32>())
            .sum();
        let bodies: usize = self
            .body_changes
            .iter()
            .map(|v| v.capacity() * std::mem::size_of::<(f32, u16)>())
            .sum();
        downs + bodies
    }

    /// One line on what it holds, for the hook log and `ds_debug_status`.
    pub fn summary(&self) -> String {
        format!(
            "{} ground crouches and {} body changes over {:.0} s of demo ({} network frames), {} KB",
            self.crouch_downs.iter().map(Vec::len).sum::<usize>(),
            self.body_changes.iter().map(Vec::len).sum::<usize>(),
            self.span.1 - self.span.0,
            self.frames,
            self.bytes() >> 10
        )
    }
}

/// Why a read gave no timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadError {
    /// The file isn't a demo, or has no directory to find its frames by.
    NotADemo,
    /// `superseded` said another demo is wanted now.
    Superseded,
}

/// Reads the demo in `file` into a [`Timeline`], a frame at a time
/// ([`Frames`]: about 1 MB of the file held at once). Stops at the first
/// frame it can't read, or where `svc_time` turns back (a map change:
/// viewdemo plays only the first map), keeping what it had. Asks
/// `superseded` every network frame and gives up once it says yes.
pub fn read<R: Read + Seek>(
    mut file: R,
    superseded: &dyn Fn() -> bool,
) -> Result<Timeline, ReadError> {
    let directory = crate::demo_file::read_directory(&mut file).ok_or(ReadError::NotADemo)?;
    read_frames(&directory, Frames::new(file), superseded)
}

/// [`read`] once the directory is in, through `frames`.
fn read_frames<R: Read + Seek>(
    directory: &[u8],
    mut frames: Frames<R>,
    superseded: &dyn Fn() -> bool,
) -> Result<Timeline, ReadError> {
    let mut reader = Reader::new();
    for (offset, _) in crate::demo_file::segments(directory) {
        if frames.seek(offset).is_err() {
            break;
        }
        match reader.segment(&mut frames, superseded) {
            Segment::Ended => {}
            Segment::Stopped => break,
            Segment::Superseded => return Err(ReadError::Superseded),
        }
    }
    Ok(reader.finish())
}

/// How reading one directory entry's frames ended.
enum Segment {
    /// At its end: on to the next.
    Ended,
    /// At a frame it can't read, or the clock turning back: the timeline is
    /// what has been read.
    Stopped,
    Superseded,
}

/// One player's state as of a snapshot.
#[derive(Clone, Copy, Debug, Default)]
struct Snap {
    time: f32,
    z: f32,
    small_hull: bool,
}

/// One player slot as the frames go by.
#[derive(Clone, Copy, Debug, Default)]
struct Slot {
    present: bool,
    z: f32,
    small_hull: bool,
    sequence: u16,
    /// The sequence last recorded in the timeline.
    recorded_sequence: Option<u16>,
    /// The two snapshots before the current one, newest last.
    older: Option<Snap>,
    newest: Option<Snap>,
}

struct Reader {
    aux: dem::types::AuxRefCell,
    slots: [Slot; PLAYERS],
    downs: Vec<Vec<f32>>,
    bodies: Vec<Vec<(f32, u16)>>,
    svc: Option<f32>,
    first: Option<f32>,
    frames: usize,
}

impl Reader {
    fn new() -> Self {
        Self {
            aux: Aux::new2(),
            slots: [Slot::default(); PLAYERS],
            downs: vec![Vec::new(); PLAYERS],
            bodies: vec![Vec::new(); PLAYERS],
            svc: None,
            first: None,
            frames: 0,
        }
    }

    /// Reads one directory entry's frames, from where `frames` was put.
    fn segment<R: Read + Seek>(
        &mut self,
        frames: &mut Frames<R>,
        superseded: &dyn Fn() -> bool,
    ) -> Segment {
        loop {
            let Some(frame) = frames.next_frame(&self.aux) else {
                return Segment::Stopped;
            };
            match frame.frame_data {
                FrameData::NextSection => return Segment::Ended,
                FrameData::NetworkMessage(message) => {
                    if superseded() {
                        return Segment::Superseded;
                    }
                    if let MessageData::Parsed(messages) = &message.1.messages {
                        self.frames += 1;
                        for m in messages {
                            if let NetMessage::EngineMessage(em) = m
                                && !self.message(em)
                            {
                                return Segment::Stopped;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// One engine message; false once the clock has turned back.
    fn message(&mut self, em: &EngineMessage) -> bool {
        match em {
            EngineMessage::SvcTime(t) => {
                let t = t.time;
                if !t.is_finite() {
                    return true;
                }
                if self.svc.is_some_and(|last| t < last - TURNED_BACK_SECONDS) {
                    return false;
                }
                self.first.get_or_insert(t);
                self.svc = Some(t);
            }
            EngineMessage::SvcPacketEntities(pe) => {
                for slot in &mut self.slots {
                    slot.present = false;
                }
                for es in &pe.entity_states {
                    self.apply(es.entity_index, Some(&es.delta));
                }
                self.snapshot();
            }
            EngineMessage::SvcDeltaPacketEntities(pe) => {
                for es in &pe.entity_states {
                    if es.remove_entity {
                        if let Some(slot) = slot_of(&mut self.slots, es.entity_index) {
                            slot.present = false;
                        }
                    } else {
                        self.apply(es.entity_index, es.delta.as_ref());
                    }
                }
                self.snapshot();
            }
            _ => {}
        }
        true
    }

    fn apply(&mut self, entity: u16, delta: Option<&Delta>) {
        let Some(slot) = slot_of(&mut self.slots, entity) else {
            return;
        };
        slot.present = true;
        let Some(delta) = delta else { return };
        if let Some(z) = field(delta, "origin[2]") {
            slot.z = f32::from_bits(z);
        }
        if let Some(h) = field(delta, "usehull") {
            slot.small_hull = h == 1;
        }
        if let Some(s) = field(delta, "sequence") {
            slot.sequence = u16::try_from(s).unwrap_or(u16::MAX);
        }
    }

    /// Every present player as of this packet: a flip into the small hull on
    /// the ground is a crouch.
    fn snapshot(&mut self) {
        let Some(time) = self.svc else { return };
        for (k, slot) in self.slots.iter_mut().enumerate() {
            if !slot.present {
                slot.older = None;
                slot.newest = None;
                continue;
            }
            if slot.recorded_sequence != Some(slot.sequence) {
                slot.recorded_sequence = Some(slot.sequence);
                self.bodies[k].push((time, slot.sequence));
            }
            let snap = Snap {
                time,
                z: slot.z,
                small_hull: slot.small_hull,
            };
            if let Some(newest) = slot.newest {
                if snap.time <= newest.time {
                    continue;
                }
                if snap.small_hull && !newest.small_hull && on_ground(slot.older, newest, snap) {
                    self.downs[k].push(time);
                }
            }
            slot.older = slot.newest;
            slot.newest = Some(snap);
        }
    }

    fn finish(self) -> Timeline {
        let mut downs = self.downs;
        for v in &mut downs {
            v.shrink_to_fit();
        }
        let mut bodies = self.bodies;
        for v in &mut bodies {
            v.shrink_to_fit();
        }
        Timeline {
            crouch_downs: downs,
            body_changes: bodies,
            frames: self.frames,
            span: (self.first.unwrap_or(0.0), self.svc.unwrap_or(0.0)),
        }
    }
}

/// Whether a flip from `newest` to `snap` came with the hull's 18-unit
/// origin step (on the ground) rather than none (in the air).
fn on_ground(older: Option<Snap>, newest: Snap, snap: Snap) -> bool {
    if snap.time - newest.time > SNAPSHOT_GAP_SECONDS {
        return false;
    }
    let motion = older
        .filter(|o| newest.time - o.time <= SNAPSHOT_GAP_SECONDS)
        .map_or(0.0, |o| newest.z - o.z);
    let expected = if snap.small_hull {
        -HULL_STEP
    } else {
        HULL_STEP
    };
    ((snap.z - newest.z - motion) - expected).abs() <= STEP_TOLERANCE
}

fn slot_of(slots: &mut [Slot; PLAYERS], entity: u16) -> Option<&mut Slot> {
    slots.get_mut(usize::from(entity).checked_sub(1)?)
}

fn field(delta: &Delta, name: &str) -> Option<u32> {
    // Delta keys come off the wire with their C string's NUL.
    let v = delta.get(name).or_else(|| {
        delta
            .iter()
            .find(|(k, _)| k.trim_end_matches('\0') == name)
            .map(|(_, v)| v)
    })?;
    let mut b = [0u8; 4];
    for (i, x) in v.iter().take(4).enumerate() {
        b[i] = *x;
    }
    Some(u32::from_le_bytes(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(time: f32, z: f32, small_hull: bool) -> Snap {
        Snap {
            time,
            z,
            small_hull,
        }
    }

    fn fixture() -> std::fs::File {
        std::fs::File::open(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../test-fixtures/ci_fixture.dem"
        ))
        .unwrap()
    }

    #[test]
    fn a_flip_with_the_hull_step_is_on_the_ground() {
        let older = Some(snap(1.0, 100.0, false));
        assert!(on_ground(
            older,
            snap(1.033, 100.0, false),
            snap(1.066, 82.0, true)
        ));
    }

    #[test]
    fn a_flip_without_the_step_is_in_the_air() {
        let older = Some(snap(1.0, 100.0, false));
        assert!(!on_ground(
            older,
            snap(1.033, 110.0, false),
            snap(1.066, 119.0, true)
        ));
    }

    #[test]
    fn a_slope_is_taken_out_of_the_step() {
        // Walking down a slope 3 units a snapshot, then crouching.
        let older = Some(snap(1.0, 106.0, false));
        assert!(on_ground(
            older,
            snap(1.033, 103.0, false),
            snap(1.066, 82.0, true)
        ));
    }

    #[test]
    fn a_gap_says_nothing() {
        assert!(!on_ground(
            None,
            snap(1.0, 100.0, false),
            snap(1.3, 82.0, true)
        ));
    }

    #[test]
    fn the_next_crouch_is_looked_up_inside_the_window() {
        let mut t = Timeline {
            crouch_downs: vec![Vec::new(); PLAYERS],
            ..Default::default()
        };
        t.crouch_downs[2] = vec![10.0, 20.0, 30.0];
        assert_eq!(t.next_crouch_down(3, 9.0, 10.5), Some(10.0));
        assert_eq!(t.next_crouch_down(3, 10.0, 10.5), None);
        assert_eq!(t.next_crouch_down(3, 15.0, 25.0), Some(20.0));
        assert_eq!(t.next_crouch_down(4, 9.0, 10.5), None);
        assert_eq!(t.next_crouch_down(0, 9.0, 10.5), None);
        assert_eq!(t.next_crouch_down(99, 9.0, 10.5), None);
    }

    #[test]
    fn body_changes_are_looked_up_inside_the_window() {
        let mut t = Timeline {
            body_changes: vec![Vec::new(); PLAYERS],
            ..Default::default()
        };
        t.body_changes[0] = vec![(1.0, 5), (1.05, 6), (1.2, 7)];
        assert_eq!(t.body_changes(1, 0.99, 1.1), &[(1.0, 5), (1.05, 6)]);
        assert_eq!(t.body_changes(1, 1.0, 1.1), &[(1.05, 6)]);
        assert!(t.body_changes(1, 1.2, 9.0).is_empty());
        assert!(t.body_changes(2, 0.0, 9.0).is_empty());
        assert!(t.body_changes(0, 0.0, 9.0).is_empty());
    }

    #[test]
    fn a_real_demo_gives_a_timeline() {
        let t = read(fixture(), &|| false).unwrap();
        assert!(t.frames > 1000, "{}", t.summary());
        assert!(t.span.1 - t.span.0 > 200.0, "{}", t.summary());
        for v in &t.crouch_downs {
            assert!(v.windows(2).all(|w| w[0] < w[1]));
        }
        assert!(
            t.body_changes.iter().any(|v| v.len() > 50),
            "{}",
            t.summary()
        );
        for v in &t.body_changes {
            assert!(v.windows(2).all(|w| w[0].0 < w[1].0));
        }
    }

    #[test]
    fn a_small_window_reads_the_same_timeline() {
        // The fixture is 10 MB: a 160 KB window refills it ~130 times, with
        // frames split across every refill.
        let whole = read(fixture(), &|| false).unwrap();
        let mut file = fixture();
        let directory = crate::demo_file::read_directory(&mut file).unwrap();
        let frames = Frames::with_window(file, 160 << 10, 80 << 10);
        let small = read_frames(&directory, frames, &|| false).unwrap();
        assert_eq!(small, whole);
    }

    #[test]
    fn a_read_stops_once_superseded() {
        let asked = std::cell::Cell::new(0);
        let superseded = || {
            asked.set(asked.get() + 1);
            asked.get() > 100
        };
        assert_eq!(read(fixture(), &superseded), Err(ReadError::Superseded));
        assert_eq!(asked.get(), 101);
    }

    #[test]
    fn a_file_that_isnt_a_demo_says_so() {
        let not_a_demo = std::io::Cursor::new(vec![0u8; 4096]);
        assert_eq!(read(not_a_demo, &|| false), Err(ReadError::NotADemo));
    }

    /// The survey demo, when it's there: `DOD_LOOKAHEAD_DEMO` or the PRE
    /// install's `wsod25_grp2_h1_hltv.dem`.
    #[test]
    #[ignore]
    fn the_survey_hltv_demo_gives_its_crouches() {
        let path = std::env::var("DOD_LOOKAHEAD_DEMO").unwrap_or_else(|_| {
            r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - PRE-Anniversary for Movies\dod\wsod25_grp2_h1_hltv.dem".to_string()
        });
        let file = std::fs::File::open(&path).expect("no survey demo: set DOD_LOOKAHEAD_DEMO");
        let start = std::time::Instant::now();
        let t = read(file, &|| false).unwrap();
        eprintln!("{} in {:?}", t.summary(), start.elapsed());
        assert!(t.crouch_downs.iter().map(Vec::len).sum::<usize>() > 100);
    }
}
