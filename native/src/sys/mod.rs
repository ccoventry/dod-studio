/// Studio analyses demos for the game's Highlights tab (#565).
#[cfg(windows)]
pub mod analysis_server;
/// Whether reading a file would download it from a cloud drive (#992).
#[cfg(not(target_arch = "wasm32"))]
pub mod cloud_file;
/// Releases the mouse a killed game left confined (#1095).
#[cfg(not(target_arch = "wasm32"))]
pub mod cursor_clip;
#[cfg(not(target_arch = "wasm32"))]
pub mod dialogs;
pub mod disk;
/// Every folder scan goes through this walker: it skips junctions and
/// symlinks and stops at a depth cap (#376).
#[cfg(not(target_arch = "wasm32"))]
pub mod folder_walk;
#[cfg(not(target_arch = "wasm32"))]
pub mod foreground_lock;
#[cfg(not(target_arch = "wasm32"))]
pub mod game_remote;
/// The game asks Studio, over the same pipe, to review its demo (#1142).
#[cfg(not(target_arch = "wasm32"))]
pub mod game_review_requests;
/// Which hook DLL and build a running game has, against the file on disk
/// (#1143).
#[cfg(not(target_arch = "wasm32"))]
pub mod hook_build;
/// The game launches from a per-build copy of the hook DLL, so the file
/// Studio ships or a test build is never locked by it (#1146).
#[cfg(not(target_arch = "wasm32"))]
pub mod hook_copy;
/// The Half-Life installs on this PC and their engine builds (#931).
#[cfg(not(target_arch = "wasm32"))]
pub mod installs;
/// Whether Studio's job object ends its processes when it closes (#545).
#[cfg(not(target_arch = "wasm32"))]
pub mod job;
/// Directory junctions, made and unlinked through Win32 (#376).
#[cfg(windows)]
pub mod junction;
/// Starts the HLAE launcher outside a dev Studio's job and tracks it (#545).
#[cfg(not(target_arch = "wasm32"))]
pub mod launcher;
#[cfg(not(target_arch = "wasm32"))]
pub mod minimize;
/// The DLLs a running process has loaded: which hook DLL a game has (#725).
#[cfg(not(target_arch = "wasm32"))]
pub mod modules;
#[cfg(not(target_arch = "wasm32"))]
pub mod packet_limit;
#[cfg(not(target_arch = "wasm32"))]
pub mod pe;
#[cfg(not(target_arch = "wasm32"))]
pub mod process;
#[cfg(not(target_arch = "wasm32"))]
pub mod recycle_bin;
/// The primary screen's size, for a new install's capture resolution.
#[cfg(not(target_arch = "wasm32"))]
pub mod screen;
#[cfg(not(target_arch = "wasm32"))]
pub mod steam;
/// Starts a process through WMI, outside every job of Studio's (#545).
#[cfg(not(target_arch = "wasm32"))]
pub mod wmi_launch;
