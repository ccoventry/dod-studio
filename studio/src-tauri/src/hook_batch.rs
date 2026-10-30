//! Seek Between Clips (#434, step 3), the Studio half: when a batch can run
//! from the hook DLL on the original demos, writes the hook's batch file in
//! place of patching each demo. `native::patch::runtime_batch` builds the
//! file; goldsrc-hooks `batch.rs` runs it. Before the file: highlights with
//! no demo-player times get them from a fresh read of their demo
//! (`viewdemo_refresh`). A demo on another drive than the game plays where it
//! is, by its full path, when the hook DLL can open it that way (#779); for
//! an older DLL it is copied into the game's folder for the batch
//! (`source_copies`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use native::patch::runtime_batch::RuntimeBatch;
use native::patch::{CaptureStreak, PatchJob, PatcherConfig};
use tauri::Emitter;

/// The batch file, beside Studio's other files in AppData.
fn batch_file_path() -> PathBuf {
    native::shared::paths::get_appdata_dir().join("dodstudio_batch.txt")
}

/// The game folder for `config` (`dod`, or the Game Folder setting, #402).
fn dod_dir(config: &PatcherConfig) -> Result<PathBuf, String> {
    native::shared::paths::mod_dir_for_exe(Path::new(&config.game_path))
        .ok_or_else(|| format!("{} has no folder", config.game_path))
}

/// Reads again every demo whose highlights have kills but no demo-player
/// times (a project saved before they existed), and fills them in, one
/// demo at a time with a status line per demo. A demo that still yields none
/// is left as it is: `prepare` then says so and the batch is patched.
pub fn refresh_viewdemo_times(app: &tauri::AppHandle, streaks: &mut [CaptureStreak]) {
    use native::patch::viewdemo_refresh::{demos_needing_times, fill_times};
    let demos = demos_needing_times(streaks);
    let total = demos.len() as u32;
    for (i, demo) in demos.iter().enumerate() {
        let name = Path::new(demo)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| demo.clone());
        let _ = app.emit(
            "capture_status",
            serde_json::json!({
                "running": true,
                "stage": "reading_again",
                "index": i as u32,
                "total": total,
                "status": crate::messages::reading_demo_again(&name, i as u32 + 1, total),
            }),
        );
        match native::patch::scan_demo_for_highlights(Path::new(demo)) {
            Ok((_, fresh, ..)) => {
                let filled = fill_times(streaks, demo, &fresh);
                native::log_markdown(&format!(
                    "🔎 **Read again** — `{name}` had no demo-player times for its kills (a project saved before they existed); {filled} highlight(s) have them now"
                ));
            }
            Err(e) => native::log_markdown(&format!(
                "⚠️ **Read again** — `{name}` could not be read for its kills' demo-player times: {e}"
            )),
        }
    }
}

/// Writes the hook's batch file for `jobs` and returns it with the copies it
/// plays from, or says why this batch has to be patched instead. Nothing is
/// copied for a batch that then can't run this way.
pub fn prepare(
    jobs: &[PatchJob],
    streaks: &[CaptureStreak],
    config: &PatcherConfig,
) -> Result<RuntimeBatch, String> {
    let Some(dll) = config.goldsrc_hooks_dll() else {
        return Err(
            "the DoD Studio hook DLL isn't installed, and only it can run a batch this way".into(),
        );
    };
    let other_drives = native::preview_in_place::hook_plays_other_drives(&dll);
    let dod_dir = dod_dir(config)?;
    let demos: Vec<&str> = jobs
        .iter()
        .filter(|j| !j.blocks.is_empty())
        .map(|j| j.source_demo.as_str())
        .collect();
    let swept = native::patch::source_copies::remove_stale(&dod_dir);
    if swept > 0 {
        native::log_markdown(&format!(
            "🧹 **Cleanup** — removed {swept} demo cop{} an earlier batch left in `{}`",
            if swept == 1 { "y" } else { "ies" },
            dod_dir.display()
        ));
    }
    let copies = native::patch::source_copies::plan(&dod_dir, demos.iter().copied(), other_drives)
        .map_err(crate::messages::failed_to_copy_demos_for_batch)?;
    // Demos on another drive the hook opens by their full path.
    let mut in_place: Vec<&str> = Vec::new();
    if other_drives {
        for &demo in &demos {
            let path = Path::new(demo);
            if !in_place.contains(&demo)
                && !copies.iter().any(|c| c.source.as_path() == path)
                && native::preview_in_place::viewdemo_arg(&dod_dir, path).is_none()
            {
                in_place.push(demo);
            }
        }
    }
    let play_as: HashMap<String, PathBuf> = copies
        .iter()
        .map(|c| (c.source.to_string_lossy().into_owned(), c.copy.clone()))
        .collect();
    // The file first: a batch that can't run this way copies nothing.
    let text = native::patch::runtime_batch::batch_file_playing(
        jobs,
        streaks,
        config.decal_flush,
        &play_as,
        // OBS records in real time: each clip waits for it to start (#1109).
        config.capture_mode == native::patch::CaptureMode::Obs,
    )?;
    if !copies.is_empty() {
        let needed: u64 = copies.iter().map(|c| c.bytes).sum();
        let free = native::sys::disk::get_available_bytes(&dod_dir);
        native::patch::source_copies::space_check(free, needed)
            .map_err(crate::messages::failed_to_copy_demos_for_batch)?;
        native::patch::source_copies::copy_all(&copies)
            .map_err(crate::messages::failed_to_copy_demos_for_batch)?;
    }
    let path = batch_file_path();
    if let Err(e) = std::fs::write(&path, text) {
        native::patch::source_copies::remove_all(copies.iter().map(|c| c.copy.as_path()));
        return Err(format!(
            "could not write the batch file {}: {e}",
            path.display()
        ));
    }
    native::log_markdown(&format!(
        "⏩ **Seek Between Clips** — the hook DLL plays the original demos and seeks to each clip; \
         nothing is patched. Batch file: `{}`",
        path.display()
    ));
    for demo in &in_place {
        native::log_markdown(&format!(
            "💽 **Played in place** — `{}` is on another drive than the game; the hook DLL opens it by its full path, nothing is copied",
            Path::new(demo).display()
        ));
    }
    Ok(RuntimeBatch {
        file: path,
        source_copies: copies.into_iter().map(|c| c.copy).collect(),
        other_drives_in_place: !in_place.is_empty(),
        // Set by the caller for a batch in the open game (#725).
        reuse_game: None,
    })
}
