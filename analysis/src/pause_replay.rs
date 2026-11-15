//! Stretches of an HLTV demo that replay part of the match after a pause.
//!
//! When a KTP server pauses the match, the server sends `showpause 0` and the
//! HLTV proxy stops relaying. When the pause ends, the proxy plays out its
//! delay buffer again: the `D` seconds before the pause (the proxy's delay,
//! 60 s on the demos measured) are sent a second time, frame for frame,
//! while `svc_time` keeps counting up. Every kill, chat line and capture in
//! that stretch is in the file twice, and so the analysis counted it twice.
//!
//! Nothing in the file marks the replay's start or end besides the
//! `showpause 0` stufftext, so `D` is found from the data: the frames just
//! after the stufftext carry every player at exactly the positions an
//! earlier frame did, `D` seconds before. When no frame matches (a pause
//! with nobody moving, say), the first kill after the stufftext is matched
//! against the kills before it instead. No match means no replay.
//!
//! The analysis keeps decoding entity deltas inside a replay, so the delta
//! state stays right, but drops everything a replayed message would record
//! (`run_analyzers`). The replay is still in the file and still plays, so
//! the demo's times stay as the demo plays them.

use crate::PlayerPose;
use crate::entity_replay::EntityReplay;
use crate::fnv1a::Fnv1a;
use crate::time::GameTime;
use dem::types::{Demo, EngineMessage, Frame, FrameData, MessageData, NetMessage};
use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

/// The stufftext a KTP server sends when the match pauses.
const SEAM_COMMAND: &[u8] = b"showpause 0";
/// The longest replay looked for: an HLTV delay is set in seconds, and no
/// proxy delays by minutes.
const MAX_REPLAY_SECS: f32 = 300.0;
/// Below this, positions that match are a player standing still for a
/// moment, not a replay.
const MIN_REPLAY_SECS: f32 = 1.0;
/// How many frames after the stufftext are matched against earlier ones.
const SAMPLES_AFTER_SEAM: usize = 64;
/// How many of them must agree on one offset. On the two demos measured,
/// about nine in ten of the frames after the seam match.
const MIN_MATCHING_SAMPLES: usize = 10;
/// How many different snapshots those matches must cover, so that a freeze
/// time with everyone standing still is not taken for a replay.
const MIN_DISTINCT_SNAPSHOTS: usize = 5;
/// How far apart two offsets can be and still be one: a replayed frame's
/// `svc_time` is off its original's by a few milliseconds of frame jitter.
const OFFSET_TOLERANCE_SECS: f32 = 0.02;
/// A snapshot needs at least this many placed players to say anything.
const MIN_PLAYERS_PER_SNAPSHOT: usize = 2;

/// One stretch of the demo that replays the `offset` before a pause.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PauseReplay {
    /// The moment the match paused: `frame_index` is the first replayed
    /// frame, the times are those of the frame that carried the pause.
    pub start: GameTime,
    /// The first frame after the replay, where the match goes on from the
    /// pause. Its `viewdemo_offset` is `start`'s plus `offset`.
    pub end: GameTime,
    /// How much of the match is replayed, and how far each replayed moment
    /// is from its first showing.
    pub offset: Duration,
}

impl PauseReplay {
    /// Whether the frame with this 1-based index (`GameTime::frame_index`)
    /// is part of the replay.
    pub fn contains_frame(&self, frame_index: usize) -> bool {
        (self.start.frame_index..self.end.frame_index).contains(&frame_index)
    }
}

/// Whether any of `replays` holds the frame with this 1-based index.
pub(crate) fn is_replayed(replays: &[PauseReplay], frame_index: usize) -> bool {
    replays.iter().any(|r| r.contains_frame(frame_index))
}

/// Every player's position in one network frame, as a hash: two frames with
/// the same hash have every player at bit-identical origins.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PoseSample {
    pub svc_time: f32,
    pub poses: u64,
}

/// One kill, as a hash of who killed whom with what, from where.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct KillMark {
    pub svc_time: f32,
    pub identity: u64,
}

