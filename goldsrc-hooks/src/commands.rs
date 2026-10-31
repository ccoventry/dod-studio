//! The `ds_*` console surface: its cvars and commands. `install`
//! (`commands/install.rs`) registers them all, `poll` (`commands/poll.rs`)
//! is the per-frame work that copies the cvars into the flags the crate
//! reads, and `commands/status.rs` is `ds_debug_status`'s report. This file
//! keeps what they share: the names, the cvar pointers, `register` and
//! `add_command(s)`, and `console_print`.
//!
//! ## Why cvars rather than commands
//!
//! These were four `pfnAddCommand` commands, and the difference is not
//! cosmetic. A command is a function the engine calls and forgets; the state
//! lives in this DLL's own atomics, where the console cannot see it. So
//! the animation setting printed nothing in the type-ahead, could not be
//! queried with a bare name the way `sensitivity` can, and — the part that
//! actually mattered — could not be set from a config file or the launch line.
//! That last gap is the entire reason the `GOLDSRC_HOOKS_*` environment
//! variables existed.
//!
//! A cvar is a named box the *engine* owns. It shows up in the type-ahead with
//! its value, answers `ds_spec_match_pov` on its own, takes
//! `+ds_spec_match_pov 1` on the launch line, and can be set from any
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
//! ## One switch for the spectator's first-person view
//!
//! `ds_spec_match_pov` drives four modules at once: the viewmodel's
//! animations, the spectator crosshair, the in-eye camera's height for a
//! prone player, and the gun lowering off screen (`spectator_gun`). The
//! strafing models that would turn round for a frame (`strafe_twitch`) have
//! their own switch, `ds_player_fix_model_flicker`: that is a bug in how the
//! game draws, which HLTV only shows more often (#927). The gunshots a demo
//! lost have their own too, `ds_sound_restore_gunshots`: POV demos lose
//! them as well, so it is not about matching POV, and it runs in both
//! (#1036). The viewmodel's animations, the lost gunshots and the crosshair
//! used to have a cvar of their own
//! (`dodstudio_hltv_show_viewmodel_animations`,
//! `dodstudio_hltv_play_missing_gunshots`, `dodstudio_match_pov_crosshair`).
//! What stays answers one question -- should a spectated first-person view
//! look like the player's own recording -- and nobody wanted one part
//! without the others, so a fix of that kind joins this cvar instead of
//! adding a name. `dodstudio_hltv_gunshots_fix` and
//! `dodstudio_hltv_gunshot_attenuation` went at the same time, for the
//! opposite reason: making gunfire carry further than POV hears it is not
//! matching POV (`fire_sounds.rs`).
//!
//! Registration must happen after `engine::engfuncs()` is captured (i.e. after
//! `client.dll`'s real `Initialize` has run), since both `pfnRegisterVariable`
//! and `pfnAddCommand` live on that same table.

use std::ffi::CString;

use crate::engine::{self, CvarSPartial};
use crate::names::console_name;
use crate::setting::Cvar;
use crate::{
    crosshair, decals, ex_interp, hand_signals, overview_map, scoreboard, texture_hires, voice,
};

mod install;
mod poll;
mod status;

pub use install::install;
pub use poll::poll;

/// Viewmodel animations, the spectator crosshair, eye height and gun lowering
/// together.
const SPEC_MATCH_POV_NAME: &str = crate::names::SPEC_MATCH_POV;
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
const CLEAR_DECALS_NAME: &str = decals::NAME;
const HAND_SIGNALS_NAME: &str = hand_signals::NAME;
const EX_INTERP_NAME: &str = ex_interp::NAME;
const OVERVIEWMAP_NAME: &str = overview_map::NAME;
const TEXTURE_HIRES_LOG_NAME: &str = texture_hires::NAME;

/// `FCVAR_ARCHIVE` is 1. Deliberately not set — see the module docs.
const CVAR_FLAGS: i32 = 0;

