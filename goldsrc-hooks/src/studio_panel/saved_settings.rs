//! What the window keeps between launches: the Settings tab's changes
//! (`settings.cfg`) and the layout files it writes (`defaults.txt`).

use std::ffi::CStr;
use std::sync::atomic::Ordering;

use super::*;

/// One line of the saved settings file: `name value`, `name` a plain cvar
/// name and `value` a number or one plain word (an HD style, #899), so the
/// file can never run anything else. A
/// name a DLL from before #555 saved (`dodstudio_...`) reads as the name
/// that replaced it: the file is ours, so it moves to the new names rather than losing
/// the user's settings.
fn settings_line(line: &str) -> Option<(std::borrow::Cow<'_, str>, &str)> {
    let (name, value) = line.trim().split_once(' ')?;
    let value = value.trim();
    let word = !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    (bound_cvar(&format!("{CVAR_BOX_PREFIX}{name}")).is_some()
        && (value.parse::<f64>().is_ok() || word))
        .then(|| (crate::names::current_name(name), value))
}

/// The saved settings, `name value` per line, with `name` set to `value`.
fn settings_with(text: &str, name: &str, value: &str) -> String {
    let mut lines: Vec<String> = text
        .lines()
        .filter_map(settings_line)
        .filter(|(n, _)| n != name)
        .map(|(n, v)| format!("{n} {v}"))
        .collect();
    lines.push(format!("{name} {value}"));
    lines.join("\n") + "\n"
}

/// The saved settings with every pre-#555 name moved to its new name, or
/// `None` when no line has one, so the file is rewritten once at most.
fn migrated_settings(text: &str) -> Option<String> {
    text.lines()
        .any(|l| {
            settings_line(l).is_some_and(|(name, _)| !l.trim_start().starts_with(name.as_ref()))
        })
        .then(|| {
            text.lines()
                .filter_map(settings_line)
                .map(|(n, v)| format!("{n} {v}\n"))
                .collect()
        })
}

/// Where Settings-tab changes are kept between launches: our own file, never
/// the user's `config.cfg`.
fn settings_path() -> std::path::PathBuf {
    res_dir().join("settings.cfg")
}

/// Each cvar's value before the saved settings were applied, so Reset can
/// put it back.
static DEFAULTS: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());
static SETTINGS_APPLIED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Applies the saved settings once, as soon as the engine can run commands
/// and our cvars exist, noting each cvar's value first.
pub(super) fn apply_saved_settings() {
    if SETTINGS_APPLIED.load(Ordering::Relaxed) {
        return;
    }
    let Some(engfuncs) = crate::engine::engfuncs() else {
        return;
    };
    // The cvars are registered with the rest of the hook's surface; wait
    // until ours is there.
    let Ok(probe) = std::ffi::CString::new(VIEWDEMO_NAME) else {
        return;
    };
    if unsafe { (engfuncs.pfn_get_cvar_pointer)(probe.as_ptr()) }.is_null() {
        return;
    }
    SETTINGS_APPLIED.store(true, Ordering::Relaxed);
    let Ok(text) = std::fs::read_to_string(settings_path()) else {
        return;
    };
    if let Some(migrated) = migrated_settings(&text) {
        let done = std::fs::write(settings_path(), migrated).is_ok();
        unsafe {
            crate::debug::report(&format!(
                "studio_panel: {} the pre-#555 {}* names in {} to {}*",
                if done { "moved" } else { "could not move" },
                crate::names::OLD_PREFIX,
                settings_path().display(),
                console_name!("")
            ))
        };
    }
    let mut defaults = DEFAULTS.lock().unwrap_or_else(|e| e.into_inner());
    let mut applied = Vec::new();
    for (name, value) in text.lines().filter_map(settings_line) {
        let Ok(c_name) = std::ffi::CString::new(name.as_ref()) else {
            continue;
        };
        if !defaults.iter().any(|(n, _)| *n == name)
            && let Some(before) = cvar_text(&c_name)
        {
            defaults.push((name.to_string(), before));
        }
        if let Ok(line) = std::ffi::CString::new(format!("{name} {value}\n")) {
            crate::engine::client_cmd(&line);
        }
        applied.push(format!("{name} {value}"));
    }
    if !applied.is_empty() {
        unsafe {
            crate::debug::report(&format!(
                "studio_panel: applied saved settings from {}: {}",
                settings_path().display(),
                applied.join(", ")
            ))
        };
    }
}

