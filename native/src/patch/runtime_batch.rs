//! A capture batch the hook DLL runs on the original demos (`ds_capture_batch`,
//! goldsrc-hooks `batch.rs`), instead of patched copies (issue #434, step 3).
//!
//! `build_batch_queue` still plans the batch: the blocks, their take folders,
//! the helper cfg's aliases and every scheduled command with its tick. This
//! module turns that plan into the hook's batch file:
//!
//! - **Ticks become demo-player seconds.** The hook runs commands on the
//!   `viewdemo` clock, the server time each kill's `viewdemo_times` entry is in.
//!   A tick is placed by the nearest kill of the same demo: that kill's
//!   `viewdemo` time, plus the playback time between the two frames
//!   (`viewdemo_clock.rs`, which the take plan reads its seconds from too).
//! - **Most of each fast-forward becomes a seek.** Each `sys_fast_forward`
//!   jumps to [`SETTLE_SECONDS`] before the next `sys_normal_speed` (the next
//!   clip's pre-roll) and fast-forwards the rest, as the patched demo does. The
//!   first one, before clip 1, jumps as soon as the demo has loaded. A gap
//!   shorter than that is fast-forwarded whole.
//!
//!   The fast-forwarded stretch is there because a seek moves the demo's clock
//!   but not the engine's client clock (`cl.time`: +6.5 s across an 800 s
//!   seek, measured 2026-10-10). Everything the seek replays on its way --
//!   kill-feed lines, centre text, the clan-match countdown, dead bodies --
//!   lands with a fresh client-clock timestamp and would still be on screen at
//!   the clip. Fast-forward moves both clocks together, so those expire as
//!   they would in a demo played straight through. It is the helper cfg's
//!   `sys_fast_forward`, so it runs uncapped and undrawn here too
//!   (`builder/speed.rs`, #724); the seek's own stepped frames still draw.
//! - **`<demo>_next` becomes `next`.** The hook loads the next demo itself; the
//!   alias (`playdemo` of the next patched copy) would not exist.
//! - The 64-byte `build_safe_echos` splitting and the breadcrumbs stay out:
//!   only the markers Studio reads go in.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::patch::types::{CaptureStreak, PatchJob};
use crate::patch::viewdemo_clock::clock_for;

/// A batch the hook runs: its file, the copies of demos on another drive it
/// plays from (`patch::source_copies`, for a hook DLL older than #779),
/// which go when it ends, and whether it plays a demo on another drive by
/// its full path (#779).
#[derive(Debug, Clone, Default)]
pub struct RuntimeBatch {
    pub file: PathBuf,
    pub source_copies: Vec<PathBuf>,
    pub other_drives_in_place: bool,
    /// The open game the batch runs in instead of launching one
    /// (`patch::batch_reuse`, #725), by PID.
    pub reuse_game: Option<u32>,
}

/// The hook's batch file header (goldsrc-hooks `batch::HEADER`).
pub const HEADER: &str = "dodstudio-batch 1";

/// The hook DLL's decal clear (goldsrc-hooks `commands.rs`).
pub const CLEAR_DECALS: &str = "ds_map_clear_decals";

/// How much of the run-up to each clip is fast-forwarded rather than jumped
/// over (see the module doc): longer than the kill feed's longest usual hold
/// (`hud_deathnotice_time`, 12 s set by the user) and the game's text holds.
pub const SETTLE_SECONDS: f64 = 30.0;

/// The hook DLL's batch command.
pub const BATCH_COMMAND: &str = "ds_capture_batch";

/// The batch line that pauses a clip's lead-in until Studio says OBS is
/// recording (goldsrc-hooks `batch::WAIT_FOR_RECORDING_LINE`, #1109).
pub const WAIT_FOR_RECORDING: &str = "ds_capture_batch wait_for_recording";

/// Studio's go-ahead for a clip waiting at [`WAIT_FOR_RECORDING`].
pub const RECORDING_GO: &str = "ds_capture_batch recording";

