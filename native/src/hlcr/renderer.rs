#![cfg(not(target_arch = "wasm32"))]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use super::config::{RenderCodec, RenderConfig};
use super::scanner::ClipData;

mod ffmpeg_args;
mod ffmpeg_run;

#[derive(Clone, Debug)]
pub enum RenderUpdate {
    Progress(String, u32),                  // (job_id, percentage)
    Speed(String, String),                  // (job_id, speed_text)
    Status(String, String),                 // (job_id, status_text)
    OutputPath(String, String),             // (job_id, absolute path to the encoded file)
    Finished(String, bool, Option<String>), // (job_id, success, error_log)
}

/// Room an export drive must have past a job's reservation before the job is
/// routed to it. Public for the automatic take cleanup (#644), which makes
/// room by the same rule.
pub const EXPORT_SAFETY_MARGIN_BYTES: u64 = 1024 * 1024 * 1024; // 1 GiB

/// Safe, deliberately conservative upper bound on the bytes a render job
/// will need on its export drive — used only to size the JIT reservation in
/// `run_render_job` below, never to predict the real (compressed) output.
/// Raw, uncompressed frame-sequence size (mirrors the AOT capture
/// allocator's own estimate, `patch::builder::block_estimates`): every codec
/// this app renders to compresses well below this in practice, so it's
/// loose but always safe. Falls back to a fixed conservative figure when a
/// clip's resolution is unknown (`ClipData::width`/`height` are `0` — e.g.
/// an autosave-recovered stub, or a take whose container isn't cheap to
/// read dimensions from) rather than reserving nothing for it.
fn estimate_reservation_bytes(clip: &ClipData) -> u64 {
    const UNKNOWN_RESOLUTION_FALLBACK_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2 GiB
    if clip.width == 0 || clip.height == 0 {
        return UNKNOWN_RESOLUTION_FALLBACK_BYTES;
    }
    clip.width as u64 * clip.height as u64 * 3 * clip.frame_count as u64
}

/// Best-effort required-bytes estimate for one job, whatever its status —
/// exact for a SourceCopy job (the source file already exists on disk, so
/// this just measures it). For every other codec: what finished renders of
/// the same codec and frame size actually wrote per frame, plus a margin,
/// once there is one to learn from (#120); until then the same conservative
/// raw-frame estimate as `estimate_reservation_bytes` above, which also caps
/// the learned figure. Shared by `run_render_job`'s own JIT drive routing
/// (only ever called there for a job about to start) and the Render footer's
/// whole-queue "Required (Estimated)" figure (called for every
/// Queued/Rendering job, not just the one about to run).
pub fn job_reservation_estimate(clip: &ClipData, codec: RenderCodec, custom_args: &str) -> u64 {
    if codec == RenderCodec::SourceCopy {
        let take_folder = PathBuf::from(&clip.take_folder);
        let video_name = clip.video_file.as_deref().unwrap_or_default();
        return std::fs::metadata(take_folder.join(&clip.img_folder).join(video_name))
            .map(|m| m.len())
            .unwrap_or_else(|_| estimate_reservation_bytes(clip));
    }
    let raw = estimate_reservation_bytes(clip);
    rate_key(clip, codec, custom_args)
        .and_then(|key| {
            let rates = learned_rates().lock().ok()?;
            rates
                .get(&key)
                .copied()
                .or_else(|| prores_rate_from_another_profile(&rates, &key, codec))
        })
        .map(|bytes_per_frame| {
            let learned = bytes_per_frame * LEARNED_RATE_MARGIN * clip.frame_count as f64;
            (learned.ceil() as u64).min(raw)
        })
        .unwrap_or(raw)
}

// ── Learned output sizes (#120) ──────────────────────────────────────────────
//
// No per-codec table of guessed
// ratios. Each codec's real bytes per frame are measured from renders that
// finished, per frame size (a 4K ProRes frame is not a 720p one), and later
// jobs reserve that plus a margin. The first clip of a kind keeps the
// uncompressed estimate, except a ProRes profile, which borrows another
// profile's measured rate through their size ratio (the one place a ratio is
// written down). Kept for the whole app session: a codec's rate at a
// frame size doesn't change between batches.

/// Head-room over the largest rate seen, since footage varies from clip to
/// clip (smoke and fast motion compress worse).
const LEARNED_RATE_MARGIN: f64 = 1.5;

/// What a rate is learned per: codec (with its custom args, which can be
/// anything), the HUD stream (always ProRes 4444 with alpha), frame size.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RateKey {
    codec: String,
    hud: bool,
    width: u32,
    height: u32,
}

fn rate_key(clip: &ClipData, codec: RenderCodec, custom_args: &str) -> Option<RateKey> {
    if clip.width == 0 || clip.height == 0 || clip.frame_count == 0 {
        return None;
    }
    let hud = clip.clip_type == "hud_only";
    Some(RateKey {
        codec: if hud {
            String::new()
        } else if codec == RenderCodec::Custom {
            format!("custom:{}", custom_args.trim())
        } else {
            codec.to_str_id().to_string()
        },
        hud,
        width: clip.width,
        height: clip.height,
    })
}

/// The ProRes profiles differ mostly in their target rate, so before the
/// first render of one profile finishes, another profile's rate at the same
/// frame size, scaled by the two profiles' size ratio
/// (`RenderCodec::prores_profile`), is a far better guess than the raw
/// estimate. The largest such figure wins, like `record_output_size`.
fn prores_rate_from_another_profile(
    rates: &HashMap<RateKey, f64>,
    key: &RateKey,
    codec: RenderCodec,
) -> Option<f64> {
    if key.hud {
        return None;
    }
    let (_, _, ratio) = codec.prores_profile()?;
    [
        RenderCodec::ProRes,
        RenderCodec::ProRes422,
        RenderCodec::ProResLt,
        RenderCodec::ProRes4444,
    ]
    .into_iter()
    .filter_map(|other| {
        let (_, _, other_ratio) = other.prores_profile()?;
        let other_key = RateKey {
            codec: other.to_str_id().to_string(),
            ..key.clone()
        };
        Some(rates.get(&other_key)? / other_ratio * ratio)
    })
    .reduce(f64::max)
}

/// Largest bytes per frame seen for each key this session.
fn learned_rates() -> &'static Mutex<HashMap<RateKey, f64>> {
    static RATES: std::sync::OnceLock<Mutex<HashMap<RateKey, f64>>> = std::sync::OnceLock::new();
    RATES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Records what a finished render of `clip` actually wrote, for later jobs'
/// reservations. Keeps the largest rate per key, so one small clip can't
/// talk the estimate down for a bigger one.
fn record_output_size(clip: &ClipData, codec: RenderCodec, custom_args: &str, bytes: u64) {
    let Some(key) = rate_key(clip, codec, custom_args) else {
        return;
    };
    let rate = bytes as f64 / clip.frame_count as f64;
    if let Ok(mut rates) = learned_rates().lock() {
        let entry = rates.entry(key).or_insert(rate);
        *entry = entry.max(rate);
    }
}

/// The reservation ledger's key for an export directory: its volume, so
/// folders on one drive share one reservation total (#774); the directory
/// itself when it has no drive or share prefix.
fn ledger_key(dir: &Path) -> PathBuf {
    crate::sys::disk::volume_key(dir)
        .map(PathBuf::from)
        .unwrap_or_else(|| dir.to_path_buf())
}

/// Reserves `amount` on the first of `dirs` whose volume has more than
/// `amount + margin` free once the ledger's reservations there are taken off.
/// Returns that directory, what was already reserved on its volume, and its
/// live free space (`None` when unreadable, which counts as having room).
fn try_reserve(
    ledger: &mut HashMap<PathBuf, u64>,
    dirs: &[PathBuf],
    amount: u64,
    margin: u64,
    mut free_of: impl FnMut(&Path) -> Option<u64>,
) -> Option<(PathBuf, u64, Option<u64>)> {
    for dir in dirs {
        let key = ledger_key(dir);
        let live_free = free_of(dir);
        let already_reserved = *ledger.get(&key).unwrap_or(&0);
        if live_free.is_none_or(|free| free.saturating_sub(already_reserved) > amount + margin) {
            *ledger.entry(key).or_insert(0) += amount;
            return Some((dir.clone(), already_reserved, live_free));
        }
    }
    None
}

/// RAII release for a JIT export-drive reservation claimed by
/// `reserve_export_dir` below. `run_render_job` has many early
/// returns after a drive is selected (success, error, cancellation) —
/// releasing on `Drop` means every one of them releases correctly, including
/// any added later, without a release call having to be hand-placed at each.
struct ReservationGuard {
    ledger: Arc<Mutex<HashMap<PathBuf, u64>>>,
    dir: PathBuf,
    amount: u64,
    // For the matching [render-reservation] claim log below — a future
    // "render says it can't find space but the drive looks fine" report is
    // the difference between guessing and reading exactly what the ledger
    // saw, at the cost of one log line per job.
    job_id: String,
}

impl Drop for ReservationGuard {
    fn drop(&mut self) {
        let mut ledger = self.ledger.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(reserved) = ledger.get_mut(&self.dir) {
            *reserved = reserved.saturating_sub(self.amount);
        }
        let remaining = ledger.get(&self.dir).copied().unwrap_or(0);
        crate::log_markdown(&format!(
            "[render-reservation] job {} released {:.2} GB on {:?} ({:.2} GB still reserved by other jobs)",
            self.job_id,
            self.amount as f64 / (1024.0 * 1024.0 * 1024.0),
            self.dir,
            remaining as f64 / (1024.0 * 1024.0 * 1024.0),
        ));
    }
}

