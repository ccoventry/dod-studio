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

/// How long after a preview's launch call returns the release waits for the
/// HLAE launcher or the game to be seen. HLAE is running by then (the launch
/// started it), and HLAE counts as a game on its way, so a slow Steam start
/// keeps the build however long it takes; this only covers a launch through
/// WMI whose process takes a moment to show.
const PREVIEW_START_WAIT: Duration = Duration::from_secs(15);

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

/// Shows each of `warnings` as a warning toast (`map_build_warning`, read by
/// map_builds.js). They are in the activity log already.
pub(crate) fn emit_warnings(app: &tauri::AppHandle, warnings: &[String]) {
    use tauri::Emitter;
    for warning in warnings {
        let _ = app.emit("map_build_warning", warning);
    }
}

/// The demos a batch plays, in play order (the order of the `DEMO_START`
/// markers): its jobs with clips, so not the primer. A trimmed copy (#207)
/// names the same map and build as its original.
pub(crate) fn map_demos(jobs: &[native::patch::PatchJob]) -> Vec<PathBuf> {
    jobs.iter()
        .filter(|j| !j.blocks.is_empty())
        .map(|j| PathBuf::from(&j.source_demo))
        .collect()
}

/// Puts the library build `demo`'s map needs in place before a preview
/// starts a fresh game of the install `game_path` points at, and shows the
/// warnings (a game in the way, a build that couldn't be placed). `None`
/// when nothing was placed.
pub(crate) fn place_for_preview(
    app: &tauri::AppHandle,
    game_path: &str,
    demo: &Path,
) -> Option<Placement> {
    let root = game_root(game_path).ok()?;
    let launch = native::maps::build_plan::place_for_launch(&[demo.to_path_buf()], &root);
    emit_warnings(app, &launch.warnings);
    (!launch.placement.is_empty()).then_some(launch.placement)
}

/// For a preview sent to a game that runs already: that game keeps the map
/// builds it has loaded, so the user hears when `demo` won't play on its own.
pub(crate) fn warn_for_open_game(app: &tauri::AppHandle, game_path: &str, demo: &Path) {
    let Ok(root) = game_root(game_path) else {
        return;
    };
    let warnings = native::maps::build_plan::open_game_warnings(&[demo.to_path_buf()], &root);
    emit_warnings(app, &warnings);
}

/// A demo a batch would play on another build of its map than it was
/// recorded on, because another demo of that map needs another build.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftOutDemo {
    pub demo_path: String,
    pub demo_name: String,
    pub map_name: String,
    /// The old build the batch plays that map on instead, as 8 hex digits,
    /// or `None` for the installed one.
    pub playing: Option<String>,
}

/// The demos of a batch of `demo_paths` that won't play on their build, for
/// the confirm before Start (map_build_confirm.js). Puts nothing in place.
#[tauri::command]
pub async fn plan_map_builds(
    demo_paths: Vec<String>,
    game_path: String,
) -> Result<Vec<LeftOutDemo>, String> {
    let root = game_root(&game_path)?;
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let demos: Vec<PathBuf> = demo_paths.iter().map(PathBuf::from).collect();
        let maps = native::maps::build_plan::plan_for(&demos, &root);
        Ok(left_out_demos(&maps.left_out))
    }))
    .await
}

fn left_out_demos(left_out: &[native::maps::build_plan::LeftOut]) -> Vec<LeftOutDemo> {
    left_out
        .iter()
        .map(|left| LeftOutDemo {
            demo_path: left.demo.to_string_lossy().into_owned(),
            demo_name: left
                .demo
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            map_name: left.map_name.clone(),
            playing: left.playing.map(native::messages::map_builds::build),
        })
        .collect()
}

/// For a batch about to run in the game that is open already: one warning
/// per map whose demos that game won't play on their build (it keeps the
/// builds it has loaded), for the confirm before Start. Puts nothing in place.
#[tauri::command]
pub async fn open_game_map_warnings(
    demo_paths: Vec<String>,
    game_path: String,
) -> Result<Vec<String>, String> {
    let root = game_root(&game_path)?;
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let demos: Vec<PathBuf> = demo_paths.iter().map(PathBuf::from).collect();
        Ok(native::maps::build_plan::open_game_warnings(&demos, &root))
    }))
    .await
}

/// When a preview's builds come out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewRelease {
    /// The launch failed: no game will read them.
    Now,
    /// Once the game it started has closed.
    AfterTheGame,
}

fn preview_release(launched: bool) -> PreviewRelease {
    if launched {
        PreviewRelease::AfterTheGame
    } else {
        PreviewRelease::Now
    }
}

