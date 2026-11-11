// messages/settings.rs
// Activity-log lines of settings that fall back or move a file
// (`players_tab.rs`, `settings_manager.rs`, `project_convert.rs`,
// `project_settings_snapshot.rs`) (#237).

use std::fmt::Display;
use std::path::Path;

pub fn players_table_not_written(err: impl Display) -> String {
    format!("⚠️ **Players** — {err}. The game keeps the table it last read.")
}

/// An unusable Game folder setting; `default` is the folder used instead.
pub fn game_folder_fell_back(err: impl Display, default: impl Display) -> String {
    format!(
        "⚠️ **Settings** — {err}. Using `{default}` until it is changed in Configuration → Paths (#402)."
    )
}

pub fn project_switched(from: impl Display, to: impl Display) -> String {
    format!("📁 **Project** — switched {from} to {to}")
}

// ── The settings snapshot older projects start from (project_settings_snapshot.rs, #1085) ──

/// Why a file or a candidate can't be used as the snapshot.
pub const SNAPSHOT_NOT_A_SNAPSHOT: &str = "it has no `settings` block";

/// The snapshot was taken: once per PC, the first start of a build with
/// project settings.
pub fn snapshot_taken(path: &Path, taken_at: &str, elapsed_ms: u128) -> String {
    format!(
        "📁 **Settings** — took the snapshot of Configuration that older projects start from: {} (taken {taken_at}, {elapsed_ms} ms)",
        path.display()
    )
}

/// An unreadable snapshot, moved aside before a new one is taken.
pub fn snapshot_damaged(path: &Path, moved_to: &Path, reason: impl Display) -> String {
    format!(
        "⚠️ **Settings** — the snapshot older projects start from, {}, could not be read ({reason}): moved to {} and taken again from Configuration",
        path.display(),
        moved_to.display()
    )
}

pub fn snapshot_not_written(path: &Path, err: impl Display) -> String {
    format!(
        "Could not save the settings snapshot older projects start from at {}: {err}",
        path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_lines_keep_their_wording() {
        assert_eq!(
            players_table_not_written("x"),
            "⚠️ **Players** — x. The game keeps the table it last read."
        );
        assert_eq!(
            game_folder_fell_back("bad", "dod"),
            "⚠️ **Settings** — bad. Using `dod` until it is changed in Configuration → Paths (#402)."
        );
        assert_eq!(
            project_switched("a", "b"),
            "📁 **Project** — switched a to b"
        );
    }
}
