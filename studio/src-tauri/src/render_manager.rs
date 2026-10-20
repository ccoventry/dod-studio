// studio/src-tauri/src/render_manager.rs
//
// Calls into native::hlcr's real render pipeline (renderer.rs/config.rs/
// autosave.rs/scanner.rs — byte-identical to dev, previously orphaned, see
// docs/archive/tauri_parity_audit.md Area 5) instead of the from-scratch
// reimplementation this module used to carry. Owns the Tauri-side
// orchestration dev's now-deleted `hlcr/ui.rs` used to provide: concurrent
// job scheduling, a JIT multi-drive export pool, per-job progress/state,
// a `.render_autosave.json` crash-recovery lockfile, and a wake lock held
// for the batch's duration.

use native::hlcr::autosave::{
    RenderJob as AutosaveJob, RenderJobStatus as AutosaveJobStatus, RenderSessionData,
};
use native::hlcr::config::{RenderCodec, RenderConfig};
use native::hlcr::renderer::{
    RenderUpdate, RenderWakeLock, hold_render_wake_lock, job_reservation_estimate, run_render_job,
};
use native::hlcr::scanner::{ClipData, clip_is_skip_eligible, scan_folder_background};
use native::hlcr::take_meta::{RenderAttempt, RenderOutcome};
use native::log_markdown;
use native::shared::paths::take_key;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[path = "render_scan_merge.rs"]
mod scan_merge;
pub use scan_merge::ScanMergeSummary;
use scan_merge::{NewJobSettings, ScannedTake, merge_scan};

/// Codec is a string id ("prores" | "dnxhr" | "h264" | "h264_nvenc") mapped
/// to `RenderCodec` via `RenderCodec::from_str_id` — see
/// docs/archive/tauri_parity_audit.md Area 5 for why `h264`/software libx264 exists
/// alongside dev's NVENC-only H.264 variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderBatchPayload {
    pub render_directories: Vec<String>,
    pub codec: String,
    /// Raw FFmpeg args, only read when `codec == "custom"`. Defaulted for
    /// backward compat with a frontend build that predates the Custom codec.
    #[serde(default)]
    pub custom_codec_args: String,
    pub fps: u32,
    pub ffmpeg_path: Option<String>,
    /// JIT multi-drive export pool, priority order — `run_render_job` picks
    /// the first entry with 20 GiB+ free.
    pub export_directories: Vec<String>,
    pub max_concurrent_renders: usize,
    /// Clip names by take key (#441), from the loaded project: the finished
    /// file of each of those takes is named after its highlight.
    #[serde(default)]
    pub clip_names: std::collections::HashMap<String, String>,
}

/// Resolve the FFmpeg binary path using the same fallback chain as the
/// legacy `settings::resolve_ffmpeg_path()`: override → bundled local →
/// system PATH.
pub(crate) fn resolve_ffmpeg(override_path: Option<&String>) -> PathBuf {
    if let Some(p) = override_path {
        let pb = PathBuf::from(p);
        if !p.trim().is_empty() && pb.exists() {
            return pb;
        }
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        let local = parent.join("local/tools/ffmpeg.exe");
        if local.exists() {
            return local;
        }
    }
    PathBuf::from("ffmpeg")
}

fn autosave_path() -> PathBuf {
    native::shared::paths::get_appdata_dir().join(".render_autosave.json")
}

// ── Per-job runtime state ───────────────────────────────────────────────────

struct RenderJobRuntime {
    id: String,
    clip: ClipData,
    status: String, // "Queued" | "Rendering" | "Finished" | "Error" | "Cancelled"
    speed: String,
    progress: u32,
    error_log: Option<String>,
    /// Absolute path to the encoded file, populated once the job finishes
    /// successfully (empty until then, and for recovered jobs — the autosave
    /// snapshot doesn't persist it).
    output_path: String,
    /// Real size of the encoded file, stat'd once at the moment `output_path`
    /// is set (never re-stat'd afterward, so a file deleted later still
    /// shows what it was). `None` until then — the frontend shows a dash
    /// rather than an estimate for anything not yet Finished; see
    /// `job_reservation_estimate`'s own doc comment for why a per-row
    /// estimate isn't shown instead.
    output_size_bytes: Option<u64>,
    cancel_flag: Arc<AtomicBool>,
    // Snapshotted onto the job itself (VirtualDub-style job queue, not one
    // shared live config) so Reset to Queued always re-renders with exactly
    // what this job was queued with — never silently picks up a codec/fps
    // change made to the panel afterward, and never silently ignores one
    // either: the Settings column shows precisely what's about to run.
    codec: RenderCodec,
    /// Raw FFmpeg args this job renders with when `codec == RenderCodec::Custom`
    /// — snapshotted alongside `codec` for the same reason: Reset always
    /// re-runs with exactly what the job was queued with.
    custom_codec_args: String,
    fps: u32,
    /// This take's render history (#438), read from its `dodstudio_take.json`
    /// when the job is queued and kept current as attempts start and end.
    history: Vec<HistoryEntry>,
    /// When the attempt in progress started, to find it again when it ends.
    current_attempt: Option<u64>,
}

/// One past attempt as the job table shows it: the recorded attempt, and
/// whether its output file was still there when last looked at.
#[derive(Clone, Serialize)]
pub struct HistoryEntry {
    #[serde(flatten)]
    attempt: RenderAttempt,
    output_exists: bool,
}

fn history_for(take_folder: &str) -> Vec<HistoryEntry> {
    native::hlcr::take_meta::read_history(std::path::Path::new(take_folder))
        .into_iter()
        .map(|attempt| HistoryEntry {
            output_exists: !attempt.output_path.is_empty()
                && std::path::Path::new(&attempt.output_path).is_file(),
            attempt,
        })
        .collect()
}

/// The newest finished render of this job's stream whose file is still
/// there (#438): `(output_path, size)`. A scan stages such a job as Finished
/// rather than Queued (#70), so Start Render Batch renders only what has no
/// render yet; Retry queues it again.
fn last_finished_output(history: &[HistoryEntry], stream: &str) -> Option<(String, Option<u64>)> {
    history
        .iter()
        .rev()
        .find(|h| {
            h.attempt.stream == stream
                && h.attempt.outcome == RenderOutcome::Finished
                && h.output_exists
        })
        .map(|h| {
            let size = h.attempt.output_size_bytes.or_else(|| {
                std::fs::metadata(&h.attempt.output_path)
                    .ok()
                    .map(|m| m.len())
            });
            (h.attempt.output_path.clone(), size)
        })
}

/// The first non-empty line of an FFmpeg error log, cut to a readable
/// length, for a failed attempt's history line.
fn first_error_line(log: &str) -> Option<String> {
    let line = log.lines().map(str::trim).find(|l| !l.is_empty())?;
    Some(line.chars().take(200).collect())
}

/// The stream an attempt is recorded under: one take can be two jobs.
fn attempt_stream(clip: &ClipData) -> String {
    if clip.clip_type == "hud_only" {
        format!("hud:{}", clip.img_folder)
    } else {
        clip.img_folder.clone()
    }
}

