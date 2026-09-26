// patch/mod.rs
// Public surface of the patch module.
//
// Declares all sub-modules and re-exports every public item that was previously
// at the flat `native::patch::*` path. All existing call sites remain unchanged.
//
// Sub-module creation order (Phase 10 sequence):
//   Step 1 (current): types.rs, mod.rs  ← foundation
//   Step 2 (pending): highlevel.rs      ← dem-crate high-level API
//   Step 3 (pending): engine.rs         ← StreamPatcher binary I/O
//   Step 4 (pending): builder.rs        ← build_batch_queue, spawn_patch_batch
//   Step 5 (pending): scanner.rs        ← scan_demo_for_highlights, is_hltv_demo

/// Cancellation as the decal-flush pipeline sees it.
///
/// A capture batch owns an `Arc<AtomicBool>` and passes it in; the offline
/// probes in `native/examples`, the `strip_decals` binary and the unit tests
/// run the same code with nothing to cancel them, and pass [`Cancel::never`].
/// Wrapping the difference here keeps every check site a plain
/// `if cancel.requested()` instead of an `Option` dance repeated a dozen times.
#[derive(Clone, Copy)]
pub struct Cancel<'a>(Option<&'a std::sync::Arc<std::sync::atomic::AtomicBool>>);

impl<'a> Cancel<'a> {
    pub fn new(token: &'a std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self {
        Self(Some(token))
    }

    /// A check that never fires, for callers outside a cancellable batch.
    pub const fn never() -> Self {
        Self(None)
    }

    #[inline]
    pub fn requested(&self) -> bool {
        self.0
            .is_some_and(|t| t.load(std::sync::atomic::Ordering::Relaxed))
    }
}

/// Returned by the decal-flush pipeline when it gave up because the batch was
/// cancelled. Deliberately not a `String` error: cancellation is not a failure
/// and must not be reported to the user as one, nor fall back to the
/// unflushed demo and carry on patching the way a real flush failure does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("cancelled by user")
    }
}

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
pub const HLTV_HEADER_SIZE: usize = 512;
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
pub const CUSTOM_CMD_WARN_LIMIT: usize = 60;
pub const PRIMER_DELAY_TICKS: i32 = 500;

/// Upper bound on the size of a `NetworkMessage` frame the decal passes will
/// append injected payload to.
///
/// A frame at or above this is already close enough to the engine's own read
/// budget that adding to it risks an `svc_bad` on playback, so the passes skip
/// it and use a smaller neighbour instead — there are always plenty. This is a
/// safety margin chosen against the engine's behaviour, not a value the format
/// states anywhere, which is exactly why it wants a name rather than a `1024`
/// sitting in two files. `decal_probe` and `decal_strip` both filter on it via
/// `is_injectable_frame`.
pub const MAX_INJECTABLE_MESSAGE_LEN: u32 = 1024;

/// The engine's own ceiling on the decal ring. `r_decals` is clamped to this,
/// so a sweep of this size turns a full revolution regardless of what the cvar
/// is set to — which is what lets the pipeline stop pinning it. See
/// `decal_strip` and `docs/archive/decal_flush_bsp_surfaces.md`.
pub const MAX_RENDER_DECALS: u32 = 4096;
pub const BREADCRUMB_INTERVAL_TICKS: i32 = 5000;

pub mod types;

#[cfg(not(target_arch = "wasm32"))]
pub mod bsp;
#[cfg(not(target_arch = "wasm32"))]
pub mod bsp_entities;
#[cfg(not(target_arch = "wasm32"))]
pub mod builder;
#[cfg(not(target_arch = "wasm32"))]
pub mod cfg_scan;
#[cfg(not(target_arch = "wasm32"))]
pub mod decal_atlas;
#[cfg(not(target_arch = "wasm32"))]
pub mod decal_probe;
#[cfg(not(target_arch = "wasm32"))]
pub mod decal_strip;
#[cfg(not(target_arch = "wasm32"))]
pub mod engine;
#[cfg(not(target_arch = "wasm32"))]
pub mod highlevel;
pub mod map_check;
#[cfg(not(target_arch = "wasm32"))]
pub mod map_fetch;
#[cfg(not(target_arch = "wasm32"))]
pub mod map_text;
#[cfg(not(target_arch = "wasm32"))]
pub mod reachability;
#[cfg(not(target_arch = "wasm32"))]
pub mod scanner;
#[cfg(not(target_arch = "wasm32"))]
pub mod sound_mute;

