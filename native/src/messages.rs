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
