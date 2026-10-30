//! The per-frame work: `poll` copies the cvars into the flags the rest of
//! the crate reads and calls every module's own `poll`, once per frame, on
//! the engine's thread (`engine::set_per_frame_prologue`).

use std::ffi::CStr;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};

use super::*;
use crate::engine::{self};
use crate::{
    anim_fix, crosshair, demo_list_folders, demo_other_drives, ex_interp, hand_signals, hudelement,
    overview_map, scoreboard, spectator_crosshair, spectator_eye, spectator_target, texture_hires,
    voice, window_layout,
};

/// Reads one cvar into a bool flag, reporting only when it changes.
///
/// This runs every frame, so an unconditional log line would flood the file and
/// slow a capture — the same reason `anim_fix`'s stage trace is
/// change-triggered.
fn poll_flag(name: &str, cvar: &Cvar, flag: &AtomicBool) {
    let Some(wanted) = cvar.try_on() else {
        return;
    };
    if flag.swap(wanted, Ordering::Relaxed) != wanted {
        let state = if wanted { "1 (on)" } else { "0 (off)" };
        unsafe { crate::debug::report(&format!("commands: {name} = {state}")) };
    }
}

/// Like `poll_flag`, but into the level `anim_fix` reads -- see
/// `anim_fix::LEVEL`. Out-of-range values are clamped rather than refused, so
/// an old `ds_spec_match_pov 2` still means on here (its "log every restored
/// gunshot" is `ds_sound_restore_gunshots 2` now, #1036).
fn poll_level(name: &str, cvar: &Cvar, level: &AtomicI32) {
    let Some(raw) = cvar.value() else {
        return;
    };
    // A cvar is a float; anything unparseable reads as 0, which is "off" and
    // is the safe way to land.
    let wanted = if raw.is_finite() { raw as i32 } else { 0 };
    let wanted = wanted.clamp(anim_fix::LEVEL_OFF, anim_fix::LEVEL_MAX);
    if level.swap(wanted, Ordering::Relaxed) != wanted {
        unsafe {
            crate::debug::report(&format!(
                "commands: {name} = {wanted} ({})",
                anim_fix::level_description(wanted)
            ))
        };
    }
}

/// Set once per code-patch cvar that could not be applied, so a failure is
/// reported once rather than sixty times a second. `client.dll` is not loaded
/// for the first few frames of a session, which is exactly when these cvars
/// are most likely to already hold a value from the launch line.
static SCOREBOARD_COMPLAINED: AtomicBool = AtomicBool::new(false);
static VOICE_COMPLAINED: AtomicBool = AtomicBool::new(false);
static CROSSHAIR_COMPLAINED: AtomicBool = AtomicBool::new(false);
static SPECTATOR_CROSSHAIR_COMPLAINED: AtomicBool = AtomicBool::new(false);
static SPECTATOR_EYE_COMPLAINED: AtomicBool = AtomicBool::new(false);
static HUDELEMENT_COMPLAINED: AtomicBool = AtomicBool::new(false);
static EX_INTERP_COMPLAINED: AtomicI32 = AtomicI32::new(0);
static OVERVIEWMAP_COMPLAINED: AtomicBool = AtomicBool::new(false);

/// Three of these cvars do not set a flag the rest of the crate reads -- they
/// write to `client.dll`'s code. Those are handed to their `apply` every frame
/// rather than compared against a cached copy: after the first scan the call is
/// a short byte compare, and deciding from the bytes is what would let the
/// setting survive `client.dll` being unloaded and reloaded -- measured *not*
/// to happen for a plain demo change (`docs/goldsrc_dod_quirks.md`), but
/// untested for a mod change or returning to the menu. `apply` reports
/// whether it wrote, so the log line is still change-triggered.
///
/// It also keeps retrying while `client.dll` is not loaded yet, which is the
/// normal state for the first frames of a session -- and exactly when these
/// cvars already hold a value handed to them on the launch line.
fn poll_code_patch(
    name: &str,
    cvar: &Cvar,
    complained: &AtomicBool,
    apply: fn(bool) -> Result<bool, String>,
    describe: fn(bool) -> &'static str,
) {
    poll_code_patch_when(name, cvar, true, complained, apply, describe);
}

