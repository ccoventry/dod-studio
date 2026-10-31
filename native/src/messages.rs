// messages.rs
// native's own error-string catalog — the same idea as
// studio/src-tauri/src/messages.rs (itself the counterpart to the
// frontend's strings.js), but crate-local: native has no direct UI of its
// own, but many of its `Result<T, String>` errors bubble straight through a
// Tauri command (studio's messages.rs wraps them with extra context,
// e.g. `failed_to_patch_preview_demo`, but the inner text — what actually
// went wrong — is authored here). Scoped to the modules studio's
// Tauri layer actually calls (patch::map_fetch, shared::hlae_ffmpeg,
// obs::session), plus the user-facing output of the real command-line
// binaries (`preview_cli`, `dod-studio-cli`, `check_maps`, `check_cfgs`; see
// the `bin/` sections below). Not in scope: the dev-only binaries
// (`dod-studio-dump`, `dod-studio-inspect`, `strip_decals`) and
// `native/examples/`, whose output is for the developer running them
// (#237, D19). Issue #33.

use std::fmt::Display;

// ── patch/map_fetch.rs ───────────────────────────────────────────────────

pub fn not_a_usable_map_name(map_name: &str) -> String {
    format!("`{}` is not a usable map name", map_name)
}

pub const MAP_MIRRORS_MUST_BE_HTTPS: &str = "map mirrors must be https";

/// Generic "<path or URL>: <underlying error>" — the shape every I/O/network
/// failure in this file reduces to once the specific operation is already
/// clear from surrounding context (a scratch-file write, a download, a
/// directory create). Six independently-authored copies of this exact
/// `format!("{}: {}", ...)` collapsed into one call.
pub fn labeled(label: impl Display, err: impl Display) -> String {
    format!("{}: {}", label, err)
}

pub fn served_unreadable_bsp(url: &str, err: impl Display) -> String {
    format!("what {} served is not a readable BSP: {}", url, err)
}

pub fn served_unparseable_map(url: &str, err: impl Display) -> String {
    format!("what {} served does not parse as a map: {}", url, err)
}

pub fn served_wrong_build(url: &str, got: u32, want: u32) -> String {
    format!(
        "{} served build {:08x}, but the demo needs {:08x} — not installing it",
        url, got, want
    )
}

pub fn could_not_move_existing_aside(target: impl Display, err: impl Display) -> String {
    format!("could not move the existing {} aside: {}", target, err)
}

pub fn url_returned_status(url: &str, status: impl Display) -> String {
    format!("{} returned {}", url, status)
}

// ── patch/scanner.rs ─────────────────────────────────────────────────────

pub fn source_demo_unreadable(demo: impl Display) -> String {
    format!(
        "{} could not be read. It was scanned, but has since been moved, deleted or \
         locked. Put it back, or remove its highlights and scan again.",
        demo
    )
}

pub fn source_demo_changed(demo: impl Display) -> String {
    format!(
        "{} is not the file that was scanned: it has been replaced or edited since. \
         Scan it again before capturing, so the highlights match what is in it now.",
        demo
    )
}

// ── shared/hlae_ffmpeg.rs ────────────────────────────────────────────────

pub fn not_a_file(path: impl Display) -> String {
    format!("{} is not a file", path)
}

pub fn could_not_run(path: impl Display, err: impl Display) -> String {
    format!("could not run {}: {}", path, err)
}

pub fn did_not_respond_to_version_flag(path: impl Display) -> String {
    format!("{} did not respond to -version", path)
}

pub fn did_not_identify_as_ffmpeg(path: impl Display) -> String {
    format!("{} did not identify itself as FFmpeg", path)
}

/// A picked file that isn't named like FFmpeg: refused without starting it.
pub fn not_named_ffmpeg(name: impl Display) -> String {
    format!(
        "{} isn't ffmpeg.exe, so it wasn't started to check it. Pick ffmpeg.exe",
        name
    )
}

pub fn reports_itself_as(path: impl Display, banner: &str) -> String {
    format!(
        "{} is not FFmpeg — it reports itself as \"{}\"",
        path, banner
    )
}

// ── obs/session.rs ───────────────────────────────────────────────────────

/// Independently authored twice (`start_record`/`stop_record`'s
/// `ObsError::Transport` guard) for the same "no client, never connected"
/// state.
pub const NOT_CONNECTED_TO_OBS: &str = "not connected to OBS";

pub fn obs_reported_file_missing(recorded: impl Display) -> String {
    format!("OBS reported {} but no file is there", recorded)
}

pub fn could_not_move_after_retries(
    recorded: impl Display,
    target: impl Display,
    retry_secs: f64,
    last_err: impl Display,
) -> String {
    format!(
        "could not move {} to {} after {} of retries: {}",
        recorded,
        target,
        crate::utils::duration_text::duration_text(retry_secs),
        last_err
    )
}

// ── hd/setup.rs ──────────────────────────────────────────────────────────

pub const HD_SETUP_CANCELLED: &str = "cancelled";

pub fn hd_download_incomplete(url: &str, got: u64, want: u64) -> String {
    format!("{url} stopped after {got} of {want} bytes")
}

// ── blender/mod.rs ───────────────────────────────────────────────────────

pub const BLENDER_CANCELLED: &str = "cancelled";

pub const BLENDER_IMPORT_FIRST: &str = "Import the .agr first: this step opens the imported scene.";

pub const BLENDER_SCENE_FIRST: &str =
    "Build the scene first: this step renders the textured scene.";

pub const BLENDER_RENDER_FIRST: &str = "Render the frames first: this step encodes them.";

pub fn blender_missing_input(path: &str) -> String {
    format!("{path} is not a file")
}

pub fn blender_no_assets(dir: &str) -> String {
    format!("{dir} has no models folder; choose the folder Crowbar decompiled the models into")
}

