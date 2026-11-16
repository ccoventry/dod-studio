//! Running a render in segments (#1451): the render's segment folder (what a
//! stopped render finished is reused, segments of the same take under other
//! settings are thrown away), rendering the segments still missing, joining
//! them into the clip, and removing them. Which renders are segmented, and
//! the FFmpeg commands, are `segment_plan`'s.
//!
//! Segments live in `<export folder>/.dodstudio-segments/<take>_<settings>/`
//! as `0000.seg`, `0001.seg`, ...: no video extension, so Find Moved Videos'
//! folder walk never takes one for a clip. Each is written under its partial
//! name (`0003.partial.seg`) and renamed once FFmpeg has finished it, so a
//! file under a finished name is always whole. They survive Studio closing
//! or crashing, which is the point; a cancel, a failure or the join removes
//! them.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use super::ffmpeg_args::Encode;
use super::ffmpeg_run::{ProgressScale, RunEnd, finish_render, run_to_end};
use super::partial_output::{self, Reserved};
use super::segment_plan::{
    FrameRate, KeySettings, Pictures, SEGMENT_EXT, Segment, SegmentArgs, SegmentPlan, TakeFacts,
    concat_list, join_args, segment_key,
};
use super::{RenderJob, RenderUpdate, remove_partial_output};
use crate::messages::render as msg;

/// The folder in each export folder that holds renders' segments.
pub(super) const SEGMENTS_DIR: &str = ".dodstudio-segments";

/// The concat demuxer's list, written beside the segments for the join.
const JOIN_LIST: &str = "join.txt";

/// What `segment_plan::plan` decides from, read off `job`'s take: the
/// picture's shape and rate (`fps_used` for a BMP sequence, an AVI's own
/// header rate) and the wav's length.
pub(super) fn take_facts(job: &RenderJob, fps_used: u32) -> TakeFacts {
    let clip = &job.clip;
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
            let path = job.take_folder.join(&clip.img_folder).join(video);
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
        .and_then(|wav| crate::hlcr::take_length::wav_seconds(&job.take_folder.join(wav)));
    TakeFacts {
        codec: job.config.target_codec,
        is_hud: job.is_hud,
        frame_count: clip.frame_count,
        pictures,
        wav_seconds,
    }
}

/// The segment folders renders running now are using, by name: two renders
/// of one take and stream never write one folder, and neither throws the
/// other's away.
fn folders_in_use() -> &'static Mutex<HashSet<String>> {
    static IN_USE: std::sync::OnceLock<Mutex<HashSet<String>>> = std::sync::OnceLock::new();
    IN_USE.get_or_init(|| Mutex::new(HashSet::new()))
}

/// One render's segment folder, claimed until dropped.
pub(super) struct SegmentFolder {
    dir: PathBuf,
    name: String,
}

impl Drop for SegmentFolder {
    fn drop(&mut self) {
        // The set stays valid after a panic mid-update: one insert or remove.
        folders_in_use()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.name);
    }
}

impl SegmentFolder {
    /// Where segment `index`'s finished file goes.
    fn segment_file(&self, index: usize) -> PathBuf {
        self.dir.join(segment_name(index))
    }

