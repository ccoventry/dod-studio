use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use super::config::{RenderCodec, RenderConfig};
use super::scanner::ClipData;
use lock_recover::lock_or_recover;

mod clip_frames;
mod ffmpeg_args;
mod ffmpeg_run;
// Also a combine's (#1496).
pub(crate) mod partial_output;
mod segment_plan;
mod segmented;
pub mod thread_share;
/// What marks a render's unfinished file name (#1439): a folder walk for
/// finished clips skips names with it.
pub use partial_output::PARTIAL_MARK;

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

/// What [`try_reserve`] found: the directory it reserved on, if any, and the
/// ones it passed over before it.
#[derive(Debug, PartialEq)]
struct Reserved {
    /// The directory, what was already reserved on its volume, and its live
    /// free space (`None` when unreadable, which counts as having room).
    picked: Option<(PathBuf, u64, Option<u64>)>,
    /// Each directory passed over, with its room: live free space less what
    /// is reserved on its volume.
    passed_over: Vec<(PathBuf, u64)>,
}

/// Reserves `amount` on the first of `dirs` whose volume has more than
/// `amount + margin_of(dir)` free once the ledger's reservations there are
/// taken off. `margin_of` is per directory: the Windows drive keeps more
/// (#1430).
fn try_reserve(
    ledger: &mut HashMap<PathBuf, u64>,
    dirs: &[PathBuf],
    amount: u64,
    mut margin_of: impl FnMut(&Path) -> u64,
    mut free_of: impl FnMut(&Path) -> Option<u64>,
) -> Reserved {
    let mut passed_over = Vec::new();
    for dir in dirs {
        let key = ledger_key(dir);
        let live_free = free_of(dir);
        let already_reserved = *ledger.get(&key).unwrap_or(&0);
        let Some(free) = live_free else {
            *ledger.entry(key).or_insert(0) += amount;
            return Reserved {
                picked: Some((dir.clone(), already_reserved, None)),
                passed_over,
            };
        };
        let room = free.saturating_sub(already_reserved);
        if room > amount.saturating_add(margin_of(dir)) {
            *ledger.entry(key).or_insert(0) += amount;
            return Reserved {
                picked: Some((dir.clone(), already_reserved, live_free)),
                passed_over,
            };
        }
        passed_over.push((dir.clone(), room));
    }
    Reserved {
        picked: None,
        passed_over,
    }
}

/// One way a render can fit on the export drives: `amount` bytes on one of
/// `dirs`, the first with room.
pub(super) struct RoomAsk {
    pub(super) dirs: Vec<PathBuf>,
    pub(super) amount: u64,
}

/// Reserves the first of `asks` that fits (see [`try_reserve`]): its index,
/// and what that ask found; the last ask's when none fits (`None`).
/// `asks` is never empty: the one-piece ask is always last.
fn reserve_first_fit(
    ledger: &mut HashMap<PathBuf, u64>,
    asks: &[RoomAsk],
    mut margin_of: impl FnMut(&Path) -> u64,
    mut free_of: impl FnMut(&Path) -> Option<u64>,
) -> (Option<usize>, Reserved) {
    let mut last = Reserved {
        picked: None,
        passed_over: Vec::new(),
    };
    for (index, ask) in asks.iter().enumerate() {
        last = try_reserve(ledger, &ask.dirs, ask.amount, &mut margin_of, &mut free_of);
        if last.picked.is_some() {
            return (Some(index), last);
        }
    }
    (None, last)
}

