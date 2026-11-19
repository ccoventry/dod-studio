//! The init commands a capture runs at demo load, and the check that each
//! fits a demo's command frame.

use crate::decals::MAX_RENDER_DECALS;
use crate::patch::MAX_CONSOLE_CMD_SAFE_LEN;
use crate::patch::builder::{RUN_IN_BACKGROUND, STAY_BEHIND, obs_game_fps_max};
use crate::patch::types::PatcherConfig;

/// The commands in `commands` that cannot fit one ConsoleCommand frame (64
/// bytes or more, so no room for the NUL), in the order given. Initial and
/// Scheduled Commands are each written into exactly one frame, as typed, so
/// these have to be refused before a batch starts (#453) rather than reach
/// `engine::write_console_cmd`.
pub fn too_long_commands(commands: &[String]) -> Vec<String> {
    commands
        .iter()
        .filter(|c| c.len() > MAX_CONSOLE_CMD_SAFE_LEN)
        .cloned()
        .collect()
}

/// The user's own Initial Commands, then the project's field of view
/// (`PatcherConfig::picture_fov`, #1085) when it sets one, with the lines'
/// own `mirv_fov <n>` left out (`picture_fov::lines_sent`): what every job
/// starts its init lines with, the primer included.
///
/// The field of view goes straight after the user's lines so it wins over
/// all of them, a cfg they `exec` included. Every way the game runs these
/// keeps that order: a patched demo writes them as `ConsoleCommand` frames
/// in this order right after `DemoStart` (`engine.rs`; the demo filter drops
/// an `exec` there anyway), and the hook's batch runs each demo's `init`
/// lines in file order through `ClientCmd` (`runtime_batch.rs`, goldsrc-hooks
/// `batch.rs`), every demo of the batch again. An `exec` runs its file
/// before the next line: the game's `Cmd_Exec_f` inserts the file's text at
/// the front of the command buffer (`Cbuf_InsertText`), ahead of the lines
/// still queued after it. One line of `mirv_fov <n>` is far under the
/// 64-byte frame limit.
///
/// `mirv_fov handleZoom enabled 1` goes right before it (owner decision 10B
/// a), so the override follows the scope zoom instead of holding while
/// scoped, unless the user's lines already set `handleZoom` either way
/// (`picture_fov::handle_zoom`): theirs stands.
pub fn user_init_commands(config: &PatcherConfig) -> Vec<String> {
    use super::picture_fov::{HANDLE_ZOOM_ON, HandleZoom, fov_line, handle_zoom, lines_sent};
    // Its own `mirv_fov <n>` lines left out: the project's comes after them.
    let mut out = lines_sent(config);
    if let Some(fov) = config.picture_fov {
        if handle_zoom(config) == Some(HandleZoom::Added) {
            out.push(HANDLE_ZOOM_ON.to_string());
        }
        out.push(fov_line(fov));
    }
    out
}

/// Every command the engine will run at demo load: the user's own init
/// commands and the project's field of view ([`user_init_commands`]), then
/// the ones the pipeline adds for itself.
///
/// Extracted so it can be asked ahead of a capture as well as during one. The
/// app's own additions override whatever the game's configs set — `capture_fps`
/// beats a `mirv_movie_fps` in `movie.cfg`, and the decal pin beats an
/// `r_decals` there — and a user is entitled to know that before it happens
/// rather than by noticing the result.
pub fn final_init_commands(config: &PatcherConfig) -> Vec<String> {
    let mut out = user_init_commands(config);
    out.push("sys_autodir".to_string());
    // AGR mode's sound-only movie (#450) runs at the AGR rate: the movie
    // recorder pins the timestep to `1 / mirv_movie_fps`, and that has to be
    // the timestep the `.agr` is stamped with.
    let movie_fps = if config.capture_mode == crate::patch::CaptureMode::Agr {
        config.effective_agr_fps()
    } else {
        config.capture_fps
    };
    out.push(format!("mirv_movie_fps {movie_fps}"));

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
        && super::ring_limit_from_init(&config.init_commands).is_none()
        && super::ring_limit_from_game_config(config).is_none()
    {
        let ring = config.decal_ring_limit.min(MAX_RENDER_DECALS);
        if ring > 0 && ring < MAX_RENDER_DECALS {
            out.push(format!("r_decals {}", ring));
        }
    }

    out
}
