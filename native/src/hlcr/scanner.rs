use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use walkdir::WalkDir;

mod media_probe;
use media_probe::{avi_dimensions, avi_frame_count, read_bmp_dimensions, video_has_audio};

/// An AVI's frame rate from its header, `(rate, scale)`: see
/// `media_probe::avi_frame_rate`. For the renderer's segmented renders
/// (#1451).
pub(crate) fn avi_frame_rate(path: &Path) -> Option<(u32, u32)> {
    media_probe::avi_frame_rate(path)
}

/// An AVI's frame count from its header: see `media_probe::avi_frame_count`.
/// For checking a segmented render's joined clip (#1451).
pub(crate) fn avi_video_frames(path: &Path) -> Option<usize> {
    avi_frame_count(path)
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct ClipData {
    pub take_folder: String,
    pub clip_type: String, // "single" or "hud_only"
    pub img_folder: String,
    /// The wav to mux in, relative to `take_folder`. `None` for an OBS take:
    /// its audio is already muxed into `video_file`, so there is nothing to
    /// mux — `run_render_job` branches on this rather than trusting a wav is
    /// always there.
    pub wav_file: Option<String>,
    pub base_name: String,
    pub frame_count: usize,
    /// Frame resolution — `0` means unknown (e.g. an autosave-recovered stub
    /// that never had a real scan, or a stream whose dimensions couldn't be
    /// read). Used by `run_render_job`'s JIT drive-reservation estimate, not
    /// by the encode itself (FFmpeg reads the actual frame dimensions
    /// straight off the source). `#[serde(default)]` so autosaves written
    /// before this existed keep loading.
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    pub date: String,
    /// The video file inside `img_folder`, when the take was captured through
    /// `mirv_movie_ffmpeg` instead of as a frame sequence. `None` is a BMP take.
    ///
    /// `#[serde(default)]` because render autosaves written before this existed
    /// have to keep loading — a recovered batch must not fail on a field that
    /// was not there when it was saved.
    #[serde(default)]
    pub video_file: Option<String>,
    /// For a `hud_only` clip, the stream folder holding the alpha half of the
    /// pair — `img_folder` is the colour half. `None` on every other clip type.
    ///
    /// The renderer used to derive this by appending the literal `hudalpha`,
    /// which only works while HLAE spells the folder exactly that way. Carrying
    /// the name the scanner actually saw on disk means the renderer is told its
    /// partner rather than guessing it, and is the one piece of the standalone
    /// HLCR's design (see `docs/archive/render_studio_hlcr_parity.md`) that does not
    /// depend on frame counts.
    ///
    /// `#[serde(default)]` for the same reason as `video_file`: autosaves
    /// written before this field existed must keep loading.
    #[serde(default)]
    pub alpha_folder: Option<String>,
    /// The highlight's clip name (#441), which the finished file is named
    /// after. Set by Render Studio's queue from the project; `None` for a take
    /// no loaded project knows, which keeps the `<demo>_<take>` name.
    #[serde(default)]
    pub clip_name: Option<String>,
}

/// Whether a clip is eligible for "Skip (keep original)" — no FFmpeg pass at
/// all, just a copy of `video_file` into the export pool.
///
/// Shared between the Tauri layer's per-job toggle gate
/// (`studio/src-tauri/src/render_manager.rs`'s `skip_available`) and
/// `run_render_job`'s own admission check, so the two can never quietly drift
/// apart about which clips qualify — the same reason `is_renderable_take` is
/// shared between the capture and render sides.
pub fn clip_is_skip_eligible(clip: &ClipData) -> bool {
    clip.clip_type != "hud_only" && clip.wav_file.is_none() && clip.video_file.is_some()
}

/// The file `mirv_movie_ffmpeg` is told to write, per stream. Also what the
/// scanner looks for to tell a video take from a frame sequence, so the two
/// cannot drift apart.
pub const VIDEO_FILE: &str = "video.avi";

/// `.wav` files sitting directly in a take folder, sorted case-insensitively
/// so take selection is deterministic.
fn collect_wav_files(take_folder: &Path) -> Vec<String> {
    let mut wav_files = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(take_folder) {
        for sub_entry in read_dir.flatten() {
            if let Ok(file_type) = sub_entry.file_type()
                && file_type.is_file()
            {
                let path = sub_entry.path();
                if let Some(ext) = path.extension()
                    && ext.to_string_lossy().to_lowercase() == "wav"
                    && let Some(name) = path.file_name()
                {
                    wav_files.push(name.to_string_lossy().into_owned());
                }
            }
        }
    }
    wav_files.sort_by_key(|a| a.to_lowercase());
    wav_files
}

/// Immediate subdirectories of a take folder that hold one capture stream —
/// either an HLAE frame sequence, identified by its `00000.bmp` first frame, or
/// a single video written by `mirv_movie_ffmpeg`.
///
/// Both land in the same place: `<take0000>/<stream>/`, where `<stream>` is
/// `all`, `hudcolor`, `hudalpha` and so on. Measured against a real capture —
/// FFmpeg mode changes what is inside the stream folder and nothing else about
/// the layout.
fn collect_image_folders(take_folder: &Path) -> Vec<PathBuf> {
    let mut image_folders = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(take_folder) {
        for sub_entry in read_dir.flatten() {
            if let Ok(file_type) = sub_entry.file_type() {
                let path = sub_entry.path();
                if file_type.is_dir()
                    && (path.join("00000.bmp").exists() || stream_video_path(&path).is_some())
                {
                    image_folders.push(path);
                }
            }
        }
    }
    image_folders
}

/// Containers a stream folder's video may be in.
///
/// `mirv_movie_ffmpeg` writes `video.avi` because that is what the pipeline
/// asks it for. OBS writes whatever container the user's OBS settings name —
/// the install this was developed against defaults to `hybrid_mp4` — and its
/// output keeps the extension it was written with rather than being renamed,
/// since a file that lies about its own container breaks every tool that reads
/// it afterwards.
const VIDEO_EXTENSIONS: &[&str] = &["avi", "mp4", "mkv", "mov"];

/// The video inside a stream folder, whatever container it is in.
pub(crate) fn stream_video_path(folder: &Path) -> Option<PathBuf> {
    // `video.avi` first so an HLAE take resolves without touching the disk
    // more than once — it is by far the common case.
    let preferred = folder.join(VIDEO_FILE);
    if preferred.is_file() {
        return Some(preferred);
    }
    VIDEO_EXTENSIONS
        .iter()
        .map(|ext| folder.join(format!("video.{ext}")))
        .find(|p| p.is_file())
}

/// A stream folder's name exactly as it appears on disk. Paths on the clip are
/// resolved relative to the take folder, so the case only has to survive to
/// keep the record honest — but a name the scanner saw beats one reconstructed
/// from a literal, especially with two spellings in play.
fn on_disk_name(folder: &Path) -> String {
    folder
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// A stream folder's frame resolution, whichever shape it was captured in —
/// mirrors `stream_frame_count`'s own AVI-vs-BMP branch. `(0, 0)` means
/// unknown (e.g. an OBS MP4/MKV take, whose container walk isn't worth doing
/// just for this — see `stream_frame_count`'s own reasoning); callers treat
/// that as "fall back to a conservative estimate" rather than a hard error.
fn stream_dimensions(folder: &Path) -> (u32, u32) {
    if let Some(video) = stream_video_path(folder) {
        return avi_dimensions(&video).unwrap_or((0, 0));
    }
    read_bmp_dimensions(folder).unwrap_or((0, 0))
}

/// Frames in a stream folder, whichever shape it was captured in.
fn stream_frame_count(folder: &Path) -> usize {
    if let Some(video) = stream_video_path(folder) {
        // Only AVI carries the count somewhere this cheap to reach. An
        // OBS-written MP4 or MKV keeps it behind a container walk that is not
        // worth doing for a progress bar, so 0 stands — which the UI shows as
        // an indeterminate bar rather than as a wrong number.
        return avi_frame_count(&video).unwrap_or(0);
    }
    count_bmps(folder)
}

/// The video inside a stream folder, when that is what it holds.
fn stream_video(folder: &Path) -> Option<String> {
    stream_video_path(folder).and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
}

/// Whether Render Studio's scanner would admit this folder as a renderable take.
///
/// Shared with the capture-side take verification so "the capture succeeded"
/// and "Render Studio can actually see it" can never silently disagree — if
/// this predicate changes, both sides change together.
///
/// HLAE's `mirv_movie` plugin auto-numbers each recording into a `take0000`,
/// `take0001`, ... subfolder under whatever directory `mirv_movie_filename`
/// points at, to avoid overwriting a previous take written to the same path —
/// so the wav/bmp sequence actually lands one level deeper than the folder we
/// asked it to write to. `scan_folder_background` below dodges this for free
/// because `WalkDir` recurses into every subdirectory on its own; this check
/// has to look explicitly since it only tests one specific folder.
pub fn is_renderable_take(take_folder: &Path) -> bool {
    if take_shape_is_renderable(take_folder) {
        return true;
    }
    if let Ok(read_dir) = std::fs::read_dir(take_folder) {
        for entry in read_dir.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false)
                && entry
                    .file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .starts_with("take")
                && take_shape_is_renderable(&entry.path())
            {
                return true;
            }
        }
    }
    false
}

