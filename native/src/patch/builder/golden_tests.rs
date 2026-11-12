//! Golden tests: what a batch writes today, byte for byte.
//!
//! The batch planner (`passes.rs`, #1085 step 10) reorganises how
//! `build_batch_queue` groups and cuts highlights, and later steps add
//! variants on top of it. None of that may change a batch that has no
//! variants: these tests pin the three things a batch hands the game, for a
//! payload shaped like a real project (several demos, an HLTV demo with two
//! players, merged, chained and far-apart highlights, a long gap between
//! kills, Scheduled Commands), in each capture mode:
//!
//! - the hook's batch file (`runtime_batch::batch_file_playing`, the default
//!   route, as `hook_batch.rs` calls it),
//! - `dodstudio_helper.cfg`,
//! - every job's init and scheduled commands, director events and blocks (the
//!   patched route's schedule, and what the take checks and manifest read).
//!
//! The expected files live in `golden/`. Only the scratch folder's path and
//! the helper cfg's date and version line are masked. To regenerate after a
//! change that is meant to alter a batch, run the tests with
//! `DS_UPDATE_GOLDEN=1` and review the diff.

use super::*;
use crate::patch::types::{CaptureMode, CommandRelation, CustomCommand, KillGapMode, KillGaps};
use crate::test_support::Scratch;
use std::sync::Arc;

/// Every fixture demo's frame count, at [`FPS`].
pub(super) const FRAMES: i32 = 60_000;
pub(super) const FPS: f32 = 100.0;
/// Where the fixture demos' server clock starts: the viewdemo clock is not
/// the playback clock, so the batch file's seconds must come from it.
const SERVER_CLOCK_START: f32 = 120.0;

pub(super) fn frame_times() -> Arc<Vec<f32>> {
    Arc::new((0..FRAMES).map(|i| i as f32 / FPS).collect())
}

/// One highlight of `player` (name, slot) over `kill_ticks` in `demo`.
pub(super) fn highlight(
    demo: &str,
    player: (&str, usize),
    kill_ticks: &[i32],
    spectated: bool,
    frames: &Arc<Vec<f32>>,
) -> CaptureStreak {
    CaptureStreak {
        start_tick: kill_ticks[0],
        end_tick: kill_ticks[kill_ticks.len() - 1],
        source_demo: demo.to_string(),
        target_player: Some(player.0.to_string()),
        kill_count: kill_ticks.len(),
        timeline_string: format!("{} kills", kill_ticks.len()),
        duration_string: String::new(),
        player_index: player.1,
        kills: kill_ticks
            .iter()
            .map(|&t| (t, t as f32 / FPS, "Weapon::Kar".to_string()))
            .collect(),
        start_index: 0,
        end_index: kill_ticks.len() - 1,
        total_demo_frames: FRAMES,
        demo_fps: FPS,
        viewdemo_times: kill_ticks
            .iter()
            .map(|&t| SERVER_CLOCK_START + t as f32 / FPS)
            .collect(),
        frame_times: Arc::clone(frames),
        match_start_tick: None,
        source_key: None,
        spectated,
        steam_id: None,
        recorded_this_demo: false,
        status: Default::default(),
    }
}

/// A project's picked highlights, deliberately not in play order, so the
/// indices each block points back at are tested too.
pub(super) fn payload() -> Vec<CaptureStreak> {
    let frames = frame_times();
    let alpha = "D:/demos/alpha_pov.dem";
    let bravo = "D:/demos/bravo_hltv.dem";
    let charlie = "D:/demos/charlie_pov.dem";
    vec![
        // Bravo, HLTV: two players whose first highlights start on one tick.
        highlight(bravo, ("stealthlol", 7), &[9000, 9300], true, &frames),
        highlight(bravo, ("Nifty", 5), &[9000, 9200], true, &frames),
        highlight(bravo, ("Nifty", 5), &[30000, 30150], true, &frames),
        // Alpha, POV: two that merge into one take, one chained straight on,
        // and one far later with a gap between kills over 8 s.
        highlight(alpha, ("m00cat", 3), &[3000, 3100, 3250], false, &frames),
        highlight(alpha, ("m00cat", 3), &[20000, 21500, 21600], false, &frames),
        highlight(alpha, ("m00cat", 3), &[3400, 3500], false, &frames),
        highlight(alpha, ("m00cat", 3), &[4000, 4100], false, &frames),
        // Charlie, POV: starts first, so it plays first.
        highlight(charlie, ("k4b00m", 1), &[1000, 1100], false, &frames),
    ]
}

