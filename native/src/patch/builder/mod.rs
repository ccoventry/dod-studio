// patch/builder/mod.rs
// Batch job construction: `build_batch_queue`, the steps it runs in order,
// and the init commands it adds. The helpers it uses live beside it —
// `blocks` (highlights into chain jobs and recording blocks), `schedule`
// (each job's scheduled commands and director events), `helper_cfg` (the
// helper cfg's aliases), `timing` (tick walks, roll floors, block merging),
// `speed` (the fast-forward and normal-speed aliases), `kill_gaps` (long
// gaps between kills recorded sped up, #831), `alloc` (drive allocation),
// `workspace` (junctions and the legacy channel-based worker spawner),
// `preview` (preview patches).
// Calls std::fs::create_dir_all and std::thread::spawn — native-only.

use crate::patch::engine::StreamPatcher;
use crate::patch::types::{
    CaptureStreak, CaptureWorker, CommandRelation, PatchEvent, PatchJob, PatcherConfig,
};
use crate::shared::paths::{CHAIN_DEMO_PREFIX, PRIMER_DEMO_STEM};
use std::sync::{Arc, atomic::AtomicBool};

mod alloc;
mod blocks;
mod helper_cfg;
pub mod kill_gaps;
mod preview;
mod schedule;
mod speed;
mod timing;
mod workspace;

use alloc::DriveBudget;
// The builder's tests reach these through `use super::*`.
#[cfg(test)]
use alloc::{allocate_blocks_first_fit_decreasing, no_room_message};
use blocks::{Blocks, HighlightGroup, cut_blocks, group_highlights};
#[cfg(test)]
use helper_cfg::record_alias_lines;
use helper_cfg::{
    helper_cfg_preamble, next_alias_line, remove_previous_batch_cfgs, route_alias_line,
    write_helper_cfg,
};
pub use preview::*;
use schedule::{
    ChainJob, ClipWindow, JobSchedule, clip_window, director_events, frame_times_for,
    log_post_roll_cut,
};
pub use speed::*;
pub use timing::*;
pub use workspace::*;

/// The `fps_max` OBS mode gives the game: twice OBS's rate (see
/// `final_init_commands`), within the engine's own 1000 cap. A rate that
/// isn't positive counts as 60, the OBS preflight's floor.
pub(crate) fn obs_game_fps_max(config: &PatcherConfig) -> i32 {
    let obs_fps = if config.obs_capture_fps > 0 {
        config.obs_capture_fps
    } else {
        60
    };
    obs_fps.saturating_mul(2).min(1000)
}

/// The `host_framerate` that makes `mirv_agr` record at `effective_agr_fps`.
pub fn agr_host_framerate(config: &PatcherConfig) -> f32 {
    1.0 / config.effective_agr_fps() as f32
}

/// The `.agr` file a block records to, inside its take folder. Named after the
/// block rather than a fixed `take.agr` so a file copied out of its folder
/// still says which take it is.
pub fn agr_file_name(demo_name: &str, block_index: usize) -> String {
    format!("{demo_name}_b{block_index}.agr")
}