/// Whether this exact folder is what the scan hands out as a clip's
/// `take_folder`: it holds the streams itself, not through a nested `take*`.
/// Delete Take (#644) asks this, so it can never act on a block or session
/// folder that merely contains a take.
pub fn is_take_folder(folder: &Path) -> bool {
    take_shape_is_renderable(folder)
}

/// Frames the longest stream directly inside `folder` holds (one take
/// folder, not a block folder with HLAE's `take*` inside): an AVI's from its
/// header, a frame sequence's by its files. `None` when no stream there can
/// be counted this cheaply (an OBS MP4/MKV, an AGR take, no stream at all),
/// so an unknown length is never taken for a short one (#1105). An AVI whose
/// header says 0 frames was never finalised and counts as 0.
pub fn take_frame_count(folder: &Path) -> Option<usize> {
    collect_image_folders(folder)
        .iter()
        .filter_map(|stream| match stream_video_path(stream) {
            Some(video)
                if video
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("avi")) =>
            {
                Some(avi_frame_count(&video).unwrap_or(0))
            }
            Some(_) => None,
            None => Some(count_bmps(stream)),
        })
        .max()
}

/// The audio HLAE wrote beside the streams in one take folder (#1105): the
/// first `.wav` by name, as the render pass picks it.
pub fn take_wav(folder: &Path) -> Option<PathBuf> {
    collect_wav_files(folder)
        .first()
        .map(|name| folder.join(name))
}

