//! The `dodstudio_*` console surface: eleven cvars and eight commands.
//!
//! ## Why cvars rather than commands
//!
//! These were four `pfnAddCommand` commands, and the difference is not
//! cosmetic. A command is a function the engine calls and forgets; the state
//! lives in this DLL's own atomics, where the console cannot see it. So
//! `dodstudio_hltv_show_viewmodel_animations` printed nothing in the type-ahead, could not
//! be queried with a bare name the way `sensitivity` can, and — the part that
//! actually mattered — could not be set from a config file or the launch line.
//! That last gap is the entire reason `GOLDSRC_HOOKS_ANIM_FIX` and
//! `ANIM_FIX_DEFAULT` existed.
//!
//! A cvar is a named box the *engine* owns. It shows up in the type-ahead with
//! its value, answers `dodstudio_hltv_show_viewmodel_animations` on its own, takes
//! `+dodstudio_hltv_show_viewmodel_animations 1` on the launch line, and can be set from any
//! `.cfg` the user execs. `poll()` copies the values into the same atomics the
//! rest of the crate already reads, once per frame, so nothing downstream
//! changed.
//!
//! ## Not archived, deliberately
//!
//! `FCVAR_ARCHIVE` would make the engine write these into the user's
//! `config.cfg` when it quits. It is not set. The pipeline's standing rule is
//! that the game's own `.cfg` files belong to the user and are detected and
//! warned about, never written (`native/src/patch/cfg_scan.rs`, and the rule in
//! `CLAUDE.md`), and a capture-time DLL quietly adding lines to `config.cfg`
//! is exactly that. Setting from a `.cfg` or the launch line still works
//! without archiving, which is the part that retires the environment
//! variables. Flip `CVAR_FLAGS` to `FCVAR_ARCHIVE` if that trade is ever
//! wanted the other way round.
//!
//! ## The fallback
//!
//! If `pfnRegisterVariable` does not hand back a cvar whose own `name` matches
//! what was asked for, the layout assumption is wrong and every subsequent read
//! would be garbage. That case registers the old commands instead and says so
//! loudly, so a bad assumption costs the type-ahead rather than the session.
//!
//! Registration must happen after `engine::engfuncs()` is captured (i.e. after
//! `client.dll`'s real `Initialize` has run), since both `pfnRegisterVariable`
//! and `pfnAddCommand` live on that same table.

use std::ffi::{CStr, CString, c_char};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU32, Ordering};

use crate::engine::{self, CvarSPartial};
use crate::names::console_name;
use crate::{
    anim_fix, crosshair, decals, demo_list_folders, ex_interp, hand_signals, hudelement,
    overview_map, scoreboard, sound_fix, spectator_crosshair, spectator_target, texture_hires,
    voice,
};

const GUNSHOTS_FIX_NAME: &str = console_name!("hltv_gunshots_fix");
const ANIMATION_FIX_NAME: &str = console_name!("hltv_show_viewmodel_animations");
const ATTENUATION_NAME: &str = console_name!("hltv_gunshot_attenuation");
// Not "..._weapon_switch": it fires on stance changes too (p_mg42pr,
// p_mg42sr), and those are the reason it exists.
const HELD_MODELS_NAME: &str = console_name!("debug_log_weapon_model");
/// See spectator_target.rs's module doc -- issue #206's diagnostic.
const SPECTATOR_TARGET_LOG_NAME: &str = console_name!("debug_log_spectator_target");
const STATUS_NAME: &str = console_name!("debug_status");
/// Each module owns its own name, because its error text uses it too.
const SCOREBOARD_NAME: &str = scoreboard::NAME;
const VOICE_NAME: &str = voice::NAME;
const CROSSHAIR_NAME: &str = crosshair::NAME;
const SPECTATOR_CROSSHAIR_NAME: &str = spectator_crosshair::NAME;
const HUDELEMENT_NAME: &str = hudelement::NAME;
const CLEAR_DECALS_NAME: &str = decals::NAME;
const HAND_SIGNALS_NAME: &str = hand_signals::NAME;
const EX_INTERP_NAME: &str = ex_interp::NAME;
const OVERVIEWMAP_NAME: &str = overview_map::NAME;
const TEXTURE_HIRES_LOG_NAME: &str = texture_hires::NAME;

/// `FCVAR_ARCHIVE` is 1. Deliberately not set — see the module docs.
const CVAR_FLAGS: i32 = 0;

/// The cvars the engine handed back, read once per frame by `poll`.
static CVAR_GUNSHOTS: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
static CVAR_ANIMATION: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
static CVAR_ATTENUATION: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
static CVAR_HELD_MODELS: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
static CVAR_SPECTATOR_TARGET_LOG: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
static CVAR_SCOREBOARD: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
static CVAR_VOICE: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
static CVAR_CROSSHAIR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
static CVAR_SPECTATOR_CROSSHAIR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
static CVAR_HAND_SIGNALS: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
static CVAR_EX_INTERP: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
static CVAR_TEXTURE_HIRES_LOG: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());

/// Set when registration succeeded, so `poll` does nothing at all on the
/// command fallback path rather than reading null pointers every frame.
static CVARS_LIVE: AtomicBool = AtomicBool::new(false);

pub(crate) fn console_print(text: &str) {
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    let Ok(c_text) = CString::new(text) else {
        return;
    };
    unsafe { (engfuncs.pfn_console_print)(c_text.as_ptr()) };
}