pub fn blender_bad_map(name: &str) -> String {
    format!("{name:?} is not a map name")
}

pub fn blender_step_failed(status: impl Display, detail: &str) -> String {
    if detail.is_empty() {
        format!("Blender stopped ({status})")
    } else {
        format!(
            "Blender stopped ({status}):
{detail}"
        )
    }
}

// ── hd/build.rs ──────────────────────────────────────────────────────────

pub const HD_BUILD_NO_GAME_FOLDER: &str =
    "The Half-Life Executable path has no folder to build into.";

pub const HD_BUILD_NOTHING_CHOSEN: &str = "Pick at least one style and one kind of file to build.";

pub fn hd_build_bad_style(style: &str) -> String {
    format!("{style:?} is not a style name: lowercase letters, digits, - and _ only")
}

pub fn hd_build_bad_type(kind: &str) -> String {
    format!("{kind:?} is not something the build makes")
}

pub fn hd_build_bad_cap(cap: u32) -> String {
    format!("{cap} is not a size the build makes: 1024, 2048 or 4096")
}

pub fn hd_build_unknown_style(style: &str) -> String {
    format!("{style:?} is neither a built-in style nor one in my_styles.txt")
}

pub fn hd_build_needs_upscaler(style: &str) -> String {
    format!("The {style} style needs the upscaler and its model: use Download first")
}

pub fn hd_build_failed(status: impl Display, detail: &str) -> String {
    if detail.is_empty() {
        format!("the build stopped ({status})")
    } else {
        format!(
            "the build stopped ({status}):
{detail}"
        )
    }
}

// ── hd/preview.rs ────────────────────────────────────────────────────────

pub fn hd_preview_bad_map(map: &str) -> String {
    format!("{map:?} is not a map name")
}

pub fn hd_preview_failed(status: impl Display, detail: &str) -> String {
    if detail.is_empty() {
        format!("the comparison stopped ({status})")
    } else {
        format!(
            "the comparison stopped ({status}):
{detail}"
        )
    }
}

// ── hd/map_list.rs ───────────────────────────────────────────────────────

pub const HD_MAP_LIST_EMPTY: &str = "The map list picks no maps, so a build would make no map textures. Add a map, or choose Every map.";

// ── hd/my_styles.rs ──────────────────────────────────────────────────────
// The scripts' own wording (styles.py's load_my_styles), so a file refused
// on the HD page reads the same as one refused on the command line.

/// Where a problem in the custom-style form is, in place of a line number.
pub const HD_STYLE_FORM: &str = "New style";

pub fn hd_style_expected_eq(where_: &str, line: &str) -> String {
    format!("{where_}: expected `name = ...`, got {line:?}")
}

pub fn hd_style_bad_name(where_: &str, name: &str) -> String {
    format!("{where_}: {name:?} -- style names are lowercase letters, digits, - and _ only")
}

pub fn hd_style_built_in(where_: &str, name: &str) -> String {
    format!("{where_}: {name:?} is a built-in style; pick another name")
}

pub fn hd_style_bad_value(where_: &str, value: &str) -> String {
    format!("{where_}: can't read {value:?}; see my_styles.example.txt")
}

pub fn hd_style_blends_unknown(name: &str, source: &str) -> String {
    format!("my_styles.txt: {name} blends {source:?}, which isn't a style")
}

pub fn hd_style_blends_itself(name: &str) -> String {
    format!("{name} can't blend itself")
}

pub fn hd_build_bad_my_styles(error: &str) -> String {
    format!("my_styles.txt needs fixing before a build: {error}")
}

pub fn hd_build_needs_model(style: &str, model: &str) -> String {
    format!("The {style} style needs the model {model} in the upscaler's models folder")
}

pub fn hd_not_an_upscaler_folder(dir: impl Display) -> String {
    format!("{dir} has no realesrgan-ncnn-vulkan.exe; choose the folder it is in")
}

pub fn hd_download_checksum(url: &str, got: &str, want: &str) -> String {
    format!("{url} is not the expected file (SHA-256 {got}, expected {want}); nothing was kept")
}

pub fn hd_unzip_failed(zip: impl Display, status: impl Display) -> String {
    format!("could not unpack {zip} ({status})")
}

pub fn hd_zip_missing_upscaler(dir: impl Display) -> String {
    format!(
        "the Real-ESRGAN zip didn't contain realesrgan-ncnn-vulkan.exe; unzip it into {dir} by hand"
    )
}

// ── sys/hook_copy.rs (#1146) ──────────────────────────────────────────────

/// What the activity log says after removing hook DLL copies no game uses.
pub fn hook_copies_removed(count: usize, root: impl Display) -> String {
    let (noun, verb) = if count == 1 {
        ("copy", "uses")
    } else {
        ("copies", "use")
    };
    format!("goldsrc-hooks: removed {count} hook DLL {noun} no game {verb} from {root}")
}

// ── patch/trimmed_copies.rs (#207) ────────────────────────────────────────

/// Why a trimmed copy wasn't saved: its drive is too full. Read after "no
/// trimmed copy could be made: ".
pub fn no_room_for_trimmed_copy(folder: &std::path::Path, free: u64, needed: u64) -> String {
    let gb = |b: u64| b as f64 / (1024.0 * 1024.0 * 1024.0);
    let mb = |b: u64| b as f64 / (1024.0 * 1024.0);
    format!(
        "the drive it goes on ({}) has {:.1} GB free, and the copy needs {:.0} MB plus {:.0} GB to spare; free some space there and try again",
        folder.display(),
        gb(free),
        mb(needed),
        gb(crate::shared::movie_copy::FREE_SPACE_HEADROOM_BYTES)
    )
}

/// The activity log's line when a demo's trimmed copy is made again because
/// its map's `.bsp` is not the one the copy was made with.
pub fn trimmed_copy_map_changed(demo: &str) -> String {
    format!(
        "✂️ **Trimmed copy** — `{demo}`'s map isn't the one its trimmed copy was made with (the .bsp changed, or was added or removed), so the copy is made again"
    )
}