/// The two admissible shapes, checked against one specific folder.
///
/// 1. **Audio beside a stream folder.** How HLAE writes a take, whether that
///    stream folder holds a BMP sequence or a `mirv_movie_ffmpeg` video: the
///    audio is always a separate `sound.wav` that the render pass muxes in.
/// 2. **A stream folder holding a video that already contains its audio.** How
///    an OBS take arrives, because OBS records A/V together and there is no
///    wav to find.
///
/// The second case tests for the audio *stream*, not merely for a video file.
/// A muted OBS source produces a video that is valid in every respect and
/// silent, and admitting it would mean the render pass emits a clip with no
/// sound and no error — exactly the plausible-looking-but-wrong output this
/// pipeline keeps being bitten by. The check is cheap because the answer is in
/// the container header.
fn take_shape_is_renderable(folder: &Path) -> bool {
    shape_is_renderable(&collect_image_folders(folder), &collect_wav_files(folder))
}

/// `take_shape_is_renderable` for a folder whose streams and wavs the caller
/// already listed, so the folder isn't read again.
fn shape_is_renderable(streams: &[PathBuf], wav_files: &[String]) -> bool {
    if streams.is_empty() {
        return false;
    }
    if !wav_files.is_empty() {
        return true;
    }
    streams
        .iter()
        .filter_map(|s| stream_video_path(s))
        .any(|v| video_has_audio(&v))
}

/// A BMP stream folder: its first frame is there. Never a take itself, and
/// the scan doesn't descend into it: listing its thousands of frames as
/// candidate takes was two of the three times each one was read.
fn is_bmp_stream_folder(entry: &walkdir::DirEntry) -> bool {
    entry.file_type().is_dir() && entry.path().join("00000.bmp").exists()
}

