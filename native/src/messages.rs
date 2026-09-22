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

pub fn reports_itself_as(path: impl Display, banner: &str) -> String {
    format!("{} is not FFmpeg — it reports itself as \"{}\"", path, banner)
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
        "could not move {} to {} after {:.1}s of retries: {}",
        recorded, target, retry_secs, last_err
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
            format!("what {} served is not a readable BSP: {}", "http://x", "bad bsp")
        );
        assert_eq!(
            served_unparseable_map("http://x", "bad map"),
            format!("what {} served does not parse as a map: {}", "http://x", "bad map")
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
            format!("could not move the existing {} aside: {}", "old.bsp", "in use")
        );
        assert_eq!(
            url_returned_status("http://x", 404),
            format!("{} returned {}", "http://x", 404)
        );
    }

    #[test]
    fn hlae_ffmpeg_messages_match_their_original_inline_text() {
        assert_eq!(not_a_file("ffmpeg.exe"), format!("{} is not a file", "ffmpeg.exe"));
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
            format!(
                "could not move {} to {} after {:.1}s of retries: {last}",
                recorded, target, retry_secs
            )
        );
    }
}
