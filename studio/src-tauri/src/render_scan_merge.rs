// studio/src-tauri/src/render_scan_merge.rs
//
// Scan for Takes merges into the queue (#772) instead of replacing it, so it
// works while a batch is queued or rendering. A child module of
// render_manager (declared there with `#[path]`): it works on the job rows
// that module keeps private. Pure: everything that touches the disk (the
// folder walk, each take's history, whether a folder is gone) is done before
// the merge, off the jobs lock, and handed in.

use super::{HistoryEntry, RenderJobRuntime, attempt_stream};
use native::hlcr::config::RenderCodec;
use native::hlcr::scanner::ClipData;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

/// One take stream the scan found, with what the disk says about it.
pub(super) struct ScannedTake {
    pub clip: ClipData,
    pub history: Vec<HistoryEntry>,
    /// `last_finished_output` of `history`: the newest finished render of
    /// this stream whose file is still there.
    pub finished_output: Option<(String, Option<u64>)>,
}

/// What a new row is queued with: the panel's settings at scan time.
pub(super) struct NewJobSettings {
    pub codec: RenderCodec,
    pub custom_codec_args: String,
    pub fps: u32,
}

/// What a scan changed, for the toast and the log.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct ScanMergeSummary {
    /// Take streams the scan found.
    pub total: usize,
    /// Rows added for takes the list didn't have.
    pub added: usize,
    /// Finished, failed or cancelled rows re-read from their take.
    pub refreshed: usize,
    /// Rows left exactly as they were: Queued or Rendering, or not found by
    /// this scan while their folder is still there.
    pub kept: usize,
    /// Rows dropped because their take folder is gone.
    pub gone: usize,
    /// How many of `gone` were Queued: a render that will no longer happen.
    pub gone_queued: usize,
}

/// A row's identity across scans: its take folder and the stream of that
/// take it renders (a take with a HUD pair is two rows).
fn identity(clip: &ClipData) -> (String, String) {
    (clip.take_folder.clone(), attempt_stream(clip))
}

/// A recovered row whose autosave lacked the scanned take (#770): it has no
/// stream yet, so it takes its folder's first stream no other row claimed.
fn is_stub(clip: &ClipData) -> bool {
    clip.img_folder.is_empty() && clip.video_file.is_none() && clip.frame_count == 0
}

