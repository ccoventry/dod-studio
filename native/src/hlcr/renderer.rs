#![cfg(not(target_arch = "wasm32"))]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;

use super::config::{RenderCodec, RenderConfig};
use super::scanner::ClipData;

#[derive(Clone, Debug)]
pub enum RenderUpdate {
    Progress(String, u32),                  // (job_id, percentage)
    Speed(String, String),                  // (job_id, speed_text)
    Status(String, String),                 // (job_id, status_text)
    OutputPath(String, String),             // (job_id, absolute path to the encoded file)
    Finished(String, bool, Option<String>), // (job_id, success, error_log)
}

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
/// live free space.
fn try_reserve(
    ledger: &mut HashMap<PathBuf, u64>,
    dirs: &[PathBuf],
    amount: u64,
    margin: u64,
    mut free_of: impl FnMut(&Path) -> u64,
) -> Option<(PathBuf, u64, u64)> {
    for dir in dirs {
        let key = ledger_key(dir);
        let live_free = free_of(dir);
        let already_reserved = *ledger.get(&key).unwrap_or(&0);
        if live_free.saturating_sub(already_reserved) > amount + margin {
            *ledger.entry(key).or_insert(0) += amount;
            return Some((dir.clone(), already_reserved, live_free));
        }
    }
    None
}

/// RAII release for a JIT export-drive reservation claimed in
/// `run_render_job`'s routing block below. `run_render_job` has many early
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

