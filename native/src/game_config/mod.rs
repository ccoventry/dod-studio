//! What the game's own configs and the app's init commands say about a
//! capture: the read-only `.cfg` scan, the report built from it, and the
//! values a batch resolves from both (the capture FOV, the decal ring,
//! the final init commands) (#604).
//!
//! The game's `.cfg` files are the user's: everything here detects and warns,
//! nothing writes one.

pub mod capture_fov;
pub mod cfg_report;
pub mod cfg_scan;
pub mod cfg_warn;
pub mod console_prefix;
pub mod decal_ring;
pub mod init_commands;
pub mod picture_fov;

use crate::patch::types::PatcherConfig;

pub use capture_fov::{capture_fov, capture_fov_from_init, capture_fov_resolved};
pub use cfg_scan::{CfgScan, CvarSetting, WATCHED_CVARS, scan as scan_game_cfgs};
pub use decal_ring::{ring_limit, ring_limit_from_game_config, ring_limit_from_init};
pub use init_commands::{final_init_commands, too_long_commands, user_init_commands};

/// The mod folder holding the game's configs, for a configured `hl.exe`.
pub(super) fn game_dir_for(config: &PatcherConfig) -> Option<std::path::PathBuf> {
    let dir = crate::shared::paths::mod_dir_for_exe(std::path::Path::new(&config.game_path))?;
    dir.is_dir().then_some(dir)
}