/// A cvar's value as text (an HD style is a word, not a number), or `None`
/// while it doesn't exist or isn't one [`settings_line`] would keep.
pub(super) fn cvar_text(name: &CStr) -> Option<String> {
    let engfuncs = crate::engine::engfuncs()?;
    // Safety: the engine's own cvar record, kept for the session; its
    // `string` is a valid C string while the cvar exists.
    let text = unsafe {
        let cvar = (engfuncs.pfn_get_cvar_pointer)(name.as_ptr());
        if cvar.is_null() || (*cvar).string.is_null() {
            return None;
        }
        CStr::from_ptr((*cvar).string)
            .to_string_lossy()
            .trim()
            .to_string()
    };
    settings_line(&format!("{} {text}", name.to_string_lossy()))
        .is_some()
        .then_some(text)
}

/// Remembers a Settings-tab change for the next launch. `default_before` is
/// the value before DoD Studio first changed it, for Reset.
pub(super) fn save_setting(name: &str, value: &str, default_before: Option<String>) {
    if let Some(before) = default_before {
        let mut defaults = DEFAULTS.lock().unwrap_or_else(|e| e.into_inner());
        if !defaults.iter().any(|(n, _)| n == name) {
            defaults.push((name.to_string(), before));
        }
    }
    let path = settings_path();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let _ = std::fs::create_dir_all(res_dir());
    let saved = std::fs::write(&path, settings_with(&text, name, value)).is_ok();
    unsafe {
        crate::debug::report(&format!(
            "studio_panel: Settings tab set {name} {value}, {} {}",
            if saved { "saved to" } else { "could not save" },
            path.display()
        ))
    };
}

/// Puts every saved setting back to its value before DoD Studio changed it,
/// and forgets them.
pub(super) fn reset_settings() -> String {
    let defaults = std::mem::take(&mut *DEFAULTS.lock().unwrap_or_else(|e| e.into_inner()));
    for (name, value) in &defaults {
        if let Ok(line) = std::ffi::CString::new(format!("{name} {value}\n")) {
            crate::engine::client_cmd(&line);
        }
    }
    let _ = std::fs::remove_file(settings_path());
    format!("{} setting(s) back to their defaults", defaults.len())
}

/// What each layout file held when this DLL (or an earlier one) wrote it:
/// `<file> <fnv1a>` per line, in `dodstudio_ui/defaults.txt`.
const DEFAULTS_FILE: &str = "defaults.txt";

fn fnv1a(text: &str) -> u32 {
    text.bytes()
        .filter(|&b| b != b'\r')
        .fold(0x811c_9dc5u32, |h, b| {
            (h ^ b as u32).wrapping_mul(0x0100_0193)
        })
}

/// Which layouts to write, given what is on disk and what was written there:
/// a missing file; on `reset`, every file; and a file still exactly as an
/// earlier DLL wrote it when this one's default differs (a new control).
/// A file edited since (build mode) is the user's and left alone.
fn res_to_write(
    reset: bool,
    on_disk: Option<&str>,
    written_before: Option<u32>,
    default: &str,
) -> bool {
    match on_disk {
        None => true,
        Some(_) if reset => true,
        Some(text) => fnv1a(text) != fnv1a(default) && written_before == Some(fnv1a(text)),
    }
}

