//! The one place this DLL's console prefix is spelled.
//!
//! Every cvar and command it registers goes through [`console_name`], so
//! renaming the whole surface is a single edit here rather than a hunt through
//! five files for string literals that happen to start the same way. The
//! prefix is `ds_` (#555): short to type, and nothing in the engine, the DoD
//! client or HLAE has a name starting with it.
//!
//! The prefix used to be `dodstudio_` ([`OLD_PREFIX`]). The old names were
//! dropped without aliases; only DoD Studio's own files still read them
//! (`studio_panel`'s saved settings and layouts).
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
//! add_commands(&[console_name!("deathmsg"), console_name!("killfeed")], command);
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
/// `spec_` is the first name to follow the layout the rest are moving to:
/// the word after the prefix is the area a command belongs to, as HLAE's
/// `mirv_movie_*` and `mirv_fx_*` are, and the action comes after it. `spec`
/// is the game's own word for the spectator (`spec_mode`, `spec_menu`).
pub(crate) const SPEC_MATCH_POV: &str = console_name!("spec_match_pov");

/// The prefix every name had before #555. Read, never registered: a name in
/// one of our own files (the saved settings, a layout's button or check box) that still
/// carries it is taken as the `ds_` name.
pub(crate) const OLD_PREFIX: &str = "dodstudio_";

/// `name` with [`OLD_PREFIX`] swapped for the current one; any other name as
/// it is.
pub(crate) fn current_name(name: &str) -> std::borrow::Cow<'_, str> {
    match name.strip_prefix(OLD_PREFIX) {
        Some(rest) if !rest.is_empty() => format!("{}{rest}", console_name!("")).into(),
        _ => name.into(),
    }
}

/// A console line with [`current_name`] applied to its first word.
// Only the in-game window's buttons use it, and the window is 32-bit only.
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
pub(crate) fn current_command(line: &str) -> std::borrow::Cow<'_, str> {
    let line = line.trim_start();
    let (word, rest) = line.split_at(line.find(char::is_whitespace).unwrap_or(line.len()));
    match current_name(word) {
        std::borrow::Cow::Owned(word) => format!("{word}{rest}").into(),
        std::borrow::Cow::Borrowed(_) => line.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pre_555_name_reads_as_the_ds_name() {
        assert_eq!(current_name("dodstudio_seek_by"), "ds_seek_by");
        assert_eq!(current_name("ds_seek_by"), "ds_seek_by");
        assert_eq!(current_name("dodstudio_"), "dodstudio_");
        assert_eq!(current_name("hud_draw"), "hud_draw");
        assert_eq!(current_command("dodstudio_seek_by -10"), "ds_seek_by -10");
        assert_eq!(current_command("  dodstudio_panel"), "ds_panel");
        assert_eq!(current_command("ds_seek_by -10"), "ds_seek_by -10");
        assert_eq!(
            current_command("echo dodstudio_panel"),
            "echo dodstudio_panel"
        );
        assert_eq!(current_command(""), "");
    }
}