/// Merges a scan into the job list (#772):
/// - Rendering and Queued rows stay as they are (status, progress, codec,
///   fps, history, current attempt); a Queued recovered stub gets its
///   scanned take filled in, which it needs to render.
/// - Finished, Error and Cancelled rows found again are refreshed (take,
///   history, output) and keep their status and id, so the Render tab keeps
///   their tick.
/// - Takes the list didn't have are added at the end with fresh ids, staged
///   the way a scan stages them: Finished when rendered before, else Queued.
/// - Rows whose take folder is gone are dropped, unless Rendering.
///
/// `next_id` is the next unused job id; it is raised past every id in
/// `existing` first, so a new row never reuses an id the Render tab has seen
/// (its ticks are keyed by id).
pub(super) fn merge_scan(
    existing: Vec<RenderJobRuntime>,
    scanned: Vec<ScannedTake>,
    gone_folders: &HashSet<String>,
    settings: &NewJobSettings,
    next_id: &mut usize,
) -> (Vec<RenderJobRuntime>, ScanMergeSummary) {
    let mut summary = ScanMergeSummary {
        total: scanned.len(),
        ..Default::default()
    };
    *next_id = existing
        .iter()
        .filter_map(|j| j.id.parse::<usize>().ok())
        .map(|n| n + 1)
        .fold(*next_id, usize::max);

    // The same take found twice (a folder listed twice, or nested in another
    // listed folder) is one row.
    let mut index: HashMap<(String, String), usize> = HashMap::new();
    let mut slots: Vec<Option<ScannedTake>> = Vec::with_capacity(scanned.len());
    for take in scanned {
        if let std::collections::hash_map::Entry::Vacant(e) = index.entry(identity(&take.clip)) {
            e.insert(slots.len());
            slots.push(Some(take));
        }
    }

    let mut merged = Vec::with_capacity(existing.len() + slots.len());
    for mut job in existing {
        let slot = index
            .get(&identity(&job.clip))
            .copied()
            .filter(|&i| slots[i].is_some())
            .or_else(|| {
                is_stub(&job.clip)
                    .then(|| {
                        slots.iter().position(|s| {
                            s.as_ref()
                                .is_some_and(|s| s.clip.take_folder == job.clip.take_folder)
                        })
                    })
                    .flatten()
            });
        let found = slot.and_then(|i| slots[i].take());
        let gone = found.is_none() && gone_folders.contains(&job.clip.take_folder);

        if job.status == "Rendering" {
            summary.kept += 1;
            merged.push(job);
        } else if job.status == "Queued" {
            if gone {
                summary.gone += 1;
                summary.gone_queued += 1;
                continue;
            }
            if let Some(take) = found
                && is_stub(&job.clip)
            {
                job.clip = take.clip;
                job.history = take.history;
            }
            summary.kept += 1;
            merged.push(job);
        } else if let Some(take) = found {
            if job.status == "Finished"
                && let Some((path, size)) = take.finished_output
            {
                job.output_path = path;
                job.output_size_bytes = size;
            }
            job.clip = take.clip;
            job.history = take.history;
            summary.refreshed += 1;
            merged.push(job);
        } else if gone {
            summary.gone += 1;
        } else {
            summary.kept += 1;
            merged.push(job);
        }
    }

    for take in slots.into_iter().flatten() {
        let (status, progress, output_path, output_size_bytes) = match take.finished_output {
            Some((path, size)) => ("Finished", 100u32, path, size),
            None => ("Queued", 0u32, String::new(), None),
        };
        merged.push(RenderJobRuntime {
            id: next_id.to_string(),
            clip: take.clip,
            status: status.to_string(),
            speed: String::new(),
            progress,
            error_log: None,
            output_path,
            output_size_bytes,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            codec: settings.codec,
            custom_codec_args: settings.custom_codec_args.clone(),
            fps: settings.fps,
            history: take.history,
            current_attempt: None,
        });
        *next_id += 1;
        summary.added += 1;
    }

    (merged, summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(take_folder: &str, img_folder: &str) -> ClipData {
        ClipData {
            clip_name: None,
            take_folder: take_folder.to_string(),
            clip_type: "single".to_string(),
            img_folder: img_folder.to_string(),
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

    fn row(id: &str, take_folder: &str, status: &str) -> RenderJobRuntime {
        RenderJobRuntime {
            id: id.to_string(),
            clip: clip(take_folder, "all"),
            status: status.to_string(),
            speed: String::new(),
            progress: 0,
            error_log: None,
            output_path: String::new(),
            output_size_bytes: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            codec: RenderCodec::ProRes,
            custom_codec_args: String::new(),
            fps: 300,
            history: Vec::new(),
            current_attempt: None,
        }
    }

    fn scanned(take_folder: &str, frames: usize) -> ScannedTake {
        let mut c = clip(take_folder, "all");
        c.frame_count = frames;
        ScannedTake {
            clip: c,
            history: Vec::new(),
            finished_output: None,
        }
    }

    fn defaults() -> NewJobSettings {
        NewJobSettings {
            codec: RenderCodec::H264Software,
            custom_codec_args: String::new(),
            fps: 60,
        }
    }

    fn merge(
        existing: Vec<RenderJobRuntime>,
        scan: Vec<ScannedTake>,
        gone: &[&str],
    ) -> (Vec<RenderJobRuntime>, ScanMergeSummary) {
        let gone: HashSet<String> = gone.iter().map(|s| s.to_string()).collect();
        let mut next_id = 0;
        merge_scan(existing, scan, &gone, &defaults(), &mut next_id)
    }

    #[test]
    fn a_rendering_row_is_left_exactly_as_it_was() {
        let mut rendering = row("3", "T:/a", "Rendering");
        rendering.progress = 42;
        rendering.speed = "31 fps".to_string();
        rendering.current_attempt = Some(77);
        let flag = rendering.cancel_flag.clone();
        let (rows, summary) = merge(vec![rendering], vec![scanned("T:/a", 999)], &[]);
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(
            (r.id.as_str(), r.status.as_str(), r.progress),
            ("3", "Rendering", 42)
        );
        assert_eq!(r.speed, "31 fps");
        assert_eq!(r.current_attempt, Some(77));
        assert_eq!(
            r.clip.frame_count, 300,
            "the clip it renders is not swapped"
        );
        assert!(Arc::ptr_eq(&r.cancel_flag, &flag));
        assert_eq!(summary.kept, 1);
        assert_eq!(summary.added, 0);
    }

    #[test]
    fn a_queued_row_keeps_its_own_codec_and_id() {
        let mut queued = row("5", "T:/a", "Queued");
        queued.codec = RenderCodec::SourceCopy;
        queued.fps = 120;
        let (rows, summary) = merge(vec![queued], vec![scanned("T:/a", 999)], &[]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "5");
        assert_eq!(rows[0].status, "Queued");
        assert_eq!(rows[0].codec, RenderCodec::SourceCopy);
        assert_eq!(rows[0].fps, 120);
        assert_eq!(summary.kept, 1);
    }

    #[test]
    fn a_finished_row_is_refreshed_and_keeps_its_id_and_status() {
        let mut finished = row("2", "T:/a", "Finished");
        finished.output_path = "E:/out/old.mov".to_string();
        let mut take = scanned("T:/a", 450);
        take.finished_output = Some(("E:/out/new.mov".to_string(), Some(9)));
        let (rows, summary) = merge(vec![finished], vec![take], &[]);
        assert_eq!(rows[0].id, "2");
        assert_eq!(rows[0].status, "Finished");
        assert_eq!(rows[0].clip.frame_count, 450);
        assert_eq!(rows[0].output_path, "E:/out/new.mov");
        assert_eq!(rows[0].output_size_bytes, Some(9));
        assert_eq!(rows[0].codec, RenderCodec::ProRes, "its own codec stays");
        assert_eq!(summary.refreshed, 1);
    }

    #[test]
    fn a_failed_row_stays_failed_when_refreshed() {
        let mut failed = row("1", "T:/a", "Error");
        failed.error_log = Some("boom".to_string());
        let (rows, summary) = merge(vec![failed], vec![scanned("T:/a", 450)], &[]);
        assert_eq!(rows[0].status, "Error");
        assert_eq!(rows[0].error_log.as_deref(), Some("boom"));
        assert_eq!(rows[0].clip.frame_count, 450);
        assert_eq!(summary.refreshed, 1);
    }

    #[test]
    fn a_new_take_is_added_after_the_rest_with_an_unused_id() {
        let existing = vec![row("0", "T:/a", "Rendering"), row("7", "T:/b", "Queued")];
        let mut rendered = scanned("T:/d", 100);
        rendered.finished_output = Some(("E:/out/d.mov".to_string(), Some(5)));
        let (rows, summary) = merge(
            existing,
            vec![
                scanned("T:/c", 100),
                scanned("T:/a", 100),
                rendered,
                scanned("T:/b", 100),
            ],
            &[],
        );
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["0", "7", "8", "9"]);
        assert_eq!(rows[2].clip.take_folder, "T:/c");
        assert_eq!(rows[2].status, "Queued");
        assert_eq!(
            rows[2].codec,
            RenderCodec::H264Software,
            "the panel's settings"
        );
        assert_eq!(rows[2].fps, 60);
        assert_eq!(
            rows[3].status, "Finished",
            "rendered before: staged Finished"
        );
        assert_eq!(rows[3].output_path, "E:/out/d.mov");
        assert_eq!(summary.added, 2);
        assert_eq!(summary.kept, 2);
        assert_eq!(summary.total, 4);
    }

    #[test]
    fn a_take_whose_folder_is_gone_is_dropped() {
        let existing = vec![
            row("0", "T:/finished", "Finished"),
            row("1", "T:/queued", "Queued"),
            row("2", "T:/kept", "Cancelled"),
        ];
        let (rows, summary) = merge(existing, vec![], &["T:/finished", "T:/queued"]);
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            ids,
            ["2"],
            "a row not found whose folder is still there stays"
        );
        assert_eq!(summary.gone, 2);
        assert_eq!(summary.gone_queued, 1);
        assert_eq!(summary.kept, 1);
    }

    #[test]
    fn a_rendering_row_whose_folder_is_gone_is_kept() {
        let (rows, summary) = merge(vec![row("0", "T:/a", "Rendering")], vec![], &["T:/a"]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "Rendering");
        assert_eq!(summary.gone, 0);
        assert_eq!(summary.kept, 1);
    }

    #[test]
    fn rows_are_matched_by_take_and_stream_not_by_position() {
        let mut hud = row("1", "T:/a", "Finished");
        hud.clip.clip_type = "hud_only".to_string();
        hud.clip.img_folder = "hudcolor".to_string();
        let existing = vec![row("0", "T:/a", "Cancelled"), hud];
        let mut hud_scan = scanned("T:/a", 10);
        hud_scan.clip.clip_type = "hud_only".to_string();
        hud_scan.clip.img_folder = "hudcolor".to_string();
        let (rows, summary) = merge(existing, vec![hud_scan, scanned("T:/a", 20)], &[]);
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].id.as_str(), rows[0].clip.frame_count), ("0", 20));
        assert_eq!((rows[1].id.as_str(), rows[1].clip.frame_count), ("1", 10));
        assert_eq!(summary.refreshed, 2);
        assert_eq!(summary.added, 0);
    }

    #[test]
    fn a_take_found_twice_is_one_row() {
        let (rows, summary) = merge(vec![], vec![scanned("T:/a", 1), scanned("T:/a", 1)], &[]);
        assert_eq!(rows.len(), 1);
        assert_eq!(summary.added, 1);
    }

    #[test]
    fn a_queued_recovered_stub_gets_its_take_filled_in() {
        let mut stub = row("0", "T:/a", "Queued");
        stub.clip.img_folder = String::new();
        stub.clip.frame_count = 0;
        stub.codec = RenderCodec::DnxHr;
        let (rows, summary) = merge(vec![stub], vec![scanned("T:/a", 300)], &[]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].clip.img_folder, "all");
        assert_eq!(rows[0].codec, RenderCodec::DnxHr);
        assert_eq!(summary.added, 0);
    }

    #[test]
    fn new_ids_continue_past_the_counter_when_it_is_higher() {
        let mut next_id = 20;
        let (rows, _) = merge_scan(
            vec![row("3", "T:/a", "Finished")],
            vec![scanned("T:/b", 1)],
            &HashSet::new(),
            &defaults(),
            &mut next_id,
        );
        assert_eq!(rows[1].id, "20");
        assert_eq!(next_id, 21);
    }
}
