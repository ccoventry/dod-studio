//! The per-player state changes a demo holds, read straight out of the
//! `.dem` ahead of playback (issue #1154).
//!
//! One pass over the file, frame by frame through `dem-patch`'s own frame
//! parser, keeping only what the look-ahead uses and dropping each frame as
//! soon as it is read: the memory it needs is the file's bytes and the
//! result, not the whole parsed demo (a full `open_demo_from_bytes` of a
//! 42 MB HLTV demo peaks above 800 MB, too much for a 32-bit game).
//!
//! Times are `svc_time`, the server clock every network frame opens with:
//! the clock an entity state's `msg_time` carries, so the hook can compare
//! them directly whatever the demo player's own clock does (seeks, and the
//! HLTV clock's millisecond step-backs, #927).

use std::collections::HashMap;

use dem::demo_parser::{parse_frame, parse_header};
use dem::types::{
    Aux, Delta, EngineMessage, FrameData, MessageData, MessageDataParseMode, NetMessage,
};

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
/// The directory's entries (`demoentry_t`) and the frame offset in one.
const ENTRY_SIZE: usize = 92;
const OFFSET_AT: usize = 84;
const MAX_ENTRIES: i32 = 1024;

/// What the look-ahead knows about the whole demo.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Timeline {
    /// For each player slot (index 0 is entity 1), the `svc_time` of every
    /// snapshot that first shows him in the small hull after a crouch on
    /// the ground, in order.
    pub crouch_downs: Vec<Vec<f32>>,
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

    /// Bytes the timeline holds, for the log.
    pub fn bytes(&self) -> usize {
        self.crouch_downs
            .iter()
            .map(|v| v.capacity() * std::mem::size_of::<f32>())
            .sum()
    }
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
    /// The two snapshots before the current one, newest last.
    older: Option<Snap>,
    newest: Option<Snap>,
}

/// Reads `bytes` (a whole `.dem` file) into a [`Timeline`]. Stops at the
/// first frame it can't read, or where `svc_time` turns back (a map change:
/// viewdemo plays only the first map), keeping what it had. `None` when the
/// file isn't a demo.
pub fn read(bytes: &[u8]) -> Option<Timeline> {
    let (_, header) = parse_header(bytes).ok()?;
    let directory = usize::try_from(header.directory_offset).ok()?;
    let count = i32::from_le_bytes(bytes.get(directory..directory + 4)?.try_into().ok()?);
    if !(1..=MAX_ENTRIES).contains(&count) {
        return None;
    }
    let mut reader = Reader::new();
    for k in 0..count as usize {
        let at = directory + 4 + k * ENTRY_SIZE;
        let entry = bytes.get(at..at + ENTRY_SIZE)?;
        let offset = i32::from_le_bytes(entry[OFFSET_AT..OFFSET_AT + 4].try_into().ok()?);
        let Ok(offset) = usize::try_from(offset) else {
            break;
        };
        if offset >= bytes.len() || !reader.entry(&bytes[offset..]) {
            break;
        }
    }
    Some(reader.finish())
}

struct Reader {
    aux: dem::types::AuxRefCell,
    slots: [Slot; PLAYERS],
    downs: Vec<Vec<f32>>,
    svc: Option<f32>,
    first: Option<f32>,
    frames: usize,
    turned_back: bool,
}

impl Reader {
    fn new() -> Self {
        Self {
            aux: Aux::new2(),
            slots: [Slot::default(); PLAYERS],
            downs: vec![Vec::new(); PLAYERS],
            svc: None,
            first: None,
            frames: 0,
            turned_back: false,
        }
    }

    /// Reads one directory entry's frames; false once reading should stop.
    fn entry(&mut self, mut input: &[u8]) -> bool {
        loop {
            let Ok((rest, frame)) =
                parse_frame(input, MessageDataParseMode::Parse, self.aux.clone())
            else {
                return false;
            };
            input = rest;
            match frame.frame_data {
                FrameData::NextSection => return true,
                FrameData::NetworkMessage(message) => {
                    if let MessageData::Parsed(messages) = &message.1.messages {
                        self.frames += 1;
                        for m in messages {
                            if let NetMessage::EngineMessage(em) = m
                                && !self.message(em)
                            {
                                return false;
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
                if self.svc.is_some_and(|last| t < last - 1.0) {
                    self.turned_back = true;
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
        Timeline {
            crouch_downs: downs,
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

/// For the log: how many ground crouches each present slot has.
pub fn summary(timeline: &Timeline) -> String {
    let per: HashMap<usize, usize> = timeline
        .crouch_downs
        .iter()
        .enumerate()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (k + 1, v.len()))
        .collect();
    let total: usize = per.values().sum();
    format!(
        "{} network frames, svc {:.1}-{:.1}, {} ground crouches over {} players, {} bytes",
        timeline.frames,
        timeline.span.0,
        timeline.span.1,
        total,
        per.len(),
        timeline.bytes()
    )
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
    fn a_real_demo_gives_a_timeline() {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../test-fixtures/ci_fixture.dem"
        ))
        .unwrap();
        let t = read(&bytes).unwrap();
        assert!(t.frames > 1000, "{}", summary(&t));
        assert!(t.span.1 - t.span.0 > 200.0, "{}", summary(&t));
        for v in &t.crouch_downs {
            assert!(v.windows(2).all(|w| w[0] < w[1]));
        }
    }

    /// The survey demo, when it's there: `DOD_LOOKAHEAD_DEMO` or the PRE
    /// install's `wsod25_grp2_h1_hltv.dem`.
    #[test]
    #[ignore]
    fn the_survey_hltv_demo_gives_its_crouches() {
        let path = std::env::var("DOD_LOOKAHEAD_DEMO").unwrap_or_else(|_| {
            r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - PRE-Anniversary for Movies\dod\wsod25_grp2_h1_hltv.dem".to_string()
        });
        let bytes = std::fs::read(&path).expect("no survey demo: set DOD_LOOKAHEAD_DEMO");
        let start = std::time::Instant::now();
        let t = read(&bytes).unwrap();
        eprintln!("{} in {:?}", summary(&t), start.elapsed());
        assert!(t.crouch_downs.iter().map(Vec::len).sum::<usize>() > 100);
    }
}