/// [`poll_code_patch`], with the patch held off while `allowed` is false even
/// though the cvar is on: the match-POV patches during a POV demo (#613). The
/// log line says it stood down, so a POV demo doesn't read as the cvar having
/// been turned off.
fn poll_code_patch_when(
    name: &str,
    cvar: &Cvar,
    allowed: bool,
    complained: &AtomicBool,
    apply: fn(bool) -> Result<bool, String>,
    describe: fn(bool) -> &'static str,
) {
    let Some(wanted) = cvar.try_on() else {
        return;
    };
    let on = wanted && allowed;
    match apply(on) {
        Ok(false) => complained.store(false, Ordering::Relaxed),
        Ok(true) => {
            complained.store(false, Ordering::Relaxed);
            let line = if wanted && !allowed {
                format!(
                    "commands: {name} stands down for a POV demo -- {}",
                    describe(false)
                )
            } else {
                format!("commands: {name} = {}", describe(on))
            };
            unsafe { crate::debug::report(&line) };
        }
        Err(why) => {
            if !complained.swap(true, Ordering::Relaxed) {
                unsafe {
                    crate::debug::report(&format!("commands: {name} not applied yet -- {why}"))
                };
            }
        }
    }
}

fn describe_scoreboard(on: bool) -> &'static str {
    if on {
        "1 (+showscores blocked)"
    } else {
        "0 (normal)"
    }
}

fn describe_voice(on: bool) -> &'static str {
    if on {
        "1 (voice commands silent)"
    } else {
        "0 (normal)"
    }
}

fn describe_crosshair(on: bool) -> &'static str {
    if on {
        "1 (crosshair hidden)"
    } else {
        "0 (normal)"
    }
}

/// Reads the cvar, clears the remembered stances when it is turned off, and
/// then does the frame's substitution. Kept out of `poll_flag` because turning
/// it off has to forget: a stance remembered before a map change is not one to
/// put back after it.
fn poll_hand_signals() {
    let Some(wanted) = CVAR_HAND_SIGNALS.try_on() else {
        return;
    };
    if wanted != hand_signals::ENABLED.swap(wanted, Ordering::Relaxed) {
        if !wanted {
            hand_signals::reset();
        }
        unsafe {
            crate::debug::report(&format!(
                "commands: {HAND_SIGNALS_NAME} = {}",
                if wanted {
                    "1 (gestures replaced)"
                } else {
                    "0 (normal)"
                }
            ))
        };
    }
    hand_signals::apply();
}

/// Reads the ceiling out of the cvar and writes it into the engine's clamp.
///
/// Its own poll rather than `poll_code_patch`'s because the setting is a value,
/// not a flag -- and it complains on the *value* rather than once, so changing
/// the cvar to another bad number says so again while an unchanged bad one
/// stays quiet.
fn poll_ex_interp() {
    let Some(wanted) = CVAR_EX_INTERP.value() else {
        return;
    };
    if !wanted.is_finite() {
        return;
    }
    let wanted = wanted as i32;
    match ex_interp::set_max(wanted) {
        Ok(false) => EX_INTERP_COMPLAINED.store(0, Ordering::Relaxed),
        Ok(true) => {
            EX_INTERP_COMPLAINED.store(0, Ordering::Relaxed);
            unsafe {
                crate::debug::report(&format!(
                    "commands: {EX_INTERP_NAME} = {wanted} -- {}",
                    ex_interp::status()
                ))
            };
        }
        Err(why) => {
            if EX_INTERP_COMPLAINED.swap(wanted, Ordering::Relaxed) != wanted {
                unsafe {
                    crate::debug::report(&format!(
                        "commands: {EX_INTERP_NAME} = {wanted} not applied -- {why}"
                    ))
                };
            }
        }
    }
}

/// Re-asserts any held overview-map rectangle. `VidInit` recomputes both on a
/// resolution change or a level load, so holding one means writing it again.
fn poll_overviewmap() {
    if !overview_map::any_held() {
        return;
    }
    match overview_map::apply() {
        Ok(0) => OVERVIEWMAP_COMPLAINED.store(false, Ordering::Relaxed),
        Ok(written) => {
            OVERVIEWMAP_COMPLAINED.store(false, Ordering::Relaxed);
            unsafe {
                crate::debug::report(&format!(
                    "commands: {OVERVIEWMAP_NAME} re-applied {written} field(s) -- VidInit had recomputed them"
                ))
            };
        }
        Err(why) => {
            if !OVERVIEWMAP_COMPLAINED.swap(true, Ordering::Relaxed) {
                unsafe {
                    crate::debug::report(&format!(
                        "commands: {OVERVIEWMAP_NAME} not applied -- {why}"
                    ))
                };
            }
        }
    }
}

