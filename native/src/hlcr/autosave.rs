#![cfg(not(target_arch = "wasm32"))]

//! Render autosave schema.
//! Defined here (inside the native library) so both `hlcr::ui` and the GUI
//! binary can access the types without a crate-private binary-path import.

/// Completion status for a single render job in an autosave snapshot.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RenderJobStatus {
    Pending,
    Completed,
}

/// A single clip record inside a render autosave snapshot.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RenderJob {
    /// Take folder path (source of BMP frames + WAV).
    pub take_folder: String,
    /// Resolved output file path (populated when FFmpeg exits successfully).
    pub output_path: String,
    /// Current status — `Pending` or `Completed`.
    pub status: RenderJobStatus,
    /// Human-readable clip base name for display in the recovery modal.
    pub name: String,
}

/// Persisted render-session snapshot written to `.render_autosave.json`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RenderSessionData {
    /// Source folder path at the time the batch started.
    pub source_folder: String,
    pub fps: u32,
    pub target_codec: String,
    /// Raw FFmpeg video-codec args, only meaningful when `target_codec` is
    /// `"custom"`. `#[serde(default)]` so an autosave file written before
    /// this field existed still parses.
    #[serde(default)]
    pub target_custom_codec_args: String,
    /// All jobs — both Pending (incomplete) and Completed.
    pub jobs: Vec<RenderJob>,
}
