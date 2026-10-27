// messages.rs
// native's own error-string catalog — the same idea as
// studio/src-tauri/src/messages.rs (itself the counterpart to the
// frontend's strings.js), but crate-local: native has no direct UI of its
// own, but many of its `Result<T, String>` errors bubble straight through a
// Tauri command (studio's messages.rs wraps them with extra context,
// e.g. `failed_to_patch_preview_demo`, but the inner text — what actually
// went wrong — is authored here). Scoped to the modules studio's
// Tauri layer actually calls (patch::map_fetch, shared::hlae_ffmpeg,
// obs::session) — native's CLI probe binaries (native/src/bin/*) are never
// reachable from the app and are out of scope. Issue #33.

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

// ── demo_rename.rs (#469) ─────────────────────────────────────────────────

pub const RENAME_NOT_IN_PLACE: &str =
    "a demo is only renamed within its own folder, never moved to another";
pub const RENAME_SOURCE_GONE: &str = "the demo is no longer there";
pub const RENAME_TARGET_EXISTS: &str = "a file with the new name is already there";

pub fn rename_bad_name(name: &str) -> String {
    format!("`{name}` isn't a usable demo name")
}

// ── demo_consolidate.rs (#872) ────────────────────────────────────────────

pub const CONSOLIDATE_NO_DESTINATION: &str = "choose the folder to move the demos into first";
pub const CONSOLIDATE_SOURCE_GONE: &str = "the demo is no longer there";
pub const CONSOLIDATE_TARGET_EXISTS: &str = "a file with that name is already there";

pub fn consolidate_destination_bad(dest: &std::path::Path) -> String {
    format!(
        "{} can't hold the demos: it isn't a folder, or a folder can't be made there",
        dest.display()
    )
}

pub fn consolidate_not_enough_space(needed: u64, free: u64, dest: &std::path::Path) -> String {
    let gb = |b: u64| b as f64 / (1024.0 * 1024.0 * 1024.0);
    format!(
        "not enough free space on the drive of {}: the demos copied from other drives need {:.1} GB and {:.1} GB is free",
        dest.display(),
        gb(needed),
        gb(free)
    )
}

pub fn consolidate_copy_mismatch(to: &std::path::Path) -> String {
    format!(
        "the copy at {} doesn't match the original's size, so the original was kept",
        to.display()
    )
}

/// A failure names the demo, since the toast shows only the first one.
pub fn consolidate_not_moved(from: &std::path::Path, err: &str) -> String {
    format!("Not moved: {}: {}", shown(from), err)
}

pub fn consolidate_not_recycled(path: &std::path::Path, err: &str) -> String {
    format!("Not recycled: {}: {}", shown(path), err)
}

pub fn consolidate_not_put_back(at: &std::path::Path, err: &str) -> String {
    format!("Not put back: {}: {}", shown(at), err)
}

// ── demo_compare.rs (#871, #872) ─────────────────────────────────────────

/// A path as Explorer shows it: the audit's canonical paths carry a `\\?\`
/// (or `\\?\UNC\`) prefix, which the Duplicates table hides too.
pub fn shown(path: &std::path::Path) -> String {
    let text = path.display().to_string();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        text
    }
}

/// A copy kept because it isn't byte for byte the copy its group keeps:
/// the audit's key reads only the first 64 KiB.
pub fn copy_differs_from_kept(
    copy: &std::path::Path,
    keep: &std::path::Path,
    offset: u64,
) -> String {
    format!(
        "Not deleted: {} is not an exact copy of {} (they differ from byte {}), so it may be a different demo",
        shown(copy),
        shown(keep),
        offset
    )
}

pub fn kept_copy_missing(copy: &std::path::Path, keep: &std::path::Path) -> String {
    format!(
        "Not deleted: {} could not be checked against the copy being kept, {}, which is no longer there",
        shown(copy),
        shown(keep)
    )
}

pub fn copy_is_the_kept_copy(copy: &std::path::Path) -> String {
    format!("Not deleted: {} is the copy being kept", shown(copy))
}

pub fn copy_compare_failed(
    copy: &std::path::Path,
    keep: &std::path::Path,
    err: impl Display,
) -> String {
    format!(
        "Not deleted: {} could not be compared with {}: {}",
        shown(copy),
        shown(keep),
        err
    )
}

/// Pins every function above against the exact `format!`/literal it replaced
/// at its original call site (text copied verbatim from the pre-PR source,
/// not re-derived), so this refactor can't have silently changed any
/// user-visible wording.
#[cfg(test)]
mod tests {
    use super::*;

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
