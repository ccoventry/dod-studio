//! Repairs of real demos: `test-fixtures/ci_fixture.dem` damaged in memory
//! (runs in CI), and the four damaged Monday demos of #15 (`#[ignore]`d).

use super::*;
use dem::types::FrameData;

fn repair(bytes: &[u8]) -> Result<RepairOutcome, RepairError> {
    no_hooks(|hooks| repair_demo(bytes, hooks))
}

/// Every playback frame's (time, ordinal), in order.
fn playback_frames(bytes: &[u8]) -> Vec<(f32, i32)> {
    let demo = dem::open_demo_from_bytes(bytes).expect("parses");
    demo.directory
        .entries
        .iter()
        .skip(1)
        .flat_map(|e| e.frames.iter())
        .filter(|f| !matches!(f.frame_data, FrameData::NextSection))
        .map(|f| (f.time, f.frame))
        .collect()
}

/// The fixture with `len` bytes from `at` overwritten by rubble that no frame
/// header makes sense of (a type byte of 0xEE).
fn damaged_fixture(at: usize, len: usize) -> Vec<u8> {
    let mut bytes = crate::tests_fixture::fixture();
    bytes[at..at + len].fill(0xEE);
    bytes
}

#[test]
fn a_healthy_demo_is_not_damaged() {
    let outcome = repair(&crate::tests_fixture::fixture()).expect("repairs");
    assert_eq!(outcome, RepairOutcome::NotDamaged);
}

#[test]
fn a_demo_with_a_hole_is_repaired_and_keeps_the_frames_around_it() {
    let original = crate::tests_fixture::fixture();
    // Mid-file, well clear of the signon and the end: the fixture is ~10.9 MB.
    let (at, len) = (original.len() / 2, 300_000);
    let outcome = repair(&damaged_fixture(at, len)).expect("repairs");
    let RepairOutcome::Repaired { bytes, report } = outcome else {
        panic!("not repaired: {outcome:?}");
    };
    assert_eq!(report.holes.len(), 1, "{report:?}");
    let hole = report.holes[0];
    assert!(hole.lost_secs > 0.0 && hole.lost_secs < 120.0, "{hole:?}");
    assert!(hole.bytes_skipped as usize >= len, "{hole:?}");
    assert_eq!(report.checks.parse_error, None);
    assert_eq!(report.checks.discarded_packets, 0);
    assert_eq!(report.checks.count_disagreements, 0);
    assert!(
        report
            .joins
            .iter()
            .all(|j| j.snapshot_placed && j.snapshot_entities > 0)
    );
    assert!(report.percent_kept() > 90.0 && report.percent_kept() < 100.0);

    // Every original frame before the hole and after it is still there, in
    // order; only the ramp's frames are new.
    let before: Vec<(f32, i32)> = playback_frames(&original)
        .into_iter()
        .filter(|(t, _)| *t <= hole.before_secs - 1.0 || *t >= hole.after_secs + 1.0)
        .collect();
    let repaired = playback_frames(&bytes);
    let mut it = repaired.iter();
    for frame in &before {
        assert!(it.any(|r| r == frame), "frame {frame:?} lost");
    }
}

