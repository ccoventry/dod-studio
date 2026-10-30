//! The plain-command fallback.
//!
//! If `pfnRegisterVariable` does not hand back a cvar whose own `name` matches
//! what was asked for, the layout assumption is wrong and every subsequent read
//! would be garbage. That case registers the old commands instead and says so
//! loudly, so a bad assumption costs the type-ahead rather than the session.
//!
//! Only reached when cvar registration could not be trusted. Kept whole rather
//! than degraded, because the alternative is a session with no way to turn
//! either fix on.

use std::ffi::{CStr, c_char};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use super::*;
use crate::engine;
use crate::{
    anim_fix, crosshair, demo_list_folders, demo_other_drives, demo_seek, hand_signals,
    missing_shots, scoreboard, spectator_crosshair, spectator_eye, spectator_target, texture_hires,
    voice, window_layout,
};

/// Reads argv(1) (if present) as "0"/"1" and stores it into `flag`, then prints
/// the resulting state.
fn handle_toggle(name: &str, flag: &AtomicBool, status: fn() -> String) {
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };

    // Cmd_Argc counts the command name itself, so a bare invocation is 1 and an
    // argument makes it 2. Bare is a query, not a no-op.
    let argc = unsafe { (engfuncs.cmd_argc)() };
    let mut assigned = false;
    if argc >= 2 {
        let arg1 = unsafe { (engfuncs.cmd_argv)(1) };
        if !arg1.is_null() {
            let value = unsafe { CStr::from_ptr(arg1 as *const c_char) }.to_string_lossy();
            match value.trim() {
                "0" => {
                    flag.store(false, Ordering::Relaxed);
                    assigned = true;
                }
                "1" => {
                    flag.store(true, Ordering::Relaxed);
                    assigned = true;
                }
                other => {
                    console_print(&format!("{name}: expected 0 or 1, got \"{other}\"\n"));
                    unsafe {
                        crate::debug::report(&format!(
                            "commands: {name} rejected argument \"{other}\""
                        ))
                    };
                    return;
                }
            }
        }
    }

    let state = if flag.load(Ordering::Relaxed) {
        "1 (on)"
    } else {
        "0 (off)"
    };
    if assigned {
        console_print(&format!("{name} = {state}\n"));
    } else {
        console_print(&format!(
            "{name} = {state}\nusage: {name} <0|1>\n{}\n",
            status()
        ));
    }
    unsafe {
        crate::debug::report(&format!(
            "commands: {name} = {state} ({}, argc={argc})",
            if assigned {
                "set"
            } else {
                "queried, unchanged"
            }
        ))
    };
}

/// `ds_spec_match_pov` as a plain command. With no cvar for `poll` to copy
/// from, the handler sets every part itself.
pub(super) unsafe extern "C" fn cmd_spec_match_pov() {
    handle_level(SPEC_MATCH_POV_NAME, &anim_fix::LEVEL, anim_fix::status);
    let on = anim_fix::enabled();
    if let Err(why) = spectator_crosshair::set_matching(on) {
        console_print(&format!(
            "{SPEC_MATCH_POV_NAME}: the spectator crosshair was not changed -- {why}\n"
        ));
    }
    if let Err(why) = spectator_eye::set_matching(on) {
        console_print(&format!(
            "{SPEC_MATCH_POV_NAME}: the in-eye camera's height was not changed -- {why}\n"
        ));
    }
}

