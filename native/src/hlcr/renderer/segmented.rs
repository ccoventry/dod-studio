//! Running a render in segments (#1451): the render's segment folder (what a
//! stopped render finished is reused, segments of the same take under other
//! settings are thrown away), rendering the segments still missing, joining
//! them into the clip, and removing them. Also the room a segmented render
//! asks of the export drives, and the sweeps that remove segments nothing
//! will join. Which renders are segmented, and the FFmpeg commands, are
//! `segment_plan`'s.
//!
//! Segments live in `<export folder>/.dodstudio-segments/<take>_<settings>/`
//! as `0000.seg`, `0001.seg`, ...: no video extension, so Find Moved Videos'
//! folder walk never takes one for a clip. Each is written under its partial
//! name (`0003.partial.seg`) and renamed once FFmpeg has finished it, so a
//! file under a finished name is always whole. They survive Studio closing
//! or crashing, which is the point; a cancel, a failure or the join removes
//! them. Beside them, `take.txt` names the take, so a folder whose take was
//! deleted is found and removed by the next render.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use super::ffmpeg_args::{Encode, encode_settings};
use super::ffmpeg_run::{ProgressScale, RunEnd, finish_render, run_to_end};
use super::partial_output::{self, Reserved};
use super::segment_plan::{
    FrameRate, KeySettings, Pictures, SEGMENT_EXT, Segment, SegmentArgs, SegmentKey, SegmentPlan,
    TakeFacts, concat_list, join_args, peak_bytes, segment_key, take_key,
};
use super::{RenderJob, RenderUpdate, RoomAsk, remove_partial_output};
use crate::hlcr::config::RenderCodec;
use crate::hlcr::scanner::ClipData;
use crate::messages::render as msg;

/// The folder in each export folder that holds renders' segments.
pub(super) const SEGMENTS_DIR: &str = ".dodstudio-segments";

/// The concat demuxer's list, written beside the segments for the join.
const JOIN_LIST: &str = "join.txt";

/// The file in each segment folder that holds its take folder's path.
const TAKE_FILE: &str = "take.txt";

/// What `segment_plan::plan` decides from, read off `clip`'s take: the
/// picture's shape and rate (`fps_used` for a BMP sequence, an AVI's own
/// header rate) and the wav's length.
pub(super) fn take_facts(clip: &ClipData, codec: RenderCodec, fps_used: u32) -> TakeFacts {
    let take_folder = Path::new(&clip.take_folder);
    let pictures = match clip.video_file.as_deref() {
        None if fps_used > 0 => Some((
            Pictures::Sequence,
            FrameRate {
                num: fps_used,
                den: 1,
            },
        )),
        None => None,
        Some(video)
            if Path::new(video)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("avi")) =>
        {
            let path = take_folder.join(&clip.img_folder).join(video);
            crate::hlcr::scanner::avi_frame_rate(&path).map(|(rate, scale)| {
                (
                    Pictures::Video,
                    FrameRate {
                        num: rate,
                        den: scale,
                    },
                )
            })
        }
        // An OBS MP4 or MKV: its rate isn't in a header this cheap to read.
        Some(_) => None,
    };
    let wav_seconds = clip
        .wav_file
        .as_deref()
        .and_then(|wav| crate::hlcr::take_length::wav_seconds(&take_folder.join(wav)));
    TakeFacts {
        codec,
        is_hud: clip.clip_type == "hud_only",
        frame_count: clip.frame_count,
        pictures,
        wav_seconds,
    }
}

/// The segment folders renders running now are using, or a sweep is
/// removing, by name: two renders of one take and stream never write one
/// folder, and neither throws the other's away.
fn folders_in_use() -> &'static Mutex<HashSet<String>> {
    static IN_USE: std::sync::OnceLock<Mutex<HashSet<String>>> = std::sync::OnceLock::new();
    IN_USE.get_or_init(|| Mutex::new(HashSet::new()))
}