/// Each network frame's `svc_time` (the server clock), in order.
fn server_clock(bytes: &[u8]) -> Vec<(f32, f32)> {
    use dem::types::{EngineMessage, MessageData, NetMessage};
    let demo = dem::open_demo_from_bytes(bytes).expect("parses");
    demo.directory
        .entries
        .iter()
        .skip(1)
        .flat_map(|e| e.frames.iter())
        .filter_map(|f| match &f.frame_data {
            FrameData::NetworkMessage(n) => match &n.1.messages {
                MessageData::Parsed(msgs) => msgs.iter().find_map(|m| match m {
                    NetMessage::EngineMessage(em) => match &**em {
                        EngineMessage::SvcTime(t) => Some((f.time, t.time)),
                        _ => None,
                    },
                    _ => None,
                }),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// `viewdemo` plays the world by the server clock (`svc_time`), which runs
/// ahead of the frame time by however long the server had been up when the
/// recording started (about 300 s in the fixture, 10.6 s in `m1_h2`). The
/// join's ramp frames were stamped with frame times, taking the clock back
/// that far at every hole, and `viewdemo` left the map (#15).
#[test]
fn a_repair_keeps_the_server_clock_rising_through_each_join() {
    let original = crate::tests_fixture::fixture();
    let (at, len) = (original.len() / 2, 300_000);
    let outcome = repair(&damaged_fixture(at, len)).expect("repairs");
    let original = server_clock(&original);
    let RepairOutcome::Repaired { bytes, report } = outcome else {
        panic!("not repaired: {outcome:?}");
    };
    assert_eq!(report.joins.len(), 1, "{report:?}");
    // How far the original's clock ever steps back between two frames.
    let worst_step = |clock: &[(f32, f32)]| {
        clock
            .windows(2)
            .map(|w| w[1].1 - w[0].1)
            .fold(0.0f32, f32::min)
    };
    let repaired = server_clock(&bytes);
    assert!(
        worst_step(&repaired) >= worst_step(&original) - 0.1,
        "the server clock steps back {} s in the repair, {} s in the original",
        -worst_step(&repaired),
        -worst_step(&original)
    );
}

/// The frame right before a hole carries a time far ahead of the recording
/// around it (a bit flip in its header, as at `m1_h2`'s third hole): the
/// search for where the recording resumes must not wait for that time, or it
/// never finds the resume and everything after the hole is lost.
#[test]
fn a_damaged_time_right_before_a_hole_does_not_hide_the_resume() {
    let original = crate::tests_fixture::fixture();
    let mut bytes = damaged_fixture(original.len() / 2, 300_000);
    let end = frame_walk::frames_end(&bytes);
    let (starts, _) = frame_walk::frame_starts(&bytes, frame_walk::DEMO_HEADER_SIZE, end);
    let last = *starts.last().expect("frames before the hole");
    bytes[last + 1..last + 5].copy_from_slice(&50_000f32.to_le_bytes());
    let outcome = repair(&bytes).expect("repairs");
    let RepairOutcome::Repaired { report, .. } = outcome else {
        panic!("not repaired: {outcome:?}");
    };
    assert!(!report.ends_at_last_hole, "{report:?}");
    assert_eq!(report.holes.len(), 1, "{report:?}");
    // The hole is the 300 KB of rubble and the frame it cut through, as
    // without the damaged time: the recording resumes right after it.
    let hole = report.holes[0];
    assert!(hole.bytes_skipped < 310_000, "{hole:?}");
    assert!(hole.lost_secs < 10.0, "{hole:?}");
}

/// Every playback frame whose view (origin, angles, health) is out of range:
/// (time, what).
fn views_out_of_range(bytes: &[u8]) -> Vec<(f32, &'static str)> {
    let demo = dem::open_demo_from_bytes(bytes).expect("parses");
    demo.directory
        .entries
        .iter()
        .skip(1)
        .flat_map(|e| e.frames.iter())
        .filter_map(|f| match &f.frame_data {
            FrameData::NetworkMessage(n) if !view_range::info_in_range(&n.1.info) => {
                Some((f.time, "network"))
            }
            FrameData::ClientData(c) if !view_range::client_data_in_range(c) => {
                Some((f.time, "client data"))
            }
            _ => None,
        })
        .collect()
}

/// Damage that reaches the bodies of the first frames after a hole, headers
/// intact, as in `m3_h2`: the first network frame after it holds a view
/// origin of 6.3e10, a yaw of 1.1e12 and a health of -2147483548, and a
/// client-data frame a yaw of 4.9e21. Every ramp frame copied that view, and
/// the screen was black for the whole cut (#15).
#[test]
fn a_damaged_view_at_the_edge_of_a_hole_reaches_no_written_frame() {
    // Offsets into a frame's body: a network frame's `demo_info` opens with
    // its timestamp, then `ref_params` (view origin at 0, client view angles
    // at 128, health at 140); a client-data frame's origin, then its angles.
    const VIEW_ORIGIN_Y: usize = 4 + 4;
    const CLIENT_YAW: usize = 4 + 128 + 4;
    const HEALTH: usize = 4 + 140;
    const CLIENT_DATA_YAW: usize = 12 + 4;
    const BODY: usize = 9;
    let original = crate::tests_fixture::fixture();
    let mut bytes = damaged_fixture(original.len() / 2, 300_000);
    let bridged = bridge::bridge(
        &bytes,
        bridge::BridgeHooks {
            progress: &mut |_| {},
            is_cancelled: &|| false,
        },
    )
    .expect("bridges");
    let resume = bridged.cuts[0].resume;
    let end = frame_walk::frames_end(&bytes);
    let (starts, _) = frame_walk::frame_starts(&bytes, resume, end);
    let mut network_damaged = 0;
    let mut client_data_damaged = 0;
    for &at in starts.iter().take(16) {
        let body = at + BODY;
        match bytes[at] {
            0 | 1 if network_damaged < 2 => {
                bytes[body + VIEW_ORIGIN_Y..][..4].copy_from_slice(&6.3e10f32.to_le_bytes());
                bytes[body + CLIENT_YAW..][..4].copy_from_slice(&1.1e12f32.to_le_bytes());
                bytes[body + HEALTH..][..4].copy_from_slice(&(-2_147_483_548i32).to_le_bytes());
                network_damaged += 1;
            }
            4 if client_data_damaged < 1 => {
                bytes[body + CLIENT_DATA_YAW..][..4].copy_from_slice(&4.9e21f32.to_le_bytes());
                client_data_damaged += 1;
            }
            _ => {}
        }
    }
    assert_eq!((network_damaged, client_data_damaged), (2, 1));
    let outcome = repair(&bytes).expect("repairs");
    let RepairOutcome::Repaired { bytes, report } = outcome else {
        panic!("not repaired: {outcome:?}");
    };
    assert_eq!(report.holes.len(), 1, "{report:?}");
    assert!(report.joins[0].ramp_frames > 0, "{report:?}");
    assert!(
        report.views_restated > report.joins[0].ramp_frames,
        "{report:?}"
    );
    let bad = views_out_of_range(&bytes);
    assert!(
        bad.is_empty(),
        "{} frames carry a view out of range, first {:?}",
        bad.len(),
        bad.first()
    );
}

/// Damage that runs to the end of the file: nothing resumes after it, so the
/// repair is the recording up to the break, with no gap to list.
#[test]
fn a_demo_damaged_to_its_end_is_repaired_up_to_the_break() {
    let original = crate::tests_fixture::fixture();
    let len = 200_000;
    let outcome = repair(&damaged_fixture(original.len() - len, len)).expect("repairs");
    let RepairOutcome::Repaired { bytes, report } = outcome else {
        panic!("not repaired: {outcome:?}");
    };
    assert!(report.ends_at_last_hole, "{report:?}");
    assert!(report.holes.is_empty(), "{report:?}");
    assert!(report.percent_kept() > 97.0, "{report:?}");
    assert_eq!(find_first_hole(&bytes), None);
}

/// Damage right where playback starts, behind an intact signon. The signon's
/// last frames sit next to the hole, and their times spread over the map's
/// loading (17 to 22.7 s in the fixture): judged as edge frames, three were
/// dropped, the first directory entry no longer ended where it said, and the
/// result did not parse.
#[test]
fn a_demo_damaged_where_playback_starts_keeps_its_signon_whole() {
    let original = crate::tests_fixture::fixture();
    let e0 = frame_walk::entry0_end(&original, frame_walk::frames_end(&original)).expect("signon");
    let outcome = repair(&damaged_fixture(e0, 300_000)).expect("repairs");
    let RepairOutcome::Repaired { report, .. } = outcome else {
        panic!("not repaired: {outcome:?}");
    };
    assert_eq!(report.holes.len(), 1, "{report:?}");
    assert_eq!(report.checks.parse_error, None);
}

#[test]
fn a_repair_can_be_cancelled() {
    let original = crate::tests_fixture::fixture();
    let bytes = damaged_fixture(original.len() / 2, 300_000);
    let result = repair_demo(
        &bytes,
        RepairHooks {
            progress: &mut |_| {},
            is_cancelled: &|| true,
        },
    );
    assert_eq!(result, Err(RepairError::Cancelled));
}

#[test]
fn progress_runs_through_every_step_and_never_goes_back() {
    let original = crate::tests_fixture::fixture();
    let bytes = damaged_fixture(original.len() / 2, 300_000);
    let mut seen: Vec<RepairProgress> = Vec::new();
    let outcome = repair_demo(
        &bytes,
        RepairHooks {
            progress: &mut |p| seen.push(p),
            is_cancelled: &|| false,
        },
    );
    assert!(matches!(outcome, Ok(RepairOutcome::Repaired { .. })));
    assert!(seen.windows(2).all(|w| w[1].fraction >= w[0].fraction));
    for step in [
        RepairStep::FindingHoles,
        RepairStep::Reading,
        RepairStep::RebuildingJoins,
        RepairStep::Writing,
        RepairStep::Checking,
    ] {
        assert!(seen.iter().any(|p| p.step == step), "{step:?} never shown");
    }
    assert_eq!(seen.last().map(|p| p.fraction), Some(1.0));
}

#[test]
fn a_file_that_is_not_a_demo_cannot_be_repaired() {
    assert_eq!(repair(&[0xEE; 4096]), Err(RepairError::NoSignon));
}

/// The four damaged Monday demos of #15 (`monday-wsod25_r07_*.dem`), from
/// `DOD_REPAIR_DEMOS` (a folder), else the PRE install's `dod` folder: each
/// repairs with its holes found and every check passed. `m1_h2`'s third hole
/// comes after a level change (`dod_anzio` to `dod_pandemic_aim`).
/// `cargo test -p analysis --release -- --ignored --nocapture monday_demos`.
#[test]
#[ignore]
fn the_monday_demos_repair_with_their_holes_found() {
    let folder = std::env::var_os("DOD_REPAIR_DEMOS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(
                r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - PRE-Anniversary for Movies\dod",
            )
        });
    // (demo, holes, hole times before/after in recording seconds)
    let expected: [(&str, &[(f32, f32)]); 4] = [
        ("m1_h1", &[(39.2, 68.6), (816.3, 828.8)]),
        (
            "m1_h2",
            &[(718.9, 740.2), (1109.6, 1116.0), (1504.3, 1519.2)],
        ),
        ("m3_h1-1", &[(70.5, 100.1), (459.9, 472.5), (849.1, 893.3)]),
        ("m3_h2", &[(636.7, 658.6)]),
    ];
    for (name, holes) in expected {
        let path = folder.join(format!("monday-wsod25_r07_{name}.dem"));
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("{}: {e} (set DOD_REPAIR_DEMOS)", path.display()));
        let outcome = repair(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        let RepairOutcome::Repaired { report, .. } = outcome else {
            panic!("{name}: not repaired: {outcome:?}");
        };
        println!(
            "{name}: {} holes, {:.0} s lost, {:.1}% kept, {} frames, joins {:?}",
            report.holes.len(),
            report.seconds_lost(),
            report.percent_kept(),
            report.checks.frames,
            report.joins
        );
        assert_eq!(
            report.holes.len(),
            holes.len(),
            "{name}: {:?}",
            report.holes
        );
        for (hole, (before, after)) in report.holes.iter().zip(holes) {
            assert!((hole.before_secs - before).abs() < 0.5, "{name}: {hole:?}");
            assert!((hole.after_secs - after).abs() < 0.5, "{name}: {hole:?}");
        }
        assert_eq!(
            report.joins.len(),
            holes.len(),
            "{name}: {:?}",
            report.joins
        );
    }
}

#[test]
fn the_first_hole_is_found_where_the_frames_stop_reading() {
    let original = crate::tests_fixture::fixture();
    assert_eq!(find_first_hole(&original), None);
    let at = original.len() / 2;
    let hole = find_first_hole(&damaged_fixture(at, 300_000)).expect("a hole");
    assert!(
        hole.byte_offset as usize <= at && hole.byte_offset as usize > at - 70_000,
        "{hole:?}"
    );
    assert!(hole.after_secs > 60.0, "{hole:?}");
}

/// The fixture with one mid-file network frame's messages overwritten, its
/// frame header (and so its length) left intact.
fn message_damaged_fixture() -> Vec<u8> {
    let mut bytes = crate::tests_fixture::fixture();
    let demo = dem::open_demo_from_bytes(&bytes).expect("parses");
    let spans: Vec<_> = demo
        .directory
        .entries
        .iter()
        .flat_map(|e| &e.frames)
        .filter_map(|f| match &f.frame_data {
            FrameData::NetworkMessage(n) => n.1.source_span.clone(),
            _ => None,
        })
        .filter(|span| span.len() > 64)
        .collect();
    let span = spans[spans.len() / 2].clone();
    bytes[span].fill(0xEE);
    bytes
}

/// Damage inside a frame whose header reads: the analyzer's tolerant parse
/// stops there and flags the demo (#811), but the frames walk to the end, so
/// the repair has no hole to cut out. Studio asks `find_first_hole` before
/// it offers Repair, and leaves Split alone for such a demo.
#[test]
fn damage_inside_a_frame_is_no_hole_and_nothing_to_repair() {
    let bytes = message_damaged_fixture();
    let analysis = crate::Analysis::try_from_bytes(&bytes).expect("the frames before it read");
    assert!(analysis.demo_info.damage.is_some(), "the analyzer flags it");
    assert_eq!(find_first_hole(&bytes), None);
    assert_eq!(repair(&bytes).expect("repairs"), RepairOutcome::NotDamaged);
}