/// `hudelement` keeps its own state -- a bitmask the settings that use it
/// set (`ds_hud_hide_teammate_info`), not a cvar -- so it cannot go through
/// `poll_code_patch`. Everything else about it is the same: applied every
/// frame, reported only when it writes, and complaining once rather than
/// sixty times a second while `client.dll` is not loaded.
fn poll_hudelements() {
    match hudelement::apply() {
        Ok(0) => HUDELEMENT_COMPLAINED.store(false, Ordering::Relaxed),
        Ok(written) => {
            HUDELEMENT_COMPLAINED.store(false, Ordering::Relaxed);
            unsafe {
                crate::debug::report(&format!(
                    "commands: hudelement wrote {written} vftable slot(s) -- {}",
                    hudelement::status()
                ))
            };
        }
        Err(why) => {
            // Nothing hidden and nothing to restore is the normal state, and
            // it is not worth a line in the log every session just because
            // client.dll has not loaded yet.
            if hudelement::hidden_count() == 0 {
                return;
            }
            if !HUDELEMENT_COMPLAINED.swap(true, Ordering::Relaxed) {
                unsafe {
                    crate::debug::report(&format!("commands: hudelement not applied yet -- {why}"))
                };
            }
        }
    }
}

fn describe_spectator_eye(on: bool) -> &'static str {
    if on {
        "on: the in-eye camera drops to the ground for a prone player and eases through a crouch"
    } else {
        "off: the in-eye camera uses the game's own heights"
    }
}

fn describe_spectator_crosshair(on: bool) -> &'static str {
    if on {
        "on: the spectator crosshair follows cl_xhair_style"
    } else {
        "off: the spectator crosshair is the game's own"
    }
}