pub async fn run_render_job(
    job_id: String,
    clip: ClipData,
    config: RenderConfig,
    tx: mpsc::Sender<RenderUpdate>,
    cancel_rx: Arc<AtomicBool>,
    reservations: Arc<Mutex<HashMap<PathBuf, u64>>>,
) {
    // "Skip" — leave an OBS take as OBS wrote it. No FFmpeg involved at all,
    // just a copy into the export pool with the pipeline's naming applied.
    let is_source_copy = config.target_codec == super::config::RenderCodec::SourceCopy;
    let ffmpeg_path = PathBuf::from(&config.ffmpeg_path);

    let take_folder = PathBuf::from(&clip.take_folder);
    // The take's own capture rate when its `dodstudio_take.json` says, else
    // Render Settings' Source FPS (`take_meta::render_fps`).
    let (fps_used, fps_from_take) = super::take_meta::render_fps(&take_folder, config.fps);
    let fps = fps_used.to_string();
    // `None` is an OBS take: its audio is already muxed into `video_file`,
    // so there is nothing to join against a wav.
    let wav_file: Option<PathBuf> = clip.wav_file.as_ref().map(|w| take_folder.join(w));
    let is_hud = clip.clip_type == "hud_only";
    // Where the clip sits in an OBS take's recording (#84): every codec cuts
    // the head and tail OBS records around it. Keep as captured too, which
    // then can't be a plain copy (`trimmed_copy_video_args`).
    let obs_trim = if clip.video_file.is_some() && clip.wav_file.is_none() && !is_hud {
        super::take_meta::obs_trim(&take_folder)
    } else {
        None
    };
    let is_plain_copy = is_source_copy && obs_trim.is_none();

    let is_global = config.ffmpeg_path == "ffmpeg";

    // Initial validations. Skip mode never spawns FFmpeg, so a misconfigured
    // FFmpeg path must not block a plain file copy.
    if !is_plain_copy && !is_global && (!ffmpeg_path.exists() || !ffmpeg_path.is_file()) {
        let _ = tx.send(RenderUpdate::Finished(
            job_id.clone(),
            false,
            Some(format!("FFmpeg not found at: {}", ffmpeg_path.display())),
        ));
        return;
    }
    if !take_folder.exists() || !take_folder.is_dir() {
        let _ = tx.send(RenderUpdate::Finished(
            job_id.clone(),
            false,
            Some(format!("Take folder not found: {}", take_folder.display())),
        ));
        return;
    }
    if let Some(wav) = &wav_file
        && !wav.exists()
    {
        let _ = tx.send(RenderUpdate::Finished(
            job_id.clone(),
            false,
            Some(format!("Audio file not found: {}", wav.display())),
        ));
        return;
    }
    // The alpha stream carries no sound of its own, so a HUD/alpha composite
    // always needs a separate wav — an OBS take's muxed-in audio can't cover it.
    if is_hud && wav_file.is_none() {
        let _ = tx.send(RenderUpdate::Finished(
            job_id.clone(),
            false,
            Some(
                "HUD/alpha clips need a separate audio track (sound.wav), but this take has none."
                    .to_string(),
            ),
        ));
        return;
    }
    if clip.video_file.is_none() && wav_file.is_none() {
        let _ = tx.send(RenderUpdate::Finished(
            job_id.clone(),
            false,
            Some(
                "No audio source for this take: no sound.wav and no audio-bearing video."
                    .to_string(),
            ),
        ));
        return;
    }
    // Gated on the same predicate the Tauri layer uses to decide whether to
    // offer the toggle at all (`clip_is_skip_eligible`), so this can never
    // quietly diverge from what the frontend thinks is skippable — this is
    // the actual enforcement, the frontend gate is only a convenience.
    if is_source_copy && !super::scanner::clip_is_skip_eligible(&clip) {
        let reason = if is_hud {
            "Skip (keep original) isn't available for HUD/alpha clips — pick a codec to render them."
        } else if clip.wav_file.is_some() {
            "Skip (keep original) isn't available for a take with its own separate audio track — pick a codec to render it."
        } else {
            "Skip (keep original) requires a captured video file, but this take has none."
        };
        let _ = tx.send(RenderUpdate::Finished(
            job_id.clone(),
            false,
            Some(reason.to_string()),
        ));
        return;
    }

    // The FPS is fed to `-framerate` *before* the BMP input, so it is not an
    // output preference — it is how FFmpeg is told to interpret the frame
    // sequence's timing. A value that disagrees with the capture produces a
    // wrong computed duration and `-shortest` then trims the audio to match, so
    // the render would succeed and the clip simply be the wrong speed. Hence the
    // take's own recorded rate wins over the setting (#14, `hlcr::take_meta`).
    //
    // Logged, not sent as a `Status` update: `job.status` is a state field with
    // a fixed vocabulary ("Queued" | "Rendering" | "Finished" | "Error" |
    // "Cancelled") that the scheduler's own transitions test against. The row
    // itself shows the rate through the job's settings summary.
    if !is_source_copy && clip.video_file.is_none() {
        crate::log_markdown(&format!(
            "[render-fps] job {} — reading the frames at {} fps, {}",
            job_id,
            fps_used,
            if fps_from_take {
                "the rate the take was captured at".to_string()
            } else {
                format!(
                    "Render Settings' Source FPS (the take doesn't say what it was captured at; {})",
                    take_folder.display()
                )
            }
        ));
    }

    let clip_type = clip.clip_type.as_str();

    // ── JIT export-drive routing ──────────────────────────────────────────────
    // Select the first directory with enough room for *this specific job* —
    // "enough room" meaning live free space minus whatever other
    // concurrently-running jobs have already provisionally reserved on that
    // same drive (`reservations`), not just the live free-space number
    // alone. Without that ledger, several jobs starting in the same
    // scheduler tick can all see the same live free-space number and all
    // pick the same drive before any of them has written a byte — the flat
    // 20 GiB threshold this replaced was only ever sized to be safe for one
    // job at a time.
    const SAFETY_MARGIN_BYTES: u64 = 1024 * 1024 * 1024; // 1 GiB
    let reservation_estimate =
        job_reservation_estimate(&clip, config.target_codec, &config.custom_codec_args);

    // No export folder: refused, never written into the working directory
    // (for an installed app, its install folder), where nobody would look.
    if config.export_directories.is_empty() {
        let _ = tx.send(RenderUpdate::Finished(
            job_id.clone(),
            false,
            Some(NO_EXPORT_FOLDER.to_string()),
        ));
        return;
    }
    let mut selected_export_dir: Option<PathBuf> = None;
    {
        let mut ledger = reservations.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((dir, already_reserved, live_free)) = try_reserve(
            &mut ledger,
            &config.export_directories,
            reservation_estimate,
            SAFETY_MARGIN_BYTES,
            crate::sys::disk::get_available_bytes,
        ) {
            // Shows this job's own estimate against what else was already
            // reserved on this drive *before* this job's own addition, so
            // concurrent jobs' numbers are directly comparable in the log —
            // this is what made it possible to confirm live, under a real
            // concurrent batch, that the ledger correctly reflects other
            // in-flight jobs' reservations rather than every job seeing the
            // same live free-space number.
            crate::log_markdown(&format!(
                "[render-reservation] job {} reserved {:.2} GB on {:?} (already had {:.2} GB reserved by other jobs, {:.2} GB live free)",
                job_id,
                reservation_estimate as f64 / (1024.0 * 1024.0 * 1024.0),
                dir,
                already_reserved as f64 / (1024.0 * 1024.0 * 1024.0),
                live_free as f64 / (1024.0 * 1024.0 * 1024.0),
            ));
            selected_export_dir = Some(dir);
        }
    }
    let Some(output_folder) = selected_export_dir.clone() else {
        let _ = tx.send(RenderUpdate::Finished(
            job_id.clone(),
            false,
            Some(format!(
                "Not rendered: every export folder ({}) has less than {:.1} GB free once the other renders running now are counted. Free some space or add another export folder in Configuration → Render Settings.",
                config.export_directories.len(),
                (reservation_estimate + SAFETY_MARGIN_BYTES) as f64 / (1024.0 * 1024.0 * 1024.0)
            )),
        ));
        return;
    };
    // Releases this job's reservation on every remaining exit path below —
    // see `ReservationGuard`'s own doc comment.
    let _reservation_guard = selected_export_dir.as_ref().map(|dir| ReservationGuard {
        ledger: reservations.clone(),
        dir: ledger_key(dir),
        amount: reservation_estimate,
        job_id: job_id.clone(),
    });

    if let Err(e) = std::fs::create_dir_all(&output_folder) {
        let _ = tx.send(RenderUpdate::Finished(
            job_id.clone(),
            false,
            Some(format!("Failed to create output folder: {}", e)),
        ));
        return;
    }

    // Naming pieces shared by both the FFmpeg render path and the skip/copy
    // path below, so a skipped take and a rendered one land under the same
    // scheme — "where do I find my finished clips" stays answered the same
    // way regardless of which option a take took.
    let stream_type = if is_hud { "hud" } else { "all" };
    let wav_part = match &clip.wav_file {
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
        // An OBS take has no wav to derive a suffix from — base_name already
        // carries the distinguishing "-obs" suffix the scanner gave it.
        None => String::new(),
    };
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros();
    let hash_str = format!("{:04x}", timestamp % 0x10000);
    // Reuses `take_folder` (already parsed from this same `clip.take_folder`
    // string above) rather than re-parsing a second `PathBuf` from it.
    let take_name = take_folder
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let demo_name = take_folder
        .parent()
        .and_then(|p| p.file_name())
        .unwrap_or_default()
        .to_string_lossy();

    if is_plain_copy {
        // Validated above: video_file and a non-hud clip_type are guaranteed here.
        let video_name = clip.video_file.as_deref().unwrap_or_default();
        let source_video = take_folder.join(&clip.img_folder).join(video_name);
        if !source_video.is_file() {
            let _ = tx.send(RenderUpdate::Finished(
                job_id.clone(),
                false,
                Some(format!(
                    "Source video not found: {}",
                    source_video.display()
                )),
            ));
            return;
        }
        // Kept from whatever OBS wrote — renaming would make the file lie
        // about its own container to every tool that reads it afterwards.
        let ext = source_video
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_else(|| "mp4".to_string());
        let final_name = match clip_file_stem(&clip, &wav_part, is_hud) {
            Some(stem) => get_unique_filename(&output_folder, &stem, &format!(".{ext}")),
            None => format!(
                "{}_{}{}_{}_{}.{}",
                demo_name, take_name, wav_part, stream_type, hash_str, ext
            ),
        };
        let out_file = output_folder.join(&final_name);
        let out_file_str = out_file.to_string_lossy().into_owned();

        if cancel_rx.load(Ordering::Relaxed) {
            let _ = tx.send(RenderUpdate::Status(
                job_id.clone(),
                "Cancelled".to_string(),
            ));
            let _ = tx.send(RenderUpdate::Finished(
                job_id,
                false,
                Some("Cancelled by user".to_string()),
            ));
            return;
        }
        let _ = tx.send(RenderUpdate::Status(
            job_id.clone(),
            "Rendering".to_string(),
        ));
        // Chunked rather than `tokio::fs::copy`, so Cancel actually lands
        // during a large copy (Custom Output/lossless OBS captures — see
        // docs/archive/obs_alternate_capture_design.md — can run tens of GB) instead of
        // being silently ignored until the whole file has already moved.
        match copy_cancellable(&source_video, &out_file, &cancel_rx).await {
            Ok(true) => {
                let _ = tx.send(RenderUpdate::Status(
                    job_id.clone(),
                    "Cancelled".to_string(),
                ));
                let _ = tx.send(RenderUpdate::Finished(
                    job_id,
                    false,
                    Some("Cancelled by user".to_string()),
                ));
            }
            Ok(false) => {
                let _ = tx.send(RenderUpdate::Status(job_id.clone(), "Finished".to_string()));
                let _ = tx.send(RenderUpdate::Progress(job_id.clone(), 100));
                let _ = tx.send(RenderUpdate::OutputPath(job_id.clone(), out_file_str));
                let _ = tx.send(RenderUpdate::Finished(job_id, true, None));
            }
            Err(e) => {
                let _ = tx.send(RenderUpdate::Status(job_id.clone(), "Error".to_string()));
                let _ = tx.send(RenderUpdate::Finished(
                    job_id,
                    false,
                    Some(format!("Failed to copy source video to export pool: {}", e)),
                ));
            }
        }
        return;
    }

    // Extension decided in the same match as the codec args, not looked up
    // separately, so the two can't silently drift apart the way the old
    // string-keyed get_codec_preset() already had (it matched "h264" for
    // .mp4, but RenderCodec::NvencH264 has always mapped to the distinct
    // "h264_nvenc" id — dead code, removed, never actually wired to a
    // render). Alpha/HUD output always needs a MOV container regardless of
    // the selected codec (QuickTime-style alpha), matching dev's own
    // behaviour and the dropdown's "MP4" label applying only to the
    // non-alpha H.264 variants.
    // Not `'static`: the Custom arm below borrows from `config.custom_codec_args`,
    // an owned `String` that lives only as long as this function call.
    let mut codec_args: Vec<&str> = Vec::new();
    let trimmed_copy_args = if is_source_copy {
        trimmed_copy_video_args(&config.ffmpeg_path).await
    } else {
        &[]
    };
    let file_ext = if is_hud {
        codec_args.extend_from_slice(&[
            "-c:v",
            "prores_ks",
            "-profile:v",
            "4444",
            "-pix_fmt",
            "yuva444p10le",
        ]);
        ".mov"
    } else {
        match config.target_codec {
            super::config::RenderCodec::NvencH264 => {
                codec_args.extend_from_slice(&[
                    "-c:v",
                    "h264_nvenc",
                    "-preset",
                    "p6",
                    "-tune",
                    "hq",
                    "-cq",
                    "15",
                    "-pix_fmt",
                    "yuv420p",
                ]);
                ".mp4"
            }
            super::config::RenderCodec::H264Software => {
                codec_args.extend_from_slice(&[
                    "-c:v", "libx264", "-preset", "fast", "-crf", "16", "-pix_fmt", "yuv420p",
                ]);
                ".mp4"
            }
            codec @ (super::config::RenderCodec::ProRes
            | super::config::RenderCodec::ProRes422
            | super::config::RenderCodec::ProResLt
            | super::config::RenderCodec::ProRes4444) => {
                codec_args.extend(prores_args(codec));
                ".mov"
            }
            super::config::RenderCodec::DnxHr => {
                codec_args.extend_from_slice(&[
                    "-c:v",
                    "dnxhd",
                    "-profile:v",
                    "dnxhr_hq",
                    "-pix_fmt",
                    "yuv422p",
                ]);
                ".mov"
            }
            super::config::RenderCodec::HuffYuv => {
                codec_args.extend_from_slice(&["-c:v", "huffyuv", "-pix_fmt", "yuv422p"]);
                ".avi"
            }
            super::config::RenderCodec::Uncompressed => {
                codec_args.extend_from_slice(&["-c:v", "rawvideo", "-pix_fmt", "yuv422p"]);
                ".avi"
            }
            // No allow-list, no parsing beyond the whitespace split --
            // FFmpeg itself is what validates this, exactly as it would a
            // codec typed directly at a terminal. `config` is owned by this
            // call and lives for the rest of the function, so these `&str`
            // slices staying borrowed from it (rather than `'static`, like
            // every other arm's literals) is fine.
            super::config::RenderCodec::Custom => {
                if config.custom_codec_args.trim().is_empty() {
                    let _ = tx.send(RenderUpdate::Finished(
                        job_id.clone(),
                        false,
                        Some(
                            "Custom codec selected but no FFmpeg arguments were entered."
                                .to_string(),
                        ),
                    ));
                    return;
                }
                codec_args.extend(config.custom_codec_args.split_whitespace());
                ".mkv"
            }
            // `is_source_copy` already returned before this match — it has
            // its own extension and never runs an FFmpeg encode at all. Not
            // `unreachable!()`: release builds run with `panic = "abort"`
            // (see `obs::session::ObsSession::stop_handle`'s doc comment), so
            // a panic here would kill the whole render batch, not just this
            // job, if some future edit ever broke that invariant. Failing
            // loudly through the normal update channel costs nothing today
            // and is safe if it ever turns out to be reachable.
            // Keep as captured on a take with a trim: the cut can't be a
            // stream copy (`trimmed_copy_video_args`).
            super::config::RenderCodec::SourceCopy if obs_trim.is_some() => {
                codec_args.extend_from_slice(trimmed_copy_args);
                ".mp4"
            }
            super::config::RenderCodec::SourceCopy => {
                let _ = tx.send(RenderUpdate::Finished(
                    job_id.clone(),
                    false,
                    Some("Internal error: SourceCopy reached the FFmpeg codec path.".to_string()),
                ));
                return;
            }
        }
    };

    // MP4 has never had solid cross-player support for raw PCM audio — ffmpeg's
    // MP4 muxer boxes it as `ipcm`, which FFmpeg/VLC read fine but which Vegas
    // Pro (and likely other strict NLEs) can't decode, producing garbled/static
    // audio despite the samples themselves being intact (confirmed: the exact
    // same PCM bytes remuxed into a .mov container instead, where ffmpeg uses
    // the older/broadly-supported `sowt` tag, play back correctly everywhere).
    // AAC sidesteps the whole box-tag mess and is the standard choice for MP4
    // audio; every other codec here (ProRes/DNxHD's .mov, HuffYUV/rawvideo's
    // .avi) stays lossless PCM since none of them are .mp4.
    let audio_codec_args: &[&str] = if is_source_copy {
        // OBS's own AAC, re-encoded once, at a rate that loses nothing audible.
        &["-c:a", "aac", "-b:a", "320k"]
    } else if file_ext == ".mp4" {
        &["-c:a", "aac", "-b:a", "192k"]
    } else {
        &["-c:a", "pcm_s16le"]
    };

    let final_name = match clip_file_stem(&clip, &wav_part, is_hud) {
        Some(stem) => get_unique_filename(&output_folder, &stem, file_ext),
        None => format!(
            "{}_{}{}_{}_{}{}",
            demo_name, take_name, wav_part, stream_type, hash_str, file_ext
        ),
    };
    let out_file = output_folder.join(&final_name);

    // Calculate thread scaling
    let max_concurrent = config.max_concurrent_renders;
    let threads_per_process = match std::thread::available_parallelism() {
        Ok(val) => std::cmp::max(1, val.get() / max_concurrent),
        Err(_) => 2,
    };

    // The alpha half of a HUD pair, as the scanner recorded it. Older autosaves
    // predate the field, so fall back to the literal the renderer used to
    // hardcode — that is exactly what those takes were scanned as.
    let alpha_folder = clip.alpha_folder.as_deref().unwrap_or("hudalpha");

    let mut cmd_args = vec!["-y", "-hide_banner"];
    let img_input: String;
    // Inputs for a take captured through `mirv_movie_ffmpeg`: one video per
    // stream folder instead of a numbered frame sequence.
    let video_input: String;
    // Shared by both HUD branches — one holds a video path, the other a BMP
    // sequence pattern, and the branches are mutually exclusive.
    let hud_color_input: String;
    let hud_alpha_input: String;
    // An OBS take's cut (#84), as input options.
    let trim_seek: String;
    let trim_length: String;

    // `-framerate` is deliberately absent for these. It tells FFmpeg how to
    // interpret an untimed image sequence; a video carries its own timing, and
    // forcing a rate onto it would re-time the clip. That also means a video
    // take cannot suffer the FPS mismatch that BMP takes can — there is no
    // second number to disagree with.
    if let Some(video) = clip.video_file.as_deref() {
        if clip_type == "hud_only" {
            let wav = clip
                .wav_file
                .as_deref()
                .expect("validated above: HUD clips require a wav");
            hud_color_input = format!("{}/{}", clip.img_folder, video);
            hud_alpha_input = format!("{}/{}", alpha_folder, video);
            cmd_args.extend(vec![
                "-i",
                &hud_color_input,
                "-i",
                &hud_alpha_input,
                "-thread_queue_size",
                "512",
                "-i",
                wav,
                "-filter_complex",
                "[1:v]extractplanes=r[alpha];[0:v][alpha]alphamerge[hud]",
                "-map",
                "[hud]",
                "-map",
                "2:a",
            ]);
        } else if let Some(wav) = clip.wav_file.as_deref() {
            video_input = format!("{}/{}", clip.img_folder, video);
            cmd_args.extend(vec![
                "-i",
                &video_input,
                "-thread_queue_size",
                "512",
                "-i",
                wav,
            ]);
        } else {
            // An OBS take: the video already carries its own audio stream, so
            // there is nothing to mux against. A single input with no `-map`
            // needed — FFmpeg's default stream selection already picks one
            // video and one audio stream out of it.
            video_input = format!("{}/{}", clip.img_folder, video);
            if let Some(trim) = obs_trim {
                // Before `-i`: FFmpeg's accurate seek, frame-exact on a
                // re-encode. `-t` as an input option counts from the seek.
                trim_seek = format!("{:.4}", trim.seek_seconds());
                cmd_args.extend(["-ss", trim_seek.as_str()]);
                if let Some(length) = trim.length_seconds() {
                    trim_length = format!("{length:.4}");
                    cmd_args.extend(["-t", trim_length.as_str()]);
                }
                crate::log_markdown(&format!(
                    "[render] job {} — cutting the OBS take to its clip: from {:.3}s{} (#84)",
                    job_id,
                    trim.start_seconds,
                    trim.length_seconds()
                        .map(|l| format!(", {l:.3}s long"))
                        .unwrap_or_else(|| " to the end of the file".to_string())
                ));
            }
            cmd_args.extend(vec!["-i", &video_input]);
        }
    } else if clip_type == "hud_only" {
        let wav = clip
            .wav_file
            .as_deref()
            .expect("validated above: HUD clips require a wav");
        hud_color_input = format!("{}/%05d.bmp", clip.img_folder);
        hud_alpha_input = format!("{}/%05d.bmp", alpha_folder);
        cmd_args.extend(vec![
            // Skip probe/analyze on known BMP sequences; add read-ahead buffering.
            "-probesize",
            "32",
            "-analyzeduration",
            "0",
            "-thread_queue_size",
            "512",
            "-framerate",
            &fps,
            "-i",
            &hud_color_input,
            "-probesize",
            "32",
            "-analyzeduration",
            "0",
            "-thread_queue_size",
            "512",
            "-framerate",
            &fps,
            "-i",
            &hud_alpha_input,
            "-thread_queue_size",
            "512",
            "-i",
            wav,
            "-filter_complex",
            "[1:v]extractplanes=r[alpha];[0:v][alpha]alphamerge[hud]",
            "-map",
            "[hud]",
            "-map",
            "2:a",
        ]);
    } else {
        // BMP shape is never admitted without a wav — see `take_shape_is_renderable`.
        let wav = clip
            .wav_file
            .as_deref()
            .expect("validated above: BMP takes require a wav");
        img_input = format!("{}/%05d.bmp", clip.img_folder);
        cmd_args.extend(vec![
            // Skip probe/analyze on known BMP sequences; add read-ahead buffering.
            "-probesize",
            "32",
            "-analyzeduration",
            "0",
            "-thread_queue_size",
            "512",
            "-framerate",
            &fps,
            "-i",
            &img_input,
            "-thread_queue_size",
            "512",
            "-i",
            wav,
        ]);
    }

    cmd_args.extend(codec_args);
    let threads_str = threads_per_process.to_string();
    let out_file_str = out_file.to_string_lossy().into_owned();

    cmd_args.extend(vec!["-threads", &threads_str]);
    cmd_args.extend_from_slice(audio_codec_args);
    cmd_args.extend(vec![
        "-shortest",
        // +faststart is only needed for HTTP streaming; omitting it avoids the
        // post-render moov-atom rewrite pass on what can be multi-GB files.
        "-progress",
        "pipe:1",
        "-loglevel",
        "error",
        &out_file_str,
    ]);

    let mut cmd = Command::new(ffmpeg_path);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);
    cmd.kill_on_drop(true);
    cmd.args(cmd_args);
    cmd.current_dir(&take_folder);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let _ = tx.send(RenderUpdate::Status(
        job_id.clone(),
        "Rendering".to_string(),
    ));

    let existed_before = out_file.exists();
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let error_msg = if e.kind() == std::io::ErrorKind::NotFound {
                "FFmpeg not found. Please install FFmpeg or set a custom path in Settings."
                    .to_string()
            } else {
                format!("Failed to spawn FFmpeg process: {}", e)
            };
            let _ = tx.send(RenderUpdate::Finished(
                job_id.clone(),
                false,
                Some(error_msg),
            ));
            return;
        }
    };

    // Spawn stderr reader task to prevent deadlock
    let mut stderr_handle = child.stderr.take().unwrap();
    let stderr_log = Arc::new(Mutex::new(String::new()));
    let stderr_log_clone = Arc::clone(&stderr_log);

    tokio::spawn(async move {
        let mut buf = vec![0u8; 1024];
        while let Ok(n) = stderr_handle.read(&mut buf).await {
            if n == 0 {
                break;
            }
            if let Ok(s) = std::str::from_utf8(&buf[..n])
                && let Ok(mut log) = stderr_log_clone.lock()
            {
                log.push_str(s);
            }
        }
    });

    // Read stdout for progress updates
    let stdout_handle = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout_handle);
    let mut line = String::new();

    // A video take has no frame count to measure against; a trimmed OBS take
    // knows its length, so its progress is the time written against that.
    let expected_seconds = obs_trim.and_then(|t| t.length_seconds());
    let mut current_fps = "0".to_string();
    let mut current_speed = "0x".to_string();

    let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(200));

    loop {
        line.clear();
        tokio::select! {
            _ = interval.tick() => {
                if cancel_rx.load(Ordering::Relaxed) {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    remove_partial_output(&out_file, existed_before).await;
                    let _ = tx.send(RenderUpdate::Status(job_id.clone(), "Cancelled".to_string()));
                    let _ = tx.send(RenderUpdate::Finished(
                        job_id.clone(),
                        false,
                        Some("Cancelled by user".to_string()),
                    ));
                    return;
                }
            }
            read_res = reader.read_line(&mut line) => {
                match read_res {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        if let Some(pos) = trimmed.find('=') {
                            let key = trimmed[..pos].trim();
                            let val = trimmed[pos + 1..].trim();

                            match key {
                                "frame" => {
                                    if let Ok(current_frame) = val.parse::<usize>()
                                        && let Some(ratio) =
                                            (current_frame * 100).checked_div(clip.frame_count)
                                    {
                                        let percent = std::cmp::min(100, ratio) as u32;
                                        let _ = tx.send(RenderUpdate::Progress(job_id.clone(), percent));
                                    }
                                }
                                "out_time_us" if clip.frame_count == 0 => {
                                    if let (Some(total), Ok(us)) = (expected_seconds, val.parse::<i64>()) {
                                        let percent = progress_percent(us as f64 / 1e6, total);
                                        let _ = tx.send(RenderUpdate::Progress(job_id.clone(), percent));
                                    }
                                }
                                "fps" => {
                                    current_fps = val.to_string();
                                    let _ = tx.send(RenderUpdate::Speed(
                                        job_id.clone(),
                                        format!("{} fps ({})", current_fps, current_speed),
                                    ));
                                }
                                "speed" => {
                                    current_speed = val.to_string();
                                    let _ = tx.send(RenderUpdate::Speed(
                                        job_id.clone(),
                                        format!("{} fps ({})", current_fps, current_speed),
                                    ));
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }

    // Wait for the process to exit with cancellation check
    let exit_status = loop {
        tokio::select! {
            res = child.wait() => {
                break res;
            }
            _ = interval.tick() => {
                if cancel_rx.load(Ordering::Relaxed) {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    remove_partial_output(&out_file, existed_before).await;
                    let _ = tx.send(RenderUpdate::Status(job_id.clone(), "Cancelled".to_string()));
                    let _ = tx.send(RenderUpdate::Finished(
                        job_id.clone(),
                        false,
                        Some("Cancelled by user".to_string()),
                    ));
                    return;
                }
            }
        }
    };

    let err_log = if let Ok(log) = stderr_log.lock() {
        if log.trim().is_empty() {
            None
        } else {
            Some(log.clone())
        }
    } else {
        None
    };

    match exit_status {
        Ok(status) if status.success() => {
            if let Ok(meta) = std::fs::metadata(&out_file_str) {
                record_output_size(
                    &clip,
                    config.target_codec,
                    &config.custom_codec_args,
                    meta.len(),
                );
            }
            let _ = tx.send(RenderUpdate::Status(job_id.clone(), "Finished".to_string()));
            let _ = tx.send(RenderUpdate::Progress(job_id.clone(), 100));
            let _ = tx.send(RenderUpdate::OutputPath(
                job_id.clone(),
                out_file_str.clone(),
            ));
            let _ = tx.send(RenderUpdate::Finished(job_id, true, None));
        }
        Ok(status) => {
            let exit_code = status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            let error_msg = format!(
                "FFmpeg failed with exit code: {}. Log: {}",
                exit_code,
                err_log.as_deref().unwrap_or("")
            );
            remove_partial_output(&out_file, existed_before).await;
            let _ = tx.send(RenderUpdate::Status(job_id.clone(), "Error".to_string()));
            let _ = tx.send(RenderUpdate::Finished(job_id, false, Some(error_msg)));
        }
        Err(e) => {
            let error_msg = format!("Failed to wait for FFmpeg: {}", e);
            let _ = tx.send(RenderUpdate::Status(job_id.clone(), "Error".to_string()));
            let _ = tx.send(RenderUpdate::Finished(job_id, false, Some(error_msg)));
        }
    }
}

/// How far along a render is, in whole percent, from the seconds written so
/// far against the seconds expected.
fn progress_percent(written: f64, total: f64) -> u32 {
    if !(total > 0.0) || !written.is_finite() {
        return 0;
    }
    (written / total * 100.0).clamp(0.0, 100.0) as u32
}

/// The same quality OBS records at in Studio's profile (CQP 16), on NVENC.
const TRIMMED_COPY_NVENC: &[&str] = &[
    "-c:v",
    "h264_nvenc",
    "-preset",
    "p5",
    "-tune",
    "hq",
    "-rc",
    "constqp",
    "-qp",
    "16",
    "-pix_fmt",
    "yuv420p",
];

/// The fallback without an NVIDIA encoder: about as fast here, smaller, a
/// little softer (`docs/obs_alternate_capture.md`).
const TRIMMED_COPY_X264: &[&str] = &[
    "-c:v", "libx264", "-preset", "fast", "-crf", "14", "-pix_fmt", "yuv420p",
];

/// The video encode for Keep as captured on an OBS take with a trim (#84).
///
/// A stream copy can only cut on a keyframe, and OBS's Simple output puts one
/// every 250 frames (about 4 s at 60 fps, measured), against a head of about
/// 0.9 s: a copy would keep the whole head. The keyframe interval can't be set
/// over obs-websocket in Simple mode, and splicing a re-encoded first GOP onto
/// copied packets breaks on the encoders' differing parameter sets. So the cut
/// is a re-encode at OBS's own quality, NVENC when FFmpeg can open it, which
/// measured about as fast as the copy's disk time for a 20 s clip.
///
/// Probed once per FFmpeg path, with a one-frame encode.
async fn trimmed_copy_video_args(ffmpeg: &str) -> &'static [&'static str] {
    static NVENC_WORKS: std::sync::OnceLock<Mutex<HashMap<String, bool>>> =
        std::sync::OnceLock::new();
    let cache = NVENC_WORKS.get_or_init(|| Mutex::new(HashMap::new()));
    let known = cache.lock().ok().and_then(|c| c.get(ffmpeg).copied());
    let works = match known {
        Some(works) => works,
        None => {
            let works = encoder_opens(ffmpeg, "h264_nvenc").await;
            if let Ok(mut c) = cache.lock() {
                c.insert(ffmpeg.to_string(), works);
            }
            works
        }
    };
    if works {
        TRIMMED_COPY_NVENC
    } else {
        TRIMMED_COPY_X264
    }
}

/// Whether FFmpeg can open `encoder` on this machine: one black frame,
/// thrown away. A build listing NVENC without an NVIDIA GPU fails here.
async fn encoder_opens(ffmpeg: &str, encoder: &str) -> bool {
    let mut cmd = Command::new(ffmpeg);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);
    cmd.kill_on_drop(true);
    cmd.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "color=c=black:s=256x256:r=60",
        "-frames:v",
        "1",
        "-c:v",
        encoder,
        "-f",
        "null",
        "-",
    ]);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());
    let Ok(mut child) = cmd.spawn() else {
        return false;
    };
    // Dropping the child on a timeout kills it (`kill_on_drop`).
    matches!(
        tokio::time::timeout(std::time::Duration::from_secs(15), child.wait()).await,
        Ok(Ok(status)) if status.success()
    )
}

