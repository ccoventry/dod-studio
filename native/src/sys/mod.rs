pub mod disk;
#[cfg(not(target_arch = "wasm32"))]
pub mod game_remote;
#[cfg(not(target_arch = "wasm32"))]
pub mod pe;
#[cfg(not(target_arch = "wasm32"))]
pub mod process;