/// The cvars the engine handed back, read once per frame by `poll`.
static CVAR_SPEC_MATCH_POV: Cvar = Cvar::new();
static CVAR_HELD_MODELS: Cvar = Cvar::new();
static CVAR_SPECTATOR_TARGET_LOG: Cvar = Cvar::new();
static CVAR_SCOREBOARD: Cvar = Cvar::new();
static CVAR_VOICE: Cvar = Cvar::new();
static CVAR_CROSSHAIR: Cvar = Cvar::new();
static CVAR_HAND_SIGNALS: Cvar = Cvar::new();
static CVAR_EX_INTERP: Cvar = Cvar::new();
static CVAR_TEXTURE_HIRES_LOG: Cvar = Cvar::new();

pub(crate) fn console_print(text: &str) {
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    // The engine cuts one print at about 1 KB: `ds_hud_deathmsg`'s usage
    // stopped mid-word after ~1000 bytes (#1021). So long text goes over in
    // pieces, split after a newline where it can be.
    for piece in console_pieces(text, CONSOLE_PIECE) {
        let Ok(c_text) = CString::new(piece) else {
            return;
        };
        unsafe { (engfuncs.pfn_console_print)(c_text.as_ptr()) };
    }
}

/// Bytes per `pfnConsolePrint` call, under the engine's ~1 KB cut.
const CONSOLE_PIECE: usize = 900;

/// `text` in pieces of at most `max` bytes, each ending after a newline when
/// there is one in reach, never inside a UTF-8 character.
fn console_pieces(text: &str, max: usize) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut rest = text;
    while rest.len() > max {
        let mut cut = max;
        while !rest.is_char_boundary(cut) {
            cut -= 1;
        }
        if let Some(newline) = rest[..cut].rfind('\n') {
            cut = newline + 1;
        }
        pieces.push(&rest[..cut]);
        rest = &rest[cut..];
    }
    if !rest.is_empty() || pieces.is_empty() {
        pieces.push(rest);
    }
    pieces
}