/// The helper cfg's sound flush, scheduled at each clip's `AUDIO_SYNC`.
const SOUND_FLUSH: &str = "sys_sound";

fn clean(line: &str) -> String {
    line.replace(['\r', '\n', '\t'], " ")
}

fn seconds_text(s: f64) -> String {
    format!("{s:.3}")
}

/// The batch file for `jobs` (from `build_batch_queue`, primer included or
/// not), with `streaks` the highlights the batch was planned from. With
/// `clear_decals` (the Flush Decals setting), every seek lands on
/// `ds_map_clear_decals`: the original demo still holds the decals the
/// patched copy's flush strips. `Err` says why this batch can't run from the
/// hook and has to be patched.
pub fn batch_file(
    jobs: &[PatchJob],
    streaks: &[CaptureStreak],
    clear_decals: bool,
) -> Result<String, String> {
    batch_file_playing(jobs, streaks, clear_decals, &HashMap::new(), false)
}

/// [`batch_file`], with a demo in `play_as` played from that path instead
/// of its own: a demo on another drive than the game is copied into the
/// game's folder for the batch when the hook DLL is older than #779
/// (`patch::source_copies`). The highlights
/// still belong to the original, so the clock is the original's. With
/// `wait_for_recording` (OBS mode), each clip's lead-in pauses at its
/// `AUDIO_SYNC` until Studio says OBS is recording (#1109).
pub fn batch_file_playing(
    jobs: &[PatchJob],
    streaks: &[CaptureStreak],
    clear_decals: bool,
    play_as: &HashMap<String, PathBuf>,
    wait_for_recording: bool,
) -> Result<String, String> {
    let mut out = vec![HEADER.to_string()];
    let mut demos = 0;
    for job in jobs.iter().filter(|j| !j.blocks.is_empty()) {
        demos += 1;
        let clock = clock_for(&job.source_demo, streaks)?;
        let next_alias = job
            .output_demo
            .file_stem()
            .map(|s| format!("{}_next", s.to_string_lossy()));
        let plays = play_as
            .get(&job.source_demo)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| job.source_demo.clone());
        out.push(format!("demo\t{}", clean(&plays)));
        for line in &job.init_commands {
            out.push(format!("init\t{}", clean(line)));
        }
        let commands: Vec<&(i32, String)> = job
            .scheduled_commands
            .iter()
            .filter(|(_, c)| !c.contains("BREADCRUMB"))
            .collect();
        // Everything up to the first fast-forward runs as soon as the demo has
        // loaded: the patched demo only waited there for the game to settle,
        // which the hook has already waited for.
        let first_fast = commands
            .iter()
            .position(|(_, c)| c == "sys_fast_forward")
            .map(|i| commands[i].0);
        let pre_roll_after = |from: usize| -> Option<i32> {
            commands[from..]
                .iter()
                .find(|(_, c)| c == "sys_normal_speed")
                .map(|(t, _)| *t)
        };
        for (i, (tick, command)) in commands.iter().enumerate() {
            let at = match first_fast {
                Some(first) if *tick <= first => 0.0,
                _ => clock.seconds(*tick),
            };
            if command == "sys_fast_forward" {
                // Nothing after it to seek to: the demo ends here.
                if let Some(to) = pre_roll_after(i) {
                    let to = clock.seconds(to);
                    let settle_from = to - SETTLE_SECONDS;
                    // Before clip 1 the clock starts wherever the demo does,
                    // so the first jump is always worth making.
                    if first_fast == Some(*tick) || settle_from > at + 1.0 {
                        out.push(format!(
                            "seek\t{}\t{}",
                            seconds_text(at),
                            seconds_text(settle_from)
                        ));
                        out.push(format!(
                            "at\t{}\t{command}",
                            seconds_text(settle_from.max(at))
                        ));
                    } else {
                        out.push(format!("at\t{}\t{command}", seconds_text(at)));
                    }
                    if clear_decals {
                        out.push(format!("at\t{}\t{CLEAR_DECALS}", seconds_text(to)));
                    }
                }
            } else if next_alias.as_deref() == Some(command.as_str()) {
                out.push(format!("next\t{}", seconds_text(at)));
            } else {
                out.push(format!("at\t{}\t{}", seconds_text(at), clean(command)));
                // `sys_sound` is the lead-in's AUDIO_SYNC stage, where OBS
                // mode starts the recording.
                if wait_for_recording && command == SOUND_FLUSH {
                    out.push(format!("at\t{}\t{WAIT_FOR_RECORDING}", seconds_text(at)));
                }
            }
        }
    }
    if demos == 0 {
        return Err("the batch has no clips".to_string());
    }
    out.push("end\tsys_capture_done_path".to_string());
    out.push(String::new());
    Ok(out.join("\n"))
}

