//! Reporting what the game's own configs set that the pipeline depends on.

use super::{cfg_scan, game_dir_for};
use crate::patch::types::PatcherConfig;

/// Report anything the game's configs set that the pipeline depends on.
///
/// Read-only, and advisory: the user's configs are theirs to change. This only
/// makes sure a value the app never hears about does not stay invisible.
pub(crate) fn warn_about_game_cfgs(config: &PatcherConfig) {
    let Some(dir) = game_dir_for(config) else {
        return;
    };
    let scan = cfg_scan::scan_cached(&dir);
    let found = scan.effective_settings();
    if found.is_empty() {
        return;
    }

    // Once per folder per run. This is called per demo, and a batch of forty
    // would otherwise write the same paragraph into the activity log forty
    // times — which is how a warning worth reading becomes one nobody reads.
    {
        static WARNED: std::sync::OnceLock<
            std::sync::RwLock<std::collections::HashSet<std::path::PathBuf>>,
        > = std::sync::OnceLock::new();
        let warned = WARNED.get_or_init(Default::default);
        let Ok(mut w) = warned.write() else {
            return;
        };
        if !w.insert(dir.clone()) {
            return;
        }
    }

    let lines: Vec<String> = found
        .iter()
        .map(|s| {
            format!(
                "`{} {}` in `{}` line {}",
                s.cvar,
                s.value,
                s.file_name(),
                s.line
            )
        })
        .collect();

    crate::log_markdown(&crate::messages::decal_flush_game_cfgs_set_values(
        lines.join(", "),
    ));
}
