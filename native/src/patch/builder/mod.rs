// patch/builder/mod.rs
// Batch job construction: `build_batch_queue` and the init commands it adds.
// The helpers it uses live beside it — `timing` (tick walks, roll floors,
// block merging), `alloc` (drive allocation), `workspace` (junctions and the
// legacy channel-based worker spawner), `preview` (preview patches).
// Calls std::fs::create_dir_all and std::thread::spawn — native-only.

use crate::patch::engine::StreamPatcher;
use crate::patch::types::{
    CaptureStreak, CaptureWorker, CommandRelation, PatchEvent, PatchJob, PatcherConfig,
};
use crate::shared::paths::{CHAIN_DEMO_PREFIX, PRIMER_DEMO_STEM};
use std::sync::{Arc, atomic::AtomicBool};

mod alloc;
mod preview;
mod timing;
mod workspace;

use alloc::allocate_blocks_first_fit_decreasing;
pub use preview::*;
pub use timing::*;
pub use workspace::*;

/// Every command the engine will run at demo load: the user's own init
/// commands, then the ones the pipeline adds for itself.
///
/// Extracted so it can be asked ahead of a capture as well as during one. The
/// app's own additions override whatever the game's configs set — `capture_fps`
/// beats a `mirv_movie_fps` in `movie.cfg`, and the decal pin beats an
/// `r_decals` there — and a user is entitled to know that before it happens
/// rather than by noticing the result.
pub fn final_init_commands(config: &PatcherConfig) -> Vec<String> {
    let mut out = config.init_commands.clone();
    out.push("sys_autodir".to_string());
    out.push(format!("mirv_movie_fps {}", config.capture_fps));

    // OBS mode is real time: HLAE issues no `mirv_movie_start` at all, so
    // `mirv_movie_fps` above is inert on this path — nothing reads it. What
    // OBS actually records is however fast the engine renders, so that rate
    // has to be pinned to the same `obs_capture_fps` OBS's own canvas is set
    // to (obs::provision::ensure_dod_studio_setup), or the two drift against
    // each other. `fps_override 1` first: GoldSrc's default `fps_max`
    // ceiling (~100) is below what obs_capture_fps is commonly set to, and
    // `fps_max` alone is silently clamped under that ceiling without it.
    if config.capture_mode == crate::patch::CaptureMode::Obs {
        out.push("fps_override 1".to_string());
        out.push(format!("fps_max {}", config.obs_capture_fps));
    }

    // The decal flush needs the ring set once, at demo load, and never again.
    // r_decals bounds how far the rotating index may travel before it wraps; it
    // does not evict anything, so lowering it once decals have accumulated
    // strands every one sitting above the new limit.
    //
    // The sweep is sized to that same number, so there is only one number here
    // and `r_decals` is where the engine reads it, at the same precedence
    // `ring_limit` itself resolves (see that function): Initial Commands, then
    // an executed config, then the app's own default. When either of the first
    // two states it, that line is already the pin — appending a second one
    // here would either silently overrule what Initial Commands asked for, or
    // be a no-op duplicate of what the config already set. Only the true
    // "nothing anywhere states it" case needs one appended, to replace the
    // engine's own uncontrolled standing value with a known one.
    //
    // Not at the maximum, though. r_decals is clamped to MAX_RENDER_DECALS, so a
    // sweep that size turns a full revolution whatever the cvar happens to be —
    // any smaller ring simply gets swept several times over. Pinning then buys
    // nothing and costs the precondition the rest of this design works around:
    // that nothing else may touch r_decals.
    if config.decal_flush
        && crate::patch::ring_limit_from_init(&config.init_commands).is_none()
        && crate::patch::ring_limit_from_game_config(config).is_none()
    {
        let ring = config.decal_ring_limit.min(crate::patch::MAX_RENDER_DECALS);
        if ring > 0 && ring < crate::patch::MAX_RENDER_DECALS {
            out.push(format!("r_decals {}", ring));
        }
    }

    out
}