/// A batch's config as Studio sends it, in `mode`, with the game, hook DLL
/// and capture folder under `root`.
pub(super) fn config(root: &std::path::Path, mode: CaptureMode) -> PatcherConfig {
    let game = root.join("game");
    std::fs::create_dir_all(game.join("dod")).unwrap();
    let hook_dll = root.join("dodstudio_goldsrc_hooks.dll");
    std::fs::write(&hook_dll, b"").unwrap();
    let mut config = PatcherConfig {
        capture_fps: 120,
        obs_capture_fps: 120,
        agr_fps: 60,
        init_commands: vec!["mirv_fov 90".to_string(), "cl_righthand 1".to_string()],
        custom_commands: vec![
            CustomCommand {
                command: "r_drawviewmodel 0".to_string(),
                offset: 2.0,
                relation: CommandRelation::Before,
            },
            CustomCommand {
                command: "r_drawviewmodel 1".to_string(),
                offset: 1.0,
                relation: CommandRelation::After,
            },
        ],
        pre_roll_seconds: 5.0,
        post_roll_seconds: 1.0,
        record_start_lead: 1.0,
        record_stop_trail: 1.0,
        capture_directories: vec![root.join("takes")],
        capture_mode: mode,
        resolution_width: 2560,
        resolution_height: 1440,
        primary_media_dir: Some(root.join("media")),
        kill_gaps: KillGaps {
            mode: KillGapMode::FastForward,
            over_seconds: 8.0,
            speed: 4.0,
        },
        session_id: "session_golden".to_string(),
        hlae_path: root.join("hlae").join("HLAE.exe").display().to_string(),
        game_path: game.join("hl.exe").display().to_string(),
        goldsrc_hooks_dll_path: Some(hook_dll.display().to_string()),
        ..PatcherConfig::default()
    };
    config.normalise_capture_mode();
    config
}

/// `text` with the scratch folder's path (as written, and JSON-escaped)
/// replaced by `<root>`, and line endings made `\n`.
fn masked(text: &str, root: &std::path::Path) -> String {
    let plain = root.display().to_string();
    let escaped = plain.replace('\\', "\\\\");
    text.replace(&escaped, "<root>")
        .replace(&plain, "<root>")
        .replace("\r\n", "\n")
}

/// The helper cfg without its creation date and the app version that wrote
/// it, the only lines that change from run to run.
fn helper_cfg_text(root: &std::path::Path) -> String {
    let text = std::fs::read_to_string(root.join("game").join("dod").join("dodstudio_helper.cfg"))
        .expect("build_batch_queue writes the helper cfg");
    text.lines()
        .filter(|l| !l.starts_with("# Date: ") && !l.starts_with("# Created by: "))
        .map(|l| format!("{l}\n"))
        .collect()
}

/// Every job as the game and the take checks see it.
fn jobs_text(jobs: &[PatchJob]) -> String {
    let mut out = String::new();
    for (i, job) in jobs.iter().enumerate() {
        out.push_str(&format!(
            "job {i}: {} -> {} ({:?})\n",
            job.source_demo,
            job.output_demo.display(),
            job.target_player
        ));
        for line in &job.init_commands {
            out.push_str(&format!("  init {line}\n"));
        }
        for (tick, command) in &job.scheduled_commands {
            out.push_str(&format!("  at {tick} {command}\n"));
        }
        for (tick, label) in &job.director_events {
            out.push_str(&format!("  director {tick} {label}\n"));
        }
        for route in &job.block_routes {
            out.push_str(&format!("  route {route:?}\n"));
        }
        for block in &job.blocks {
            out.push_str(&format!(
                "  block {}\n",
                serde_json::to_string(block).expect("a block serialises")
            ));
        }
    }
    out
}