#[derive(Clone, Serialize)]
pub struct RenderJobView {
    pub id: String,
    pub name: String,
    pub stream: String,
    pub frames: usize,
    pub date: String,
    pub status: String,
    pub speed: String,
    pub progress: u32,
    pub error_log: Option<String>,
    /// e.g. "ProRes @ 300fps" — this job's own settings, not the panel's
    /// current ones. A single summary string rather than separate
    /// codec/fps columns so a future new setting doesn't need its own
    /// column too.
    pub settings_summary: String,
    /// Encoded output file, once finished. Empty until then.
    pub output_path: String,
    /// Real size of the encoded file, once finished. `None` otherwise — the
    /// frontend shows a dash rather than a per-row estimate for anything not
    /// yet Finished.
    pub output_size_bytes: Option<u64>,
    /// Source take folder (captured BMPs/WAV) — always known, even before
    /// this job has rendered, so "reveal in Explorer" works pre- and
    /// post-render.
    pub take_folder: String,
    /// This job's codec as a `RenderCodec::to_str_id()` value — what
    /// `set_render_job_codec` expects back, and what the frontend checks
    /// against `"source_copy"` to show the Skip toggle as checked.
    pub codec_id: String,
    /// This job's raw custom-codec args, when `codec_id == "custom"` —
    /// empty otherwise. The frontend carries this forward across a Skip
    /// toggle the same way it already does `codec_id`, so unchecking Skip
    /// restores the actual args, not just the fact that the codec was Custom.
    pub custom_codec_args: String,
    /// Whether "Skip (keep original)" is an offerable choice for this job —
    /// only true for an OBS-shaped clip (its own muxed-in audio, not HUD/alpha,
    /// a captured video). `set_render_job_codec` enforces the same rule; this
    /// is what lets the frontend decide whether to show the toggle at all.
    pub skip_available: bool,
    /// Every recorded render of this job's stream of the take, oldest first
    /// (#438).
    pub history: Vec<HistoryEntry>,
    /// When the attempt rendering now started: the history's `started` entry
    /// with this time is in progress, any other `started` one was cut off.
    pub current_attempt: Option<u64>,
}

impl RenderJobRuntime {
    fn to_view(&self) -> RenderJobView {
        let is_source_copy = self.codec == RenderCodec::SourceCopy;
        // Custom's `label()` is just the fixed word "Custom" -- the actual
        // args live in `custom_codec_args`, so the summary has to build them
        // in specially rather than through `label()` alone.
        let codec_label = if self.codec == RenderCodec::Custom {
            format!("Custom ({})", self.custom_codec_args)
        } else {
            self.codec.label().to_string()
        };
        RenderJobView {
            id: self.id.clone(),
            name: self
                .clip
                .clip_name
                .clone()
                .unwrap_or_else(|| self.clip.base_name.clone()),
            stream: if self.clip.clip_type == "hud_only" {
                "HUD ONLY".to_string()
            } else {
                self.clip.img_folder.clone()
            },
            frames: self.clip.frame_count,
            date: self.clip.date.clone(),
            status: self.status.clone(),
            speed: self.speed.clone(),
            progress: self.progress,
            error_log: self.error_log.clone(),
            // Skip mode never reads `fps` — showing it would imply a setting
            // that has no effect on a plain file copy.
            settings_summary: if is_source_copy {
                codec_label
            } else {
                format!("{} @ {}fps", codec_label, self.fps)
            },
            output_path: self.output_path.clone(),
            output_size_bytes: self.output_size_bytes,
            take_folder: self.clip.take_folder.clone(),
            codec_id: self.codec.to_str_id().to_string(),
            custom_codec_args: self.custom_codec_args.clone(),
            skip_available: clip_is_skip_eligible(&self.clip),
            history: {
                let stream = attempt_stream(&self.clip);
                self.history
                    .iter()
                    .filter(|h| h.attempt.stream == stream)
                    .cloned()
                    .collect()
            },
            current_attempt: self.current_attempt,
        }
    }
}

// ── Manager state ────────────────────────────────────────────────────────────

pub struct RenderManager {
    jobs: Arc<Mutex<Vec<RenderJobRuntime>>>,
    is_rendering: Arc<AtomicBool>,
    global_cancel: Arc<AtomicBool>,
    wake_lock: Arc<Mutex<Option<RenderWakeLock>>>,
    render_session: Arc<Mutex<Option<RenderSessionData>>>,
    /// Persisted so Start can run the existing job list again (after a
    /// Reset) without a fresh scan+payload.
    last_config: Arc<Mutex<Option<RenderConfig>>>,
    /// Why a recovered batch has no `last_config` (#770): what Start says
    /// instead of "nothing queued" until a scan replaces the batch.
    recovery_gap: Arc<Mutex<Option<String>>>,
    /// JIT export-drive reservation ledger — bytes each concurrently-running
    /// `run_render_job` has provisionally claimed per drive, so one job's
    /// drive check accounts for what other in-flight jobs have already
    /// claimed instead of only seeing the drive's live free-space number
    /// (which several jobs starting in the same scheduler tick would all
    /// see as identical).
    export_reservations: Arc<Mutex<HashMap<PathBuf, u64>>>,
    /// Clear Queue (#70) while a batch runs: the scheduler empties the list
    /// once the cancelled renders have stopped, since a Rendering row can't
    /// be removed under a live FFmpeg.
    clear_on_finish: Arc<AtomicBool>,
    /// A Scan for Takes is walking the folders (#772): a second one waits.
    scanning: AtomicBool,
    /// The next unused job id: a scan merges into the list (#772), so new
    /// rows take ids no earlier row had. Read and written under `jobs`.
    next_job_id: AtomicUsize,
}

struct SchedulerHandles {
    jobs: Arc<Mutex<Vec<RenderJobRuntime>>>,
    is_rendering: Arc<AtomicBool>,
    global_cancel: Arc<AtomicBool>,
    wake_lock: Arc<Mutex<Option<RenderWakeLock>>>,
    render_session: Arc<Mutex<Option<RenderSessionData>>>,
    export_reservations: Arc<Mutex<HashMap<PathBuf, u64>>>,
    clear_on_finish: Arc<AtomicBool>,
}

/// Back to Queued with the run's leftovers cleared, keeping the job's own
/// codec/fps (see `reset_render_job`).
fn requeue(job: &mut RenderJobRuntime) {
    job.status = "Queued".to_string();
    job.progress = 0;
    job.speed = String::new();
    job.error_log = None;
    job.output_size_bytes = None;
    job.cancel_flag = Arc::new(AtomicBool::new(false));
}