/// The set stays valid after a panic mid-update: one insert or remove.
fn lock_in_use() -> std::sync::MutexGuard<'static, HashSet<String>> {
    folders_in_use()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// One render's segment folder, claimed until dropped.
pub(super) struct SegmentFolder {
    dir: PathBuf,
    name: String,
}

impl Drop for SegmentFolder {
    fn drop(&mut self) {
        lock_in_use().remove(&self.name);
    }
}

impl SegmentFolder {
    /// Where segment `index`'s finished file goes.
    fn segment_file(&self, index: usize) -> PathBuf {
        self.dir.join(segment_name(index))
    }

    /// Removes the folder and every segment in it, off the async worker.
    /// Returns how many finished segments went.
    async fn remove(&self) -> usize {
        let dir = self.dir.clone();
        tokio::task::spawn_blocking(move || remove_segment_dir(&dir))
            .await
            // Only a panic in the removal ends the task without a count.
            .unwrap_or(0)
    }

    /// Removes the segments of a render that ended without its clip, and
    /// logs it.
    async fn remove_for(&self, job: &RenderJob, why: &str) {
        let removed = self.remove().await;
        if removed > 0 {
            crate::log_markdown(&msg::segments_removed(&job.job_id, removed, why));
        }
    }
}

/// Removes segment folder `dir` and everything in it, and the segments root
/// when that leaves it empty. Returns how many finished segments went.
fn remove_segment_dir(dir: &Path) -> usize {
    let finished = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| is_finished_segment(&e.path()))
                .count()
        })
        .unwrap_or(0);
    if let Err(e) = std::fs::remove_dir_all(dir)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        crate::log_markdown(&msg::segments_left(dir.display(), e));
    }
    remove_root_if_empty(dir);
    finished
}

/// Removes the segments root above segment folder `dir` if nothing is left
/// in it.
fn remove_root_if_empty(dir: &Path) {
    if let Some(root) = dir.parent() {
        // Fails, as it should, while other renders' segments are there.
        let _ = std::fs::remove_dir(root);
    }
}

/// `0003.seg`.
fn segment_name(index: usize) -> String {
    format!("{index:04}.{SEGMENT_EXT}")
}

/// Whether `path` is a finished segment, not one still being written.
fn is_finished_segment(path: &Path) -> bool {
    path.extension()
        .is_some_and(|x| x.eq_ignore_ascii_case(SEGMENT_EXT))
        && !path.file_stem().is_some_and(|stem| {
            stem.to_string_lossy()
                .ends_with(partial_output::PARTIAL_MARK)
        })
}

/// The take's wav and video sizes and modified times, for the segment key:
/// a take written over in place changes them, even a BMP take captured again
/// at the same length, whose frames aren't in the key but whose wav is
/// written again with them.
fn source_stamp(clip: &ClipData) -> String {
    let take_folder = Path::new(&clip.take_folder);
    let stamp = |path: PathBuf| {
        std::fs::metadata(path).map_or_else(
            |_| "-".to_string(),
            |meta| {
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_nanos());
                format!("{}@{modified}", meta.len())
            },
        )
    };
    let wav = clip
        .wav_file
        .as_deref()
        .map_or_else(String::new, |wav| stamp(take_folder.join(wav)));
    let video = clip
        .video_file
        .as_deref()
        .map_or_else(String::new, |video| {
            stamp(take_folder.join(&clip.img_folder).join(video))
        });
    format!("{wav};{video}")
}

/// Each export folder's segments root.
fn segment_roots(job: &RenderJob) -> Vec<PathBuf> {
    job.config
        .export_directories
        .iter()
        .map(|dir| dir.join(SEGMENTS_DIR))
        .collect()
}

/// A render of `job` that would run in segments, decided before any room is
/// reserved: its plan, its segment key, and the segments a stopped render of
/// the same key left, if any.
pub(super) struct Prepared {
    pub(super) plan: SegmentPlan,
    pub(super) key: SegmentKey,
    stopped: Option<Stopped>,
}

