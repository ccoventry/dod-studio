//! Trims a demo so no snapshot carries more packet entities than the engine
//! takes (#207).
//!
//! `CL_ParsePacketEntities` closes the game at the first snapshot over
//! `MAX_PACKET_ENTITIES` (256 on the pre-Anniversary `hw.dll`, 1024 on the
//! 25th Anniversary one, `sys::packet_limit`). `dod_lennon2` HLTV demos carry
//! 276-303 entities in every snapshot from the first, so the pre-Anniversary
//! engine dies 1.3 s in. Trimming the map can't help a recorded demo (its
//! entities are baked into the packets, #207's comments), so this trims the
//! packets: it drops one fixed set of safe static entities from every
//! snapshot ([`select`]), and re-encodes the packets around the gap
//! ([`rewrite`]). The R&D probe it comes from, PR #1184, played two trimmed
//! WSOD25 HLTV demos on pre-Anniversary past 600-1200 s.
//!
//! Frames are never added, removed or reordered, and only packet-entities
//! messages change, so every tick, demo-player time and highlight of the
//! original holds for the trimmed copy.
//!
//! Pure: bytes in, bytes out. `patch::trimmed_copies` decides when to trim and
//! where the copy goes.

mod carry;
mod map_props;
mod rewrite;
mod select;

use std::collections::HashSet;

use dem::types::{Demo, FrameData};

pub use map_props::MapProps;
pub use select::{Dropped, Tier};

/// How far under the engine's limit a trim aims, so the busiest moment of a
/// demo isn't one entity from the crash.
pub const HEADROOM_ENTITIES: u32 = 10;

/// The most entities a trimmed snapshot may hold for an engine whose limit
/// is `limit`.
pub fn target_for(limit: u32) -> u32 {
    limit.saturating_sub(HEADROOM_ENTITIES)
}

/// A demo trimmed under its target.
#[derive(Debug, Clone)]
pub struct Trimmed {
    pub bytes: Vec<u8>,
    /// The most entities any snapshot held, before and after.
    pub peak_before: u32,
    pub peak_after: u32,
    /// The entities dropped from every snapshot.
    pub dropped: Vec<Dropped>,
}

impl Trimmed {
    /// How many of [`Trimmed::dropped`] are of `tier`.
    pub fn count_of(&self, tier: Tier) -> usize {
        self.dropped.iter().filter(|d| d.tier == tier).count()
    }
}

/// What [`trim_demo`] found.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// No snapshot is over the target: the original plays as it is.
    Unneeded {
        peak: u32,
    },
    Trimmed(Trimmed),
}

/// Why a demo could not be trimmed. Each says it in words a player reads.
#[derive(Debug, Clone, PartialEq)]
pub enum TrimError {
    /// The demo would not parse.
    Unreadable(String),
    /// There are not enough entities that are safe to drop.
    NotEnough {
        peak: u32,
        target: u32,
        available: usize,
        /// The map's `.bsp` was not there to read, so its decorative props
        /// could not be told apart from walls.
        without_map: bool,
    },
    /// The trimmed demo did not pass the check it is put through before
    /// anything plays it.
    CheckFailed(String),
    Cancelled,
}

impl std::fmt::Display for TrimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrimError::Unreadable(why) => write!(f, "the demo could not be read: {why}"),
            TrimError::NotEnough {
                peak,
                target,
                available,
                without_map,
            } => {
                write!(
                    f,
                    "it has up to {peak} entities in one snapshot, and only {available} of them are safe to leave out, \
                     not the {} it would take to get under {target}",
                    peak.saturating_sub(*target)
                )?;
                if *without_map {
                    write!(
                        f,
                        " (its map's .bsp wasn't found in the game's maps folder, so its decorations couldn't be counted)"
                    )?;
                }
                Ok(())
            }
            TrimError::CheckFailed(why) => {
                write!(
                    f,
                    "the trimmed copy failed its check ({why}), so it isn't used"
                )
            }
            TrimError::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// Where a trim is: reading the demo, writing the copy, or reading it back to
/// check it, with how far through that step it is (0.0-1.0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Step {
    Reading(f32),
    Writing,
    Checking(f32),
}

/// What [`trim_demo`] needs besides the demo.
pub struct TrimRequest<'a> {
    /// The engine's limit: a demo with no snapshot over it is left alone.
    pub limit: u32,
    /// The most entities a snapshot may hold afterwards ([`target_for`] of
    /// the limit).
    pub target: u32,
    /// The demo's map, for the decorative props; `None` leaves them out.
    pub map: Option<&'a MapProps>,
    pub cancel: &'a dyn Fn() -> bool,
    pub progress: &'a mut dyn FnMut(Step),
}