// ── crash_maps.rs (#207) ──────────────────────────────────────────────────

/// What the hook log says about a crash that ended a batch.
pub fn crash_explained(what: &str, site: &str, map: Option<&str>, cause: Option<&str>) -> String {
    let on = map.map(|m| format!(" on {m}")).unwrap_or_default();
    match cause {
        Some(cause) => format!("The game crashed{on} ({what} at {site}). {cause}"),
        None => format!(
            "The game crashed{on} ({what} at {site}), a crash DoD Studio doesn't know yet. goldsrc-hooks/tools/crash_report.py shows what led up to it."
        ),
    }
}

// ── hlcr/combine.rs (#107) ────────────────────────────────────────────────

pub const COMBINE_NEEDS_TWO: &str = "pick at least two clips to combine";
pub const COMBINE_OUTPUT_IS_INPUT: &str =
    "the combined video can't be saved over one of the clips going into it";
pub const COMBINE_CANCELLED: &str = "cancelled";

pub fn combine_unreadable_clip(path: &str) -> String {
    format!("FFmpeg can't read {path} as a video")
}

pub fn combine_failed(log: &str) -> String {
    if log.is_empty() {
        "FFmpeg stopped without saying why".to_string()
    } else {
        format!("FFmpeg failed: {log}")
    }
}

// ── hlcr/take_frames.rs (#1069) ───────────────────────────────────────────

pub const TAKE_FRAMES_CANCELLED: &str = "cancelled";

pub fn take_frames_no_stream(stream: &str, take: impl Display) -> String {
    format!("{take} has no stream folder `{stream}` with frames or a video in it")
}

pub fn take_frames_no_length(video: impl Display) -> String {
    format!("FFmpeg can't tell how long {video} is, so no frames were picked")
}

pub fn take_frames_no_frame(at: &str, source: impl Display) -> String {
    format!("FFmpeg made no picture at {at} in {source}")
}

// ── sys/wmi_late_start.rs (#1243) ─────────────────────────────────────────
// Activity-log lines.

pub fn late_launcher_watch_started(secs: u64) -> String {
    format!(
        "[launch] Watching {secs} s for a launcher the cancelled WMI call may still start, to end it"
    )
}

pub fn late_launcher_watch_failed(err: impl Display) -> String {
    format!(
        "[launch] Could not watch for a launcher the cancelled WMI call may still start ({err}); \
         one that starts late keeps running"
    )
}

pub fn late_launcher_ended(pid: u32, games: usize, after_secs: f64) -> String {
    format!(
        "[launch] The cancelled WMI call started the launcher (PID {pid}) late, {after_secs:.1} s \
         into the watch; ended it and {games} game(s) it had started"
    )
}

pub fn late_launcher_none(secs: u64) -> String {
    format!("[launch] The cancelled WMI call started no launcher within {secs} s")
}

pub fn late_launcher_watch_superseded() -> String {
    "[launch] Stopped watching for the cancelled WMI call's launcher: a new launch began"
        .to_string()
}

pub fn take_frames_timed_out(secs: u64) -> String {
    format!("FFmpeg took over {secs} s for one picture and was stopped")
}

pub fn take_frames_bad_stream_name(stream: &str) -> String {
    format!("`{stream}` isn't a stream folder name")
}

// ── hlcr/take_checks.rs (#1084) ───────────────────────────────────────────

pub fn take_checks_no_rate(take: impl Display) -> String {
    format!("{take}'s frame rate isn't known, so its frames can't be timed for the checks")
}

pub fn take_checks_unnumbered(take: impl Display) -> String {
    format!(
        "{take}'s frame files aren't numbered the way HLAE numbers them, so they weren't checked"
    )
}

pub fn take_checks_timed_out(minutes: u64) -> String {
    format!("FFmpeg took over {minutes} min checking the take and was stopped")
}

// ── demo_rename.rs (#469) ─────────────────────────────────────────────────

pub const RENAME_NOT_IN_PLACE: &str =
    "a demo is only renamed within its own folder, never moved to another";
pub const RENAME_SOURCE_GONE: &str = "the demo is no longer there";
pub const RENAME_TARGET_EXISTS: &str = "a file with the new name is already there";

pub fn rename_bad_name(name: &str) -> String {
    format!("`{name}` isn't a usable demo name")
}

// ── capture_engine/watch.rs (#237) ────────────────────────────────────────
// Activity-log lines and the reasons a batch fails with.

pub fn stall_log_line(stalled_for: impl Display, never_started: bool) -> String {
    format!(
        "[HLAE] No console markers for {stalled_for} with hl.exe still running — treating the batch as stalled. {}",
        if never_started {
            "Playback never produced a single marker, so the demo most likely failed to load."
        } else {
            "`disconnect` typed in the console, a demo that ended early, and a frozen engine all look like this from out here."
        }
    )
}

pub fn stall_reason(never_started: bool) -> &'static str {
    if never_started {
        "the demo never started: the game came up, but no demo began playing in it. Check that the \
         hook DLL is up to date (an older one may not start this batch) and that the demo file is \
         still where Studio found it"
    } else {
        "the batch stopped progressing while hl.exe was still running — no console markers arrived for several minutes"
    }
}

pub const BORROWED_GAME_CLOSED_LOG: &str = "[HLAE] Closing the game that was already open \
    (the batch borrowed it): an error box stops the game until it is answered, and the boxes it \
    shows (a fatal error, a failed assertion) end it once they are (#1231)";

pub fn error_box_closed_borrowed_game(summary: &str) -> String {
    format!(
        "{summary} DoD Studio closed the game you had open: it was stuck on that error box \
         and couldn't carry on."
    )
}