/// The console lines sent the moment the game takes commands: full speed in
/// the background, the stay-behind setting, then the game stepping out of the
/// way (goldsrc-hooks `hlae_window.rs`). The batch's own init commands set
/// both settings too, but only once the first demo has loaded, and an
/// unfocused game loads a demo several times slower without the first (85 s
/// for a 21-minute HLTV demo, measured). Sent ahead of [`start_lines`]'
/// settle, so the game holds the user's screen and mouse for as short a time
/// as it can. With `keep_game_behind` off the game stays in front, and the
/// setting goes to 0 in case a game left from an earlier batch (#666) still
/// has it on.
pub fn early_lines(keep_game_behind: bool) -> Vec<String> {
    use crate::patch::builder::{RUN_IN_BACKGROUND, STAY_BEHIND};
    let mut lines = vec![
        format!("{RUN_IN_BACKGROUND} 1"),
        format!("{STAY_BEHIND} {}", u8::from(keep_game_behind)),
    ];
    if keep_game_behind {
        lines.push(format!("{BATCH_COMMAND} away"));
    }
    lines
}

/// The console line that starts `batch`'s file, once the game has settled.
/// A batch that plays a demo on another drive by its full path first turns
/// the hook's other-drive opens on, in case the player turned them off.
pub fn start_lines(batch: &RuntimeBatch) -> Vec<String> {
    let mut lines = Vec::new();
    if batch.other_drives_in_place {
        lines.push(crate::preview_in_place::OTHER_DRIVES_ON.to_string());
    }
    lines.push(format!(
        "{BATCH_COMMAND} start \"{}\"",
        batch.file.display()
    ));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn streak(demo: &str, kills: &[(i32, f32)], fps: f32) -> CaptureStreak {
        CaptureStreak {
            start_tick: kills[0].0,
            end_tick: kills[kills.len() - 1].0,
            source_demo: demo.to_string(),
            target_player: None,
            kill_count: kills.len(),
            timeline_string: String::new(),
            duration_string: String::new(),
            player_index: 0,
            kills: kills
                .iter()
                .map(|&(t, _)| (t, t as f32 / fps, "Weapon::Kar".to_string()))
                .collect(),
            start_index: 0,
            end_index: kills.len() - 1,
            total_demo_frames: 20_000,
            demo_fps: fps,
            viewdemo_times: kills.iter().map(|&(_, vd)| vd).collect(),
            frame_times: Arc::new((0..20_000).map(|i| i as f32 / fps).collect()),
            status: Default::default(),
            match_start_tick: None,
            source_key: None,
            spectated: false,
            steam_id: None,
            recorded_this_demo: false,
        }
    }

    fn job(demo: &str, commands: &[(i32, &str)]) -> PatchJob {
        PatchJob {
            source_demo: demo.to_string(),
            output_demo: "C:/game/dod/dodstudio_chain_01.dem".into(),
            streaks: Vec::new(),
            target_player: None,
            init_commands: vec!["mirv_movie_fps 60".into()],
            scheduled_commands: commands.iter().map(|&(t, c)| (t, c.to_string())).collect(),
            director_events: Vec::new(),
            block_routes: Vec::new(),
            blocks: vec![crate::patch::types::CaptureBlock {
                demo_name: "dodstudio_chain_01".into(),
                block_index: 0,
                drive_index: 0,
                take_folder: "C:/takes/dodstudio_chain_01_b0".into(),
                take_key: String::new(),
                source_streak_indices: vec![0],
                start_tick: 0,
                end_tick: 0,
                record_start_tick: 0,
                record_stop_tick: 0,
                ramped_gaps: 0,
                plan: None,
            }],
        }
    }

    #[test]
    fn ticks_land_on_the_viewdemo_clock_by_the_nearest_kill() {
        // 100 fps; kill at tick 3000 (30 s of playback) is at 330 s of server time.
        let streaks = [streak("a.dem", &[(3000, 330.0), (9000, 390.5)], 100.0)];
        let jobs = [job(
            "a.dem",
            &[
                (200, "sys_fast_forward"),
                (200, "echo [dod-studio] DEMO_START 1 1 2"),
                (200, "echo \"[dod-studio] BREADCRUMB - Tick 200\""),
                (2500, "sys_normal_speed"),
                (2900, "sys_record_start"),
                (3500, "sys_record_stop"),
                (3600, "sys_fast_forward"),
                (8500, "sys_normal_speed"),
                (9100, "sys_record_stop"),
                (9200, "sys_fast_forward"),
                (9200, "dodstudio_chain_01_next"),
            ],
        )];
        let text = batch_file(&jobs, &streaks, false).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines,
            [
                HEADER,
                "demo\ta.dem",
                "init\tmirv_movie_fps 60",
                // The first fast-forward seeks at once to 30 s before clip 1's
                // pre-roll and fast-forwards the rest.
                "seek\t0.000\t295.000",
                "at\t295.000\tsys_fast_forward",
                "at\t0.000\techo [dod-studio] DEMO_START 1 1 2",
                "at\t325.000\tsys_normal_speed",
                "at\t329.000\tsys_record_start",
                "at\t335.000\tsys_record_stop",
                // Nearest kill is now the second one.
                "seek\t336.000\t355.500",
                "at\t355.500\tsys_fast_forward",
                "at\t385.500\tsys_normal_speed",
                "at\t391.500\tsys_record_stop",
                // The last fast-forward has nothing to seek to.
                "next\t392.500",
                "end\tsys_capture_done_path",
            ]
        );
    }

    #[test]
    fn without_frame_times_ticks_go_by_the_demo_fps() {
        let mut s = streak("a.dem", &[(3000, 330.0)], 100.0);
        s.frame_times = Arc::new(Vec::new());
        let jobs = [job("a.dem", &[(2900, "sys_record_start")])];
        let text = batch_file(&jobs, &[s], false).unwrap();
        assert!(text.contains("at\t329.000\tsys_record_start"), "{text}");
    }

    #[test]
    fn obs_mode_waits_for_the_recording_at_each_audio_sync() {
        let streaks = [streak("a.dem", &[(3000, 330.0)], 100.0)];
        let jobs = [job(
            "a.dem",
            &[
                (2800, "sys_sound"),
                (2800, "echo [dod-studio] AUDIO_SYNC - Tick 2800"),
                (2900, "sys_record_start"),
            ],
        )];
        // The hook's own names (goldsrc-hooks `batch.rs`, whose test pins
        // the same text).
        assert_eq!(WAIT_FOR_RECORDING, "ds_capture_batch wait_for_recording");
        assert_eq!(RECORDING_GO, "ds_capture_batch recording");
        let wait = format!("at\t328.000\t{WAIT_FOR_RECORDING}");
        let obs = batch_file_playing(&jobs, &streaks, false, &HashMap::new(), true).unwrap();
        assert!(
            obs.contains(&format!("at\t328.000\tsys_sound\n{wait}\n")),
            "{obs}"
        );
        assert_eq!(obs.matches(WAIT_FOR_RECORDING).count(), 1);
        let hlae = batch_file(&jobs, &streaks, false).unwrap();
        assert!(!hlae.contains(WAIT_FOR_RECORDING), "{hlae}");
    }

    #[test]
    fn a_demo_with_no_viewdemo_times_has_to_be_patched() {
        let mut s = streak("a.dem", &[(3000, 330.0)], 100.0);
        s.viewdemo_times.clear();
        let e = batch_file(&[job("a.dem", &[(1, "x")])], &[s], false).unwrap_err();
        assert!(e.contains("no demo-player times"), "{e}");
    }

    #[test]
    fn a_copied_demo_is_played_from_its_copy_on_the_originals_clock() {
        let streaks = [streak("D:/demos/a.dem", &[(3000, 330.0)], 100.0)];
        let jobs = [job("D:/demos/a.dem", &[(2900, "sys_record_start")])];
        let play_as = HashMap::from([(
            "D:/demos/a.dem".to_string(),
            PathBuf::from("C:/game/dod/dodstudio_src_01_a.dem"),
        )]);
        let text = batch_file_playing(&jobs, &streaks, false, &play_as, false).unwrap();
        assert!(
            text.contains("demo\tC:/game/dod/dodstudio_src_01_a.dem\n"),
            "{text}"
        );
        assert!(text.contains("at\t329.000\tsys_record_start"), "{text}");
    }

    #[test]
    fn the_primer_is_left_out() {
        let streaks = [streak("a.dem", &[(3000, 330.0)], 100.0)];
        let mut primer = job("a.dem", &[(500, "dodstudio_primer_next")]);
        primer.blocks.clear();
        let text = batch_file(&[primer, job("a.dem", &[(2900, "x")])], &streaks, false).unwrap();
        assert_eq!(text.matches("demo\t").count(), 1, "{text}");
        assert!(!text.contains("primer"), "{text}");
    }

    #[test]
    fn with_the_decal_flush_every_seek_lands_on_a_clear() {
        let streaks = [streak("a.dem", &[(3000, 330.0)], 100.0)];
        let jobs = [job(
            "a.dem",
            &[(200, "sys_fast_forward"), (2500, "sys_normal_speed")],
        )];
        let text = batch_file(&jobs, &streaks, true).unwrap();
        assert!(
            text.contains("at\t325.000\tds_map_clear_decals\n"),
            "{text}"
        );
    }

    #[test]
    fn a_gap_shorter_than_the_settle_is_fast_forwarded_whole() {
        let streaks = [streak("a.dem", &[(3000, 330.0)], 100.0)];
        // Clip ends at 336 s, the next pre-roll starts at 356 s: 20 s apart.
        let jobs = [job(
            "a.dem",
            &[
                (200, "sys_fast_forward"),
                (2500, "sys_normal_speed"),
                (3600, "sys_fast_forward"),
                (5600, "sys_normal_speed"),
            ],
        )];
        let text = batch_file(&jobs, &streaks, false).unwrap();
        assert_eq!(text.matches("seek\t").count(), 1, "{text}");
        assert!(text.contains("at\t336.000\tsys_fast_forward\n"), "{text}");
    }

    #[test]
    fn the_early_lines_step_aside_and_the_start_line_quotes_the_file() {
        assert_eq!(
            early_lines(true),
            [
                "ds_capture_run_in_background 1",
                "ds_capture_stay_behind 1",
                "ds_capture_batch away"
            ]
        );
        assert_eq!(
            early_lines(false),
            ["ds_capture_run_in_background 1", "ds_capture_stay_behind 0"]
        );
        let mut batch = RuntimeBatch {
            file: PathBuf::from("C:/x y/b.txt"),
            ..Default::default()
        };
        assert_eq!(
            start_lines(&batch),
            ["ds_capture_batch start \"C:/x y/b.txt\""]
        );
        // A demo played from another drive by its full path (#779).
        batch.other_drives_in_place = true;
        assert_eq!(
            start_lines(&batch),
            [
                "ds_demo_other_drives 1",
                "ds_capture_batch start \"C:/x y/b.txt\""
            ]
        );
    }
}