/// The path `mirv_agr start` is given for a block: through the same
/// `_route_<drive>` junction `mirv_movie_filename` uses, so it lands in the
/// block's `take_folder`.
///
/// Relative, and unquoted, on purpose. Both were settled offline rather than
/// assumed (#450's one open question):
///
/// - **Relative works.** `AfxHookGoldSrc.dll` (2.25.3) hands `mirv_agr start`'s
///   argument, converted to UTF-16 and otherwise untouched, straight to
///   `_wfopen_s` (the call at `+0x28bfb`). A relative path therefore resolves
///   against `hl.exe`'s working directory, which the custom loader leaves at
///   the game root (`docs/hlae_protocols.md`) — the folder the `_route_N`
///   junctions are made in.
/// - **An absolute path cannot get here at all.** GoldSrc's tokenizer splits a
///   bare word at `:` (`COM_Parse`; addresses here are the pre-Anniversary
///   `hw.dll`'s, `0x1d2b1ae`), so `C:\x.agr` reaches HLAE as `C` — #403's
///   "writes nothing unquoted". It must be
///   quoted, but an alias body cannot carry quotes: `alias` rebuilds its body
///   from the parsed arguments (`0x1d27cf0`), dropping every quote. `exec` of
///   a per-block cfg is no way round it either — from a demo it is refused
///   unless the file is on a short class/map-config whitelist (`0x1d27858`),
///   and an alias invoked from a demo runs its body with the same privilege
///   (`0x1d28647`). And a quoted absolute path typed straight into a
///   ConsoleCommand frame has 64 bytes to fit in, which a real capture folder
///   plus a file name does not.
///
/// The relative form contains none of the characters the tokenizer splits on
/// (`{ } ( ) ' , :`, quotes, whitespace), so it survives both the alias and
/// the tokenizer as one argument. It also never appears in a demo frame: the
/// frame carries only the `<demo>_route_<N>` alias name, as in every mode.
pub fn agr_route_path(drive_idx: usize, demo_name: &str, block_index: usize) -> String {
    format!(
        "_route_{drive_idx}/{demo_name}_b{block_index}/{}",
        agr_file_name(demo_name, block_index)
    )
}

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

    // A batch runs at full speed with another window in front: without it the
    // engine waits up to 20 ms for input every frame while it isn't the active
    // app, which slows the fast-forward between clips 5-8x (measured on both
    // builds). The hook DLL's setting; without the hook it is an unknown
    // command and does nothing. Whether the window stays behind is a separate
    // setting, sent as 0 too when the user wants to watch, so a game left
    // running from an earlier batch (#666) drops it.
    out.push(format!("{RUN_IN_BACKGROUND} 1"));
    out.push(format!(
        "{STAY_BEHIND} {}",
        u8::from(config.keep_game_behind)
    ));

    // OBS mode is real time: HLAE issues no `mirv_movie_start` at all, so
    // `mirv_movie_fps` above is inert on this path — nothing reads it. OBS
    // samples whatever frame the game last drew, once per frame of its own
    // `obs_capture_fps` (obs::provision::ensure_dod_studio_setup), so the
    // game has to draw faster than that: at exactly OBS's rate the two beat
    // against each other and OBS repeats a frame every time the game is a
    // little late. Measured (#645, POST, 120 fps): 1.5-2.1% repeated frames
    // with `fps_max` at OBS's rate, 0.2-0.8% with the game drawing faster.
    // Hence twice OBS's rate, and `gl_vsync 0`, since vsync would cap the
    // game at the monitor's refresh, below OBS's rate on a 60 Hz screen.
    // `fps_override 1` first: GoldSrc's default `fps_max` ceiling (~100) is
    // below these, and `fps_max` alone is silently clamped under it.
    if config.capture_mode == crate::patch::CaptureMode::Obs {
        out.push("fps_override 1".to_string());
        out.push(format!("fps_max {}", obs_game_fps_max(config)));
        out.push("gl_vsync 0".to_string());
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

/// The hook DLL's camera follow for HLTV demos (goldsrc-hooks
/// `spectator_follow.rs`, #206): the command that puts the camera on a
/// player by number, and the setting that keeps it there through their death.
pub const SPEC_TARGET: &str = "ds_spec_target";
pub const SPEC_LOCK: &str = "ds_spec_lock";
/// The game's first-person spectator mode (HL's `OBS_IN_EYE`): an HLTV demo
/// otherwise plays in a chase camera behind the player.
pub const IN_EYE: &str = "spec_mode 4";

/// The hook DLL's `ds_capture_run_in_background` (goldsrc-hooks
/// `run_in_background.rs`), which every batch turns on.
pub const RUN_IN_BACKGROUND: &str = "ds_capture_run_in_background";

/// Bytes one recording block is expected to need on its capture drive: the
/// figure `build_batch_queue` routes blocks to drives by.
fn block_byte_estimate(config: &PatcherConfig, streak: &CaptureStreak, demo_fps: f32) -> u64 {
    let anchor_duration = ((streak.end_tick - streak.start_tick) as f32) / demo_fps.max(1.0);
    let clip_duration_secs = config.calculate_total_capture_duration(anchor_duration);
    if config.capture_mode == crate::patch::CaptureMode::Agr {
        crate::sys::disk::calculate_agr_bytes(config.effective_agr_fps(), clip_duration_secs)
    } else if config.capture_mode == crate::patch::CaptureMode::Obs {
        // OBS's encoder takes, by its quality setting (#1096).
        crate::sys::disk::calculate_obs_bytes(
            config.obs.recording_quality,
            config.resolution_width,
            config.resolution_height,
            config.obs_capture_fps,
            clip_duration_secs,
        )
    } else if config.capture_mode == crate::patch::CaptureMode::DirectToVideo
        && config.ffmpeg_capture_codec != crate::patch::types::CaptureCodec::RawVideo
    {
        // A compressed take, not raw frames (#1096).
        crate::sys::disk::calculate_lossless_video_bytes(
            config.resolution_width,
            config.resolution_height,
            config.capture_fps,
            clip_duration_secs,
        )
    } else {
        crate::sys::disk::calculate_raw_sequence_bytes(
            config.resolution_width,
            config.resolution_height,
            config.capture_fps,
            clip_duration_secs,
        )
    }
}

/// What a whole batch is expected to need on its capture drives, before
/// `build_batch_queue` runs: the automatic take cleanup (#644) makes room for
/// this much. Each highlight is billed as its own block, so two highlights
/// that `build_batch_queue` later merges into one recording count their
/// shared footage twice: an upper bound, never an under-count.
pub fn estimate_batch_capture_bytes(streaks: &[CaptureStreak], config: &PatcherConfig) -> u64 {
    estimate_batch_capture_blocks(streaks, config)
        .into_iter()
        .fold(0u64, u64::saturating_add)
}

/// What each recording block of a batch is expected to need on the one
/// capture drive it is written to, before `build_batch_queue` runs: the
/// pieces the automatic take cleanup (#644) makes room for, drive by drive.
/// Highlights of one demo and player whose recordings would merge
/// (`blocks_merge`, as `build_batch_queue` decides it) count as one block,
/// billed at the sum of their own estimates, so a block is never
/// under-counted.
pub fn estimate_batch_capture_blocks(
    streaks: &[CaptureStreak],
    config: &PatcherConfig,
) -> Vec<u64> {
    let fps_of = |streak: &CaptureStreak| {
        Some(streak.demo_fps)
            .filter(|&fps| fps > 0.0)
            .unwrap_or(30.0)
    };
    let mut order: Vec<&CaptureStreak> = streaks.iter().collect();
    order.sort_by(|a, b| {
        (&a.source_demo, a.player_index, first_kill_frame(a)).cmp(&(
            &b.source_demo,
            b.player_index,
            first_kill_frame(b),
        ))
    });
    let mut blocks: Vec<u64> = Vec::new();
    let mut prev: Option<(&str, usize, i32)> = None;
    for streak in order {
        let demo_fps = fps_of(streak);
        let bytes = block_byte_estimate(config, streak, demo_fps);
        let lead = (config.record_start_lead * demo_fps) as i32;
        let trail =
            ((config.record_stop_trail + take_separation_seconds(config)) * demo_fps) as i32;
        let merges = prev.is_some_and(|(demo, player, prev_end)| {
            demo == streak.source_demo
                && player == streak.player_index
                && blocks_merge(prev_end, first_kill_frame(streak), lead, trail)
        });
        let end = last_kill_frame(streak);
        match (merges, blocks.last_mut(), prev.as_mut()) {
            (true, Some(last), Some(p)) => {
                *last = last.saturating_add(bytes);
                p.2 = p.2.max(end);
            }
            _ => {
                blocks.push(bytes);
                prev = Some((&streak.source_demo, streak.player_index, end));
            }
        }
    }
    blocks
}

/// The hook DLL's `ds_capture_stay_behind` (goldsrc-hooks `hlae_window.rs`),
/// which every batch sets from `PatcherConfig::keep_game_behind`.
pub const STAY_BEHIND: &str = "ds_capture_stay_behind";

/// On a capture batch's game launch line, with `keep_game_behind` on: the hook
/// DLL keeps the window behind from the moment it opens, before the pipe can
/// carry the setting (goldsrc-hooks `hlae_window::START_BEHIND_FLAG`; keep the
/// two in step). The engine ignores a `-` argument it doesn't know.
pub const START_BEHIND_ARG: &str = "-dodstudio_start_behind";

/// Whether a patched batch plays the primer demo before its first chain job,
/// given whether the launch injects the hook DLL (`config.goldsrc_hooks_dll()`,
/// the test `build_hlae_process` makes).
///
/// The primer loads the first map once so the real first demo isn't dark, and
/// plays [`crate::patch::PRIMER_DELAY_TICKS`] before `playdemo`. With the hook
/// DLL neither is needed (#365): its `lightmap_gamma` fixes the first load's
/// lighting (#397), the first demo goes over its pipe only after config.cfg
/// and movie.cfg have run (#705), and `GameHandoff`'s settle wait covers the
/// delay. Every chain job carries `init_commands` and `sys_autodir` itself
/// ([`final_init_commands`]). Without the DLL nothing else covers the first
/// two, so the primer stays.
pub fn plays_primer(hook_dll: bool) -> bool {
    !hook_dll
}

/// Builds a capture batch: one [`PatchJob`] per demo and player whose
/// highlights `raw_streaks` lists (plus the primer first when
/// [`plays_primer`]), each block routed to a capture drive, and the helper
/// cfg every job's scheduled commands call into. Returns the jobs in play
/// order and the headroom left on each capture drive the batch records to.
///
/// `global_arrays` holds each demo's frame times when the caller has them,
/// by demo path; a highlight's own `frame_times` stand in otherwise.
///
/// Side effects, in the game's folder: removes the previous batch's cfg
/// files, writes `dodstudio_helper.cfg`, makes the `_route_<N>` junctions
/// and the session folders, and in AGR mode every block's take folder.
/// Logs a warning for rolls shorter than the capture needs, a note for each
/// Scheduled Command that runs during fast-forward and each post-roll cut at
/// a demo's end.
///
/// Errors when `game_path` has no parent folder or `primary_media_dir` is
/// unset (`NotFound`), when a block fits on no capture drive, and when a
/// folder or the helper cfg can't be made.
pub fn build_batch_queue(
    raw_streaks: Vec<CaptureStreak>,
    config: &PatcherConfig,
    global_arrays: &std::collections::HashMap<std::path::PathBuf, std::sync::Arc<Vec<f32>>>,
) -> Result<(Vec<PatchJob>, Vec<crate::patch::types::DriveHeadroom>), std::io::Error> {
    let groups = group_highlights(&raw_streaks);
    warn_short_rolls(config);

    let dod_dir = game_mod_dir(config)?;
    remove_previous_batch_cfgs(&dod_dir);
    // Snapshot every capture directory's free bytes so each clip is routed
    // to a drive with room for it at build time.
    let mut drives = DriveBudget::snapshot(&config.capture_directories);
    let session_dir = make_session_dir(config)?;

    let mut helper_cfg = helper_cfg_preamble(config, &dod_dir);
    announce_capture_mode(config);

    let total_jobs = groups.len();
    let mut jobs = Vec::new();
    if total_jobs > 0 {
        helper_cfg.push_str("# Demo specific next demos\n");
    }
    // The primer, only when the launch has no hook DLL ([`plays_primer`],
    // #365). It is then first, so a test asserting on this queue gets one job
    // more than the streak groups it passed in; with the DLL the game starts
    // on dodstudio_chain_01 directly.
    if let Some(first_group) = groups.first()
        && plays_primer(config.goldsrc_hooks_dll().is_some())
    {
        let (primer, alias_line) = primer_job(config, first_group.source_demo, &dod_dir);
        helper_cfg.push_str(&alias_line);
        jobs.push(primer);
    }

    let batch = BatchContext {
        config,
        highlights: &raw_streaks,
        global_arrays,
        dod_dir: &dod_dir,
        session_dir: &session_dir,
        total_jobs,
    };
    for (job_idx, group) in groups.into_iter().enumerate() {
        jobs.push(chain_job(
            &batch,
            job_idx,
            group,
            &mut drives,
            &mut helper_cfg,
        )?);
    }

    // Every patched demo lands in dod/, never on a capture directory, so the
    // drives in use are only those that receive a recording block (#8).
    let utilized_drives: std::collections::HashSet<usize> = jobs
        .iter()
        .flat_map(|j| j.blocks.iter().map(|b| b.drive_index))
        .collect();
    make_route_junctions(config, &utilized_drives)?;
    make_agr_take_folders(config, &jobs)?;
    write_helper_cfg(&dod_dir, &helper_cfg)?;

    let drive_headroom = drives.headroom(utilized_drives, &config.capture_directories);
    Ok((jobs, drive_headroom))
}

/// What every chain job of a batch reads and none changes.
struct BatchContext<'a> {
    config: &'a PatcherConfig,
    /// The batch's highlights as dispatched, for the `viewdemo` clock.
    highlights: &'a [CaptureStreak],
    global_arrays: &'a std::collections::HashMap<std::path::PathBuf, std::sync::Arc<Vec<f32>>>,
    dod_dir: &'a std::path::Path,
    session_dir: &'a std::path::Path,
    total_jobs: usize,
}

