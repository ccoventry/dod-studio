//! The one place this DLL's console prefix is spelled.
//!
//! Every cvar and command it registers goes through [`console_name`], so
//! renaming the whole surface is a single edit here rather than a hunt through
//! five files for string literals that happen to start the same way. The
//! prefix is `ds_` (#555): short to type, and nothing in the engine, the DoD
//! client or HLAE has a name starting with it.
//!
//! The prefix used to be `dodstudio_` ([`OLD_PREFIX`]), and #555 also put
//! each name's area second (`ds_hud_hide_scoreboard`). The old names were
//! dropped without aliases; only DoD Studio's own files still read them
//! (`studio_panel`'s saved settings and layouts, through
//! [`crate::old_names`]).
//!
//! It is a macro rather than a `const` + runtime `format!` because these names
//! are needed at compile time: they end up in `const` items and in the
//! `format!` strings of every usage and error message, where a runtime
//! concatenation would mean allocating, and would stop the names being usable
//! in `concat!`/inline-format position at all.
//!
//! ## Aliases
//!
//! Nothing here forces one name per feature. [`crate::commands::add_commands`]
//! takes a slice, so a command can be registered under several names at once —
//! a renamed command can keep its old name working for a release, and a variant
//! spelling costs one more array entry:
//!
//! ```ignore
//! add_commands(&[console_name!("hud_deathmsg"), console_name!("killfeed")], command);
//! ```

/// Builds a console name from the shared prefix.
///
/// ```ignore
/// const STATUS: &str = console_name!("status");   // "ds_status"
/// ```
macro_rules! console_name {
    ($suffix:literal) => {
        concat!("ds_", $suffix)
    };
}

pub(crate) use console_name;

/// The one switch for everything whose job is making a spectated first-person
/// view look and sound like the player's own recording: the viewmodel's
/// animations (`anim_fix`), the gunshots an HLTV demo lost (`missing_shots`)
/// and the crosshair (`spectator_crosshair`). It lives here because all three
/// modules name it in their own messages.
///
/// Like every name (#555), the word after the prefix is the area a command
/// belongs to, as HLAE's `mirv_movie_*` and `mirv_fx_*` are, and the action
/// comes after it. `spec` is the game's own word for the spectator
/// (`spec_mode`, `spec_menu`).
pub(crate) const SPEC_MATCH_POV: &str = console_name!("spec_match_pov");

/// The prefix every name had before #555. Read, never registered: a name in
/// one of our own files (the saved settings, a layout's button or check box)
/// that still carries it is taken as the name that replaced it
/// ([`crate::old_names::OLD_NAMES`]).
pub(crate) const OLD_PREFIX: &str = "dodstudio_";

/// The name that replaced a pre-#555 one (case does not matter, as it does
/// not to the console); any other name as it is.
pub(crate) fn current_name(name: &str) -> std::borrow::Cow<'_, str> {
    crate::old_names::OLD_NAMES
        .iter()
        .find(|(old, _)| old.eq_ignore_ascii_case(name))
        .map_or(name.into(), |(_, new)| (*new).into())
}

/// A console line with [`current_name`] applied to its first word.
// Only the in-game window's buttons use it, and the window is 32-bit only.
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
pub(crate) fn current_command(line: &str) -> std::borrow::Cow<'_, str> {
    let line = line.trim_start();
    let (word, rest) = line.split_at(line.find(char::is_whitespace).unwrap_or(line.len()));
    let new = current_name(word);
    if new == word {
        line.into()
    } else {
        format!("{new}{rest}").into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pre_555_name_reads_as_the_ds_name() {
        assert_eq!(current_name("dodstudio_seek_by"), "ds_demo_seek_by");
        assert_eq!(current_name("ds_demo_seek_by"), "ds_demo_seek_by");
        assert_eq!(
            current_name("DodStudio_Hide_Scoreboard"),
            "ds_hud_hide_scoreboard"
        );
        assert_eq!(current_name("dodstudio_"), "dodstudio_");
        assert_eq!(current_name("dodstudio_hd"), "dodstudio_hd");
        assert_eq!(current_name("hud_draw"), "hud_draw");
        assert_eq!(
            current_command("dodstudio_seek_by -10"),
            "ds_demo_seek_by -10"
        );
        assert_eq!(current_command("  dodstudio_panel"), "ds_ui_panel");
        assert_eq!(
            current_command("ds_demo_seek_by -10"),
            "ds_demo_seek_by -10"
        );
        assert_eq!(
            current_command("echo dodstudio_panel"),
            "echo dodstudio_panel"
        );
        assert_eq!(current_command(""), "");
    }

    /// Every new name in the table is one this DLL registers, so a moved
    /// setting never lands on a name that does nothing.
    #[test]
    fn every_new_name_is_registered() {
        let mut code = String::new();
        let mut dirs = vec![std::path::PathBuf::from("src")];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    code.push_str(&std::fs::read_to_string(&path).unwrap());
                }
            }
        }
        for (old, new) in crate::old_names::OLD_NAMES {
            let suffix = new.strip_prefix(console_name!("")).unwrap();
            assert!(
                code.contains(&format!("console_name!(\"{suffix}\")")),
                "{old} -> {new}: {new} is not registered"
            );
        }
    }
}