/// Segments a stopped render left: the export folder they are in and how
/// many of the take's frames they hold.
#[derive(Debug, PartialEq)]
struct Stopped {
    export_dir: PathBuf,
    frames_on_disk: usize,
}

/// `job` as a segmented render, or `None` when it renders in one piece
/// (`segment_plan::plan`, or a codec FFmpeg isn't given settings for).
pub(super) fn prepare(job: &RenderJob, fps_used: u32) -> Option<Prepared> {
    let plan =
        super::segment_plan::plan(&take_facts(&job.clip, job.config.target_codec, fps_used))?;
    // The one-piece path refuses a codec without settings, with its reason.
    let encode = encode_settings(job.is_hud, &job.config).ok()?;
    let key = key_for(job, &plan, &encode);
    let stopped = find_stopped(&job.config.export_directories, &key, &plan);
    Some(Prepared { plan, key, stopped })
}

/// `job`'s segment key under `plan` and `encode`.
fn key_for(job: &RenderJob, plan: &SegmentPlan, encode: &Encode) -> SegmentKey {
    let stamp = source_stamp(&job.clip);
    segment_key(
        &job.clip,
        &KeySettings {
            encode,
            plan,
            ffmpeg_path: &job.config.ffmpeg_path,
            source_stamp: &stamp,
        },
    )
}

/// The first export folder holding finished segments of `key`, and how many
/// frames they cover.
fn find_stopped(export_dirs: &[PathBuf], key: &SegmentKey, plan: &SegmentPlan) -> Option<Stopped> {
    let name = key.folder_name();
    export_dirs.iter().find_map(|export_dir| {
        let dir = export_dir.join(SEGMENTS_DIR).join(&name);
        let frames_on_disk: usize = plan
            .segments()
            .iter()
            .filter(|s| dir.join(segment_name(s.index)).is_file())
            .map(|s| s.frames)
            .sum();
        (frames_on_disk > 0).then(|| Stopped {
            export_dir: export_dir.clone(),
            frames_on_disk,
        })
    })
}

impl Prepared {
    /// What this render asks of the export drives, in order of preference,
    /// for a clip of `clip_bytes`: a resume on the drive its segments are on
    /// (the clip plus the segments still to render), then a fresh start on
    /// any (twice the clip). The caller adds the one-piece ask after these.
    pub(super) fn room_asks(&self, export_dirs: &[PathBuf], clip_bytes: u64) -> Vec<RoomAsk> {
        let mut asks = Vec::new();
        if let Some(stopped) = &self.stopped {
            asks.push(RoomAsk {
                dirs: vec![stopped.export_dir.clone()],
                amount: peak_bytes(clip_bytes, &self.plan, stopped.frames_on_disk),
            });
        }
        asks.push(RoomAsk {
            dirs: export_dirs.to_vec(),
            amount: peak_bytes(clip_bytes, &self.plan, 0),
        });
        asks
    }
}

/// Claims this render's segment folder in `output_folder`, the export folder
/// its room was reserved in: a stopped render's segments of the same key are
/// used there, and thrown away in any other export folder (nothing was
/// reserved on that drive for the segments still to render). Throws away,
/// and logs, segments of this take and stream under other settings that no
/// running render is using. `None`, and a log line, when another render is
/// using the folder or it can't be made: the render then runs in one piece.
pub(super) async fn claim_folder(
    job: &RenderJob,
    key: &SegmentKey,
    output_folder: &Path,
) -> Option<SegmentFolder> {
    let name = key.folder_name();
    if !lock_in_use().insert(name.clone()) {
        crate::log_markdown(&msg::segments_busy(&job.job_id));
        return None;
    }
    let folder = SegmentFolder {
        dir: output_folder.join(SEGMENTS_DIR).join(&name),
        name,
    };
    let mut roots = segment_roots(job);
    let own_root = output_folder.join(SEGMENTS_DIR);
    if !roots.contains(&own_root) {
        roots.push(own_root);
    }
    let setup = FolderSetup {
        roots,
        key: key.clone(),
        dir: folder.dir.clone(),
        take_folder: job.clip.take_folder.clone(),
    };
    let made = tokio::task::spawn_blocking(move || setup.run())
        .await
        .unwrap_or_else(|e| Err(std::io::Error::other(e)));
    if let Err(e) = made {
        crate::log_markdown(&msg::segments_folder_failed(
            &job.job_id,
            folder.dir.display(),
            e,
        ));
        return None;
    }
    Some(folder)
}