/// The game's mod folder (`dod/`) beside `config.game_path`: where every
/// patched demo and the helper cfg go, since it is the only place GoldSrc's
/// `playdemo` finds a demo (#8). `NotFound` when `game_path` has no parent.
fn game_mod_dir(config: &PatcherConfig) -> std::io::Result<std::path::PathBuf> {
    match std::path::Path::new(&config.game_path).parent() {
        Some(parent) => Ok(crate::shared::paths::mod_dir(parent)),
        None => Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Target game output directory not found",
        )),
    }
}

/// The batch's session folder under `primary_media_dir` (the folder itself
/// when there is no session id), made if missing: where a block's take lands
/// when no capture directory is configured. `NotFound` when
/// `primary_media_dir` is unset.
fn make_session_dir(config: &PatcherConfig) -> std::io::Result<std::path::PathBuf> {
    let Some(active_export_dir) = config.primary_media_dir.clone() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Target game output directory not found",
        ));
    };
    let session_dir = if !config.session_id.is_empty() {
        active_export_dir.join(&config.session_id)
    } else {
        active_export_dir
    };
    if !session_dir.exists() {
        // A folder that can't be made shows up when the first take can't be
        // written there, which names the folder.
        let _ = std::fs::create_dir_all(&session_dir);
    }
    Ok(session_dir)
}