impl RenderManager {
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(Mutex::new(Vec::new())),
            is_rendering: Arc::new(AtomicBool::new(false)),
            global_cancel: Arc::new(AtomicBool::new(false)),
            wake_lock: Arc::new(Mutex::new(None)),
            render_session: Arc::new(Mutex::new(None)),
            last_config: Arc::new(Mutex::new(None)),
            recovery_gap: Arc::new(Mutex::new(None)),
            export_reservations: Arc::new(Mutex::new(HashMap::new())),
            clear_on_finish: Arc::new(AtomicBool::new(false)),
            scanning: AtomicBool::new(false),
            next_job_id: AtomicUsize::new(0),
        }
    }

    /// The statuses of the jobs rendering from `take_folder`: two for a take
    /// with a HUD pair, none for a take not in the list. Delete Take (#644)
    /// refuses on these.
    pub(crate) fn take_statuses(&self, take_folder: &str) -> Vec<String> {
        self.jobs
            .lock()
            .unwrap()
            .iter()
            .filter(|j| j.clip.take_folder == take_folder)
            .map(|j| j.status.clone())
            .collect()
    }

    /// Takes these takes' rows out of the list before their folders go to the
    /// Recycle Bin (#644), checking under the same lock that none is rendering,
    /// so a Reset can't start one in between. A queued row goes with the rest:
    /// the scheduler starts jobs under this lock too, so a job removed here
    /// never starts, which is how Delete Take pulls a take from the queue.
    pub(crate) fn remove_take_rows(
        &self,
        app: &AppHandle,
        take_folders: &[String],
    ) -> Result<(), String> {
        {
            let mut jobs = self.jobs.lock().unwrap();
            for folder in take_folders {
                let statuses: Vec<&str> = jobs
                    .iter()
                    .filter(|j| &j.clip.take_folder == folder)
                    .map(|j| j.status.as_str())
                    .collect();
                if let Some(refusal) = crate::take_delete_manager::refusal(folder, &statuses) {
                    return Err(refusal);
                }
            }
            jobs.retain(|j| !take_folders.contains(&j.clip.take_folder));
        }
        clear_batch_state_if_empty(self);
        emit_jobs_snapshot(app, &self.jobs);
        Ok(())
    }

    /// Whether this process already holds a batch (#771): a page reload
    /// over it is not an interruption to recover from.
    fn holds_batch(&self) -> bool {
        self.is_rendering.load(Ordering::SeqCst) || !self.jobs.lock().unwrap().is_empty()
    }

    fn views(&self) -> Vec<RenderJobView> {
        self.jobs
            .lock()
            .unwrap()
            .iter()
            .map(RenderJobRuntime::to_view)
            .collect()
    }

    /// What Start runs with, and how many rows it starts. A recovered batch
    /// that couldn't rebuild its config says what a scan has to fill in.
    fn start_config(&self) -> Result<(RenderConfig, usize), String> {
        let starting = self
            .jobs
            .lock()
            .unwrap()
            .iter()
            .filter(|j| j.status == "Queued")
            .count();
        if starting == 0 {
            return Err(crate::messages::NOTHING_QUEUED_TO_RENDER.to_string());
        }
        match self.last_config.lock().unwrap().clone() {
            Some(config) => Ok((config, starting)),
            None => Err(self
                .recovery_gap
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| crate::messages::NOTHING_QUEUED_TO_RENDER.to_string())),
        }
    }

    /// Puts an autosaved batch back as the job list (#770), with the config
    /// Start needs rebuilt from the autosave and the current settings.
    fn install_recovered(
        &self,
        session: RenderSessionData,
        settings: &RecoverySettings,
    ) -> Vec<RenderJobView> {
        let jobs = recovered_jobs(&session);
        match recovered_config(&session, settings) {
            Ok(config) => {
                *self.last_config.lock().unwrap() = Some(config);
                *self.recovery_gap.lock().unwrap() = None;
            }
            Err(gap) => {
                log_markdown(&format!("[render] recover_render_batch: {}", gap));
                *self.last_config.lock().unwrap() = None;
                *self.recovery_gap.lock().unwrap() = Some(gap);
            }
        }
        *self.jobs.lock().unwrap() = jobs;
        *self.render_session.lock().unwrap() = Some(session);
        self.views()
    }

    fn handles(&self) -> SchedulerHandles {
        SchedulerHandles {
            jobs: self.jobs.clone(),
            is_rendering: self.is_rendering.clone(),
            global_cancel: self.global_cancel.clone(),
            wake_lock: self.wake_lock.clone(),
            render_session: self.render_session.clone(),
            export_reservations: self.export_reservations.clone(),
            clear_on_finish: self.clear_on_finish.clone(),
        }
    }
}