    /// Removes the folder and every segment in it, and the segments root
    /// when that leaves it empty. Returns how many finished segments went.
    fn remove(&self) -> usize {
        let finished = std::fs::read_dir(&self.dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| is_finished_segment(&e.path()))
                    .count()
            })
            .unwrap_or(0);
        if let Err(e) = std::fs::remove_dir_all(&self.dir)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            crate::log_markdown(&msg::segments_left(self.dir.display(), e));
        }
        if let Some(root) = self.dir.parent() {
            // Fails, as it should, while other renders' segments are there.
            let _ = std::fs::remove_dir(root);
        }
        finished
    }

    /// Removes the segments of a render that ended without its clip, and
    /// logs it.
    fn remove_for(&self, job: &RenderJob, why: &str) {
        let removed = self.remove();
        if removed > 0 {
            crate::log_markdown(&msg::segments_removed(&job.job_id, removed, why));
        }
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

/// The take's wav and video sizes together, for the segment key: a take
/// written over in place changes them.
fn source_bytes(job: &RenderJob) -> u64 {
    let clip = &job.clip;
    let size = |path: PathBuf| std::fs::metadata(path).map_or(0, |m| m.len());
    let wav = clip
        .wav_file
        .as_deref()
        .map_or(0, |wav| size(job.take_folder.join(wav)));
    let video = clip.video_file.as_deref().map_or(0, |video| {
        size(job.take_folder.join(&clip.img_folder).join(video))
    });
    wav + video
}

/// Claims this render's segment folder: the one a stopped render of the
/// same take, stream and settings left in any export folder, else a new one
/// in `output_folder`. Throws away, and logs, segments of this take and
/// stream under other settings that no running render is using. `None`, and
/// a log line, when another render is using the folder or it can't be made:
/// the render then runs in one piece.
pub(super) fn claim_folder(
    job: &RenderJob,
    plan: &SegmentPlan,
    encode: &Encode,
    output_folder: &Path,
) -> Option<SegmentFolder> {
    let key = segment_key(
        &job.clip,
        &KeySettings {
            encode,
            plan,
            ffmpeg_path: &job.config.ffmpeg_path,
            source_bytes: source_bytes(job),
        },
    );
    let name = key.folder_name();
    let mut in_use = folders_in_use()
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if !in_use.insert(name.clone()) {
        crate::log_markdown(&msg::segments_busy(&job.job_id));
        return None;
    }
    let mut roots: Vec<PathBuf> = job
        .config
        .export_directories
        .iter()
        .map(|dir| dir.join(SEGMENTS_DIR))
        .collect();
    let own_root = output_folder.join(SEGMENTS_DIR);
    if !roots.contains(&own_root) {
        roots.push(own_root.clone());
    }
    remove_other_settings(&roots, |folder| {
        key.is_other_settings(folder) && !in_use.contains(folder)
    });
    drop(in_use);
    let folder = SegmentFolder {
        dir: roots
            .iter()
            .map(|root| root.join(&name))
            .find(|dir| dir.is_dir())
            .unwrap_or_else(|| own_root.join(&name)),
        name,
    };
    if let Err(e) = std::fs::create_dir_all(&folder.dir) {
        crate::log_markdown(&msg::segments_folder_failed(
            &job.job_id,
            folder.dir.display(),
            e,
        ));
        return None;
    }
    Some(folder)
}

/// Removes every segment folder under `roots` that `is_stale` picks, and
/// logs each.
fn remove_other_settings(roots: &[PathBuf], is_stale: impl Fn(&str) -> bool) {
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let folder = entry.file_name().to_string_lossy().into_owned();
            if !is_stale(&folder) {
                continue;
            }
            let path = entry.path();
            match std::fs::remove_dir_all(&path) {
                Ok(()) => crate::log_markdown(&msg::stale_segments_removed(path.display())),
                Err(e) => crate::log_markdown(&msg::segments_left(path.display(), e)),
            }
        }
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
    };
    match run_to_end(job, args, Some(scale)).await {
        RunEnd::Succeeded => {
            flush_to_disk(&held.partial);
            match partial_output::in_flight().finish(&held) {
                Ok(_) => true,
                Err(e) => {
                    let message =
                        msg::rename_failed(held.partial.display(), held.out_file.display(), e);
                    remove_partial_output(&held.partial).await;
                    render.folder.remove_for(job, msg::SEGMENTS_FAILED);
                    job.finish_error(message);
                    false
                }
            }
        }
        // The finished segments stay; Studio's way out removes this one.
        RunEnd::Exiting => false,
        end => {
            remove_partial_output(&held.partial).await;
            end_without_clip(job, render, end);
            false
        }
    }
}

/// Ends a render that stopped before its clip was joined: removes its
/// segments and sends the job's last updates for how `end` went.
fn end_without_clip(job: &RenderJob, render: &SegmentedRender<'_>, end: RunEnd) {
    match end {
        RunEnd::Cancelled => {
            render.folder.remove_for(job, msg::SEGMENTS_CANCELLED);
            job.finish_cancelled();
        }
        RunEnd::NotStarted(reason) => {
            render.folder.remove_for(job, msg::SEGMENTS_FAILED);
            job.refuse(reason);
        }
        RunEnd::Failed(message) | RunEnd::Lost(message) => {
            render.folder.remove_for(job, msg::SEGMENTS_FAILED);
            job.finish_error(message);
        }
        RunEnd::Succeeded | RunEnd::Exiting => {}
    }
}

/// A finished segment's bytes, written to the disk before it takes its
/// finished name: a power cut after the rename would otherwise leave a
/// finished name over data that never reached the disk, and the resume
/// would join it.
fn flush_to_disk(path: &Path) {
    // Best effort: a segment that can't be flushed is still whole as far as
    // FFmpeg is concerned, and it only matters across a power cut.
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) {
        let _ = file.sync_all();
    }
}

/// Joins every segment into the clip's partial file by stream copy, gives
/// it its name, removes the segments and sends the job's last updates.
async fn join_segments(job: &RenderJob, render: &SegmentedRender<'_>, count: usize) {
    let names: Vec<String> = (0..count).map(segment_name).collect();
    let list = render.folder.dir.join(JOIN_LIST);
    if let Err(e) = std::fs::write(&list, concat_list(&names)) {
        render.folder.remove_for(job, msg::SEGMENTS_FAILED);
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
        RunEnd::Succeeded => {
            crate::log_markdown(&msg::segments_joined(&job.job_id, count, began.elapsed()));
            render.folder.remove();
            finish_render(job, output);
        }
        // The segments stay for the resume; Studio's way out removes the
        // half-joined clip.
        RunEnd::Exiting => {}
        end => {
            remove_partial_output(&output.partial).await;
            end_without_clip(job, render, end);
        }
    }
}

#[cfg(test)]
#[path = "segmented_tests.rs"]
mod tests;