/// Why a render with no export folder is refused. Studio's Render button
/// says the same before it gets this far.
pub const NO_EXPORT_FOLDER: &str =
    "Not rendered: there is no export folder. Add one in Configuration → Render Settings.";

/// Renders one take into the export pool and reports it on `tx`, ending
/// with exactly one `Finished`: checks the take, reserves room on an export
/// drive, then copies the source video (Skip) or runs FFmpeg.
pub async fn run_render_job(
    job_id: String,
    clip: ClipData,
    config: RenderConfig,
    tx: mpsc::Sender<RenderUpdate>,
    cancel_rx: Arc<AtomicBool>,
    reservations: Arc<Mutex<HashMap<PathBuf, u64>>>,
) {
    let job = RenderJob::new(job_id, clip, config, tx, cancel_rx);
    // The take's own capture rate when its `dodstudio_take.json` says, else
    // Render Settings' Source FPS (`take_meta::render_fps`).
    let (fps_used, fps_from_take) = super::take_meta::render_fps(&job.take_folder, job.config.fps);

    if let Err(reason) = check_inputs(&job) {
        job.refuse(reason);
        return;
    }
    log_render_fps(&job, fps_used, fps_from_take);

    // Releases this job's reservation on every exit path below — see
    // `ReservationGuard`'s own doc comment.
    let (output_folder, _reservation_guard) = match reserve_export_dir(&job, &reservations) {
        Ok(reserved) => reserved,
        Err(reason) => {
            job.refuse(reason);
            return;
        }
    };
    if let Err(e) = std::fs::create_dir_all(&output_folder) {
        job.refuse(format!("Failed to create output folder: {}", e));
        return;
    }
    let naming = OutputNaming::new(&job);

    if job.is_source_copy {
        copy_source_video(&job, &output_folder, &naming).await;
        return;
    }

    let encode = match ffmpeg_args::encode_settings(job.is_hud, &job.config) {
        Ok(encode) => encode,
        Err(reason) => {
            job.refuse(reason);
            return;
        }
    };
    let out_file = output_folder.join(naming.file_name(&job, &output_folder, encode.file_ext));
    let threads = ffmpeg_args::threads_per_process(
        std::thread::available_parallelism().ok().map(|n| n.get()),
        job.config.max_concurrent_renders,
    );
    let args = ffmpeg_args::FfmpegArgs {
        clip: &job.clip,
        fps: fps_used,
        encode: &encode,
        threads,
        out_file: &out_file,
    }
    .to_args();
    ffmpeg_run::run_ffmpeg(&job, args, &out_file).await;
}

/// One render job as every stage sees it: the job, where its updates go,
/// its cancel flag, and what is worked out once from the take.
struct RenderJob {
    job_id: String,
    clip: ClipData,
    config: RenderConfig,
    tx: mpsc::Sender<RenderUpdate>,
    cancel: Arc<AtomicBool>,
    take_folder: PathBuf,
    is_hud: bool,
    /// "Skip" — leave an OBS take as OBS wrote it. No FFmpeg involved at
    /// all, just a copy into the export pool with the pipeline's naming applied.
    is_source_copy: bool,
}

impl RenderJob {
    fn new(
        job_id: String,
        clip: ClipData,
        config: RenderConfig,
        tx: mpsc::Sender<RenderUpdate>,
        cancel: Arc<AtomicBool>,
    ) -> Self {
        let take_folder = PathBuf::from(&clip.take_folder);
        let is_hud = clip.clip_type == "hud_only";
        let is_source_copy = config.target_codec == RenderCodec::SourceCopy;
        Self {
            job_id,
            clip,
            config,
            tx,
            cancel,
            take_folder,
            is_hud,
            is_source_copy,
        }
    }

    /// The listener may be gone (the window closed); nobody is left to tell.
    fn send(&self, update: RenderUpdate) {
        let _ = self.tx.send(update);
    }

    /// Ends a job that never started its work: one `Finished` with the reason.
    fn refuse(&self, reason: String) {
        self.send(RenderUpdate::Finished(
            self.job_id.clone(),
            false,
            Some(reason),
        ));
    }

    fn finish_cancelled(&self) {
        self.send(RenderUpdate::Status(
            self.job_id.clone(),
            "Cancelled".to_string(),
        ));
        self.send(RenderUpdate::Finished(
            self.job_id.clone(),
            false,
            Some("Cancelled by user".to_string()),
        ));
    }

    /// `Finished` before 100 % and the path: the scheduler's completion
    /// handling reads them in this order.
    fn finish_ok(&self, out_file: String) {
        self.send(RenderUpdate::Status(
            self.job_id.clone(),
            "Finished".to_string(),
        ));
        self.send(RenderUpdate::Progress(self.job_id.clone(), 100));
        self.send(RenderUpdate::OutputPath(self.job_id.clone(), out_file));
        self.send(RenderUpdate::Finished(self.job_id.clone(), true, None));
    }

    fn finish_error(&self, message: String) {
        self.send(RenderUpdate::Status(
            self.job_id.clone(),
            "Error".to_string(),
        ));
        self.send(RenderUpdate::Finished(
            self.job_id.clone(),
            false,
            Some(message),
        ));
    }
}

/// Why the job can't start, checked in this order before anything is
/// reserved or written: FFmpeg, the take folder, its wav, a HUD take's wav,
/// any audio at all, and whether Skip is allowed for the take.
fn check_inputs(job: &RenderJob) -> Result<(), String> {
    let clip = &job.clip;
    let ffmpeg_path = PathBuf::from(&job.config.ffmpeg_path);
    let is_global = job.config.ffmpeg_path == "ffmpeg";
    // `None` is an OBS take: its audio is already muxed into `video_file`,
    // so there is nothing to join against a wav.
    let wav_file: Option<PathBuf> = clip.wav_file.as_ref().map(|w| job.take_folder.join(w));

    // Skip mode never spawns FFmpeg, so a misconfigured FFmpeg path must not
    // block a plain file copy.
    if !job.is_source_copy && !is_global && (!ffmpeg_path.exists() || !ffmpeg_path.is_file()) {
        return Err(format!("FFmpeg not found at: {}", ffmpeg_path.display()));
    }
    if !job.take_folder.exists() || !job.take_folder.is_dir() {
        return Err(format!(
            "Take folder not found: {}",
            job.take_folder.display()
        ));
    }
    if let Some(wav) = &wav_file
        && !wav.exists()
    {
        return Err(format!("Audio file not found: {}", wav.display()));
    }
    // The alpha stream carries no sound of its own, so a HUD/alpha composite
    // always needs a separate wav — an OBS take's muxed-in audio can't cover it.
    if job.is_hud && wav_file.is_none() {
        return Err(
            "HUD/alpha clips need a separate audio track (sound.wav), but this take has none."
                .to_string(),
        );
    }
    if clip.video_file.is_none() && wav_file.is_none() {
        return Err(
            "No audio source for this take: no sound.wav and no audio-bearing video.".to_string(),
        );
    }
    // Gated on the same predicate the Tauri layer uses to decide whether to
    // offer the toggle at all (`clip_is_skip_eligible`), so this can never
    // quietly diverge from what the frontend thinks is skippable — this is
    // the actual enforcement, the frontend gate is only a convenience.
    if job.is_source_copy && !super::scanner::clip_is_skip_eligible(clip) {
        let reason = if job.is_hud {
            "Skip (keep original) isn't available for HUD/alpha clips — pick a codec to render them."
        } else if clip.wav_file.is_some() {
            "Skip (keep original) isn't available for a take with its own separate audio track — pick a codec to render it."
        } else {
            "Skip (keep original) requires a captured video file, but this take has none."
        };
        return Err(reason.to_string());
    }
    Ok(())
}

/// Logs the rate a BMP take's frames are read at, and where it came from.
///
/// The FPS is fed to `-framerate` *before* the BMP input, so it is not an
/// output preference — it is how FFmpeg is told to interpret the frame
/// sequence's timing. A value that disagrees with the capture produces a
/// wrong computed duration and `-shortest` then trims the audio to match, so
/// the render would succeed and the clip simply be the wrong speed. Hence the
/// take's own recorded rate wins over the setting (#14, `hlcr::take_meta`).
///
/// Logged, not sent as a `Status` update: `job.status` is a state field with
/// a fixed vocabulary ("Queued" | "Rendering" | "Finished" | "Error" |
/// "Cancelled") that the scheduler's own transitions test against. The row
/// itself shows the rate through the job's settings summary.
fn log_render_fps(job: &RenderJob, fps_used: u32, fps_from_take: bool) {
    if !job.is_source_copy && job.clip.video_file.is_none() {
        crate::log_markdown(&format!(
            "[render-fps] job {} — reading the frames at {} fps, {}",
            job.job_id,
            fps_used,
            if fps_from_take {
                "the rate the take was captured at".to_string()
            } else {
                format!(
                    "Render Settings' Source FPS (the take doesn't say what it was captured at; {})",
                    job.take_folder.display()
                )
            }
        ));
    }
}