/// Says in the activity log how a direct-to-video or AGR batch records,
/// since neither writes the frame sequence the other modes do.
fn announce_capture_mode(config: &PatcherConfig) {
    if config.capture_mode == crate::patch::CaptureMode::DirectToVideo {
        crate::log_markdown(&format!(
            "🎬 **Direct-to-video capture** — HLAE pipes frames to FFmpeg (`{}`) instead of \
             writing a BMP sequence, one `{}` per stream folder. Render Studio reads these the \
             same way it reads frame sequences.",
            config.ffmpeg_capture_codec.args(),
            crate::hlcr::scanner::VIDEO_FILE,
        ));
    }

    if config.capture_mode == crate::patch::CaptureMode::Agr {
        crate::log_markdown(&format!(
            "🦴 **AGR capture** — HLAE records no video. Each clip is saved as one `.agr` file \
             (models and camera, for the Blender page) at {} fps, in its take folder.",
            config.effective_agr_fps(),
        ));
    }
}

/// The patched demo stem of chain job `job_idx` (0-based):
/// `dodstudio_chain_01` for the first.
fn chain_demo_name(job_idx: usize) -> String {
    format!("{}{:02}", CHAIN_DEMO_PREFIX, job_idx + 1)
}

/// The primer job, played before the first chain job when the launch has no
/// hook DLL: `first_source` (the first chain job's demo) patched to load its
/// map and, after [`crate::patch::PRIMER_DELAY_TICKS`], play
/// `dodstudio_chain_01`. Returns the job and the helper cfg line defining
/// the alias it schedules.
fn primer_job(
    config: &PatcherConfig,
    first_source: &str,
    dod_dir: &std::path::Path,
) -> (PatchJob, String) {
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
    let primer_next_alias = format!("{PRIMER_DEMO_STEM}_next");
    let alias_line = format!("alias {primer_next_alias} \"playdemo {CHAIN_DEMO_PREFIX}01\"\n");
    let primer_scheduled = vec![(crate::patch::PRIMER_DELAY_TICKS, primer_next_alias)];

    let job = PatchJob {
        source_demo: first_source.to_string(),
        output_demo: primer_out,
        streaks: Vec::new(),
        target_player: None,
        init_commands: primer_init,
        scheduled_commands: primer_scheduled,
        director_events: Vec::new(),
        block_routes: Vec::new(),
        blocks: Vec::new(),
    };
    (job, alias_line)
}

