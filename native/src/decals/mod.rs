//! Clearing decals out of a demo before it is recorded (#60): the flush
//! pipeline, the probes that measure it, the decal atlas it reads, and the
//! player positions it treats as proof of what a map allows (#604).

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

pub mod decal_atlas;
pub mod decal_probe;
pub mod decal_strip;

pub use decal_strip::{
    CleanedSource, DECALS_PER_POSITION, DEFAULT_LEAD_SECONDS, DecalCleanError, DecalCleanOptions,
    DecalCleanStats, FlushSource, MAX_OVERLAP_DECALS, VisibilityBasis, clean_demo_decals,
    on_screen_half_angle, prepare_flushed_source, proven_world_coordinates,
    strip_decals_outside_windows,
};

pub use decal_probe::{
    CameraView, GridStats, Probe, ProbeOptions, ProbeRow, ProbeStats, Sighting, best_view_for,
    camera_at_time, decal_texture_histogram, probe_decal_offsets, project,
};

/// Whether a frame can carry injected payload: it must be a `NetworkMessage`
/// whose contents were actually parsed (an unparsed one is opaque bytes there
/// is nothing safe to append to) and under [`MAX_INJECTABLE_MESSAGE_LEN`].
///
/// `entry_idx`/`frame_idx` index `demo.directory.entries[..].frames[..]`; an
/// out-of-range pair is simply not injectable rather than a panic, so callers
/// can hand this raw indices straight out of a frame-ordinal walk.
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