/// JIT export-drive routing: the first export folder with enough room for
/// *this specific job*, its reservation claimed in `reservations` until the
/// returned guard drops; or why there is none.
///
/// "Enough room" means live free space minus whatever other
/// concurrently-running jobs have already provisionally reserved on that
/// same drive (`reservations`), not just the live free-space number
/// alone. Without that ledger, several jobs starting in the same
/// scheduler tick can all see the same live free-space number and all
/// pick the same drive before any of them has written a byte — the flat
/// 20 GiB threshold this replaced was only ever sized to be safe for one
/// job at a time.
fn reserve_export_dir(
    job: &RenderJob,
    reservations: &Arc<Mutex<HashMap<PathBuf, u64>>>,
) -> Result<(PathBuf, ReservationGuard), String> {
    let config = &job.config;
    let reservation_estimate =
        job_reservation_estimate(&job.clip, config.target_codec, &config.custom_codec_args);

    // No export folder: refused, never written into the working directory
    // (for an installed app, its install folder), where nobody would look.
    if config.export_directories.is_empty() {
        return Err(NO_EXPORT_FOLDER.to_string());
    }
    let mut ledger = reservations.lock().unwrap_or_else(|p| p.into_inner());
    let Some((dir, already_reserved, live_free)) = try_reserve(
        &mut ledger,
        &config.export_directories,
        reservation_estimate,
        EXPORT_SAFETY_MARGIN_BYTES,
        crate::sys::disk::get_available_bytes,
    ) else {
        return Err(format!(
            "Not rendered: every export folder ({}) has less than {:.1} GB free once the other renders running now are counted. Free some space or add another export folder in Configuration → Render Settings.",
            config.export_directories.len(),
            (reservation_estimate + EXPORT_SAFETY_MARGIN_BYTES) as f64 / (1024.0 * 1024.0 * 1024.0)
        ));
    };
    // Shows this job's own estimate against what else was already
    // reserved on this drive *before* this job's own addition, so
    // concurrent jobs' numbers are directly comparable in the log —
    // this is what made it possible to confirm live, under a real
    // concurrent batch, that the ledger correctly reflects other
    // in-flight jobs' reservations rather than every job seeing the
    // same live free-space number.
    let live_free_text = live_free.map_or("unknown".to_string(), |free| {
        format!("{:.2} GB", free as f64 / (1024.0 * 1024.0 * 1024.0))
    });
    crate::log_markdown(&format!(
        "[render-reservation] job {} reserved {:.2} GB on {:?} (already had {:.2} GB reserved by other jobs, {} live free)",
        job.job_id,
        reservation_estimate as f64 / (1024.0 * 1024.0 * 1024.0),
        dir,
        already_reserved as f64 / (1024.0 * 1024.0 * 1024.0),
        live_free_text,
    ));
    drop(ledger);
    let guard = ReservationGuard {
        ledger: reservations.clone(),
        dir: ledger_key(&dir),
        amount: reservation_estimate,
        job_id: job.job_id.clone(),
    };
    Ok((dir, guard))
}

/// The pieces of a finished file's name, shared by the FFmpeg render path
/// and the Skip copy, so a skipped take and a rendered one land under the
/// same scheme — "where do I find my finished clips" stays answered the
/// same way regardless of which option a take took.
struct OutputNaming {
    stream_type: &'static str,
    wav_part: String,
    hash_str: String,
    take_name: String,
    demo_name: String,
}

impl OutputNaming {
    fn new(job: &RenderJob) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros();
        let take_folder = &job.take_folder;
        Self {
            stream_type: if job.is_hud { "hud" } else { "all" },
            wav_part: wav_part(job.clip.wav_file.as_deref()),
            hash_str: format!("{:04x}", timestamp % 0x10000),
            take_name: take_folder
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            demo_name: take_folder
                .parent()
                .and_then(|p| p.file_name())
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        }
    }

    /// The file name in `output_folder` for an output with `file_ext` (with
    /// its dot): the clip name made unique there (#441), else
    /// `<demo>_<take><wav part>_<stream>_<hash><ext>`.
    fn file_name(&self, job: &RenderJob, output_folder: &Path, file_ext: &str) -> String {
        match clip_file_stem(&job.clip, &self.wav_part, job.is_hud) {
            Some(stem) => get_unique_filename(output_folder, &stem, file_ext),
            None => format!(
                "{}_{}{}_{}_{}{}",
                self.demo_name,
                self.take_name,
                self.wav_part,
                self.stream_type,
                self.hash_str,
                file_ext
            ),
        }
    }
}

/// The wav's part of a file name: `_<stem>`, nothing for the usual
/// `sound.wav`. An OBS take has no wav to derive a suffix from — base_name
/// already carries the distinguishing "-obs" suffix the scanner gave it.
fn wav_part(wav_file: Option<&str>) -> String {
    match wav_file {
        Some(wav) => {
            let wav_stem = std::path::Path::new(wav)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            if wav_stem.to_lowercase() == "sound" {
                String::new()
            } else {
                format!("_{}", wav_stem)
            }
        }
        None => String::new(),
    }
}

/// Skip: copies the take's video into `output_folder` as OBS wrote it and
/// sends the job's updates to its `Finished`.
async fn copy_source_video(job: &RenderJob, output_folder: &Path, naming: &OutputNaming) {
    let clip = &job.clip;
    // `check_inputs` ensured video_file and a non-hud clip_type here.
    let video_name = clip.video_file.as_deref().unwrap_or_default();
    let source_video = job.take_folder.join(&clip.img_folder).join(video_name);
    if !source_video.is_file() {
        job.refuse(format!(
            "Source video not found: {}",
            source_video.display()
        ));
        return;
    }
    // Kept from whatever OBS wrote — renaming would make the file lie
    // about its own container to every tool that reads it afterwards.
    let ext = source_video
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_else(|| "mp4".to_string());
    let final_name = naming.file_name(job, output_folder, &format!(".{ext}"));
    let out_file = output_folder.join(&final_name);
    let out_file_str = out_file.to_string_lossy().into_owned();

    if job.cancel.load(Ordering::Relaxed) {
        job.finish_cancelled();
        return;
    }
    job.send(RenderUpdate::Status(
        job.job_id.clone(),
        "Rendering".to_string(),
    ));
    // Chunked rather than `tokio::fs::copy`, so Cancel actually lands
    // during a large copy (Custom Output/lossless OBS captures — see
    // docs/archive/obs_alternate_capture_design.md — can run tens of GB) instead of
    // being silently ignored until the whole file has already moved.
    match copy_cancellable(&source_video, &out_file, &job.cancel).await {
        Ok(true) => job.finish_cancelled(),
        Ok(false) => job.finish_ok(out_file_str),
        Err(e) => job.finish_error(format!("Failed to copy source video to export pool: {}", e)),
    }
}

/// Opaque wake-lock guard for a render batch's duration. Wrapped so
/// `studio/src-tauri` never needs `keepawake` as a direct
/// dependency — `native` already depends on it (see
/// `capture_engine.rs::CaptureCleanupGuard`'s own wake lock, the
/// capture-side equivalent of this).
pub struct RenderWakeLock(#[allow(dead_code)] keepawake::KeepAwake);

pub fn hold_render_wake_lock() -> Option<RenderWakeLock> {
    keepawake::Builder::default()
        .display(false)
        .idle(true)
        .sleep(true)
        .create()
        .ok()
        .map(RenderWakeLock)
}

/// Copies `src` to `dst` in chunks, checking `cancel` between reads.
///
/// The "Skip" render path's only work is this copy, and a plain
/// `tokio::fs::copy` has no hook to interrupt partway through — Cancel would
/// silently do nothing until the whole file had already moved, unlike the
/// FFmpeg path, which polls its cancel flag every 200ms and kills the child.
///
/// Returns `Ok(true)` if cancelled partway through, in which case the partial
/// `dst` is removed before returning — a truncated file left behind under
/// the pipeline's naming would look like a finished export.
async fn copy_cancellable(src: &Path, dst: &Path, cancel: &AtomicBool) -> std::io::Result<bool> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut reader = tokio::fs::File::open(src).await?;
    let mut writer = tokio::fs::File::create(dst).await?;
    // 2 MiB: large enough that per-chunk overhead is negligible against the
    // copy itself, and a cancel lands within a second or two even on a slow
    // disk.
    let mut buf = vec![0u8; 2 * 1024 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(writer);
            let _ = tokio::fs::remove_file(dst).await;
            return Ok(true);
        }
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n]).await?;
    }
    writer.flush().await?;
    Ok(false)
}

/// Removes what FFmpeg wrote of `out_file` before it was stopped or failed:
/// a truncated file under the pipeline's naming looks like a finished export,
/// the same reason `copy_cancellable` removes its partial copy. A file that was
/// there before the job started is never touched.
async fn remove_partial_output(out_file: &Path, existed_before: bool) {
    if !existed_before {
        let _ = tokio::fs::remove_file(out_file).await;
    }
}

/// The finished file's name, without its extension, when the take has a clip
/// name (#441): the name, the wav suffix, and `_hud` for the HUD stream. The
/// caller makes it unique, so a re-render never overwrites an earlier file.
/// `None` without a usable clip name.
fn clip_file_stem(clip: &ClipData, wav_part: &str, is_hud: bool) -> Option<String> {
    let name = sanitize_file_stem(clip.clip_name.as_deref()?);
    if name.is_empty() {
        return None;
    }
    Some(format!(
        "{}{}{}",
        name,
        wav_part,
        if is_hud { "_hud" } else { "" }
    ))
}

