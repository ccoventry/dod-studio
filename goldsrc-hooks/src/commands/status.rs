//! `ds_debug_status`: every setting and what each fix is doing, in one report.

use std::sync::atomic::Ordering;

use super::*;
use crate::{
    anim_fix, crosshair, decals, ex_interp, fire_sounds, hand_signals, hudelement, missing_shots,
    overview_map, scoreboard, spectator_crosshair, spectator_eye, spectator_target, texture_hires,
    voice,
};

/// Everything in one place, for debugging -- not the settings surface a
/// player is expected to type. That is what `debug_` in the name signals:
/// every value here is also visible piecemeal (a suppression cvar's own
/// bare-name query, the console type-ahead, `ds_hud_deathmsg`'s own
/// status), but this is the one command that dumps all of it together, which
/// is what a support question actually needs -- so it covers the *entire*
/// `ds_*` surface, not a subset.
///
/// The suppression cvars and `log_weapon_model` are listed unconditionally,
/// on or off, because there is no progress to gate them on -- they are just
/// a byte, and "what is it set to" is exactly what this command exists to
/// answer without hunting down each bare name individually. The two fixes
/// below them are gated on being enabled, because for *those* a flag being
/// on says nothing about whether the preconditions are being met in the
/// current view, and "the fix isn't working" has twice turned out to be "the
/// log budget ran out" -- their counters are the honest number, and are
/// noise when off.
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
            "{} = {} -- {}",
            crate::map_sounds::VICTORY_MUSIC_NAME,
            bit(crate::map_sounds::Family::VictoryMusic.on()),
            crate::map_sounds::status()
        ),
        format!(
            "{} = {} -- the same svc sound calls; {}",
            crate::map_sounds::FLAG_CAPTURE_NAME,
            bit(crate::map_sounds::Family::FlagCapture.on()),
            crate::map_sounds::map_status()
        ),
        format!(
            "{CROSSHAIR_NAME} = {} -- {}",
            bit(crosshair::hidden()),
            crosshair::status()
        ),
        format!(
            "{SPEC_MATCH_POV_NAME} = {} -- viewmodel animations, the spectator crosshair, the prone and crouch eye height and the gun lowering, as the player's own recording has them",
            bit(anim_fix::enabled())
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
        lines.push(format!(
            "spectator crosshair: {}",
            spectator_crosshair::status()
        ));
        lines.push(format!("spectator eye height: {}", spectator_eye::status()));
        lines.push(format!(
            "look-ahead (crouch ease, sprint let-go): {}",
            crate::lookahead::status()
        ));
        lines.push(format!("spectator gun: {}", crate::spectator_gun::status()));
    }
    lines.push(format!(
        "{} = {} -- strafe twitch: {}",
        crate::strafe_twitch::NAME,
        bit(crate::strafe_twitch::enabled()),
        crate::strafe_twitch::status()
    ));
    lines.push(format!(
        "{} = {} -- rounds an HLTV or POV demo has no fire event for are played (HLTV: every player; POV: the others from the body, the recorder from his clip)",
        missing_shots::NAME,
        bit(missing_shots::enabled()),
    ));
    // Both the animations and the gunshot pairing read this hook.
    if anim_fix::enabled() || missing_shots::enabled() {
        lines.push(format!("gunshot sounds: {}", fire_sounds::status()));
    }
    if let Some(line) = crate::pov_crosshair_gap::status_line() {
        lines.push(line);
    }
    if let Some(deathcam) = crate::spectator_deathcam::status_line() {
        lines.push(deathcam);
    }
    lines.push(crate::deathmsg::status().trim_end().to_string());
    // Always shown (#430): the one fact that decides how big HD textures can
    // get before the game runs out of memory.
    lines.push(if crate::pe::process_is_large_address_aware() {
        "address space: 4 GB (hl.exe is large-address-aware)".to_string()
    } else {
        "address space: 2 GB (hl.exe isn't large-address-aware; very large HD textures can run it out)".to_string()
    });
    // Gated like the two fixes above rather than always shown like the
    // suppression cvars: logging is off by default and a permanent "logging
    // nothing" line would be noise in the overwhelmingly common case.
    if let Some(msglog) = crate::msglog::status_line() {
        lines.push(msglog);
    }
    // Same reasoning as msglog above: hiding is off by default and a
    // permanent "hiding nothing" line would be noise in the common case.
    if let Some(hide_asset) = crate::hide_asset::status_line() {
        lines.push(hide_asset);
    }
    // What ds_hud_hide_teammate_info actually wrote, which its cvar's value
    // alone does not say.
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
    if let Some(lighting) = crate::lightmap_gamma::status_line() {
        lines.push(lighting);
    }
    if let Some(tempent) = crate::tempent_fix::status_line() {
        lines.push(tempent);
    }
    if let Some(map_files) = crate::map_file_leak::status_line() {
        lines.push(map_files);
    }
    if let Some(hull) = crate::hull_trace_guard::status_line() {
        lines.push(hull);
    }
    if let Some(fast_load) = crate::fast_load::status_line() {
        lines.push(fast_load);
    }
    if let Some(blood) = crate::own_blood::status_line() {
        lines.push(blood);
    }
    if let Some(weather) = crate::map_weather::status_line() {
        lines.push(weather);
    }
    lines.push(crate::player_overrides::status());
    if let Some(renames) = crate::player_names::status_line() {
        lines.push(renames);
    }
    if let Some(sprays) = crate::spray_hltv::status_line() {
        lines.push(sprays);
    }
    if let Some(crosshair) = crate::player_crosshair::status_line() {
        lines.push(crosshair);
    }
    if let Some(esc) = crate::frame_esc::status_line() {
        lines.push(esc);
    }
    if let Some(pmove) = crate::pmove_guard::status_line() {
        lines.push(pmove);
    }
    if let Some(events) = crate::events::status_line() {
        lines.push(events);
    }
    if let Some(sprites) = crate::sprite_blend::status_line() {
        lines.push(sprites);
    }
    if let Some(game_dir) = crate::game_dir_check::status_line() {
        lines.push(game_dir);
    }
    if let Some(keys) = crate::demo_console_keys::status_line() {
        lines.push(keys);
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
    if let Some(shots) = missing_shots::status_line() {
        lines.push(shots);
    }
    if let Some(dry_fire) = crate::pov_recorder_shots::status_line() {
        lines.push(dry_fire);
    }
    if let Some(bars) = crate::spectator_bars::status_line() {
        lines.push(bars);
    }
    if let Some(lock) = crate::spectator_follow::status_line() {
        lines.push(lock);
    }
    if let Some(hd) = crate::overview_hd::status_line() {
        lines.push(hd);
    }
    if let Some(icons) = crate::overview_players::status_line() {
        lines.push(icons);
    }
    if let Some(marker) = crate::overview_marker::status_line() {
        lines.push(marker);
    }
    if let Some(speaker) = crate::overview_speaker::status_line() {
        lines.push(speaker);
    }
    if let Some(scope) = crate::demo_scope::status_line() {
        lines.push(scope);
    }
    if let Some(shaders) = crate::world_shaders::status_line() {
        lines.push(shaders);
    }
    if let Some(map_text) = crate::map_text::status_line() {
        lines.push(map_text);
    }
    if let Some(clan_text) = crate::clan_text::status_line() {
        lines.push(clan_text);
    }
    if let Some(center_text) = crate::center_text::status_line() {
        lines.push(center_text);
    }
    if let Some(spectator_text) = crate::spectator_text::status_line() {
        lines.push(spectator_text);
    }
    if let Some(hints) = crate::hint_panel::status_line() {
        lines.push(hints);
    }
    if let Some(plugin_text) = crate::plugin_text::status_line() {
        lines.push(plugin_text);
    }
    if let Some(voice_menus) = crate::voice_menus::status_line() {
        lines.push(voice_menus);
    }
    if let Some(weapon_list) = crate::weapon_list::status_line() {
        lines.push(weapon_list);
    }
    if let Some(hltv_messages) = crate::hltv_messages::status_line() {
        lines.push(hltv_messages);
    }
    if lines.is_empty() {
        // Not an error, and worth saying out loud: the suppressions leave no
        // trace to count, so silence here would read as a broken command.
        return "nothing active that reports progress\n".to_string();
    }
    format!("{}\n", lines.join("\n"))
}