/// Copies the cvars into the flags the rest of the crate reads. Registered as
/// the per-frame prologue so it lands before `anim_fix::apply()` runs --
/// under both `install()` and `install_fallback_commands()`, since
/// `poll_hudelements`, `deathmsg::poll`, `spectator_target::poll` and
/// `msglog::poll` below do not depend on cvars existing at all and must run
/// either way (issue #324).
pub fn poll() {
    // Cvar-backed polling only: every function below reads a `CVAR_*`
    // pointer that stays null on the fallback path, but each already
    // null-checks it individually -- this outer guard is purely the fast
    // path that skips all nine checks at once when cvars never registered.
    if CVARS_LIVE.load(Ordering::Relaxed) {
        // One cvar, several readers: the viewmodel's animations here, and
        // the spectator crosshair and eye height below.
        poll_level(SPEC_MATCH_POV_NAME, &CVAR_SPEC_MATCH_POV, &anim_fix::LEVEL);
        poll_flag(
            HELD_MODELS_NAME,
            &CVAR_HELD_MODELS,
            &anim_fix::LOG_HELD_MODELS,
        );
        poll_flag(
            SPECTATOR_TARGET_LOG_NAME,
            &CVAR_SPECTATOR_TARGET_LOG,
            &spectator_target::LOG,
        );
        poll_code_patch(
            SCOREBOARD_NAME,
            &CVAR_SCOREBOARD,
            &SCOREBOARD_COMPLAINED,
            scoreboard::set_hidden,
            describe_scoreboard,
        );
        poll_code_patch(
            VOICE_NAME,
            &CVAR_VOICE,
            &VOICE_COMPLAINED,
            voice::set_muted,
            describe_voice,
        );
        poll_code_patch(
            CROSSHAIR_NAME,
            &CVAR_CROSSHAIR,
            &CROSSHAIR_COMPLAINED,
            crosshair::set_hidden,
            describe_crosshair,
        );
        // Polled every frame like the rest, and for one extra reason: this is
        // also how it notices `cl_xhair_style` changing under it.
        //
        // Both stand down in a POV demo, which is the recording they match.
        let allowed = anim_fix::active();
        poll_code_patch_when(
            SPEC_MATCH_POV_NAME,
            &CVAR_SPEC_MATCH_POV,
            allowed,
            &SPECTATOR_CROSSHAIR_COMPLAINED,
            spectator_crosshair::set_matching,
            describe_spectator_crosshair,
        );
        poll_code_patch_when(
            SPEC_MATCH_POV_NAME,
            &CVAR_SPEC_MATCH_POV,
            allowed,
            &SPECTATOR_EYE_COMPLAINED,
            spectator_eye::set_matching,
            describe_spectator_eye,
        );
        poll_hand_signals();
        poll_ex_interp();
        poll_flag(
            TEXTURE_HIRES_LOG_NAME,
            &CVAR_TEXTURE_HIRES_LOG,
            &texture_hires::LOG_TEXTURE_LOADS,
        );
    }
    // Everything below has nothing to do with cvars and must run under both
    // paths (#324).
    // ds_hud_hide_teammate_info sets hudelement's bit, which the next line
    // writes.
    crate::teammate_info::poll();
    poll_hudelements();
    // ds_hud_overview_layout keeps its own state -- held rects, not a cvar --
    // so it belongs here alongside hudelement rather than in the gated block
    // above.
    poll_overviewmap();
    // Re-prepends our DeathMsg handler when the engine has rebuilt the user
    // message list (it frees the whole list on disconnect). A no-op otherwise.
    crate::deathmsg::poll();
    // Runs in the prologue (before anim_fix::apply(), the one per-frame
    // callback slot), so its viewmodel-entity half reads apply()'s previous
    // frame's result, not this one's -- see spectator_target.rs's module doc.
    spectator_target::poll();
    // Same reason, for whichever messages ds_debug_msglog currently wants.
    crate::msglog::poll();
    // Reads the map's own on-screen strings once per level while
    // ds_hud_hide_map_text is on, and keeps its HudText handler prepended.
    crate::map_text::poll();
    // Keeps the TextMsg, ClanTimer and GameRules handlers prepended, and puts
    // the game's countdown and warm-up flag right when
    // ds_hud_hide_clan_text changes.
    crate::clan_text::poll();
    // ds_player_change_name: who each slot is and which rename wins, for the
    // GetPlayerInfo detour to read; keeps the SayText handler prepended.
    crate::player_names::poll();
    // ds_hud_hide_center_text: its filter rides on clan_text's TextMsg
    // handler, so this only follows the cvar.
    crate::center_text::poll();
    // ds_spec_hide_text: two code patches, byte-checked each frame.
    crate::spectator_text::poll();
    // ds_hud_hide_plugin_text: one hw.dll call, byte-checked each frame.
    crate::plugin_text::poll();
    // Keeps the ShowMenu handler prepended and the voice and command menus
    // hidden while ds_hud_hide_voice_menus is on, and every other menu while
    // ds_hud_hide_plugin_text is (after plugin_text::poll, which it reads).
    crate::voice_menus::poll();
    // ds_hud_hide_hints: one code patch, byte-checked each frame.
    crate::hint_panel::poll();
    // Keeps the weapon list off screen in demo playback with hud_fastswitch 1:
    // one code byte, checked each frame.
    crate::weapon_list::poll();
    // Follows ds_hd_enabled / ds_hd_style, then notes what each map
    // uses for ds_debug_hd_misses. Cheap unless one of them changed.
    log_level_changes();
    crate::lightmap_gamma::poll();
    crate::tempent_fix::poll();
    crate::hull_trace_guard::poll();
    // Installs once GameUI.dll and FileSystem_Stdio.dll are found, then costs
    // one atomic load.
    demo_list_folders::poll();
    // Installs once FileSystem_Stdio.dll is found, then costs one atomic load.
    demo_other_drives::poll();
    // Installs once GameUI.dll is found, then costs one atomic load.
    crate::frame_esc::poll();
    // Installs once GameUI.dll is found, then costs one atomic load.
    crate::engine_buttons::poll();
    // Every few frames, once GameUI, vgui2 and hw are found; a cvar read or
    // two while both of its settings are off.
    window_layout::poll();
    // Runs any console commands Studio has sent over the pipe.
    crate::remote::poll();
    // One step of a long forward seek, or a landed seek's follow-up command.
    crate::demo_seek::poll();
    // Patches Core.dll's packet loop once viewdemo loads it, and logs each
    // load's time (#798).
    crate::fast_load::poll();
    // Only until playdemo is wrapped, normally already done at install.
    crate::demo_reload::poll();
    // Only until connect is wrapped, normally already done at install.
    crate::connect_guard::poll();
    crate::events::poll();
    crate::batch_end::poll();
    texture_hires::poll_hd();
    texture_hires::poll_map();
    // Once a second: rereads Studio's Players table when it changed and
    // installs the demo spray detours once it asks for anything (#428).
    crate::spray_hltv::poll();
    // After the table is read: this frame's crosshair for the player on
    // screen, which the HUD's sprite draws then pick up (#428).
    crate::player_crosshair::poll();
    // Re-raises sv_allow_shaders after each demo load's disconnect reset.
    crate::world_shaders::poll();
    crate::gun_log::poll();
    crate::missing_shots::poll();
    // Keeps the CurWeapon handler prepended during a POV demo and plays the
    // rounds it found with no recorded sound.
    crate::pov_recorder_shots::poll();
    crate::spectator_bars::poll();
    // After spectator_bars::poll, so it lays out by this frame's bar state.
    crate::spectator_hud::poll();
    crate::spectator_follow::poll();
    // While a review runs, the viewer's clicks don't move its camera (#826).
    crate::spectator_buttons::poll();
    // ...and the console can't move its demo or camera (#855).
    crate::review_lock::console::poll();
    crate::overview_players::poll();
    crate::overview_marker::poll();
    crate::overview_speaker::poll();
    // Lifts the scope's demo gate and keeps the in-eye rifle current (#307).
    crate::demo_scope::poll();
    crate::run_in_background::poll();
    // Times this frame while ds_debug_record_frame_times is on (#646).
    crate::frame_times::poll();
    // An atomic load; an atomic add while a clip records (#1221).
    crate::clip_log::poll();
    crate::pov_live_bob::poll();
    // A float compare unless ds_view_show_own_blood changed (#1035).
    crate::own_blood::poll();
    // A float compare unless ds_map_weather changed (#1129).
    crate::map_weather::poll();
    crate::map_sounds::poll();
    crate::screen_effects::poll();
    crate::hlae_window::poll();
    crate::detail_cache::poll();
    crate::studio_panel::poll();
    crate::review::poll();
    crate::schedule::poll();
    crate::batch::poll();
}