/// Builds chain job `job_idx` from one demo and player's highlights: cuts
/// them into blocks, routes each block to a drive in `drives`, schedules
/// every clip, and appends the job's route and `_next` aliases to
/// `helper_cfg`. Errors when a block fits on no drive or a capture
/// directory's path can't be made absolute.
fn chain_job(
    batch: &BatchContext,
    job_idx: usize,
    group: HighlightGroup,
    drives: &mut DriveBudget,
    helper_cfg: &mut String,
) -> std::io::Result<PatchJob> {
    let config = batch.config;
    let first = group.first();

    // An HLTV demo's camera follows no one by itself (#247): every clip
    // puts it on the highlight's player, by number (slot + 1), and the
    // lock keeps it there through their death. A job is one player's
    // highlights (grouped by demo and player).
    let spectate = first
        .spectated
        .then(|| format!("{SPEC_TARGET} {}", first.player_index + 1));
    let director_events = director_events(&group);
    let demo_fps = Some(first.demo_fps)
        .filter(|&fps| fps > 0.0)
        .unwrap_or(30.0);
    let total_demo_frames = first.total_demo_frames;

    let demo_name = chain_demo_name(job_idx);
    // Lands directly in dod/ -- see `primer_job` for why.
    let output_demo = batch.dod_dir.join(format!("{}.dem", demo_name));

    let cut = cut_blocks(&group.highlights, config, demo_fps);
    let block_estimates: Vec<u64> = cut
        .streaks
        .iter()
        .map(|streak| block_byte_estimate(config, streak, demo_fps))
        .collect();
    let assignments = drives.allocate(&block_estimates, config)?;
    let (mut blocks, block_routes) =
        capture_blocks(batch, &demo_name, &cut, &assignments, helper_cfg)?;
    helper_cfg.push_str(&next_alias_line(&demo_name, job_idx, batch.total_jobs));

    let spectated = spectate.is_some();
    let job = ChainJob {
        demo_name: &demo_name,
        index: job_idx,
        total: batch.total_jobs,
        clip_count: cut.streaks.len(),
        demo_fps,
        spectate,
    };
    let scheduled_commands = schedule_clips(batch, job, &cut, &mut blocks, total_demo_frames);

    let mut final_init_commands = final_init_commands(config);
    if spectated {
        final_init_commands.push(format!("{SPEC_LOCK} 1"));
    }

    Ok(PatchJob {
        source_demo: group.source_demo.to_string(),
        output_demo,
        streaks: cut.streaks,
        target_player: group.target_player.map(|s| s.to_string()),
        init_commands: final_init_commands,
        scheduled_commands,
        director_events,
        block_routes,
        blocks,
    })
}

