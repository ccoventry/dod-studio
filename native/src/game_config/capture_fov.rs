//! The FOV a capture will run at: what Initial Commands state, what an
//! executed game config sets, and the configured default, in that order.

use super::{cfg_scan, game_dir_for};
use crate::patch::types::PatcherConfig;

/// The FOV a capture will actually run at.
///
/// Read out of `init_commands` when HLAE's `mirv_fov` is among them, because
/// that is the value the engine will be using and a separate setting could
/// silently disagree with it. The last one wins, matching the console. Falls
/// back to the configured default when nothing sets it.
pub fn capture_fov(config: &PatcherConfig) -> f32 {
    capture_fov_from_init(&config.init_commands).unwrap_or(config.capture_fov)
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