/// Registers one cvar and checks the engine handed back what was asked for.
///
/// The name check is the whole point: it costs one `strcmp` at startup and it
/// is the only evidence available at runtime that `CvarSPartial`'s layout and
/// the engine's agree. A wrong layout would otherwise show up as a toggle that
/// silently does nothing. On `None` the caller leaves the setting
/// unregistered (no stand-in command, #1208), and the line logged here says
/// which name failed and why, so a user's hook log explains a missing setting.
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
                    "commands: registered {name} but the cvar came back named {:?} -- cvar_s layout is wrong, leaving it unregistered",
                    other.as_deref()
                ))
            };
            None
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::missing_shots;

    #[test]
    fn long_console_text_splits_after_newlines_and_loses_nothing() {
        let text = "aaaa\nbbbb\ncccc\n";
        assert_eq!(console_pieces(text, 11), vec!["aaaa\nbbbb\n", "cccc\n"]);
        assert_eq!(console_pieces("short", 900), vec!["short"]);
        let long = "x".repeat(25);
        let pieces = console_pieces(&long, 10);
        assert_eq!(pieces.concat(), long);
        assert!(pieces.iter().all(|p| p.len() <= 10));
        let wide = "é".repeat(10);
        assert_eq!(console_pieces(&wide, 5).concat(), wide);
    }

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

    /// The console's type-ahead completes to the longest match, so a name
    /// that starts with `ds_spec_match_pov` would be offered in its
    /// place. A fix that joins this switch must not bring a name like that.
    #[test]
    fn nothing_else_starts_with_the_spec_match_pov_name() {
        assert_eq!(SPEC_MATCH_POV_NAME, "ds_spec_match_pov");
        for other in [
            HELD_MODELS_NAME,
            SPECTATOR_TARGET_LOG_NAME,
            STATUS_NAME,
            SCOREBOARD_NAME,
            VOICE_NAME,
            CROSSHAIR_NAME,
            CLEAR_DECALS_NAME,
            HAND_SIGNALS_NAME,
            EX_INTERP_NAME,
            OVERVIEWMAP_NAME,
            TEXTURE_HIRES_LOG_NAME,
            crate::spectator_bars::NAME,
            crate::world_shaders::NAME,
            crate::spectator_follow::LOCK_NAME,
            crate::spectator_follow::TARGET_NAME,
            crate::spectator_deathcam::NAME,
            crate::overview_players::NAME,
            crate::overview_marker::NAME,
            crate::overview_speaker::NAME,
            crate::center_text::NAME,
            crate::spectator_text::NAME,
            crate::plugin_text::NAME,
            crate::demo_scope::NAME,
            crate::run_in_background::NAME,
            crate::frame_times::NAME,
            crate::hlae_window::CLOAK_NAME,
            crate::pov_live_bob::NAME,
            crate::pov_live_bob::LOG_NAME,
            crate::own_blood::NAME,
            crate::map_weather::NAME,
            crate::map_sounds::VICTORY_MUSIC_NAME,
            crate::map_sounds::FLAG_CAPTURE_NAME,
            crate::map_sounds::LOG_NAME,
            crate::screen_effects::FLASH_NAME,
            crate::screen_effects::SHAKE_NAME,
            crate::hlae_window::NAME,
            crate::detail_cache::KEEP_NAME,
            crate::strafe_twitch::NAME,
        ] {
            assert!(!other.starts_with(SPEC_MATCH_POV_NAME), "{other}");
        }
    }

    /// The console's autocomplete swaps a name that is the whole start of
    /// another for the longer one on space, so none of #677's names, nor
    /// #927's, may be the start of any name in the crate, or have one as its
    /// start.
    #[test]
    fn the_hide_text_names_are_no_other_names_start() {
        let ours = [
            crate::overview_speaker::NAME,
            crate::center_text::NAME,
            crate::spectator_text::NAME,
            crate::plugin_text::NAME,
            crate::teammate_info::NAME,
            crate::hint_panel::NAME,
            crate::strafe_twitch::NAME,
            missing_shots::NAME,
            crate::own_blood::NAME,
            crate::map_weather::NAME,
        ];
        let mut all: Vec<String> = Vec::new();
        for entry in std::fs::read_dir("src")
            .unwrap()
            .chain(std::fs::read_dir("src/anim_fix").unwrap())
        {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for part in text.split("console_name!(\"").skip(1) {
                let name = part.split('"').next().unwrap();
                // `console_name!("")` is the bare prefix, not a name.
                if name.is_empty() {
                    continue;
                }
                all.push(format!("{}{name}", console_name!("")));
            }
        }
        assert!(all.len() > 50, "found only {} names", all.len());
        for a in ours {
            for b in &all {
                if a == b {
                    continue;
                }
                assert!(!b.starts_with(a), "{a} is the start of {b}");
                assert!(!a.starts_with(b.as_str()), "{b} is the start of {a}");
            }
        }
    }

    /// The names have to carry the sense, since the value alone cannot. A cvar
    /// called after its subject (`ds_scoreboard`) leaves the reader to
    /// guess whether 1 means "scoreboard" or "suppress the scoreboard"; one
    /// called after the action does not.
    #[test]
    fn suppression_cvars_are_named_after_the_action() {
        for name in [SCOREBOARD_NAME, CROSSHAIR_NAME, VOICE_NAME] {
            // After the prefix comes the area (`hud_`, `sound_`, #555).
            let rest = name.trim_start_matches(console_name!(""));
            let verb = rest.split_once('_').map_or(rest, |(_, verb)| verb);
            assert!(
                verb.starts_with("hide_") || verb.starts_with("mute_"),
                "{name} is named after its subject, not the action it performs"
            );
        }
    }
}