/// The disk work of claiming a segment folder, run off the async worker.
struct FolderSetup {
    roots: Vec<PathBuf>,
    key: SegmentKey,
    dir: PathBuf,
    take_folder: String,
}

impl FolderSetup {
    /// Throws away other settings' segments of this take and this key's
    /// folder in other export folders, then makes the folder and writes its
    /// take file.
    fn run(self) -> std::io::Result<()> {
        let key = &self.key;
        remove_unclaimed(&self.roots, |name, _| {
            key.is_other_settings(name)
                .then_some(msg::LEFTOVER_OTHER_SETTINGS)
        });
        // This render holds the name, so no sweep or render is in these.
        for root in &self.roots {
            let elsewhere = root.join(key.folder_name());
            if elsewhere != self.dir && elsewhere.is_dir() {
                remove_logged(&elsewhere, msg::LEFTOVER_OTHER_FOLDER);
                remove_root_if_empty(&elsewhere);
            }
        }
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(self.dir.join(TAKE_FILE), &self.take_folder)
    }
}

/// Removes every segment folder whose take is gone (its take file names a
/// folder that isn't there), in any export folder, and logs each. Run as
/// every render starts, so a deleted take's segments don't stay.
pub(super) async fn remove_orphans(job: &RenderJob) {
    let roots = segment_roots(job);
    let _ = tokio::task::spawn_blocking(move || {
        remove_unclaimed(&roots, |_, path| {
            take_is_gone(path).then_some(msg::LEFTOVER_TAKE_GONE)
        });
    })
    .await;
}

/// Whether segment folder `dir`'s take file names a folder that isn't
/// there. A folder without one (being made, or unreadable) is kept.
fn take_is_gone(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join(TAKE_FILE))
        .is_ok_and(|take| !take.trim().is_empty() && !Path::new(take.trim()).is_dir())
}

/// Removes every segment folder of `job`'s take and stream, under any
/// settings, that no running render is using: the take is rendering in one
/// piece, so nothing will join them.
pub(super) async fn remove_take_segments(job: &RenderJob) {
    let roots = segment_roots(job);
    let take = take_key(&job.clip);
    let _ = tokio::task::spawn_blocking(move || {
        remove_unclaimed(&roots, |name, _| {
            name.strip_prefix(&take)
                .is_some_and(|rest| rest.starts_with('_'))
                .then_some(msg::LEFTOVER_ONE_PIECE)
        });
    })
    .await;
}

/// Removes every segment folder under `roots` that `pick` gives a reason
/// for and no render is using, and logs each with that reason. Blocking:
/// run it off the async worker. The folders are listed without the in-use
/// lock and claimed under it while they are removed, so a render starting
/// meanwhile renders in one piece rather than writing into one.
fn remove_unclaimed(roots: &[PathBuf], pick: impl Fn(&str, &Path) -> Option<&'static str>) {
    let mut picked: Vec<(String, PathBuf, &'static str)> = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if let Some(why) = pick(&name, &path) {
                picked.push((name, path, why));
            }
        }
    }
    if picked.is_empty() {
        return;
    }
    let claimed: HashSet<String> = {
        let mut in_use = lock_in_use();
        picked
            .iter()
            .filter(|(name, _, _)| in_use.insert(name.clone()))
            .map(|(name, _, _)| name.clone())
            .collect()
    };
    for (name, path, why) in &picked {
        if claimed.contains(name) {
            remove_logged(path, why);
            remove_root_if_empty(path);
        }
    }
    let mut in_use = lock_in_use();
    for name in &claimed {
        in_use.remove(name);
    }
}