pub fn build_batch_queue(
    raw_streaks: Vec<CaptureStreak>,
    config: &PatcherConfig,
    global_arrays: &std::collections::HashMap<std::path::PathBuf, std::sync::Arc<Vec<f32>>>,
) -> Result<(Vec<PatchJob>, Vec<crate::patch::types::DriveHeadroom>), std::io::Error> {
    // tickrate is extracted dynamically from streaks per-demo.
    // Each streak is carried alongside its index in `raw_streaks` so the blocks
    // built below can point back at the exact highlights the caller dispatched,
    // even after the overlap merge collapses several into one recording.
    let mut grouped: std::collections::HashMap<(&str, Option<&str>), Vec<(usize, &CaptureStreak)>> =
        std::collections::HashMap::new();
    for (idx, streak) in raw_streaks.iter().enumerate() {
        grouped
            .entry((streak.source_demo.as_str(), streak.target_player.as_deref()))
            .or_default()
            .push((idx, streak));
    }

    // The rolls are load-bearing now — the audio resync, the sound flush, the
    // decal sweep's lead and any Scheduled Command's offset all measure against
    // them — so say when one is too short rather than leaving it to be found in
    // a capture that looks almost right.
    let floors = roll_floors(config);
    if config.pre_roll_seconds < floors.pre_roll {
        crate::log_markdown(&format!(
            "⚠️ **Pre-roll is shorter than this capture needs** — {:.1}s, against a {:.1}s floor \
             set by {}. Playback returns to real time {:.1}s before recording, which is not enough \
             for it: the engine's audio buffers are left unflushed by the fast-forward. Raise the \
             pre-roll to at least {:.1}s.",
            config.pre_roll_seconds,
            floors.pre_roll,
            floors.pre_roll_binding,
            config.pre_roll_seconds,
            floors.pre_roll
        ));
    }
    if config.post_roll_seconds < floors.post_roll {
        crate::log_markdown(&format!(
            "⚠️ **Post-roll is shorter than this capture needs** — {:.1}s, against a {:.1}s floor \
             set by {}. Anything past the post-roll fires while playback is fast-forwarding again.",
            config.post_roll_seconds, floors.post_roll, floors.post_roll_binding
        ));
    }

    // Sort grouped chronologically by the start_tick of their first streak
    let mut sorted_groups: Vec<_> = grouped.into_iter().collect();
    sorted_groups
        .sort_by_key(|(_, streaks)| streaks.iter().map(|(_, s)| s.start_tick).min().unwrap_or(0));

    let mut jobs = Vec::new();
    let total_jobs = sorted_groups.len();

    let date_time = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let mut helper_cfg_content = String::new();

    let game_path_buf = std::path::PathBuf::from(&config.game_path);
    let dod_dir = match game_path_buf.parent() {
        Some(parent) => parent.join("dod"),
        None => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Target game output directory not found",
            ));
        }
    };

    // Remove stale config from dod_dir
    let _ = std::fs::remove_file(dod_dir.join("dodstudio_helper.cfg"));
    let _ = std::fs::remove_file(dod_dir.join("dodstudio_capture_done.cfg"));
    let _ = std::fs::remove_file(dod_dir.join("dod_quit.cfg"));
    if let Ok(entries) = std::fs::read_dir(&dod_dir) {
        for entry in entries.flatten() {
            let filename = entry.file_name().to_string_lossy().to_string();
            if filename.starts_with("dodstudio_chain_") && filename.ends_with(".cfg") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    // ── AOT disk-space simulation ─────────────────────────────────────────────
    // Snapshot current free bytes for every configured capture directory so we
    // can route each clip to the drive with sufficient headroom at build time.
    const FAILOVER_THRESHOLD: u64 = crate::sys::disk::MIN_DRIVE_HEADROOM_BYTES;
    let mut drive_free: Vec<u64> = config
        .capture_directories
        .iter()
        .map(|p| crate::sys::disk::get_available_bytes(p))
        .collect();
    let mut active_drive_idx: usize = 0;
    // If no capture directories are configured, fall back to a single sentinel
    // that has "unlimited" free space so existing single-drive behaviour is
    // preserved without an error.
    if drive_free.is_empty() {
        drive_free.push(u64::MAX);
    }

    let active_export_dir = match config.primary_media_dir.clone() {
        Some(dir) => dir,
        None => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Target game output directory not found",
            ));
        }
    };
    let session_dir = if !config.session_id.is_empty() {
        active_export_dir.join(&config.session_id)
    } else {
        active_export_dir
    };
    if !session_dir.exists() {
        let _ = std::fs::create_dir_all(&session_dir);
    }

    helper_cfg_content.push_str(&format!(
        "# dodstudio_helper.cfg\n# Created by: dod_studio.exe v{}\n# Date: {}\n\n",
        crate::VERSION,
        date_time
    ));
    // Marks exactly where this file execs relative to the engine's own
    // config.cfg/movie.cfg chain, which lands in qconsole.log too (-condebug
    // is passed on every launch) — cheap enough to leave in permanently rather than
    // re-add it every time this ordering question comes up again. See
    // docs/goldsrc_dod_quirks.md's Command Precedence entry.
    helper_cfg_content.push_str("echo dodstudio_helper.cfg exec'd here\n\n");

    helper_cfg_content.push_str("# Global aliases\n");
    helper_cfg_content.push_str("alias sys_autodir \"spec_autodirector 1\"\n");
    helper_cfg_content
        .push_str("alias sys_normal_speed \"sys_autodir; clear; host_framerate 0\"\n");
    helper_cfg_content.push_str(&format!(
        "alias sys_fast_forward \"host_framerate {}\"\n",
        fast_forward_host_framerate(config)
    ));
    helper_cfg_content.push_str("alias sys_sound \"stopsound\"\n");
    // In OBS mode HLAE records nothing: the recorder is an external process
    // driven off the console-log markers these same stages already echo, so the
    // record aliases must not issue `mirv_recordmovie_*` at all.
    //
    // This is what keeps the clip at real time. `mirv_recordmovie_start` pins
    // the engine's timestep to `1/mirv_movie_fps`; without it the engine simply
    // stays at the `host_framerate 0` that `sys_normal_speed` set at the
    // pre-roll, which is exactly what OBS needs in order to capture a clip that
    // plays at the right speed.
    //
    // `stopsound` is kept. It exists to flush the audio buffers fast-forward
    // corrupts, and that corruption happens whoever is recording. It lands
    // ~1s before the first captured frame, inside the pre-roll head that gets
    // trimmed, so it is not audible in the finished clip.
    //
    // `sys_record_stop` becomes an echo rather than an empty alias: GoldSrc
    // treats an alias with an empty body as a parse oddity, and a no-op that
    // announces itself is easier to recognise in a log than one that vanishes.
    if config.capture_mode.hlae_records() {
        helper_cfg_content
            .push_str("alias sys_record_start \"mirv_recordmovie_start; stopsound\"\n");
        helper_cfg_content.push_str("alias sys_record_stop \"mirv_recordmovie_stop\"\n");
    } else {
        helper_cfg_content.push_str("alias sys_record_start \"stopsound\"\n");
        helper_cfg_content
            .push_str("alias sys_record_stop \"echo [dod-studio] OBS_MODE_NO_HLAE_STOP\"\n");
    }
    helper_cfg_content.push_str("alias sys_capture_done_path \"mirv_movie_filename DOD_STUDIO_EXIT_TRIGGER; mirv_recordmovie_start; mirv_recordmovie_stop\"\n");

    // Direct-to-video (docs/direct_to_video_capture.md), driven by the capture-
    // mode toggle. The probe that introduced this settled all four of its open
    // questions against a real capture: {AFX_STREAM_PATH} resolves to the
    // per-stream folder, it follows mirv_movie_filename (so the _route_N
    // junction routing keeps working), takes are still auto-numbered into
    // take0000, and sound.wav still lands beside the video.
    //
    // `all` here is HLAE's stream *group*, not the folder name it happens to
    // share with the composited stream. Measured with mirv_movie_separate_hud
    // on: the group does reach hudColor and hudAlpha, so all three streams get
    // a video and no per-stream enabled/options lines are needed. The folders
    // HLAE writes are lowercase (`hudcolor`/`hudalpha`) even though the command
    // names those streams in camelCase.
    //
    // Set here, once, at load — never as an injected ConsoleCommand frame. The
    // options string is several times the 64 bytes a ConsoleCommand frame's
    // command field holds, with no staggering available (one argument to one
    // command), and an
    // injected frame would shift every later frame ordinal by +1 and desync the
    // scheduled capture commands. Same rule r_decals follows.
    //
    // Not inside an alias either: GoldSrc cannot parse nested quotes in one, and
    // this argument has to carry them. As a plain command in an exec'd cfg it is
    // ordinary syntax. {QUOTE} is HLAE's own token for a literal quote, which is
    // what keeps the inner path quoting away from the engine's parser entirely.
    //
    // The codec comes from the config — see `CaptureCodec` for why every option
    // is lossless and RGB/4:4:4, and why the size ranking there is not the
    // real-time-viability ranking.
    if config.capture_mode == crate::patch::CaptureMode::DirectToVideo {
        let codec_args = config.ffmpeg_capture_codec.args();
        helper_cfg_content.push_str("\n# Direct-to-video capture\n");
        helper_cfg_content.push_str("mirv_movie_ffmpeg all enabled 1\n");
        helper_cfg_content.push_str(&format!(
            "mirv_movie_ffmpeg all options \"{} {{QUOTE}}{{AFX_STREAM_PATH}}\\{}{{QUOTE}}\"\n",
            codec_args,
            crate::hlcr::scanner::VIDEO_FILE,
        ));
        crate::log_markdown(&format!(
            "🎬 **Direct-to-video capture** — HLAE pipes frames to FFmpeg (`{}`) instead of \
             writing a BMP sequence, one `{}` per stream folder. Render Studio reads these the \
             same way it reads frame sequences.",
            codec_args,
            crate::hlcr::scanner::VIDEO_FILE,
        ));
    }

    // 1. Primer Job. Always first when there is any work, so a test asserting
    // on this queue gets one job more than the streak groups it passed in.
    if total_jobs > 0 {
        let first_source = sorted_groups[0].0.0.to_string();
        let mut primer_init = config.init_commands.clone();
        primer_init.push("sys_autodir".to_string());

        // Every patched demo lands directly in the game's own dod/ folder --
        // that's the only place GoldSrc's `playdemo` can find it. This used
        // to resolve to capture_directories[0] instead, with capture_engine.rs
        // copying it into dod/ as a second step -- pure redundant I/O, since
        // that copy loop already ran everything upfront before hl.exe even
        // launched, so dod/'s drive needed the full batch footprint either
        // way. capture_directories is only ever about where recorded video
        // blocks land now, not these small demo files. See issue #8.
        let primer_out = dod_dir.join(format!("{PRIMER_DEMO_STEM}.dem"));

        // Delay playdemo of the first chain to tick 500 (~5 seconds) to allow the engine to fully finish the
        // 2-second GoldSrc server handshake without buffer overflows before jumping to the first real chain.
        let mut primer_scheduled = Vec::new();
        helper_cfg_content.push_str("# Demo specific next demos\n");
        let primer_next_alias = format!("{PRIMER_DEMO_STEM}_next");
        helper_cfg_content.push_str(&format!(
            "alias {primer_next_alias} \"playdemo {CHAIN_DEMO_PREFIX}01\"\n"
        ));
        primer_scheduled.push((crate::patch::PRIMER_DELAY_TICKS, primer_next_alias));

        jobs.push(PatchJob {
            source_demo: first_source.clone(),
            output_demo: primer_out,
            streaks: Vec::new(),
            target_player: None,
            init_commands: primer_init,
            scheduled_commands: primer_scheduled,
            director_events: Vec::new(),
            block_routes: Vec::new(),
            blocks: Vec::new(),
        });
    }

    // 2. Chained Jobs
    // Every patched demo file now lands directly in dod/ (see the primer's
    // own resolution above), not on any capture_directories entry -- so
    // utilized_drives only ever needs to track drives that actually receive
    // a real recording block. See issue #8.
    let mut utilized_drives = std::collections::HashSet::new();
    for (job_idx, ((source_demo, target_player), mut streak_refs)) in
        sorted_groups.into_iter().enumerate()
    {
        // Sort by start_tick in ascending order
        streak_refs.sort_by_key(|(_, s)| s.start_tick);
        let (streak_payload_indices, streaks): (Vec<usize>, Vec<CaptureStreak>) = streak_refs
            .into_iter()
            .map(|(idx, s)| (idx, s.clone()))
            .unzip();

        let total_demo_frames = streaks.first().map(|s| s.total_demo_frames).unwrap_or(0);

        // One svc_director STUFFTEXT event per streak — label mirrors the highlight table:
        // "#<row>: <kill_count> kills: <timeline_string>"
        let mut director_events: Vec<(i32, String)> = streaks
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let label = format!("#{}: {} kills: {}", i + 1, s.kill_count, s.timeline_string);
                (s.start_tick, label)
            })
            .collect();

        if let Some(first_streak) = streaks.first() {
            let match_tick = first_streak.match_start_tick.unwrap_or(0);
            director_events.push((match_tick, "echo [dod-studio] MATCH_START".to_string()));
            let demo_end_tick = total_demo_frames;
            director_events.push((demo_end_tick, "echo [dod-studio] DEMO_END".to_string()));
        }
        director_events.sort_by_key(|e| e.0);

        let demo_fps = streaks
            .first()
            .map(|s| s.demo_fps)
            .filter(|&fps| fps > 0.0)
            .unwrap_or(30.0);

        let demo_name = format!("{}{:02}", CHAIN_DEMO_PREFIX, job_idx + 1);
        let next_demo_name = format!("{}{:02}", CHAIN_DEMO_PREFIX, job_idx + 2);
        let output_name = format!("{}.dem", demo_name);
        // Lands directly in dod/ -- see the primer's own resolution above for why.
        let output_demo = dod_dir.join(&output_name);

        // ── AOT failover routing (Per-Block) ───────────────────────────────────

        // Block cutting.
        //
        // Two separate questions, deliberately not conflated (they used to be,
        // which made every roll-window collision collapse into one take full of
        // dead air between the two highlights):
        //
        //  1. Do the recordings themselves overlap? Only then must the two
        //     highlights become a single take — you can't run two
        //     mirv_recordmovie sessions at once.
        //  2. Otherwise they stay separate takes, but if the fast-forward
        //     round trip between them doesn't fit, it's dropped and playback
        //     just stays at normal speed across the gap. Costs a couple of
        //     seconds of real-time playback and yields two clean clips instead
        //     of one blob. The stopsound flush goes with it — that exists to
        //     repair audio the fast-forward desyncs, and there's no
        //     fast-forward here to repair.
        //
        // `merged_sources` stays index-aligned with `merged_streaks`, recording
        // which dispatched highlights each block covers — the merge keeps only
        // the first streak's fields, so the mapping back to the caller's
        // highlights would otherwise be lost here. `chained_to_previous` marks
        // blocks that run straight on from the one before at normal speed.
        let dynamic_pre_roll_ticks = (config.pre_roll_seconds * demo_fps) as i32;
        let dynamic_post_roll_ticks = (config.post_roll_seconds * demo_fps) as i32;
        let start_lead_ticks = (config.record_start_lead * demo_fps) as i32;
        let stop_trail_ticks = (config.record_stop_trail * demo_fps) as i32;

        let mut merged_streaks: Vec<CaptureStreak> = Vec::new();
        let mut merged_sources: Vec<Vec<usize>> = Vec::new();
        let mut chained_to_previous: Vec<bool> = Vec::new();

        for (current, payload_idx) in streaks.into_iter().zip(streak_payload_indices) {
            if merged_streaks.is_empty() {
                merged_streaks.push(current);
                merged_sources.push(vec![payload_idx]);
                chained_to_previous.push(false);
                continue;
            }

            let prev_stop = last_kill_frame(merged_streaks.last().unwrap());
            let next_start = first_kill_frame(&current);

            // Recordings overlap, or sit too close for a safe stop/start cycle.
            // OBS needs longer between takes than HLAE does, and the number is
            // measured rather than guessed: ~1.065s to finalise a file after
            // `StopRecord` returned, against the 1.0s this guard promises. So
            // the HLAE-derived constant is already too tight for that path by a
            // small margin, and two highlights that merge today would otherwise
            // ask OBS for a stop/start cycle it cannot service.
            let min_separation_ticks = (take_separation_seconds(config) * demo_fps) as i32;
            if blocks_merge(
                prev_stop,
                next_start,
                start_lead_ticks,
                stop_trail_ticks + min_separation_ticks,
            ) {
                let last = merged_streaks.last_mut().unwrap();
                last.end_tick = last.end_tick.max(current.end_tick);
                // The record-stop mark is derived from kills[end_index] below,
                // not from end_tick, so the absorbed highlight's final kill has
                // to join this block's kill list too. Without it recording stops
                // at the *first* highlight's last kill and everything merged in
                // after that is missing from the take — while still looking like
                // one successfully captured block.
                let absorbed_last = current.end_index.min(current.kills.len().saturating_sub(1));
                if let Some(kill) = current.kills.get(absorbed_last).cloned() {
                    last.kills.push(kill);
                    last.end_index = last.kills.len() - 1;
                }
                merged_sources.last_mut().unwrap().push(payload_idx);
            } else {
                let rolls_collide = blocks_merge(
                    prev_stop,
                    next_start,
                    start_lead_ticks + dynamic_pre_roll_ticks,
                    stop_trail_ticks + dynamic_post_roll_ticks,
                );
                merged_streaks.push(current);
                merged_sources.push(vec![payload_idx]);
                chained_to_previous.push(rolls_collide);
            }
        }

        let mut block_routes = Vec::new();

        // Byte estimate per block, index-aligned with merged_streaks.
        let block_estimates: Vec<u64> = merged_streaks
            .iter()
            .map(|streak| {
                let anchor_duration =
                    ((streak.end_tick - streak.start_tick) as f32) / demo_fps.max(1.0);
                let clip_duration_secs = config.calculate_total_capture_duration(anchor_duration);
                crate::sys::disk::calculate_raw_sequence_bytes(
                    config.resolution_width,
                    config.resolution_height,
                    config.capture_fps,
                    clip_duration_secs,
                )
            })
            .collect();

        let assignments = allocate_blocks_first_fit_decreasing(
            &block_estimates,
            &mut drive_free,
            &mut active_drive_idx,
            FAILOVER_THRESHOLD,
        )
        .map_err(|_| {
            std::io::Error::other("Insufficient space across all mapped drives to allocate a block")
        })?;

        // Route aliases key off each block's original index in merged_streaks,
        // not allocation order, so the scheduled-command lookup below (which
        // still walks merged_streaks in its original order) is unaffected.
        let mut blocks: Vec<crate::patch::types::CaptureBlock> = Vec::new();
        for (block_index, drive_idx) in assignments {
            let streak = &merged_streaks[block_index];
            block_routes.push((streak.start_tick, streak.end_tick, drive_idx));
            utilized_drives.insert(drive_idx);
            helper_cfg_content.push_str(&format!(
                "alias {}_route_{} \"mirv_movie_filename _route_{}/{}_b{}\"\n",
                demo_name, block_index, drive_idx, demo_name, block_index
            ));

            // Mirror of the junction target built after this loop, so the two
            // can't drift: _route_{drive} links to <capture_dir>/<session_id>,
            // and HLAE writes <demo_name>_b<block_index> underneath it.
            let take_folder = match config.capture_directories.get(drive_idx) {
                Some(out_dir) => {
                    let absolute_drive = std::path::absolute(out_dir)?;
                    let session_root = if config.session_id.is_empty() {
                        absolute_drive
                    } else {
                        absolute_drive.join(&config.session_id)
                    };
                    session_root.join(format!("{}_b{}", demo_name, block_index))
                }
                // No capture directories configured — the drive_free sentinel
                // path. Falls back to the session dir resolved from
                // primary_media_dir above, matching where output actually lands.
                None => session_dir.join(format!("{}_b{}", demo_name, block_index)),
            };

            blocks.push(crate::patch::types::CaptureBlock {
                demo_name: demo_name.clone(),
                block_index,
                drive_index: drive_idx,
                take_key: crate::shared::paths::take_key(&take_folder).unwrap_or_default(),
                take_folder,
                source_streak_indices: merged_sources[block_index].clone(),
                start_tick: streak.start_tick,
                end_tick: streak.end_tick,
                // Filled in by the scheduling loop below, which is where the
                // record bounds are actually derived. Left at 0 here rather
                // than duplicating that arithmetic.
                record_start_tick: 0,
                record_stop_tick: 0,
            });
        }
        blocks.sort_by_key(|b| b.block_index);

        if job_idx < total_jobs - 1 {
            helper_cfg_content.push_str(&format!(
                "alias {}_next \"playdemo {}\"\n",
                demo_name, next_demo_name
            ));
        } else {
            helper_cfg_content.push_str(&format!(
                "alias {}_next \"sys_capture_done_path\"\n",
                demo_name
            ));
        }

        // Generate scheduled commands
        let mut scheduled_commands = Vec::new();

        // Initialize Engine Speed after Initial Load Delay
        let initial_delay_ticks = (config.initial_delay * demo_fps) as i32;
        scheduled_commands.push((initial_delay_ticks, "sys_fast_forward".to_string()));

        // One marker per demo, at its very first scheduled tick — the engine's
        // own live signal that this demo has started playing. Read back by
        // `native/src/obs/log_tail.rs` as `MarkerKind::DemoStart`, carrying
        // (job_idx, total_jobs, clip_count) 1-based, and forwarded up as
        // `EngineEvent::DemoLoading` for the "loading demo N of M" OS
        // notification (issue #98). `merged_streaks.len()` is this job's
        // final clip count, already settled by the merge loop above.
        for (t, echo_cmd) in build_safe_echos(
            initial_delay_ticks,
            &format!(
                "DEMO_START {} {} {}",
                job_idx + 1,
                total_jobs,
                merged_streaks.len()
            ),
        ) {
            scheduled_commands.push((t, echo_cmd));
        }

        // Fast-forward towards clip 1 starts at the very same tick as
        // DEMO_START above — playback has nothing to do before it. Read back
        // as `MarkerKind::NextClip` for the "fast-forwarding to clip N of M"
        // OS notification (issue #98).
        for (t, echo_cmd) in build_safe_echos(
            initial_delay_ticks,
            &format!(
                "NEXT_CLIP {} {} {} {}",
                job_idx + 1,
                total_jobs,
                1,
                merged_streaks.len()
            ),
        ) {
            scheduled_commands.push((t, echo_cmd));
        }

        for (i, streak) in merged_streaks.iter().enumerate() {
            let frame_times_ref = global_arrays
                .get(std::path::Path::new(&streak.source_demo))
                .map(|a| a.as_slice())
                .unwrap_or_else(|| streak.frame_times.as_slice());

            // Always use streak.total_demo_frames (which maps to the demo's true
            // playback_frames from the header) as the authoritative final frame count.
            // Do not trust frame_times_ref.len() on its own, as it is truncated
            // at the demo section boundary (type_byte == 5) during scanning.
            let absolute_final_frame = streak.total_demo_frames.max(frame_times_ref.len() as i32);
            let exit_frame = absolute_final_frame.saturating_sub(5);
            let danger_zone = absolute_final_frame.saturating_sub(10);

            // Resolve the physical frame index for the start/end kills directly from
            // kills[i].0 (= frame_index, the 1-based frame counter used during scanning),
            // which is aligned with frame_times_ref[].
            //
            // NOTE: Do NOT apply a .min(frame_times_ref.len()-1) clamp here.
            // When frame_times_ref is empty that saturates to 0 and destroys the index.
            // find_tick_backwards / find_tick_forwards already clamp start_frame
            // internally, and return start_frame as-is when frame_times is empty.
            //
            // NOTE: Do NOT use viewdemo_times here. viewdemo_times[i] stores
            // viewdemo_offset (= SVC_TIME server game clock, e.g. 300.0 s from server
            // start), while frame_times_ref[] contains demo-playback timestamps starting
            // near 0.0. The two domains are incompatible — a position() search would
            // always return None → unwrap_or(0) → every command collapsed to tick 0.
            let physical_frame = first_kill_frame(streak).max(0) as usize;
            let physical_end_frame = last_kill_frame(streak).max(0) as usize;

            let record_start_tick = find_tick_backwards(
                physical_frame,
                config.record_start_lead,
                frame_times_ref,
                demo_fps,
            );
            // Pre-roll is the settle window: playback drops back to normal speed
            // this far ahead of the record start so audio isn't glitched by the
            // fast-forward, then stopsound flushes just before recording begins.
            // The sound flush is clamped so it can never precede the speed drop
            // (which it would for any pre-roll under a second).
            let s_speed_tick = find_tick_backwards(
                record_start_tick.max(0) as usize,
                config.pre_roll_seconds,
                frame_times_ref,
                demo_fps,
            );
            let sound_lead = config.pre_roll_seconds.min(SOUND_FLUSH_LEAD_SECONDS);
            let s_sound_tick = find_tick_backwards(
                record_start_tick.max(0) as usize,
                sound_lead,
                frame_times_ref,
                demo_fps,
            );
            let mut r_stop = find_tick_forwards(
                physical_end_frame,
                config.record_stop_trail,
                frame_times_ref,
                demo_fps,
            );
            let mut s_end = find_tick_forwards(
                r_stop.max(0) as usize,
                config.post_roll_seconds,
                frame_times_ref,
                demo_fps,
            );

            let mut is_clutch = false;
            if s_end >= danger_zone {
                let demo_file_name = std::path::Path::new(&streak.source_demo)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                crate::log_markdown(&format!(
                    "⚠️ **EOF Boundary Reached:** Highlight #{} in demo '{}' (Player: {}) has a post-roll that exceeds the demo's end frame. Post-roll truncated to save batch.",
                    i + 1,
                    demo_file_name,
                    streak.target_player.as_deref().unwrap_or("Unknown")
                ));
                is_clutch = true;
                r_stop = r_stop.min(exit_frame);
                s_end = exit_frame;
            } else {
                r_stop = r_stop.min(exit_frame);
                s_end = s_end.min(exit_frame);
            }

            // Hand the finished record bounds back to the block. This loop and
            // the block-allocation loop above both walk `merged_streaks` in its
            // original order, so `i` is the block index — matched on rather
            // than indexed with, since allocation may reorder `blocks`.
            // The decal flush is the consumer: it needs the frames that end up
            // in the take, which `start_tick`/`end_tick` are not.
            if let Some(block) = blocks.iter_mut().find(|b| b.block_index == i) {
                block.record_start_tick = record_start_tick;
                block.record_stop_tick = r_stop;
            }

            // Custom command overrides
            for (idx, custom) in config.custom_commands.iter().enumerate() {
                let relation_str = match custom.relation {
                    CommandRelation::Before => "BEFORE",
                    CommandRelation::After => "AFTER",
                };
                let target_tick = match custom.relation {
                    CommandRelation::Before => {
                        let mut t = find_tick_backwards(
                            physical_frame,
                            custom.offset,
                            frame_times_ref,
                            demo_fps,
                        );
                        if t == s_speed_tick || t == s_sound_tick || t == record_start_tick {
                            t += 1;
                        }
                        t
                    }
                    CommandRelation::After => {
                        let mut t = find_tick_forwards(
                            physical_end_frame,
                            custom.offset,
                            frame_times_ref,
                            demo_fps,
                        );
                        if t == r_stop {
                            t += 1;
                        }
                        t
                    }
                };

                for (t, echo_cmd) in build_safe_echos(
                    target_tick,
                    &format!(
                        "CUSTOM_CMD{}_{} - Tick {}",
                        idx + 1,
                        relation_str,
                        target_tick
                    ),
                ) {
                    scheduled_commands.push((t, echo_cmd));
                }
                // Playback runs at `host_framerate 0.05` until the pre-roll
                // drops it back to real time at `s_speed_tick`, and resumes
                // fast-forwarding once the post-roll ends at `s_end`. A command
                // landing outside that window still executes, but it executes
                // while the engine is racing through frames with its audio
                // buffers in a bad state — so anything about sound, timing or
                // rendering does something other than what it looks like it
                // does, and nothing in the captured video explains why.
                if runs_during_fast_forward(target_tick, s_speed_tick, s_end) {
                    let (where_, fix) = if target_tick < s_speed_tick {
                        (
                            "before playback drops back to real time",
                            format!(
                                "keep the offset under the {:.1}s pre-roll, or raise the pre-roll",
                                config.pre_roll_seconds
                            ),
                        )
                    } else {
                        (
                            "after the post-roll ends and fast-forward resumes",
                            format!(
                                "keep the offset under the {:.1}s post-roll, or raise the post-roll",
                                config.post_roll_seconds
                            ),
                        )
                    };
                    crate::log_markdown(&format!(
                        "ℹ️ **Scheduled command runs during fast-forward** — `{}` is set {} {:.1}s \
                         {} the highlight, which lands at tick {}, {}. Setting a cvar there is \
                         usually fine — it simply takes effect early. It is worth checking only \
                         for commands that depend on playback running at real speed: anything \
                         touching sound, recording, or rendering, since the engine is at \
                         `host_framerate {}` with its audio buffers unflushed. To have it run \
                         at normal speed instead, {}.",
                        custom.command,
                        relation_str.to_lowercase(),
                        custom.offset,
                        if matches!(custom.relation, CommandRelation::Before) {
                            "before"
                        } else {
                            "after"
                        },
                        target_tick,
                        where_,
                        fast_forward_host_framerate(config),
                        fix
                    ));
                }
                scheduled_commands.push((target_tick, custom.command.clone()));
            }

            // Stages 1 and 1.5 are the exit from fast-forward. A block chained
            // to the one before it never left normal speed, so there's nothing
            // to drop back to and no fast-forward-induced audio drift to flush.
            let resumes_from_fast_forward = !chained_to_previous.get(i).copied().unwrap_or(false);

            if resumes_from_fast_forward {
                // At Speed Flush (Stage 1)
                scheduled_commands.push((s_speed_tick, "sys_normal_speed".to_string()));
                scheduled_commands.push((s_speed_tick + 1, "sys_normal_speed".to_string()));
                scheduled_commands.push((s_speed_tick + 2, "sys_normal_speed".to_string()));
                for (t, echo_cmd) in build_safe_echos(
                    s_speed_tick,
                    &format!("SPEED_FLUSH - Tick {}", s_speed_tick),
                ) {
                    scheduled_commands.push((t, echo_cmd));
                }

                // At Sound Flush (Stage 1.5)
                scheduled_commands.push((s_sound_tick, "sys_sound".to_string()));
                for (t, echo_cmd) in
                    build_safe_echos(s_sound_tick, &format!("AUDIO_SYNC - Tick {}", s_sound_tick))
                {
                    scheduled_commands.push((t, echo_cmd));
                }
            }

            // At Start Frame (Stage 2)
            scheduled_commands.push((record_start_tick, format!("{}_route_{}", demo_name, i)));
            scheduled_commands.push((record_start_tick, "sys_record_start".to_string()));
            for (t, echo_cmd) in build_safe_echos(
                record_start_tick,
                &format!("START_RECORD - Tick {}", record_start_tick),
            ) {
                scheduled_commands.push((t, echo_cmd));
            }

            // At End Frame (Stage 3)
            scheduled_commands.push((r_stop, "sys_record_stop".to_string()));
            for (t, echo_cmd) in build_safe_echos(r_stop, &format!("STOP_RECORD - Tick {}", r_stop))
            {
                scheduled_commands.push((t, echo_cmd));
            }

            // At Post-Roll End (Stage 4).
            // Skipped when the next block starts too soon for the round trip to
            // fit — playback just stays at normal speed into it instead.
            let next_block_chained = chained_to_previous.get(i + 1).copied().unwrap_or(false);
            if !next_block_chained {
                scheduled_commands.push((s_end, "sys_fast_forward".to_string()));
                for (t, echo_cmd) in
                    build_safe_echos(s_end, &format!("FAST_FORWARD - Tick {}", s_end))
                {
                    scheduled_commands.push((t, echo_cmd));
                }
                // `chained_to_previous.get(i + 1)` also returns the "false"
                // default when there simply is no block i+1 (the last block
                // in the demo) -- that's not a real next clip to announce, so
                // this needs its own bounds check rather than reusing
                // `!next_block_chained` alone.
                if i + 1 < merged_streaks.len() {
                    for (t, echo_cmd) in build_safe_echos(
                        s_end,
                        &format!(
                            "NEXT_CLIP {} {} {} {}",
                            job_idx + 1,
                            total_jobs,
                            i + 2,
                            merged_streaks.len()
                        ),
                    ) {
                        scheduled_commands.push((t, echo_cmd));
                    }
                }
            }

            if i == merged_streaks.len() - 1 {
                // At Absolute EOF
                if job_idx == total_jobs - 1 {
                    let echos = build_safe_echos(s_end, "BATCH_COMPLETE");
                    let echos_len = echos.len() as i32;
                    for (t, echo_cmd) in echos {
                        scheduled_commands.push((t, echo_cmd));
                    }
                    let final_tick = if is_clutch {
                        exit_frame
                    } else {
                        s_end + echos_len
                    };
                    scheduled_commands.push((final_tick, format!("{}_next", demo_name)));
                } else {
                    let final_tick = if is_clutch { exit_frame } else { s_end };
                    scheduled_commands.push((final_tick, format!("{}_next", demo_name)));
                }
            }
        }

        // Implement Global Breadcrumb Loop
        let total_demo_frames = merged_streaks
            .first()
            .map(|s| s.total_demo_frames)
            .unwrap_or(0);
        let mut step = 0;
        while step < total_demo_frames {
            scheduled_commands.push((
                step,
                format!("echo \"[dod-studio] BREADCRUMB - Tick {}\"", step),
            ));
            step += crate::patch::BREADCRUMB_INTERVAL_TICKS;
        }

        // Sort scheduled_commands by tick
        scheduled_commands.sort_by_key(|(tick, _)| *tick);

        let final_init_commands = final_init_commands(config);

        jobs.push(PatchJob {
            source_demo: source_demo.to_string(),
            output_demo,
            streaks: merged_streaks,
            target_player: target_player.map(|s| s.to_string()),
            init_commands: final_init_commands,
            scheduled_commands,
            director_events,
            block_routes,
            blocks,
        });
    }

    // Create directory junctions for utilized drives
    let game_path_buf = std::path::PathBuf::from(&config.game_path);
    let hl_exe_parent = game_path_buf.parent().unwrap_or(std::path::Path::new(""));
    for &drive_idx in &utilized_drives {
        if let Some(out_dir) = config.capture_directories.get(drive_idx) {
            let absolute_drive = std::path::absolute(out_dir)?;
            let session_dir = if !config.session_id.is_empty() {
                absolute_drive.join(&config.session_id)
            } else {
                absolute_drive
            };

            if !session_dir.exists() {
                let _ = std::fs::create_dir_all(&session_dir);
            }

            let junction_path = hl_exe_parent.join(format!("_route_{}", drive_idx));
            let _ = std::fs::remove_dir(&junction_path);

            let junction_str = junction_path.to_str().unwrap_or_default();
            let target_str = session_dir.to_str().unwrap_or_default();

            if !junction_str.is_empty() && !target_str.is_empty() {
                let _ = std::process::Command::new("cmd")
                    .args(["/C", "mklink", "/J", junction_str, target_str])
                    .output();
            }
        }
    }

    // Write dodstudio_helper.cfg to dod_dir
    if !dod_dir.exists() {
        std::fs::create_dir_all(&dod_dir)?;
    }
    let cfg_path = dod_dir.join("dodstudio_helper.cfg");
    std::fs::write(&cfg_path, helper_cfg_content)?;

    // Final per-drive headroom for every drive this batch actually touches,
    // handed back so the pre-launch abort in `capture_engine.rs` re-validates
    // the exact numbers this allocation pass already computed instead of
    // recomputing a third, narrower (primary-drive-only) answer.
    let drive_headroom: Vec<crate::patch::types::DriveHeadroom> = utilized_drives
        .into_iter()
        .filter_map(|idx| {
            config
                .capture_directories
                .get(idx)
                .map(|p| crate::patch::types::DriveHeadroom {
                    path: p.clone(),
                    free_bytes: drive_free[idx],
                })
        })
        .collect();

    Ok((jobs, drive_headroom))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod grouping_tests;