/// The free space of the golden batch's capture drive: room for every block,
/// whatever the machine running the tests has free (a CI runner has far
/// less than a block of 2560x1440 frames needs).
const CAPTURE_DRIVE_FREE: u64 = 1_000_000_000_000;

/// [`build_batch_queue`] on a capture drive with [`CAPTURE_DRIVE_FREE`].
pub(super) fn build_golden_batch(
    streaks: &[CaptureStreak],
    config: &PatcherConfig,
) -> (Vec<PatchJob>, Vec<crate::patch::types::DriveHeadroom>) {
    build_batch_queue_with(
        streaks.to_vec(),
        config,
        &std::collections::HashMap::new(),
        |_| Some(CAPTURE_DRIVE_FREE),
    )
    .expect("the golden batch builds")
}

/// Builds the payload's batch in `mode` and checks it against
/// `golden/<name>.txt`.
fn check_golden(name: &str, mode: CaptureMode) {
    let root = Scratch::new(format!("batch_golden_{name}"));
    let config = config(&root, mode);
    let streaks = payload();
    let (jobs, _headroom) = build_golden_batch(&streaks, &config);
    let batch_file = crate::patch::runtime_batch::batch_file_playing(
        &jobs,
        &streaks,
        config.decal_flush,
        &std::collections::HashMap::new(),
        config.capture_mode == CaptureMode::Obs,
    )
    .expect("the golden batch runs from the hook");

    let actual = masked(
        &format!(
            "== batch file\n{batch_file}== helper cfg\n{}== jobs\n{}",
            helper_cfg_text(&root),
            jobs_text(&jobs)
        ),
        &root,
    );
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/patch/builder/golden")
        .join(format!("{name}.txt"));
    if std::env::var_os("DS_UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("no golden file {}: {e}", path.display()))
        .replace("\r\n", "\n");
    assert!(
        actual == expected,
        "the {name} batch differs from {}; first differing line: {:?}",
        path.display(),
        actual
            .lines()
            .zip(expected.lines())
            .find(|(a, e)| a != e)
            .map(|(a, e)| format!("got `{a}`, expected `{e}`"))
            .unwrap_or_else(|| "one is a prefix of the other".to_string())
    );
}

#[test]
fn a_frame_sequence_batch_writes_what_it_always_has() {
    check_golden("frame_sequence", CaptureMode::FrameSequence);
}

#[test]
fn a_direct_to_video_batch_writes_what_it_always_has() {
    check_golden("direct_to_video", CaptureMode::DirectToVideo);
}

#[test]
fn an_obs_batch_writes_what_it_always_has() {
    check_golden("obs", CaptureMode::Obs);
}

#[test]
fn an_agr_batch_writes_what_it_always_has() {
    check_golden("agr", CaptureMode::Agr);
}

/// The take cleanup's disk estimate and the batch read one plan (#1085): one
/// figure per block the batch records, in play order, each the sum of its
/// highlights' own estimates.
#[test]
fn the_disk_estimate_bills_the_blocks_the_batch_records() {
    let root = Scratch::new("batch_golden_estimate");
    let config = config(&root, CaptureMode::FrameSequence);
    let streaks = payload();
    let (jobs, _headroom) = build_golden_batch(&streaks, &config);
    let billed: Vec<u64> = jobs
        .iter()
        .flat_map(|job| job.blocks.iter())
        .map(|block| {
            block
                .source_streak_indices
                .iter()
                .map(|&i| block_byte_estimate(&config, &streaks[i], FPS))
                .sum()
        })
        .collect();
    assert_eq!(billed.len(), 7);
    assert_eq!(estimate_batch_capture_blocks(&streaks, &config), billed);
}
