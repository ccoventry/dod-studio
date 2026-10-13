//! The hook DLL's console prefix, and the names it had before #555.
//!
//! #555 renamed every `dodstudio_*` cvar and command the hook registers to
//! `ds_*`, keeping the rest of each name. The game gets no aliases, so an old
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

/// Every name the hook registered when the prefix changed, without the prefix.
/// Frozen, plus the ones that shipped on `dev` with the old prefix while #555
/// was open (`batch` #702, `debug_keep_detail_cache` #718, `show_demo_scope`
/// #721, `hide_headshot_red_screen` and `hide_screen_shake` #606,
/// `stay_behind_while_capturing` #434); a name added since never had the old
/// prefix. The rest of each name
/// did not change, so the new name is [`PREFIX`] plus the same suffix.
pub const RENAMED: &[&str] = &[
    "allow_shaders",
    "batch",
    "clear_decals",
    "console_in_panel",
    "deathmsg",
    "debug_hd_misses",
    "debug_keep_detail_cache",
    "debug_log_spectator_target",
    "debug_log_texture_loads",
    "debug_log_weapon_model",
    "debug_msglog",
    "debug_position",
    "debug_status",
    "demo_list_count_subfolders",
    "demo_list_folders",
    "demo_list_hide_empty",
    "ex_interp_max",
    "hd_enabled",
    "hd_style",
    "hide_asset",
    "hide_clan_text",
    "hide_crosshair",
    "hide_hand_signals",
    "hide_headshot_red_screen",
    "hide_hltv_messages",
    "hide_hudelement",
    "hide_map_text",
    "hide_scoreboard",
    "hide_screen_shake",
    "hide_spectator_bars",
    "hud_map_players",
    "hud_map_team_marker",
    "mute_voice_commands",
    "objectives",
    "overviewmap",
    "panel",
    "reload_demo",
    "remember_window_layout",
    "resizable_windows",
    "review",
    "run_in_background",
    "schedule",
    "seek_by",
    "seek_skip_between",
    "seek_to",
    "show_demo_scope",
    "spec_lock",
    "spec_match_pov",
    "spec_target",
    "stay_behind_while_capturing",
    "viewdemo_in_panel",
];

/// The current name for a pre-#555 one (`dodstudio_hide_scoreboard` gives
/// `ds_hide_scoreboard`), or `None` for any other word. Case does not matter,
/// as it does not to the console; a folder or file that shares the old prefix
/// (`dodstudio_hd`, `dodstudio_helper`) is no console name and gives `None`.
pub fn current_name(word: &str) -> Option<String> {
    let head = word.get(..OLD_PREFIX.len())?;
    if !head.eq_ignore_ascii_case(OLD_PREFIX) {
        return None;
    }
    let suffix = word[OLD_PREFIX.len()..].to_ascii_lowercase();
    RENAMED
        .contains(&suffix.as_str())
        .then(|| format!("{PREFIX}{suffix}"))
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

    #[test]
    fn an_old_name_becomes_the_same_name_with_the_new_prefix() {
        assert_eq!(
            current_name("dodstudio_hide_scoreboard").as_deref(),
            Some("ds_hide_scoreboard")
        );
        assert_eq!(
            current_name("DodStudio_Seek_To").as_deref(),
            Some("ds_seek_to")
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
                    "ds_clear_decals".to_string()
                ),
                ("dodstudio_seek_by".to_string(), "ds_seek_by".to_string()),
            ]
        );
        assert!(old_names_in("exec dodstudio_helper.cfg").is_empty());
        assert!(old_names_in("ds_hide_scoreboard 1").is_empty());
        assert!(old_names_in("xdodstudio_hide_scoreboard 1").is_empty());
    }

    #[test]
    fn a_command_is_migrated_word_by_word() {
        assert_eq!(
            migrated("dodstudio_hide_scoreboard 1").as_deref(),
            Some("ds_hide_scoreboard 1")
        );
        assert_eq!(
            migrated("dodstudio_deathmsg block !76561197962734863;dodstudio_spec_lock 1")
                .as_deref(),
            Some("ds_deathmsg block !76561197962734863;ds_spec_lock 1")
        );
        assert_eq!(migrated("mirv_fov 90"), None);
        assert_eq!(migrated("ds_spec_lock 1"), None);
        assert_eq!(migrated("echo dodstudio_hd/world"), None);
    }

    /// The new names have to be ones the hook registers, or a migrated
    /// setting and a config warning would both point at a name that does
    /// nothing. The hook is a cdylib, so its sources are read instead. A
    /// later rename (#555's area layout) fails here: then this list becomes
    /// old -> new pairs.
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
        for suffix in RENAMED {
            assert!(
                code.contains(&format!("console_name!(\"{suffix}\")")),
                "{PREFIX}{suffix} is not registered by goldsrc-hooks"
            );
        }
    }
}
