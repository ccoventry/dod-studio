//! The FOV a capture will run at: what Initial Commands state, what an
//! executed game config sets, and the configured default, in that order.

use super::{cfg_scan, game_dir_for};
use crate::patch::types::PatcherConfig;

/// The FOV a capture will actually run at.
///
/// Read out of `init_commands` when HLAE's `mirv_fov` is among them, because
/// that is the value the engine will be using and a separate setting could
/// silently disagree with it. The last one wins, matching the console. Falls
/// back to the configured default when nothing sets it. The project's own
/// field of view (`picture_fov`) comes first: it is sent after every line.
pub fn capture_fov(config: &PatcherConfig) -> f32 {
    config
        .picture_fov
        .or_else(|| capture_fov_from_init(&config.init_commands))
        .unwrap_or(config.capture_fov)
}

/// The FOV `init_commands` states, if it states one at all.
///
/// Separate from `capture_fov` because "nothing was stated" and "the stated
/// value happens to equal the default" are different facts, and collapsing them
/// into one number loses the difference. `capture_fov_resolved` needs it: the
/// app's own seeded default is `mirv_fov 90`, so a user who takes the defaults
/// and has `mirv_fov 105` in a config would otherwise have their explicit 90
/// silently replaced by the config's 105.
pub fn capture_fov_from_init(init_commands: &[String]) -> Option<f32> {
    for cmd in init_commands.iter().rev() {
        let trimmed = cmd.trim();
        let Some(rest) = trimmed
            .strip_prefix("mirv_fov")
            .or_else(|| trimmed.strip_prefix("default_fov"))
        else {
            continue;
        };
        // Guard against matching a longer command that merely starts the same.
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        // Real .cfg syntax quotes every value ("105", not 105) — the same
        // convention "Load from .cfg file…" carries straight into Initial
        // Commands. Parsing the raw token instead of the unquoted value
        // means this silently reads a quoted line as "nothing stated" and
        // falls through to the default, which is the whole bug this exists
        // to prevent.
        if let Ok(v) = cfg_scan::unquote(rest.trim()).parse::<f32>()
            && v > 0.0
        {
            return Some(v);
        }
    }
    None
}

