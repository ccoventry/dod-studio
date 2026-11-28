// map_builds_manager.rs
// Studio's map builds (#1547), the Studio half: adding a build from the map
// warnings, putting builds in place for a preview's fresh game, and clearing
// builds an earlier Studio left in place. The rules live in
// `native::maps::build_library`, `build_placement` and `build_plan`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use native::log_markdown;
use native::maps::build_placement::Placement;
use serde::Serialize;

/// How long a preview's game gets to show up before its builds are taken
/// back out (a launch that failed after the launcher started).
const PREVIEW_START_WAIT: Duration = Duration::from_secs(120);

/// The folder holding `hl.exe` for a configured game path.
fn game_root(game_path: &str) -> Result<PathBuf, String> {
    Path::new(game_path)
        .parent()
        .filter(|root| native::shared::paths::mod_dir(root).is_dir())
        .map(Path::to_path_buf)
        .ok_or_else(|| crate::messages::no_game_folder_beside(game_path))
}

/// What adding a build did, for the map warnings.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddedMapBuild {
    pub map_name: String,
    pub checksum: u32,
    pub path: String,
    pub already_there: bool,
}

/// One `map_build_progress` event: bytes read and written of the copy.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapBuildProgress {
    pub map: String,
    pub done: u64,
    pub total: u64,
}

/// Adds `source_path` to the map builds of the install `game_path` points at,
/// as a build of `map_name`: from the file picker, or a copy found beside the
/// map. The source is only read. Sends `map_build_progress` at most every
/// ~33 ms, and the last count.
#[tauri::command]
pub async fn add_map_build(
    app: tauri::AppHandle,
    map_name: String,
    source_path: String,
    game_path: String,
) -> Result<AddedMapBuild, String> {
    use tauri::Emitter;
    let root = game_root(&game_path)?;
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let library = native::maps::build_library::library_dir(&root);
        let source = PathBuf::from(&source_path);
        let started = Instant::now();
        let mut last_emit: Option<Instant> = None;
        let mut emit = |done: u64, total: u64| {
            let now = Instant::now();
            let due = done >= total
                || last_emit.is_none_or(|t| now - t >= crate::download_progress::EMIT_EVERY);
            if due {
                last_emit = Some(now);
                let _ = app.emit(
                    "map_build_progress",
                    MapBuildProgress {
                        map: map_name.clone(),
                        done,
                        total,
                    },
                );
            }
        };
        let added = native::maps::build_library::add_build(&library, &map_name, &source, &mut emit)
            .map_err(|e| {
                log_markdown(&native::messages::map_builds::add_failed(&map_name, &e));
                crate::messages::map_build_add_failed(&e)
            })?;
        let path = added.path.display();
        if added.already_there {
            log_markdown(&native::messages::map_builds::already_had(
                &added.map_name,
                added.checksum,
                path,
            ));
        } else {
            log_markdown(&native::messages::map_builds::added(
                &added.map_name,
                added.checksum,
                source.display(),
                path,
                started.elapsed().as_millis(),
            ));
        }
        Ok(AddedMapBuild {
            map_name: added.map_name,
            checksum: added.checksum,
            path: added.path.to_string_lossy().into_owned(),
            already_there: added.already_there,
        })
    }))
    .await
}

/// Puts the library build `demo`'s map needs in place before a preview
/// starts a fresh game of the install `game_path` points at. Nothing when
/// the demo needs none, or a game is running already.
pub(crate) fn place_for_preview(game_path: &str, demo: &Path) -> Option<Placement> {
    let root = game_root(game_path).ok()?;
    let placement = native::maps::build_plan::place_for_launch(&[demo.to_path_buf()], &root);
    (!placement.is_empty()).then_some(placement)
}

/// After the preview's launch: its builds come out once its game has closed,
/// or now when the launch failed.
pub(crate) fn after_preview_launch(placement: Option<Placement>, launched: bool) {
    let Some(placement) = placement else { return };
    if launched {
        placement.release_after_game(PREVIEW_START_WAIT);
    } else {
        placement.release();
    }
}

/// At start-up: builds an earlier Studio left in place in any install it
/// knows (the configured one and those found on this PC) are removed once no
/// game runs.
pub(crate) fn clean_leftovers_at_start(configured_game_path: Option<String>) {
    let mut roots: Vec<PathBuf> = native::sys::installs::find(None)
        .into_iter()
        .map(|install| PathBuf::from(install.path))
        .collect();
    if let Some(root) = configured_game_path
        .as_deref()
        .and_then(|p| game_root(p).ok())
    {
        roots.push(root);
    }
    roots.sort();
    roots.dedup();
    for root in roots {
        native::maps::build_placement::release_leftovers_after_games(
            &native::shared::paths::addon_dir(&root),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_progress_event_is_shaped_as_the_banner_reads_it() {
        let json = serde_json::to_value(MapBuildProgress {
            map: "dod_armory_b5".into(),
            done: 10,
            total: 20,
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "map": "dod_armory_b5", "done": 10, "total": 20 })
        );
    }

    #[test]
    fn a_game_path_with_no_game_folder_says_so() {
        let err = game_root("Z:/nowhere/hl.exe").unwrap_err();
        assert!(err.contains("Z:/nowhere/hl.exe"), "{err}");
    }
}