/// The main picture's `-c:v` args for a ProRes choice; empty for any other
/// codec. The HUD stream never comes here: it is always 4444 with alpha.
fn prores_args(codec: RenderCodec) -> Vec<&'static str> {
    match codec.prores_profile() {
        Some((profile, pix_fmt, _)) => vec![
            "-c:v",
            "prores_ks",
            "-profile:v",
            profile,
            "-pix_fmt",
            pix_fmt,
        ],
        None => Vec::new(),
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
    // 2 MiB, matching this codebase's stated heap-buffer payload limit
    // (CLAUDE.md's Memory Safety rule) — still large enough that per-chunk
    // overhead is negligible against the copy itself, and a cancel lands
    // within a second or two even on a slow disk.
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
        let free = |p: &Path| if p.starts_with(r"C:\") { 15 } else { 12 };
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
        assert_eq!(ledger.get(Path::new("c:")), Some(&10));
        assert_eq!(ledger.get(Path::new("d:")), Some(&10));
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

    #[test]
    fn each_prores_choice_renders_with_its_own_profile() {
        let args = |c: RenderCodec| prores_args(c).join(" ");
        assert_eq!(
            args(RenderCodec::ProRes),
            "-c:v prores_ks -profile:v hq -pix_fmt yuv422p10le"
        );
        assert_eq!(
            args(RenderCodec::ProRes422),
            "-c:v prores_ks -profile:v standard -pix_fmt yuv422p10le"
        );
        assert_eq!(
            args(RenderCodec::ProResLt),
            "-c:v prores_ks -profile:v lt -pix_fmt yuv422p10le"
        );
        assert_eq!(
            args(RenderCodec::ProRes4444),
            "-c:v prores_ks -profile:v 4444 -pix_fmt yuv444p10le"
        );
        assert_eq!(args(RenderCodec::DnxHr), "");
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
}
