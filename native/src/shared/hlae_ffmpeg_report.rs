// native/src/shared/hlae_ffmpeg_report.rs
//
// What the Settings "HLAE's FFmpeg" row shows: whether HLAE can reach an
// FFmpeg of its own, whether it is the one Render Studio uses, and whether
// Studio's own FFmpeg is usable. The decisions live here, beside the
// `hlae_ffmpeg` probes they combine, so `cargo test` reaches them; the Tauri
// command only forwards the two settings paths (#1174).

use super::hlae_ffmpeg as hf;
use serde::Serialize;
use std::path::Path;

/// The default the frontend sends when no FFmpeg override is set.
const DEFAULT_FFMPEG: &str = "ffmpeg";

/// The reply of the `check_hlae_ffmpeg` command, field for field.
#[derive(Debug, Serialize)]
pub struct HlaeFfmpegReport {
    pub state: hf::HlaeFfmpeg,
    pub usable: bool,
    pub can_link: bool,
    /// The FFmpeg Render Studio would run, when its setting resolves to a file.
    pub app_ffmpeg: Option<String>,
    /// Whether HLAE's linked FFmpeg is the app's; `None` when there is nothing
    /// to compare (see `agrees_with_app`).
    pub agrees_with_app: Option<bool>,
    /// Why Studio's own FFmpeg is not usable, when it is not.
    pub app_ffmpeg_problem: Option<String>,
    /// The hook DLL HLAE would load but cannot find. Advisory.
    pub missing_hook_dll: Option<String>,
}

/// Probes the HLAE install and the app's FFmpeg setting and combines the
/// answers. Reads the disk and may start `ffmpeg -version` (up to 5 s), so call
/// it off the main thread.
pub fn report(hlae_exe: &Path, ffmpeg_setting: Option<&str>) -> HlaeFfmpegReport {
    let state = hf::detect(hlae_exe);
    let app_ffmpeg = hf::resolve_absolute(ffmpeg_setting.unwrap_or(DEFAULT_FFMPEG));
    let agrees_with_app = agrees_with_app(&state, app_ffmpeg.as_deref());
    let app_ffmpeg_problem =
        app_ffmpeg_problem(app_ffmpeg.as_deref(), ffmpeg_setting.unwrap_or("").trim());

    // Answered by the file the pipeline actually consumes rather than by what
    // the exe calls itself: `build_hlae_process` passes this DLL as
    // `-hookDllPath`, so its absence means a capture cannot work whatever the
    // exe is named.
    let missing_hook_dll = hf::missing_hook_dll(hlae_exe).map(|p| p.to_string_lossy().into_owned());

    HlaeFfmpegReport {
        usable: state.is_usable(),
        can_link: state.can_link(),
        state,
        app_ffmpeg: app_ffmpeg.map(|p| p.to_string_lossy().into_owned()),
        agrees_with_app,
        app_ffmpeg_problem,
        missing_hook_dll,
    }
}

/// Whether the FFmpeg HLAE is linked to is the one the app uses.
///
/// "HLAE is pointed somewhere" and "HLAE is pointed at the same FFmpeg Render
/// Studio uses" are different questions, and only the second keeps both halves
/// of the pipeline encoding with the same build, which was the stated reason
/// for writing an ini instead of copying the binary. Nothing checked it stayed
/// true, so changing the app's FFmpeg silently left HLAE on the old one.
///
/// `None` when there is nothing to compare: a bundled binary is HLAE's own and
/// is meant to differ, and with nothing linked there is nothing to disagree
/// with.
pub fn agrees_with_app(state: &hf::HlaeFfmpeg, app_ffmpeg: Option<&Path>) -> Option<bool> {
    match (state, app_ffmpeg) {
        (hf::HlaeFfmpeg::Linked { target, .. }, Some(app)) => Some(hf::same_file(target, app)),
        _ => None,
    }
}

/// Why the app's FFmpeg setting is not a working FFmpeg, or `None` when it is.
///
/// `app_ffmpeg` is what the setting resolved to and `configured` the trimmed
/// setting itself. "It exists" was the only test the picker applied, and
/// ffplay.exe and ffprobe.exe sit in the same folder as ffmpeg.exe; checking
/// here as well as at link time means the row says so before the button is
/// pressed. A setting that does not resolve is reported too: chaining the check
/// off the resolved path left a typo'd override saying nothing at all.
///
/// Starts the file when it is named like FFmpeg (`verify_is_ffmpeg`).
pub fn app_ffmpeg_problem(app_ffmpeg: Option<&Path>, configured: &str) -> Option<String> {
    match app_ffmpeg {
        Some(p) => hf::verify_is_ffmpeg(p).err(),
        None if no_override(configured) => Some(crate::messages::NO_FFMPEG_ON_PATH.to_string()),
        None => Some(crate::messages::no_ffmpeg_file_at(configured)),
    }
}

