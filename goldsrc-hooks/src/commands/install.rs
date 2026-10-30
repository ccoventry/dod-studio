//! `install`: registers every `ds_*` cvar and command, in the order the
//! console's cvar list and type-ahead show them.

use std::sync::atomic::Ordering;

use super::fallback::*;
use super::poll::poll;
use super::status::cmd_status;
use super::*;
use crate::engine;
use crate::{
    anim_fix, decals, demo_list_folders, demo_other_drives, demo_seek, ex_interp, missing_shots,
    overview_map, spectator_target, texture_hires, window_layout,
};

/// Registers the console surface. Must be called after `engine::engfuncs()`
/// returns `Some`.
///
/// The defaults handed to the engine are whatever the environment variables
/// already put in the flags, so `GOLDSRC_HOOKS_SPEC_MATCH_POV=1` starts the session
/// with it on — and a value in a `.cfg` or on the launch line, applied after
/// registration, wins over it.
pub fn install() {
    if engine::engfuncs().is_none() {
        unsafe {
            crate::debug::report(
                "commands::install called before engfuncs were captured -- this is a bug in install ordering",
            )
        };
        return;
    }

    // `ds_debug_status` is a command under either path: it takes no value, so
    // there is nothing for a cvar to hold.
    add_command(STATUS_NAME, cmd_status);

    // Always commands, never cvars: both have subcommands and a variable
    // number of arguments, which a cvar's single value cannot carry.
    add_commands(crate::deathmsg::COMMAND_NAMES, crate::deathmsg::command);
    add_commands(crate::msglog::COMMAND_NAMES, crate::msglog::command);
    add_commands(crate::objicons::COMMAND_NAMES, crate::objicons::command);
    add_commands(crate::hide_asset::COMMAND_NAMES, crate::hide_asset::command);
    add_commands(
        texture_hires::MISSES_COMMAND_NAMES,
        texture_hires::misses_command,
    );
    add_command(CLEAR_DECALS_NAME, decals::command);
    add_command(crate::demo_reload::NAME, crate::demo_reload::command);
    crate::demo_reload::install();
    crate::connect_guard::install();
    crate::events::install();
    // The rest of what a review locks is wrapped by its poll as it appears.
    crate::review_lock::console::install();
    add_command(OVERVIEWMAP_NAME, overview_map::command);
    add_command(demo_seek::SEEK_TO_NAME, demo_seek::seek_to);
    add_command(demo_seek::SEEK_BY_NAME, demo_seek::seek_by);
    add_command(crate::studio_panel::NAME, crate::studio_panel::command);
    add_command(crate::review::NAME, crate::review::command);
    add_command(crate::schedule::NAME, crate::schedule::command);
    add_command(crate::batch::NAME, crate::batch::command);
    add_command(crate::spray::NAME, crate::spray::command);
    add_command(crate::player_names::NAME, crate::player_names::command);

    // Standalone, like `ds_hd_enabled`: the seek reads it when it runs,
    // so it needs no poll, and a failed registration costs only this one
    // setting's type-ahead -- a plain toggle stands in for it.
    match register(demo_seek::SKIP_BETWEEN_NAME, "0") {
        Some(cvar) => demo_seek::SKIP_BETWEEN.set_cvar(cvar),
        None => add_command(demo_seek::SKIP_BETWEEN_NAME, cmd_seek_skip_between),
    }
    add_command(
        crate::spectator_follow::TARGET_NAME,
        crate::spectator_follow::target_command,
    );
    add_command(crate::position::NAME, crate::position::command);

    // Standalone, like `ds_hd_enabled`: the hooks read it when the
    // Load Demo window asks for its list, so it needs no poll, and a failed
    // registration costs only this one setting's type-ahead -- a plain toggle
    // stands in for it.
    match register(demo_list_folders::NAME, "1") {
        Some(cvar) => demo_list_folders::FOLDERS.set_cvar(cvar),
        None => add_command(demo_list_folders::NAME, cmd_demo_list_folders),
    }
    // Read by the file system's Open hook on a `D:/...` name (#776).
    match register(demo_other_drives::NAME, "1") {
        Some(cvar) => demo_other_drives::SETTING.set_cvar(cvar),
        None => add_command(demo_other_drives::NAME, cmd_demo_other_drives),
    }
    // The three below default off (0.2.0): each lists more by reading every
    // folder, which slows the window on a big demo library.
    match register(demo_list_folders::HIDE_EMPTY.name, "0") {
        Some(cvar) => demo_list_folders::HIDE_EMPTY.set_cvar(cvar),
        None => add_command(demo_list_folders::HIDE_EMPTY.name, cmd_demo_list_hide_empty),
    }
    match register(demo_list_folders::COUNT_SUBFOLDERS.name, "0") {
        Some(cvar) => demo_list_folders::COUNT_SUBFOLDERS.set_cvar(cvar),
        None => add_command(
            demo_list_folders::COUNT_SUBFOLDERS.name,
            cmd_demo_list_count_subfolders,
        ),
    }
    // On by default (#798); read by fast_load::poll each frame.
    match register(crate::fast_load::NAME, "1") {
        Some(cvar) => crate::fast_load::SETTING.set_cvar(cvar),
        None => add_command(crate::fast_load::NAME, cmd_demo_fast_load),
    }
    match register(demo_list_folders::SHOW_COUNTS.name, "0") {
        Some(cvar) => demo_list_folders::SHOW_COUNTS.set_cvar(cvar),
        None => add_command(
            demo_list_folders::SHOW_COUNTS.name,
            cmd_demo_list_show_counts,
        ),
    }

    // Standalone, like `ds_hd_enabled`: window_layout reads them where
    // it walks the windows, so they need no poll here, and a failed
    // registration costs only their type-ahead -- plain toggles stand in.
    // Both on by default: a window you can resize and that opens where you
    // left it is a win for everyone.
    match (
        register(window_layout::RESIZABLE_NAME, "1"),
        register(window_layout::REMEMBER_NAME, "1"),
    ) {
        (Some(resizable), Some(remember)) => {
            window_layout::RESIZABLE.set_cvar(resizable);
            window_layout::REMEMBER.set_cvar(remember);
        }
        _ => {
            add_command(window_layout::RESIZABLE_NAME, cmd_resizable_windows);
            add_command(window_layout::REMEMBER_NAME, cmd_remember_window_layout);
        }
    }

    // Both on by default: the window is how DoD
    // Studio's console and playback controls are reached.
    match register(crate::studio_panel::VIEWDEMO_NAME, "1") {
        Some(cvar) => crate::studio_panel::VIEWDEMO_IN_PANEL.set_cvar(cvar),
        None => add_command(crate::studio_panel::VIEWDEMO_NAME, cmd_viewdemo_in_panel),
    }
    match register(crate::studio_panel::CONSOLE_NAME, "1") {
        Some(cvar) => crate::studio_panel::CONSOLE_IN_PANEL.set_cvar(cvar),
        None => add_command(crate::studio_panel::CONSOLE_NAME, cmd_console_in_panel),
    }
    // On by default too: it only shows anything on the layout Studio's own
    // launch ships, so nothing changes for a game started another way.
    match register(crate::demo_bar_progress::NAME, "1") {
        Some(cvar) => crate::demo_bar_progress::SETTING.set_cvar(cvar),
        None => add_command(crate::demo_bar_progress::NAME, cmd_show_load_progress),
    }
    // The review's on-screen panel and answer keys (#660), both on: they
    // only show or take a key while a review runs.
    match register(crate::review_keys::KEYS.name, "1") {
        Some(cvar) => crate::review_keys::KEYS.set_cvar(cvar),
        None => add_command(crate::review_keys::KEYS.name, cmd_review_keys),
    }
    match register(crate::review_keys::INFO.name, "1") {
        Some(cvar) => crate::review_keys::INFO.set_cvar(cvar),
        None => add_command(crate::review_keys::INFO.name, cmd_review_info),
    }

    let bit = |flag: bool| if flag { "1" } else { "0" };
    let spec_match_pov = register(SPEC_MATCH_POV_NAME, &anim_fix::level().to_string());
    let held_models = register(
        HELD_MODELS_NAME,
        bit(anim_fix::LOG_HELD_MODELS.load(Ordering::Relaxed)),
    );
    let spectator_target_log = register(
        SPECTATOR_TARGET_LOG_NAME,
        bit(spectator_target::LOG.load(Ordering::Relaxed)),
    );
    // These four default to the game's own behaviour (0): hiding part of the
    // HUD is a big change not everyone wants, so it waits to be asked for.
    let scoreboard_cvar = register(SCOREBOARD_NAME, "0");
    let voice_cvar = register(VOICE_NAME, "0");
    let crosshair_cvar = register(CROSSHAIR_NAME, "0");
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
    // Outside the all-or-nothing tuple for the same reason as the HD switch:
    // it is independent of every other setting here.
    if let Some(gun_log) = register(crate::gun_log::NAME, "0") {
        crate::gun_log::set_cvar(gun_log);
    }
    if let Some(shaders) = register(crate::world_shaders::NAME, "0") {
        crate::world_shaders::set_cvar(shaders);
    }
    // Same: independent of every other setting, so outside the tuple.
    if let Some(map_text) = register(crate::map_text::NAME, "0") {
        crate::map_text::CVAR.set(map_text);
    }
    if let Some(clan_text) = register(crate::clan_text::NAME, "0") {
        crate::clan_text::CVAR.set(clan_text);
    }
    if let Some(center_text) = register(crate::center_text::NAME, "0") {
        crate::center_text::CVAR.set(center_text);
    }
    if let Some(spectator_text) = register(crate::spectator_text::NAME, "0") {
        crate::spectator_text::CVAR.set(spectator_text);
    }
    if let Some(plugin_text) = register(crate::plugin_text::NAME, "0") {
        crate::plugin_text::CVAR.set(plugin_text);
    }
    if let Some(teammate_info) = register(crate::teammate_info::NAME, "0") {
        crate::teammate_info::CVAR.set(teammate_info);
    }
    // On by default: the hint panel is the recorder's own tutorial and
    // covers a corner of the shot (#286).
    if let Some(hints) = register(crate::hint_panel::NAME, "1") {
        crate::hint_panel::CVAR.set(hints);
    }
    if let Some(voice_menus) = register(crate::voice_menus::NAME, "0") {
        crate::voice_menus::CVAR.set(voice_menus);
    }
    // Same again: read by the HUD_DirectorMessage trampoline itself, not
    // polled.
    if let Some(hltv_messages) = register(crate::hltv_messages::NAME, "0") {
        crate::hltv_messages::CVAR.set(hltv_messages);
    }
    // The same: a switch of its own, off until asked for.
    if let Some(bars) = register(crate::spectator_bars::NAME, "0") {
        crate::spectator_bars::CVAR.set(bars);
    }
    if let Some(lock) = register(crate::spectator_follow::LOCK_NAME, "0") {
        crate::spectator_follow::set_cvar(lock);
    }
    if let Some(deathcam) = register(crate::spectator_deathcam::NAME, "0") {
        crate::spectator_deathcam::set_cvar(deathcam);
    }
    if let Some(icons) = register(crate::overview_players::NAME, "0") {
        crate::overview_players::set_cvar(icons);
    }
    if let Some(marker) = register(crate::overview_marker::NAME, "0") {
        crate::overview_marker::set_cvar(marker);
    }
    if let Some(speaker) = register(crate::overview_speaker::NAME, "0") {
        crate::overview_speaker::set_cvar(speaker);
    }
    // On by default: a model turned round for a frame is wrong in any demo
    // (#927). A failed registration falls back to a plain toggle.
    match register(crate::strafe_twitch::NAME, "1") {
        Some(flicker) => crate::strafe_twitch::FIX_MODEL_FLICKER.set_cvar(flicker),
        None => add_command(crate::strafe_twitch::NAME, cmd_fix_model_flicker),
    }
    // On by default: a round fired in view should be heard (#1036). One
    // switch for every gunshot path, read by `missing_shots::poll` and
    // `pov_recorder_shots::poll` themselves; `2` also logs every round.
    match register(missing_shots::NAME, "1") {
        Some(restore) => missing_shots::SETTING.set_cvar(restore),
        None => add_command(missing_shots::NAME, cmd_restore_gunshots),
    }
    // On by default: a demo then shows the sniper scope the player saw.
    if let Some(scope) = register(crate::demo_scope::NAME, "1") {
        crate::demo_scope::set_cvar(scope);
    }
    if let Some(background) = register(crate::run_in_background::NAME, "0") {
        crate::run_in_background::set_cvar(background);
    }
    if let Some(times) = register(crate::frame_times::NAME, "0") {
        crate::frame_times::set_cvar(times);
    }
    if let Some(cloak) = register(crate::hlae_window::CLOAK_NAME, "0") {
        crate::hlae_window::set_cloak_cvar(cloak);
    }
    if let Some(bob) = register(crate::pov_live_bob::NAME, "0") {
        crate::pov_live_bob::set_cvar(bob);
    }
    if let Some(log) = register(crate::pov_live_bob::LOG_NAME, "0") {
        crate::pov_live_bob::set_log_cvar(log);
    }
    if let Some(blood) = register(crate::own_blood::NAME, "0") {
        crate::own_blood::set_cvar(blood);
    }
    if let Some(weather) = register(crate::map_weather::NAME, "0") {
        crate::map_weather::set_cvar(weather);
    }
    for family in crate::map_sounds::Family::ALL {
        if let Some(mute) = register(family.setting(), "0") {
            family.set_cvar(mute);
        }
    }
    if let Some(log) = register(crate::map_sounds::LOG_NAME, "0") {
        crate::map_sounds::set_log_cvar(log);
    }
    // Off unless Studio turns it on: a game not started from Studio keeps
    // its own sprays and crosshair (#428).
    if let Some(overrides) = register(crate::player_overrides::NAME, "0") {
        crate::player_overrides::set_cvar(overrides);
    }
    if let (Some(flash), Some(shake)) = (
        register(crate::screen_effects::FLASH_NAME, "0"),
        register(crate::screen_effects::SHAKE_NAME, "0"),
    ) {
        crate::screen_effects::set_cvars(flash, shake);
    }
    if let Some(behind) = register(crate::hlae_window::NAME, "0") {
        crate::hlae_window::set_cvar(behind);
    }
    if let Some(keep) = register(crate::detail_cache::KEEP_NAME, "0") {
        crate::detail_cache::set_keep_cvar(keep);
    }
    let texture_hires_log_cvar = register(
        TEXTURE_HIRES_LOG_NAME,
        bit(texture_hires::LOG_TEXTURE_LOADS.load(Ordering::Relaxed)),
    );

    let (
        Some(spec_match_pov),
        Some(held_models),
        Some(spectator_target_log),
        Some(scoreboard_cvar),
        Some(voice_cvar),
        Some(crosshair_cvar),
        Some(hand_signals_cvar),
        Some(ex_interp_cvar),
        Some(texture_hires_log_cvar),
    ) = (
        spec_match_pov,
        held_models,
        spectator_target_log,
        scoreboard_cvar,
        voice_cvar,
        crosshair_cvar,
        hand_signals_cvar,
        ex_interp_cvar,
        texture_hires_log_cvar,
    )
    else {
        install_fallback_commands();
        return;
    };

    CVAR_SPEC_MATCH_POV.store(spec_match_pov, Ordering::Relaxed);
    CVAR_HELD_MODELS.store(held_models, Ordering::Relaxed);
    CVAR_SPECTATOR_TARGET_LOG.store(spectator_target_log, Ordering::Relaxed);
    CVAR_SCOREBOARD.store(scoreboard_cvar, Ordering::Relaxed);
    CVAR_VOICE.store(voice_cvar, Ordering::Relaxed);
    CVAR_CROSSHAIR.store(crosshair_cvar, Ordering::Relaxed);
    CVAR_HAND_SIGNALS.store(hand_signals_cvar, Ordering::Relaxed);
    CVAR_EX_INTERP.store(ex_interp_cvar, Ordering::Relaxed);
    CVAR_TEXTURE_HIRES_LOG.store(texture_hires_log_cvar, Ordering::Relaxed);
    CVARS_LIVE.store(true, Ordering::Release);
    engine::set_per_frame_prologue(poll);

    unsafe {
        crate::debug::report(&format!(
            "commands: registered cvars {SPEC_MATCH_POV_NAME}, {HELD_MODELS_NAME}, {SPECTATOR_TARGET_LOG_NAME}, {SCOREBOARD_NAME}, {VOICE_NAME}, {CROSSHAIR_NAME}, {HAND_SIGNALS_NAME}, {EX_INTERP_NAME}, {TEXTURE_HIRES_LOG_NAME} and command {STATUS_NAME}"
        ))
    };
}
