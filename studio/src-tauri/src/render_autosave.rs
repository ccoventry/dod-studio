// studio/src-tauri/src/render_autosave.rs
//
// The render batch's crash-recovery lockfile (`.render_autosave.json`): where
// it lives, how the batch is written to it, and how a saved batch comes back
// as job rows and a Start config (#770, #1178). A child module of
// `render_manager`, which owns the job list these functions read and write.

use super::{RenderJobRuntime, history_for, resolve_ffmpeg};
use native::hlcr::autosave::{
    RenderJob as AutosaveJob, RenderJobStatus as AutosaveJobStatus, RenderSessionData,
};
use native::hlcr::config::{RenderCodec, RenderConfig};
use native::hlcr::scanner::ClipData;
use native::hlcr::take_cleanup::attempt_stream;
use native::log_markdown;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

pub(super) fn autosave_path() -> PathBuf {
    native::shared::paths::get_appdata_dir().join(".render_autosave.json")
}

pub(super) fn write_autosave(
    render_session: &Arc<Mutex<Option<RenderSessionData>>>,
    jobs: &[RenderJobRuntime],
    config: &RenderConfig,
) {
    let session = RenderSessionData {
        source_folder: config.source_folder.clone(),
        fps: config.fps,
        target_codec: config.target_codec.to_str_id().to_string(),
        target_custom_codec_args: config.custom_codec_args.clone(),
        jobs: jobs
            .iter()
            .map(|j| AutosaveJob {
                take_folder: j.clip.take_folder.clone(),
                output_path: j.output_path.clone(),
                // A take staged as Finished from its history (#70) comes
                // back as Finished after a recovery too.
                status: if j.status == "Finished" {
                    AutosaveJobStatus::Completed
                } else {
                    AutosaveJobStatus::Pending
                },
                name: j.clip.base_name.clone(),
                codec: Some(j.codec.to_str_id().to_string()),
                custom_codec_args: Some(j.custom_codec_args.clone()),
                fps: Some(j.fps),
                clip: Some(j.clip.clone()),
            })
            .collect(),
    };
    save_autosave(&session);
    *render_session.lock().unwrap() = Some(session);
}

/// The autosave's entry for the row rendering `clip`, found by take folder
/// and stream rather than by job id: a scan merges into the list (#772), so
/// an id is not the row's place in the autosave. A recovered stub saved
/// without its clip matches on the folder alone.
pub(super) fn autosave_entry<'a>(
    session: &'a mut RenderSessionData,
    clip: &ClipData,
) -> Option<&'a mut AutosaveJob> {
    let stream = attempt_stream(clip);
    session.jobs.iter_mut().find(|rj| {
        rj.take_folder == clip.take_folder
            && rj.clip.as_ref().is_none_or(|c| attempt_stream(c) == stream)
    })
}

/// Writes `session` to `.render_autosave.json`.
pub(super) fn save_autosave(session: &RenderSessionData) {
    if let Ok(json) = serde_json::to_string_pretty(session)
        && let Err(e) = std::fs::write(autosave_path(), json)
    {
        log_markdown(&format!(
            "[render_autosave] Failed to write lockfile: {}",
            e
        ));
    }
}

#[derive(Serialize)]
pub struct RenderAutosaveSummary {
    pub source_folder: String,
    pub pending_count: usize,
    pub completed_count: usize,
}

/// What the recovery prompt offers, or `None` when every job had finished:
/// a scan writes the autosave too, so a scan after a batch ended (or one
/// whose takes were all rendered before) leaves a file with nothing left to
/// render, and the prompt said "didn't finish cleanly" over "Pending: 0".
pub(super) fn recovery_summary(session: RenderSessionData) -> Option<RenderAutosaveSummary> {
    let pending_count = session
        .jobs
        .iter()
        .filter(|j| j.status == AutosaveJobStatus::Pending)
        .count();
    if pending_count == 0 {
        return None;
    }
    let completed_count = session
        .jobs
        .iter()
        .filter(|j| j.status == AutosaveJobStatus::Completed)
        .count();
    Some(RenderAutosaveSummary {
        source_folder: session.source_folder,
        pending_count,
        completed_count,
    })
}

/// The batch-wide settings a recovered batch takes from Settings, the same
/// ones the Render tab puts in a scan's payload: the autosave doesn't hold
/// them.
pub(super) struct RecoverySettings {
    ffmpeg_path: Option<String>,
    export_directories: Vec<String>,
    max_concurrent_renders: i32,
}

impl From<&crate::settings_manager::AppSettings> for RecoverySettings {
    fn from(s: &crate::settings_manager::AppSettings) -> Self {
        Self {
            ffmpeg_path: s.ffmpeg_path.clone(),
            export_directories: s.render_export_dirs.clone(),
            max_concurrent_renders: s.render_max_concurrent,
        }
    }
}

/// The `RenderConfig` Start needs for a recovered batch (#770): codec and
/// fps from the autosave (each job also carries its own), FFmpeg, export
/// pool and concurrency from current settings. A Queued job the autosave
/// stored without its scanned take can't render, so that one needs a scan.
pub(super) fn recovered_config(
    session: &RenderSessionData,
    settings: &RecoverySettings,
) -> Result<RenderConfig, String> {
    if let Some(stub) = session
        .jobs
        .iter()
        .find(|j| j.status == AutosaveJobStatus::Pending && j.clip.is_none())
    {
        return Err(crate::messages::recovered_batch_needs_scan(&format!(
            "the frames and audio of {}",
            stub.name
        )));
    }
    Ok(RenderConfig {
        ffmpeg_path: resolve_ffmpeg(settings.ffmpeg_path.as_ref())
            .to_string_lossy()
            .into_owned(),
        source_folder: session.source_folder.clone(),
        export_directories: settings
            .export_directories
            .iter()
            .filter(|d| !d.trim().is_empty())
            .map(PathBuf::from)
            .collect(),
        fps: session.fps.max(1),
        target_codec: RenderCodec::from_str_id(&session.target_codec),
        custom_codec_args: session.target_custom_codec_args.clone(),
        // The Render tab's own range for this field.
        max_concurrent_renders: settings.max_concurrent_renders.clamp(1, 8) as usize,
    })
}