/// `handle_toggle` for a setting `anim_fix::LEVEL` holds, which is a number
/// rather than a flag. Only reached on the fallback path, when cvar
/// registration failed -- the cvar itself is what normally carries this.
fn handle_level(name: &str, level: &AtomicI32, status: fn() -> String) {
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };

    let argc = unsafe { (engfuncs.cmd_argc)() };
    let mut assigned = false;
    if argc >= 2 {
        let arg1 = unsafe { (engfuncs.cmd_argv)(1) };
        if !arg1.is_null() {
            let value = unsafe { CStr::from_ptr(arg1 as *const c_char) }.to_string_lossy();
            match value.trim().parse::<i32>() {
                Ok(n) => {
                    level.store(
                        n.clamp(anim_fix::LEVEL_OFF, anim_fix::LEVEL_MAX),
                        Ordering::Relaxed,
                    );
                    assigned = true;
                }
                Err(_) => {
                    let max = anim_fix::LEVEL_MAX;
                    console_print(&format!(
                        "{name}: expected 0-{max}, got \"{}\"\n",
                        value.trim()
                    ));
                    unsafe {
                        crate::debug::report(&format!(
                            "commands: {name} rejected argument \"{}\"",
                            value.trim()
                        ))
                    };
                    return;
                }
            }
        }
    }

    let now = anim_fix::level();
    let state = format!("{now} ({})", anim_fix::level_description(now));
    if assigned {
        console_print(&format!("{name} = {state}\n"));
    } else {
        let max = anim_fix::LEVEL_MAX;
        let mut usage = format!("{name} = {state}\nusage: {name} <0-{max}>\n");
        for n in anim_fix::LEVEL_OFF..=max {
            usage.push_str(&format!("  {n}  {}\n", anim_fix::level_description(n)));
        }
        console_print(&format!("{usage}{}\n", status()));
    }
    unsafe {
        crate::debug::report(&format!(
            "commands: {name} = {state} ({}, argc={argc})",
            if assigned {
                "set"
            } else {
                "queried, unchanged"
            }
        ))
    };
}

/// The fallback for the three code-patch cvars. Unlike the other toggles these
/// have to report a failure to the console: `client.dll` may not be loaded, or
/// a signature may not match this build, and silently doing nothing would look
/// exactly like a setting that refuses to take.
fn handle_code_patch(
    name: &str,
    usage: &str,
    apply: fn(bool) -> Result<bool, String>,
    current: fn() -> bool,
    status: fn() -> String,
) {
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };

    if unsafe { (engfuncs.cmd_argc)() } >= 2 {
        let arg1 = unsafe { (engfuncs.cmd_argv)(1) };
        if !arg1.is_null() {
            let raw = unsafe { CStr::from_ptr(arg1 as *const c_char) }
                .to_string_lossy()
                .into_owned();
            let on = match raw.trim() {
                "0" => false,
                "1" => true,
                other => {
                    console_print(&format!("{name}: expected 0 or 1, got \"{other}\"\n"));
                    return;
                }
            };
            let bit = if on { "1" } else { "0" };
            match apply(on) {
                Ok(_) => {
                    console_print(&format!("{name} = {bit}\n"));
                    unsafe { crate::debug::report(&format!("commands: {name} = {bit} (set)")) };
                }
                Err(why) => {
                    console_print(&format!("{name}: {why}\n"));
                    unsafe { crate::debug::report(&format!("commands: {name} failed -- {why}")) };
                }
            }
            return;
        }
    }

    console_print(&format!(
        "{name} = {}\nusage: {name} <0|1>  ({usage})\n{}\n",
        if current() { "1" } else { "0" },
        status()
    ));
}

pub(super) unsafe extern "C" fn cmd_scoreboard() {
    handle_code_patch(
        SCOREBOARD_NAME,
        "1 blocks +showscores",
        scoreboard::set_hidden,
        scoreboard::suppressed,
        scoreboard::status,
    );
}

pub(super) unsafe extern "C" fn cmd_voice() {
    handle_code_patch(
        VOICE_NAME,
        "1 silences voice commands",
        voice::set_muted,
        voice::muted,
        voice::status,
    );
}

pub(super) unsafe extern "C" fn cmd_crosshair() {
    handle_code_patch(
        CROSSHAIR_NAME,
        "1 hides the crosshair",
        crosshair::set_hidden,
        crosshair::hidden,
        crosshair::status,
    );
}

pub(super) unsafe extern "C" fn cmd_hand_signals() {
    handle_toggle(HAND_SIGNALS_NAME, &hand_signals::ENABLED, || {
        hand_signals::status()
    });
}

/// `ds_debug_log_texture_loads` -- verbose per-texture logging for the
/// (opt-in, `GOLDSRC_HOOKS_TEXTURE_HIRES`-gated) world-texture swap: one line
/// per world texture uploaded, saying whether it was replaced and why not.
/// Registering the toggle unconditionally, whether or not the hook is
/// actually installed this session, matches every other cvar here -- setting
/// it when the hook is off just does nothing yet, rather than the command not
/// existing at all.
pub(super) unsafe extern "C" fn cmd_texture_hires_log() {
    handle_toggle(
        TEXTURE_HIRES_LOG_NAME,
        &texture_hires::LOG_TEXTURE_LOADS,
        texture_hires::status,
    );
}

