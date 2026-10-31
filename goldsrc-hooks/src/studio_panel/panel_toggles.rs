//! The ways into the window: `viewdemo` (`ds_ui_viewdemo_in_panel`), the
//! console key (`ds_ui_console_in_panel`) and DoD Studio's main menu.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::setting::Setting;

/// How many frames after `viewdemo` to wait for the VCR bar to appear.
const VIEWDEMO_WAIT_FRAMES: u32 = 600;

/// `ds_ui_viewdemo_in_panel 1`: `viewdemo` opens our window on the
/// Playback tab and parks the VCR bar off screen. Not `ds_ui_panel_...`:
/// no name may be the start of another (the console's autocomplete).
pub const VIEWDEMO_NAME: &str = console_name!("ui_viewdemo_in_panel");
/// The cvar, or its default (on) when it could not be registered.
pub static VIEWDEMO_IN_PANEL: Setting = Setting::new(VIEWDEMO_NAME, true);
/// Frames left to wait for the VCR bar after a `viewdemo`, or 0.
pub(super) static VIEWDEMO_PENDING: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

/// `ds_ui_console_in_panel 1`: the console key (`toggleconsole`) opens our
/// window on its Console tab instead of the console window.
pub const CONSOLE_NAME: &str = console_name!("ui_console_in_panel");
/// The cvar, or its default (on) when it could not be registered.
pub static CONSOLE_IN_PANEL: Setting = Setting::new(CONSOLE_NAME, true);
/// Frames left to wait for the console window after `toggleconsole`, or 0.
pub(super) static CONSOLE_PENDING: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);
/// The engine's own `toggleconsole`, once wrapped.
pub(super) static REAL_TOGGLECONSOLE: AtomicUsize = AtomicUsize::new(0);
static TOGGLECONSOLE_WRAPPED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(super) fn console_in_panel() -> bool {
    CONSOLE_IN_PANEL.on()
}

/// Wraps `toggleconsole` (the console key's binding), once the engine has it.
pub(super) fn wrap_toggleconsole() {
    if TOGGLECONSOLE_WRAPPED.load(Ordering::Relaxed) {
        return;
    }
    let mut found = false;
    crate::cmd_list::for_each(|name, entry| {
        if name.eq_ignore_ascii_case(b"toggleconsole") {
            found |= crate::cmd_list::wrap(entry, &REAL_TOGGLECONSOLE, wrapped_toggleconsole);
        }
    });
    if found {
        TOGGLECONSOLE_WRAPPED.store(true, Ordering::Relaxed);
        unsafe {
            crate::debug::report("studio_panel: wrapped toggleconsole for ds_ui_console_in_panel")
        };
    }
}

/// The engine's own `toggleconsole` handler (`Con_ToggleConsole_f`), whether
/// or not [`wrap_toggleconsole`] has swapped it for ours yet. None until the
/// engine's command list is reachable.
pub fn engine_toggleconsole() -> Option<usize> {
    let mut handler = None;
    crate::cmd_list::for_each(|name, entry| {
        if name.eq_ignore_ascii_case(b"toggleconsole") {
            handler = entry.function.map(|f| {
                let ours: crate::engine::ConsoleCommandFn = wrapped_toggleconsole;
                if f as usize == ours as usize {
                    REAL_TOGGLECONSOLE.load(Ordering::Relaxed)
                } else {
                    f as usize
                }
            });
        }
    });
    handler.filter(|&address| address != 0)
}

/// The console key. With `ds_ui_console_in_panel 1` it goes back to the
/// game when the Console tab is showing, leaving our window open for the next
/// ESC; otherwise it runs the engine's own `toggleconsole` (which also brings
/// up the menu when in game) and [`poll`] then swaps the console window for
/// our Console tab.
unsafe extern "C" fn wrapped_toggleconsole() {
    if console_in_panel() {
        #[cfg(target_arch = "x86")]
        if hook::back_to_game_if_on_console() {
            // The game closes its console window its own way, which also
            // closes the menu. That also keeps ESC on the main menu from
            // bringing the stock console back.
            unsafe {
                crate::debug::report("studio_panel: the console key went back to the game");
                crate::cmd_list::call_real(&REAL_TOGGLECONSOLE);
            }
            return;
        }
        CONSOLE_PENDING.store(VIEWDEMO_WAIT_FRAMES, Ordering::Release);
    }
    unsafe { crate::cmd_list::call_real(&REAL_TOGGLECONSOLE) };
}

