//! The maps a game session crashed on (#207): `native::crash_maps`, for the
//! warning before a capture batch.

use native::crash_maps;
use serde::Serialize;
use std::path::Path;

/// A picked demo on a map a session crashed on.
#[derive(Debug, Clone, Serialize)]
pub struct CrashMapWarning {
    pub demo_name: String,
    pub map: String,
    pub cause: String,
    pub build: Option<String>,
    pub count: u32,
    pub last_unix_secs: u64,
}

/// The demos in `demo_paths` recorded on a map a session crashed on with a
/// known cause. Reads each demo's header only.
#[tauri::command]
pub async fn crash_map_warnings(demo_paths: Vec<String>) -> Result<Vec<CrashMapWarning>, String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let dir = native::shared::paths::get_appdata_dir();
        let remembered = crash_maps::load(&dir);
        if remembered.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for path in demo_paths {
            let Ok(reference) = native::patch::map_check::map_reference(Path::new(&path)) else {
                continue;
            };
            let demo_name = Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.clone());
            for crash in remembered
                .iter()
                .filter(|c| c.map.eq_ignore_ascii_case(&reference.map_name))
            {
                out.push(CrashMapWarning {
                    demo_name: demo_name.clone(),
                    map: crash.map.clone(),
                    cause: crash.cause.clone(),
                    build: crash.build.clone(),
                    count: crash.count,
                    last_unix_secs: crash.last_unix_secs,
                });
            }
        }
        Ok(out)
    }))
    .await
}

/// Forgets the crashes remembered on `map`.
#[tauri::command]
pub fn forget_crash_map(map: String) -> Result<(), String> {
    crash_maps::forget(&native::shared::paths::get_appdata_dir(), &map).map_err(|e| e.to_string())
}