/// Removes segment folder `path`, logging that it went and why, or why not.
fn remove_logged(path: &Path, why: &str) {
    match std::fs::remove_dir_all(path) {
        Ok(()) => crate::log_markdown(&msg::leftover_segments_removed(path.display(), why)),
        Err(e) => crate::log_markdown(&msg::segments_left(path.display(), e)),
    }
}

/// Segment numbers as the log shows them, counted from 1 and run together:
/// `1-4, 6`.
fn segment_ranges(indexes: &[usize]) -> String {
    let mut ranges: Vec<String> = Vec::new();
    let mut iter = indexes.iter().copied().peekable();
    while let Some(first) = iter.next() {
        let mut last = first;
        while iter.peek() == Some(&(last + 1)) {
            last += 1;
            iter.next();
        }
        ranges.push(if first == last {
            format!("{}", first + 1)
        } else {
            format!("{}-{}", first + 1, last + 1)
        });
    }
    ranges.join(", ")
}

/// Everything a segmented render needs beyond the job.
pub(super) struct SegmentedRender<'a> {
    pub(super) plan: &'a SegmentPlan,
    pub(super) folder: SegmentFolder,
    pub(super) encode: &'a Encode<'a>,
    pub(super) threads: usize,
    /// The clip's reserved name: the join writes its partial file.
    pub(super) output: &'a Reserved<'a>,
}

/// Renders `job` in segments and sends every update up to its `Finished`,
/// like `ffmpeg_run::run_ffmpeg`: the segments a stopped render finished are
/// kept, the rest rendered one after another with one progress bar across
/// them, then all joined into `render.output` and removed. A cancel or a
/// failure removes them too; Studio closing leaves the finished ones for
/// the next start's recovery to resume from.
pub(super) async fn run_segmented(job: &RenderJob, render: SegmentedRender<'_>) {
    job.send(RenderUpdate::Status(
        job.job_id.clone(),
        "Rendering".to_string(),
    ));
    let plan = render.plan;
    let segments = plan.segments();
    let finished: Vec<usize> = segments
        .iter()
        .map(|s| s.index)
        .filter(|&i| render.folder.segment_file(i).is_file())
        .collect();
    let reused = if finished.is_empty() {
        String::new()
    } else {
        msg::segments_reused(&segment_ranges(&finished), finished.len(), segments.len())
    };
    crate::log_markdown(&msg::segments_started(
        &job.job_id,
        segments.len(),
        plan.segment_frames,
        render.folder.dir.display(),
        reused,
    ));
    for segment in segments.iter().filter(|s| !finished.contains(&s.index)) {
        if !render_segment(job, &render, *segment).await {
            return;
        }
    }
    join_segments(job, &render, segments.len()).await;
}

/// Renders one segment into its file. `false` when the render ended here
/// (cancelled, failed, or Studio closing), with its updates sent.
async fn render_segment(job: &RenderJob, render: &SegmentedRender<'_>, segment: Segment) -> bool {
    let held = partial_output::in_flight().hold(render.folder.segment_file(segment.index));
    let args = SegmentArgs {
        clip: &job.clip,
        plan: render.plan,
        segment,
        encode: render.encode,
        threads: render.threads,
        out_file: &held.partial,
    }
    .to_args();
    let scale = ProgressScale {
        frames_before: segment.start_frame,
        total_frames: render.plan.total_frames,
        max_percent: 99,
    };
    match run_to_end(job, args, Some(scale)).await {
        RunEnd::Succeeded { .. } => {
            flush_to_disk(&held.partial).await;
            match partial_output::in_flight().finish(&held) {
                Ok(_) => true,
                Err(e) => {
                    let message =
                        msg::rename_failed(held.partial.display(), held.out_file.display(), e);
                    remove_partial_output(&held.partial).await;
                    render.folder.remove_for(job, msg::SEGMENTS_FAILED).await;
                    job.finish_error(message);
                    false
                }
            }
        }
        // The finished segments stay; Studio's way out removes this one.
        RunEnd::Exiting => false,
        end => {
            remove_partial_output(&held.partial).await;
            end_without_clip(job, render, end).await;
            false
        }
    }
}