pub const COMPLETION_VIA_PIPE: &str = "BATCH_COMPLETE from the game events pipe";
pub const COMPLETION_VIA_OBS_MARKER: &str = "BATCH_COMPLETE marker";

pub fn completion_via_folders(done_marker: bool, exit_trigger: bool) -> String {
    format!("done marker: {done_marker}, exit trigger: {exit_trigger}")
}

pub fn obs_recording_game_waited(waited_ms: u128) -> String {
    format!("[OBS] Recording; the game waited {waited_ms} ms for it and plays on")
}

pub fn hold_sent(command: impl Display) -> String {
    format!("[HLAE] Sent `{command}`")
}

pub fn hold_send_failed(command: impl Display, outcome: impl std::fmt::Debug) -> String {
    format!("[HLAE] Could not send `{command}` yet ({outcome:?}); trying again")
}

pub fn launcher_exited(after: impl Display, status: impl Display) -> String {
    format!(
        "[HLAE] Launcher exited after {after} (status: {status}) — expected handoff behaviour, not a crash by itself"
    )
}

pub fn launcher_try_wait_failed(err: impl Display) -> String {
    format!("[HLAE] try_wait() failed: {err}")
}

pub fn game_up_batch_started(pid: u32, lines: impl Display) -> String {
    format!("[HLAE] The game is up (PID {pid}); started the batch: `{lines}`")
}

pub fn handoff_failed(why: impl Display) -> String {
    format!("[HLAE] {why}")
}

pub fn error_box_seen(pid: u32, after: impl Display, text: impl Display) -> String {
    format!("[HLAE] Error box from PID {pid} after {after}: {text}")
}

pub fn error_box_answered_ignore(text: impl Display) -> String {
    format!(
        "[HLAE] The game was already exiting and couldn't be ended; answered its box with Ignore: {text}"
    )
}

pub fn cancelled_by_user(after: impl Display) -> String {
    format!("[HLAE] Cancelled by user after {after}")
}

pub const LAUNCHER_CLOSED_TOO: &str =
    "[HLAE] Closed the launcher too: it had not handed off to hl.exe yet";

pub fn obs_refused_abort(why: impl Display) -> String {
    format!("[HLAE] {why} — aborting rather than finishing the batch with nothing recorded.")
}

pub const OBS_UNREACHABLE_LOG: &str = "[HLAE] OBS is unreachable and could not be reconnected — aborting rather than finishing the batch with nothing recorded.";

pub const OBS_LOST_REASON: &str = "lost contact with OBS mid-batch and could not reconnect";

pub fn batch_complete_log(
    after: impl Display,
    via: impl Display,
    leaving_game_open: bool,
) -> String {
    format!(
        "[HLAE] Batch complete after {after} (via {via}) — {}",
        if leaving_game_open {
            "leaving the game open"
        } else {
            "taskkilling hl.exe"
        }
    )
}

pub fn game_never_started_log(after: impl Display) -> String {
    format!(
        "[HLAE] hl.exe never came up after the launcher exited ({after} elapsed) — treating as failure"
    )
}

pub const GAME_NEVER_STARTED_REASON: &str = "hl.exe never started after the HLAE launcher exited. Check that Steam is running and signed in to an account that owns Day of Defeat";

pub fn game_went_away_log(after: impl Display) -> String {
    format!(
        "[HLAE] hl.exe is gone with no exit trigger after {after} — either the game was closed (quit / ALT+F4 / End Process) or it crashed"
    )
}

pub fn game_crash_log(crash: impl Display) -> String {
    format!("[HLAE] {crash}")
}

pub const GAME_ENDED_EARLY_REASON: &str = "hl.exe ended before the batch finished — the game was either closed manually or crashed (no exit trigger was written)";

/// The loud warning for a console log the engine isn't writing.
pub fn console_log_not_written(log_path: impl Display, unchanged_for: impl Display) -> String {
    format!(
        "⚠️ **The engine is not writing its console log.** `{log_path}` is unchanged \
         {unchanged_for} after the game started, so dod-studio is getting no console \
         markers from it.\n\n\
         Per-demo progress, fast-forward-to-clip notifications, crash \
         detection and OBS capture all read that log — without it they do \
         not fail, they simply never fire, and this batch may finish looking \
         like it worked.\n\n\
         `-condebug` is passed on every launch and this is the folder the \
         engine writes to, so the cause is on the writing side: the folder \
         may not be writable (antivirus and Controlled Folder Access can \
         block it as well as permissions), something else may be holding \
         the file open, or HLAE may not be passing the flag through to the \
         game."
    )
}

// ── sys/launcher.rs, sys/wmi_launch.rs (#545, #1243) ──────────────────────

pub const LAUNCH_OUTSIDE_JOB: &str =
    "[launch] Started outside Studio's job, so the game outlives Studio (#545)";

pub fn launch_through_wmi(pid: u32) -> String {
    format!("[launch] Started through WMI (PID {pid}), so the game outlives this dev Studio (#545)")
}

pub fn launch_cancelled_in_wmi(program: impl Display, started: Option<u32>) -> String {
    format!(
        "[launch] Cancelled while WMI was starting {program}; {}",
        match started {
            Some(pid) => format!("ended the launcher it had started (PID {pid})"),
            None => "it had started nothing yet".to_string(),
        }
    )
}

pub fn launch_wmi_failed_fell_back(program: impl Display, err: impl Display) -> String {
    format!(
        "[launch] Could not start {program} outside Studio's job ({err}); started it as Studio's own, \
         so closing this dev Studio closes the game too"
    )
}

pub fn launcher_still_running(pid: u32, secs: u64) -> String {
    format!("[launch] The launcher (PID {pid}) was still running {secs} s after it was ended")
}

pub fn wmi_ended_without_answer(err: impl Display, pid: u32) -> String {
    format!(
        "[launch] WMI's call ended without an answer ({err}), but it had started the \
         launcher (PID {pid}): that one is used, not a second"
    )
}