/// After the preview's launch: its builds come out once its game has closed,
/// or now when the launch failed.
pub(crate) fn after_preview_launch(placement: Option<Placement>, launched: bool) {
    let Some(placement) = placement else { return };
    match preview_release(launched) {
        PreviewRelease::AfterTheGame => placement.release_after_game(PREVIEW_START_WAIT),
        PreviewRelease::Now => {
            placement.release();
        }
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
        let addon = native::shared::paths::addon_dir(&root);
        native::maps::build_library::sweep_unfinished_adds(
            &native::maps::build_library::library_dir(&root),
        );
        native::maps::build_placement::sweep_part_files(&addon);
        native::maps::build_placement::release_leftovers_after_games(&addon);
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

    fn job(demo: &str, clips: usize) -> native::patch::PatchJob {
        native::patch::PatchJob {
            source_demo: demo.into(),
            output_demo: PathBuf::from(format!("{demo}.out")),
            streaks: vec![],
            target_player: None,
            init_commands: vec![],
            scheduled_commands: vec![],
            director_events: vec![],
            block_routes: vec![],
            blocks: (0..clips)
                .map(|i| native::patch::types::CaptureBlock {
                    demo_name: demo.into(),
                    block_index: i,
                    drive_index: 0,
                    take_folder: PathBuf::from("t"),
                    take_key: String::new(),
                    source_streak_indices: vec![],
                    start_tick: 0,
                    end_tick: 1,
                    record_start_tick: 0,
                    record_stop_tick: 1,
                    ramped_gaps: 0,
                    plan: None,
                })
                .collect(),
        }
    }

    #[test]
    fn a_batchs_map_demos_are_its_demos_with_clips_in_play_order_without_the_primer() {
        let jobs = [
            job("first.dem", 0),
            job("b.dem", 2),
            job("a.dem", 1),
            job("b.dem", 1),
        ];
        assert_eq!(
            map_demos(&jobs),
            vec![
                PathBuf::from("b.dem"),
                PathBuf::from("a.dem"),
                PathBuf::from("b.dem")
            ]
        );
    }

    #[test]
    fn a_demo_left_out_of_a_batch_is_named_for_the_confirm() {
        let json = serde_json::to_value(left_out_demos(&[native::maps::build_plan::LeftOut {
            demo: PathBuf::from("C:/d/old.dem"),
            map_name: "dod_x".into(),
            wanted: 2,
            playing: None,
        }]))
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!([{ "demoPath": "C:/d/old.dem", "demoName": "old.dem", "mapName": "dod_x", "playing": null }])
        );
    }

    /// The smallest map the checksum reads, its first lump holding `seed`.
    fn map_bytes(seed: u8) -> Vec<u8> {
        let header = 4 + 15 * 8;
        let mut bytes = vec![0u8; header + 4];
        bytes[0] = 30;
        bytes[12..16].copy_from_slice(&(header as u32).to_le_bytes());
        bytes[16..20].copy_from_slice(&4u32.to_le_bytes());
        bytes[header..].copy_from_slice(&[seed; 4]);
        bytes
    }

    #[test]
    fn a_preview_that_failed_to_launch_takes_its_build_out_at_once() {
        let scratch = crate::test_support::Scratch::new("map_builds_preview");
        let addon = scratch.path().join("dod_addon");
        let source = scratch.path().join("old.bsp");
        std::fs::write(&source, map_bytes(2)).unwrap();
        let library = addon.join(native::maps::build_library::LIBRARY_FOLDER);
        let added =
            native::maps::build_library::add_build(&library, "dod_x", &source, &mut |_, _| {})
                .unwrap();
        let want = [native::maps::build_placement::MapBuild {
            map_name: "dod_x".into(),
            checksum: added.checksum,
        }];
        let slot = native::maps::build_placement::slot_path(&addon, "dod_x");

        let (failed, _) = native::maps::build_placement::place_builds(&addon, &want);
        assert!(slot.exists());
        after_preview_launch(Some(failed), false);
        assert!(!slot.exists(), "a launch that failed starts no game");
    }

    #[test]
    fn a_preview_that_launched_keeps_its_build_until_the_game_has_gone() {
        // The wait itself is native's (`wait_for_games_to_end`, tested there
        // with a scripted process list).
        assert_eq!(preview_release(true), PreviewRelease::AfterTheGame);
        assert_eq!(preview_release(false), PreviewRelease::Now);
    }

    #[test]
    fn closing_studio_takes_its_map_builds_out() {
        // The exit handler in lib.rs calls the native release (#1547); a
        // build left in place would play every later demo of that map on it.
        let lib = include_str!("lib.rs");
        let exit = lib
            .split("if let tauri::RunEvent::Exit = event {")
            .nth(1)
            .and_then(|rest| rest.split('}').next())
            .expect("the exit handler");
        assert!(
            exit.contains("native::maps::build_placement::release_at_exit()"),
            "{exit}"
        );
    }
}