/// Ends a render that stopped before its clip was joined: removes its
/// segments and sends the job's last updates for how `end` went.
async fn end_without_clip(job: &RenderJob, render: &SegmentedRender<'_>, end: RunEnd) {
    match end {
        RunEnd::Cancelled => {
            render.folder.remove_for(job, msg::SEGMENTS_CANCELLED).await;
            job.finish_cancelled();
        }
        RunEnd::NotStarted(reason) => {
            render.folder.remove_for(job, msg::SEGMENTS_FAILED).await;
            job.refuse(reason);
        }
        RunEnd::Failed(message) | RunEnd::Lost(message) => {
            render.folder.remove_for(job, msg::SEGMENTS_FAILED).await;
            job.finish_error(message);
        }
        RunEnd::Succeeded { .. } | RunEnd::Exiting => {}
    }
}

/// A finished segment's bytes, written to the disk before it takes its
/// finished name, off the async worker (a segment can be gigabytes): a power
/// cut after the rename would otherwise leave a finished name over data that
/// never reached the disk, and the resume would join it.
async fn flush_to_disk(path: &Path) {
    let path = path.to_path_buf();
    // Best effort: a segment that can't be flushed is still whole as far as
    // FFmpeg is concerned, and it only matters across a power cut.
    let _ = tokio::task::spawn_blocking(move || {
        if let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) {
            let _ = file.sync_all();
        }
    })
    .await;
}

/// Joins every segment into the clip's partial file by stream copy, checks
/// it holds every frame, gives it its name, removes the segments and sends
/// the job's last updates.
async fn join_segments(job: &RenderJob, render: &SegmentedRender<'_>, count: usize) {
    job.send(RenderUpdate::Speed(
        job.job_id.clone(),
        msg::joining_segments(count),
    ));
    let names: Vec<String> = (0..count).map(segment_name).collect();
    let list = render.folder.dir.join(JOIN_LIST);
    if let Err(e) = std::fs::write(&list, concat_list(&names)) {
        render.folder.remove_for(job, msg::SEGMENTS_FAILED).await;
        job.finish_error(msg::join_list_failed(list.display(), e));
        return;
    }
    let output = render.output;
    let args = join_args(
        &list,
        &output.partial,
        render.encode.file_ext,
        job.config.clip_tag.as_deref(),
    );
    let began = std::time::Instant::now();
    match run_to_end(job, args, None).await {
        RunEnd::Succeeded { last_frame } if last_frame == Some(render.plan.total_frames) => {
            crate::log_markdown(&msg::segments_joined(&job.job_id, count, began.elapsed()));
            // Named before the segments go: Studio closing in between leaves
            // the clip, or the segments to join again, never neither.
            finish_render(job, output);
            render.folder.remove().await;
        }
        // A segment the concat demuxer read only part of, without an error
        // even under `-xerror`: the clip is short, so it is no clip.
        RunEnd::Succeeded { last_frame } => {
            let message = msg::join_short(last_frame, render.plan.total_frames);
            crate::log_markdown(&msg::join_failed(&job.job_id, &message));
            remove_partial_output(&output.partial).await;
            render.folder.remove_for(job, msg::SEGMENTS_FAILED).await;
            job.finish_error(message);
        }
        // The segments stay for the resume; Studio's way out removes the
        // half-joined clip.
        RunEnd::Exiting => {}
        RunEnd::Failed(message) => {
            crate::log_markdown(&msg::join_failed(&job.job_id, &message));
            remove_partial_output(&output.partial).await;
            end_without_clip(job, render, RunEnd::Failed(message)).await;
        }
        end => {
            remove_partial_output(&output.partial).await;
            end_without_clip(job, render, end).await;
        }
    }
}

#[cfg(test)]
#[path = "segmented_tests.rs"]
mod tests;
