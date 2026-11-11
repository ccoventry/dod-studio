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