fn write_autosave(
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
fn autosave_entry<'a>(
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
fn save_autosave(session: &RenderSessionData) {
    if let Ok(json) = serde_json::to_string_pretty(session)
        && let Err(e) = std::fs::write(autosave_path(), json)
    {
        log::warn!("[render_autosave] Failed to write lockfile: {}", e);
    }
}

fn emit_jobs_snapshot(app: &AppHandle, jobs: &Arc<Mutex<Vec<RenderJobRuntime>>>) {
    let views: Vec<RenderJobView> = jobs
        .lock()
        .unwrap()
        .iter()
        .map(RenderJobRuntime::to_view)
        .collect();
    let _ = app.emit("render_jobs_snapshot", views);
}

fn apply_render_update(
    app: &AppHandle,
    jobs: &Arc<Mutex<Vec<RenderJobRuntime>>>,
    render_session: &Arc<Mutex<Option<RenderSessionData>>>,
    update: RenderUpdate,
) {
    match update {
        RenderUpdate::Progress(id, pct) => {
            if let Some(job) = jobs.lock().unwrap().iter_mut().find(|j| j.id == id) {
                job.progress = pct;
            }
        }
        RenderUpdate::Speed(id, speed) => {
            if let Some(job) = jobs.lock().unwrap().iter_mut().find(|j| j.id == id) {
                job.speed = speed;
            }
        }
        RenderUpdate::Status(id, status) => {
            if let Some(job) = jobs.lock().unwrap().iter_mut().find(|j| j.id == id) {
                job.status = status;
            }
        }
        RenderUpdate::OutputPath(id, path) => {
            if let Some(job) = jobs.lock().unwrap().iter_mut().find(|j| j.id == id) {
                // Stat'd once, here, rather than on every `to_view()` call —
                // this only ever arrives once per job, right after the file
                // has been fully written (see run_render_job), so this is
                // both the one moment the size is knowable and the only
                // moment it needs computing.
                job.output_size_bytes = std::fs::metadata(&path).ok().map(|m| m.len());
                job.output_path = path;
            }
        }
        RenderUpdate::Finished(id, success, err_log) => {
            // `just_finished` is keyed on `success` alone, deliberately not
            // on the `job.status == "Rendering"` guard below: on the real
            // completion path run_render_job sends Status("Finished") *then*
            // Finished(true, None), so by the time this arrives job.status
            // already reads "Finished" and that guard has already tripped —
            // it exists to stop this generic transition from clobbering a
            // more specific status a preceding Status update already set
            // (e.g. never overwrite "Cancelled" with "Error"), not to gate
            // "is this the one authoritative completion". Each run_render_job
            // invocation sends exactly one Finished, at its single return
            // point, so success=true can't double-fire this within one run;
            // recover_render_batch separately can't trigger it at all, since
            // it repopulates already-Finished jobs by constructing them
            // directly rather than replaying this update.
            let mut just_finished = None;
            let mut just_failed = None;
            let mut attempt_ended = None;
            let mut finished_clip: Option<ClipData> = None;
            {
                let mut guard = jobs.lock().unwrap();
                if let Some(job) = guard.iter_mut().find(|j| j.id == id) {
                    // #438: how this attempt ended, for the take's history.
                    // Status updates have already run, so "Cancelled" is
                    // already set for a cancelled job.
                    if let Some(started) = job.current_attempt.take() {
                        let outcome = if success {
                            RenderOutcome::Finished
                        } else if job.status == "Cancelled" {
                            RenderOutcome::Cancelled
                        } else {
                            RenderOutcome::Failed
                        };
                        let error = (outcome == RenderOutcome::Failed)
                            .then(|| err_log.as_deref().and_then(first_error_line))
                            .flatten();
                        let path = if success {
                            job.output_path.clone()
                        } else {
                            String::new()
                        };
                        let size = if success { job.output_size_bytes } else { None };
                        if let Some(entry) = job
                            .history
                            .iter_mut()
                            .rev()
                            .find(|h| h.attempt.started_unix_ms == started)
                        {
                            entry.attempt.outcome = outcome;
                            entry.attempt.output_path = path.clone();
                            entry.attempt.output_size_bytes = size;
                            entry.attempt.error = error.clone();
                            entry.output_exists = success;
                        }
                        attempt_ended = Some((
                            job.clip.take_folder.clone(),
                            started,
                            attempt_stream(&job.clip),
                            outcome,
                            path,
                            size,
                            error,
                        ));
                    }
                    if job.status == "Rendering" {
                        job.status = if err_log.is_some() {
                            "Error".to_string()
                        } else {
                            "Finished".to_string()
                        };
                        if job.status == "Finished" {
                            job.progress = 100;
                        }
                    }
                    if success {
                        finished_clip = Some(job.clip.clone());
                        just_finished = Some((
                            job.clip.take_folder.clone(),
                            job.clip.base_name.clone(),
                            job.clip.clip_type.clone(),
                            job.output_path.clone(),
                        ));
                    } else {
                        just_failed = Some((job.clip.base_name.clone(), err_log.clone()));
                    }
                    job.error_log = err_log;
                }
            }
            if let Some((take_folder, started, stream, outcome, path, size, error)) = attempt_ended
                && let Err(e) = native::hlcr::take_meta::record_render_end(
                    std::path::Path::new(&take_folder),
                    started,
                    &stream,
                    outcome,
                    &path,
                    size,
                    error,
                )
            {
                log_markdown(&format!(
                    "[render-history] couldn't record how a render of {} ended: {}",
                    take_folder, e
                ));
            }
            if let Some((take_folder, base_name, clip_type, _)) = &just_finished {
                let key = take_key(std::path::Path::new(take_folder));
                log_markdown(&format!(
                    "[render-take-finished] job {} take_key={:?} take_folder={} base_name={} clip_type={}",
                    id, key, take_folder, base_name, clip_type
                ));
                let _ = app.emit(
                    "render_take_finished",
                    serde_json::json!({
                        "job_id": id.clone(),
                        "take_key": key,
                        "take_folder": take_folder,
                        "base_name": base_name,
                        "clip_type": clip_type,
                    }),
                );
            }
            if let Some((base_name, err_log)) = &just_failed {
                log_markdown(&format!(
                    "[render-take-failed] job {} base_name={} error={}",
                    id,
                    base_name,
                    err_log.as_deref().unwrap_or("(no error log)")
                ));
            }
            // Autosave only tracks success — matches dev's ui.rs exactly.
            if success && let Some(session) = render_session.lock().unwrap().as_mut() {
                if let Some(clip) = &finished_clip
                    && let Some(rj) = autosave_entry(session, clip)
                {
                    rj.status = AutosaveJobStatus::Completed;
                    if let Some((_, _, _, output_path)) = &just_finished {
                        rj.output_path = output_path.clone();
                    }
                }
                if let Ok(json) = serde_json::to_string_pretty(session) {
                    let _ = std::fs::write(autosave_path(), json);
                }
            }
        }
    }
}

/// Concurrent job scheduler — ports dev's `HlcrState::update_channels`
/// per-frame scheduling loop (`hlcr/ui.rs`, deleted at HEAD) to a periodic
/// async poll: starts queued jobs up to `max_concurrent_renders`, giving a
/// lone job all available CPU threads via the same `effective_concurrent`
/// calculation dev used, and tears down (wake lock, autosave lockfile) once
/// nothing is Rendering or Queued.
fn spawn_scheduler(app: AppHandle, handles: SchedulerHandles, config: RenderConfig) {
    tokio::spawn(async move {
        let (tx, rx) = std::sync::mpsc::channel::<RenderUpdate>();
        let max_concurrent = config.max_concurrent_renders.max(1);

        loop {
            if handles.global_cancel.load(Ordering::SeqCst) {
                let mut guard = handles.jobs.lock().unwrap();
                for job in guard.iter_mut() {
                    if job.status == "Rendering" || job.status == "Queued" {
                        job.cancel_flag.store(true, Ordering::Relaxed);
                        if job.status == "Queued" {
                            job.status = "Cancelled".to_string();
                        }
                    }
                }
            }

            let mut dirty = false;
            while let Ok(update) = rx.try_recv() {
                apply_render_update(&app, &handles.jobs, &handles.render_session, update);
                dirty = true;
            }

            let mut attempts_started: Vec<(String, RenderAttempt)> = Vec::new();
            let started_any = {
                let mut guard = handles.jobs.lock().unwrap();
                let active_count = guard.iter().filter(|j| j.status == "Rendering").count();
                let mut started = 0usize;
                if active_count < max_concurrent {
                    let limit = max_concurrent - active_count;
                    let queued_count = guard.iter().filter(|j| j.status == "Queued").count();
                    let jobs_starting = queued_count.min(limit);
                    // Real concurrent count (already-running + newly starting), capped —
                    // gives a lone job all available threads instead of only 1/max of them.
                    let effective_concurrent =
                        (active_count + jobs_starting).min(max_concurrent).max(1);

                    for job in guard.iter_mut() {
                        if started >= limit {
                            break;
                        }
                        if job.status == "Queued" {
                            job.status = "Rendering".to_string();
                            // #438: recorded as Started before
                            // FFmpeg runs, written once the lock is released.
                            let attempt = RenderAttempt {
                                started_unix_ms: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_millis() as u64)
                                    .unwrap_or(0),
                                stream: attempt_stream(&job.clip),
                                codec: job.codec.to_str_id().to_string(),
                                custom_codec_args: if job.codec == RenderCodec::Custom {
                                    job.custom_codec_args.clone()
                                } else {
                                    String::new()
                                },
                                fps: job.fps,
                                outcome: RenderOutcome::Started,
                                output_path: String::new(),
                                output_size_bytes: None,
                                error: None,
                            };
                            job.current_attempt = Some(attempt.started_unix_ms);
                            job.history.push(HistoryEntry {
                                attempt: attempt.clone(),
                                output_exists: false,
                            });
                            attempts_started.push((job.clip.take_folder.clone(), attempt));
                            let job_id = job.id.clone();
                            let clip = job.clip.clone();
                            let cancel_flag = job.cancel_flag.clone();
                            // Codec/fps come from the job itself, not the
                            // scheduler's shared config — see the comment on
                            // RenderJobRuntime. Everything else (ffmpeg path,
                            // export pool, concurrency) genuinely is
                            // batch-wide, so still comes from config.
                            let mut job_config = config.clone();
                            job_config.target_codec = job.codec;
                            job_config.custom_codec_args = job.custom_codec_args.clone();
                            job_config.fps = job.fps;
                            job_config.max_concurrent_renders = effective_concurrent;
                            let tx2 = tx.clone();
                            let reservations = handles.export_reservations.clone();
                            tokio::spawn(async move {
                                run_render_job(
                                    job_id,
                                    clip,
                                    job_config,
                                    tx2,
                                    cancel_flag,
                                    reservations,
                                )
                                .await;
                            });
                            started += 1;
                        }
                    }
                }
                started > 0
            };
            for (take_folder, attempt) in attempts_started {
                if let Err(e) = native::hlcr::take_meta::record_render_start(
                    std::path::Path::new(&take_folder),
                    &attempt,
                ) {
                    log_markdown(&format!(
                        "[render-history] couldn't record the start of a render of {}: {}",
                        take_folder, e
                    ));
                }
            }

            if dirty || started_any {
                emit_jobs_snapshot(&app, &handles.jobs);
            }

            // The check and the teardown happen under one jobs lock: a scan
            // merging Queued rows in (#772) either lands before it, and the
            // loop keeps going for them, or after it, and writes its own
            // autosave once this one is gone.
            let finished = {
                let mut guard = handles.jobs.lock().unwrap();
                let has_active_or_queued = guard
                    .iter()
                    .any(|j| j.status == "Rendering" || j.status == "Queued");
                if !has_active_or_queued {
                    *handles.wake_lock.lock().unwrap() = None; // Drop releases the wake lock.
                    let _ = std::fs::remove_file(autosave_path());
                    *handles.render_session.lock().unwrap() = None;
                    // Clear Queue (#70) asked while this batch ran: every
                    // render has stopped by now, so the whole list can go.
                    if handles.clear_on_finish.swap(false, Ordering::SeqCst) {
                        guard.clear();
                        log_markdown(
                            "[render] clear_render_queue: list emptied once the batch stopped",
                        );
                    }
                    handles.global_cancel.store(false, Ordering::SeqCst);
                    handles.is_rendering.store(false, Ordering::SeqCst);
                }
                !has_active_or_queued
            };
            if finished {
                break;
            }

            tokio::time::sleep(Duration::from_millis(150)).await;
        }

        log_markdown("[render] scheduler loop exiting: nothing left Rendering or Queued");
        emit_jobs_snapshot(&app, &handles.jobs);
        let _ = app.emit("render_batch_finished", serde_json::json!({}));
    });
}

/// Scans and stages a batch as `Queued` jobs — real rows in the job table,
/// each with its own settings snapshot and (for an OBS-shaped take) a
/// Skip toggle — without starting the scheduler. Split from starting so a
/// batch has a genuine "queued, not yet running" window: the previous
/// single Start action queued and launched the scheduler in the same call,
/// which for a small batch (especially Skip jobs, which are a plain file
/// copy) left no realistic time to review or toggle anything before it was
/// already done.
///
/// A scan merges into the list rather than replacing it (#772), so it works
/// while rows are Queued or Rendering and renders keep running: see
/// `merge_scan` for what happens to each row. A second scan while one runs
/// is refused.
#[tauri::command]
pub async fn queue_render_batch(
    app: AppHandle,
    state: tauri::State<'_, RenderManager>,
    payload: RenderBatchPayload,
) -> Result<ScanMergeSummary, String> {
    if state.scanning.swap(true, Ordering::SeqCst) {
        return Err(crate::messages::RENDER_SCAN_ALREADY_RUNNING.to_string());
    }
    let result = scan_and_merge(&app, &state, payload).await;
    state.scanning.store(false, Ordering::SeqCst);
    result
}