/// One [`CaptureBlock`](crate::patch::types::CaptureBlock) per block of
/// `cut`, on the drive `assignments` gives it, sorted by block index, and
/// the job's `(start_tick, end_tick, drive)` block routes. Appends each
/// block's route alias to `helper_cfg`, in allocation order. The record
/// bounds and plan are left empty for [`schedule_clips`] to fill.
fn capture_blocks(
    batch: &BatchContext,
    demo_name: &str,
    cut: &Blocks,
    assignments: &[(usize, usize)],
    helper_cfg: &mut String,
) -> std::io::Result<(
    Vec<crate::patch::types::CaptureBlock>,
    Vec<(i32, i32, usize)>,
)> {
    // Route aliases key off each block's original index in `cut`, not
    // allocation order, so the scheduling (which walks `cut` in its
    // original order) is unaffected.
    let mut block_routes = Vec::new();
    let mut blocks: Vec<crate::patch::types::CaptureBlock> = Vec::new();
    for &(block_index, drive_idx) in assignments {
        let streak = &cut.streaks[block_index];
        block_routes.push((streak.start_tick, streak.end_tick, drive_idx));
        helper_cfg.push_str(&route_alias_line(
            batch.config,
            demo_name,
            block_index,
            drive_idx,
        ));

        let take_folder = take_folder(batch, demo_name, block_index, drive_idx)?;
        blocks.push(crate::patch::types::CaptureBlock {
            demo_name: demo_name.to_string(),
            block_index,
            drive_index: drive_idx,
            take_key: crate::shared::paths::take_key(&take_folder).unwrap_or_default(),
            take_folder,
            source_streak_indices: cut.sources[block_index].clone(),
            start_tick: streak.start_tick,
            end_tick: streak.end_tick,
            // Filled in by `schedule_clips`, which is where the record
            // bounds are actually derived. Left at 0 here rather than
            // duplicating that arithmetic.
            record_start_tick: 0,
            record_stop_tick: 0,
            ramped_gaps: 0,
            plan: None,
        });
    }
    blocks.sort_by_key(|b| b.block_index);
    Ok((blocks, block_routes))
}

