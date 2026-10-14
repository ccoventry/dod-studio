//! The game folder this game runs: the `<mod>` of `-game <mod>`, `dod` when
//! the command line has none (#402).
//!
//! Studio launches with its Game folder setting there, so reading it back
//! here keeps the DLL's direct disk reads (HD textures, layouts, overviews,
//! map files) in the folder the engine itself searches, with no build per
//! mod. The command line is read rather than the engine's
//! `pfnGetGameDirectory` because it is there the moment the DLL is injected,
//! and `pfnGetGameDirectory` only once `client.dll` has initialised, after
//! the first checks (`texture_hires`'s enable check at load) have run.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// What the engine runs without `-game`, as far as DoD Studio is concerned:
/// every launch it makes names a folder, and a hand-started DoD game names
/// `dod`.
pub(crate) const DEFAULT: &str = "dod";

/// The folder named after `-game` in `args`, or [`DEFAULT`]. The first
/// `-game` counts, as with the engine's own parameter lookup. A value that
/// is not a plain folder name (a path, `..`, another option) is ignored, so
/// nothing here builds a path outside the install.
pub(crate) fn from_args(args: impl IntoIterator<Item = String>) -> String {
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg.eq_ignore_ascii_case("-game") {
            return args
                .next()
                .map(|v| v.trim().trim_matches('"').to_string())
                .filter(|v| is_plain_name(v))
                .unwrap_or_else(|| DEFAULT.to_string());
        }
    }
    DEFAULT.to_string()
}

/// A single folder name: no separators, drive colon, `..`, or leading `-`/`+`.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains("..")
        && !name.starts_with(['-', '+'])
        && !name.chars().any(|c| {
            matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()
        })
}

/// This game's folder name, read off its command line once. Cached because
/// the command line never changes and the HD and layout lookups call this
/// often; `OnceLock::get` after the first call takes no lock.
pub(crate) fn name() -> &'static str {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| {
        // `args_os`, not `args`: `std::env::args()` panics on an argument
        // that isn't valid Unicode, and a panic is a game crash under
        // `panic = "abort"`.
        from_args(std::env::args_os().map(|a| a.to_string_lossy().into_owned()))
    })
}

/// The folder holding `hl.exe`.
pub(crate) fn game_root() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_default()
}

/// `<hl.exe folder>\<game folder>`.
pub(crate) fn mod_dir() -> PathBuf {
    game_root().join(name())
}

/// `<game folder>_addon` beside it: the engine names the `-addons` folder
/// after `-game`'s.
pub(crate) fn addon_name() -> String {
    format!("{}_addon", name())
}

/// `<game folder>_downloads`, where the engine saves server downloads; named
/// after `-game`'s like [`addon_name`].
pub(crate) fn downloads_name() -> String {
    format!("{}_downloads", name())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn the_folder_after_game_is_used() {
        assert_eq!(
            from_args(args("hl.exe -game dod_movies -insecure -addons")),
            "dod_movies"
        );
        assert_eq!(from_args(args("hl.exe -GAME dod_1_2")), "dod_1_2");
        assert_eq!(
            from_args(args("hl.exe -game \"dod_3_1_beta\" -w 800")),
            "dod_3_1_beta"
        );
    }

    #[test]
    fn no_game_option_means_dod() {
        assert_eq!(from_args(args("hl.exe -insecure -windowed")), "dod");
        assert_eq!(from_args(Vec::new()), "dod");
        // A dangling -game at the end of the line.
        assert_eq!(from_args(args("hl.exe -game")), "dod");
    }

    #[test]
    fn the_first_game_option_counts() {
        assert_eq!(
            from_args(args("hl.exe -game dod_movies -game valve")),
            "dod_movies"
        );
    }

    /// None of these may become a path: each falls back to `dod`.
    #[test]
    fn values_that_are_not_folder_names_fall_back_to_dod() {
        for line in [
            "hl.exe -game -insecure",
            "hl.exe -game +map",
            "hl.exe -game ../elsewhere",
            "hl.exe -game C:/Games/dod",
            r"hl.exe -game dod\maps",
            "hl.exe -game ..",
        ] {
            assert_eq!(from_args(args(line)), "dod", "{line}");
        }
    }

    #[test]
    fn the_siblings_are_named_after_the_folder() {
        // `name()` reads the test runner's own command line, which has no
        // `-game`.
        assert_eq!(name(), "dod");
        assert_eq!(addon_name(), "dod_addon");
        assert_eq!(downloads_name(), "dod_downloads");
        assert!(mod_dir().ends_with("dod"));
    }
}