async fn scan_and_merge(
    app: &AppHandle,
    state: &RenderManager,
    payload: RenderBatchPayload,
) -> Result<ScanMergeSummary, String> {
    log_markdown(&format!(
        "[render] queue_render_batch starting: {} source dir(s), codec={}, fps={}, max_concurrent={}",
        payload.render_directories.len(),
        payload.codec,
        payload.fps,
        payload.max_concurrent_renders
    ));

    // The folders of the rows already listed, to tell a take that is gone
    // from one this scan just didn't reach. Checked with the walk below, off
    // the jobs lock.
    let mut known_folders: Vec<String> = state
        .jobs
        .lock()
        .unwrap()
        .iter()
        .map(|j| j.clip.take_folder.clone())
        .collect();
    known_folders.sort();
    known_folders.dedup();

    let scan_app = app.clone();
    let clip_names = payload.clip_names.clone();
    let (scanned, gone_folders): (Vec<ScannedTake>, HashSet<String>) =
        crate::messages::spawn_blocking_result(tokio::task::spawn_blocking({
            let dirs = payload.render_directories.clone();
            move || {
                let source_folders: Vec<PathBuf> = dirs.into_iter().map(PathBuf::from).collect();
                let (clip_tx, clip_rx) = std::sync::mpsc::channel();
                let (status_tx, status_rx) = std::sync::mpsc::channel::<String>();

                // scan_folder_background sends one status line per take it finds
                // (blocking, on this thread); drain it concurrently on its own
                // thread so the frontend gets live progress instead of the whole
                // batch arriving at once when the scan finishes. Exits on its own
                // once status_tx drops (scan_folder_background returns below).
                let status_thread = std::thread::spawn(move || {
                    while let Ok(msg) = status_rx.recv() {
                        let _ = scan_app.emit("render_scan_status", msg);
                    }
                });

                // scan_folder_background is blocking and exhausts sends before returning.
                // try_recv is therefore race-free here.
                scan_folder_background(source_folders, clip_tx, status_tx);
                let _ = status_thread.join();

                let mut clips: Vec<ClipData> = Vec::new();
                while let Ok(clip) = clip_rx.try_recv() {
                    clips.push(clip);
                }
                // Each take's history and surviving output are read here too,
                // so the merge below only moves rows around under the lock.
                let scanned: Vec<ScannedTake> = clips
                    .into_iter()
                    .map(|mut clip| {
                        clip.clip_name = take_key(std::path::Path::new(&clip.take_folder))
                            .and_then(|key| clip_names.get(&key).cloned());
                        let history = history_for(&clip.take_folder);
                        // A stream rendered before, with the file still
                        // there, comes in as Finished (#70): Start renders
                        // what has no render yet, and Retry queues it again.
                        let finished_output =
                            last_finished_output(&history, &attempt_stream(&clip));
                        ScannedTake {
                            clip,
                            history,
                            finished_output,
                        }
                    })
                    .collect();
                let gone: HashSet<String> = known_folders
                    .into_iter()
                    .filter(|f| !std::path::Path::new(f).is_dir())
                    .collect();
                (scanned, gone)
            }
        }))
        .await?;

    let ffmpeg_path = resolve_ffmpeg(payload.ffmpeg_path.as_ref())
        .to_string_lossy()
        .into_owned();
    let export_directories: Vec<PathBuf> = payload
        .export_directories
        .iter()
        .map(PathBuf::from)
        .collect();
    let config = RenderConfig {
        ffmpeg_path,
        source_folder: payload
            .render_directories
            .first()
            .cloned()
            .unwrap_or_default(),
        export_directories,
        fps: payload.fps.max(1),
        target_codec: RenderCodec::from_str_id(&payload.codec),
        custom_codec_args: payload.custom_codec_args.clone(),
        max_concurrent_renders: payload.max_concurrent_renders.max(1),
    };
    let new_job_settings = NewJobSettings {
        codec: config.target_codec,
        custom_codec_args: config.custom_codec_args.clone(),
        fps: config.fps,
    };

    // The merge and the autosave rewrite share one jobs lock, so the
    // scheduler can neither start a row mid-merge nor tear the autosave down
    // between the two (see its exit check). Nothing here touches the disk
    // but the autosave write itself.
    let (summary, empty) = {
        let mut guard = state.jobs.lock().unwrap();
        let mut next_id = state.next_job_id.load(Ordering::SeqCst);
        let (merged, summary) = merge_scan(
            std::mem::take(&mut *guard),
            scanned,
            &gone_folders,
            &new_job_settings,
            &mut next_id,
        );
        state.next_job_id.store(next_id, Ordering::SeqCst);
        *guard = merged;
        if !guard.is_empty() {
            write_autosave(&state.render_session, &guard, &config);
        }
        (summary, guard.is_empty())
    };
    log_markdown(&format!(
        "[render] queue_render_batch: {} take(s) found; {} new, {} refreshed, {} kept, {} gone ({} of them queued)",
        summary.total,
        summary.added,
        summary.refreshed,
        summary.kept,
        summary.gone,
        summary.gone_queued
    ));

    if empty {
        clear_batch_state_if_empty(state);
    } else {
        // What the next Start runs with; a batch already rendering keeps the
        // config it started with.
        *state.last_config.lock().unwrap() = Some(config);
        *state.recovery_gap.lock().unwrap() = None;
    }

    emit_jobs_snapshot(app, &state.jobs);

    Ok(summary)
}

/// Starts the scheduler on whatever `queue_render_batch` already staged.
/// Takes no settings of its own — a queued job carries its own
/// codec/fps snapshot (including any Skip toggle applied while it sat
/// Queued), and batch-wide infrastructure (ffmpeg path, export pool,
/// concurrency) came from `queue_render_batch`'s payload via `last_config`.
///
/// Every Queued row renders (#70): the tick boxes scope Cancel, Retry,
/// Remove and Delete, not Start. A row to leave out is cancelled or removed.
#[tauri::command]
pub async fn start_queued_render(
    app: AppHandle,
    state: tauri::State<'_, RenderManager>,
) -> Result<(), String> {
    if state.is_rendering.swap(true, Ordering::SeqCst) {
        log_markdown("[render] start_queued_render rejected: a batch is already in progress");
        return Err(crate::messages::RENDER_BATCH_ALREADY_IN_PROGRESS.to_string());
    }
    let (config, starting) = match state.start_config() {
        Ok(found) => found,
        Err(e) => {
            state.is_rendering.store(false, Ordering::SeqCst);
            return Err(e);
        }
    };

    log_markdown(&format!(
        "[render] start_queued_render: starting scheduler on {} job(s)",
        starting
    ));
    state.global_cancel.store(false, Ordering::SeqCst);
    *state.wake_lock.lock().unwrap() = hold_render_wake_lock();
    spawn_scheduler(app, state.handles(), config);

    Ok(())
}

#[tauri::command]
pub async fn cancel_render_batch(
    app: AppHandle,
    state: tauri::State<'_, RenderManager>,
) -> Result<(), String> {
    log_markdown("[render] cancel_render_batch requested");
    if state.is_rendering.load(Ordering::SeqCst) {
        state.global_cancel.store(true, Ordering::SeqCst);
    } else {
        // Nothing is actively rendering, so this is a staged batch (queued
        // via queue_render_batch, Start not yet clicked) — no scheduler loop
        // is running to ever observe global_cancel in that state. Mark every
        // Queued job Cancelled directly instead — mirrors exactly what the
        // scheduler loop above does when global_cancel fires mid-render, so
        // Cancel All behaves the same regardless of whether Start was ever
        // clicked. Rows stay in the list (Reset can still revive one, Remove
        // All clears them) rather than being wiped outright.
        let mut jobs = state.jobs.lock().unwrap();
        for job in jobs.iter_mut() {
            if job.status == "Queued" {
                job.cancel_flag.store(true, Ordering::Relaxed);
                job.status = "Cancelled".to_string();
            }
        }
        drop(jobs);
        emit_jobs_snapshot(&app, &state.jobs);
    }
    Ok(())
}