// ── capture_engine/game.rs, launch.rs, markers.rs, obs_capture.rs (#237) ──

pub const GAME_NEVER_SEEN_LOG: &str = "[HLAE] This batch's hl.exe was never seen in the process list, so no game is ended: \
     any hl.exe open was running before the launch and isn't the batch's (#1212)";

pub const MOUSE_RELEASED_LOG: &str =
    "[capture] Released the mouse the closed game had confined to its window.";

pub fn game_left_open(pid: u32, lines: impl Display) -> String {
    format!("[HLAE] Left the game open (PID {pid}): `{lines}`")
}

pub fn game_unreachable_to_stop(pid: u32, outcome: impl std::fmt::Debug) -> String {
    format!(
        "[HLAE] Could not reach the game (PID {pid}) to stop its recording and demo: {outcome:?}"
    )
}

pub const FOREGROUND_LOCKED_LOG: &str = "[HLAE] The game can't take the foreground while it starts";

pub fn hlae_spawning(program: impl std::fmt::Debug, args: impl std::fmt::Debug) -> String {
    format!("[HLAE] Spawning: {program:?} {args:?}")
}

pub fn hlae_reusing_open_game(pid: u32) -> String {
    format!(
        "[HLAE] Running the batch in the game that is already open (PID {pid}), not starting one (#725)"
    )
}

pub fn hlae_spawned(pid: u32) -> String {
    format!("[HLAE] Spawned (PID {pid})")
}

pub fn failed_to_launch_hlae(program: impl Display, err: impl Display) -> String {
    format!("Failed to launch HLAE ({program}): {err}")
}

pub fn events_pipe_connected(pid: u32, hello: impl Display) -> String {
    format!(
        "[HLAE] Game events pipe connected (hl.exe PID {pid}, `{hello}`) — markers now come from the game directly, not qconsole.log"
    )
}

pub fn events_reader_failed(err: impl Display) -> String {
    format!("[HLAE] Could not start the game events reader ({err}); using qconsole.log only")
}

pub fn obs_connected(
    obs_version: impl Display,
    websocket_version: impl Display,
    address: impl Display,
    width: impl Display,
    height: impl Display,
    fps: f64,
    scene: impl Display,
) -> String {
    format!(
        "🎥 **OBS capture** — connected to OBS {obs_version} (obs-websocket {websocket_version}) at {address}. \
         Canvas {width}x{height} @ {fps:.0}fps, scene `{scene}`. HLAE will record nothing; OBS is \
         driven from the engine's own console markers."
    )
}

pub fn obs_blocks_summary(recorded: usize, skipped: usize) -> String {
    format!("🎥 **OBS capture** — {recorded} block(s) recorded, {skipped} skipped.")
}

pub fn obs_problem(problem: impl Display) -> String {
    format!("⚠️ **OBS** — {problem}")
}

pub fn obs_recorded_block(
    take_folder: impl Display,
    duration: impl Display,
    salvaged: bool,
    video: impl Display,
) -> String {
    format!(
        "- [obs] {take_folder} ({duration}{}) -> {video}",
        if salvaged {
            " of recording, salvaged — the file is shorter"
        } else {
            ""
        }
    )
}

pub const OBS_NO_BLOCKS_PLANNED: &str = "OBS capture was requested but no blocks were planned";

pub fn obs_could_not_start(err: impl Display) -> String {
    format!("OBS capture could not start — {err}")
}

// ── capture_engine/workspace.rs, leftovers.rs (#237) ──────────────────────

pub fn cleanup_failed(what: &str, path: impl Display, err: impl Display) -> String {
    format!("[cleanup] Failed to {what} {path}: {err}")
}

pub fn pool_junction_created(junction: impl Display, target: impl Display) -> String {
    format!("[pool] Junction created: {junction} -> {target}")
}

pub fn io_copy_from(os: &str, source: impl Display) -> String {
    format!("- [IO] Copying ({os}) from {source}")
}

pub fn io_copy_to(os: &str, dest: impl Display) -> String {
    format!("- [IO] Copying ({os}) to {dest}")
}

pub fn io_copy_ok(bytes: u64) -> String {
    format!("- [IO] Copy SUCCESS! Bytes written: {bytes}")
}

pub fn io_copy_failed(err: impl Display) -> String {
    format!("- [IO] Copy FAILED! Error: {err}")
}

pub fn io_saved_local_copy(demo_filename: impl Display) -> String {
    format!("- [IO] Saved local copy to demos/{demo_filename}")
}

pub fn io_local_copy_failed(dest: impl Display, err: impl Display) -> String {
    format!("- [IO] Failed to save local patched copy to {dest}: {err}")
}

pub fn failed_to_copy_demo(err: impl Display) -> String {
    format!("Failed to copy demo to game folder: {err}")
}

// ── capture_engine/mod.rs, workspace.rs: what a batch aborts with (#237) ──

/// The `Error` text a batch ends with, around what failed.
pub fn capture_aborted(msg: impl Display) -> String {
    format!("Capture Engine Aborted — {msg} (see View Logs for details)")
}

pub fn capture_thread_failed(err: impl Display) -> String {
    format!("Could not start the capture thread: {err}")
}

pub const HL_PATH_NO_PARENT: &str = "Invalid hl.exe path: it has no parent folder";

pub const LAUNCHING_EVENT_UNSENT: &str = "Failed to send Launching event (channel disconnected)";

pub fn console_log_reader_failed(err: impl Display) -> String {
    format!("could not start the console log reader: {err}")
}

pub fn batch_failed_see_log(reason: impl Display) -> String {
    format!("{reason} — see [HLAE] lines above in this log for timing.")
}

