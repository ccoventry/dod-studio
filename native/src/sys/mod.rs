/// Studio analyses demos for the game's Highlights tab (#565).
#[cfg(windows)]
pub mod analysis_server;
/// Whether reading a file would download it from a cloud drive (#992).
pub mod cloud_file;
/// Releases the mouse a killed game left confined (#1095).
pub mod cursor_clip;
pub mod dialogs;
pub mod disk;
/// Every folder scan goes through this walker: it skips junctions and
/// symlinks and stops at a depth cap (#376).
pub mod folder_walk;
pub mod foreground_lock;
pub mod game_remote;
/// The game asks Studio, over the same pipe, to review its demo (#1142).
pub mod game_review_requests;
/// Which hook DLL and build a running game has, against the file on disk
/// (#1143).
pub mod hook_build;
/// The game launches from a per-build copy of the hook DLL, so the file
/// Studio ships or a test build is never locked by it (#1146).
pub mod hook_copy;
/// The Half-Life installs on this PC and their engine builds (#931).
pub mod installs;
/// Whether Studio's job object ends its processes when it closes (#545).
pub mod job;
/// Directory junctions, made and unlinked through Win32 (#376).
#[cfg(windows)]
pub mod junction;
/// Starts the HLAE launcher outside a dev Studio's job and tracks it (#545).
pub mod launcher;
pub mod minimize;
/// The DLLs a running process has loaded: which hook DLL a game has (#725).
pub mod modules;
pub mod packet_limit;
pub mod pe;
pub mod process;
pub mod recycle_bin;
/// The primary screen's size, for a new install's capture resolution.
pub mod screen;
pub mod steam;
/// Which Studio's pipe a game asks, so two Studios don't answer each other's
/// game (#1235).
#[cfg(windows)]
pub mod studio_pipe;
/// Ends a launcher a cancelled WMI call starts late (#1243).
pub mod wmi_late_start;
/// Starts a process through WMI, outside every job of Studio's (#545).
pub mod wmi_launch;