/// The frontend sends the bare default `ffmpeg` when no override is set, so
/// that is "no override" too, not a file called ffmpeg that went missing.
fn no_override(configured: &str) -> bool {
    configured.is_empty() || configured.eq_ignore_ascii_case(DEFAULT_FFMPEG)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;
    use std::path::PathBuf;

    fn linked_to(target: &str) -> hf::HlaeFfmpeg {
        hf::HlaeFfmpeg::Linked {
            ini: PathBuf::from("ffmpeg.ini"),
            target: PathBuf::from(target),
            target_exists: true,
            ours: true,
        }
    }

    #[test]
    fn a_link_to_the_apps_ffmpeg_agrees_whatever_the_case() {
        let state = linked_to("C:/tools/FFmpeg.exe");
        assert_eq!(
            agrees_with_app(&state, Some(Path::new("c:/tools/ffmpeg.exe"))),
            Some(true)
        );
    }

    #[test]
    fn a_link_to_another_ffmpeg_disagrees() {
        let state = linked_to("C:/old/ffmpeg.exe");
        assert_eq!(
            agrees_with_app(&state, Some(Path::new("C:/new/ffmpeg.exe"))),
            Some(false)
        );
    }

    #[test]
    fn a_bundled_ffmpeg_is_not_compared_with_the_app() {
        let state = hf::HlaeFfmpeg::Bundled {
            path: PathBuf::from("C:/hlae/ffmpeg/bin/ffmpeg.exe"),
        };
        assert_eq!(
            agrees_with_app(&state, Some(Path::new("C:/new/ffmpeg.exe"))),
            None
        );
    }

    #[test]
    fn nothing_is_compared_when_hlae_has_nothing_linked_or_the_app_has_no_ffmpeg() {
        let missing = hf::HlaeFfmpeg::Missing {
            folder: PathBuf::from("C:/hlae/ffmpeg"),
        };
        assert_eq!(
            agrees_with_app(&missing, Some(Path::new("C:/new/ffmpeg.exe"))),
            None
        );
        assert_eq!(agrees_with_app(&hf::HlaeFfmpeg::NoInstall, None), None);
        assert_eq!(agrees_with_app(&linked_to("C:/old/ffmpeg.exe"), None), None);
    }

    #[test]
    fn an_unset_or_default_setting_that_finds_nothing_blames_the_path() {
        for configured in ["", "ffmpeg", "FFmpeg"] {
            assert_eq!(
                app_ffmpeg_problem(None, configured).as_deref(),
                Some(crate::messages::NO_FFMPEG_ON_PATH),
                "{configured:?}"
            );
        }
    }

    #[test]
    fn a_typod_override_that_resolves_to_nothing_is_still_reported() {
        let problem = app_ffmpeg_problem(None, "D:/tools/ffmpgg.exe").expect("a problem");
        assert_eq!(problem, "there is no file at \"D:/tools/ffmpgg.exe\"");
    }

    #[test]
    fn a_file_that_is_not_named_like_ffmpeg_is_refused_without_starting_it() {
        let dir = Scratch::new("hlae_ffmpeg_report_ffplay");
        let ffplay = dir.join("ffplay.exe");
        std::fs::write(&ffplay, b"").expect("ffplay");
        let problem = app_ffmpeg_problem(Some(&ffplay), "ffplay.exe").expect("a problem");
        assert!(
            problem.starts_with("ffplay.exe isn't ffmpeg.exe"),
            "{problem}"
        );
    }

    #[test]
    fn a_resolved_path_that_is_not_a_file_is_reported() {
        let dir = Scratch::new("hlae_ffmpeg_report_gone");
        let gone = dir.join("ffmpeg.exe");
        let problem = app_ffmpeg_problem(Some(&gone), "ffmpeg.exe").expect("a problem");
        assert!(problem.ends_with("is not a file"), "{problem}");
    }
}