pub fn mklink_unusable_path(junction: impl Display, target: impl Display) -> String {
    format!("Can't link {junction} to {target}: the path has characters mklink can't take")
}

pub fn mklink_failed(junction: impl Display, target: impl Display, stderr: impl Display) -> String {
    format!("Could not link {junction} to {target} (mklink): {stderr}")
}

pub fn mklink_not_run(junction: impl Display, err: impl Display) -> String {
    format!("Could not run mklink to link {junction}: {err}")
}

pub fn capture_dir_create_failed(dir: impl Display, err: impl Display) -> String {
    format!("Failed to create capture directory {dir}: {err}")
}

pub fn capture_dir_unusable_path(junction: impl Display, dir: impl Display) -> String {
    format!(
        "Can't link {junction} to capture directory {dir}: the path has characters mklink can't take"
    )
}

pub fn capture_dir_link_failed(
    junction: impl Display,
    dir: impl Display,
    stderr: impl Display,
) -> String {
    format!("Could not link {junction} to capture directory {dir} (mklink): {stderr}")
}

pub fn invalid_demo_path(path: impl Display) -> String {
    format!("Invalid demo path: {path}")
}

pub fn source_demo_open_failed(err: impl Display) -> String {
    format!("Failed to open source demo for copy: {err}")
}

pub fn demo_copy_target_busy(dest: impl Display, source: impl Display) -> String {
    format!(
        "Could not create {dest} after several tries (copying from {source}). Is the file open in another program?"
    )
}

// ── bin/check_cfgs.rs (#237) ──────────────────────────────────────────────
// Constants that start with "\n" keep the blank line the report puts before
// a section; the binary prints each with `println!("{}", ..)`.

pub const CHECK_CFGS_USAGE: &str = "usage: check_cfgs <dod-folder> [\"init command\"...]";
pub const CFGS_NOTHING_SET: &str = "Nothing the pipeline depends on is set by an executed config.";
pub const CFGS_SET_BY_CONFIGS: &str = "Set by configs the engine runs on its own:";
pub const CFGS_OVERRIDDEN_LATER: &str = "\nAlso set, but overridden later in the chain:";
pub const CFGS_NO_INIT_CONFLICT: &str = "No value those init commands set conflicts with another.";
pub const CFGS_INIT_CONFLICTS: &str = "Values that conflict (the last one is in effect):";
pub const CFGS_WILL_QUIT_GAME: &str = "\nWill quit the game the moment the HUD renders:";
pub const CFGS_UNREFERENCED: &str =
    "\nConfigs present that nothing execs (they set nothing today):";

pub fn cfgs_folder_and_count(dir: impl Display, files_read: impl Display) -> String {
    format!("{}\n{} config file(s) executed\n", dir, files_read)
}

/// One `cvar value file:line` row of the effective and overridden lists.
pub fn cfgs_setting_row(
    cvar: impl Display,
    value: impl Display,
    file: impl Display,
    line: impl Display,
) -> String {
    format!("  {:<12} {:<8} {}:{}", cvar, value, file, line)
}

pub fn cfgs_assignments_seen(count: usize) -> String {
    format!("\n{} assignment(s) seen across those configs.", count)
}

pub fn cfgs_value_from_config(
    value: impl Display,
    file: impl Display,
    line: impl Display,
) -> String {
    format!("{} ({}:{})", value, file, line)
}

pub fn cfgs_value_from_init(value: impl Display) -> String {
    format!("{} (init)", value)
}

pub fn cfgs_conflict_row(cvar: impl Display, values: impl Display) -> String {
    format!("  {:<20} {}", cvar, values)
}

pub fn cfgs_fatal_row(
    cvar: impl Display,
    value: impl Display,
    required: impl Display,
    file: impl Display,
    line: impl Display,
) -> String {
    format!(
        "  {:<12} {:<8} DoD requires {} ({}:{})",
        cvar, value, required, file, line
    )
}

pub fn cfgs_unreferenced_row(file: impl Display) -> String {
    format!("  {}", file)
}

// ── bin/check_maps.rs (#237) ──────────────────────────────────────────────

pub const CHECK_MAPS_USAGE: &str = "usage: check_maps <demo-or-folder> [maps-dir] [--fetch]";
pub const MAPS_READ_FOLDER_FAILED: &str = "read folder";
pub const MAPS_TO_FETCH: &str = "\nmaps to fetch:";
pub const MAPS_PASS_FETCH: &str = "\npass --fetch to download these.";

pub fn maps_dir_line(dir: impl Display) -> String {
    format!("maps: {}", dir)
}

pub fn maps_demo_count(count: usize) -> String {
    format!("demos: {}\n", count)
}

pub fn maps_demo_status(name: impl Display, summary: impl Display) -> String {
    format!("{:<52} {}", name, summary)
}

pub fn maps_unreadable_demo(name: impl Display, err: impl Display) -> String {
    format!("{:<52} unreadable demo: {}", name, err)
}

pub fn maps_totals(
    ok: usize,
    wrong: usize,
    missing: usize,
    unverifiable: usize,
    unreadable_map: usize,
    unreadable_demo: usize,
) -> String {
    format!(
        "\nok {}  wrong-build {}  missing {}  unverifiable {}  unreadable-map {}  unreadable-demo {}",
        ok, wrong, missing, unverifiable, unreadable_map, unreadable_demo
    )
}

pub fn maps_to_fetch_row(map: impl Display, build: u32, demos: usize, url: impl Display) -> String {
    format!(
        "  {:<24} build {:08x}, {} demo(s)  {}",
        map, build, demos, url
    )
}

pub fn maps_to_fetch_row_unstated(map: impl Display, demos: usize, url: impl Display) -> String {
    format!("  {:<24} build unstated, {} demo(s)  {}", map, demos, url)
}

pub fn maps_already_correct(map: impl Display) -> String {
    format!("  {:<24} already correct", map)
}

