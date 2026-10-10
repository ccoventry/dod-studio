//! A capture batch the hook DLL runs on the original demos (`dodstudio_batch`,
//! goldsrc-hooks `batch.rs`), instead of patched copies (issue #434, step 3).
//!
//! `build_batch_queue` still plans the batch: the blocks, their take folders,
//! the helper cfg's aliases and every scheduled command with its tick. This
//! module turns that plan into the hook's batch file:
//!
//! - **Ticks become demo-player seconds.** The hook runs commands on the
//!   `viewdemo` clock, the server time each kill's `viewdemo_times` entry is in.
//!   A tick is placed by the nearest kill of the same demo: that kill's
//!   `viewdemo` time, plus the playback time between the two frames.
//! - **Fast-forward becomes a seek.** Each `sys_fast_forward` jumps to where
//!   the next `sys_normal_speed` drops back to real time, which is the pre-roll
//!   of the next clip. The first one, before clip 1, jumps there as soon as
//!   the demo has loaded.
//! - **`<demo>_next` becomes `next`.** The hook loads the next demo itself; the
//!   alias (`playdemo` of the next patched copy) would not exist.
//! - The 64-byte `build_safe_echos` splitting and the breadcrumbs stay out:
//!   only the markers Studio reads go in.

use std::path::Path;

use crate::patch::types::{CaptureStreak, PatchJob};

/// The hook's batch file header (goldsrc-hooks `batch::HEADER`).
pub const HEADER: &str = "dodstudio-batch 1";

/// The hook DLL's decal clear (goldsrc-hooks `commands.rs`).
pub const CLEAR_DECALS: &str = "dodstudio_clear_decals";

/// The hook DLL's batch command.
pub const BATCH_COMMAND: &str = "dodstudio_batch";

/// One kill whose frame and `viewdemo` time are both known.
#[derive(Debug, Clone, Copy)]
struct Anchor {
    tick: i32,
    viewdemo: f64,
}

/// Places ticks of one demo on the `viewdemo` clock.
struct Clock<'a> {
    anchors: Vec<Anchor>,
    frame_times: &'a [f32],
    fps: f64,
}

impl Clock<'_> {
    fn playback(&self, tick: i32) -> Option<f64> {
        usize::try_from(tick)
            .ok()
            .and_then(|t| self.frame_times.get(t))
            .map(|&t| f64::from(t))
    }

    fn seconds(&self, tick: i32) -> f64 {
        let anchor = self
            .anchors
            .iter()
            .min_by_key(|a| (i64::from(a.tick) - i64::from(tick)).abs())
            .copied()
            .expect("a clock has anchors");
        let between = match (self.playback(tick), self.playback(anchor.tick)) {
            (Some(t), Some(a)) => t - a,
            _ => f64::from(tick - anchor.tick) / self.fps,
        };
        (anchor.viewdemo + between).max(0.0)
    }
}

fn clock_for<'a>(demo: &str, streaks: &'a [CaptureStreak]) -> Result<Clock<'a>, String> {
    let mine: Vec<&CaptureStreak> = streaks.iter().filter(|s| s.source_demo == demo).collect();
    let anchors: Vec<Anchor> = mine
        .iter()
        .flat_map(|s| {
            s.kills
                .iter()
                .zip(&s.viewdemo_times)
                .filter(|(_, vd)| vd.is_finite() && **vd > 0.0)
                .map(|(kill, vd)| Anchor {
                    tick: kill.0,
                    viewdemo: f64::from(*vd),
                })
        })
        .collect();
    if anchors.is_empty() {
        return Err(format!(
            "{} has no demo-player times for its kills (scan it again)",
            Path::new(demo).display()
        ));
    }
    let frame_times = mine
        .iter()
        .map(|s| s.frame_times.as_slice())
        .max_by_key(|f| f.len())
        .unwrap_or(&[]);
    let fps = mine
        .iter()
        .map(|s| s.demo_fps)
        .find(|&f| f > 0.0)
        .unwrap_or(30.0);
    Ok(Clock {
        anchors,
        frame_times,
        fps: f64::from(fps),
    })
}

fn clean(line: &str) -> String {
    line.replace(['\r', '\n', '\t'], " ")
}

fn seconds_text(s: f64) -> String {
    format!("{s:.3}")
}

/// The batch file for `jobs` (from `build_batch_queue`, primer included or
/// not), with `streaks` the highlights the batch was planned from. With
/// `clear_decals` (the Flush Decals setting), every seek lands on
/// `dodstudio_clear_decals`: the original demo still holds the decals the
/// patched copy's flush strips. `Err` says why this batch can't run from the
/// hook and has to be patched.
pub fn batch_file(
    jobs: &[PatchJob],
    streaks: &[CaptureStreak],
    clear_decals: bool,
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
        out.push(format!("demo\t{}", clean(&job.source_demo)));
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
                    let to = seconds_text(clock.seconds(to));
                    out.push(format!("seek\t{}\t{to}", seconds_text(at)));
                    if clear_decals {
                        out.push(format!("at\t{to}\t{CLEAR_DECALS}"));
                    }
                }
            } else if next_alias.as_deref() == Some(command.as_str()) {
                out.push(format!("next\t{}", seconds_text(at)));
            } else {
                out.push(format!("at\t{}\t{}", seconds_text(at), clean(command)));
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

/// The console lines that start `file`. The background setting goes first:
/// the batch's own init commands set it too, but only once the first demo has
/// loaded, and an unfocused game loads a demo several times slower without it
/// (85 s for a 21-minute HLTV demo, measured). On, the batch also starts with
/// the game out of the way (goldsrc-hooks `hlae_window.rs`).
pub fn start_lines(file: &Path) -> Vec<String> {
    vec![
        format!("{} 1", crate::patch::builder::RUN_IN_BACKGROUND),
        format!("{BATCH_COMMAND} start \"{}\"", file.display()),
    ]
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
                // The first fast-forward seeks to clip 1's pre-roll at once.
                "seek\t0.000\t325.000",
                "at\t0.000\techo [dod-studio] DEMO_START 1 1 2",
                "at\t325.000\tsys_normal_speed",
                "at\t329.000\tsys_record_start",
                "at\t335.000\tsys_record_stop",
                // Nearest kill is now the second one.
                "seek\t336.000\t385.500",
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
    fn a_demo_with_no_viewdemo_times_has_to_be_patched() {
        let mut s = streak("a.dem", &[(3000, 330.0)], 100.0);
        s.viewdemo_times.clear();
        let e = batch_file(&[job("a.dem", &[(1, "x")])], &[s], false).unwrap_err();
        assert!(e.contains("no demo-player times"), "{e}");
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
            text.contains("seek\t0.000\t325.000\nat\t325.000\tdodstudio_clear_decals\n"),
            "{text}"
        );
    }

    #[test]
    fn the_start_lines_turn_on_the_background_setting_and_quote_the_file() {
        assert_eq!(
            start_lines(Path::new("C:/x y/b.txt")),
            [
                "dodstudio_run_in_background 1",
                "dodstudio_batch start \"C:/x y/b.txt\""
            ]
        );
    }
}
