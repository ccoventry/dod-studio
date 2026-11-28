//! The copies a capture batch plays in place of its demos: a trimmed copy of
//! a demo over the engine's entity limit (#207, `trimmed_demos`) and, when
//! the hook DLL runs the batch with `viewdemo`, a converted copy of a
//! protocol-47 demo (#1549, `protocol48_demos`). The originals are never
//! written.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use native::patch::copy_cache::Pins;
use native::patch::{PatchJob, PatcherConfig};

/// What a batch plays from. Hold `pins` until the batch's game is done with
/// the copies: they keep them from being pruned.
pub struct PlayedCopies {
    /// Each original's trimmed copy (originals under the limit aren't in
    /// it): what a patched batch patches from.
    pub trimmed: HashMap<String, PathBuf>,
    /// The file a hook-run batch plays for each original: its converted
    /// copy, else its trimmed copy (neither: the original plays).
    pub hook_plays: HashMap<String, PathBuf>,
    pub pins: (Pins, Pins),
}

/// Makes (or reuses) the copies `jobs` play from, reporting each step on the
/// Capture footer. `Err` is the batch's error line: a demo that can't be
/// trimmed or converted, a copy with no room, or Cancel.
pub fn prepare(
    app: &tauri::AppHandle,
    jobs: &[PatchJob],
    config: &PatcherConfig,
    cancel: &AtomicBool,
) -> Result<PlayedCopies, String> {
    let trimmed = crate::trimmed_demos::for_batch(app, jobs, &config.game_path, cancel)?;
    let converted = crate::protocol48_demos::for_batch(app, jobs, &trimmed.copies, config, cancel)?;
    Ok(PlayedCopies {
        hook_plays: crate::protocol48_demos::plays_from(&trimmed.copies, &converted.copies),
        trimmed: trimmed.copies,
        pins: (trimmed.pins, converted.pins),
    })
}