/// Registers one cvar and checks the engine handed back what was asked for.
///
/// The name check is the whole point: it costs one `strcmp` at startup and it
/// is the only evidence available at runtime that `CvarSPartial`'s layout and
/// the engine's agree. A wrong layout would otherwise show up as a toggle that
/// silently does nothing.
fn register(name: &str, default: &str) -> Option<*mut CvarSPartial> {
    let engfuncs = engine::engfuncs()?;
    let c_name = CString::new(name).ok()?;
    let c_default = CString::new(default).ok()?;
    let cvar = unsafe {
        (engfuncs.pfn_register_variable)(c_name.as_ptr(), c_default.as_ptr(), CVAR_FLAGS)
    };
    // The engine keeps both strings for the life of the session.
    std::mem::forget(c_name);
    std::mem::forget(c_default);

    if cvar.is_null() {
        unsafe {
            crate::debug::report(&format!(
                "commands: pfnRegisterVariable returned null for {name}"
            ))
        };
        return None;
    }
    match unsafe { (*cvar).name_str() } {
        Some(got) if got == name => Some(cvar),
        other => {
            unsafe {
                crate::debug::report(&format!(
                    "commands: registered {name} but the cvar came back named {:?} -- cvar_s layout is wrong, falling back to commands",
                    other.as_deref()
                ))
            };
            None
        }
    }
}

/// Reads one cvar into a bool flag, reporting only when it changes.
///
/// This runs every frame, so an unconditional log line would flood the file and
/// slow a capture — the same reason `anim_fix`'s stage trace is
/// change-triggered.
fn poll_flag(name: &str, cvar: &AtomicPtr<CvarSPartial>, flag: &AtomicBool) {
    let ptr = cvar.load(Ordering::Relaxed);
    if ptr.is_null() {
        return;
    }
    let wanted = unsafe { (*ptr).value } != 0.0;
    if flag.swap(wanted, Ordering::Relaxed) != wanted {
        let state = if wanted { "1 (on)" } else { "0 (off)" };
        unsafe { crate::debug::report(&format!("commands: {name} = {state}")) };
    }
}