/// Only registered when `ds_ui_demo_list_folders` could not be a cvar.
pub(super) unsafe extern "C" fn cmd_demo_list_folders() {
    handle_toggle(
        demo_list_folders::NAME,
        &demo_list_folders::ENABLED,
        demo_list_folders::status,
    );
}

/// Only registered when `ds_demo_other_drives` could not be a cvar.
pub(super) unsafe extern "C" fn cmd_demo_other_drives() {
    handle_toggle(
        demo_other_drives::NAME,
        &demo_other_drives::ENABLED,
        demo_other_drives::status,
    );
}

/// Only registered when `ds_ui_demo_list_hide_empty` could not be a cvar.
pub(super) unsafe extern "C" fn cmd_demo_list_hide_empty() {
    handle_toggle(
        demo_list_folders::HIDE_EMPTY.name,
        &demo_list_folders::HIDE_EMPTY.fallback,
        demo_list_folders::hide_empty_status,
    );
}

/// Only registered when `ds_ui_demo_list_count_subfolders` could not be
/// a cvar.
pub(super) unsafe extern "C" fn cmd_demo_list_count_subfolders() {
    handle_toggle(
        demo_list_folders::COUNT_SUBFOLDERS.name,
        &demo_list_folders::COUNT_SUBFOLDERS.fallback,
        demo_list_folders::count_subfolders_status,
    );
}

/// Only registered when `ds_ui_demo_list_show_counts` could not be a
/// cvar.
pub(super) unsafe extern "C" fn cmd_demo_list_show_counts() {
    handle_toggle(
        demo_list_folders::SHOW_COUNTS.name,
        &demo_list_folders::SHOW_COUNTS.fallback,
        demo_list_folders::show_counts_status,
    );
}

/// Only registered when `ds_demo_fast_load` could not be a cvar: on
/// is the default budget.
pub(super) unsafe extern "C" fn cmd_demo_fast_load() {
    handle_toggle(
        crate::fast_load::NAME,
        &crate::fast_load::FALLBACK_ON,
        crate::fast_load::status,
    );
}

/// Only registered when `ds_ui_viewdemo_in_panel` could not be a cvar.
pub(super) unsafe extern "C" fn cmd_viewdemo_in_panel() {
    handle_toggle(
        crate::studio_panel::VIEWDEMO_NAME,
        &crate::studio_panel::VIEWDEMO_IN_PANEL,
        crate::studio_panel::viewdemo_status,
    );
}

/// Only registered when `ds_player_fix_model_flicker` could not be a cvar.
pub(super) unsafe extern "C" fn cmd_fix_model_flicker() {
    handle_toggle(
        crate::strafe_twitch::NAME,
        &crate::strafe_twitch::FIX_MODEL_FLICKER,
        crate::strafe_twitch::status,
    );
}

/// Only registered when `missing_shots::NAME` could not be a cvar.
pub(super) unsafe extern "C" fn cmd_restore_gunshots() {
    handle_toggle(missing_shots::NAME, &missing_shots::WITHOUT_A_CVAR, || {
        String::from("HLTV and POV demos: rounds with no fire event")
    });
}

/// Only registered when `ds_ui_console_in_panel` could not be a cvar.
pub(super) unsafe extern "C" fn cmd_console_in_panel() {
    handle_toggle(
        crate::studio_panel::CONSOLE_NAME,
        &crate::studio_panel::CONSOLE_IN_PANEL,
        crate::studio_panel::console_status,
    );
}

/// Only registered when `ds_demo_show_load_progress` could not be a cvar.
pub(super) unsafe extern "C" fn cmd_show_load_progress() {
    handle_toggle(
        crate::demo_bar_progress::NAME,
        &crate::demo_bar_progress::SETTING.fallback,
        crate::demo_bar_progress::status,
    );
}

/// Only registered when `ds_ui_review_keys` could not be a cvar.
pub(super) unsafe extern "C" fn cmd_review_keys() {
    handle_toggle(
        crate::review_keys::KEYS.name,
        &crate::review_keys::KEYS.fallback,
        crate::review_keys::keys_status,
    );
}

