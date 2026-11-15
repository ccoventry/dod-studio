// map_manager.rs
// Whether the demos on screen can actually be played, and on the right map.
//
// Kept out of the scan pipeline deliberately. Scanning parses every frame of
// every demo and is the slow, threaded part of loading a folder; this reads 544
// bytes per demo and one map file per distinct map, so it runs after a scan
// without slowing it, and a failure here costs a badge rather than the folder.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use native::patch::map_check::{self, MapStatus};
use native::patch::map_fetch;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapCheckRow {
    pub demo_path: String,
    pub demo_name: String,
    pub map_name: String,
    /// The build the demo was recorded on. Absent for HLTV demos, which do not
    /// record one.
    pub expected_checksum: Option<u32>,
    /// `ok` | `wrongBuild` | `missing` | `unverifiable` | `unreadableMap` |
    /// `unreadableDemo`
    pub state: String,
    pub detail: String,
    /// Whether the demo can be played at all.
    pub playable: bool,
}

/// Where maps live for a configured `hl.exe`, or an error a person can act on.
fn maps_dir(game_path: &str) -> Result<PathBuf, String> {
    let exe = Path::new(game_path);
    map_check::maps_dir_for_exe(exe)
        .ok_or_else(|| crate::messages::no_map_folder_beside_exe(game_path))
}

/// Check a list of demos against the map library.
///
/// Map files are read once each however many demos want them, because the
/// checksum walks the whole file and a folder of 400 demos covers maybe twenty
/// maps.
#[tauri::command]
pub async fn check_demo_maps(
    demo_paths: Vec<String>,
    game_path: String,
) -> Result<Vec<MapCheckRow>, String> {
    let dir = maps_dir(&game_path)?;

    tokio::task::spawn_blocking(move || {
        let mut seen: HashMap<String, MapStatus> = HashMap::new();
        let mut rows = Vec::with_capacity(demo_paths.len());

        for path in demo_paths {
            let p = PathBuf::from(&path);
            let demo_name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.clone());

            let reference = match map_check::map_reference(&p) {
                Ok(r) => r,
                Err(e) => {
                    rows.push(MapCheckRow {
                        demo_path: path,
                        demo_name,
                        map_name: String::new(),
                        expected_checksum: None,
                        state: "unreadableDemo".to_string(),
                        detail: e,
                        playable: false,
                    });
                    continue;
                }
            };

            // Keyed on name AND wanted build: two demos of the same map name
            // wanting different builds are genuinely different questions.
            let cache_key = format!(
                "{}:{}",
                reference.map_name,
                reference.expected_checksum.unwrap_or(0)
            );
            let status = seen
                .entry(cache_key)
                .or_insert_with(|| map_check::status_of(&reference, &dir))
                .clone();

            let state = match &status {
                MapStatus::Ok { .. } => "ok",
                MapStatus::WrongBuild { .. } => "wrongBuild",
                MapStatus::Missing => "missing",
                MapStatus::Unverifiable => "unverifiable",
                MapStatus::Unreadable { .. } => "unreadableMap",
            };

            rows.push(MapCheckRow {
                demo_path: path,
                demo_name,
                detail: status.summary(&reference.map_name),
                playable: status.is_playable(),
                map_name: reference.map_name,
                expected_checksum: reference.expected_checksum,
                state: state.to_string(),
            });
        }

        rows
    })
    .await
    .map_err(crate::messages::map_check_failed)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapFetchResult {
    pub map_name: String,
    pub installed_path: String,
    pub checksum: u32,
    pub bytes: u64,
    pub already_correct: bool,
    /// Where an existing file was moved to, when one was in the way. Nothing is
    /// ever overwritten in place.
    pub replaced_path: Option<String>,
}

/// One `map_download_progress` event. `step` is `"downloading"` or
/// `"checking"` (the checksum read and the install).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapDownloadProgress {
    pub map: String,
    pub step: &'static str,
    pub downloaded: u64,
    pub total: Option<u64>,
}

/// Download one map and install it, verified against the build the demo wants.
///
/// This writes into the user's game folder and talks to the network, so it is
/// only ever reached from an explicit action — never from a scan, and never as
/// a side effect of starting a capture.
///
/// Sends `map_download_progress` ([`MapDownloadProgress`]) as it goes: bytes
/// at most every ~33 ms, the last count, then the check (#957).
#[tauri::command]
pub async fn download_map(
    app: tauri::AppHandle,
    map_name: String,
    expected_checksum: Option<u32>,
    game_path: String,
) -> Result<MapFetchResult, String> {
    use tauri::Emitter;
    let dir = maps_dir(&game_path)?;

    tokio::task::spawn_blocking(move || {
        let mut meter = crate::download_progress::DownloadMeter::default();
        let emit = |step: &'static str, p: crate::download_progress::DownloadProgress| {
            let _ = app.emit(
                "map_download_progress",
                MapDownloadProgress {
                    map: map_name.clone(),
                    step,
                    downloaded: p.downloaded,
                    total: p.total,
                },
            );
        };
        map_fetch::fetch_map_with_progress(
            &map_name,
            expected_checksum,
            &dir,
            map_fetch::DEFAULT_MIRROR,
            &mut |step| match step {
                map_fetch::FetchStep::Downloaded { chunk, total } => {
                    if let Some(p) = meter.chunk(chunk, total, std::time::Instant::now()) {
                        emit("downloading", p);
                    }
                }
                map_fetch::FetchStep::Checking => {
                    emit("downloading", meter.progress());
                    emit("checking", meter.progress());
                }
            },
        )
        .map(|o| MapFetchResult {
            map_name: o.map_name,
            installed_path: o.installed.to_string_lossy().to_string(),
            checksum: o.checksum,
            bytes: o.bytes,
            already_correct: o.already_correct,
            replaced_path: o.replaced.map(|p| p.to_string_lossy().to_string()),
        })
    })
    .await
    .map_err(crate::messages::map_download_failed)?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_map_download_event_is_shaped_as_the_banner_reads_it() {
        let json = serde_json::to_value(MapDownloadProgress {
            map: "dod_anzio".into(),
            step: "downloading",
            downloaded: 2_100_000,
            total: Some(6_000_000),
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "map": "dod_anzio",
                "step": "downloading",
                "downloaded": 2_100_000,
                "total": 6_000_000,
            })
        );
    }

    #[test]
    fn a_game_path_with_no_map_folder_says_so_rather_than_failing_later() {
        let err = maps_dir("Z:/nowhere/hl.exe").unwrap_err();
        assert!(err.contains("dod/maps"), "{}", err);
    }
}