/// The replay length the frames after a pause show: the offset most of
/// `after` (the frames after the seam, oldest first) agree on to an
/// earlier frame in `before`, when enough of them agree. `None` when
/// they don't, which is also every demo without a replay.
pub(crate) fn offset_from_poses(
    seam_secs: f32,
    before: &[PoseSample],
    after: &[PoseSample],
) -> Option<f32> {
    let mut times_of: HashMap<u64, Vec<f32>> = HashMap::new();
    for sample in before {
        if sample.svc_time <= seam_secs && seam_secs - sample.svc_time <= MAX_REPLAY_SECS {
            times_of
                .entry(sample.poses)
                .or_default()
                .push(sample.svc_time);
        }
    }
    let after = &after[..after.len().min(SAMPLES_AFTER_SEAM)];
    let mut buckets: BTreeMap<i64, OffsetBucket> = BTreeMap::new();
    for (index, sample) in after.iter().enumerate() {
        for earlier in times_of.get(&sample.poses).into_iter().flatten() {
            let offset = sample.svc_time - earlier;
            if offset >= MIN_REPLAY_SECS {
                let bucket = buckets.entry(bucket_of(offset)).or_default();
                bucket.samples |= 1 << index;
                bucket.offset_sum += f64::from(offset);
                bucket.offsets += 1;
            }
        }
    }
    best_agreed_offset(&buckets, after)
}

/// The offsets that fall in one [`BUCKET_SECS`] slot, and which samples
/// after the seam gave them (bit `i` is `after[i]`; there are at most
/// [`SAMPLES_AFTER_SEAM`] of them).
#[derive(Default)]
struct OffsetBucket {
    samples: u64,
    offset_sum: f64,
    offsets: usize,
}

/// The width of one [`OffsetBucket`]; offsets within
/// [`OFFSET_TOLERANCE_SECS`] of a bucket are counted with it.
const BUCKET_SECS: f32 = 0.01;
const NEIGHBOUR_BUCKETS: i64 = (OFFSET_TOLERANCE_SECS / BUCKET_SECS) as i64;

fn bucket_of(offset: f32) -> i64 {
    (offset / BUCKET_SECS).round() as i64
}

/// The offset with the most samples behind it, counting each sample once,
/// when there are enough of them and they are not one snapshot standing
/// still. Returns their mean, which irons out the frame jitter.
fn best_agreed_offset(buckets: &BTreeMap<i64, OffsetBucket>, after: &[PoseSample]) -> Option<f32> {
    let mut best: Option<(u32, f32)> = None;
    for &centre in buckets.keys() {
        let near = (centre - NEIGHBOUR_BUCKETS..=centre + NEIGHBOUR_BUCKETS)
            .filter_map(|key| buckets.get(&key));
        let (mut samples, mut sum, mut count) = (0u64, 0.0f64, 0usize);
        for bucket in near {
            samples |= bucket.samples;
            sum += bucket.offset_sum;
            count += bucket.offsets;
        }
        let matched = samples.count_ones();
        if (matched as usize) < MIN_MATCHING_SAMPLES
            || distinct_poses(samples, after) < MIN_DISTINCT_SNAPSHOTS
        {
            continue;
        }
        if best.is_none_or(|(most, _)| matched > most) {
            best = Some((matched, (sum / count as f64) as f32));
        }
    }
    best.map(|(_, offset)| offset)
}

/// How many different snapshots the samples in the `samples` mask have.
fn distinct_poses(samples: u64, after: &[PoseSample]) -> usize {
    let mut poses: Vec<u64> = (0..after.len())
        .filter(|i| samples & (1 << i) != 0)
        .map(|i| after[i].poses)
        .collect();
    poses.sort_unstable();
    poses.dedup();
    poses.len()
}

/// The replay length the kills after a pause show: the first kill after
/// the seam is the same kill as one before it, and every kill up to the
/// replay's end has its twin the same distance back. `None` when no
/// earlier kill fits.
pub(crate) fn offset_from_kills(seam_secs: f32, kills: &[KillMark]) -> Option<f32> {
    let (before, after): (Vec<&KillMark>, Vec<&KillMark>) =
        kills.iter().partition(|k| k.svc_time <= seam_secs);
    let first = after.first()?;
    before
        .iter()
        .rev()
        .filter(|earlier| earlier.identity == first.identity)
        .map(|earlier| first.svc_time - earlier.svc_time)
        .filter(|offset| (MIN_REPLAY_SECS..=MAX_REPLAY_SECS).contains(offset))
        .find(|&offset| {
            after
                .iter()
                .take_while(|k| k.svc_time < seam_secs + offset)
                .all(|k| {
                    before.iter().any(|b| {
                        b.identity == k.identity
                            && (k.svc_time - offset - b.svc_time).abs() <= OFFSET_TOLERANCE_SECS
                    })
                })
        })
}

/// A network frame's place in the demo: its 1-based index among all frames
/// and its times once its `svc_time` is read.
#[derive(Clone, Copy, Debug)]
struct FrameStamp {
    frame_index: usize,
    svc_secs: f32,
    real_secs: f32,
}