/// Only registered when `ds_ui_review_info` could not be a cvar.
pub(super) unsafe extern "C" fn cmd_review_info() {
    handle_toggle(
        crate::review_keys::INFO.name,
        &crate::review_keys::INFO.fallback,
        crate::review_keys::info_status,
    );
}

/// Only registered when the window-layout cvars could not be registered.
pub(super) unsafe extern "C" fn cmd_resizable_windows() {
    handle_toggle(
        window_layout::RESIZABLE_NAME,
        &window_layout::RESIZABLE,
        window_layout::resizable_status,
    );
}

/// Only registered when the window-layout cvars could not be registered.
pub(super) unsafe extern "C" fn cmd_remember_window_layout() {
    handle_toggle(
        window_layout::REMEMBER_NAME,
        &window_layout::REMEMBER,
        window_layout::remember_status,
    );
}

/// Only registered when `ds_demo_seek_skip_between` could not be a cvar.
pub(super) unsafe extern "C" fn cmd_seek_skip_between() {
    handle_toggle(
        demo_seek::SKIP_BETWEEN_NAME,
        &demo_seek::SKIP_BETWEEN,
        demo_seek::status,
    );
}

pub(super) unsafe extern "C" fn cmd_log_held_models() {
    handle_toggle(HELD_MODELS_NAME, &anim_fix::LOG_HELD_MODELS, || {
        "logs the third-person model the spectated player holds, each time it changes".into()
    });
}

/// Issue #206's diagnostic -- see `spectator_target.rs`'s module doc.
pub(super) unsafe extern "C" fn cmd_log_spectator_target() {
    handle_toggle(SPECTATOR_TARGET_LOG_NAME, &spectator_target::LOG, || {
        "logs CHudSpectator's own target alongside the engine's rendered viewmodel entity, whenever either changes".into()
    });
}

pub(super) fn install_fallback_commands() {
    add_command(SPEC_MATCH_POV_NAME, cmd_spec_match_pov);
    add_command(HELD_MODELS_NAME, cmd_log_held_models);
    add_command(SPECTATOR_TARGET_LOG_NAME, cmd_log_spectator_target);
    add_command(SCOREBOARD_NAME, cmd_scoreboard);
    add_command(VOICE_NAME, cmd_voice);
    add_command(CROSSHAIR_NAME, cmd_crosshair);
    add_command(HAND_SIGNALS_NAME, cmd_hand_signals);
    add_command(TEXTURE_HIRES_LOG_NAME, cmd_texture_hires_log);
    // Without this, `poll`'s cvar-independent half (hudelement, deathmsg,
    // spectator_target, msglog) never ran on the fallback path either --
    // see issue #324. `poll` itself stays a no-op for the cvar-backed
    // commands above, since `CVARS_LIVE` is never set on this path; they
    // apply synchronously from their own command handler instead.
    engine::set_per_frame_prologue(poll);
    unsafe {
        crate::debug::report(&format!(
            "commands: fell back to plain commands -- {SPEC_MATCH_POV_NAME}, {HELD_MODELS_NAME}, {SPECTATOR_TARGET_LOG_NAME}, {SCOREBOARD_NAME}, {VOICE_NAME}, {CROSSHAIR_NAME} (no type-ahead value, no .cfg or launch-line setting)"
        ))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #324: `install_fallback_commands()` used to add its plain
    /// commands and stop, never registering `poll` as the per-frame prologue
    /// -- so `poll`'s cvar-independent half (hudelement, deathmsg,
    /// spectator_target, msglog) silently never ran on a build where cvar
    /// registration fails. `engine::engfuncs()` is `None` in this test
    /// process, so the `add_command` calls are themselves no-ops, but
    /// `set_per_frame_prologue` has no such dependency -- this is the one
    /// piece of `install_fallback_commands()` a unit test can observe.
    #[test]
    fn install_fallback_commands_registers_the_per_frame_prologue() {
        assert!(
            !engine::per_frame_prologue_is_set(),
            "some earlier test already registered one"
        );
        install_fallback_commands();
        assert!(engine::per_frame_prologue_is_set());
    }
}