/// Writes `level: maps/<name>.bsp` to the log whenever the loaded level
/// changes, so a crash further down the log can be tied to the map it
/// happened on (`tools/crash_report.py` reads it). A frame where it hasn't
/// changed costs a hash of the name.
fn log_level_changes() {
    static LAST: AtomicU32 = AtomicU32::new(0);
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    // Safety: a pointer into the engine's client state, valid for the
    // session; checked for null before reading.
    let raw = unsafe { (engfuncs.pfn_get_level_name)() };
    if raw.is_null() {
        return;
    }
    let name = unsafe { CStr::from_ptr(raw) }.to_bytes();
    if name.is_empty() {
        return;
    }
    let hash = name.iter().fold(0x811c_9dc5_u32, |h, &b| {
        (h ^ b as u32).wrapping_mul(0x0100_0193)
    });
    if LAST.swap(hash, Ordering::Relaxed) != hash {
        unsafe { crate::debug::report(&format!("level: {}", String::from_utf8_lossy(name))) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every suppression cvar reads the same way: **1 does the thing the name
    /// says**, 0 leaves the game alone. That is the whole point of naming them
    /// `hide_*` / `mute_*` rather than after the thing they act on, and it is
    /// the one property a future edit could invert without any test noticing --
    /// the byte-level tests check widths and encodings, not sense.
    #[test]
    fn one_means_suppressed_for_every_suppression_cvar() {
        assert!(
            describe_scoreboard(true).contains("blocked"),
            "{}",
            describe_scoreboard(true)
        );
        assert!(
            describe_scoreboard(false).contains("normal"),
            "{}",
            describe_scoreboard(false)
        );

        assert!(
            describe_crosshair(true).contains("hidden"),
            "{}",
            describe_crosshair(true)
        );
        assert!(
            describe_crosshair(false).contains("normal"),
            "{}",
            describe_crosshair(false)
        );

        assert!(
            describe_voice(true).contains("silent"),
            "{}",
            describe_voice(true)
        );
        assert!(
            describe_voice(false).contains("normal"),
            "{}",
            describe_voice(false)
        );

        for d in [
            describe_scoreboard(true),
            describe_crosshair(true),
            describe_voice(true),
        ] {
            assert!(d.starts_with('1'), "{d}");
        }
    }
}