/// What the walk over the demo collects for the matching.
#[derive(Default)]
struct Walk {
    stamps: Vec<FrameStamp>,
    samples: Vec<PoseSample>,
    kills: Vec<KillMark>,
    /// The stamp of each frame that carried the pause stufftext.
    seams: Vec<FrameStamp>,
}

/// Every pause replay in the demo, oldest first. Empty for a demo with no
/// `showpause 0`, which costs one look at each message.
pub(crate) fn find_replays(demo: &Demo) -> Vec<PauseReplay> {
    if !frames(demo).any(|frame| messages(frame).any(is_seam)) {
        return Vec::new();
    }
    let walk = walk(demo);
    let total_frames = frames(demo).count();
    let mut replays: Vec<PauseReplay> = Vec::new();
    for seam in &walk.seams {
        // A second pause inside a replay is the first one's echo.
        if is_replayed(&replays, seam.frame_index) {
            continue;
        }
        let Some(offset) = replay_offset(&walk, seam) else {
            continue;
        };
        replays.push(replay_at(&walk, seam, offset, total_frames));
    }
    replays
}

fn replay_offset(walk: &Walk, seam: &FrameStamp) -> Option<f32> {
    let split = walk
        .samples
        .partition_point(|s| s.svc_time <= seam.svc_secs);
    offset_from_poses(
        seam.svc_secs,
        &walk.samples[..split],
        &walk.samples[split..],
    )
    .or_else(|| offset_from_kills(seam.svc_secs, &walk.kills))
}

/// The replay after `seam`: from the frame after it to the first frame at
/// or past `seam + offset`, or past the demo's end when it stops inside.
fn replay_at(walk: &Walk, seam: &FrameStamp, offset: f32, total_frames: usize) -> PauseReplay {
    let end_secs = seam.svc_secs + offset;
    let end = walk
        .stamps
        .iter()
        .find(|s| s.frame_index > seam.frame_index && s.svc_secs >= end_secs);
    let game_time = |frame_index, svc_secs: f32, real_secs: f32| GameTime {
        real_offset: Duration::try_from_secs_f32(real_secs).unwrap_or_default(),
        viewdemo_offset: Duration::try_from_secs_f32(svc_secs).unwrap_or_default(),
        frame_index,
    };
    PauseReplay {
        start: game_time(seam.frame_index + 1, seam.svc_secs, seam.real_secs),
        end: match end {
            Some(s) => game_time(s.frame_index, end_secs, s.real_secs),
            None => game_time(total_frames + 1, end_secs, walk_end_secs(walk)),
        },
        offset: Duration::try_from_secs_f32(offset).unwrap_or_default(),
    }
}

fn walk_end_secs(walk: &Walk) -> f32 {
    walk.stamps.last().map(|s| s.real_secs).unwrap_or(0.0)
}

fn frames(demo: &Demo) -> impl Iterator<Item = &Frame> {
    demo.directory.entries.iter().flat_map(|e| &e.frames)
}

fn messages(frame: &Frame) -> impl Iterator<Item = &NetMessage> {
    let parsed = match &frame.frame_data {
        FrameData::NetworkMessage(bt) => match &bt.1.messages {
            MessageData::Parsed(msgs) => msgs.as_slice(),
            _ => &[],
        },
        _ => &[],
    };
    parsed.iter()
}

fn is_seam(message: &NetMessage) -> bool {
    let NetMessage::EngineMessage(engine) = message else {
        return false;
    };
    let EngineMessage::SvcStuffText(text) = engine.as_ref() else {
        return false;
    };
    let command = text.command.as_slice();
    let end = command
        .iter()
        .rposition(|b| !matches!(b, 0 | b'\n' | b' ' | b';'))
        .map_or(0, |i| i + 1);
    &command[..end] == SEAM_COMMAND
}

/// One pass over the demo collecting every frame's player positions, every
/// kill and every pause stufftext. Entity snapshots are replayed the way
/// `position.rs` does, on their own copy.
fn walk(demo: &Demo) -> Walk {
    let mut walk = Walk::default();
    let mut players: EntityReplay<PlayerPose> = EntityReplay::default();
    let mut svc_secs = 0.0f32;
    for (offset, frame) in frames(demo).enumerate() {
        let FrameData::NetworkMessage(bt) = &frame.frame_data else {
            continue;
        };
        players.begin_frame(bt.1.sequence_info.incoming_sequence);
        let mut stamp = FrameStamp {
            frame_index: offset + 1,
            svc_secs,
            real_secs: frame.time,
        };
        for message in messages(frame) {
            match message {
                NetMessage::EngineMessage(engine) => {
                    match engine.as_ref() {
                        EngineMessage::SvcTime(t) => {
                            svc_secs = t.time;
                            stamp.svc_secs = t.time;
                        }
                        EngineMessage::SvcServerInfo(info) => {
                            players = EntityReplay::new(1..=u16::from(info.max_players.max(1)))
                        }
                        _ => {}
                    }
                    if is_seam(message) {
                        walk.seams.push(stamp);
                    }
                    players.apply(engine);
                }
                NetMessage::UserMessage(user) => {
                    if user.name.as_slice().starts_with(b"DeathMsg") {
                        walk.kills.push(KillMark {
                            svc_time: svc_secs,
                            identity: kill_identity(&user.data, &players),
                        });
                    }
                }
            }
        }
        walk.stamps.push(stamp);
        if let Some(poses) = pose_hash(&players) {
            walk.samples.push(PoseSample {
                svc_time: svc_secs,
                poses,
            });
        }
    }
    walk
}

