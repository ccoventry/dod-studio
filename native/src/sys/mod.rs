/// Studio analyses demos for the game's Killstreaks tab (#565).
#[cfg(windows)]
pub mod analysis_server;
#[cfg(not(target_arch = "wasm32"))]
pub mod dialogs;
pub mod disk;
#[cfg(not(target_arch = "wasm32"))]
pub mod game_remote;
#[cfg(not(target_arch = "wasm32"))]
pub mod minimize;
#[cfg(not(target_arch = "wasm32"))]
pub mod pe;
#[cfg(not(target_arch = "wasm32"))]
pub mod process;
#[cfg(not(target_arch = "wasm32"))]
pub mod steam;