/// Trims `bytes` so no snapshot holds more than `request.target` entities,
/// or says it needn't: no snapshot holds more than `request.limit`. The trimmed demo is read back and checked before it is
/// returned: it parses, every snapshot is at or under the target, no dropped
/// entity is still listed, every declared count agrees with the snapshot the
/// engine builds wherever the original's did, every entry that takes its
/// baseline from an earlier entry still names the same entity, and its frames
/// are the original's, in order.
pub fn trim_demo(bytes: &[u8], request: TrimRequest<'_>) -> Result<Outcome, TrimError> {
    let TrimRequest {
        limit,
        target,
        map,
        cancel,
        progress,
    } = request;
    let mut demo = dem::open_demo_from_bytes_with_progress(bytes, &mut |read, total| {
        progress(Step::Reading(read as f32 / total.max(1) as f32));
    })
    .map_err(|e| TrimError::Unreadable(e.to_string()))?;
    let before = rewrite::measure(&mut demo, &HashSet::new());
    if before.peak_before <= limit {
        return Ok(Outcome::Unneeded {
            peak: before.peak_before,
        });
    }
    if cancel() {
        return Err(TrimError::Cancelled);
    }
    let need = (before.peak_before - target) as usize;
    let mut dropped = select::Survey::of(&demo).candidates(map);
    if dropped.len() < need {
        return Err(TrimError::NotEnough {
            peak: before.peak_before,
            target,
            available: dropped.len(),
            without_map: map.is_none(),
        });
    }
    dropped.truncate(need);
    let dropping: HashSet<u16> = dropped.iter().map(|d| d.index).collect();
    let frames = frame_order(&demo);
    rewrite::apply(&mut demo, &dropping);
    progress(Step::Writing);
    let out = demo
        .write_to_bytes_reusing_source_cancellable(bytes, cancel)
        .ok_or(TrimError::Cancelled)?;
    // The parsed original goes before the copy is parsed, so the two (about
    // 12 times the file's size each) are never in memory together.
    drop(demo);
    let after = check(&out, &dropping, &frames, &before, target, progress)?;
    Ok(Outcome::Trimmed(Trimmed {
        bytes: out,
        peak_before: before.peak_before,
        peak_after: after.peak_before,
        dropped,
    }))
}

/// Every frame's kind, time and frame number, entry by entry.
type FrameOrder = Vec<Vec<(std::mem::Discriminant<FrameData>, u32, i32)>>;

fn frame_order(demo: &Demo) -> FrameOrder {
    demo.directory
        .entries
        .iter()
        .map(|e| {
            e.frames
                .iter()
                .map(|f| {
                    (
                        std::mem::discriminant(&f.frame_data),
                        f.time.to_bits(),
                        f.frame,
                    )
                })
                .collect()
        })
        .collect()
}

/// Reads the trimmed demo back and checks it (see [`trim_demo`]). Returns
/// its measure.
fn check(
    out: &[u8],
    dropped: &HashSet<u16>,
    frames: &FrameOrder,
    before: &rewrite::Tally,
    target: u32,
    progress: &mut dyn FnMut(Step),
) -> Result<rewrite::Tally, TrimError> {
    let mut again = dem::open_demo_from_bytes_with_progress(out, &mut |read, total| {
        progress(Step::Checking(read as f32 / total.max(1) as f32));
    })
    .map_err(|e| TrimError::CheckFailed(format!("it doesn't read back: {e}")))?;
    let after = rewrite::measure(&mut again, dropped);
    let failed = |why: String| Err(TrimError::CheckFailed(why));
    if frame_order(&again) != *frames {
        return failed("its frames aren't the original's".into());
    }
    if after.packets != before.packets {
        return failed(format!(
            "{} entity packets read back, the original has {}",
            after.packets, before.packets
        ));
    }
    if after.peak_before > target {
        return failed(format!(
            "a snapshot still holds {} entities",
            after.peak_before
        ));
    }
    if after.dropped_still_listed > 0 {
        return failed(format!(
            "{} entries of dropped entities are still listed",
            after.dropped_still_listed
        ));
    }
    if after.count_mismatch > before.count_mismatch {
        return failed(format!(
            "{} snapshots declare a size the engine wouldn't build, the original {}",
            after.count_mismatch, before.count_mismatch
        ));
    }
    if !offsets_agree(&before.offset_bases, &after.offset_bases, dropped) {
        return failed(
            "an entity takes its baseline from a different entity than in the original".into(),
        );
    }
    Ok(after)
}