/// Every placed player's index and origin, hashed; `None` when too few
/// players are placed to tell one moment from another.
fn pose_hash(players: &EntityReplay<PlayerPose>) -> Option<u64> {
    let mut hash = Fnv1a::new();
    let mut placed = 0;
    for (index, pose) in players.entities() {
        let Some(origin) = pose.known_origin() else {
            continue;
        };
        hash.update(&index.to_le_bytes());
        for axis in origin {
            hash.update(&axis.to_bits().to_le_bytes());
        }
        placed += 1;
    }
    (placed >= MIN_PLAYERS_PER_SNAPSHOT).then(|| hash.finish())
}

/// A `DeathMsg`'s bytes (killer, victim, weapon) and both players' origins,
/// hashed: a replayed kill has all of them the same as its first showing.
fn kill_identity(data: &[u8], players: &EntityReplay<PlayerPose>) -> u64 {
    let mut hash = Fnv1a::new();
    hash.update(data);
    for &client_index in data.iter().take(2) {
        let origin = players
            .get(u16::from(client_index))
            .and_then(PlayerPose::known_origin);
        for axis in origin.unwrap_or_default() {
            hash.update(&axis.to_bits().to_le_bytes());
        }
    }
    hash.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stream of distinct snapshots, one every 10 ms from `from_secs`,
    /// numbered from `first_pose`.
    fn moving(from_secs: f32, count: usize, first_pose: u64) -> Vec<PoseSample> {
        (0..count)
            .map(|i| PoseSample {
                svc_time: from_secs + i as f32 * 0.01,
                poses: first_pose + i as u64,
            })
            .collect()
    }

    /// The match from 0 s to `seam`, then a replay of the `delay` before
    /// it, then play that goes on from the pause.
    fn paused(seam: f32, delay: f32) -> (Vec<PoseSample>, Vec<PoseSample>) {
        let frames_before = (seam * 100.0) as usize;
        let before = moving(0.0, frames_before, 0);
        let replayed_from = frames_before - (delay * 100.0) as usize;
        let mut after: Vec<PoseSample> = before[replayed_from..]
            .iter()
            .map(|s| PoseSample {
                svc_time: s.svc_time + delay + 0.003,
                ..*s
            })
            .collect();
        after.extend(moving(seam + delay + 0.01, 100, 1_000_000));
        (before, after)
    }

    #[test]
    fn a_replay_after_a_pause_is_found_at_its_offset() {
        let (before, after) = paused(120.0, 60.0);
        let offset = offset_from_poses(120.0, &before, &after).expect("a replay");
        assert!((offset - 60.003).abs() < 0.001, "{offset}");
    }

    #[test]
    fn each_pause_finds_its_own_replay_length() {
        // A second pause later in the match, behind a 30 s delay: what
        // came before it, the first replay included, is still history.
        let (mut history, after_first) = paused(120.0, 60.0);
        history.extend(after_first);
        let seam = history.last().unwrap().svc_time;
        let replayed: Vec<PoseSample> = history
            .iter()
            .filter(|s| s.svc_time > seam - 30.0)
            .map(|s| PoseSample {
                svc_time: s.svc_time + 30.0,
                ..*s
            })
            .collect();
        let offset = offset_from_poses(seam, &history, &replayed).expect("a replay");
        assert!((offset - 30.0).abs() < 0.001, "{offset}");
    }

    #[test]
    fn play_that_goes_on_after_the_seam_is_no_replay() {
        let before = moving(0.0, 12_000, 0);
        let after = moving(120.0, 200, 1_000_000);
        assert_eq!(offset_from_poses(120.0, &before, &after), None);
    }

    #[test]
    fn everyone_standing_still_is_no_replay() {
        // A freeze time: the same snapshot before and after the seam.
        let still = |from: f32| -> Vec<PoseSample> {
            (0..500)
                .map(|i| PoseSample {
                    svc_time: from + i as f32 * 0.01,
                    poses: 7,
                })
                .collect()
        };
        assert_eq!(offset_from_poses(100.0, &still(90.0), &still(100.01)), None);
    }

    fn kill(svc_time: f32, identity: u64) -> KillMark {
        KillMark { svc_time, identity }
    }

    #[test]
    fn a_replay_with_no_matching_positions_is_found_by_its_kills() {
        let kills = [
            kill(50.0, 1),
            kill(75.0, 2),
            kill(80.0, 3),
            kill(99.0, 4),
            // Pause at 100 s, 60 s replayed.
            kill(135.01, 2),
            kill(140.02, 3),
            kill(159.0, 4),
            kill(170.0, 5),
        ];
        let offset = offset_from_kills(100.0, &kills).expect("a replay");
        assert!((offset - 60.01).abs() < 0.001, "{offset}");
    }

    #[test]
    fn a_kill_after_the_pause_that_happened_once_is_no_replay() {
        let kills = [kill(50.0, 1), kill(75.0, 2), kill(130.0, 9)];
        assert_eq!(offset_from_kills(100.0, &kills), None);
    }

    #[test]
    fn a_repeat_of_one_kill_without_its_neighbours_is_no_replay() {
        // The same pair and weapon again, but the next kill in the would-be
        // replay has no twin: a coincidence, not a replay.
        let kills = [kill(75.0, 2), kill(135.0, 2), kill(140.0, 8)];
        assert_eq!(offset_from_kills(100.0, &kills), None);
    }

    /// The KTP HLTV demo the replay was found in: paused at 13:25 (805 s),
    /// replaying 745-805 s, with 18 kills in it. `DOD_PAUSE_REPLAY_DEMO`, or
    /// the PRE install's copy. Ignored because CI has no such demo:
    /// `cargo test -p analysis -- --ignored the_armory_demos_replayed_kills_count_once`.
    #[test]
    #[ignore]
    fn the_armory_demos_replayed_kills_count_once() {
        let path = std::env::var_os("DOD_PAUSE_REPLAY_DEMO")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(
                    r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - PRE-Anniversary for Movies\dod\ktps9qf-dice_clinic_m1_armory_h1_hltv.dem",
                )
            });
        let bytes = std::fs::read(&path).unwrap_or_else(|e| {
            panic!(
                "no demo at {} ({e}): set DOD_PAUSE_REPLAY_DEMO",
                path.display()
            )
        });
        let analysis = crate::Analysis::try_from_bytes(&bytes).expect("the demo parses");
        let state = &analysis.state;

        assert_eq!(state.pause_replays.len(), 1, "{:?}", state.pause_replays);
        let replay = &state.pause_replays[0];
        let secs = |d: Duration| d.as_secs_f32();
        assert!((secs(replay.offset) - 60.0).abs() < 0.05, "{replay:?}");
        assert!((secs(replay.start.viewdemo_offset) - 805.0).abs() < 0.05);

        let kills_in = |from: f32, to: f32| {
            state
                .kill_positions
                .iter()
                .filter(|k| (from..to).contains(&secs(k.time.viewdemo_offset)))
                .count()
        };
        assert_eq!(kills_in(745.0, 805.0), 18, "the first showing is kept");
        assert_eq!(
            state.kill_positions.len(),
            303,
            "321 counted before, 18 of them twice"
        );
        assert_eq!(kills_in(805.0, 865.0), 0, "the replay adds none");
        // No kill anywhere is a copy of another one the replay's length back.
        for kill in &state.kill_positions {
            let twin = state.kill_positions.iter().find(|other| {
                other.killer == kill.killer
                    && other.victim == kill.victim
                    && other.weapon == kill.weapon
                    && other.victim_origin == kill.victim_origin
                    && (secs(kill.time.viewdemo_offset)
                        - secs(other.time.viewdemo_offset)
                        - secs(replay.offset))
                    .abs()
                        < 0.05
            });
            assert!(twin.is_none(), "{kill:?} repeats {twin:?}");
        }
    }

    #[test]
    fn a_replay_holds_its_frames_and_not_the_one_after() {
        let replay = PauseReplay {
            start: GameTime {
                frame_index: 10,
                ..Default::default()
            },
            end: GameTime {
                frame_index: 20,
                ..Default::default()
            },
            offset: Duration::from_secs(60),
        };
        assert!(!replay.contains_frame(9));
        assert!(replay.contains_frame(10));
        assert!(replay.contains_frame(19));
        assert!(!replay.contains_frame(20));
    }
}
