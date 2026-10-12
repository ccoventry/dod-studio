//! Seek Between Clips (#434, step 3), the Studio half: when a batch can run
//! from the hook DLL on the original demos, writes the hook's batch file in
//! place of patching each demo. `native::patch::runtime_batch` builds the
//! file; goldsrc-hooks `batch.rs` runs it.

use std::path::{Path, PathBuf};

use native::patch::{CaptureStreak, PatchJob, PatcherConfig};

/// The batch file, beside Studio's other files in AppData.
fn batch_file_path() -> PathBuf {
    native::shared::paths::get_appdata_dir().join("dodstudio_batch.txt")
}

/// Writes the hook's batch file for `jobs` and returns its path, or says why
/// this batch has to be patched instead.
pub fn prepare(
    jobs: &[PatchJob],
    streaks: &[CaptureStreak],
    config: &PatcherConfig,
) -> Result<PathBuf, String> {
    if config.goldsrc_hooks_dll().is_none() {
        return Err(
            "the DoD Studio hook DLL isn't installed, and only it can run a batch this way".into(),
        );
    }
    let dod_dir = Path::new(&config.game_path)
        .parent()
        .map(|p| p.join("dod"))
        .ok_or_else(|| format!("{} has no folder", config.game_path))?;
    for job in jobs.iter().filter(|j| !j.blocks.is_empty()) {
        let demo = Path::new(&job.source_demo);
        if native::preview_in_place::viewdemo_arg(&dod_dir, demo).is_none() {
            return Err(format!(
                "the game can't open {} with viewdemo: it has to be on the same drive as {}",
                demo.display(),
                dod_dir.display()
            ));
        }
    }
    let text = native::patch::runtime_batch::batch_file(jobs, streaks, config.decal_flush)?;
    let path = batch_file_path();
    std::fs::write(&path, text)
        .map_err(|e| format!("could not write the batch file {}: {e}", path.display()))?;
    native::log_markdown(&format!(
        "⏩ **Seek Between Clips** — the hook DLL plays the original demos and seeks to each clip; \
         nothing is patched. Batch file: `{}`",
        path.display()
    ));
    Ok(path)
}