/// Changes one Queued job's codec — the VirtualDub-style per-job setting
/// `RenderJobRuntime` already carries, exposed here specifically so a job can
/// be flipped to/from `SourceCopy` ("Skip, keep original") before it starts.
/// Refused once a job is no longer Queued: a running or finished job's
/// settings are fixed, matching `reset_render_job`'s own doc comment on why
/// per-job settings never change out from under a job in flight.
#[tauri::command]
pub async fn set_render_job_codec(
    app: AppHandle,
    state: tauri::State<'_, RenderManager>,
    job_id: String,
    codec: String,
    custom_codec_args: Option<String>,
) -> Result<(), String> {
    let requested = RenderCodec::from_str_id(&codec);
    let mut jobs = state.jobs.lock().unwrap();
    let Some(job) = jobs.iter_mut().find(|j| j.id == job_id) else {
        return Err(crate::messages::no_such_job(&job_id));
    };
    if job.status != "Queued" {
        return Err(crate::messages::job_not_queued(&job_id, &job.status));
    }
    if requested == RenderCodec::SourceCopy && !clip_is_skip_eligible(&job.clip) {
        return Err(crate::messages::SKIP_ONLY_FOR_OBS_TAKE.to_string());
    }
    job.codec = requested;
    // Only meaningful when `requested == Custom`, but set unconditionally --
    // the frontend always sends the value that belongs with `codec` (empty
    // for every built-in codec), so there is nothing to special-case here.
    job.custom_codec_args = custom_codec_args.unwrap_or_default();
    log_markdown(&format!(
        "[render] set_render_job_codec {} -> {}",
        job_id,
        requested.to_str_id()
    ));
    // Into the autosave too, so a crash before this job renders recovers it
    // with the codec it was given, not the batch's (#85).
    if let Some(session) = state.render_session.lock().unwrap().as_mut()
        && let Some(rj) = autosave_entry(session, &job.clip)
    {
        rj.codec = Some(job.codec.to_str_id().to_string());
        rj.custom_codec_args = Some(job.custom_codec_args.clone());
        save_autosave(session);
    }
    drop(jobs);
    emit_jobs_snapshot(&app, &state.jobs);
    Ok(())
}

#[tauri::command]
pub async fn cancel_render_job(
    app: AppHandle,
    state: tauri::State<'_, RenderManager>,
    job_id: String,
) -> Result<(), String> {
    log_markdown(&format!(
        "[render] cancel_render_job requested for {}",
        job_id
    ));
    let mut jobs = state.jobs.lock().unwrap();
    let mutated = if let Some(job) = jobs.iter_mut().find(|j| j.id == job_id) {
        job.cancel_flag.store(true, Ordering::Relaxed);
        if job.status == "Queued" {
            job.status = "Cancelled".to_string();
            true
        } else {
            false
        }
    } else {
        false
    };
    drop(jobs);
    // A Queued job's status just changed here, directly — not from inside
    // the scheduler loop, which is what every other snapshot emission relies
    // on and isn't running at all if Start Render Batch hasn't been clicked
    // yet. Without this, the row silently stays "Queued" in the UI until
    // something unrelated happens to trigger the next snapshot. See #121.
    if mutated {
        emit_jobs_snapshot(&app, &state.jobs);
    }
    Ok(())
}

/// Once a removal empties the job list entirely, there's nothing left to
/// Reset against — clear the same batch-scoped state `cancel_render_batch`
/// used to wipe unconditionally, so a stale `last_config`/autosave file
/// doesn't linger describing a batch that no longer has any rows.
fn clear_batch_state_if_empty(state: &RenderManager) {
    if state.jobs.lock().unwrap().is_empty() {
        *state.last_config.lock().unwrap() = None;
        *state.recovery_gap.lock().unwrap() = None;
        let _ = std::fs::remove_file(autosave_path());
        *state.render_session.lock().unwrap() = None;
    }
}

/// Deletes one row outright — unlike Cancel, there's no Reset back from this.
/// Refused for a Rendering job: pulling its row out from under a live ffmpeg
/// process (still writing to `export_reservations`) has no defined outcome.
/// The frontend never shows the button in that state either; this is the
/// defense-in-depth backstop, matching `set_render_job_codec`'s own guard.
#[tauri::command]
pub async fn remove_render_job(
    app: AppHandle,
    state: tauri::State<'_, RenderManager>,
    job_id: String,
) -> Result<(), String> {
    let mut jobs = state.jobs.lock().unwrap();
    let Some(idx) = jobs.iter().position(|j| j.id == job_id) else {
        return Err(crate::messages::no_such_job(&job_id));
    };
    if jobs[idx].status == "Rendering" {
        return Err(crate::messages::job_still_rendering(&job_id));
    }
    jobs.remove(idx);
    drop(jobs);
    log_markdown(&format!("[render] remove_render_job {} removed", job_id));
    clear_batch_state_if_empty(&state);
    emit_jobs_snapshot(&app, &state.jobs);
    Ok(())
}

/// Bulk form of `remove_render_job` — clears every row except whatever is
/// actively Rendering (Queued, Cancelled, Finished and Error all qualify).
#[tauri::command]
pub async fn remove_non_rendering_render_jobs(
    app: AppHandle,
    state: tauri::State<'_, RenderManager>,
) -> Result<(), String> {
    let mut jobs = state.jobs.lock().unwrap();
    let before = jobs.len();
    jobs.retain(|j| j.status == "Rendering");
    let removed_count = before - jobs.len();
    drop(jobs);
    if removed_count == 0 {
        return Ok(());
    }
    log_markdown(&format!(
        "[render] remove_non_rendering_render_jobs removed {} job(s)",
        removed_count
    ));
    clear_batch_state_if_empty(&state);
    emit_jobs_snapshot(&app, &state.jobs);
    Ok(())
}

/// Resets a Finished/Error/Cancelled job back to Queued (#70): it joins a
/// batch that is rendering, and otherwise waits for Start Render Batch like
/// every other Queued row. Reset never starts rendering on its own; doing so
/// started every other Queued row with it.
///
/// Deliberately does not touch `codec`/`fps` — a reset job re-renders with
/// exactly the settings it already carries (VirtualDub-style: each job owns
/// its settings, the panel is only ever a template for *new* jobs), never
/// whatever the panel currently shows.
fn log_requeued(state: &RenderManager, context: &str) {
    if state.is_rendering.load(Ordering::SeqCst) {
        log_markdown(&format!(
            "[render] {} queued; the running batch picks it up",
            context
        ));
    } else {
        log_markdown(&format!(
            "[render] {} queued; waits for Start Render Batch",
            context
        ));
    }
}

#[tauri::command]
pub async fn reset_render_job(
    app: AppHandle,
    state: tauri::State<'_, RenderManager>,
    job_id: String,
) -> Result<(), String> {
    let mut previous_status = None;
    {
        let mut jobs = state.jobs.lock().unwrap();
        if let Some(job) = jobs.iter_mut().find(|j| j.id == job_id) {
            previous_status = Some(job.status.clone());
            requeue(job);
        }
    }
    log_markdown(&format!(
        "[render] reset_render_job {} (was {:?}) -> Queued",
        job_id, previous_status
    ));
    emit_jobs_snapshot(&app, &state.jobs);
    log_requeued(&state, &format!("reset_render_job {}", job_id));
    Ok(())
}

