//! The checks on the executables Path Routing points at (#1174): the HLAE
//! and `hl.exe` paths, and the FFmpeg HLAE spawns for itself.

use crate::messages;

#[tauri::command]
pub async fn validate_paths(hlae_path: String, hl_path: String) -> Result<bool, String> {
    let hlae_p = std::path::Path::new(&hlae_path);
    let hl_p = std::path::Path::new(&hl_path);

    if !hlae_p.exists() || !hlae_p.is_file() {
        return Err(messages::HLAE_EXECUTABLE_NOT_FOUND.into());
    }
    if !hl_p.exists() || !hl_p.is_file() {
        return Err(messages::HL_EXECUTABLE_NOT_FOUND.into());
    }
    Ok(true)
}

/// Whether HLAE can reach an FFmpeg of its own, which is a different question
/// from whether Render Studio can.
///
/// `mirv_movie_ffmpeg` makes HLAE spawn FFmpeg itself and it does not consult
/// the app's resolution chain, so this has to be reported before a batch rather
/// than discovered after one — the failure is a capture that runs to completion
/// and produces no video. See `native::shared::hlae_ffmpeg`.
///
/// On the blocking pool (#853): it searches PATH and runs `ffmpeg -version`,
/// polling for up to 5 s, and start-up calls it twice.
#[tauri::command]
pub async fn check_hlae_ffmpeg(
    hlae_path: String,
    ffmpeg_path: Option<String>,
) -> Result<native::shared::hlae_ffmpeg_report::HlaeFfmpegReport, String> {
    messages::spawn_blocking_result(tokio::task::spawn_blocking(move || {
        native::shared::hlae_ffmpeg_report::report(
            std::path::Path::new(&hlae_path),
            ffmpeg_path.as_deref(),
        )
    }))
    .await
}

/// Whether each configured executable path actually points at a file.
///
/// The Path Routing fields accepted anything: `validate_paths` only ran at
/// capture launch, so a typo sat there looking fine until a batch failed
/// minutes later. Returns a state per path rather than a message, so the
/// wording stays in `strings.js` with every other user-facing string.
#[tauri::command]
pub async fn diagnose_executable_paths(paths: Vec<String>) -> Result<Vec<String>, String> {
    Ok(paths
        .iter()
        .map(|raw| {
            let path = raw.trim();
            if path.is_empty() {
                // Not a complaint: these fields are legitimately blank before
                // they are filled in, and the FFmpeg override is optional.
                return "empty";
            }
            let p = std::path::Path::new(path);
            if p.is_file() {
                "ok"
            } else if p.is_dir() {
                // The classic mistake these fields invite — the folder rather
                // than the executable inside it.
                "not_a_file"
            } else {
                "not_found"
            }
        })
        .map(str::to_string)
        .collect())
}

/// Points HLAE at an FFmpeg by writing `ffmpeg.ini`, on request only.
///
/// Never overwrites an existing ini: HLAE installs are shared with Source work
/// and other projects, so silently repointing one would break somebody else's
/// setup to fix ours. `link` refuses in that case and the error says so.
/// `elevated` retries the same write through a UAC prompt. The HLAE installer
/// puts the target under `Program Files`, so an unelevated write fails for most
/// people — the frontend asks first, then calls back with this set.
#[tauri::command]
pub async fn link_hlae_ffmpeg(
    hlae_path: String,
    ffmpeg_path: String,
    elevated: Option<bool>,
) -> Result<serde_json::Value, String> {
    use native::shared::hlae_ffmpeg::{self, LinkError};

    let ffmpeg = hlae_ffmpeg::resolve_absolute(&ffmpeg_path)
        .ok_or_else(|| messages::ffmpeg_could_not_be_resolved(&ffmpeg_path))?;

    let hlae = std::path::Path::new(&hlae_path);
    let result = if elevated.unwrap_or(false) {
        hlae_ffmpeg::link_elevated(hlae, &ffmpeg)
    } else {
        hlae_ffmpeg::link(hlae, &ffmpeg)
    };

    match result {
        Ok(ini) => {
            native::log_markdown(&format!(
                "[hlae-ffmpeg] wrote {} pointing at {} — HLAE can now spawn FFmpeg for \
                 `mirv_movie_ffmpeg`.",
                ini.display(),
                ffmpeg.display()
            ));
            Ok(serde_json::json!({ "ini": ini.to_string_lossy() }))
        }
        // Reported as data, not an error string: the frontend has to tell this
        // apart from a real failure so it can offer the prompt rather than show
        // somebody "Access is denied. (os error 5)" and leave them there.
        Err(LinkError::NeedsElevation { ini }) => Ok(serde_json::json!({
            "needs_elevation": true,
            "ini": ini.to_string_lossy(),
        })),
        Err(e) => Err(e.to_string()),
    }
}