/// `previous_kept` is the ", previous kept at <path>" tail, or empty.
pub fn maps_installed(
    map: impl Display,
    build: u32,
    megabytes: f64,
    previous_kept: &str,
) -> String {
    format!(
        "  {:<24} installed {:08x} ({:.1} MB){}",
        map, build, megabytes, previous_kept
    )
}

pub fn maps_previous_kept_at(path: impl Display) -> String {
    format!(", previous kept at {}", path)
}

pub fn maps_fetch_failed(map: impl Display, err: impl Display) -> String {
    format!("  {:<24} FAILED: {}", map, err)
}

// ── bin/preview_cli/main.rs (#237) ────────────────────────────────────────

pub const PREVIEW_DROP_PROMPT: &str =
    "Drag and drop demo files or directories into this window and press Enter.";
pub const PREVIEW_BLANK_TO_EXIT: &str = "(To exit without processing, leave blank and press Enter)";
pub const PREVIEW_INPUT_PROMPT: &str = "> ";
pub const PREVIEW_PRESS_ENTER_TO_EXIT: &str = "\nPress Enter to exit...";
pub const PREVIEW_UNNAMED_PLAYER: &str = "<unnamed>";
pub const PREVIEW_ALL_PLAYERS_HINT: &str =
    "  HLTV demo: bookmarking all players. Re-run with --player to narrow it to one.";
pub const PREVIEW_ROSTER_HEADER: &str =
    "  players with highlights (pass one to --player, by name or index):";

pub fn preview_create_dir_failed(dir: impl Display, err: impl Display) -> String {
    format!("Failed to create output directory {}: {}", dir, err)
}

pub fn preview_output_dir(dir: impl Display) -> String {
    format!("Output directory: {}", dir)
}

pub fn preview_read_dir_failed(dir: impl Display, err: impl Display) -> String {
    format!("Failed to read directory {}: {}", dir, err)
}

pub fn preview_skipped_not_dem(path: impl Display) -> String {
    format!("Skipped: {} — not a .dem file", path)
}

pub fn preview_skipped_not_found(path: impl Display) -> String {
    format!("Skipped: {} — not found or not accessible", path)
}

pub fn preview_batch_complete(processed: usize, skipped: usize) -> String {
    format!(
        "\nBatch Complete\n  Processed: {}  |  Skipped: {}",
        processed, skipped
    )
}

pub fn preview_roster_row(index: usize, kills: u32, streaks: usize, name: impl Display) -> String {
    format!("    {index:>3}  {kills:>3} kills in {streaks:>2} streaks   {name}")
}

pub fn preview_processing(name: impl Display) -> String {
    format!("Processing: {}", name)
}

pub fn preview_skipped_unreadable(name: impl Display, err: impl Display) -> String {
    format!("  Skipped: {} — could not read it as a demo: {}", name, err)
}

pub fn preview_player_ignored_for_pov(name: impl Display) -> String {
    format!(
        "  Note: {} is a POV demo — --player ignored (its camera is already one player's)",
        name
    )
}

pub fn preview_skipped_no_highlights(name: impl Display) -> String {
    format!("  Skipped: {} — no highlights found", name)
}

pub fn preview_skipped_no_player_match(name: impl Display, want: impl Display) -> String {
    format!(
        "  Skipped: {} — no highlights for a player matching \"{}\"",
        name, want
    )
}

pub fn preview_bookmarking_only(name: impl Display, index: impl Display) -> String {
    format!(
        "  Bookmarking only {} (index {}) — the camera stays with the auto-director",
        name, index
    )
}

pub fn preview_saved(name: impl Display) -> String {
    format!("  Saved: {}", name)
}

pub fn preview_write_failed(name: impl Display, err: impl Display) -> String {
    format!("  Failed to write {}: {}", name, err)
}

// ── bin/cli.rs (dod-studio-cli, #237) ─────────────────────────────────────
// The Markdown report's headings and table columns are the report's layout,
// not messages, and stay in cli.rs; so does clap's `--help` text, which is the
// doc comments on its argument structs.

pub const CLI_NO_DEMOS: &str =
    "Error: Please specify at least one demo file to analyze, or use a subcommand.";
pub const CLI_READING_RAW_BYTES: &str = "Reading raw demo bytes...";
pub const CLI_PATCHING_HIGHLIGHTS: &str = "Patching demo highlights...";
pub const CLI_EXPORT_DONE: &str = "Successfully exported patched demo!";

pub fn cli_error(err: impl Display) -> String {
    format!("Error: {}", err)
}

pub fn cli_error_analyzing(path: impl Display, err: impl Display) -> String {
    format!("Error analyzing {}: {}", path, err)
}

pub fn cli_analyzing_input(path: impl Display) -> String {
    format!("Analyzing input demo: {}", path)
}

pub fn cli_no_player_match(query: impl Display, available: impl Display) -> String {
    format!(
        "Could not find player matching query '{}'. Available players: {}",
        query, available
    )
}

/// `patch-streak` found nothing to patch: says which player, how long a streak
/// was asked for and the longest one the player had, so a short demo reads as
/// "no streak" and not as a failure (#1237).
pub fn cli_no_killstreaks(
    player: impl Display,
    min_streak: usize,
    weapon: Option<&str>,
    longest: usize,
) -> String {
    let weapon = weapon
        .map(|w| format!(" with a weapon matching '{w}'"))
        .unwrap_or_default();
    format!(
        "{player} has no killstreak of {min_streak} or more kills{weapon} in this demo \
         (longest streak: {longest}), so there is nothing to patch. \
         Lower --min-streak to patch a shorter one."
    )
}

pub fn cli_found_player(name: impl Display, id: impl Display) -> String {
    format!("Found player: {} ({})", name, id)
}

pub fn cli_selected_streak(kills: usize, start: f32, stop: f32) -> String {
    format!(
        "Selected Streak: {} kills starting at {:.2}s, ending at {:.2}s",
        kills, start, stop
    )
}