pub fn scan_folder_background(
    source_folders: Vec<PathBuf>,
    tx: mpsc::Sender<ClipData>,
    status_tx: mpsc::Sender<String>,
) -> usize {
    let mut count = 0;
    let mut processed_folders = HashSet::new();
    let mut accumulated_clips = Vec::new();

    for source_folder in source_folders {
        if !source_folder.exists() || !source_folder.is_dir() {
            continue;
        }

        // walkdir does not follow links by default; the depth cap is #376's.
        // An output folder can be a drive root, whose Recycle Bin keeps
        // deleted takes under their own names (#843).
        for entry in WalkDir::new(&source_folder)
            .max_depth(crate::sys::folder_walk::Walker::DEFAULT_MAX_DEPTH)
            .into_iter()
            .filter_entry(|e| {
                !is_bmp_stream_folder(e)
                    && !(e.depth() > 0
                        && e.file_type().is_dir()
                        && crate::sys::folder_walk::is_system_folder(e.file_name()))
            })
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_dir() {
                continue;
            }

            let take_folder = entry.path().to_path_buf();
            if processed_folders.contains(&take_folder) {
                continue;
            }

            let mut image_folders = collect_image_folders(&take_folder);
            if image_folders.is_empty() {
                continue;
            }
            let wav_files = collect_wav_files(&take_folder);
            // Shared with the capture-side take-verification predicate so the
            // two can never silently disagree about what counts as a take —
            // admits both the wav-beside-a-stream shape and an OBS take
            // (a stream folder whose video already carries its own audio).
            if !shape_is_renderable(&image_folders, &wav_files) {
                continue;
            }

            // Valid take found!
            processed_folders.insert(take_folder.clone());
            let _ = status_tx.send(format!(
                "Found take: {}",
                take_folder
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            ));

            // Prioritize sound.wav if it exists. `None` means an OBS take —
            // its audio is already muxed into the video, so there is no wav
            // to pick among.
            let sound_wav_exists = wav_files.iter().any(|f| f.to_lowercase() == "sound.wav");
            let wav_to_use = if sound_wav_exists {
                Some("sound.wav".to_string())
            } else {
                wav_files.first().cloned()
            };

            let take_name = take_folder
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let demo_name = take_folder
                .parent()
                .and_then(|p| p.file_name())
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();

            let base_name = match &wav_to_use {
                Some(wav) => {
                    let wav_stem = Path::new(wav)
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    if wav_stem.to_lowercase() == "sound" {
                        format!("{}-{}-{}", demo_name, take_name, wav_stem)
                    } else {
                        wav_stem
                    }
                }
                None => format!("{}-{}-obs", demo_name, take_name),
            };

            // Keyed lowercase. The BMP path produces `all`/`hudcolor`/`hudalpha`,
            // but HLAE names those same streams `hudColor`/`hudAlpha` in the
            // `mirv_movie_ffmpeg` command, and nothing here has proven which
            // spelling FFmpeg mode uses for the folder. An exact-case lookup
            // that guessed wrong would not error — it would quietly skip the
            // HUD bundling and emit two unrelated clips instead of one merged
            // render, which is the kind of wrong output this pipeline is bad at
            // noticing. Windows resolves the paths case-insensitively either
            // way, so only this lookup was ever at risk.
            //
            // `docs/archive/render_studio_hlcr_parity.md` notes the standalone HLCR
            // pairs folders generically instead — any `alpha`/`mask` folder with
            // a same-frame-count `color`/`rgb` one — which is more robust than
            // literal names and remains the better long-term shape.
            let folder_names: HashMap<String, PathBuf> = image_folders
                .iter()
                .map(|p| {
                    let name = p
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_lowercase();
                    (name, p.clone())
                })
                .collect();

            // Bundle HLAE split streams if "all", "hudcolor", and "hudalpha" exist
            if folder_names.contains_key("all")
                && folder_names.contains_key("hudcolor")
                && folder_names.contains_key("hudalpha")
            {
                #[allow(
                    clippy::unwrap_used,
                    clippy::expect_used,
                    reason = "contains_key was checked for all three names just above"
                )]
                let all_folder = folder_names.get("all").unwrap();
                #[allow(
                    clippy::unwrap_used,
                    clippy::expect_used,
                    reason = "contains_key was checked for all three names just above"
                )]
                let hud_color_folder = folder_names.get("hudcolor").unwrap();
                #[allow(
                    clippy::unwrap_used,
                    clippy::expect_used,
                    reason = "contains_key was checked for all three names just above"
                )]
                let hud_alpha_folder = folder_names.get("hudalpha").unwrap();
                let frame_count = stream_frame_count(all_folder);
                let (width, height) = stream_dimensions(all_folder);
                let date = get_clip_date(all_folder);

                // `take_shape_is_renderable` above only asked whether *some*
                // stream in this take has audio, which is enough to admit the
                // take but not enough to admit `all` specifically — the same
                // reasoning the "process remaining folders" loop below applies
                // per stream. And a HUD/alpha composite always needs a real
                // wav regardless: the alpha stream is a mask, never a mix, so
                // `run_render_job` rejects `hud_only` with `wav_file: None`
                // outright — bundling one without a wav would only queue a
                // job that is guaranteed to fail at render time.
                let all_has_audio = wav_to_use.is_some()
                    || stream_video_path(all_folder)
                        .map(|v| video_has_audio(&v))
                        .unwrap_or(false);

                if all_has_audio {
                    let clip_all = ClipData {
                        clip_name: None,
                        take_folder: take_folder.to_string_lossy().into_owned(),
                        clip_type: "single".to_string(),
                        img_folder: on_disk_name(all_folder),
                        wav_file: wav_to_use.clone(),
                        base_name: base_name.clone(),
                        frame_count,
                        width,
                        height,
                        date: date.clone(),
                        video_file: stream_video(all_folder),
                        alpha_folder: None,
                    };
                    accumulated_clips.push(clip_all);

                    if wav_to_use.is_some() {
                        let clip_hud = ClipData {
                            clip_name: None,
                            take_folder: take_folder.to_string_lossy().into_owned(),
                            clip_type: "hud_only".to_string(),
                            // Both halves of the pair are named from what is actually on
                            // disk, so the renderer never has to reconstruct either one
                            // from a literal. The lookup keys above are lowercased for
                            // matching only — HLAE writes `hudColor`/`hudAlpha` in the
                            // `mirv_movie_ffmpeg` command, and these carry whichever
                            // spelling the capture produced.
                            img_folder: on_disk_name(hud_color_folder),
                            wav_file: wav_to_use.clone(),
                            base_name: base_name.clone(),
                            frame_count,
                            width,
                            height,
                            date: date.clone(),
                            video_file: stream_video(hud_color_folder),
                            alpha_folder: Some(on_disk_name(hud_alpha_folder)),
                        };
                        accumulated_clips.push(clip_hud);
                    }
                }

                // Remove bundled folders from list to avoid double-processing
                image_folders.retain(|p| {
                    let name = p
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_lowercase();
                    name != "all" && name != "hudcolor" && name != "hudalpha"
                });
            }

            // Process remaining folders
            for img_folder in image_folders {
                // `take_shape_is_renderable` above only asked whether *some*
                // stream in this take has audio — enough to admit the take,
                // not enough to admit every stream in it. With no take-level
                // wav, a stream whose own video is silent must still be
                // skipped individually, or it would render with no audio and
                // no error: exactly what `video_has_audio` exists to prevent
                // for a lone OBS take.
                if wav_to_use.is_none() {
                    let has_own_audio = stream_video_path(&img_folder)
                        .map(|v| video_has_audio(&v))
                        .unwrap_or(false);
                    if !has_own_audio {
                        continue;
                    }
                }
                let frame_count = stream_frame_count(&img_folder);
                let (width, height) = stream_dimensions(&img_folder);
                let folder_name = img_folder
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let date = get_clip_date(&img_folder);
                let clip = ClipData {
                    clip_name: None,
                    take_folder: take_folder.to_string_lossy().into_owned(),
                    clip_type: "single".to_string(),
                    img_folder: folder_name,
                    wav_file: wav_to_use.clone(),
                    base_name: base_name.clone(),
                    frame_count,
                    width,
                    height,
                    date,
                    video_file: stream_video(&img_folder),
                    alpha_folder: None,
                };
                accumulated_clips.push(clip);
            }
        }
    }

    // Deterministic sorting
    accumulated_clips.sort_by(|a, b| {
        a.take_folder
            .cmp(&b.take_folder)
            .then_with(|| a.img_folder.cmp(&b.img_folder))
            .then_with(|| a.clip_type.cmp(&b.clip_type))
    });

    for clip in accumulated_clips {
        let _ = tx.send(clip);
        count += 1;
    }

    count
}

fn count_bmps(folder: &Path) -> usize {
    let mut count = 0;
    if let Ok(read_dir) = std::fs::read_dir(folder) {
        for entry in read_dir.flatten() {
            if let Ok(file_type) = entry.file_type()
                && file_type.is_file()
            {
                let path = entry.path();
                if let Some(ext) = path.extension()
                    && ext.to_string_lossy().to_lowercase() == "bmp"
                {
                    count += 1;
                }
            }
        }
    }
    count
}

fn get_clip_date(img_folder_path: &Path) -> String {
    let bmp_path = img_folder_path.join("00000.bmp");
    if let Ok(metadata) =
        std::fs::metadata(&bmp_path).or_else(|_| std::fs::metadata(img_folder_path))
        && let Ok(created) = metadata.created().or_else(|_| metadata.modified())
    {
        return chrono::DateTime::<chrono::Local>::from(created)
            .format("%Y-%m-%d %I:%M %p")
            .to_string();
    }
    "-".to_string()
}

#[cfg(test)]
#[path = "scanner_tests.rs"]
mod tests;