/// Bulk form of `reset_render_job` — every Cancelled/Finished/Error row goes
/// back to Queued in one shot, e.g. after fixing whatever caused a batch of
/// failures and wanting to retry all of them without clicking Reset one row
/// at a time.
#[tauri::command]
pub async fn reset_all_render_jobs(
    app: AppHandle,
    state: tauri::State<'_, RenderManager>,
) -> Result<(), String> {
    let mut reset_count = 0usize;
    {
        let mut jobs = state.jobs.lock().unwrap();
        for job in jobs.iter_mut() {
            if is_terminal(job) {
                requeue(job);
                reset_count += 1;
            }
        }
    }
    if reset_count == 0 {
        return Ok(());
    }
    log_markdown(&format!(
        "[render] reset_all_render_jobs: {} job(s) -> Queued",
        reset_count
    ));
    emit_jobs_snapshot(&app, &state.jobs);
    log_requeued(&state, "reset_all_render_jobs");
    Ok(())
}

/// Cancelled, Finished or Error: a run that ended, which a reset can start
/// over.
fn is_terminal(job: &RenderJobRuntime) -> bool {
    job.status == "Cancelled" || job.status == "Finished" || job.status == "Error"
}

// ── Bulk actions on the Render tab's ticked rows (#70) ───────────────────────

/// `cancel_render_job` for several rows at once: a Rendering job stops, a
/// Queued one becomes Cancelled. Ids that are neither are left alone.
#[tauri::command]
pub async fn cancel_render_jobs(
    app: AppHandle,
    state: tauri::State<'_, RenderManager>,
    job_ids: Vec<String>,
) -> Result<(), String> {
    let mut cancelled = 0usize;
    let mut mutated = false;
    {
        let mut jobs = state.jobs.lock().unwrap();
        for job in jobs.iter_mut().filter(|j| job_ids.contains(&j.id)) {
            if job.status == "Rendering" || job.status == "Queued" {
                job.cancel_flag.store(true, Ordering::Relaxed);
                cancelled += 1;
            }
            if job.status == "Queued" {
                job.status = "Cancelled".to_string();
                mutated = true;
            }
        }
    }
    log_markdown(&format!(
        "[render] cancel_render_jobs: {} of {} ticked job(s) cancelled",
        cancelled,
        job_ids.len()
    ));
    // A Queued job changed status outside the scheduler loop, which may not
    // be running at all; the loop handles a Rendering one's own update.
    if mutated {
        emit_jobs_snapshot(&app, &state.jobs);
    }
    Ok(())
}

/// `reset_render_job` for several rows at once; only a Cancelled, Finished
/// or Error job goes back to Queued.
#[tauri::command]
pub async fn reset_render_jobs(
    app: AppHandle,
    state: tauri::State<'_, RenderManager>,
    job_ids: Vec<String>,
) -> Result<(), String> {
    let mut reset_count = 0usize;
    {
        let mut jobs = state.jobs.lock().unwrap();
        for job in jobs.iter_mut().filter(|j| job_ids.contains(&j.id)) {
            if is_terminal(job) {
                requeue(job);
                reset_count += 1;
            }
        }
    }
    if reset_count == 0 {
        return Ok(());
    }
    log_markdown(&format!(
        "[render] reset_render_jobs: {} of {} ticked job(s) -> Queued",
        reset_count,
        job_ids.len()
    ));
    emit_jobs_snapshot(&app, &state.jobs);
    log_requeued(&state, "reset_render_jobs");
    Ok(())
}

/// `remove_render_job` for several rows at once. A Rendering row stays, for
/// the reason `remove_render_job` gives; the rest of the ids still go.
#[tauri::command]
pub async fn remove_render_jobs(
    app: AppHandle,
    state: tauri::State<'_, RenderManager>,
    job_ids: Vec<String>,
) -> Result<usize, String> {
    let removed_count = {
        let mut jobs = state.jobs.lock().unwrap();
        let before = jobs.len();
        jobs.retain(|j| j.status == "Rendering" || !job_ids.contains(&j.id));
        before - jobs.len()
    };
    if removed_count == 0 {
        return Ok(0);
    }
    log_markdown(&format!(
        "[render] remove_render_jobs: {} of {} ticked job(s) removed",
        removed_count,
        job_ids.len()
    ));
    clear_batch_state_if_empty(&state);
    emit_jobs_snapshot(&app, &state.jobs);
    Ok(removed_count)
}

/// Clear Queue (#70): every row goes. With a batch running, the renders are
/// cancelled first and the scheduler empties the list once they have
/// stopped (`clear_on_finish`); the frontend confirms before asking for that.
#[tauri::command]
pub async fn clear_render_queue(
    app: AppHandle,
    state: tauri::State<'_, RenderManager>,
) -> Result<(), String> {
    if state.is_rendering.load(Ordering::SeqCst) {
        log_markdown("[render] clear_render_queue: cancelling the running batch first");
        state.clear_on_finish.store(true, Ordering::SeqCst);
        state.global_cancel.store(true, Ordering::SeqCst);
        return Ok(());
    }
    let count = {
        let mut jobs = state.jobs.lock().unwrap();
        let count = jobs.len();
        jobs.clear();
        count
    };
    log_markdown(&format!(
        "[render] clear_render_queue: {} row(s) removed",
        count
    ));
    clear_batch_state_if_empty(&state);
    emit_jobs_snapshot(&app, &state.jobs);
    Ok(())
}

/// Summed required-bytes estimate across every job still ahead of the
/// export pool — Queued and Rendering, not Finished/Error/Cancelled, which
/// have either already landed on disk or never will. Exposed so the Render
/// footer can show a "Required (Estimated)" figure mirroring Capture's own
/// Free+Required pair (issue #119).
///
/// Deliberately whole-queue, not just `export_reservations` (the JIT ledger,
/// which only ever holds bytes for jobs *currently* rendering) — a figure
/// that only reflects what's already in flight can't answer "will my batch
/// fit," which is the question this exists to answer. And deliberately a
/// loose upper bound, not a tight prediction, unlike Capture's figure — see
/// `job_reservation_estimate`'s own doc comment and #116's PR description
/// for real-vs-estimated size ratios observed live (~70-160x for
/// h264_nvenc). The frontend's label says so explicitly rather than implying
/// a number this loose is a real prediction.
#[tauri::command]
pub fn get_render_required_estimate_gb(state: tauri::State<'_, RenderManager>) -> f64 {
    let total: u64 = state
        .jobs
        .lock()
        .unwrap()
        .iter()
        .filter(|j| j.status == "Queued" || j.status == "Rendering")
        .map(|j| job_reservation_estimate(&j.clip, j.codec, &j.custom_codec_args))
        .sum();
    total as f64 / 1_073_741_824.0
}

/// On the blocking pool (#853): a disk-space cache miss re-lists every
/// volume, which can wait on a sleeping or network drive.
#[tauri::command]
pub async fn get_export_pool_free_gb(directories: Vec<String>) -> Result<f64, String> {
    crate::messages::spawn_blocking_result(tokio::task::spawn_blocking(move || {
        export_pool_free_gb(&directories)
    }))
    .await
}

fn export_pool_free_gb(directories: &[String]) -> f64 {
    let mut total: u64 = 0;
    for dir in directories {
        let free = native::sys::disk::get_available_bytes(&PathBuf::from(dir));
        if free != u64::MAX {
            total += free;
        }
    }
    total as f64 / 1_073_741_824.0
}

#[derive(Serialize)]
pub struct RenderAutosaveSummary {
    pub source_folder: String,
    pub pending_count: usize,
    pub completed_count: usize,
}

#[tauri::command(async)]
pub fn check_render_autosave(
    state: tauri::State<'_, RenderManager>,
) -> Option<RenderAutosaveSummary> {
    // A render can legitimately still be running when this fires: pressing
    // F5 reloads the frontend only, not this Rust process, so the autosave
    // file (written continuously as jobs finish, not just at batch end)
    // genuinely exists on disk even though nothing was actually interrupted
    // — the "recovery" prompt was firing on every reload of an active batch.
    // Only treat it as a real interruption when nothing is actively running,
    // and nothing is queued either (#771): a reload over a staged batch
    // leaves the file there too, and Recover would replace the live queue.
    if state.holds_batch() {
        return None;
    }
    let json = std::fs::read_to_string(autosave_path()).ok()?;
    let session: RenderSessionData = serde_json::from_str(&json).ok()?;
    recovery_summary(session)
}

