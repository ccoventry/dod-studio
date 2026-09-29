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
    /// This job's own settings (#85), which can differ from the session's:
    /// "Skip" and Reset's carry-over both set them per job. `None` in an
    /// autosave written before these existed, which recovers with the
    /// session-wide values instead.
    #[serde(default)]
    pub codec: Option<String>,
    #[serde(default)]
    pub custom_codec_args: Option<String>,
    #[serde(default)]
    pub fps: Option<u32>,
    /// The scanned take, so a recovered job renders as it was queued instead
    /// of as a stub that needs a re-scan. `None` in an older autosave.
    #[serde(default)]
    pub clip: Option<crate::hlcr::scanner::ClipData>,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// An autosave written before #85 has no per-job settings or clip; it
    /// must still load, with those left for the session-wide fallback.
    #[test]
    fn an_autosave_from_before_per_job_settings_still_loads() {
        let json = r#"{"source_folder":"D:/c","fps":300,"target_codec":"prores",
            "jobs":[{"take_folder":"D:/c/s/t","output_path":"","status":"Pending","name":"t"}]}"#;
        let session: RenderSessionData = serde_json::from_str(json).unwrap();
        let job = &session.jobs[0];
        assert_eq!(job.codec, None);
        assert_eq!(job.fps, None);
        assert!(job.clip.is_none());
    }
}