pub(crate) fn viewdemo_in_panel() -> bool {
    VIEWDEMO_IN_PANEL.on()
}

/// Called by `demo_reload`'s `viewdemo` wrapper for a bare `viewdemo`, which
/// on its own only prints its usage. With `ds_ui_viewdemo_in_panel 1` it
/// opens our window on the Playback tab instead, as `viewdemo` opens the VCR
/// bar, and returns whether it did.
pub fn bare_viewdemo() -> bool {
    if !viewdemo_in_panel() {
        return false;
    }
    #[cfg(target_arch = "x86")]
    {
        let line = match hook::open_on(PLAYBACK_PAGE, false) {
            Ok(state) => format!("{NAME}: viewdemo opened the window -- {state}"),
            Err(why) => format!("{NAME}: viewdemo could not open the window -- {why}"),
        };
        crate::commands::console_print(&format!("{line}\n"));
        true
    }
    #[cfg(not(target_arch = "x86"))]
    false
}

/// The main menu DoD Studio writes, in `dod_addon` (read only with
/// `-addons`, #412, and over the game's own `dod\resource` one, which is the
/// user's and never written). "DoD Studio" opens our window; in a game or a
/// demo the menu also has Resume and Disconnect; Options and Quit stay.
const GAME_MENU: &str = include_str!("../../ui/GameMenu.res");
/// What marks a `dod_addon` menu as DoD Studio's own, to be kept up to date;
/// any other file there is left alone.
const GAME_MENU_MARK: &str = "DoD Studio";

/// What to do with `dod_addon\resource\GameMenu.res`, given what is there.
fn game_menu_action(existing: Option<&str>) -> Option<&'static str> {
    match existing {
        None => Some("wrote"),
        Some(text) if text == GAME_MENU => None,
        Some(text) if text.contains(GAME_MENU_MARK) => Some("updated"),
        Some(_) => None,
    }
}

/// Writes DoD Studio's main menu into `dod_addon`, unless a menu that isn't
/// ours is already there. Runs at load, before GameUI reads the menu.
/// `GOLDSRC_HOOKS_GAME_MENU=0` turns it off.
pub fn write_game_menu() {
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
    else {
        return;
    };
    // `<game folder>_addon`, which `-addons` puts on the engine's search
    // path for whatever `-game` names (#402).
    let path = dir
        .join(crate::game_folder::addon_name())
        .join("resource")
        .join("GameMenu.res");
    let existing = std::fs::read_to_string(&path).ok();
    let Some(verb) = game_menu_action(existing.as_deref()) else {
        if existing.is_some_and(|t| !t.contains(GAME_MENU_MARK)) {
            unsafe {
                crate::debug::report(&format!(
                    "studio_panel: {} is not DoD Studio's, left alone",
                    path.display()
                ))
            };
        }
        return;
    };
    let written = path
        .parent()
        .is_some_and(|p| std::fs::create_dir_all(p).is_ok())
        && std::fs::write(&path, GAME_MENU).is_ok();
    unsafe {
        crate::debug::report(&format!(
            "studio_panel: {} the DoD Studio main menu at {}{}",
            if written { verb } else { "could not write" },
            path.display(),
            if written {
                " (shown when the game is launched with -addons)"
            } else {
                ""
            }
        ))
    };
}

/// Called by `demo_reload`'s `viewdemo` wrapper after the engine's own
/// `viewdemo` ran: the bar appears a few frames later, and [`poll`] opens our
/// window then.
pub fn after_viewdemo() {
    if viewdemo_in_panel() {
        VIEWDEMO_PENDING.store(VIEWDEMO_WAIT_FRAMES, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_main_menu_is_written_only_over_our_own() {
        assert_eq!(game_menu_action(None), Some("wrote"));
        assert_eq!(game_menu_action(Some(GAME_MENU)), None);
        assert_eq!(
            game_menu_action(Some("// DoD Studio old menu")),
            Some("updated")
        );
        assert_eq!(game_menu_action(Some("\"GameMenu\" { }")), None);
        assert!(GAME_MENU.contains(GAME_MENU_MARK));
        assert!(GAME_MENU.contains("engine ds_ui_panel 1"));
    }
}
