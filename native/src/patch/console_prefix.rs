//! The hook DLL's console prefix, and the names it had before #555.
//!
//! #555 renamed every `dodstudio_*` cvar and command the hook registers: the
//! prefix became `ds_` and the area a name belongs to comes second
//! (`dodstudio_hide_scoreboard` is `ds_hud_hide_scoreboard`). The old -> new
//! table is the hook's own `old_names.rs`, compiled in here as a module so
//! both sides read one list. The game gets no aliases, so an old
//! name typed in the console, or left in a config, does nothing. What the app
//! can do about that lives here:
//!
//! - Studio's own settings move to the new names when they load
//!   (`settings_manager`), since that file is the app's.
//! - The game's `.cfg` files are the user's and are never written:
//!   `cfg_scan` reports a line that still uses an old name, and which name
//!   replaces it.

/// The prefix of every console name the hook registers (`goldsrc-hooks`'
/// `names.rs`).
pub const PREFIX: &str = "ds_";

/// The prefix before #555.
pub const OLD_PREFIX: &str = "dodstudio_";

#[path = "../../../goldsrc-hooks/src/old_names.rs"]
mod old_names;
pub use old_names::OLD_NAMES;

/// The name that replaced a pre-#555 one (`dodstudio_hide_scoreboard` gives
/// `ds_hud_hide_scoreboard`), or `None` for any other word. Case does not
/// matter, as it does not to the console; a folder or file that shares the old
/// prefix (`dodstudio_hd`, `dodstudio_helper`) is no console name and gives
/// `None`.
pub fn current_name(word: &str) -> Option<String> {
    OLD_NAMES
        .iter()
        .find(|(old, _)| old.eq_ignore_ascii_case(word))
        .map(|(_, new)| (*new).to_string())
}