/// The folder a block's take lands in.
///
/// Mirror of the junction target `make_route_junctions` builds, so the two
/// can't drift: `_route_{drive}` links to `<capture_dir>/<session_id>`, and
/// HLAE writes `<demo_name>_b<block_index>` underneath it. With no capture
/// directories configured (the drive budget's sentinel), the session dir
/// under `primary_media_dir`, where output actually lands.
fn take_folder(
    batch: &BatchContext,
    demo_name: &str,
    block_index: usize,
    drive_idx: usize,
) -> std::io::Result<std::path::PathBuf> {
    let folder = format!("{}_b{}", demo_name, block_index);
    Ok(match batch.config.capture_directories.get(drive_idx) {
        Some(out_dir) => {
            let absolute_drive = std::path::absolute(out_dir)?;
            let session_root = if batch.config.session_id.is_empty() {
                absolute_drive
            } else {
                absolute_drive.join(&batch.config.session_id)
            };
            session_root.join(folder)
        }
        None => batch.session_dir.join(folder),
    })
}

/// A chain job's scheduled commands, sorted by tick: its start, every
/// clip's stages and Scheduled Commands, the hand-over and the breadcrumbs
/// up to `total_demo_frames`.
///
/// Also hands each of `blocks` the record window, ramp count and take plan
/// its clip was just scheduled with: the decal flush needs the frames that
/// end up in the take, which `start_tick`/`end_tick` are not, and the take
/// checks (#1084) need the plan.
fn schedule_clips(
    batch: &BatchContext,
    job: ChainJob,
    cut: &Blocks,
    blocks: &mut [crate::patch::types::CaptureBlock],
    total_demo_frames: i32,
) -> Vec<(i32, String)> {
    let config = batch.config;
    let demo_fps = job.demo_fps;
    let mut schedule = JobSchedule::start(config, job);
    let ramp_speed = config.kill_gaps.ramp_speed();

    for (i, streak) in cut.streaks.iter().enumerate() {
        let frame_times = frame_times_for(streak, batch.global_arrays);
        let window = clip_window(streak, frame_times, config, demo_fps);
        if window.cut_at_demo_end {
            log_post_roll_cut(i, streak);
        }

        // On the clock the hook records on (`viewdemo_clock`), from the
        // same highlights `runtime_batch` builds it from: `frame_times`
        // stops at the demo's loading segment and never reaches a clip's
        // frames.
        let clock =
            crate::patch::viewdemo_clock::clock_for(&streak.source_demo, batch.highlights).ok();
        let timeline = kill_gaps::Timeline {
            frame_times,
            fps: demo_fps,
            clock: clock.as_ref(),
        };
        // Long gaps between kills, recorded sped up (#831). Inside the
        // record window by construction: between two of its kills.
        let ramps = match ramp_speed {
            Some(_) => kill_gaps::ramps(
                &cut.kill_frames[i],
                config.kill_gaps.over_seconds,
                &timeline,
                (window.record_start_tick, window.record_stop_tick),
            ),
            None => Vec::new(),
        };
        // `blocks` is sorted by block index and `cut` is in block order, so
        // `i` is the block index — matched on rather than indexed with.
        if let Some(block) = blocks.iter_mut().find(|b| b.block_index == i) {
            block.record_start_tick = window.record_start_tick;
            block.record_stop_tick = window.record_stop_tick;
            block.ramped_gaps = ramps.len();
            block.plan = take_plan(&timeline, &window, &cut.kill_frames[i], &ramps, ramp_speed);
        }

        schedule.push_custom_commands(&window, frame_times, streak);
        schedule.push_clip_stages(i, &window, &cut.chained_to_previous, &ramps);
    }

    schedule.push_breadcrumbs(total_demo_frames);
    schedule.into_sorted()
}

