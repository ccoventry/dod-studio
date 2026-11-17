//! How many decals the ring holds (`r_decals`): Initial Commands first, an
//! executed game config next, the configured default last (#604).

use super::{cfg_scan, game_dir_for};
use crate::decals::MAX_RENDER_DECALS;
use crate::patch::types::PatcherConfig;

/// `r_decals` as stated in `init_commands`, if it is stated there at all.
///
/// The last one wins, matching the console. Clamped to the engine's own
/// ceiling, because the engine clamps it too and a sweep sized past the ceiling
/// would spend its extra positions turning a ring that had already come round.
pub fn ring_limit_from_init(init_commands: &[String]) -> Option<u32> {
    for cmd in init_commands.iter().rev() {
        let trimmed = cmd.trim();
        let Some(rest) = trimmed.strip_prefix("r_decals") else {
            continue;
        };
        // Guard against matching a longer command that merely starts the same.
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        // Quoted, same as `capture_fov::capture_fov_from_init` and for the same
        // reason: real .cfg syntax quotes every value, and a raw parse on
        // `"512"` fails silently, reading a stated line as unstated.
        if let Ok(v) = cfg_scan::unquote(rest.trim()).parse::<u32>() {
            return Some(v.min(MAX_RENDER_DECALS));
        }
    }
    None
}

/// How many decals the ring will hold, and so how many positions a full sweep
/// has to turn.
///
/// This is one number, and `r_decals` is where the engine reads it. A separate
/// setting could only ever agree with the cvar or silently disagree with it, so
/// an `init_commands` entry is the authority when there is one, an executed
/// config is next, and the configured default fills in when neither states it.
/// Same precedence as `capture_fov_resolved` — genuinely the same rule now,
/// not merely similar (see that function's doc comment for the history of why
/// `r_decals` used to be the one exception).
pub fn ring_limit(config: &PatcherConfig) -> u32 {
    ring_limit_from_init(&config.init_commands)
        .or_else(|| ring_limit_from_game_config(config))
        .unwrap_or_else(|| config.decal_ring_limit.min(MAX_RENDER_DECALS))
}

/// `r_decals` as an executed config states it, if one does and the value
/// actually parses — same shape as `ring_limit_from_init`, just for the other
/// place a value can come from. Clamped the same way, for the same reason.
///
/// Read-only, like everything touching the user's configs — this decides what
/// the pipeline treats as the standing value, never what the config says.
/// See [`cfg_scan`].
pub fn ring_limit_from_game_config(config: &PatcherConfig) -> Option<u32> {
    let dir = game_dir_for(config)?;
    let setting = cfg_scan::scan_cached(&dir).effective("r_decals")?.clone();
    setting
        .value
        .parse::<u32>()
        .ok()
        .map(|v| v.min(MAX_RENDER_DECALS))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::fake_game;

    #[test]
    fn a_config_that_switches_decals_off_is_now_adopted_like_fov_is() {
        // Regression: this used to assert the opposite — that a config's
        // `r_decals 0` was ignored in favor of the app's own default, on the
        // reasoning that adopting it would silently stand the flush down.
        // Now that stand-down is its own loud, reported fact
        // (decal_flush_is_noop) rather than something to hide by disagreeing
        // with the config, so r_decals follows mirv_fov's precedence exactly:
        // init commands, then an executed config, then the app's default.
        let (_root, exe) = fake_game("decals_off", "exec movie.cfg\n", Some("r_decals \"0\"\n"));
        let config = PatcherConfig {
            game_path: exe.to_string_lossy().to_string(),
            ..PatcherConfig::default()
        };

        assert_eq!(
            ring_limit(&config),
            0,
            "the config's own value now wins, same as mirv_fov's would"
        );
    }

    #[test]
    fn a_nonzero_r_decals_the_game_config_sets_is_adopted() {
        let (_root, exe) = fake_game(
            "decals_from_config",
            "exec movie.cfg\n",
            Some("r_decals \"512\"\n"),
        );
        let config = PatcherConfig {
            game_path: exe.to_string_lossy().to_string(),
            decal_ring_limit: 128,
            ..PatcherConfig::default()
        };

        assert_eq!(
            ring_limit(&config),
            512,
            "the config's value, not the app's own default"
        );
    }

    #[test]
    fn an_init_command_still_outranks_a_game_config_for_r_decals() {
        let (_root, exe) = fake_game(
            "decals_init_outranks",
            "exec movie.cfg\n",
            Some("r_decals \"0\"\n"),
        );
        let config = PatcherConfig {
            game_path: exe.to_string_lossy().to_string(),
            init_commands: vec!["r_decals 512".to_string()],
            ..PatcherConfig::default()
        };

        assert_eq!(
            ring_limit(&config),
            512,
            "stated in Initial Commands, so it wins outright"
        );
    }

    #[test]
    fn the_last_r_decals_wins_like_the_console_does() {
        let config = PatcherConfig {
            init_commands: vec!["r_decals 512".to_string(), "r_decals 64".to_string()],
            ..PatcherConfig::default()
        };

        assert_eq!(ring_limit(&config), 64);
    }

    #[test]
    fn a_quoted_r_decals_in_init_commands_is_still_recognised() {
        // Real .cfg syntax quotes every value, and "Load from .cfg file…"
        // carries that straight into Initial Commands — a raw, unquoted
        // parse would silently read `r_decals "512"` as unstated and fall
        // through to the app's own default instead of the user's value.
        let config = PatcherConfig {
            decal_ring_limit: 128,
            init_commands: vec!["r_decals \"512\"".to_string()],
            ..PatcherConfig::default()
        };

        assert_eq!(ring_limit_from_init(&config.init_commands), Some(512));
        assert_eq!(ring_limit(&config), 512);
    }

    #[test]
    fn the_configured_default_fills_in_when_nothing_states_it() {
        let config = PatcherConfig {
            decal_ring_limit: 128,
            init_commands: vec!["mirv_fov 105".to_string()],
            ..PatcherConfig::default()
        };

        assert_eq!(ring_limit_from_init(&config.init_commands), None);
        assert_eq!(ring_limit(&config), 128);
    }

    #[test]
    fn a_ring_past_the_engine_ceiling_is_clamped_to_it() {
        // The engine clamps r_decals to MAX_RENDER_DECALS, so positions beyond
        // that would be spent turning a ring that had already come round.
        let config = PatcherConfig {
            init_commands: vec!["r_decals 99999".to_string()],
            ..PatcherConfig::default()
        };

        assert_eq!(ring_limit(&config), crate::decals::MAX_RENDER_DECALS);
    }

    #[test]
    fn a_longer_command_that_merely_starts_the_same_is_not_the_ring() {
        let config = PatcherConfig {
            decal_ring_limit: 256,
            init_commands: vec!["r_decals_enabled 1".to_string()],
            ..PatcherConfig::default()
        };

        assert_eq!(ring_limit_from_init(&config.init_commands), None);
        assert_eq!(ring_limit(&config), 256);
    }
}