/// The first directory in `passed_over` that had room for `amount` past the
/// usual [`EXPORT_SAFETY_MARGIN_BYTES`] and was passed over only for
/// "Keep free on the Windows drive" (#1430), with its room. `margin_of` is the
/// margin the choice used.
fn held_back_for_windows_drive(
    passed_over: &[(PathBuf, u64)],
    amount: u64,
    mut margin_of: impl FnMut(&Path) -> u64,
) -> Option<&(PathBuf, u64)> {
    passed_over.iter().find(|(dir, room)| {
        *room > amount.saturating_add(EXPORT_SAFETY_MARGIN_BYTES)
            && margin_of(dir) > EXPORT_SAFETY_MARGIN_BYTES
    })
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
        let mut ledger = lock_or_recover(&self.ledger);
        if let Some(reserved) = ledger.get_mut(&self.dir) {
            *reserved = reserved.saturating_sub(self.amount);
        }
        let remaining = ledger.get(&self.dir).copied().unwrap_or(0);
        crate::log_markdown(&crate::messages::render::reservation_released(
            &self.job_id,
            self.amount,
            &self.dir,
            remaining,
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
    // Studio is closing (#1439): a job the scheduler started on its way out
    // does nothing and says nothing.
    if is_exiting() {
        return;
    }
    let job = RenderJob::new(job_id, clip, config, tx, cancel_rx);
    // The take's own capture rate when its `dodstudio_take.json` says, else
    // Render Settings' Source FPS (`take_meta::render_fps`).
    let (fps_used, fps_from_take) = super::take_meta::render_fps(&job.take_folder, job.config.fps);

    if let Err(reason) = check_inputs(&job) {
        job.refuse(reason);
        return;
    }
    log_render_fps(&job, fps_used, fps_from_take);
    // A deleted take's segments go whatever this render is (#1451), the
    // setting off included: turning it off to save space gets the space back.
    segmented::remove_orphans(&job).await;

    // The guard releases this job's reservation on every exit path below —
    // see `ReservationGuard`'s own doc comment.
    let Some((output_folder, _reservation_guard, segments)) =
        reserve_room(&job, &reservations, fps_used).await
    else {
        return;
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
    let output = naming.reserve(&job, &output_folder, encode.file_ext);
    if let Some(prepared) = &segments
        && let Some(folder) = segmented::claim_folder(&job, prepared, &output_folder).await
    {
        // Each segment counts its own threads as it starts (#1512).
        let render = segmented::SegmentedRender {
            plan: &prepared.plan,
            folder,
            encode: &encode,
            output: &output,
        };
        // A segment that came out short sends the take to one piece, below.
        if segmented::run_segmented(&job, render).await == segmented::SegmentedEnd::Ended {
            return;
        }
    }
    // One FFmpeg run: it keeps the share the job started with (#1512).
    let threads =
        ffmpeg_args::threads_per_process(available_threads(), job.config.max_concurrent_renders);
    let args = ffmpeg_args::FfmpegArgs {
        clip: &job.clip,
        fps: fps_used,
        encode: &encode,
        threads,
        out_file: &output.partial,
        clip_tag: job.config.clip_tag.as_deref(),
    }
    .to_args();
    ffmpeg_run::run_ffmpeg(&job, args, &output).await;
}

/// The machine's threads, or `None` when it won't say.
fn available_threads() -> Option<usize> {
    std::thread::available_parallelism().ok().map(|n| n.get())
}

/// Reserves room for `job` on an export drive: the export folder, the guard
/// that releases it, and the segmented render it has room for, or `None`
/// for one piece. A long take renders in segments that a stopped render
/// resumes from (#1451); its segments and the clip they join into are both
/// on the drive for a moment, so it asks for room for both, on the drive its
/// stopped segments are on first. With room for the clip only, it renders
/// in one piece as before, and this take's segments are removed. `None`,
/// with the job refused, when no drive has room even for that.
async fn reserve_room(
    job: &RenderJob,
    reservations: &Arc<Mutex<HashMap<PathBuf, u64>>>,
    fps_used: u32,
) -> Option<(PathBuf, ReservationGuard, Option<segmented::Prepared>)> {
    let mut prepared = segmented::prepare(job, fps_used);
    let one_piece_bytes = job_reservation_estimate(
        &job.clip,
        job.config.target_codec,
        &job.config.custom_codec_args,
    );
    let mut asks = prepared
        .as_ref()
        .map(|s| s.room_asks(&job.config.export_directories, one_piece_bytes))
        .unwrap_or_default();
    // FFmpeg joins in the segment folder, and Windows refuses a working
    // folder past its path limit (#1451 review): with no export folder short
    // enough, the take renders in one piece from the start, reserving and
    // making room for the clip alone.
    if prepared.is_some() && asks.is_empty() {
        crate::log_markdown(&crate::messages::render::segments_path_too_long(
            &job.job_id,
            segmented::MAX_SEGMENT_DIR_UNITS,
        ));
        prepared = None;
    }
    asks.push(RoomAsk {
        dirs: job.config.export_directories.clone(),
        amount: one_piece_bytes,
    });
    let (output_folder, guard, picked) = match reserve_export_dir(job, reservations, &asks) {
        Ok(reserved) => reserved,
        Err(reason) => {
            job.refuse(reason);
            return None;
        }
    };
    // Only the last ask is one piece; a segmented render asked more first.
    if prepared.is_some() && picked + 1 == asks.len() {
        crate::log_markdown(&crate::messages::render::segments_no_room(
            &job.job_id,
            asks[picked - 1].amount,
        ));
        prepared = None;
    }
    // A take rendering in one piece, the setting off included, frees the
    // room its stopped segments hold: nothing will join them.
    if prepared.is_none() {
        segmented::remove_take_segments(job).await;
    }
    Some((output_folder, guard, prepared))
}

/// A render batch's first activity log line: the FFmpeg `configured`
/// (Render Settings' path, or a bare `ffmpeg` on PATH) resolves to and its
/// `-version` banner, or why that can't be read, and whether "Resumable
/// renders" is on as it starts. Starts FFmpeg once and waits up to 5 s
/// (`hlae_ffmpeg::verify_is_ffmpeg`): call it once per batch, off the main
/// thread. Only logged: whether a join is whole never depends on the
/// version.
pub fn render_batch_start_line(configured: &str, resumable: bool) -> String {
    use crate::messages::render as msg;
    let Some(exe) = crate::shared::hlae_ffmpeg::resolve_absolute(configured) else {
        return msg::render_batch_start_unread(configured, "not found", resumable);
    };
    match crate::shared::hlae_ffmpeg::verify_is_ffmpeg(&exe) {
        Ok(banner) => msg::render_batch_start(&banner, exe.display(), resumable),
        Err(why) => msg::render_batch_start_unread(configured, why, resumable),
    }
}

/// Whether export folder `export_dir`'s path is short enough to hold a
/// render's segment folder (#1451): one longer renders long takes in one
/// piece, and room is made for the clip alone there.
pub fn segments_fit_under(export_dir: &Path) -> bool {
    segmented::segments_fit_under(export_dir)
}

/// What a render of `clip` reserves on its export drive at most when it
/// starts from nothing: twice the clip for a take that renders in segments
/// (#1451), the clip otherwise (`job_reservation_estimate`). For the take
/// cleanup that makes room before a batch starts, so a long take gets the
/// room to render in segments. Reads the take's wav header and, for an AVI
/// take, its video header.
pub fn job_peak_reservation_estimate(
    clip: &ClipData,
    codec: RenderCodec,
    custom_args: &str,
    setting_fps: u32,
) -> u64 {
    let one_piece = job_reservation_estimate(clip, codec, custom_args);
    if codec == RenderCodec::SourceCopy {
        return one_piece;
    }
    let (fps_used, _) = super::take_meta::render_fps(Path::new(&clip.take_folder), setting_fps);
    segment_plan::plan(&segmented::take_facts(clip, codec, fps_used)).map_or(one_piece, |plan| {
        segment_plan::peak_bytes(one_piece, &plan, 0)
    })
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
        let source = if fps_from_take {
            crate::messages::render::FPS_FROM_TAKE.to_string()
        } else {
            crate::messages::render::fps_from_render_settings(job.take_folder.display())
        };
        crate::log_markdown(&crate::messages::render::render_fps(
            &job.job_id,
            fps_used,
            source,
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
///
/// `asks` are the ways the job can fit, in order of preference (a segmented
/// render's before its one-piece fallback, #1451); the last is the one
/// piece, whose amount the refusal names. Returns the folder, its guard and
/// which ask fitted.
fn reserve_export_dir(
    job: &RenderJob,
    reservations: &Arc<Mutex<HashMap<PathBuf, u64>>>,
    asks: &[RoomAsk],
) -> Result<(PathBuf, ReservationGuard, usize), String> {
    let config = &job.config;

    // No export folder: refused, never written into the working directory
    // (for an installed app, its install folder), where nobody would look.
    if config.export_directories.is_empty() {
        return Err(NO_EXPORT_FOLDER.to_string());
    }
    // Read per job, not per batch: a change to "Keep free on the Windows
    // drive" reaches the next render of a running batch (#1430).
    let margins: HashMap<PathBuf, u64> = config
        .export_directories
        .iter()
        .map(|dir| {
            let margin = crate::sys::windows_drive::headroom_for(dir, EXPORT_SAFETY_MARGIN_BYTES);
            (dir.clone(), margin)
        })
        .collect();
    let margin_of = |dir: &Path| {
        margins
            .get(dir)
            .copied()
            .unwrap_or(EXPORT_SAFETY_MARGIN_BYTES)
    };
    let mut ledger = lock_or_recover(reservations);
    let (fitted, reserved) = reserve_first_fit(
        &mut ledger,
        asks,
        margin_of,
        crate::sys::disk::get_available_bytes,
    );
    let reservation_estimate = asks[fitted.unwrap_or(asks.len() - 1)].amount;
    let held_back =
        held_back_for_windows_drive(&reserved.passed_over, reservation_estimate, margin_of);
    let keep_free = crate::sys::windows_drive::keep_free_bytes();
    if let Some((dir, room)) = held_back {
        crate::log_markdown(&crate::messages::windows_drive::render_folder_skipped(
            &job.job_id,
            dir.display(),
            *room,
            keep_free,
        ));
    }
    let (Some(fitted), Some((dir, already_reserved, live_free))) = (fitted, reserved.picked) else {
        let note = held_back
            .map(|_| crate::messages::windows_drive::kept_free_note(keep_free))
            .unwrap_or_default();
        return Err(crate::messages::render::no_export_folder_with_room(
            config.export_directories.len(),
            reservation_estimate + EXPORT_SAFETY_MARGIN_BYTES,
            note,
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
    crate::log_markdown(&crate::messages::render::reservation_made(
        &job.job_id,
        reservation_estimate,
        &dir,
        already_reserved,
        live_free_text,
    ));
    drop(ledger);
    let guard = ReservationGuard {
        ledger: reservations.clone(),
        dir: ledger_key(&dir),
        amount: reservation_estimate,
        job_id: job.job_id.clone(),
    };
    Ok((dir, guard, fitted))
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
            // Only seeds a file-name hash; a clock before 1970 gives 0.
            .map_or(0, |d| d.as_micros());
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

    /// Reserves the output's name in `output_folder` for an output with
    /// `file_ext` (with its dot), made unique there: the clip name (#441),
    /// cut to fit that folder (#1430), else
    /// `<demo>_<take><wav part>_<stream>_<hash>`. Held until the job ends
    /// (`partial_output`, #1439).
    fn reserve(
        &self,
        job: &RenderJob,
        output_folder: &Path,
        file_ext: &str,
    ) -> partial_output::Reserved<'static> {
        let room = name_room(output_folder, &self.wav_part, job.is_hud, file_ext);
        if let Some(name) = job.clip.clip_name.as_deref()
            && sanitize_file_stem(name).encode_utf16().count() > room
        {
            crate::log_markdown(&crate::messages::render::clip_name_shortened(
                name,
                output_folder.display(),
            ));
        }
        let stem =
            clip_file_stem(&job.clip, &self.wav_part, job.is_hud, room).unwrap_or_else(|| {
                format!(
                    "{}_{}{}_{}_{}",
                    self.demo_name, self.take_name, self.wav_part, self.stream_type, self.hash_str
                )
            });
        partial_output::in_flight().reserve(output_folder, &stem, file_ext)
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
    let output = naming.reserve(job, output_folder, &format!(".{ext}"));

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
    // Like FFmpeg, the copy writes under the partial name and takes the
    // real one only once it is whole (#1439).
    let copied = copy_cancellable(&source_video, &output.partial, &job.cancel).await;
    if is_exiting() {
        return;
    }
    match copied {
        Ok(true) => job.finish_cancelled(),
        Ok(false) => match partial_output::in_flight().finish(&output) {
            Ok(done) => job.finish_ok(done.to_string_lossy().into_owned()),
            Err(e) => job.finish_error(crate::messages::render::rename_failed(
                output.partial.display(),
                output.out_file.display(),
                e,
            )),
        },
        Err(e) => {
            remove_partial_output(&output.partial).await;
            job.finish_error(format!("Failed to copy source video to export pool: {}", e));
        }
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
/// Returns `Ok(true)` if cancelled partway through, or Studio is closing
/// (#1439), in which case the partial `dst` is removed before returning — a
/// truncated file left behind would look like a finished export.
async fn copy_cancellable(src: &Path, dst: &Path, cancel: &AtomicBool) -> std::io::Result<bool> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut reader = tokio::fs::File::open(src).await?;
    let mut writer = tokio::fs::File::create(dst).await?;
    // 2 MiB: large enough that per-chunk overhead is negligible against the
    // copy itself, and a cancel lands within a second or two even on a slow
    // disk.
    let mut buf = vec![0u8; 2 * 1024 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) || is_exiting() {
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

/// Removes what a stopped or failed render wrote of its partial file. The
/// name is the job's own reservation (`partial_output`), so whatever is there
/// is this job's, or a leftover of a render that never finished.
async fn remove_partial_output(partial: &Path) {
    let _ = tokio::fs::remove_file(partial).await;
}

/// Set once Studio is on its way out (#1439): renders stopped then report
/// nothing and rename nothing, and the scheduler starts nothing more.
static EXITING: AtomicBool = AtomicBool::new(false);

/// Whether Studio is closing: see [`stop_renders_for_exit`].
pub fn is_exiting() -> bool {
    EXITING.load(Ordering::SeqCst)
}

/// Studio's way out (#1439): ends every FFmpeg in Studio's job at once
/// (renders, combines, take checks, the OBS benchmark: #1496) and removes
/// the unfinished files the renders and combines were writing, so none
/// outlives Studio or stays in an export folder. Marks Studio as exiting
/// first, so a stopped render isn't recorded as failed and nothing new
/// starts. Blocks for up to about 2 s while Windows lets go of the files.
/// Logs what it did. Named for the renders it was written for.
pub fn stop_renders_for_exit() {
    EXITING.store(true, Ordering::SeqCst);
    let began = std::time::Instant::now();
    match crate::sys::child_job::end_all_with_studio() {
        Ok(stopped) if !stopped.is_empty() => {
            crate::log_markdown(&crate::messages::exit_children_stopped(&stopped));
        }
        Ok(_) => {}
        Err(e) => crate::log_markdown(&crate::messages::render::exit_stop_failed(e)),
    }
    let removed = partial_output::in_flight().remove_all();
    if removed.in_flight == 0 && removed.combines == 0 {
        return;
    }
    crate::log_markdown(&crate::messages::render::exit_renders_stopped(
        removed.in_flight,
        removed.combines,
        removed.removed,
        began.elapsed(),
    ));
    for (path, error) in &removed.failed {
        crate::log_markdown(&crate::messages::render::exit_partial_left(
            path.display(),
            error,
        ));
    }
}

/// The longest output path a render writes, in UTF-16 units as Windows
/// counts them: `PATH_LIMIT` in `studio/src/clip_name.js`, which cuts clip
/// names to fit the longest export folder when they are queued.
const OUTPUT_PATH_LIMIT: usize = 250;
/// What reserving a free name may add (`_99`, `partial_output`).
const UNIQUE_SUFFIX_UNITS: usize = 3;

/// How many UTF-16 units a clip name may take in `output_folder` so the
/// whole path, with the wav suffix, `_hud`, a uniqueness suffix, the
/// [`PARTIAL_MARK`] the file is written under until it is finished (#1439)
/// and the extension, stays within [`OUTPUT_PATH_LIMIT`]: the unfinished
/// file's path is the longer one, so it is the one that must fit. Names are cut to fit when
/// they are queued, but an export folder added mid-batch (#1430) can be
/// longer than the ones they were cut for.
fn name_room(output_folder: &Path, wav_part: &str, is_hud: bool, file_ext: &str) -> usize {
    let units = |s: &str| s.encode_utf16().count();
    let taken = units(&output_folder.to_string_lossy())
        + 1
        + units(wav_part)
        + if is_hud { units("_hud") } else { 0 }
        + UNIQUE_SUFFIX_UNITS
        + units(PARTIAL_MARK)
        + units(file_ext);
    OUTPUT_PATH_LIMIT.saturating_sub(taken)
}

/// `name` cut to at most `room` UTF-16 units, without the trailing dots and
/// spaces Windows drops.
fn fit_name(name: &str, room: usize) -> String {
    let mut used = 0;
    let cut: String = name
        .chars()
        .take_while(|c| {
            used += c.len_utf16();
            used <= room
        })
        .collect();
    cut.trim_end_matches(['.', ' ']).to_string()
}

/// The finished file's name, without its extension, when the take has a clip
/// name (#441): the name cut to `name_room` ([`name_room`]), the wav suffix,
/// and `_hud` for the HUD stream. The caller makes it unique, so a re-render
/// never overwrites an earlier file. `None` without a usable clip name, or
/// with no room left for one (the hashed default name is used then).
fn clip_file_stem(
    clip: &ClipData,
    wav_part: &str,
    is_hud: bool,
    name_room: usize,
) -> Option<String> {
    let name = fit_name(&sanitize_file_stem(clip.clip_name.as_deref()?), name_room);
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

#[cfg(test)]
#[path = "renderer_tests.rs"]
mod tests;