// ── Re-export wall ────────────────────────────────────────────────────────────
// All items below were previously at the top level of patch.rs.
// Every existing `native::patch::*` call site resolves here unchanged.

pub use types::{
    CaptureBlock, CaptureCodec, CaptureMode, CaptureStreak, CommandRelation, CustomCommand,
    DriveHeadroom, HighlightRules, HighlightStatus, MAX_PAYLOAD_SIZE, ObsConfig, PatchJob,
    PatchOptions, PatcherConfig, default_goldsrc_hooks_dll_path,
};

#[cfg(not(target_arch = "wasm32"))]
pub use types::{CaptureWorker, PatchEvent};

#[cfg(not(target_arch = "wasm32"))]
pub use highlevel::patch_demo_highlights;

#[cfg(not(target_arch = "wasm32"))]
pub use decal_strip::{
    CleanedSource, DECALS_PER_POSITION, DEFAULT_LEAD_SECONDS, DecalCleanError, DecalCleanOptions,
    DecalCleanStats, FlushSource, MAX_OVERLAP_DECALS, VisibilityBasis, capture_fov,
    clean_demo_decals, on_screen_half_angle, prepare_flushed_source, proven_world_coordinates,
    ring_limit, ring_limit_from_game_config, ring_limit_from_init, strip_decals_outside_windows,
};

#[cfg(not(target_arch = "wasm32"))]
pub use decal_probe::{
    CameraView, GridStats, Probe, ProbeOptions, ProbeRow, ProbeStats, Sighting, best_view_for,
    camera_at_time, decal_texture_histogram, probe_decal_offsets, project,
};

#[cfg(not(target_arch = "wasm32"))]
pub use cfg_scan::{CfgScan, CvarSetting, WATCHED_CVARS, scan as scan_game_cfgs};

#[cfg(not(target_arch = "wasm32"))]
pub use decal_strip::{capture_fov_from_init, capture_fov_resolved};

#[cfg(not(target_arch = "wasm32"))]
pub use map_check::{MapReference, MapStatus, check_demo, map_reference};

#[cfg(not(target_arch = "wasm32"))]
pub use sound_mute::{MapSounds, MuteSelection, MuteStats, map_sounds, mute_sounds};

#[cfg(not(target_arch = "wasm32"))]
pub use map_text::{MapText, TextSelection, TextStats, hide_map_text, map_text};

#[cfg(not(target_arch = "wasm32"))]
pub use map_fetch::{DEFAULT_MIRROR, FetchOutcome, fetch_map, map_url};

#[cfg(not(target_arch = "wasm32"))]
pub use engine::{PatchStage, StreamPatcher};

#[cfg(not(target_arch = "wasm32"))]
pub use builder::{
    WorkspaceGuard, build_batch_queue, build_director_message, build_director_stufftext,
    build_preview_patch_jobs, final_init_commands, playdemo_safe_stem, spawn_patch_batch,
};

#[cfg(not(target_arch = "wasm32"))]
pub use scanner::{is_hltv_demo, scan_demo_for_highlights, scan_demo_for_highlights_with_analysis};

/// Whether a frame can carry injected payload: it must be a `NetworkMessage`
/// whose contents were actually parsed (an unparsed one is opaque bytes there
/// is nothing safe to append to) and under [`MAX_INJECTABLE_MESSAGE_LEN`].
///
/// `entry_idx`/`frame_idx` index `demo.directory.entries[..].frames[..]`; an
/// out-of-range pair is simply not injectable rather than a panic, so callers
/// can hand this raw indices straight out of a frame-ordinal walk.
#[cfg(not(target_arch = "wasm32"))]
pub fn is_injectable_frame(demo: &dem::types::Demo, entry_idx: usize, frame_idx: usize) -> bool {
    use dem::types::{FrameData, MessageData};

    demo.directory
        .entries
        .get(entry_idx)
        .and_then(|entry| entry.frames.get(frame_idx))
        .is_some_and(|frame| match &frame.frame_data {
            FrameData::NetworkMessage(b) => {
                matches!(b.1.messages, MessageData::Parsed(_))
                    && b.1.message_length < MAX_INJECTABLE_MESSAGE_LEN
            }
            _ => false,
        })
}