/// The FOV a capture will actually run at, including what the game's own
/// configs set.
///
/// `capture_fov` reads the app's init commands, which was the whole story only
/// as long as nothing else set it. It is not: a `config.cfg` ending in
/// `exec movie.cfg`, with `mirv_fov 105` inside, renders at 105 while the
/// pipeline sizes its on-screen test for the configured default — a cone some
/// seven degrees too narrow, calling in-shot positions hidden.
///
/// So the order is init commands, then an executed config, then the configured
/// default. `ring_limit` (r_decals) follows exactly this same order. Adopting
/// a `movie.cfg`'s `r_decals 0` does not silently stand the flush down: a
/// resolved 0 with Flush Decals on is its own loud, reported fact
/// (`decal_flush_is_noop` in the studio report) rather than something this
/// function would hide by disagreeing with the config.
///
/// Reads config files. Never writes one.
pub fn capture_fov_resolved(config: &PatcherConfig) -> f32 {
    // The project's own field of view is sent after every Initial Command
    // (`game_config::user_init_commands`), so nothing else has the last word.
    if let Some(fov) = config.picture_fov {
        return fov;
    }
    // An init command is the app's own statement and outranks a config —
    // including when it states the same number the default happens to be. The
    // seeded default IS `mirv_fov 90`, so treating that as "nothing was said"
    // would hand every defaulted install over to whatever its movie.cfg says.
    if let Some(stated) = capture_fov_from_init(&config.init_commands) {
        return stated;
    }

    let Some(dir) = game_dir_for(config) else {
        return config.capture_fov;
    };
    let scan = cfg_scan::scan_cached(&dir);
    for cvar in ["mirv_fov", "default_fov"] {
        if let Some(setting) = scan.effective(cvar)
            && let Ok(v) = setting.value.parse::<f32>()
            && v > 0.0
        {
            return v;
        }
    }
    config.capture_fov
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::fake_game;

    #[test]
    fn a_fov_the_game_config_sets_is_found_rather_than_assumed() {
        // The real shape, and the reason this exists: config.cfg ends with
        // `exec movie.cfg`, movie.cfg carries `mirv_fov 105`, and the app was
        // never told. Sizing the cone for the default 90 makes it ~7 degrees
        // too narrow and calls in-shot positions hidden.
        let (_root, exe) = fake_game("found", "exec movie.cfg\n", Some("mirv_fov \"105\"\n"));
        let config = PatcherConfig {
            game_path: exe.to_string_lossy().to_string(),
            ..PatcherConfig::default()
        };

        assert_eq!(
            capture_fov(&config),
            config.capture_fov,
            "init commands say nothing"
        );
        assert_eq!(capture_fov_resolved(&config), 105.0, "the config does");
    }

    #[test]
    fn an_init_command_stating_the_default_value_still_outranks_the_config() {
        // The trap: `mirv_fov 90` is what the app seeds into a new install's
        // init commands, and 90 is also the configured default. Deciding
        // "was one stated?" by comparing against the default would read that as
        // silence and hand every defaulted install to its movie.cfg — here,
        // rendering the cone for 105 when the user asked for 90.
        let (_root, exe) = fake_game(
            "states_default",
            "exec movie.cfg\n",
            Some("mirv_fov \"105\"\n"),
        );
        let config = PatcherConfig {
            game_path: exe.to_string_lossy().to_string(),
            init_commands: vec!["mirv_fov 90".to_string()],
            ..PatcherConfig::default()
        };

        assert_eq!(
            config.capture_fov, 90.0,
            "the default this could be confused with"
        );
        assert_eq!(capture_fov_from_init(&config.init_commands), Some(90.0));
        assert_eq!(capture_fov_resolved(&config), 90.0, "the user asked for 90");
    }

    #[test]
    fn an_init_command_outranks_the_game_config() {
        let (_root, exe) = fake_game("outrank", "exec movie.cfg\n", Some("mirv_fov \"105\"\n"));
        let config = PatcherConfig {
            game_path: exe.to_string_lossy().to_string(),
            init_commands: vec!["mirv_fov 120".to_string()],
            ..PatcherConfig::default()
        };

        assert_eq!(capture_fov_resolved(&config), 120.0);
    }

    #[test]
    fn a_quoted_mirv_fov_in_init_commands_is_still_recognised() {
        let config = PatcherConfig {
            capture_fov: 90.0,
            init_commands: vec!["mirv_fov \"105\"".to_string()],
            ..PatcherConfig::default()
        };

        assert_eq!(capture_fov_from_init(&config.init_commands), Some(105.0));
        assert_eq!(capture_fov(&config), 105.0);
    }

    #[test]
    fn mirv_fov_in_the_init_commands_wins() {
        // The capture runs at whatever the console was last told, so reading it
        // from there is the only way the two cannot disagree.
        let mut config = PatcherConfig {
            capture_fov: 90.0,
            ..PatcherConfig::default()
        };
        assert_eq!(capture_fov(&config), 90.0);

        config.init_commands = vec!["exec autoexec".to_string(), "mirv_fov 105".to_string()];
        assert_eq!(capture_fov(&config), 105.0);

        // Last one wins, as at the console.
        config.init_commands.push("mirv_fov 100".to_string());
        assert_eq!(capture_fov(&config), 100.0);
    }

    /// The project's field of view (#1085) is sent after every line, so it
    /// is the one the capture runs at, whatever the lines say.
    #[test]
    fn the_projects_field_of_view_wins_over_the_lines() {
        let config = PatcherConfig {
            init_commands: vec!["mirv_fov 105".to_string()],
            picture_fov: Some(110.0),
            ..PatcherConfig::default()
        };
        assert_eq!(capture_fov(&config), 110.0);
        assert_eq!(capture_fov_resolved(&config), 110.0);
    }

    #[test]
    fn a_command_that_merely_starts_the_same_is_not_mistaken_for_it() {
        let config = PatcherConfig {
            capture_fov: 90.0,
            init_commands: vec!["mirv_fov_something_else 12".to_string()],
            ..PatcherConfig::default()
        };
        assert_eq!(capture_fov(&config), 90.0);
    }
}