pub(super) unsafe extern "C" fn cmd_status() {
    // A console line's semicolon-joined commands all run together, in one
    // pass, before `poll` gets another turn as the per-frame prologue -- so
    // `ds_hud_hide_scoreboard 1;ds_debug_status` on one line would
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The settings are listed on or off; what each part of matching POV is
    /// actually doing is only reported while it is on, because its counters
    /// are noise otherwise.
    #[test]
    fn status_reports_suppression_cvars_always_and_fixes_only_with_progress() {
        // anim_fix::LEVEL is also mutated by anim_fix's own tests, and
        // cargo runs a crate's tests in parallel by default -- without this,
        // one of those can flip LEVEL mid-assertion here (issue #321).
        // anim_fix's tests already take the same lock for the same reason.
        let _statics = anim_fix::tests::lock_statics();
        let _hidden = hudelement::tests::lock_hidden();

        let anim = anim_fix::LEVEL.load(Ordering::Relaxed);

        anim_fix::LEVEL.store(0, Ordering::Relaxed);
        let idle = status_text();
        // The suppression cvars and log_weapon_model are always listed, on
        // or off -- that is the whole point of `debug_status` over the
        // bare-name query. deathmsg's own status is always folded in too.
        for name in [
            SCOREBOARD_NAME,
            VOICE_NAME,
            CROSSHAIR_NAME,
            SPEC_MATCH_POV_NAME,
            HELD_MODELS_NAME,
            SPECTATOR_TARGET_LOG_NAME,
            // Their own switches (#927, #1036), listed whatever match_pov is.
            crate::strafe_twitch::NAME,
            missing_shots::NAME,
        ] {
            assert!(idle.contains(name), "{name} missing from:\n{idle}");
        }
        assert!(idle.contains("ds_hud_deathmsg"), "{idle}");
        for part in ["viewmodel animations:", "spectator crosshair:"] {
            assert!(!idle.contains(part), "{part} reported while off:\n{idle}");
        }
        // The gunshot hook's counters follow the gunshot switch, on by
        // default, not match_pov.
        assert_eq!(
            idle.contains("gunshot sounds:"),
            missing_shots::enabled(),
            "{idle}"
        );

        // One switch turns its parts on, and each then says what it is doing.
        anim_fix::LEVEL.store(1, Ordering::Relaxed);
        let on = status_text();
        for part in [
            "viewmodel animations:",
            "gunshot sounds:",
            "spectator crosshair:",
        ] {
            assert!(on.contains(part), "{part} missing while on:\n{on}");
        }

        // What hudelement wrote is only said here.
        hudelement::show_all();
        assert!(!status_text().contains("HUD elements"), "{}", status_text());
        hudelement::set_hidden(hudelement::find("statusbar").unwrap(), true);
        assert!(status_text().contains("statusbar"), "{}", status_text());
        hudelement::show_all();

        anim_fix::LEVEL.store(anim, Ordering::Relaxed);
    }
}