/// Replaces what Windows refuses in a file name, and trims the trailing dots
/// and spaces it drops silently. The frontend cleans names already; this is
/// the last line, since a name can come from a hand-edited project file.
fn sanitize_file_stem(name: &str) -> String {
    let replaced: String = name
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    replaced.trim().trim_end_matches(['.', ' ']).to_string()
}

fn get_unique_filename(output_dir: &Path, base_name: &str, ext: &str) -> String {
    let mut counter = 1;
    let mut final_name = format!("{}{}", base_name, ext);
    while output_dir.join(&final_name).exists() {
        final_name = format!("{}_{}{}", base_name, counter, ext);
        counter += 1;
    }
    final_name
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hlcr::config::RenderCodec;
    use crate::test_support::Scratch;

    fn scratch(name: &str) -> Scratch {
        Scratch::new(format_args!("renderer_test_{name}"))
    }

    fn sized_clip(width: u32, height: u32, frame_count: usize, clip_type: &str) -> ClipData {
        ClipData {
            take_folder: String::new(),
            clip_type: clip_type.to_string(),
            img_folder: "all".to_string(),
            wav_file: None,
            base_name: "t".to_string(),
            frame_count,
            width,
            height,
            date: String::new(),
            video_file: None,
            alpha_folder: None,
            clip_name: None,
        }
    }

    /// #774: a reservation on C:\a comes off what C:\b has, so with room for
    /// one job on C: the second goes to D:, and to nowhere once D: is full.
    #[cfg(windows)]
    #[test]
    fn reservations_on_one_folder_reduce_its_drives_other_folders() {
        let mut ledger = HashMap::new();
        let free = |p: &Path| Some(if p.starts_with(r"C:\") { 15 } else { 12 });
        let c_only = [PathBuf::from(r"C:\a"), PathBuf::from(r"C:\b")];
        let with_d = [
            PathBuf::from(r"C:\a"),
            PathBuf::from(r"C:\b"),
            PathBuf::from(r"D:\x"),
        ];

        let first = try_reserve(&mut ledger, &c_only, 10, 0, free);
        assert_eq!(first.map(|r| r.0), Some(PathBuf::from(r"C:\a")));
        assert_eq!(try_reserve(&mut ledger, &c_only, 10, 0, free), None);
        let second = try_reserve(&mut ledger, &with_d, 10, 0, free);
        assert_eq!(second.map(|r| r.0), Some(PathBuf::from(r"D:\x")));
        assert_eq!(try_reserve(&mut ledger, &with_d, 10, 0, free), None);
        assert_eq!(ledger.get(&ledger_key(Path::new(r"C:\a"))), Some(&10));
        assert_eq!(ledger.get(&ledger_key(Path::new(r"D:\x"))), Some(&10));
    }

    /// A folder whose free space can't be read is reserved on, as it was when
    /// "unknown" was the largest number; a known one still has to have room.
    #[test]
    fn an_unreadable_export_folder_still_takes_a_reservation() {
        let mut ledger = HashMap::new();
        let dirs = [PathBuf::from("no-drive-a")];
        let hit = try_reserve(&mut ledger, &dirs, 10, 5, |_| None);
        assert_eq!(hit, Some((PathBuf::from("no-drive-a"), 0, None)));
        assert_eq!(ledger.get(&ledger_key(&dirs[0])), Some(&10));
        // Reservations never make an unreadable folder refuse.
        assert!(try_reserve(&mut ledger, &dirs, 10, 5, |_| None).is_some());
        let mut ledger = HashMap::new();
        assert_eq!(try_reserve(&mut ledger, &dirs, 10, 5, |_| Some(15)), None);
        assert!(try_reserve(&mut ledger, &dirs, 10, 5, |_| Some(16)).is_some());
    }

    /// #120: the first clip of a kind reserves the raw estimate; once one
    /// has finished, later ones reserve its real rate plus the margin,
    /// never more than raw, and only for the same codec and frame size.
    #[test]
    fn reservations_learn_from_finished_renders() {
        // Frame sizes no other test uses, since the rates are session-wide.
        let first = sized_clip(1234, 567, 100, "single");
        let raw = 1234 * 567 * 3 * 100;
        assert_eq!(
            job_reservation_estimate(&first, RenderCodec::H264Software, ""),
            raw
        );

        record_output_size(&first, RenderCodec::H264Software, "", 1_000_000);
        let later = sized_clip(1234, 567, 200, "single");
        assert_eq!(
            job_reservation_estimate(&later, RenderCodec::H264Software, ""),
            (1_000_000.0 / 100.0 * LEARNED_RATE_MARGIN * 200.0) as u64
        );
        // A smaller clip never lowers the rate that's kept.
        record_output_size(&first, RenderCodec::H264Software, "", 10);
        assert_eq!(
            job_reservation_estimate(&later, RenderCodec::H264Software, ""),
            (1_000_000.0 / 100.0 * LEARNED_RATE_MARGIN * 200.0) as u64
        );
        // Another codec, frame size, custom args or the HUD stream: raw.
        assert_eq!(
            job_reservation_estimate(&later, RenderCodec::ProRes, ""),
            raw * 2
        );
        let hud = sized_clip(1234, 567, 200, "hud_only");
        assert_eq!(
            job_reservation_estimate(&hud, RenderCodec::H264Software, ""),
            raw * 2
        );
        record_output_size(&first, RenderCodec::Custom, "-c:v mpeg4", 1_000);
        assert_eq!(
            job_reservation_estimate(&later, RenderCodec::Custom, "-c:v ffv1"),
            raw * 2
        );
        // Never above raw, however much a render wrote.
        let big = sized_clip(1234, 568, 10, "single");
        record_output_size(&big, RenderCodec::DnxHr, "", u64::MAX / 4);
        assert_eq!(
            job_reservation_estimate(&big, RenderCodec::DnxHr, ""),
            1234 * 568 * 3 * 10
        );
    }

    /// Each ProRes profile learns its own rate; until it has one, it borrows
    /// another profile's through their size ratio. Other codecs never do.
    #[test]
    fn a_prores_profile_borrows_another_profiles_rate_until_it_has_its_own() {
        // A frame size no other test uses, since the rates are session-wide.
        let clip = sized_clip(1236, 571, 100, "single");
        let raw = 1236 * 571 * 3 * 100;
        assert_eq!(
            job_reservation_estimate(&clip, RenderCodec::ProResLt, ""),
            raw
        );
        // HQ wrote 1000 bytes a frame.
        record_output_size(&clip, RenderCodec::ProRes, "", 100_000);
        let expect = |ratio: f64| (1000.0 * ratio * LEARNED_RATE_MARGIN * 100.0).ceil() as u64;
        assert_eq!(
            job_reservation_estimate(&clip, RenderCodec::ProRes, ""),
            expect(1.0)
        );
        assert_eq!(
            job_reservation_estimate(&clip, RenderCodec::ProRes422, ""),
            expect(0.67)
        );
        assert_eq!(
            job_reservation_estimate(&clip, RenderCodec::ProResLt, ""),
            expect(0.47)
        );
        assert_eq!(
            job_reservation_estimate(&clip, RenderCodec::ProRes4444, ""),
            expect(1.5)
        );
        // LT's own measured rate wins over the borrowed one.
        record_output_size(&clip, RenderCodec::ProResLt, "", 30_000);
        assert_eq!(
            job_reservation_estimate(&clip, RenderCodec::ProResLt, ""),
            (300.0 * LEARNED_RATE_MARGIN * 100.0).ceil() as u64
        );
        // Not for another codec, nor the HUD stream.
        assert_eq!(job_reservation_estimate(&clip, RenderCodec::DnxHr, ""), raw);
        let hud = sized_clip(1236, 571, 100, "hud_only");
        assert_eq!(
            job_reservation_estimate(&hud, RenderCodec::ProRes422, ""),
            raw
        );
    }

    /// Renders a generated 2560x1440 Ut Video take with every ProRes choice
    /// through `run_render_job` and reads back each file's profile with
    /// ffprobe. Needs FFmpeg: `DOD_FFMPEG` is the path to ffmpeg.exe, with
    /// ffprobe.exe beside it. Prints each file's size next to HQ's.
    #[tokio::test]
    #[ignore = "needs FFmpeg: set DOD_FFMPEG to ffmpeg.exe's path"]
    async fn prores_choices_render_with_their_profile() {
        let ffmpeg = PathBuf::from(
            std::env::var("DOD_FFMPEG").expect("DOD_FFMPEG must name ffmpeg.exe for this test"),
        );
        assert!(ffmpeg.is_file(), "no FFmpeg at {}", ffmpeg.display());
        let ffprobe = ffmpeg.with_file_name(if cfg!(windows) {
            "ffprobe.exe"
        } else {
            "ffprobe"
        });
        let root = scratch("prores_profiles");
        let take_folder = root.join("demo").join("take0000");
        let stream = take_folder.join("all");
        std::fs::create_dir_all(&stream).unwrap();
        let export_dir = root.join("export");
        std::fs::create_dir_all(&export_dir).unwrap();
        let frames = 150usize;
        let made = std::process::Command::new(&ffmpeg)
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
            ])
            .arg(format!(
                "testsrc2=size=2560x1440:rate=300:duration={}",
                frames as f64 / 300.0
            ))
            .args(["-pix_fmt", "rgb24", "-c:v", "utvideo"])
            .arg(stream.join("video.avi"))
            .status()
            .unwrap();
        assert!(made.success());
        let made = std::process::Command::new(&ffmpeg)
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
            ])
            .arg(format!(
                "sine=frequency=440:duration={}",
                frames as f64 / 300.0
            ))
            .arg(take_folder.join("sound.wav"))
            .status()
            .unwrap();
        assert!(made.success());

        let mut sizes = Vec::new();
        for (codec, profile) in [
            (RenderCodec::ProRes, "HQ"),
            (RenderCodec::ProRes422, "Standard"),
            (RenderCodec::ProResLt, "LT"),
            (RenderCodec::ProRes4444, "4444"),
        ] {
            let clip = ClipData {
                clip_name: Some(codec.to_str_id().to_string()),
                take_folder: take_folder.to_string_lossy().into_owned(),
                clip_type: "single".to_string(),
                img_folder: "all".to_string(),
                wav_file: Some("sound.wav".to_string()),
                base_name: "t".to_string(),
                frame_count: frames,
                width: 2560,
                height: 1440,
                date: "-".to_string(),
                video_file: Some("video.avi".to_string()),
                alpha_folder: None,
            };
            let config = RenderConfig {
                ffmpeg_path: ffmpeg.to_string_lossy().into_owned(),
                target_codec: codec,
                ..source_copy_config(&export_dir)
            };
            let (tx, rx) = mpsc::channel();
            run_render_job(
                "0".to_string(),
                clip,
                config,
                tx,
                Arc::new(AtomicBool::new(false)),
                Arc::new(Mutex::new(HashMap::new())),
            )
            .await;
            let updates = drain(&rx);
            assert_eq!(last_finished(&updates), Some((true, None)), "{:?}", updates);
            let out = updates
                .iter()
                .find_map(|u| match u {
                    RenderUpdate::OutputPath(_, p) => Some(p.clone()),
                    _ => None,
                })
                .unwrap();
            let probe = std::process::Command::new(&ffprobe)
                .args([
                    "-v",
                    "error",
                    "-select_streams",
                    "v:0",
                    "-show_entries",
                    "stream=codec_name,profile,pix_fmt",
                    "-of",
                    "default=noprint_wrappers=1",
                ])
                .arg(&out)
                .output()
                .unwrap();
            let report = String::from_utf8_lossy(&probe.stdout);
            assert!(
                report.lines().any(|l| l == format!("profile={profile}")),
                "{codec:?}: {report}"
            );
            assert!(report.contains("codec_name=prores"), "{codec:?}: {report}");
            sizes.push((codec, std::fs::metadata(&out).unwrap().len()));
        }
        let hq = sizes[0].1 as f64;
        for (codec, size) in &sizes {
            println!(
                "{:?}: {} bytes, {:.2}x HQ (default ratio {})",
                codec,
                size,
                *size as f64 / hq,
                codec.prores_profile().unwrap().2
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    fn source_copy_config(export_dir: &Path) -> RenderConfig {
        RenderConfig {
            ffmpeg_path: "ffmpeg".to_string(),
            source_folder: String::new(),
            export_directories: vec![export_dir.to_path_buf()],
            fps: 300,
            target_codec: RenderCodec::SourceCopy,
            custom_codec_args: String::new(),
            max_concurrent_renders: 1,
        }
    }

    fn drain(rx: &mpsc::Receiver<RenderUpdate>) -> Vec<RenderUpdate> {
        let mut out = Vec::new();
        while let Ok(u) = rx.try_recv() {
            out.push(u);
        }
        out
    }

    fn last_finished(updates: &[RenderUpdate]) -> Option<(bool, Option<String>)> {
        updates.iter().rev().find_map(|u| match u {
            RenderUpdate::Finished(_, success, err) => Some((*success, err.clone())),
            _ => None,
        })
    }

    /// The core of issue #82's "skip" path: an OBS take with no separate wav
    /// (audio already muxed into the video) is routed into the export pool
    /// as a plain copy — no FFmpeg spawn, original file left in place.
    #[tokio::test]
    async fn skip_copies_the_source_video_into_the_export_pool() {
        let root = scratch("skip_ok");
        let take_folder = root.join("take");
        let stream = take_folder.join("all");
        std::fs::create_dir_all(&stream).unwrap();
        std::fs::write(stream.join("video.mp4"), b"fake obs video bytes").unwrap();
        let export_dir = root.join("export");
        std::fs::create_dir_all(&export_dir).unwrap();

        let clip = ClipData {
            clip_name: None,
            take_folder: take_folder.to_string_lossy().into_owned(),
            clip_type: "single".to_string(),
            img_folder: "all".to_string(),
            wav_file: None,
            base_name: "demo-take-obs".to_string(),
            frame_count: 0,
            width: 0,
            height: 0,
            date: "-".to_string(),
            video_file: Some("video.mp4".to_string()),
            alpha_folder: None,
        };

        let (tx, rx) = mpsc::channel();
        run_render_job(
            "0".to_string(),
            clip,
            source_copy_config(&export_dir),
            tx,
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(HashMap::new())),
        )
        .await;

        let updates = drain(&rx);
        assert_eq!(last_finished(&updates), Some((true, None)), "{:?}", updates);
        let output_path = updates
            .iter()
            .find_map(|u| match u {
                RenderUpdate::OutputPath(_, p) => Some(p.clone()),
                _ => None,
            })
            .expect("OutputPath was not sent");
        assert!(output_path.ends_with(".mp4"), "{}", output_path);
        assert_eq!(
            std::fs::read(&output_path).unwrap(),
            b"fake obs video bytes"
        );
        // A copy, not a move — the original take is untouched.
        assert!(stream.join("video.mp4").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// No export folder: refused, with nothing written anywhere (it used to
    /// land in the working directory).
    #[tokio::test]
    async fn a_render_with_no_export_folder_is_refused() {
        let root = scratch("no_export");
        let take_folder = root.join("take");
        let stream = take_folder.join("all");
        std::fs::create_dir_all(&stream).unwrap();
        std::fs::write(stream.join("video.mp4"), b"fake obs video bytes").unwrap();
        let clip = ClipData {
            clip_name: None,
            take_folder: take_folder.to_string_lossy().into_owned(),
            clip_type: "single".to_string(),
            img_folder: "all".to_string(),
            wav_file: None,
            base_name: "no-export-take".to_string(),
            frame_count: 0,
            width: 0,
            height: 0,
            date: "-".to_string(),
            video_file: Some("video.mp4".to_string()),
            alpha_folder: None,
        };
        let mut config = source_copy_config(&root);
        config.export_directories.clear();

        let (tx, rx) = mpsc::channel();
        run_render_job(
            "0".to_string(),
            clip,
            config,
            tx,
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(HashMap::new())),
        )
        .await;

        let updates = drain(&rx);
        assert_eq!(
            last_finished(&updates),
            Some((false, Some(NO_EXPORT_FOLDER.to_string()))),
            "{:?}",
            updates
        );
        assert!(
            !updates
                .iter()
                .any(|u| matches!(u, RenderUpdate::OutputPath(..)))
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A clip name (#441) names the finished file, and a second render of the
    /// same clip gets a suffix instead of overwriting the first.
    #[tokio::test]
    async fn a_clip_name_names_the_file_and_never_overwrites() {
        let root = scratch("clip_name");
        let take_folder = root.join("take");
        let stream = take_folder.join("all");
        std::fs::create_dir_all(&stream).unwrap();
        std::fs::write(stream.join("video.mp4"), b"v").unwrap();
        let export_dir = root.join("export");
        std::fs::create_dir_all(&export_dir).unwrap();

        let clip = ClipData {
            clip_name: Some("anzio_krod_4k".to_string()),
            take_folder: take_folder.to_string_lossy().into_owned(),
            clip_type: "single".to_string(),
            img_folder: "all".to_string(),
            wav_file: None,
            base_name: "demo-take-obs".to_string(),
            frame_count: 0,
            width: 0,
            height: 0,
            date: "-".to_string(),
            video_file: Some("video.mp4".to_string()),
            alpha_folder: None,
        };

        for _ in 0..2 {
            let (tx, rx) = mpsc::channel();
            run_render_job(
                "0".to_string(),
                clip.clone(),
                source_copy_config(&export_dir),
                tx,
                Arc::new(AtomicBool::new(false)),
                Arc::new(Mutex::new(HashMap::new())),
            )
            .await;
            assert_eq!(last_finished(&drain(&rx)), Some((true, None)));
        }
        assert!(export_dir.join("anzio_krod_4k.mp4").is_file());
        assert!(export_dir.join("anzio_krod_4k_1.mp4").is_file());
    }

    #[test]
    fn a_clip_name_is_cleaned_for_windows() {
        assert_eq!(sanitize_file_stem("anzio: krod?  "), "anzio_ krod_");
        assert_eq!(sanitize_file_stem("a/b\\c|d."), "a_b_c_d");
        assert_eq!(sanitize_file_stem(" . "), "");
    }

    /// HUD/alpha compositing always needs the FFmpeg alpha-merge pass, so
    /// skip cannot apply to it regardless of how the clip's audio arrived.
    #[tokio::test]
    async fn skip_is_refused_for_a_hud_clip() {
        let root = scratch("skip_hud_refused");
        let take_folder = root.join("take");
        let stream = take_folder.join("hudcolor");
        std::fs::create_dir_all(&stream).unwrap();
        std::fs::write(stream.join("video.mp4"), b"x").unwrap();
        let export_dir = root.join("export");
        std::fs::create_dir_all(&export_dir).unwrap();

        let clip = ClipData {
            clip_name: None,
            take_folder: take_folder.to_string_lossy().into_owned(),
            clip_type: "hud_only".to_string(),
            img_folder: "hudcolor".to_string(),
            wav_file: None,
            base_name: "demo-take-obs".to_string(),
            frame_count: 0,
            width: 0,
            height: 0,
            date: "-".to_string(),
            video_file: Some("video.mp4".to_string()),
            alpha_folder: Some("hudalpha".to_string()),
        };

        let (tx, rx) = mpsc::channel();
        run_render_job(
            "0".to_string(),
            clip,
            source_copy_config(&export_dir),
            tx,
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(HashMap::new())),
        )
        .await;

        let updates = drain(&rx);
        match last_finished(&updates) {
            Some((false, Some(_))) => {}
            other => panic!("expected a rejection, got {:?}", other),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A clip with neither a wav nor a captured video has no audio source at
    /// all — this must fail loudly rather than render a silent clip.
    #[tokio::test]
    async fn a_clip_with_no_audio_source_at_all_is_refused() {
        let root = scratch("no_audio_source");
        let take_folder = root.join("take");
        std::fs::create_dir_all(take_folder.join("all")).unwrap();
        let export_dir = root.join("export");
        std::fs::create_dir_all(&export_dir).unwrap();

        let clip = ClipData {
            clip_name: None,
            take_folder: take_folder.to_string_lossy().into_owned(),
            clip_type: "single".to_string(),
            img_folder: "all".to_string(),
            wav_file: None,
            base_name: "demo-take-obs".to_string(),
            frame_count: 0,
            width: 0,
            height: 0,
            date: "-".to_string(),
            video_file: None,
            alpha_folder: None,
        };

        let (tx, rx) = mpsc::channel();
        run_render_job(
            "0".to_string(),
            clip,
            source_copy_config(&export_dir),
            tx,
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(HashMap::new())),
        )
        .await;

        let updates = drain(&rx);
        match last_finished(&updates) {
            Some((false, Some(_))) => {}
            other => panic!("expected a rejection, got {:?}", other),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Skip needs an actual captured video to copy — an OBS-shaped clip
    /// missing its video file (moved, deleted) must fail rather than copy
    /// nothing and report success.
    #[tokio::test]
    async fn skip_is_refused_when_the_source_video_is_missing() {
        let root = scratch("skip_missing_video");
        let take_folder = root.join("take");
        std::fs::create_dir_all(take_folder.join("all")).unwrap();
        let export_dir = root.join("export");
        std::fs::create_dir_all(&export_dir).unwrap();

        let clip = ClipData {
            clip_name: None,
            take_folder: take_folder.to_string_lossy().into_owned(),
            clip_type: "single".to_string(),
            img_folder: "all".to_string(),
            wav_file: None,
            base_name: "demo-take-obs".to_string(),
            frame_count: 0,
            width: 0,
            height: 0,
            date: "-".to_string(),
            // Claims a video that was never written.
            video_file: Some("video.mp4".to_string()),
            alpha_folder: None,
        };

        let (tx, rx) = mpsc::channel();
        run_render_job(
            "0".to_string(),
            clip,
            source_copy_config(&export_dir),
            tx,
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(HashMap::new())),
        )
        .await;

        let updates = drain(&rx);
        match last_finished(&updates) {
            Some((false, Some(_))) => {}
            other => panic!("expected a rejection, got {:?}", other),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A clip with its own separate wav has something real to mux — skipping
    /// it would silently discard that audio track rather than reflect it,
    /// so `clip_is_skip_eligible` (shared with the Tauri per-job toggle gate)
    /// must refuse it even if a caller forces `SourceCopy` anyway.
    #[tokio::test]
    async fn skip_is_refused_when_the_clip_still_has_a_separate_wav() {
        let root = scratch("skip_has_wav");
        let take_folder = root.join("take");
        let stream = take_folder.join("all");
        std::fs::create_dir_all(&stream).unwrap();
        std::fs::write(stream.join("video.mp4"), b"x").unwrap();
        std::fs::write(take_folder.join("sound.wav"), b"wav").unwrap();
        let export_dir = root.join("export");
        std::fs::create_dir_all(&export_dir).unwrap();

        let clip = ClipData {
            clip_name: None,
            take_folder: take_folder.to_string_lossy().into_owned(),
            clip_type: "single".to_string(),
            img_folder: "all".to_string(),
            wav_file: Some("sound.wav".to_string()),
            base_name: "demo-take".to_string(),
            frame_count: 0,
            width: 0,
            height: 0,
            date: "-".to_string(),
            video_file: Some("video.mp4".to_string()),
            alpha_folder: None,
        };

        let (tx, rx) = mpsc::channel();
        run_render_job(
            "0".to_string(),
            clip,
            source_copy_config(&export_dir),
            tx,
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(HashMap::new())),
        )
        .await;

        let updates = drain(&rx);
        match last_finished(&updates) {
            Some((false, Some(_))) => {}
            other => panic!("expected a rejection, got {:?}", other),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #80's sibling gap on the Skip path itself: a plain `tokio::fs::copy`
    /// has no way to honor a mid-copy cancel. `copy_cancellable` must remove
    /// whatever partial file it started, not leave a truncated one sitting
    /// under the pipeline's naming looking like a finished export.
    #[tokio::test]
    async fn copy_cancellable_removes_the_partial_file_when_cancelled() {
        let root = scratch("copy_cancel");
        let src = root.join("src.bin");
        std::fs::write(&src, vec![0u8; 1024]).unwrap();
        let dst = root.join("dst.bin");
        let cancel = AtomicBool::new(true);

        let cancelled = copy_cancellable(&src, &dst, &cancel).await.unwrap();
        assert!(cancelled);
        assert!(
            !dst.exists(),
            "a cancelled copy must not leave a partial file behind"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn copy_cancellable_completes_normally_without_a_cancel() {
        let root = scratch("copy_ok");
        let src = root.join("src.bin");
        std::fs::write(&src, b"hello world").unwrap();
        let dst = root.join("dst.bin");
        let cancel = AtomicBool::new(false);

        let cancelled = copy_cancellable(&src, &dst, &cancel).await.unwrap();
        assert!(!cancelled);
        assert_eq!(std::fs::read(&dst).unwrap(), b"hello world");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A cancelled or failed FFmpeg render removes the file it was writing,
    /// but never one that was there before the job started.
    #[tokio::test]
    async fn remove_partial_output_only_removes_the_jobs_own_file() {
        let root = scratch("partial_output");
        let written = root.join("clip.mp4");
        std::fs::write(&written, b"48 bytes of a header").unwrap();
        remove_partial_output(&written, false).await;
        assert!(!written.exists(), "the job's partial file must go");

        let earlier = root.join("earlier.mp4");
        std::fs::write(&earlier, b"someone's finished clip").unwrap();
        remove_partial_output(&earlier, true).await;
        assert!(earlier.exists(), "a file from before the job must stay");
        let _ = std::fs::remove_dir_all(&root);
    }

    // ── Whole-job behaviour, pinned before #1177 split `run_render_job` ──────

    /// A take folder `<root>/demo/take0000` with an `all` stream folder, and
    /// an export folder beside it.
    fn take_dirs(root: &Path) -> (PathBuf, PathBuf) {
        let take_folder = root.join("demo").join("take0000");
        std::fs::create_dir_all(take_folder.join("all")).unwrap();
        let export_dir = root.join("export");
        std::fs::create_dir_all(&export_dir).unwrap();
        (take_folder, export_dir)
    }

    fn take_clip(take_folder: &Path) -> ClipData {
        ClipData {
            clip_name: None,
            take_folder: take_folder.to_string_lossy().into_owned(),
            clip_type: "single".to_string(),
            img_folder: "all".to_string(),
            wav_file: None,
            base_name: "t".to_string(),
            frame_count: 0,
            width: 0,
            height: 0,
            date: "-".to_string(),
            video_file: None,
            alpha_folder: None,
        }
    }

    async fn run(clip: ClipData, config: RenderConfig, cancel: bool) -> Vec<RenderUpdate> {
        let (tx, rx) = mpsc::channel();
        run_render_job(
            "7".to_string(),
            clip,
            config,
            tx,
            Arc::new(AtomicBool::new(cancel)),
            Arc::new(Mutex::new(HashMap::new())),
        )
        .await;
        drain(&rx)
    }

    /// The updates as plain text, so a sequence compares in one assert.
    fn describe(updates: &[RenderUpdate]) -> Vec<String> {
        updates
            .iter()
            .map(|u| match u {
                RenderUpdate::Progress(id, p) => format!("{id} progress {p}"),
                RenderUpdate::Speed(id, s) => format!("{id} speed {s}"),
                RenderUpdate::Status(id, s) => format!("{id} status {s}"),
                RenderUpdate::OutputPath(id, _) => format!("{id} output"),
                RenderUpdate::Finished(id, ok, err) => {
                    format!("{id} finished {ok} {}", err.as_deref().unwrap_or("-"))
                }
            })
            .collect()
    }

    fn only_refusal(updates: &[RenderUpdate], text: &str) {
        assert_eq!(
            describe(updates),
            vec![format!("7 finished false {text}")],
            "{updates:?}"
        );
    }

    fn files_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// Each refusal before any work is one `Finished` with its own text, in
    /// this order: FFmpeg, take folder, wav, HUD without a wav, no audio, Skip
    /// not allowed.
    #[tokio::test]
    async fn refusals_before_any_work_send_one_finished_with_their_text() {
        let root = scratch("refusals");
        let (take_folder, export_dir) = take_dirs(&root);
        let h264 = |ffmpeg: &str| RenderConfig {
            ffmpeg_path: ffmpeg.to_string(),
            target_codec: RenderCodec::H264Software,
            ..source_copy_config(&export_dir)
        };
        let missing_ffmpeg = root.join("nope").join("ffmpeg.exe");

        let mut with_wav = take_clip(&take_folder);
        with_wav.wav_file = Some("sound.wav".to_string());
        let updates = run(
            with_wav.clone(),
            h264(&missing_ffmpeg.to_string_lossy()),
            false,
        )
        .await;
        only_refusal(
            &updates,
            &format!("FFmpeg not found at: {}", missing_ffmpeg.display()),
        );

        // Skip never runs FFmpeg, so a bad path doesn't stop it; the missing
        // take folder does.
        let gone = root.join("demo").join("take9999");
        let mut skip_cfg = source_copy_config(&export_dir);
        skip_cfg.ffmpeg_path = missing_ffmpeg.to_string_lossy().into_owned();
        let updates = run(take_clip(&gone), skip_cfg, false).await;
        only_refusal(
            &updates,
            &format!("Take folder not found: {}", gone.display()),
        );

        let updates = run(with_wav.clone(), h264("ffmpeg"), false).await;
        only_refusal(
            &updates,
            &format!(
                "Audio file not found: {}",
                take_folder.join("sound.wav").display()
            ),
        );

        let mut hud = take_clip(&take_folder);
        hud.clip_type = "hud_only".to_string();
        hud.video_file = Some("video.avi".to_string());
        let updates = run(hud, h264("ffmpeg"), false).await;
        only_refusal(
            &updates,
            "HUD/alpha clips need a separate audio track (sound.wav), but this take has none.",
        );

        let updates = run(take_clip(&take_folder), h264("ffmpeg"), false).await;
        only_refusal(
            &updates,
            "No audio source for this take: no sound.wav and no audio-bearing video.",
        );

        std::fs::write(take_folder.join("sound.wav"), b"wav").unwrap();
        let mut hud = with_wav.clone();
        hud.clip_type = "hud_only".to_string();
        hud.video_file = Some("video.avi".to_string());
        let updates = run(hud, source_copy_config(&export_dir), false).await;
        only_refusal(
            &updates,
            "Skip (keep original) isn't available for HUD/alpha clips — pick a codec to render them.",
        );
        let mut wav_and_video = with_wav.clone();
        wav_and_video.video_file = Some("video.avi".to_string());
        let updates = run(wav_and_video, source_copy_config(&export_dir), false).await;
        only_refusal(
            &updates,
            "Skip (keep original) isn't available for a take with its own separate audio track — pick a codec to render it.",
        );
        assert!(files_in(&export_dir).is_empty());
    }

    /// No export folder with room for the job: refused with the space it
    /// needed, before anything is written.
    #[tokio::test]
    async fn a_job_too_big_for_every_export_folder_is_refused_with_the_space_it_needs() {
        let root = scratch("no_room");
        let (take_folder, export_dir) = take_dirs(&root);
        std::fs::write(take_folder.join("sound.wav"), b"wav").unwrap();
        let mut clip = take_clip(&take_folder);
        clip.wav_file = Some("sound.wav".to_string());
        // A frame size no other test uses, since learned rates are session-wide.
        (clip.width, clip.height, clip.frame_count) = (100_003, 100_000, 1_000_000);
        let config = RenderConfig {
            target_codec: RenderCodec::H264Software,
            ..source_copy_config(&export_dir)
        };
        let needed = 100_003u64 * 100_000 * 3 * 1_000_000 + EXPORT_SAFETY_MARGIN_BYTES;
        let updates = run(clip, config, false).await;
        only_refusal(
            &updates,
            &format!(
                "Not rendered: every export folder (1) has less than {:.1} GB free once the other renders running now are counted. Free some space or add another export folder in Configuration → Render Settings.",
                needed as f64 / (1024.0 * 1024.0 * 1024.0)
            ),
        );
    }

    /// Custom with no arguments is refused after the export folder is
    /// picked, with nothing written into it.
    #[tokio::test]
    async fn custom_without_arguments_is_refused() {
        let root = scratch("custom_empty");
        let (take_folder, export_dir) = take_dirs(&root);
        std::fs::write(take_folder.join("sound.wav"), b"wav").unwrap();
        let mut clip = take_clip(&take_folder);
        clip.wav_file = Some("sound.wav".to_string());
        let config = RenderConfig {
            target_codec: RenderCodec::Custom,
            custom_codec_args: "   ".to_string(),
            ..source_copy_config(&export_dir)
        };
        let updates = run(clip, config, false).await;
        only_refusal(
            &updates,
            "Custom codec selected but no FFmpeg arguments were entered.",
        );
        assert!(files_in(&export_dir).is_empty());
    }

    /// An FFmpeg path that names a file which isn't a program: `Rendering`,
    /// then the spawn error.
    #[tokio::test]
    async fn an_ffmpeg_that_cannot_start_reports_the_spawn_error() {
        let root = scratch("spawn_fail");
        let (take_folder, export_dir) = take_dirs(&root);
        std::fs::write(take_folder.join("sound.wav"), b"wav").unwrap();
        let not_a_program = root.join("ffmpeg.txt");
        std::fs::write(&not_a_program, b"not a program").unwrap();
        let mut clip = take_clip(&take_folder);
        clip.wav_file = Some("sound.wav".to_string());
        let config = RenderConfig {
            ffmpeg_path: not_a_program.to_string_lossy().into_owned(),
            target_codec: RenderCodec::H264Software,
            ..source_copy_config(&export_dir)
        };
        let updates = run(clip, config, false).await;
        let text = describe(&updates);
        assert_eq!(text.len(), 2, "{text:?}");
        assert_eq!(text[0], "7 status Rendering");
        assert!(
            text[1].starts_with("7 finished false Failed to spawn FFmpeg process: "),
            "{text:?}"
        );
    }

    /// A Skip job's whole update sequence, and the file name it gets without
    /// a clip name: `<demo>_<take>_all_<4 hex digits>.<source extension>`.
    #[tokio::test]
    async fn skip_sends_its_updates_in_order_and_names_the_file_after_the_take() {
        let root = scratch("skip_sequence");
        let (take_folder, export_dir) = take_dirs(&root);
        std::fs::write(take_folder.join("all").join("video.mkv"), b"v").unwrap();
        let mut clip = take_clip(&take_folder);
        clip.video_file = Some("video.mkv".to_string());
        let updates = run(clip, source_copy_config(&export_dir), false).await;
        assert_eq!(
            describe(&updates),
            vec![
                "7 status Rendering",
                "7 status Finished",
                "7 progress 100",
                "7 output",
                "7 finished true -",
            ]
        );
        let names = files_in(&export_dir);
        assert_eq!(names.len(), 1);
        let name = &names[0];
        let hash = name
            .strip_prefix("demo_take0000_all_")
            .and_then(|rest| rest.strip_suffix(".mkv"))
            .unwrap_or_else(|| panic!("{name}"));
        assert!(
            hash.len() == 4 && hash.chars().all(|c| c.is_ascii_hexdigit()),
            "{name}"
        );
        let output = updates.iter().find_map(|u| match u {
            RenderUpdate::OutputPath(_, p) => Some(PathBuf::from(p)),
            _ => None,
        });
        assert_eq!(output, Some(export_dir.join(name)));
    }

    /// Cancel set before a Skip job starts: `Cancelled`, nothing copied.
    #[tokio::test]
    async fn skip_cancelled_before_it_starts_copies_nothing() {
        let root = scratch("skip_cancel_first");
        let (take_folder, export_dir) = take_dirs(&root);
        std::fs::write(take_folder.join("all").join("video.mp4"), b"v").unwrap();
        let mut clip = take_clip(&take_folder);
        clip.video_file = Some("video.mp4".to_string());
        let updates = run(clip, source_copy_config(&export_dir), true).await;
        assert_eq!(
            describe(&updates),
            vec!["7 status Cancelled", "7 finished false Cancelled by user"]
        );
        assert!(files_in(&export_dir).is_empty());
    }

    fn real_ffmpeg() -> PathBuf {
        let ffmpeg = PathBuf::from(
            std::env::var("DOD_FFMPEG").expect("DOD_FFMPEG must name ffmpeg.exe for this test"),
        );
        assert!(ffmpeg.is_file(), "no FFmpeg at {}", ffmpeg.display());
        ffmpeg
    }

    /// A 10-frame BMP take with a sound.wav, made by FFmpeg.
    fn make_bmp_take(ffmpeg: &Path, take_folder: &Path) {
        let made = std::process::Command::new(ffmpeg)
            .args(["-y", "-hide_banner", "-loglevel", "error", "-f", "lavfi"])
            .args(["-i", "testsrc2=size=64x48:rate=30:duration=0.334"])
            .args(["-frames:v", "10", "-start_number", "0"])
            .arg(take_folder.join("all").join("%05d.bmp"))
            .status()
            .unwrap();
        assert!(made.success());
        let made = std::process::Command::new(ffmpeg)
            .args(["-y", "-hide_banner", "-loglevel", "error", "-f", "lavfi"])
            .args(["-i", "sine=frequency=440:duration=0.334"])
            .arg(take_folder.join("sound.wav"))
            .status()
            .unwrap();
        assert!(made.success());
    }

    /// Cancel set before FFmpeg's first progress line: killed on the first
    /// poll, its partial file removed.
    #[tokio::test]
    #[ignore = "needs FFmpeg: set DOD_FFMPEG to ffmpeg.exe's path"]
    async fn an_ffmpeg_render_cancelled_at_once_leaves_no_file() {
        let ffmpeg = real_ffmpeg();
        let root = scratch("ffmpeg_cancel");
        let (take_folder, export_dir) = take_dirs(&root);
        make_bmp_take(&ffmpeg, &take_folder);
        let mut clip = take_clip(&take_folder);
        clip.wav_file = Some("sound.wav".to_string());
        clip.frame_count = 10;
        let config = RenderConfig {
            ffmpeg_path: ffmpeg.to_string_lossy().into_owned(),
            target_codec: RenderCodec::H264Software,
            ..source_copy_config(&export_dir)
        };
        let updates = run(clip, config, true).await;
        assert_eq!(
            describe(&updates),
            vec![
                "7 status Rendering",
                "7 status Cancelled",
                "7 finished false Cancelled by user",
            ]
        );
        assert!(files_in(&export_dir).is_empty());
    }

    /// FFmpeg exiting with an error: `Error`, its exit code and log in the
    /// message, its partial file removed.
    #[tokio::test]
    #[ignore = "needs FFmpeg: set DOD_FFMPEG to ffmpeg.exe's path"]
    async fn an_ffmpeg_failure_reports_its_exit_code_and_log() {
        let ffmpeg = real_ffmpeg();
        let root = scratch("ffmpeg_fail");
        let (take_folder, export_dir) = take_dirs(&root);
        make_bmp_take(&ffmpeg, &take_folder);
        let mut clip = take_clip(&take_folder);
        clip.wav_file = Some("sound.wav".to_string());
        clip.frame_count = 10;
        let config = RenderConfig {
            ffmpeg_path: ffmpeg.to_string_lossy().into_owned(),
            target_codec: RenderCodec::Custom,
            custom_codec_args: "-c:v no_such_codec".to_string(),
            ..source_copy_config(&export_dir)
        };
        let text = describe(&run(clip, config, false).await);
        assert_eq!(text.len(), 3, "{text:?}");
        assert_eq!(text[0], "7 status Rendering");
        assert_eq!(text[1], "7 status Error");
        assert!(
            text[2].starts_with("7 finished false FFmpeg failed with exit code: ")
                && text[2].contains("no_such_codec"),
            "{text:?}"
        );
        assert!(files_in(&export_dir).is_empty());
    }

    /// A finished BMP render: `Rendering`, progress and speed lines while
    /// FFmpeg reports, then `Finished`, 100, the output path, success.
    #[tokio::test]
    #[ignore = "needs FFmpeg: set DOD_FFMPEG to ffmpeg.exe's path"]
    async fn an_ffmpeg_render_reports_progress_then_finishes() {
        let ffmpeg = real_ffmpeg();
        let root = scratch("ffmpeg_ok");
        let (take_folder, export_dir) = take_dirs(&root);
        make_bmp_take(&ffmpeg, &take_folder);
        let mut clip = take_clip(&take_folder);
        clip.wav_file = Some("sound.wav".to_string());
        clip.frame_count = 10;
        clip.clip_name = Some("bmp_take".to_string());
        let config = RenderConfig {
            ffmpeg_path: ffmpeg.to_string_lossy().into_owned(),
            target_codec: RenderCodec::H264Software,
            fps: 30,
            ..source_copy_config(&export_dir)
        };
        let text = describe(&run(clip, config, false).await);
        assert_eq!(text.first().map(String::as_str), Some("7 status Rendering"));
        assert_eq!(
            text[text.len() - 4..],
            [
                "7 status Finished",
                "7 progress 100",
                "7 output",
                "7 finished true -"
            ]
        );
        let middle = &text[1..text.len() - 4];
        assert!(middle.contains(&"7 progress 100".to_string()), "{text:?}");
        assert!(
            middle
                .iter()
                .all(|l| l.starts_with("7 progress ") || l.starts_with("7 speed ")),
            "{text:?}"
        );
        assert_eq!(files_in(&export_dir), vec!["bmp_take.mp4"]);
    }

    /// FFmpeg failing after it has started writing (a broken frame halfway,
    /// `-xerror`): `Error` with its log, and the half-written file removed.
    #[tokio::test]
    #[ignore = "needs FFmpeg: set DOD_FFMPEG to ffmpeg.exe's path"]
    async fn an_ffmpeg_failure_partway_removes_the_half_written_file() {
        let ffmpeg = real_ffmpeg();
        let root = scratch("ffmpeg_fail_partway");
        let (take_folder, export_dir) = take_dirs(&root);
        make_bmp_take(&ffmpeg, &take_folder);
        std::fs::write(take_folder.join("all").join("00005.bmp"), b"not a bmp").unwrap();
        let mut clip = take_clip(&take_folder);
        clip.wav_file = Some("sound.wav".to_string());
        clip.frame_count = 10;
        let config = RenderConfig {
            ffmpeg_path: ffmpeg.to_string_lossy().into_owned(),
            target_codec: RenderCodec::Custom,
            custom_codec_args: "-c:v ffv1 -xerror".to_string(),
            fps: 30,
            ..source_copy_config(&export_dir)
        };
        let text = describe(&run(clip, config, false).await);
        assert_eq!(text.first().map(String::as_str), Some("7 status Rendering"));
        assert_eq!(text[text.len() - 2], "7 status Error", "{text:?}");
        assert!(
            text[text.len() - 1].starts_with("7 finished false FFmpeg failed with exit code: "),
            "{text:?}"
        );
        assert!(files_in(&export_dir).is_empty());
    }

    /// Cancel partway through a render: the export drive stays reserved
    /// while FFmpeg writes, the half-written file is removed, and the
    /// reservation is released once the job ends.
    #[tokio::test]
    #[ignore = "needs FFmpeg: set DOD_FFMPEG to ffmpeg.exe's path"]
    async fn an_ffmpeg_render_cancelled_partway_keeps_its_reservation_till_it_ends() {
        let ffmpeg = real_ffmpeg();
        let root = scratch("ffmpeg_cancel_partway");
        let (take_folder, export_dir) = take_dirs(&root);
        make_bmp_take(&ffmpeg, &take_folder);
        let mut clip = take_clip(&take_folder);
        clip.wav_file = Some("sound.wav".to_string());
        clip.frame_count = 10;
        // `realtime` slows the 10 frames to several seconds, so the job is
        // still running when it is looked at and cancelled.
        let config = RenderConfig {
            ffmpeg_path: ffmpeg.to_string_lossy().into_owned(),
            target_codec: RenderCodec::Custom,
            custom_codec_args: "-c:v ffv1 -vf realtime=speed=0.05".to_string(),
            fps: 30,
            ..source_copy_config(&export_dir)
        };
        let reserved = |ledger: &Arc<Mutex<HashMap<PathBuf, u64>>>| -> u64 {
            ledger.lock().unwrap().values().sum()
        };
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let ledger = Arc::new(Mutex::new(HashMap::new()));
        let job = tokio::spawn(run_render_job(
            "7".to_string(),
            clip,
            config,
            tx,
            cancel.clone(),
            ledger.clone(),
        ));

        let mut updates = Vec::new();
        let started = std::time::Instant::now();
        while files_in(&export_dir).is_empty() {
            assert!(
                started.elapsed() < std::time::Duration::from_secs(20),
                "FFmpeg never wrote its output: {:?}",
                describe(&updates)
            );
            assert!(!job.is_finished(), "{:?}", describe(&drain(&rx)));
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            updates.extend(drain(&rx));
        }
        assert!(reserved(&ledger) > 0, "nothing reserved while rendering");
        cancel.store(true, Ordering::Relaxed);
        tokio::time::timeout(std::time::Duration::from_secs(10), job)
            .await
            .expect("the job did not stop on Cancel")
            .unwrap();
        updates.extend(drain(&rx));

        let text = describe(&updates);
        assert_eq!(text.first().map(String::as_str), Some("7 status Rendering"));
        assert_eq!(
            text[text.len() - 2..],
            ["7 status Cancelled", "7 finished false Cancelled by user"],
            "{text:?}"
        );
        assert!(
            text[1..text.len() - 2]
                .iter()
                .all(|l| l.starts_with("7 progress ") || l.starts_with("7 speed ")),
            "{text:?}"
        );
        assert!(files_in(&export_dir).is_empty());
        assert_eq!(reserved(&ledger), 0);
    }
}
