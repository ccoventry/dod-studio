//! `dodstudio_helper.cfg`, the cfg the game execs at a batch's start: the
//! aliases every patched demo's scheduled commands call by name, and the
//! direct-to-video settings. A demo frame carries only an alias's name, so
//! the 64-byte ConsoleCommand limit does not apply to what is defined here.

use super::*;

/// The helper cfg's file name, in the game's mod folder.
const HELPER_CFG: &str = "dodstudio_helper.cfg";

/// Removes the cfg files an earlier batch left in `dod_dir`: the helper cfg,
/// `dodstudio_capture_done.cfg`, `dod_quit.cfg` and every
/// `dodstudio_chain_*.cfg`. A file that won't go is left for the new batch
/// to overwrite (the helper cfg) or ignore (the rest, which nothing execs).
pub(super) fn remove_previous_batch_cfgs(dod_dir: &std::path::Path) {
    let _ = std::fs::remove_file(dod_dir.join(HELPER_CFG));
    let _ = std::fs::remove_file(dod_dir.join("dodstudio_capture_done.cfg"));
    let _ = std::fs::remove_file(dod_dir.join("dod_quit.cfg"));
    if let Ok(entries) = std::fs::read_dir(dod_dir) {
        for entry in entries.flatten() {
            let filename = entry.file_name().to_string_lossy().to_string();
            if filename.starts_with("dodstudio_chain_") && filename.ends_with(".cfg") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// Writes the finished helper cfg into `dod_dir`, making the folder first if
/// it is missing.
pub(super) fn write_helper_cfg(dod_dir: &std::path::Path, content: &str) -> std::io::Result<()> {
    if !dod_dir.exists() {
        std::fs::create_dir_all(dod_dir)?;
    }
    std::fs::write(dod_dir.join(HELPER_CFG), content)
}

/// The helper cfg's start, ahead of any job's own lines: its header, the
/// global aliases every job calls (speed, sound, record, batch end) and, in
/// direct-to-video mode, HLAE's FFmpeg settings. `dod_dir`'s configs are
/// scanned for the values the normal-speed alias puts back.
pub(super) fn helper_cfg_preamble(config: &PatcherConfig, dod_dir: &std::path::Path) -> String {
    let date_time = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let mut helper_cfg_content = String::new();
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
    // The fast-forward runs uncapped with nothing drawn, and the pre-roll
    // puts back whatever the user's own commands and configs had (#724). An
    // alias body is not a ConsoleCommand frame: the demo carries only the
    // name, so the 64-byte frame limit does not apply to it.
    helper_cfg_content.push_str(&speed_alias_lines(
        config,
        &final_init_commands(config),
        Some(&crate::patch::cfg_scan::scan_cached(dod_dir)),
    ));
    helper_cfg_content.push_str(&ramp_alias_lines(config));
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
    //
    // AGR mode has its own pair — see `record_alias_lines`.
    helper_cfg_content.push_str(&record_alias_lines(config));
    helper_cfg_content.push_str("alias sys_capture_done_path \"mirv_movie_filename DOD_STUDIO_EXIT_TRIGGER; mirv_recordmovie_start; mirv_recordmovie_stop\"\n");

    if config.capture_mode == crate::patch::CaptureMode::DirectToVideo {
        helper_cfg_content.push_str(&direct_to_video_lines(config));
    }
    helper_cfg_content
}

/// Direct-to-video's HLAE settings (docs/direct_to_video_capture.md).
///
/// The probe that introduced this settled all four of its open questions
/// against a real capture: {AFX_STREAM_PATH} resolves to the per-stream
/// folder, it follows mirv_movie_filename (so the _route_N junction routing
/// keeps working), takes are still auto-numbered into take0000, and
/// sound.wav still lands beside the video.
///
/// `all` here is HLAE's stream *group*, not the folder name it happens to
/// share with the composited stream. Measured with mirv_movie_separate_hud
/// on: the group does reach hudColor and hudAlpha, so all three streams get
/// a video and no per-stream enabled/options lines are needed. The folders
/// HLAE writes are lowercase (`hudcolor`/`hudalpha`) even though the command
/// names those streams in camelCase.
///
/// Set here, once, at load — never as an injected ConsoleCommand frame. The
/// options string is several times the 64 bytes a ConsoleCommand frame's
/// command field holds, with no staggering available (one argument to one
/// command), and an
/// injected frame would shift every later frame ordinal by +1 and desync the
/// scheduled capture commands. Same rule r_decals follows.
///
/// Not inside an alias either: GoldSrc cannot parse nested quotes in one, and
/// this argument has to carry them. As a plain command in an exec'd cfg it is
/// ordinary syntax. {QUOTE} is HLAE's own token for a literal quote, which is
/// what keeps the inner path quoting away from the engine's parser entirely.
///
/// The codec comes from the config — see `CaptureCodec` for why every option
/// is lossless and RGB/4:4:4, and why the size ranking there is not the
/// real-time-viability ranking.
fn direct_to_video_lines(config: &PatcherConfig) -> String {
    let codec_args = config.ffmpeg_capture_codec.args();
    let mut lines = String::new();
    lines.push_str("\n# Direct-to-video capture\n");
    lines.push_str("mirv_movie_ffmpeg all enabled 1\n");
    lines.push_str(&format!(
        "mirv_movie_ffmpeg all options \"{} {{QUOTE}}{{AFX_STREAM_PATH}}\\{}{{QUOTE}}\"\n",
        codec_args,
        crate::hlcr::scanner::VIDEO_FILE,
    ));
    lines
}

/// The helper cfg's `sys_record_start` / `sys_record_stop` definitions for this
/// batch's capture mode.
///
/// Every mode schedules the same two alias names at the same ticks, so the
/// patched demos are identical whatever the mode — only what the names do
/// changes, and that lives here.
///
/// AGR mode (#450): `mirv_agr` records one frame per `Host_Frame`, stamped with
/// `host_frametime`, so the recorded rate is whatever `host_framerate` pins it
/// to (#403 did exactly this by hand). Start pins it to `1 / agr_fps`; stop
/// ends the recording and hands playback back to real time for the post-roll,
/// the way `mirv_recordmovie_stop` releases its own timestep. The recording
/// itself is started by the block's route alias, which is the one place that
/// knows the block's file — see `route_alias_line`.
pub(super) fn record_alias_lines(config: &PatcherConfig) -> String {
    match config.capture_mode {
        crate::patch::CaptureMode::FrameSequence | crate::patch::CaptureMode::DirectToVideo => {
            "alias sys_record_start \"mirv_recordmovie_start; stopsound\"\n\
             alias sys_record_stop \"mirv_recordmovie_stop\"\n"
                .to_string()
        }
        // No `stopsound` at the record start, unlike HLAE's modes: OBS has been
        // recording for a second by then (it starts at AUDIO_SYNC, whose own
        // `stopsound` clears the fast-forward's sound), and nothing changes
        // the timestep here, so the stop would only cut the clip's sound.
        // Measured (#645): with it, every OBS clip had ~250 ms of silence at
        // its start; without it the sound runs on unbroken.
        crate::patch::CaptureMode::Obs => {
            "alias sys_record_start \"echo [dod-studio] OBS_MODE_NO_HLAE_START\"\n\
             alias sys_record_stop \"echo [dod-studio] OBS_MODE_NO_HLAE_STOP\"\n"
                .to_string()
        }
        crate::patch::CaptureMode::Agr => format!(
            "alias sys_record_start \"host_framerate {}\"\n\
             alias sys_record_stop \"mirv_agr stop; host_framerate 0\"\n",
            agr_host_framerate(config)
        ),
    }
}

/// The helper cfg line defining a block's `<demo>_route_<N>` alias, scheduled
/// at the block's record start just ahead of `sys_record_start`.
///
/// For the movie modes it points `mirv_movie_filename` at the block's take
/// folder; HLAE makes the folder itself. In AGR mode it starts the recording
/// into that folder instead (made ahead of the batch by `build_batch_queue`).
pub(super) fn route_alias_line(
    config: &PatcherConfig,
    demo_name: &str,
    block_index: usize,
    drive_idx: usize,
) -> String {
    if config.capture_mode == crate::patch::CaptureMode::Agr {
        format!(
            "alias {}_route_{} \"mirv_agr start {}\"\n",
            demo_name,
            block_index,
            agr_route_path(drive_idx, demo_name, block_index)
        )
    } else {
        format!(
            "alias {}_route_{} \"mirv_movie_filename _route_{}/{}_b{}\"\n",
            demo_name, block_index, drive_idx, demo_name, block_index
        )
    }
}

/// The helper cfg line defining chain job `job_idx`'s `<demo>_next` alias,
/// run after its last clip: it plays the next chain demo, or, after the
/// batch's last job, triggers the capture-done exit.
pub(super) fn next_alias_line(demo_name: &str, job_idx: usize, total_jobs: usize) -> String {
    if job_idx < total_jobs - 1 {
        format!(
            "alias {}_next \"playdemo {}\"\n",
            demo_name,
            chain_demo_name(job_idx + 1)
        )
    } else {
        format!("alias {}_next \"sys_capture_done_path\"\n", demo_name)
    }
}
