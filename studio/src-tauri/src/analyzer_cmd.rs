//! The Demo Analyzer tab's commands (#1174): the full analysis of one demo,
//! the weapon names its tables show, and the overview image under its kill
//! map.

use crate::messages;

// ── Full-fidelity analysis payload for the standalone Demo Analyzer tab ─────────
// Passes the typed `analysis::DemoInfo`/`AnalyzerState` straight through so the frontend can
// reconstruct every report sub-view (Summary/Scoreboard/Player Details/Team
// Details/Timeline/Rounds/Chat) with full fidelity, matching the data the
// egui report views on `dev` were built from.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct AnalyzerReportPayload {
    pub file_name: String,
    pub file_path: String,
    pub file_dir: String,
    pub file_size_mb: f64,
    pub file_created_unix_secs: f64,
    pub demo_info: analysis::DemoInfo,
    pub state: analysis::AnalyzerState,
}

/// One `analyzer_progress` event. The path lets the page ignore an earlier
/// click's analysis still running beside this one; `cached` says the demo
/// is read from the analyzer cache, with no frames to count.
fn analyzer_progress_payload(
    path: &str,
    processed: usize,
    total: usize,
    cached: bool,
) -> serde_json::Value {
    serde_json::json!({ "processed": processed, "total": total, "path": path, "cached": cached })
}

#[tauri::command]
pub async fn analyze_demo_full(
    app_handle: tauri::AppHandle,
    demo_path: String,
) -> Result<AnalyzerReportPayload, String> {
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        use tauri::Emitter;

        let path = std::path::PathBuf::from(&demo_path);

        if !path.exists() || !path.is_file() {
            return Err(messages::demo_file_not_found(&demo_path));
        }

        // A demo the analyzer cache has is read, not parsed: the page says so
        // instead of a % that never comes (#958).
        if analysis::cache::is_fresh(&native::analyzer_cache_root(), &path) {
            let _ = app_handle.emit(
                "analyzer_progress",
                analyzer_progress_payload(&demo_path, 0, 0, true),
            );
        }

        // Throttled to ~30fps per CLAUDE.md's telemetry-throttling guardrail —
        // `try_from_bytes_with_progress` calls back every ~500 frames, which
        // can be 1000+ times for a large demo.
        let mut last_emit = std::time::Instant::now() - std::time::Duration::from_secs(1);
        let progress_cb = |processed: usize, total: usize| {
            let now = std::time::Instant::now();
            if now.duration_since(last_emit) >= std::time::Duration::from_millis(33)
                || processed == total
            {
                last_emit = now;
                let _ = app_handle.emit(
                    "analyzer_progress",
                    analyzer_progress_payload(&demo_path, processed, total, false),
                );
            }
        };

        match native::run_analyzer_cached(&path, progress_cb) {
            Ok((file_info, analysis, from_cache)) => {
                native::log_markdown(&format!(
                    "[analyzer] opened {} ({})",
                    path.display(),
                    if from_cache {
                        "from the cache"
                    } else {
                        "parsed"
                    }
                ));
                // Independent metadata lookup (not `file_info.created_at`) to avoid
                // depending on `web_time::SystemTime`'s exact type identity here.
                let created_unix_secs = std::fs::metadata(&path)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs_f64())
                    .unwrap_or(0.0);

                let file_dir = std::path::Path::new(&file_info.path)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();

                Ok(AnalyzerReportPayload {
                    file_name: file_info.name,
                    file_path: file_info.path,
                    file_dir,
                    file_size_mb: file_info.size_bytes as f64 / 1_048_576.0,
                    file_created_unix_secs: created_unix_secs,
                    demo_info: analysis.demo_info,
                    state: analysis.state,
                })
            }
            Err(e) => {
                native::log_markdown(&format!(
                    "[analyzer] could not read {}: {e}",
                    path.display()
                ));
                Err(messages::analyzer_error(e))
            }
        }
    }))
    .await
}

/// Every `Weapon` variant's resolved display name, keyed by its raw JSON tag
/// (e.g. `"ScopedK98"` -> "Scoped Kar98k") — the same names
/// `native::patch::scanner` bakes into a kill streak's timeline text, so the
/// frontend's weapon tables (analyzer_pane.js) can show identical text
/// instead of independently re-deriving a name from the raw enum tag.
///
/// On the blocking pool (#853): the first call at every boot loads the
/// localization files from disk.
#[tauri::command]
pub async fn get_weapon_display_names() -> Result<std::collections::HashMap<String, String>, String>
{
    messages::spawn_blocking_result(tokio::task::spawn_blocking(
        analysis::all_weapon_display_names,
    ))
    .await
}

/// The overview image for `map_name` and how it lies over the world, for the
/// Demo Analyzer's kill map (#448). `None` when no `overviews/` folder near
/// the game or the demo has one the app can show -- not an error, since
/// custom maps often ship without.
#[tauri::command]
pub async fn load_map_overview(
    game_path: Option<String>,
    demo_path: String,
    map_name: String,
) -> Result<Option<native::map_overview::MapOverview>, String> {
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let game = game_path
            .filter(|p| !p.trim().is_empty())
            .map(std::path::PathBuf::from);
        let dirs = native::map_overview::overview_search_dirs(
            game.as_deref(),
            std::path::Path::new(&demo_path),
        );
        Ok(native::map_overview::find_map_overview(&dirs, &map_name))
    }))
    .await
}

#[cfg(test)]
mod analyzer_progress_tests {
    use super::analyzer_progress_payload;

    /// The fields analyzer_pane.js's `analyzer_progress` listener reads.
    #[test]
    fn a_cached_demo_is_said_to_be_cached() {
        let cached = analyzer_progress_payload("D:/demos/a.dem", 0, 0, true);
        assert_eq!(cached["path"], "D:/demos/a.dem");
        assert_eq!(cached["cached"], true);
        let parsing = analyzer_progress_payload("D:/demos/a.dem", 30, 100, false);
        assert_eq!(parsing["processed"], 30);
        assert_eq!(parsing["total"], 100);
        assert_eq!(parsing["cached"], false);
    }
}
