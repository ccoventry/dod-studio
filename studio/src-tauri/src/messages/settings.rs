// messages/settings.rs
// Activity-log lines of settings that fall back or move a file
// (`players_tab.rs`, `settings_manager.rs`, `project_convert.rs`) (#237).

use std::fmt::Display;

/// Settings migration 3 changed a used table's POV fallback (#1322).
pub const POV_OWN_SPRAY_RETIRED: &str =
    "[settings] POV fallback spray: \"Their own spray\" is gone, so it is now \"No spray\" (#1322)";

pub fn players_table_not_written(err: impl Display) -> String {
    format!("⚠️ **Players** — {err}. The game keeps the table it last read.")
}

/// An unusable Game folder setting; `default` is the folder used instead.
pub fn game_folder_fell_back(err: impl Display, default: impl Display) -> String {
    format!(
        "⚠️ **Settings** — {err}. Using `{default}` until it is changed in Configuration → Paths (#402)."
    )
}

/// "Keep free on the Windows drive" (#1430) now keeps `bytes`.
pub fn windows_drive_keep_free(bytes: u64) -> String {
    format!(
        "[settings] Keep free on the Windows drive: {} (captures and renders skip the Windows drive once it would drop to this)",
        native::hlcr::take_cleanup::format_gb(bytes)
    )
}

pub fn project_switched(from: impl Display, to: impl Display) -> String {
    format!("📁 **Project** — switched {from} to {to}")
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