/// Each word of `text` (a run of letters, digits and `_`) with its byte range.
fn words(text: &str) -> Vec<(usize, &str)> {
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut start = None;
    let mut out = Vec::new();
    for (i, c) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        match (start, is_word(c)) {
            (None, true) => start = Some(i),
            (Some(s), false) => {
                out.push((s, &text[s..i]));
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// Every pre-#555 name in `text`, as (the old name as written, its current
/// name), first use first and each once. A whole word only, so a bind's or
/// an alias's quoted payload counts too: it fails just the same when the key
/// is pressed.
pub fn old_names_in(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (_, word) in words(text) {
        if let Some(new) = current_name(word)
            && !out.iter().any(|(old, _)| old == word)
        {
            out.push((word.to_string(), new));
        }
    }
    out
}

/// `command` with every pre-#555 name swapped for its current one, or `None`
/// when it has none. Only whole words the hook registered change, so a path
/// such as `dodstudio_hd/world` is left as it is.
pub fn migrated(command: &str) -> Option<String> {
    let mut out = String::with_capacity(command.len());
    let mut copied = 0;
    for (start, word) in words(command) {
        if let Some(new) = current_name(word) {
            out.push_str(&command[copied..start]);
            out.push_str(&new);
            copied = start + word.len();
        }
    }
    (copied > 0).then(|| {
        out.push_str(&command[copied..]);
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Names the hook registers that never had a `dodstudio_` one.
    const NEW_SINCE_555: &[&str] = &[
        "debug_log_gun_lowered",
        "debug_log_server_sounds",
        "debug_log_view_bob",
        "hud_hide_center_text",
        "hud_hide_server_text",
        "hud_overview_hide_speaker",
        "player_change_name",
        "player_fix_model_flicker",
        "sound_mute_flag_capture",
        "sound_mute_victory_music",
        "sound_restore_gunshots",
        "spec_hide_text",
        "view_show_own_blood",
    ];

    #[test]
    fn an_old_name_becomes_the_name_in_the_table() {
        assert_eq!(
            current_name("dodstudio_hide_scoreboard").as_deref(),
            Some("ds_hud_hide_scoreboard")
        );
        assert_eq!(
            current_name("DodStudio_Seek_To").as_deref(),
            Some("ds_demo_seek_to")
        );
        assert_eq!(
            current_name("dodstudio_pov_live_bob").as_deref(),
            Some("ds_view_use_cl_bob")
        );
        // Not console names: folders, files, the bare prefix, unknown names,
        // names already moved.
        for word in [
            "dodstudio_hd",
            "dodstudio_ui",
            "dodstudio_helper",
            "dodstudio_primer",
            "dodstudio_chain_01",
            "dodstudio_",
            "dodstudio_hide_sprite",
            "ds_hide_scoreboard",
            "ds_hud_hide_scoreboard",
            "hide_scoreboard",
            "",
        ] {
            assert_eq!(current_name(word), None, "{word}");
        }
    }

    #[test]
    fn old_names_are_found_as_whole_words_anywhere_on_a_line() {
        assert_eq!(
            old_names_in(
                "bind F7 \"dodstudio_clear_decals; dodstudio_seek_by -10\"; dodstudio_clear_decals"
            ),
            vec![
                (
                    "dodstudio_clear_decals".to_string(),
                    "ds_map_clear_decals".to_string()
                ),
                (
                    "dodstudio_seek_by".to_string(),
                    "ds_demo_seek_by".to_string()
                ),
            ]
        );
        assert!(old_names_in("exec dodstudio_helper.cfg").is_empty());
        assert!(old_names_in("ds_hud_hide_scoreboard 1").is_empty());
        assert!(old_names_in("xdodstudio_hide_scoreboard 1").is_empty());
    }

    #[test]
    fn a_command_is_migrated_word_by_word() {
        assert_eq!(
            migrated("dodstudio_hide_scoreboard 1").as_deref(),
            Some("ds_hud_hide_scoreboard 1")
        );
        assert_eq!(
            migrated("dodstudio_deathmsg block !76561197962734863;dodstudio_spec_lock 1")
                .as_deref(),
            Some("ds_hud_deathmsg block !76561197962734863;ds_spec_lock 1")
        );
        assert_eq!(migrated("mirv_fov 90"), None);
        assert_eq!(migrated("ds_spec_lock 1"), None);
        assert_eq!(migrated("echo dodstudio_hd/world"), None);
    }

    /// The new names have to be ones the hook registers, or a migrated
    /// setting and a config warning would both point at a name that does
    /// nothing. The hook is a cdylib, so its sources are read instead. And
    /// every name the hook registers is in the table, so none was missed.
    #[test]
    fn every_new_name_is_one_the_hook_registers() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../goldsrc-hooks/src");
        let mut code = String::new();
        let mut dirs = vec![src];
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
        assert!(
            code.contains(&format!("concat!(\"{PREFIX}\", $suffix)")),
            "names.rs no longer uses {PREFIX}"
        );
        for (old, new) in OLD_NAMES {
            assert!(old.starts_with(OLD_PREFIX), "{old}");
            let suffix = new.strip_prefix(PREFIX).unwrap();
            assert!(
                code.contains(&format!("console_name!(\"{suffix}\")")),
                "{old} -> {new}: {new} is not registered by goldsrc-hooks"
            );
        }
        // Every other suffix but the two doc examples in names.rs, and
        // comments that write `console_name!("...")`.
        let mut registered: Vec<&str> = code
            .split("console_name!(\"")
            .skip(1)
            .filter_map(|s| s.split('"').next())
            .filter(|s| {
                !s.is_empty()
                    && *s != "status"
                    && *s != "killfeed"
                    && s.bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            })
            .collect();
        registered.sort_unstable();
        registered.dedup();
        for suffix in registered {
            let name = format!("{PREFIX}{suffix}");
            assert!(
                OLD_NAMES.iter().any(|(_, new)| *new == name) || NEW_SINCE_555.contains(&suffix),
                "{name} has no old name in old_names.rs; a name added since #555 goes in NEW_SINCE_555"
            );
        }
    }
}
