// patch/mod.rs
// Demo rewriting: scanning a demo for highlights, building the batch that
// records them, and patching frames into the copies it plays. Decal flushing
// is `decals`, the game's configs `game_config`, map files `maps` (#604).

// Engine & Memory Limits

/// The width of a Type-3 `ConsoleCommand` frame's `char command[64]` field.
///
/// This is a **demo file format** property, not an engine one, so it cannot be
/// raised: `dem-patch`'s `parse_console_command` reads exactly 64 bytes, and a
/// longer string would run into the next frame. GoldSrc's own command buffer is
/// 16,384 bytes (`Cbuf_Init`, `hw.dll+0x272b0`) and imposes nothing here — see
/// `docs/goldsrc_hw_dll_survey.md` §3.1. A command too long for one frame has to
/// be staggered across several ticks.
pub const MAX_CONSOLE_CMD_LEN: usize = 64;
/// The longest command that still leaves room for the field's NUL terminator.
pub const MAX_CONSOLE_CMD_SAFE_LEN: usize = 63;

pub const MAX_DIRECTOR_STUFFTEXT_LEN: usize = 253;
pub const IO_BUFFER_CAPACITY: usize = 262_144;
pub const MAX_PAYLOAD_LIMIT_BYTES: usize = 2_097_152;

// Binary Frame & Header Sizes
pub const DEMO_HEADER_SIZE: usize = 544;
pub const DIRECTORY_OFFSET_POS: usize = 540;
pub const FRAME_HEADER_SIZE: usize = 9;
pub const NETMSG_INFO_SIZE: usize = 464;
pub const NETWORK_HEADER_ALIGNMENT: usize = 468;
pub const DIR_ENTRY_SIZE: usize = 92;
pub const SCANNER_SECTION_BOUNDARY: u8 = 5;

// Frame Type Payload Sizes
pub const CMD_FRAME_SIZE: usize = 64;
pub const CLIENT_DATA_FRAME_SIZE: usize = 32;
pub const EVENT_FRAME_SIZE: usize = 84;

// Command Injection Logic
pub const MAX_ECHO_CHUNK_SIZE: usize = 55;
/// How long the primer demo plays before `playdemo dodstudio_chain_01`. Only a
/// batch launched without the hook DLL has a primer (#365).
pub const PRIMER_DELAY_TICKS: i32 = 500;

pub const BREADCRUMB_INTERVAL_TICKS: i32 = 5000;

pub mod types;

/// A Seek Between Clips batch in the game that is already open (#725).
pub mod batch_reuse;
pub mod builder;
pub mod copy_cache;
pub mod engine;
pub mod entity_trim;
pub mod game_dir;
pub mod highlevel;
pub mod launch_settings;
pub mod map_text;
pub mod protocol47;
pub mod runtime_batch;
pub mod scanner;
pub mod sound_mute;
pub mod source_copies;
pub mod trimmed_copies;
pub mod viewdemo_clock;
pub mod viewdemo_refresh;

// ── Re-export wall ────────────────────────────────────────────────────────────
// All items below were previously at the top level of patch.rs.
// Every existing `native::patch::*` call site resolves here unchanged.

pub use types::{
    CaptureBlock, CaptureCodec, CaptureMode, CaptureStreak, CommandRelation, CustomCommand,
    DEFAULT_AGR_FPS, DriveHeadroom, HighlightRules, HighlightStatus, MAX_PAYLOAD_SIZE, ObsConfig,
    ObsRecordingQuality, PatchJob, PatchOptions, PatcherConfig,
};

pub use types::{CaptureWorker, PatchEvent};

pub use highlevel::patch_demo_highlights;

pub use sound_mute::{MapSounds, MuteSelection, MuteStats, map_sounds, mute_sounds};

pub use map_text::{MapText, TextSelection, TextStats, hide_map_text, map_text};

pub use engine::{PatchStage, StreamPatcher};

pub use builder::{
    WorkspaceGuard, build_batch_queue, build_director_message, build_director_stufftext,
    build_preview_patch_jobs, estimate_batch_capture_blocks, estimate_batch_capture_bytes,
    playdemo_safe_stem, spawn_patch_batch,
};

pub use scanner::{
    check_sources_unchanged, is_hltv_demo, is_hltv_head, scan_demo_for_highlights,
    scan_demo_for_highlights_with_analysis,
};