/// What the recovery prompt offers, or `None` when every job had finished:
/// a scan writes the autosave too, so a scan after a batch ended (or one
/// whose takes were all rendered before) leaves a file with nothing left to
/// render, and the prompt said "didn't finish cleanly" over "Pending: 0".
fn recovery_summary(session: RenderSessionData) -> Option<RenderAutosaveSummary> {
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

#[tauri::command(async)]
pub fn discard_render_autosave() -> Result<(), String> {
    match std::fs::remove_file(autosave_path()) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Repopulates the job table from `.render_autosave.json` — Completed jobs
/// show as "Finished"/100%, Pending ones as "Queued"/0%, both via stub
/// `ClipData` (the snapshot stores take_folder/name/status and the job's
/// codec/fps, not the full scanner output). Matches dev's own recovery flow exactly
/// (`main.rs`'s render-recovery modal rebuilds the same kind of stub with
/// blank stream/frames/date — dev never had richer data to recover either).
/// Does not auto-start rendering; the user still clicks Start Render, which
/// runs with the config rebuilt here (#770).
#[tauri::command(async)]
pub fn recover_render_batch(
    state: tauri::State<'_, RenderManager>,
    settings: tauri::State<'_, crate::settings_manager::SettingsManager>,
) -> Result<Vec<RenderJobView>, String> {
    let path = autosave_path();
    let json = std::fs::read_to_string(&path)
        .map_err(|e| format!("Couldn't read {}: {}", path.display(), e))?;
    let session: RenderSessionData = serde_json::from_str(&json)
        .map_err(|e| format!("Couldn't parse {}: {}", path.display(), e))?;
    let recovery_settings = RecoverySettings::from(&*settings.inner.lock().unwrap());
    let views = state.install_recovered(session, &recovery_settings);
    let _ = std::fs::remove_file(&path);
    Ok(views)
}

/// The job list as this process holds it (#771). The page asks for it on
/// load, since a webview reload (F5, HMR) keeps the queue here but the page
/// otherwise only learns of it from the next `render_jobs_snapshot`.
#[tauri::command(async)]
pub fn get_render_jobs(state: tauri::State<'_, RenderManager>) -> Vec<RenderJobView> {
    state.views()
}

/// The batch-wide settings a recovered batch takes from Settings, the same
/// ones the Render tab puts in a scan's payload: the autosave doesn't hold
/// them.
struct RecoverySettings {
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
fn recovered_config(
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
fn recovered_jobs(session: &RenderSessionData) -> Vec<RenderJobRuntime> {
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
                history: history_for(&rj.take_folder),
                current_attempt: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempt(stream: &str, outcome: RenderOutcome, path: &str) -> RenderAttempt {
        RenderAttempt {
            started_unix_ms: 1,
            stream: stream.to_string(),
            codec: "prores".to_string(),
            custom_codec_args: String::new(),
            fps: 300,
            outcome,
            output_path: path.to_string(),
            output_size_bytes: Some(7),
            error: None,
        }
    }

    fn entry(stream: &str, outcome: RenderOutcome, path: &str, exists: bool) -> HistoryEntry {
        HistoryEntry {
            attempt: attempt(stream, outcome, path),
            output_exists: exists,
        }
    }

    #[test]
    fn a_stream_rendered_before_is_staged_from_its_newest_surviving_output() {
        let history = vec![
            entry("all", RenderOutcome::Finished, "E:/out/old.mov", true),
            entry("all", RenderOutcome::Failed, "", false),
            entry("hud:hud", RenderOutcome::Finished, "E:/out/hud.mov", true),
            entry("all", RenderOutcome::Finished, "E:/out/new.mov", true),
        ];
        assert_eq!(
            last_finished_output(&history, "all"),
            Some(("E:/out/new.mov".to_string(), Some(7)))
        );
        assert_eq!(
            last_finished_output(&history, "hud:hud"),
            Some(("E:/out/hud.mov".to_string(), Some(7)))
        );
        assert_eq!(last_finished_output(&history, "other"), None);
    }

    #[test]
    fn a_deleted_or_failed_render_does_not_count() {
        let history = vec![
            entry("all", RenderOutcome::Finished, "E:/out/gone.mov", false),
            entry("all", RenderOutcome::Cancelled, "", false),
        ];
        assert_eq!(last_finished_output(&history, "all"), None);
        assert_eq!(last_finished_output(&[], "all"), None);
    }

    fn clip(take_folder: &str) -> ClipData {
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

    fn autosave_job(name: &str, status: AutosaveJobStatus, with_clip: bool) -> AutosaveJob {
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

    fn session(jobs: Vec<AutosaveJob>) -> RenderSessionData {
        RenderSessionData {
            source_folder: "Z:/no-such-takes".to_string(),
            fps: 300,
            target_codec: "prores".to_string(),
            target_custom_codec_args: String::new(),
            jobs,
        }
    }

    fn settings() -> RecoverySettings {
        RecoverySettings {
            ffmpeg_path: None,
            export_directories: vec!["E:/exports".to_string(), "  ".to_string()],
            max_concurrent_renders: 3,
        }
    }

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

    #[test]
    fn a_recovered_batch_starts_with_a_config_rebuilt_from_autosave_and_settings() {
        let manager = RenderManager::new();
        let views = manager.install_recovered(
            session(vec![
                autosave_job("a", AutosaveJobStatus::Pending, true),
                autosave_job("b", AutosaveJobStatus::Completed, true),
            ]),
            &settings(),
        );
        assert_eq!(views.len(), 2);
        let (config, starting) = manager.start_config().expect("a recovered batch starts");
        assert_eq!(starting, 1);
        assert_eq!(config.source_folder, "Z:/no-such-takes");
        assert_eq!(config.fps, 300);
        assert_eq!(config.target_codec, RenderCodec::from_str_id("prores"));
        assert_eq!(config.export_directories, vec![PathBuf::from("E:/exports")]);
        assert_eq!(config.max_concurrent_renders, 3);
        assert!(!config.ffmpeg_path.is_empty());
        // Each job keeps its own settings, not the batch's.
        assert_eq!(views[0].codec_id, "h264");
        assert!(views[0].settings_summary.ends_with("@ 60fps"));
    }

    #[test]
    fn reset_to_queued_on_a_recovered_finished_row_can_start() {
        let manager = RenderManager::new();
        manager.install_recovered(
            session(vec![autosave_job("b", AutosaveJobStatus::Completed, true)]),
            &settings(),
        );
        assert_eq!(
            manager.start_config().unwrap_err(),
            crate::messages::NOTHING_QUEUED_TO_RENDER
        );
        for job in manager.jobs.lock().unwrap().iter_mut() {
            requeue(job);
        }
        assert!(manager.start_config().is_ok());
    }

    #[test]
    fn a_queued_job_saved_without_its_take_asks_for_a_scan() {
        let manager = RenderManager::new();
        manager.install_recovered(
            session(vec![autosave_job("old", AutosaveJobStatus::Pending, false)]),
            &settings(),
        );
        let err = manager.start_config().unwrap_err();
        assert!(
            err.starts_with("recovered batch: scan again to set "),
            "{}",
            err
        );
        assert!(err.contains("old"), "{}", err);
    }

    #[test]
    fn the_autosave_prompt_stays_away_while_the_process_holds_a_batch() {
        let manager = RenderManager::new();
        assert!(!manager.holds_batch());
        assert!(manager.views().is_empty());
        manager.install_recovered(
            session(vec![autosave_job("a", AutosaveJobStatus::Pending, true)]),
            &settings(),
        );
        assert!(manager.holds_batch());
        assert_eq!(manager.views().len(), 1);
    }
}