/// Whether every kept entry that bases on an earlier entry of its packet
/// names the same entity in the copy as in the original (`rewrite`'s module
/// doc), and no other entry does.
fn offsets_agree(before: &[(u16, u16)], after: &[(u16, u16)], dropped: &HashSet<u16>) -> bool {
    before
        .iter()
        .filter(|(entity, _)| !dropped.contains(entity))
        .eq(after.iter())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dem::types::EngineMessage;

    fn fixture() -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("test-fixtures")
            .join("ci_fixture.dem");
        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    fn trim(bytes: &[u8], limit: u32, cancel: &dyn Fn() -> bool) -> Result<Outcome, TrimError> {
        let mut progress = |_| {};
        trim_demo(
            bytes,
            TrimRequest {
                limit,
                target: limit,
                map: None,
                cancel,
                progress: &mut progress,
            },
        )
    }

    fn peak(bytes: &[u8]) -> u32 {
        let mut demo = dem::open_demo_from_bytes(bytes).unwrap();
        rewrite::measure(&mut demo, &HashSet::new()).peak_before
    }

    /// How many times each entity is listed, across every packet.
    fn listings(demo: &Demo) -> std::collections::BTreeMap<u16, usize> {
        let mut n = std::collections::BTreeMap::new();
        for em in select::engine_messages(demo) {
            let indices: Vec<u16> = match em {
                EngineMessage::SvcPacketEntities(pe) => {
                    pe.entity_states.iter().map(|e| e.entity_index).collect()
                }
                EngineMessage::SvcDeltaPacketEntities(d) => {
                    d.entity_states.iter().map(|e| e.entity_index).collect()
                }
                _ => Vec::new(),
            };
            for i in indices {
                *n.entry(i).or_default() += 1;
            }
        }
        n
    }

    /// The first full snapshot's five highest non-player entities: real,
    /// moving map entities on the POV fixture, which has no static ones.
    fn some_map_entities(demo: &Demo) -> HashSet<u16> {
        let first = select::engine_messages(demo)
            .find_map(|em| match em {
                EngineMessage::SvcPacketEntities(pe) => Some(pe),
                _ => None,
            })
            .expect("the fixture has a full snapshot");
        let mut indices: Vec<u16> = first
            .entity_states
            .iter()
            .map(|e| e.entity_index)
            .filter(|&i| i > 32)
            .collect();
        indices.sort_unstable();
        indices.into_iter().rev().take(5).collect()
    }

    /// Drops `some_map_entities` from the fixture and checks the result the
    /// way `trim_demo` does. Returns the original, the copy's bytes and the
    /// drop set.
    fn rewrite_fixture() -> (Vec<u8>, Vec<u8>, HashSet<u16>) {
        let bytes = fixture();
        let mut demo = dem::open_demo_from_bytes(&bytes).unwrap();
        let before = rewrite::measure(&mut demo, &HashSet::new());
        let dropping = some_map_entities(&demo);
        let frames = frame_order(&demo);
        let applied = rewrite::apply(&mut demo, &dropping);
        assert!(applied.entries_dropped > 0);
        let out = demo
            .write_to_bytes_reusing_source_cancellable(&bytes, &|| false)
            .unwrap();
        check(
            &out,
            &dropping,
            &frames,
            &before,
            before.peak_before,
            &mut |_| {},
        )
        .expect("the rewritten fixture passes the check");
        (bytes, out, dropping)
    }

    #[test]
    fn a_demo_under_the_limit_is_left_alone() {
        let bytes = fixture();
        let outcome = trim(&bytes, peak(&bytes), &|| false).unwrap();
        assert!(matches!(outcome, Outcome::Unneeded { .. }));
    }

    #[test]
    fn a_rewrite_keeps_every_frame_in_order() {
        let (bytes, out, _) = rewrite_fixture();
        let original = dem::open_demo_from_bytes(&bytes).unwrap();
        let trimmed = dem::open_demo_from_bytes(&out).unwrap();
        assert_eq!(frame_order(&original), frame_order(&trimmed));
    }

    #[test]
    fn a_rewrite_drops_every_entry_of_its_entities_and_no_other() {
        let (bytes, out, dropping) = rewrite_fixture();
        let mut want = listings(&dem::open_demo_from_bytes(&bytes).unwrap());
        want.retain(|i, _| !dropping.contains(i));
        assert_eq!(listings(&dem::open_demo_from_bytes(&out).unwrap()), want);
    }

    #[test]
    fn a_rewrite_keeps_every_declared_count_true_to_the_snapshot() {
        let (bytes, out, _) = rewrite_fixture();
        let mut original = dem::open_demo_from_bytes(&bytes).unwrap();
        let mut trimmed = dem::open_demo_from_bytes(&out).unwrap();
        let before = rewrite::measure(&mut original, &HashSet::new());
        let after = rewrite::measure(&mut trimmed, &HashSet::new());
        assert_eq!(after.packets, before.packets);
        assert_eq!(after.count_mismatch, before.count_mismatch);
        assert!(after.peak_before < before.peak_before);
    }

    #[test]
    fn too_few_safe_entities_is_an_error() {
        // A POV demo's entities all move or leave its view, so none is safe.
        let bytes = fixture();
        let err = trim(&bytes, peak(&bytes) - 1, &|| false).unwrap_err();
        assert!(
            matches!(
                err,
                TrimError::NotEnough {
                    available: 0,
                    without_map: true,
                    ..
                }
            ),
            "{err:?}"
        );
        assert!(err.to_string().contains("safe to leave out"));
    }

    #[test]
    fn a_cancelled_trim_returns_cancelled() {
        let bytes = fixture();
        let err = trim(&bytes, peak(&bytes) - 1, &|| true).unwrap_err();
        assert_eq!(err, TrimError::Cancelled);
    }

    #[test]
    fn the_check_rejects_an_entry_basing_on_another_entity_than_before() {
        let dropped = HashSet::from([40]);
        let before = [(50, 2), (40, 2), (60, 50)];
        assert!(offsets_agree(&before, &[(50, 2), (60, 50)], &dropped));
        assert!(!offsets_agree(&before, &[(50, 41), (60, 50)], &dropped));
        assert!(
            !offsets_agree(&before, &[(60, 50)], &dropped),
            "a base lost"
        );
    }

    #[test]
    fn the_target_leaves_headroom_under_the_limit() {
        assert_eq!(target_for(256), 246);
        assert_eq!(target_for(1024), 1014);
    }

    /// A real over-limit HLTV demo, trimmed end to end against its map (#207):
    /// `DOD_TRIM_DEMO`, else `wsod25_grp3_h1_hltv.dem` in the pre-Anniversary
    /// for Movies install's `dod\` folder; the map from that folder's `maps\`
    /// (or `DOD_TRIM_MAP`).
    #[test]
    #[ignore]
    fn a_dod_lennon2_hltv_demo_trims_under_the_pre_anniversary_limit() {
        let dod = std::path::Path::new(
            "C:/Program Files (x86)/Steam/steamapps/common/Half-Life - PRE-Anniversary for Movies/dod",
        );
        let demo = std::env::var_os("DOD_TRIM_DEMO")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| dod.join("wsod25_grp3_h1_hltv.dem"));
        let map = std::env::var_os("DOD_TRIM_MAP")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| dod.join("maps").join("dod_lennon2.bsp"));
        let bytes = std::fs::read(&demo).unwrap_or_else(|e| {
            panic!(
                "no demo to trim at {} ({e}): set DOD_TRIM_DEMO",
                demo.display()
            )
        });
        let map = MapProps::from_file(&map).expect("the demo's map: set DOD_TRIM_MAP");
        let started = std::time::Instant::now();
        let mut progress = |_| {};
        let outcome = trim_demo(
            &bytes,
            TrimRequest {
                limit: 256,
                target: target_for(256),
                map: Some(&map),
                cancel: &|| false,
                progress: &mut progress,
            },
        )
        .unwrap();
        let Outcome::Trimmed(t) = outcome else {
            panic!("{} is under the limit", demo.display());
        };
        eprintln!(
            "{}: {} -> {}, {} dropped ({} invisible, {} sprites, {} decorations), {:.1} s",
            demo.display(),
            t.peak_before,
            t.peak_after,
            t.dropped.len(),
            t.count_of(Tier::Invisible),
            t.count_of(Tier::Sprite),
            t.count_of(Tier::Decoration),
            started.elapsed().as_secs_f32()
        );
        assert!(t.peak_after <= target_for(256));
        assert_eq!(peak(&t.bytes), t.peak_after);
    }
}