/// The autosave's jobs as rows: Completed ones Finished, Pending ones Queued.
/// Each job recovers with its own codec/fps (#85): the snapshot records them
/// per job, and `set_render_job_codec` updates it when a job's Skip toggle
/// changes. A snapshot from before that falls back to the batch's.
pub(super) fn recovered_jobs(session: &RenderSessionData) -> Vec<RenderJobRuntime> {
    session
        .jobs
        .iter()
        .enumerate()
        .map(|(i, rj)| {
            let settings = session.job_settings(rj);
            let (status, progress) = if rj.status == AutosaveJobStatus::Completed {
                ("Finished".to_string(), 100u32)
            } else {
                ("Queued".to_string(), 0u32)
            };
            // Only worth a stat for a job that actually finished and has a real
            // path recorded — a re-queued job has neither yet.
            let output_size_bytes = (rj.status == AutosaveJobStatus::Completed
                && !rj.output_path.is_empty())
            .then(|| std::fs::metadata(&rj.output_path).ok().map(|m| m.len()))
            .flatten();
            RenderJobRuntime {
                id: i.to_string(),
                // The scanned clip when the autosave has it; otherwise a stub
                // that a re-scan fills in.
                clip: rj.clip.clone().unwrap_or_else(|| ClipData {
                    clip_name: None,
                    take_folder: rj.take_folder.clone(),
                    clip_type: "single".to_string(),
                    img_folder: String::new(),
                    // The autosave snapshot stores take_folder/name/status only, so
                    // this reconstruction cannot know which kind of take it was —
                    // wav-and-frames or OBS-shaped — the same reason `img_folder`
                    // and `video_file` are blank below. Guessing `Some("sound.wav")`
                    // here used to be harmless when every take had one; now that an
                    // OBS take legitimately has none, guessing would misrepresent
                    // it. `None` matches the rest of this stub's "unknown, a
                    // re-scan is what fills it in" treatment, and fails at
                    // `run_render_job`'s clear "no audio source" guard instead of a
                    // misleading "sound.wav not found" if resumed without a rescan.
                    wav_file: None,
                    base_name: rj.name.clone(),
                    frame_count: 0,
                    width: 0,
                    height: 0,
                    date: String::new(),
                    video_file: None,
                    alpha_folder: None,
                }),
                status,
                speed: String::new(),
                progress,
                error_log: None,
                output_path: rj.output_path.clone(),
                output_size_bytes,
                cancel_flag: Arc::new(AtomicBool::new(false)),
                codec: RenderCodec::from_str_id(&settings.codec),
                custom_codec_args: settings.custom_codec_args,
                fps: settings.fps,
                take_fps: native::hlcr::take_meta::recorded_fps(std::path::Path::new(
                    &rj.take_folder,
                )),
                history: history_for(&rj.take_folder),
                current_attempt: None,
            }
        })
        .collect()
}

#[cfg(test)]
pub(super) mod fixtures {
    use super::*;

    pub fn clip(take_folder: &str) -> ClipData {
        ClipData {
            clip_name: None,
            take_folder: take_folder.to_string(),
            clip_type: "single".to_string(),
            img_folder: "all".to_string(),
            wav_file: Some("sound.wav".to_string()),
            base_name: "take".to_string(),
            frame_count: 300,
            width: 1920,
            height: 1080,
            date: String::new(),
            video_file: None,
            alpha_folder: None,
        }
    }

    pub fn autosave_job(name: &str, status: AutosaveJobStatus, with_clip: bool) -> AutosaveJob {
        let take_folder = format!("Z:/no-such-takes/{}", name);
        AutosaveJob {
            output_path: String::new(),
            status,
            name: name.to_string(),
            codec: Some("h264".to_string()),
            custom_codec_args: Some(String::new()),
            fps: Some(60),
            clip: with_clip.then(|| clip(&take_folder)),
            take_folder,
        }
    }

    pub fn session(jobs: Vec<AutosaveJob>) -> RenderSessionData {
        RenderSessionData {
            source_folder: "Z:/no-such-takes".to_string(),
            fps: 300,
            target_codec: "prores".to_string(),
            target_custom_codec_args: String::new(),
            jobs,
        }
    }

    pub fn settings() -> RecoverySettings {
        RecoverySettings {
            ffmpeg_path: None,
            export_directories: vec!["E:/exports".to_string(), "  ".to_string()],
            max_concurrent_renders: 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    #[test]
    fn the_recovery_prompt_needs_a_job_left_to_render() {
        let done = session(vec![
            autosave_job("a", AutosaveJobStatus::Completed, true),
            autosave_job("b", AutosaveJobStatus::Completed, true),
        ]);
        assert!(recovery_summary(done).is_none());
        let summary = recovery_summary(session(vec![
            autosave_job("a", AutosaveJobStatus::Completed, true),
            autosave_job("b", AutosaveJobStatus::Pending, true),
        ]))
        .expect("a pending job is worth recovering");
        assert_eq!((summary.pending_count, summary.completed_count), (1, 1));
    }
}
