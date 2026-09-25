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
    /// This job's own codec (a `RenderCodec` id), custom codec args and fps.
    /// A job can differ from the batch -- the per-job Skip toggle, for one --
    /// and without these a crash recovered it with the batch's settings,
    /// silently dropping the change (#85). `None` in an autosave written
    /// before they existed: see [`RenderSessionData::job_settings`].
    #[serde(default)]
    pub codec: Option<String>,
    #[serde(default)]
    pub custom_codec_args: Option<String>,
    #[serde(default)]
    pub fps: Option<u32>,
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

/// One job's settings as recovered: its own, else the session's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSettings {
    pub codec: String,
    pub custom_codec_args: String,
    pub fps: u32,
}

impl RenderSessionData {
    /// `job`'s codec, custom args and fps, each falling back to the
    /// session-wide value when the snapshot has none for it (an autosave
    /// written before per-job settings were recorded).
    pub fn job_settings(&self, job: &RenderJob) -> JobSettings {
        JobSettings {
            codec: job
                .codec
                .clone()
                .unwrap_or_else(|| self.target_codec.clone()),
            custom_codec_args: job
                .custom_codec_args
                .clone()
                .unwrap_or_else(|| self.target_custom_codec_args.clone()),
            fps: job.fps.unwrap_or(self.fps),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD_SNAPSHOT: &str = r#"{
        "source_folder": "D:/takes",
        "fps": 60,
        "target_codec": "prores",
        "jobs": [
            {"take_folder": "D:/takes/a", "output_path": "", "status": "Pending", "name": "a"}
        ]
    }"#;

    #[test]
    fn a_snapshot_from_before_per_job_settings_recovers_with_the_batchs() {
        let session: RenderSessionData = serde_json::from_str(OLD_SNAPSHOT).unwrap();
        assert_eq!(
            session.job_settings(&session.jobs[0]),
            JobSettings {
                codec: "prores".into(),
                custom_codec_args: String::new(),
                fps: 60
            }
        );
    }

    #[test]
    fn a_jobs_own_settings_survive_the_round_trip() {
        let mut session: RenderSessionData = serde_json::from_str(OLD_SNAPSHOT).unwrap();
        session.jobs[0].codec = Some("source_copy".into());
        session.jobs[0].fps = Some(30);
        let back: RenderSessionData =
            serde_json::from_str(&serde_json::to_string(&session).unwrap()).unwrap();
        let settings = back.job_settings(&back.jobs[0]);
        assert_eq!(settings.codec, "source_copy");
        assert_eq!(settings.fps, 30);
        // Not recorded for this job: the batch's.
        assert_eq!(settings.custom_codec_args, "");
    }
}