/// The plan a block's take is checked against (#1084): its length and where
/// each of `kill_frames` falls in it, every kill the block records, the
/// merged highlights' too, so the check finds any of them outside the
/// video. A ramp makes the video shorter than the window, which the plan
/// carries. `None` without a `viewdemo` clock.
fn take_plan(
    timeline: &kill_gaps::Timeline,
    window: &ClipWindow,
    kill_frames: &[i32],
    ramps: &[kill_gaps::Ramp],
    ramp_speed: Option<f32>,
) -> Option<crate::hlcr::take_plan::TakePlan> {
    let clock = timeline.clock?;
    let mut plan = crate::hlcr::take_plan::plan_for_block(
        |tick| clock.seconds(tick),
        window.record_start_tick,
        window.record_stop_tick,
        kill_frames,
    )?;
    let start = clock.seconds(window.record_start_tick);
    plan.sped_up = kill_gaps::sped_up(ramps, ramp_speed.unwrap_or(1.0), |tick| {
        clock.seconds(tick) - start
    });
    Some(plan)
}

/// Makes every block's take folder ahead of an AGR batch.
///
/// `mirv_agr start` opens its file with a plain `_wfopen_s` and creates no
/// folders (#403: a missing parent means it says "Started AGR recording."
/// and writes nothing). HLAE's movie recorder makes its own take folder, so
/// only this mode needs them made in advance. Failing here is loud on
/// purpose: the alternative is a batch that runs to the end and saves
/// nothing.
fn make_agr_take_folders(config: &PatcherConfig, jobs: &[PatchJob]) -> std::io::Result<()> {
    if config.capture_mode == crate::patch::CaptureMode::Agr {
        for block in jobs.iter().flat_map(|j| j.blocks.iter()) {
            std::fs::create_dir_all(&block.take_folder)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod grouping_tests;