/// Like `poll_flag`, but the animation fix carries an iteration number rather
/// than a flag -- see `anim_fix::LEVEL`. Out-of-range values are clamped
/// rather than refused, so `dodstudio_hltv_show_viewmodel_animations 99` is a usable way to
/// ask for the newest behaviour without remembering what the newest is.
fn poll_level(name: &str, cvar: &AtomicPtr<CvarSPartial>, level: &AtomicI32) {
    let ptr = cvar.load(Ordering::Relaxed);
    if ptr.is_null() {
        return;
    }
    let raw = unsafe { (*ptr).value };
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

/// The last attenuation value rejected, so a bad setting is reported once
/// rather than sixty times a second.
static REJECTED_ATTENUATION: AtomicU32 = AtomicU32::new(0);

fn poll_attenuation() {
    let ptr = CVAR_ATTENUATION.load(Ordering::Relaxed);
    if ptr.is_null() {
        return;
    }
    let wanted = unsafe { (*ptr).value };
    if wanted == sound_fix::carry_attenuation() {
        return;
    }
    match sound_fix::set_carry_attenuation(wanted) {
        Ok(()) => {
            REJECTED_ATTENUATION.store(0, Ordering::Relaxed);
            unsafe { crate::debug::report(&format!("commands: {ATTENUATION_NAME} = {wanted}")) };
        }
        Err(why) => {
            // Out-of-range values sit in the cvar until the user changes them,
            // so this would repeat every frame without the guard. The previous
            // good value stays in effect.
            if REJECTED_ATTENUATION.swap(wanted.to_bits(), Ordering::Relaxed) != wanted.to_bits() {
                unsafe {
                    crate::debug::report(&format!(
                        "commands: {ATTENUATION_NAME} = {wanted} rejected -- {why}; still using {}",
                        sound_fix::carry_attenuation()
                    ))
                };
            }
        }
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
    cvar: &AtomicPtr<CvarSPartial>,
    complained: &AtomicBool,
    apply: fn(bool) -> Result<bool, String>,
    describe: fn(bool) -> &'static str,
) {
    let ptr = cvar.load(Ordering::Relaxed);
    if ptr.is_null() {
        return;
    }
    let on = unsafe { (*ptr).value } != 0.0;
    match apply(on) {
        Ok(false) => complained.store(false, Ordering::Relaxed),
        Ok(true) => {
            complained.store(false, Ordering::Relaxed);
            unsafe { crate::debug::report(&format!("commands: {name} = {}", describe(on))) };
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
    let ptr = CVAR_HAND_SIGNALS.load(Ordering::Relaxed);
    if ptr.is_null() {
        return;
    }
    let wanted = unsafe { (*ptr).value } != 0.0;
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
    let ptr = CVAR_EX_INTERP.load(Ordering::Relaxed);
    if ptr.is_null() {
        return;
    }
    let wanted = unsafe { (*ptr).value };
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

/// `dodstudio_hide_hudelement` keeps its own state -- a bitmask, not a cvar --
/// so it cannot go through `poll_code_patch`. Everything else about it is the
/// same: applied every frame, reported only when it writes, and complaining
/// once rather than sixty times a second while `client.dll` is not loaded.
fn poll_hudelements() {
    match hudelement::apply() {
        Ok(0) => HUDELEMENT_COMPLAINED.store(false, Ordering::Relaxed),
        Ok(written) => {
            HUDELEMENT_COMPLAINED.store(false, Ordering::Relaxed);
            unsafe {
                crate::debug::report(&format!(
                    "commands: {HUDELEMENT_NAME} wrote {written} vftable slot(s) -- {}",
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
                    crate::debug::report(&format!(
                        "commands: {HUDELEMENT_NAME} not applied yet -- {why}"
                    ))
                };
            }
        }
    }
}

fn describe_spectator_crosshair(on: bool) -> &'static str {
    if on {
        "1 (spectator crosshair follows cl_xhair_style)"
    } else {
        "0 (normal)"
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
        poll_flag(GUNSHOTS_FIX_NAME, &CVAR_GUNSHOTS, &sound_fix::ENABLED);
        poll_level(ANIMATION_FIX_NAME, &CVAR_ANIMATION, &anim_fix::LEVEL);
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
        poll_attenuation();
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
        poll_code_patch(
            SPECTATOR_CROSSHAIR_NAME,
            &CVAR_SPECTATOR_CROSSHAIR,
            &SPECTATOR_CROSSHAIR_COMPLAINED,
            spectator_crosshair::set_matching,
            describe_spectator_crosshair,
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
    // paths -- it was silently skipped on the fallback path before #324.
    poll_hudelements();
    // dodstudio_overviewmap keeps its own state -- held rects, not a cvar --
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
    // Same reason, for whichever messages dodstudio_debug_msglog currently wants.
    crate::msglog::poll();
    // Follows dodstudio_hd_enabled / dodstudio_hd_style, then notes what each map
    // uses for dodstudio_debug_hd_misses. Cheap unless one of them changed.
    log_level_changes();
    crate::tempent_fix::poll();
    crate::hull_trace_guard::poll();
    // Installs once GameUI.dll and FileSystem_Stdio.dll are found, then costs
    // one atomic load.
    demo_list_folders::poll();
    texture_hires::poll_hd();
    texture_hires::poll_map();
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

/// Everything in one place, for debugging -- not the settings surface a
/// player is expected to type. That is what `debug_` in the name signals:
/// every value here is also visible piecemeal (a suppression cvar's own
/// bare-name query, the console type-ahead, `dodstudio_deathmsg`'s own
/// status), but this is the one command that dumps all of it together, which
/// is what a support question actually needs -- so it covers the *entire*
/// `dodstudio_*` surface, not a subset.
///
/// The suppression cvars and `log_weapon_model` are listed unconditionally,
/// on or off, because there is no progress to gate them on -- they are just
/// a byte, and "what is it set to" is exactly what this command exists to
/// answer without hunting down each bare name individually. The two fixes
/// below them are gated on being enabled, because for *those* a flag being
/// on says nothing about whether the preconditions are being met in the
/// current view, and "the fix isn't working" has twice turned out to be "the
/// log budget ran out" -- their counters are the honest number, and are
/// noise when off. `dodstudio_hltv_gunshot_attenuation`'s value is folded
/// into the gunshots line rather than given its own, since it does nothing
/// while the fix is off.
fn status_text() -> String {
    let bit = |on: bool| if on { "1" } else { "0" };
    let mut lines: Vec<String> = vec![
        format!(
            "{SCOREBOARD_NAME} = {} -- {}",
            bit(scoreboard::suppressed()),
            scoreboard::status()
        ),
        format!(
            "{VOICE_NAME} = {} -- {}",
            bit(voice::muted()),
            voice::status()
        ),
        format!(
            "{CROSSHAIR_NAME} = {} -- {}",
            bit(crosshair::hidden()),
            crosshair::status()
        ),
        format!(
            "{SPECTATOR_CROSSHAIR_NAME} = {} -- {}",
            bit(spectator_crosshair::matching()),
            spectator_crosshair::status()
        ),
        format!(
            "{HELD_MODELS_NAME} = {} -- logs the third-person model the spectated player holds, each time it changes",
            bit(anim_fix::LOG_HELD_MODELS.load(Ordering::Relaxed))
        ),
        format!(
            "{SPECTATOR_TARGET_LOG_NAME} = {} -- logs CHudSpectator's own target alongside the engine's rendered viewmodel entity, whenever either changes (issue #206)",
            bit(spectator_target::LOG.load(Ordering::Relaxed))
        ),
    ];
    if anim_fix::enabled() {
        lines.push(format!("viewmodel animations: {}", anim_fix::status()));
    }
    if sound_fix::ENABLED.load(Ordering::Relaxed) {
        lines.push(format!(
            "gunshots: {} ({ATTENUATION_NAME} = {})",
            sound_fix::status(),
            sound_fix::carry_attenuation()
        ));
    }
    lines.push(crate::deathmsg::status().trim_end().to_string());
    // Gated like the two fixes above rather than always shown like the
    // suppression cvars: logging is off by default and a permanent "logging
    // nothing" line would be noise in the overwhelmingly common case.
    if let Some(msglog) = crate::msglog::status_line() {
        lines.push(msglog);
    }
    // Same reasoning as msglog above: hiding is off by default and a
    // permanent "hiding nothing" line would be noise in the common case.
    if let Some(hide_sprite) = crate::hide_sprite::status_line() {
        lines.push(hide_sprite);
    }
    // The one setting the console's own type-ahead cannot report, because it
    // is a command rather than a cvar -- which is the reason the rest are left
    // out of here and this is not.
    if hudelement::hidden_count() > 0 {
        lines.push(format!("HUD elements: {}", hudelement::status()));
    }
    // Also a command rather than a cvar, and for the same reason: it does
    // something once instead of holding a value.
    if decals::has_run() {
        lines.push(format!("decals: {}", decals::status()));
    }
    // Reported whenever it is on, because "is it finding anything?" is the one
    // question the cvar's own value cannot answer.
    if hand_signals::ENABLED.load(Ordering::Relaxed) || hand_signals::has_acted() {
        lines.push(format!("hand signals: {}", hand_signals::status()));
    }
    // Reported whenever it differs from the engine's own ceiling: the cvar says
    // what was asked for, this says what the engine is actually clamping to.
    if ex_interp::active() != 0 && ex_interp::active() != ex_interp::STOCK_MS {
        lines.push(format!("interpolation: {}", ex_interp::status()));
    }
    // Always shown once installed: it is on by default, and "did it ever
    // catch anything?" is the question a crash-free session raises.
    if let Some(tempent) = crate::tempent_fix::status_line() {
        lines.push(tempent);
    }
    if let Some(hull) = crate::hull_trace_guard::status_line() {
        lines.push(hull);
    }
    if overview_map::any_held() {
        lines.push(format!("overview map: {}", overview_map::status()));
    }
    // Only reported once it has actually seen something -- the hook itself is
    // off by default (GOLDSRC_HOOKS_TEXTURE_HIRES), so "0 observed" would be
    // the permanent, noisy default state for everyone who hasn't opted in.
    if texture_hires::has_observed() {
        lines.push(texture_hires::status());
    }
    if lines.is_empty() {
        // Not an error, and worth saying out loud: the suppressions leave no
        // trace to count, so silence here would read as a broken command.
        return "nothing active that reports progress\n".to_string();
    }
    format!("{}\n", lines.join("\n"))
}

unsafe extern "C" fn cmd_status() {
    // A console line's semicolon-joined commands all run together, in one
    // pass, before `poll` gets another turn as the per-frame prologue -- so
    // `dodstudio_hide_scoreboard 1;dodstudio_debug_status` on one line would
    // otherwise report the state from *before* that same line's own change.
    // `poll` is cheap and idempotent (it already runs every frame), so
    // forcing one here just makes this report always current.
    poll();
    let report = status_text();
    console_print(&report);
    // Also to the log, so it stays a complete record of what was actually
    // enabled during a capture -- console scrollback does not survive the
    // session, and "was the fix even on for that take?" is the first question
    // worth answering when a capture looks unchanged.
    unsafe { crate::debug::report(&format!("commands: {STATUS_NAME} --\n{report}")) };
}

// ── Fallback path ────────────────────────────────────────────────────────────
// Only reached when cvar registration could not be trusted. Kept whole rather
// than degraded, because the alternative is a session with no way to turn
// either fix on.

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

unsafe extern "C" fn cmd_gunshots_fix() {
    handle_toggle(GUNSHOTS_FIX_NAME, &sound_fix::ENABLED, sound_fix::status);
}

unsafe extern "C" fn cmd_animation_fix() {
    handle_level(ANIMATION_FIX_NAME, &anim_fix::LEVEL, anim_fix::status);
}

/// `handle_toggle` for the animation fix, which takes an iteration number
/// instead of a flag. Only reached on the fallback path, when cvar
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

unsafe extern "C" fn cmd_scoreboard() {
    handle_code_patch(
        SCOREBOARD_NAME,
        "1 blocks +showscores",
        scoreboard::set_hidden,
        scoreboard::suppressed,
        scoreboard::status,
    );
}

unsafe extern "C" fn cmd_voice() {
    handle_code_patch(
        VOICE_NAME,
        "1 silences voice commands",
        voice::set_muted,
        voice::muted,
        voice::status,
    );
}

unsafe extern "C" fn cmd_crosshair() {
    handle_code_patch(
        CROSSHAIR_NAME,
        "1 hides the crosshair",
        crosshair::set_hidden,
        crosshair::hidden,
        crosshair::status,
    );
}

unsafe extern "C" fn cmd_spectator_crosshair() {
    handle_code_patch(
        SPECTATOR_CROSSHAIR_NAME,
        "1 draws the spectator crosshair from customXHair.spr, like the POV one",
        spectator_crosshair::set_matching,
        spectator_crosshair::matching,
        spectator_crosshair::status,
    );
}

/// `dodstudio_hide_hudelement [<name> <0|1>]`.
///
/// A command rather than a cvar: it takes two arguments, which a cvar's single
/// value cannot carry, and there are thirteen of them -- thirteen cvars would
/// bury everything else in the console's type-ahead.
unsafe extern "C" fn cmd_hudelement() {
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    let argc = unsafe { (engfuncs.cmd_argc)() };

    let argv = |n: i32| -> Option<String> {
        let raw = unsafe { (engfuncs.cmd_argv)(n) };
        (!raw.is_null()).then(|| {
            unsafe { CStr::from_ptr(raw as *const c_char) }
                .to_string_lossy()
                .trim()
                .to_owned()
        })
    };

    if argc < 2 {
        console_print(&format!(
            "usage: {HUDELEMENT_NAME} <element> <0|1>   (1 hides it)\n       {HUDELEMENT_NAME} all 0        (show everything again)\n{}",
            hudelement::listing()
        ));
        return;
    }

    let Some(name) = argv(1) else { return };

    // Checked before the argument count, so a bad name is reported as a bad
    // name whether or not a 0/1 followed it -- "expected <0|1>" would
    // otherwise lead someone to believe the name was fine and only the
    // second argument was missing.
    let is_all = name.eq_ignore_ascii_case("all");
    let index = if is_all {
        None
    } else {
        match hudelement::find(&name) {
            Some(i) => Some(i),
            None => {
                console_print(&format!(
                    "{HUDELEMENT_NAME}: no element called \"{name}\"\n{}",
                    hudelement::listing()
                ));
                return;
            }
        }
    };

    if argc < 3 {
        console_print(&format!(
            "{HUDELEMENT_NAME}: expected {HUDELEMENT_NAME} {name} <0|1>\n"
        ));
        return;
    }
    let Some(raw) = argv(2) else { return };
    let on = match raw.as_str() {
        "0" => false,
        "1" => true,
        other => {
            console_print(&format!(
                "{HUDELEMENT_NAME}: expected 0 or 1, got \"{other}\"\n"
            ));
            return;
        }
    };

    if is_all {
        if on {
            // Deliberately refused. Hiding every element at once includes the
            // menus, and a capture session that cannot see the class menu is a
            // support question, not a feature.
            console_print(&format!(
                "{HUDELEMENT_NAME}: `all 1` would hide the team and class menus too -- name the elements you want gone\n"
            ));
            return;
        }
        hudelement::show_all();
        console_print(&format!("{HUDELEMENT_NAME}: every element shown again\n"));
        unsafe { crate::debug::report(&format!("commands: {HUDELEMENT_NAME} all 0")) };
        return;
    }

    let index =
        index.expect("validated above: not `all`, so `find` succeeded or we already returned");
    hudelement::set_hidden(index, on);
    let bit = if on { "1" } else { "0" };
    // Applied here as well as in `poll`, so the console reports the real
    // outcome rather than "set" for something that could not be written.
    match hudelement::apply() {
        Ok(_) => {
            console_print(&format!("{HUDELEMENT_NAME} {name} = {bit}\n"));
            unsafe { crate::debug::report(&format!("commands: {HUDELEMENT_NAME} {name} = {bit}")) };
        }
        Err(why) => {
            console_print(&format!("{HUDELEMENT_NAME}: {why}\n"));
            unsafe {
                crate::debug::report(&format!(
                    "commands: {HUDELEMENT_NAME} {name} failed -- {why}"
                ))
            };
        }
    }
}

/// `dodstudio_clear_decals` -- takes no arguments and holds no state, so a
/// command is the whole of what it needs to be.
unsafe extern "C" fn cmd_clear_decals() {
    match decals::clear() {
        Ok(removed) => {
            console_print(&format!(
                "{CLEAR_DECALS_NAME}: removed {removed} decal(s)\n"
            ));
            unsafe {
                crate::debug::report(&format!("commands: {CLEAR_DECALS_NAME} removed {removed}"))
            };
        }
        Err(why) => {
            console_print(&format!("{CLEAR_DECALS_NAME}: {why}\n"));
            unsafe {
                crate::debug::report(&format!("commands: {CLEAR_DECALS_NAME} failed -- {why}"))
            };
        }
    }
}

unsafe extern "C" fn cmd_hand_signals() {
    handle_toggle(HAND_SIGNALS_NAME, &hand_signals::ENABLED, || {
        hand_signals::status()
    });
}

/// `dodstudio_debug_log_texture_loads` -- verbose per-texture logging for the
/// (opt-in, `GOLDSRC_HOOKS_TEXTURE_HIRES`-gated) world-texture swap: one line
/// per world texture uploaded, saying whether it was replaced and why not. Registering the toggle unconditionally, whether or not
/// the hook is actually installed this session, matches every other cvar
/// here -- setting it when the hook is off just does nothing yet, rather than
/// the command not existing at all.
unsafe extern "C" fn cmd_texture_hires_log() {
    handle_toggle(
        TEXTURE_HIRES_LOG_NAME,
        &texture_hires::LOG_TEXTURE_LOADS,
        texture_hires::status,
    );
}

/// Only registered when `dodstudio_demo_list_folders` could not be a cvar.
unsafe extern "C" fn cmd_demo_list_folders() {
    handle_toggle(
        demo_list_folders::NAME,
        &demo_list_folders::ENABLED,
        demo_list_folders::status,
    );
}

/// `dodstudio_overviewmap [full|mini <x> <y> <w> <h>] [default]`.
///
/// A command rather than a cvar: four numbers and a name do not fit in one
/// value, and two cvars per rectangle would be eight names in the type-ahead
/// for something set once.
unsafe extern "C" fn cmd_overviewmap() {
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    let argc = unsafe { (engfuncs.cmd_argc)() };
    let argv = |n: i32| -> Option<String> {
        let raw = unsafe { (engfuncs.cmd_argv)(n) };
        (!raw.is_null()).then(|| {
            unsafe { CStr::from_ptr(raw as *const c_char) }
                .to_string_lossy()
                .trim()
                .to_owned()
        })
    };

    if argc < 2 {
        console_print(&format!(
            "usage: {OVERVIEWMAP_NAME} <full|mini> <x> <y> <w> <h>\n       {OVERVIEWMAP_NAME} default   (let the game place them again)\n{}",
            overview_map::listing()
        ));
        return;
    }

    let Some(first) = argv(1) else { return };
    if first.eq_ignore_ascii_case("default") {
        overview_map::release();
        console_print(&format!(
            "{OVERVIEWMAP_NAME}: released; the next resolution change or level load recomputes them\n"
        ));
        unsafe { crate::debug::report(&format!("commands: {OVERVIEWMAP_NAME} default")) };
        return;
    }

    let Some(which) = overview_map::Which::parse(&first) else {
        console_print(&format!(
            "{OVERVIEWMAP_NAME}: no rect called \"{first}\" -- try full, mini or default\n"
        ));
        return;
    };
    if argc < 6 {
        console_print(&format!(
            "{OVERVIEWMAP_NAME}: {} needs four numbers -- x y w h\n",
            which.name()
        ));
        return;
    }

    let mut fields = [0_i32; 4];
    for (index, slot) in fields.iter_mut().enumerate() {
        let Some(raw) = argv(2 + index as i32) else {
            return;
        };
        match raw.parse::<i32>() {
            Ok(value) => *slot = value,
            Err(_) => {
                console_print(&format!("{OVERVIEWMAP_NAME}: \"{raw}\" is not a number\n"));
                return;
            }
        }
    }
    let rect = overview_map::Rect {
        x: fields[0],
        y: fields[1],
        w: fields[2],
        h: fields[3],
    };

    match overview_map::hold(which, rect).and_then(|()| overview_map::apply()) {
        Ok(_) => {
            console_print(&format!(
                "{OVERVIEWMAP_NAME} {} = {rect}   (shown while _cl_minimap is {})\n",
                which.name(),
                which.mode()
            ));
            unsafe {
                crate::debug::report(&format!(
                    "commands: {OVERVIEWMAP_NAME} {} = {rect}",
                    which.name()
                ))
            };
        }
        Err(why) => {
            console_print(&format!("{OVERVIEWMAP_NAME}: {why}\n"));
            unsafe {
                crate::debug::report(&format!("commands: {OVERVIEWMAP_NAME} failed -- {why}"))
            };
        }
    }
}

unsafe extern "C" fn cmd_log_held_models() {
    handle_toggle(HELD_MODELS_NAME, &anim_fix::LOG_HELD_MODELS, || {
        "logs the third-person model the spectated player holds, each time it changes".into()
    });
}

/// Issue #206's diagnostic -- see `spectator_target.rs`'s module doc.
unsafe extern "C" fn cmd_log_spectator_target() {
    handle_toggle(SPECTATOR_TARGET_LOG_NAME, &spectator_target::LOG, || {
        "logs CHudSpectator's own target alongside the engine's rendered viewmodel entity, whenever either changes".into()
    });
}

/// How far boosted gunshots carry. Separate from the on/off toggle because it
/// is the value you actually want to sweep while listening.
unsafe extern "C" fn cmd_gunshot_attenuation() {
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };

    if unsafe { (engfuncs.cmd_argc)() } >= 2 {
        let arg1 = unsafe { (engfuncs.cmd_argv)(1) };
        if !arg1.is_null() {
            let raw = unsafe { CStr::from_ptr(arg1 as *const c_char) }
                .to_string_lossy()
                .into_owned();
            match raw.trim().parse::<f32>() {
                Ok(value) => match sound_fix::set_carry_attenuation(value) {
                    Ok(()) => {
                        console_print(&format!("{ATTENUATION_NAME} = {value}\n"));
                        unsafe {
                            crate::debug::report(&format!(
                                "commands: {ATTENUATION_NAME} = {value} (set)"
                            ))
                        };
                        return;
                    }
                    Err(why) => {
                        console_print(&format!("{ATTENUATION_NAME}: {why}\n"));
                        unsafe {
                            crate::debug::report(&format!(
                                "commands: {ATTENUATION_NAME} rejected \"{raw}\" -- {why}"
                            ))
                        };
                        return;
                    }
                },
                Err(_) => {
                    console_print(&format!(
                        "{ATTENUATION_NAME}: expected a number, got \"{raw}\"\n"
                    ));
                    return;
                }
            }
        }
    }

    console_print(&format!(
        "{ATTENUATION_NAME} = {}\nusage: {ATTENUATION_NAME} <0.05..0.79>  (lower carries further; the game's own default is 0.8)\n",
        sound_fix::carry_attenuation()
    ));
}

/// Registers one command under several names at once.
///
/// The engine keeps each name independently, so every entry becomes a working
/// spelling of the same command -- which is how a rename keeps the old name
/// alive for a release, and how a variant spelling is added without touching
/// the handler. See `names.rs`.
pub(crate) fn add_commands(names: &[&str], function: engine::ConsoleCommandFn) {
    for name in names {
        add_command(name, function);
    }
}

fn add_command(name: &str, function: engine::ConsoleCommandFn) {
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    let Ok(c_name) = CString::new(name) else {
        return;
    };
    unsafe { (engfuncs.pfn_add_command)(c_name.as_ptr(), function) };
    // Leak intentionally: pfnAddCommand keeps this pointer for the life of the
    // engine session, the same lifetime as the DLL itself.
    std::mem::forget(c_name);
}

fn install_fallback_commands() {
    add_command(GUNSHOTS_FIX_NAME, cmd_gunshots_fix);
    add_command(ANIMATION_FIX_NAME, cmd_animation_fix);
    add_command(ATTENUATION_NAME, cmd_gunshot_attenuation);
    add_command(HELD_MODELS_NAME, cmd_log_held_models);
    add_command(SPECTATOR_TARGET_LOG_NAME, cmd_log_spectator_target);
    add_command(SCOREBOARD_NAME, cmd_scoreboard);
    add_command(VOICE_NAME, cmd_voice);
    add_command(CROSSHAIR_NAME, cmd_crosshair);
    add_command(SPECTATOR_CROSSHAIR_NAME, cmd_spectator_crosshair);
    add_command(HAND_SIGNALS_NAME, cmd_hand_signals);
    add_command(TEXTURE_HIRES_LOG_NAME, cmd_texture_hires_log);
    // Without this, `poll`'s cvar-independent half (hudelement, deathmsg,
    // spectator_target, msglog) never ran on the fallback path either --
    // see issue #324. `poll` itself stays a no-op for the nine cvar-backed
    // commands above, since `CVARS_LIVE` is never set on this path; they
    // apply synchronously from their own command handler instead.
    engine::set_per_frame_prologue(poll);
    unsafe {
        crate::debug::report(&format!(
            "commands: fell back to plain commands -- {GUNSHOTS_FIX_NAME}, {ANIMATION_FIX_NAME}, {ATTENUATION_NAME}, {HELD_MODELS_NAME}, {SPECTATOR_TARGET_LOG_NAME}, {SCOREBOARD_NAME}, {VOICE_NAME}, {CROSSHAIR_NAME}, {SPECTATOR_CROSSHAIR_NAME} (no type-ahead value, no .cfg or launch-line setting)"
        ))
    };
}

/// Registers the console surface. Must be called after `engine::engfuncs()`
/// returns `Some`.
///
/// The defaults handed to the engine are whatever the environment variables
/// already put in the flags, so `GOLDSRC_HOOKS_ANIM_FIX=1` keeps working
/// exactly as before — and a value in a `.cfg` or on the launch line, applied
/// after registration, wins over it.
pub fn install() {
    if engine::engfuncs().is_none() {
        unsafe {
            crate::debug::report(
                "commands::install called before engfuncs were captured -- this is a bug in install ordering",
            )
        };
        return;
    }

    // `dodstudio_debug_status` is a command under either path: it takes no value, so
    // there is nothing for a cvar to hold.
    add_command(STATUS_NAME, cmd_status);

    // Always commands, never cvars: both have subcommands and a variable
    // number of arguments, which a cvar's single value cannot carry.
    add_commands(crate::deathmsg::COMMAND_NAMES, crate::deathmsg::command);
    add_commands(crate::msglog::COMMAND_NAMES, crate::msglog::command);
    add_commands(crate::objicons::COMMAND_NAMES, crate::objicons::command);
    add_commands(
        crate::hide_sprite::COMMAND_NAMES,
        crate::hide_sprite::command,
    );
    add_commands(
        texture_hires::MISSES_COMMAND_NAMES,
        texture_hires::misses_command,
    );
    add_command(HUDELEMENT_NAME, cmd_hudelement);
    add_command(CLEAR_DECALS_NAME, cmd_clear_decals);
    add_command(OVERVIEWMAP_NAME, cmd_overviewmap);

    // Standalone, like `dodstudio_hd_enabled`: the hooks read it when the
    // Load Demo window asks for its list, so it needs no poll, and a failed
    // registration costs only this one setting's type-ahead -- a plain toggle
    // stands in for it.
    match register(demo_list_folders::NAME, "0") {
        Some(cvar) => demo_list_folders::set_cvar(cvar),
        None => add_command(demo_list_folders::NAME, cmd_demo_list_folders),
    }

    let bit = |flag: bool| if flag { "1" } else { "0" };
    let gunshots = register(
        GUNSHOTS_FIX_NAME,
        bit(sound_fix::ENABLED.load(Ordering::Relaxed)),
    );
    let animation = register(ANIMATION_FIX_NAME, &anim_fix::level().to_string());
    let attenuation = register(
        ATTENUATION_NAME,
        &sound_fix::carry_attenuation().to_string(),
    );
    let held_models = register(
        HELD_MODELS_NAME,
        bit(anim_fix::LOG_HELD_MODELS.load(Ordering::Relaxed)),
    );
    let spectator_target_log = register(
        SPECTATOR_TARGET_LOG_NAME,
        bit(spectator_target::LOG.load(Ordering::Relaxed)),
    );
    // These three default to the game's own behaviour. Nothing this DLL does
    // should change what a session looks like until it is asked to -- which is
    // why the mute defaults to 0 while the other two default to 1.
    let scoreboard_cvar = register(SCOREBOARD_NAME, "0");
    let voice_cvar = register(VOICE_NAME, "0");
    let crosshair_cvar = register(CROSSHAIR_NAME, "0");
    let spectator_crosshair_cvar = register(SPECTATOR_CROSSHAIR_NAME, "0");
    let hand_signals_cvar = register(HAND_SIGNALS_NAME, "0");
    // Defaults to the engine's own ceiling, so registering it changes nothing
    // until someone asks for more.
    let ex_interp_cvar = register(EX_INTERP_NAME, &ex_interp::STOCK_MS.to_string());
    // Independent of whether the hook itself installed this session (gated
    // separately by GOLDSRC_HOOKS_TEXTURE_HIRES) -- registering the cvar
    // either way means the type-ahead and .cfg/launch-line all work the same
    // as every other setting here, even before the hook is proven enough to
    // default on.
    // A string, read once when the HD folders are first indexed (the first map
    // load), so it sits outside the polled set below -- and outside the
    // all-or-nothing tuple, since the HD textures fall back to their default
    // style without it.
    if let Some(style) = register(texture_hires::STYLE_NAME, texture_hires::DEFAULT_STYLE) {
        texture_hires::set_style_cvar(style);
    }
    // Same for the HD switch; texture_hires::poll_hd follows it, and installs
    // the hook if it's turned on in a session that started without it.
    if let Some(hd) = register(texture_hires::HD_NAME, bit(texture_hires::enabled())) {
        texture_hires::set_hd_cvar(hd);
    }
    let texture_hires_log_cvar = register(
        TEXTURE_HIRES_LOG_NAME,
        bit(texture_hires::LOG_TEXTURE_LOADS.load(Ordering::Relaxed)),
    );

    let (
        Some(gunshots),
        Some(animation),
        Some(attenuation),
        Some(held_models),
        Some(spectator_target_log),
        Some(scoreboard_cvar),
        Some(voice_cvar),
        Some(crosshair_cvar),
        Some(spectator_crosshair_cvar),
        Some(hand_signals_cvar),
        Some(ex_interp_cvar),
        Some(texture_hires_log_cvar),
    ) = (
        gunshots,
        animation,
        attenuation,
        held_models,
        spectator_target_log,
        scoreboard_cvar,
        voice_cvar,
        crosshair_cvar,
        spectator_crosshair_cvar,
        hand_signals_cvar,
        ex_interp_cvar,
        texture_hires_log_cvar,
    )
    else {
        install_fallback_commands();
        return;
    };

    CVAR_GUNSHOTS.store(gunshots, Ordering::Relaxed);
    CVAR_ANIMATION.store(animation, Ordering::Relaxed);
    CVAR_ATTENUATION.store(attenuation, Ordering::Relaxed);
    CVAR_HELD_MODELS.store(held_models, Ordering::Relaxed);
    CVAR_SPECTATOR_TARGET_LOG.store(spectator_target_log, Ordering::Relaxed);
    CVAR_SCOREBOARD.store(scoreboard_cvar, Ordering::Relaxed);
    CVAR_VOICE.store(voice_cvar, Ordering::Relaxed);
    CVAR_CROSSHAIR.store(crosshair_cvar, Ordering::Relaxed);
    CVAR_SPECTATOR_CROSSHAIR.store(spectator_crosshair_cvar, Ordering::Relaxed);
    CVAR_HAND_SIGNALS.store(hand_signals_cvar, Ordering::Relaxed);
    CVAR_EX_INTERP.store(ex_interp_cvar, Ordering::Relaxed);
    CVAR_TEXTURE_HIRES_LOG.store(texture_hires_log_cvar, Ordering::Relaxed);
    CVARS_LIVE.store(true, Ordering::Release);
    engine::set_per_frame_prologue(poll);

    unsafe {
        crate::debug::report(&format!(
            "commands: registered cvars {GUNSHOTS_FIX_NAME}, {ANIMATION_FIX_NAME}, {ATTENUATION_NAME}, {HELD_MODELS_NAME}, {SPECTATOR_TARGET_LOG_NAME}, {SCOREBOARD_NAME}, {VOICE_NAME}, {CROSSHAIR_NAME}, {SPECTATOR_CROSSHAIR_NAME}, {HAND_SIGNALS_NAME}, {EX_INTERP_NAME}, {TEXTURE_HIRES_LOG_NAME} and command {STATUS_NAME}"
        ))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tripwire, not a style check. `FCVAR_ARCHIVE` would have the engine
    /// write these into the user's `config.cfg`, and the pipeline's standing
    /// rule is that the game's own `.cfg` files are detected and warned about,
    /// never written. Flipping this is a decision, so it should not be possible
    /// to make it by accident.
    #[test]
    fn cvars_are_not_archived() {
        const FCVAR_ARCHIVE: i32 = 1;
        assert_eq!(CVAR_FLAGS & FCVAR_ARCHIVE, 0);
    }

    /// The status reply deliberately no longer lists the settings -- the
    /// console's own type-ahead shows each cvar and its value as you type it,
    /// so repeating them here was duplicated output that wrapped badly in a
    /// narrow console. What it must still do is report the two fixes that have
    /// preconditions, and say *something* when neither is on rather than
    /// returning an empty reply that reads as a broken command.
    #[test]
    fn status_reports_suppression_cvars_always_and_fixes_only_with_progress() {
        // anim_fix::LEVEL is also mutated by anim_fix.rs's own tests, and
        // cargo runs a crate's tests in parallel by default -- without this,
        // one of those can flip LEVEL mid-assertion here (issue #321).
        // anim_fix.rs's tests already take the same lock for the same
        // reason; sound_fix::ENABLED has no other test touching it, so it
        // does not need one of its own.
        let _statics = anim_fix::tests::lock_statics();

        let anim = anim_fix::LEVEL.load(Ordering::Relaxed);
        let sound = sound_fix::ENABLED.load(Ordering::Relaxed);

        anim_fix::LEVEL.store(0, Ordering::Relaxed);
        sound_fix::ENABLED.store(false, Ordering::Relaxed);
        let idle = status_text();
        // The suppression cvars and log_weapon_model are always listed, on
        // or off -- that is the whole point of `debug_status` over the
        // bare-name query. deathmsg's own status is always folded in too.
        for name in [
            SCOREBOARD_NAME,
            VOICE_NAME,
            CROSSHAIR_NAME,
            SPECTATOR_CROSSHAIR_NAME,
            HELD_MODELS_NAME,
            SPECTATOR_TARGET_LOG_NAME,
        ] {
            assert!(idle.contains(name), "{name} missing from:\n{idle}");
        }
        assert!(idle.contains("dodstudio_deathmsg"), "{idle}");
        assert!(!idle.contains("viewmodel animations"), "{idle}");
        assert!(!idle.contains("gunshots"), "{idle}");

        sound_fix::ENABLED.store(true, Ordering::Relaxed);
        let gunshots_on = status_text();
        assert!(gunshots_on.contains("gunshots"), "{gunshots_on}");
        // The attenuation value is folded into the gunshots line rather than
        // given its own, since it does nothing while the fix is off.
        assert!(gunshots_on.contains(ATTENUATION_NAME), "{gunshots_on}");

        anim_fix::LEVEL.store(1, Ordering::Relaxed);
        let both = status_text();
        assert!(both.contains("viewmodel animations"), "{both}");
        assert!(both.contains("gunshots"), "{both}");

        // A command has no type-ahead value to read, so this is the only
        // place that says which elements are hidden.
        hudelement::show_all();
        assert!(!status_text().contains("HUD elements"), "{}", status_text());
        hudelement::set_hidden(hudelement::find("saytext").unwrap(), true);
        assert!(status_text().contains("saytext"), "{}", status_text());
        hudelement::show_all();

        anim_fix::LEVEL.store(anim, Ordering::Relaxed);
        sound_fix::ENABLED.store(sound, Ordering::Relaxed);
    }

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

    /// The names have to carry the sense, since the value alone cannot. A cvar
    /// called after its subject (`dodstudio_scoreboard`) leaves the reader to
    /// guess whether 1 means "scoreboard" or "suppress the scoreboard"; one
    /// called after the action does not.
    #[test]
    fn suppression_cvars_are_named_after_the_action() {
        for name in [SCOREBOARD_NAME, CROSSHAIR_NAME, VOICE_NAME] {
            let verb = name.trim_start_matches("dodstudio_");
            assert!(
                verb.starts_with("hide_") || verb.starts_with("mute_"),
                "{name} is named after its subject, not the action it performs"
            );
        }
    }

    /// Issue #324: `install_fallback_commands()` used to add its nine plain
    /// commands and stop, never registering `poll` as the per-frame prologue
    /// -- so `poll`'s cvar-independent half (hudelement, deathmsg,
    /// spectator_target, msglog) silently never ran on a build where cvar
    /// registration fails. `engine::engfuncs()` is `None` in this test
    /// process, so the nine `add_command` calls are themselves no-ops, but
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