pub fn cli_failed_to_read(path: impl Display, err: impl Display) -> String {
    format!("Failed to read {}: {}", path, err)
}

pub fn cli_writing_patched(path: impl Display) -> String {
    format!("Writing patched demo to: {}", path)
}

pub fn cli_failed_to_write(path: impl Display, err: impl Display) -> String {
    format!("Failed to write {}: {}", path, err)
}

// ── vegas.rs ─────────────────────────────────────────────────────────────

pub const VEGAS_NO_SCRIPT: &str = "This copy of DoD Studio has no VEGAS script to install (the vegas folder in the DoD Studio repo).";

pub const VEGAS_NO_DOCUMENTS: &str =
    "Windows did not say where your Documents folder is, so the VEGAS script cannot be placed.";

/// Pins every function above against the exact `format!`/literal it replaced
/// at its original call site (text copied verbatim from the pre-PR source,
/// not re-derived), so this refactor can't have silently changed any
/// user-visible wording.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_killstreaks_names_the_player_the_ask_and_the_longest_streak() {
        assert_eq!(
            cli_no_killstreaks("Pvt. Clopton", 3, Some("k98"), 2),
            "Pvt. Clopton has no killstreak of 3 or more kills with a weapon matching 'k98' \
             in this demo (longest streak: 2), so there is nothing to patch. \
             Lower --min-streak to patch a shorter one."
        );
    }

    #[test]
    fn map_fetch_messages_match_their_original_inline_text() {
        assert_eq!(
            not_a_usable_map_name("bad name"),
            format!("`{}` is not a usable map name", "bad name")
        );
        assert_eq!(MAP_MIRRORS_MUST_BE_HTTPS, "map mirrors must be https");
        assert_eq!(
            labeled("some/path", "boom"),
            format!("{}: {}", "some/path", "boom")
        );
        assert_eq!(
            served_unreadable_bsp("http://x", "bad bsp"),
            format!(
                "what {} served is not a readable BSP: {}",
                "http://x", "bad bsp"
            )
        );
        assert_eq!(
            served_unparseable_map("http://x", "bad map"),
            format!(
                "what {} served does not parse as a map: {}",
                "http://x", "bad map"
            )
        );
        assert_eq!(
            served_wrong_build("http://x", 0x1234, 0x5678),
            format!(
                "{} served build {:08x}, but the demo needs {:08x} — not installing it",
                "http://x", 0x1234u32, 0x5678u32
            )
        );
        assert_eq!(
            could_not_move_existing_aside("old.bsp", "in use"),
            format!(
                "could not move the existing {} aside: {}",
                "old.bsp", "in use"
            )
        );
        assert_eq!(
            url_returned_status("http://x", 404),
            format!("{} returned {}", "http://x", 404)
        );
    }

    #[test]
    fn hlae_ffmpeg_messages_match_their_original_inline_text() {
        assert_eq!(
            not_a_file("ffmpeg.exe"),
            format!("{} is not a file", "ffmpeg.exe")
        );
        assert_eq!(
            could_not_run("ffmpeg.exe", "access denied"),
            format!("could not run {}: {}", "ffmpeg.exe", "access denied")
        );
        assert_eq!(
            did_not_respond_to_version_flag("ffmpeg.exe"),
            format!("{} did not respond to -version", "ffmpeg.exe")
        );
        assert_eq!(
            did_not_identify_as_ffmpeg("ffmpeg.exe"),
            format!("{} did not identify itself as FFmpeg", "ffmpeg.exe")
        );
        assert_eq!(
            reports_itself_as("ffmpeg.exe", "handbrake version 1.0"),
            format!(
                "{} is not FFmpeg — it reports itself as \"{}\"",
                "ffmpeg.exe", "handbrake version 1.0"
            )
        );
    }

    #[test]
    fn bin_messages_match_their_original_inline_text() {
        assert_eq!(
            cfgs_setting_row("r_decals", 9, "movie.cfg", 1),
            "  r_decals     9        movie.cfg:1"
        );
        assert_eq!(
            cfgs_fatal_row("cl_x", 1, 0, "a.cfg", 3),
            "  cl_x         1        DoD requires 0 (a.cfg:3)"
        );
        assert_eq!(cfgs_value_from_init(3), "3 (init)");
        assert_eq!(
            maps_to_fetch_row("dod_anzio", 0x801b771f, 1, "u"),
            "  dod_anzio                build 801b771f, 1 demo(s)  u"
        );
        assert_eq!(
            maps_installed("m", 0xab, 1.25, ""),
            "  m                        installed 000000ab (1.2 MB)"
        );
        assert_eq!(
            preview_roster_row(2, 4, 3, "x"),
            "      2    4 kills in  3 streaks   x"
        );
        assert_eq!(
            preview_batch_complete(1, 2),
            "\nBatch Complete\n  Processed: 1  |  Skipped: 2"
        );
        assert_eq!(
            cli_selected_streak(3, 1.0, 2.5),
            "Selected Streak: 3 kills starting at 1.00s, ending at 2.50s"
        );
        assert_eq!(cli_error("boom"), "Error: boom");
    }

    #[test]
    fn obs_session_messages_match_their_original_inline_text() {
        assert_eq!(NOT_CONNECTED_TO_OBS, "not connected to OBS");
        assert_eq!(
            obs_reported_file_missing("clip.mp4"),
            format!("OBS reported {} but no file is there", "clip.mp4")
        );
        let recorded = "clip.mp4";
        let target = "video.mp4";
        let retry_secs = 5.0_f64;
        let last = "sharing violation";
        assert_eq!(
            could_not_move_after_retries(recorded, target, retry_secs, last),
            format!("could not move {recorded} to {target} after 5.0 s of retries: {last}")
        );
    }
}
