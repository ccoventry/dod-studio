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
        // Quoted, same as capture_fov_from_init above and for the same
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