/// Writes each default layout that is missing, still as an earlier DLL wrote
/// it but changed since, or (on `reset`) all of them, and says what it wrote.
/// Never touches a file edited since: build-mode edits are the user's.
pub(super) fn ensure_res(reset: bool) -> Result<Option<String>, String> {
    let dir = res_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let stamps_path = dir.join(DEFAULTS_FILE);
    let mut stamps: Vec<(String, u32)> = std::fs::read_to_string(&stamps_path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let (file, hash) = l.split_once(' ')?;
            Some((file.to_string(), u32::from_str_radix(hash.trim(), 16).ok()?))
        })
        .collect();
    let mut written = Vec::new();
    let layouts = [WINDOW_RES, FORM_RES]
        .into_iter()
        .chain(PAGES.iter().map(|p| p.res));
    for (_, file, default) in layouts {
        let path = dir.join(file);
        let on_disk = std::fs::read_to_string(&path).ok();
        let before = stamps.iter().find(|(f, _)| f == file).map(|(_, h)| *h);
        let write = res_to_write(reset, on_disk.as_deref(), before, default);
        if write {
            std::fs::write(&path, default)
                .map_err(|e| format!("could not write {}: {e}", path.display()))?;
            written.push(file);
        }
        // Note what this file holds when it is ours: just written, or still
        // the default this DLL would write.
        if write
            || on_disk
                .as_deref()
                .is_some_and(|t| fnv1a(t) == fnv1a(default))
        {
            stamps.retain(|(f, _)| f != file);
            stamps.push((file.to_string(), fnv1a(default)));
        }
    }
    let text: String = stamps
        .iter()
        .map(|(f, h)| format!("{f} {h:08x}\n"))
        .collect();
    let _ = std::fs::write(&stamps_path, text);
    Ok((!written.is_empty()).then(|| format!("wrote {} to {}", written.join(", "), dir.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_file_only_holds_cvar_number_lines() {
        assert_eq!(settings_line("hud_draw 0"), Some(("hud_draw".into(), "0")));
        assert_eq!(settings_line("hud_draw 0;quit"), None);
        assert_eq!(settings_line("quit"), None);
        let text = settings_with("hud_draw 0\njunk\nr_drawviewmodel 1\n", "hud_draw", "1");
        assert_eq!(text, "r_drawviewmodel 1\nhud_draw 1\n");
    }

    /// #555 renamed every name. The settings file is ours, so a line an older
    /// DLL saved still applies, under the new name, and is written back that
    /// way.
    #[test]
    fn saved_settings_with_the_old_prefix_read_as_the_new_names() {
        let old = format!("{}hide_scoreboard 1", crate::names::OLD_PREFIX);
        assert_eq!(
            settings_line(&old),
            Some((console_name!("hud_hide_scoreboard").into(), "1"))
        );
        assert_eq!(
            settings_line(&format!("{}spec_lock 1;quit", crate::names::OLD_PREFIX)),
            None
        );
        // The bare old prefix is no name at all, and other names pass as is.
        assert_eq!(
            settings_line(&format!("{} 1", crate::names::OLD_PREFIX)),
            Some((crate::names::OLD_PREFIX.into(), "1"))
        );
        assert_eq!(settings_line("hud_draw 0"), Some(("hud_draw".into(), "0")));

        let file = format!(
            "{old}\nhud_draw 0\n{}viewdemo_in_panel 0\njunk\n",
            crate::names::OLD_PREFIX
        );
        assert_eq!(
            migrated_settings(&file).as_deref(),
            Some("ds_hud_hide_scoreboard 1\nhud_draw 0\nds_ui_viewdemo_in_panel 0\n")
        );
        assert_eq!(
            migrated_settings("ds_hud_hide_scoreboard 1\nhud_draw 0\n"),
            None
        );

        // Saving a setting over an old line replaces it instead of keeping both.
        assert_eq!(
            settings_with(&file, console_name!("hud_hide_scoreboard"), "0"),
            "hud_draw 0\nds_ui_viewdemo_in_panel 0\nds_hud_hide_scoreboard 0\n"
        );
    }

    #[test]
    fn a_layout_is_rewritten_only_while_it_is_still_ours() {
        let (old, new, edited) = ("a 1", "a 2", "a 1 moved");
        // Missing: written.
        assert!(res_to_write(false, None, None, new));
        // As an earlier DLL wrote it, and the default changed: upgraded.
        assert!(res_to_write(false, Some(old), Some(fnv1a(old)), new));
        // Edited in build mode since: left alone.
        assert!(!res_to_write(false, Some(edited), Some(fnv1a(old)), new));
        // Never stamped (written before stamps existed): left alone.
        assert!(!res_to_write(false, Some(old), None, new));
        // Already the default: nothing to do.
        assert!(!res_to_write(false, Some(new), Some(fnv1a(new)), new));
        // Reset writes regardless.
        assert!(res_to_write(true, Some(edited), None, new));
        // Line endings don't count as an edit.
        assert_eq!(fnv1a("a\r\nb"), fnv1a("a\nb"));
    }
}
