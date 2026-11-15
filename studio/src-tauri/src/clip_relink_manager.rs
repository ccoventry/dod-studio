//! Find Moved Videos (#1442): points the project's rendered clips back at
//! their files by the DoD Studio tag on each video. The scan itself is
//! `native::hlcr::clip_id::relink`; this parses the request, picks the
//! folders (Render Settings' export folders, plus one the user picked), sends
//! the progress and logs the start and the end.

use crate::settings_manager::SettingsManager;
use native::hlcr::clip_id::ClipTag;
use native::hlcr::clip_id::relink::{
    FoundBy, RecordedClip, RelinkReport, RelinkStep, relink_clips,
};
use native::log_markdown;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::AtomicU32;
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager};

/// One rendered clip as the project recorded it (`take_renders.js`).
#[derive(Deserialize)]
pub struct ClipToFind {
    take_key: String,
    started_unix_ms: u64,
    stream: String,
    output_path: String,
    #[serde(default)]
    output_size_bytes: Option<u64>,
    #[serde(default)]
    content_hash: Option<String>,
}

impl ClipToFind {
    fn into_recorded(self) -> RecordedClip {
        RecordedClip {
            tag: ClipTag::new(&self.take_key, self.started_unix_ms, &self.stream),
            path: self.output_path,
            size_bytes: self.output_size_bytes,
            content_hash: self.content_hash,
        }
    }
}

/// The `clip_relink_progress` event: which step, and how far.
#[derive(Serialize, Clone, Debug, PartialEq)]
struct RelinkProgress {
    /// `tagging`, `listing` or `checking`.
    step: &'static str,
    done: usize,
    /// 0 while listing, when the total isn't known yet.
    total: usize,
}

impl From<RelinkStep> for RelinkProgress {
    fn from(step: RelinkStep) -> Self {
        match step {
            RelinkStep::Tagging { done, total } => Self {
                step: "tagging",
                done,
                total,
            },
            RelinkStep::Listing { files } => Self {
                step: "listing",
                done: files,
                total: 0,
            },
            RelinkStep::Checking { done, total } => Self {
                step: "checking",
                done,
                total,
            },
        }
    }
}

/// The folders to search: the export folders, then `extra` (the folder the
/// user picked), blanks dropped.
fn search_folders(export_dirs: &[String], extra: Option<&str>) -> Vec<PathBuf> {
    export_dirs
        .iter()
        .map(String::as_str)
        .chain(extra)
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// The activity log's counts for `report`.
fn counts_of(report: &RelinkReport) -> crate::messages::clip_relink::ScanCounts {
    let count = |how: FoundBy| report.clips.iter().filter(|c| c.found_by == how).count();
    crate::messages::clip_relink::ScanCounts {
        in_place: count(FoundBy::InPlace),
        by_file_tag: count(FoundBy::FileTag),
        by_stream_tag: count(FoundBy::StreamTag),
        by_fingerprint: count(FoundBy::Fingerprint),
        missing: count(FoundBy::Missing),
        files_checked: report.files_checked,
        tags_written: report.stream_tags_written,
        tags_failed: report.stream_tags_failed,
        search_complete: report.search_complete,
    }
}

/// Finds each of `clips` (the project's rendered videos) where it was
/// rendered or under the export folders and `extra_folder`, tagging the ones
/// it places, and reports where each is now, in order. Sends
/// `clip_relink_progress` at most every ~33 ms. Fails only when the picked
/// folder isn't there or the background task crashed. Writes only stream
/// tags, never a video's bytes.
#[tauri::command]
pub async fn relink_rendered_clips(
    app: AppHandle,
    clips: Vec<ClipToFind>,
    extra_folder: Option<String>,
) -> Result<RelinkReport, String> {
    if let Some(folder) = extra_folder.as_deref()
        && !PathBuf::from(folder).is_dir()
    {
        return Err(crate::messages::clip_relink::folder_not_found(
            PathBuf::from(folder).display(),
        ));
    }
    let export_dirs = {
        let settings = app.state::<SettingsManager>();
        let s = settings.inner.lock().unwrap_or_else(|p| p.into_inner());
        s.render_export_dirs.clone()
    };
    let folders = search_folders(&export_dirs, extra_folder.as_deref());
    let recorded: Vec<RecordedClip> = clips.into_iter().map(ClipToFind::into_recorded).collect();
    log_markdown(&crate::messages::clip_relink::scan_started(
        recorded.len(),
        folders.len(),
    ));
    let handle = tokio::task::spawn_blocking(move || {
        let started = Instant::now();
        let last_emit = AtomicU32::new(u32::MAX);
        let report = relink_clips(&recorded, &folders, &mut |step| {
            let now_ms = started.elapsed().as_millis().min(u32::MAX as u128 - 1) as u32;
            if crate::scan_manager::progress_emit_due(&last_emit, now_ms) {
                let _ = app.emit("clip_relink_progress", RelinkProgress::from(step));
            }
        });
        log_markdown(&crate::messages::clip_relink::scan_finished(
            &counts_of(&report),
            started.elapsed().as_millis(),
        ));
        report
    });
    crate::messages::spawn_blocking_result(handle).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_picked_folder_is_searched_after_the_export_folders() {
        let folders = search_folders(&["D:/exports".into(), "  ".into()], Some("E:/sorted"));
        assert_eq!(
            folders,
            vec![PathBuf::from("D:/exports"), PathBuf::from("E:/sorted")]
        );
        assert!(search_folders(&[], None).is_empty());
    }

    #[test]
    fn a_recorded_clip_becomes_the_tag_its_render_wrote() {
        let clip = ClipToFind {
            take_key: "session/take_0001".into(),
            started_unix_ms: 42,
            stream: "all".into(),
            output_path: "D:/exports/clip.mov".into(),
            output_size_bytes: Some(10),
            content_hash: None,
        };
        let recorded = clip.into_recorded();
        assert_eq!(recorded.tag.to_string(), "v1/session/take_0001/42/all");
        assert_eq!(recorded.size_bytes, Some(10));
    }

    #[test]
    fn listing_progress_has_no_total_yet() {
        assert_eq!(
            RelinkProgress::from(RelinkStep::Listing { files: 7 }),
            RelinkProgress {
                step: "listing",
                done: 7,
                total: 0
            }
        );
    }
}
