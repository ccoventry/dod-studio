//! The scan that points a project's rendered clips back at their files after
//! the user moved or renamed them (#1442).
//!
//! Each clip the project recorded is looked at where it was rendered first.
//! One still there is tagged in place (the stream tag, when it has none) and
//! its fingerprint taken when the project has none, so a later move can be
//! found. The rest are looked for under the search folders: every video file
//! is asked for its tag inside the file, then its stream tag, and only a file
//! with neither is matched by size and fingerprint. A file whose tag names a
//! render this project does not have (another project's clip) is left alone,
//! and so is a fingerprint two recorded clips share.
//!
//! A file whose data is only in the cloud (OneDrive and the like) is never
//! read: reading its tag would download it. It is counted instead.

use super::{ClipTag, METADATA_KEY, fingerprint, mov_tags, stream_tag};
use crate::hlcr::renderer::PARTIAL_MARK;
use crate::sys::cloud_file::CloudCheck;
use crate::sys::folder_walk::{Kind, Step, Walker};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// How deep under a search folder videos are looked for: the folder and five
/// levels of subfolders (a per-project, per-day, per-team folder tree).
pub const SEARCH_DEPTH: usize = 6;

/// Entries read across all search folders before the listing stops, so a
/// folder picked at a drive root ends in seconds.
pub const SEARCH_ENTRY_BUDGET: usize = 200_000;

/// Extensions a render can have (`renderer::ffmpeg_args::encode_settings`,
/// and an OBS take's own file copied by Skip).
const VIDEO_EXTENSIONS: [&str; 5] = ["mov", "mp4", "mkv", "avi", "m4v"];

/// One rendered clip as the project recorded it.
#[derive(Debug, Clone)]
pub struct RecordedClip {
    pub tag: ClipTag,
    /// Where it was rendered, or where the last scan found it.
    pub path: String,
    pub size_bytes: Option<u64>,
    pub content_hash: Option<String>,
}

/// How a clip was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FoundBy {
    /// Still at its recorded path.
    InPlace,
    /// Elsewhere, by the tag inside the file.
    FileTag,
    /// Elsewhere, by its stream tag.
    StreamTag,
    /// Elsewhere, by size and fingerprint.
    Fingerprint,
    /// A later render of the same take and stream was recorded at the same
    /// path, so this render's file was replaced: not looked for, and not
    /// counted as not found.
    Superseded,
    Missing,
}

/// Where one recorded clip is now, in the order the clips were given.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RelinkedClip {
    /// The file it is at now; `None` when it is missing or superseded.
    pub path: Option<String>,
    pub found_by: FoundBy,
    /// The file's size and fingerprint when found (taken again when the
    /// project had none or the size changed), else what the project had.
    pub size_bytes: Option<u64>,
    pub content_hash: Option<String>,
}

/// What a scan found.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RelinkReport {
    pub clips: Vec<RelinkedClip>,
    /// Video files under the search folders that were checked.
    pub files_checked: usize,
    /// False when the listing stopped at its depth or entry budget, so a
    /// missing clip may be further down.
    pub search_complete: bool,
    pub stream_tags_written: usize,
    /// Clips whose stream tag could not be written (a FAT/exFAT drive, a
    /// read-only or locked file). Their other tags still find them.
    pub stream_tags_failed: usize,
    /// Files left unread because their data is only in the cloud.
    pub cloud_files_skipped: usize,
    /// The scan was cancelled; what it found before that still stands.
    pub cancelled: bool,
}

/// Where a scan is, for the progress bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelinkStep {
    /// Looking at the clips' recorded paths, tagging those still there.
    Tagging { done: usize, total: usize },
    /// Listing the search folders: video files found so far.
    Listing { files: usize },
    /// Reading the tags of the listed files.
    Checking { done: usize, total: usize },
}

/// One scan's shared state: the clips, the cancel flag, the report it fills
/// and the cloud check that keeps it from reading online-only files.
struct Scan<'a> {
    clips: &'a [RecordedClip],
    cancel: &'a AtomicBool,
    cloud: CloudCheck,
    report: RelinkReport,
}

/// Finds every clip of `clips` under its recorded path or under `roots`,
/// tagging the ones it places. Writes only stream tags (never a video's
/// bytes); reads every video file under `roots` up to [`SEARCH_DEPTH`],
/// except online-only ones. Stops early, with `cancelled` set, once `cancel`
/// is set.
pub fn relink_clips(
    clips: &[RecordedClip],
    roots: &[PathBuf],
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(RelinkStep),
) -> RelinkReport {
    let mut scan = Scan {
        clips,
        cancel,
        cloud: CloudCheck::new(),
        report: RelinkReport {
            clips: clips.iter().map(missing).collect(),
            files_checked: 0,
            search_complete: true,
            stream_tags_written: 0,
            stream_tags_failed: 0,
            cloud_files_skipped: 0,
            cancelled: false,
        },
    };
    for i in superseded(clips) {
        scan.report.clips[i].found_by = FoundBy::Superseded;
    }
    scan.check_recorded_paths(progress);
    let wanted: Vec<usize> = (0..clips.len())
        .filter(|&i| scan.report.clips[i].found_by == FoundBy::Missing)
        .collect();
    if wanted.is_empty() || scan.stopped() {
        return scan.report;
    }
    let in_place: HashSet<String> = scan
        .report
        .clips
        .iter()
        .filter_map(|c| c.path.as_deref().map(str::to_lowercase))
        .collect();
    let candidates = scan.list_videos(roots, &in_place, progress);
    for (index, path, found_by) in scan.match_candidates(&wanted, &candidates, progress) {
        scan.report.clips[index] = scan.place(&clips[index], &path, found_by);
    }
    scan.report
}

/// A clip's entry before it is found: missing, with what the project had.
fn missing(clip: &RecordedClip) -> RelinkedClip {
    RelinkedClip {
        path: None,
        found_by: FoundBy::Missing,
        size_bytes: clip.size_bytes,
        content_hash: clip.content_hash.clone(),
    }
}

/// The clips a later render of the same take and stream replaced: recorded
/// at the same path (any case) with an earlier start. A re-render after the
/// old video was deleted gets the old name, and only the newest render is
/// that file.
fn superseded(clips: &[RecordedClip]) -> Vec<usize> {
    let key = |c: &RecordedClip| {
        (
            c.tag.take_key().to_string(),
            c.tag.stream().to_string(),
            c.path.to_lowercase(),
        )
    };
    let mut newest: HashMap<(String, String, String), u64> = HashMap::new();
    for clip in clips.iter().filter(|c| !c.path.is_empty()) {
        let start = newest.entry(key(clip)).or_insert(0);
        *start = (*start).max(clip.tag.started_unix_ms());
    }
    (0..clips.len())
        .filter(|&i| {
            !clips[i].path.is_empty() && newest[&key(&clips[i])] > clips[i].tag.started_unix_ms()
        })
        .collect()
}

/// The tag on `file`, and which of the two it was: inside the file first,
/// then the stream.
fn tag_on(file: &Path) -> Option<(ClipTag, FoundBy)> {
    if let Some(tag) = mov_tags::read_mov_tag(file, METADATA_KEY).and_then(|t| ClipTag::parse(&t)) {
        return Some((tag, FoundBy::FileTag));
    }
    stream_tag::read_stream_tag(file)
        .and_then(|t| ClipTag::parse(&t))
        .map(|tag| (tag, FoundBy::StreamTag))
}

/// Whether the file at `path` is `clip`: it carries `clip`'s tag, or no tag
/// and the size the project recorded (when it recorded one). A file tagged
/// as another render, or untagged with another size, is a different video
/// that took the name.
fn is_clip_at(clip: &RecordedClip, path: &Path) -> bool {
    match tag_on(path) {
        Some((tag, _)) => tag == clip.tag,
        None => clip
            .size_bytes
            .is_none_or(|size| std::fs::metadata(path).is_ok_and(|m| m.len() == size)),
    }
}

/// Whether `path` is a finished video a render could have made. A render
/// still being written (`<name>.partial<ext>`, #1439) is never a finished
/// clip, so never matched.
fn is_video(path: &Path) -> bool {
    let is_partial = path.file_stem().is_some_and(|stem| {
        stem.to_string_lossy()
            .to_lowercase()
            .ends_with(PARTIAL_MARK)
    });
    !is_partial
        && path.extension().is_some_and(|ext| {
            VIDEO_EXTENSIONS
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
}

impl Scan<'_> {
    /// Whether the scan was cancelled; records it in the report.
    fn stopped(&mut self) -> bool {
        if self.cancel.load(Ordering::Relaxed) {
            self.report.cancelled = true;
        }
        self.report.cancelled
    }

    /// Looks at each wanted clip's recorded path and places the ones still
    /// there. An online-only file there is taken as the clip, unread.
    fn check_recorded_paths(&mut self, progress: &mut dyn FnMut(RelinkStep)) {
        let clips = self.clips;
        for (i, clip) in clips.iter().enumerate() {
            if self.stopped() {
                return;
            }
            progress(RelinkStep::Tagging {
                done: i,
                total: clips.len(),
            });
            let path = PathBuf::from(&clip.path);
            if self.report.clips[i].found_by != FoundBy::Missing
                || clip.path.is_empty()
                || !path.is_file()
            {
                continue;
            }
            if self.cloud.is_online_only(&path) {
                self.report.cloud_files_skipped += 1;
                self.report.clips[i] = RelinkedClip {
                    path: Some(path.display().to_string()),
                    found_by: FoundBy::InPlace,
                    ..missing(clip)
                };
            } else if is_clip_at(clip, &path) {
                self.report.clips[i] = self.place(clip, &path, FoundBy::InPlace);
            }
        }
    }

    /// `clip`'s entry once found at `path`: the stream tag written when the
    /// file lacks it, and the size and fingerprint taken again when the
    /// project has none or the file's size is not the recorded one.
    fn place(&mut self, clip: &RecordedClip, path: &Path, found_by: FoundBy) -> RelinkedClip {
        let tag = clip.tag.to_string();
        if stream_tag::read_stream_tag(path).as_deref() != Some(tag.as_str()) {
            match stream_tag::write_stream_tag(path, &tag) {
                Ok(()) => self.report.stream_tags_written += 1,
                Err(_) => self.report.stream_tags_failed += 1,
            }
        }
        let size_bytes = std::fs::metadata(path).ok().map(|m| m.len());
        let same_size = size_bytes.is_some() && size_bytes == clip.size_bytes;
        let content_hash = clip
            .content_hash
            .clone()
            .filter(|_| same_size)
            .or_else(|| fingerprint::content_hash(path).ok());
        RelinkedClip {
            path: Some(path.display().to_string()),
            found_by,
            size_bytes: size_bytes.or(clip.size_bytes),
            content_hash,
        }
    }

    /// Every video file under `roots` that is not one of the `in_place`
    /// clips' own files (lowercase paths) nor online-only (counted instead).
    /// Notes in the report whether the listing finished.
    fn list_videos(
        &mut self,
        roots: &[PathBuf],
        in_place: &HashSet<String>,
        progress: &mut dyn FnMut(RelinkStep),
    ) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let mut seen = HashSet::new();
        let cancel = self.cancel;
        let cloud = &mut self.cloud;
        let mut cloud_skipped = 0;
        let outcome = Walker::new()
            .breadth_first()
            .skip_system_folders()
            .max_depth(SEARCH_DEPTH)
            .max_entries(SEARCH_ENTRY_BUDGET)
            .walk_roots(roots, &mut |entry| {
                if cancel.load(Ordering::Relaxed) {
                    return Step::Stop;
                }
                if entry.kind == Kind::File && is_video(&entry.path) {
                    let key = entry.path.to_string_lossy().to_lowercase();
                    // Two search folders where one holds the other list a file twice.
                    if !in_place.contains(&key) && seen.insert(key) {
                        if cloud.is_online_only(&entry.path) {
                            cloud_skipped += 1;
                        } else {
                            files.push(entry.path.clone());
                            progress(RelinkStep::Listing { files: files.len() });
                        }
                    }
                }
                Step::Enter
            });
        self.report.cloud_files_skipped += cloud_skipped;
        self.report.search_complete = outcome.complete;
        files
    }

    /// Which of `candidates` is which wanted clip: `(clip index, file, how)`.
    /// Tags win outright; a file with no tag matches by size and fingerprint
    /// only when exactly one wanted clip has that pair.
    fn match_candidates(
        &mut self,
        wanted: &[usize],
        candidates: &[PathBuf],
        progress: &mut dyn FnMut(RelinkStep),
    ) -> Vec<(usize, PathBuf, FoundBy)> {
        let clips = self.clips;
        let mut by_tag: HashMap<&ClipTag, usize> =
            wanted.iter().map(|&i| (&clips[i].tag, i)).collect();
        let mut by_size: HashMap<u64, Vec<usize>> = HashMap::new();
        for &i in wanted {
            if let (Some(size), Some(_)) = (clips[i].size_bytes, &clips[i].content_hash) {
                by_size.entry(size).or_default().push(i);
            }
        }
        let mut found = Vec::new();
        let mut fingerprinted: HashMap<usize, Vec<PathBuf>> = HashMap::new();
        for (done, file) in candidates.iter().enumerate() {
            if by_tag.is_empty() || self.stopped() {
                break;
            }
            progress(RelinkStep::Checking {
                done,
                total: candidates.len(),
            });
            // Counts the files read, not the files listed: a cancelled scan,
            // or one whose clips were all found early, read fewer.
            self.report.files_checked += 1;
            match tag_on(file) {
                Some((tag, how)) => {
                    if let Some(i) = by_tag.remove(&tag) {
                        found.push((i, file.clone(), how));
                    }
                }
                None => {
                    if let Some(i) = fingerprint_match(clips, &by_size, file) {
                        fingerprinted.entry(i).or_default().push(file.clone());
                    }
                }
            }
        }
        // A fingerprint is trusted for a clip no tag found, and only when one
        // file has it (two copies of one clip cannot say which the project uses).
        let tagged: HashSet<usize> = found.iter().map(|(i, _, _)| *i).collect();
        for (i, mut files) in fingerprinted {
            if !tagged.contains(&i) && files.len() == 1 {
                found.push((i, files.remove(0), FoundBy::Fingerprint));
            }
        }
        found
    }
}

/// The one wanted clip whose recorded size and fingerprint `file` has.
fn fingerprint_match(
    clips: &[RecordedClip],
    by_size: &HashMap<u64, Vec<usize>>,
    file: &Path,
) -> Option<usize> {
    let size = std::fs::metadata(file).ok()?.len();
    let same_size = by_size.get(&size)?;
    let hash = fingerprint::content_hash(file).ok()?;
    let mut matching = same_size
        .iter()
        .filter(|&&i| clips[i].content_hash.as_deref() == Some(hash.as_str()));
    let first = *matching.next()?;
    matching.next().is_none().then_some(first)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    fn clip(take: &str, path: &Path) -> RecordedClip {
        RecordedClip {
            tag: ClipTag::new(take, 1_000, "all"),
            path: path.display().to_string(),
            size_bytes: None,
            content_hash: None,
        }
    }

    fn write(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    fn run(clips: &[RecordedClip], roots: &[PathBuf]) -> RelinkReport {
        relink_clips(clips, roots, &AtomicBool::new(false), &mut |_| {})
    }

    #[test]
    fn a_clip_still_in_place_is_tagged_and_fingerprinted() {
        let dir = Scratch::new("relink_in_place");
        let video = dir.path().join("clip.mov");
        write(&video, b"video");
        let clips = [clip("s/t1", &video)];
        let report = run(&clips, &[]);
        let found = &report.clips[0];
        assert_eq!(found.found_by, FoundBy::InPlace);
        assert_eq!(found.size_bytes, Some(5));
        assert_eq!(found.content_hash, fingerprint::content_hash(&video).ok());
        assert_eq!(report.stream_tags_written, 1);
        assert_eq!(
            stream_tag::read_stream_tag(&video),
            Some(clips[0].tag.to_string())
        );
    }

    #[test]
    fn a_tagged_clip_renamed_into_a_subfolder_is_found_by_its_stream_tag() {
        let dir = Scratch::new("relink_renamed");
        let old = dir.path().join("exports").join("clip.mov");
        write(&old, b"video");
        let clips = [clip("s/t1", &old)];
        run(&clips, &[]);
        let moved = dir
            .path()
            .join("exports")
            .join("monday")
            .join("best kill.mov");
        std::fs::create_dir_all(moved.parent().unwrap()).unwrap();
        std::fs::rename(&old, &moved).unwrap();
        let report = run(&clips, &[dir.path().join("exports")]);
        assert_eq!(report.clips[0].found_by, FoundBy::StreamTag);
        assert_eq!(report.clips[0].path, Some(moved.display().to_string()));
        assert_eq!(report.stream_tags_written, 0);
    }

    #[test]
    fn a_clip_with_no_tag_is_found_by_size_and_fingerprint() {
        let dir = Scratch::new("relink_fingerprint");
        let moved = dir.path().join("sorted").join("renamed.mov");
        write(&moved, b"untagged video");
        let mut recorded = clip("s/t1", &dir.path().join("gone.mov"));
        recorded.size_bytes = Some(14);
        recorded.content_hash = fingerprint::content_hash(&moved).ok();
        let report = run(&[recorded], &[dir.path().to_path_buf()]);
        assert_eq!(report.clips[0].found_by, FoundBy::Fingerprint);
        assert_eq!(report.clips[0].path, Some(moved.display().to_string()));
    }

    #[test]
    fn a_tag_wins_over_a_fingerprint() {
        let dir = Scratch::new("relink_order");
        // Byte-identical files: one carries the clip's tag, the other none.
        let copy = dir.path().join("a").join("copy.mov");
        let tagged = dir.path().join("b").join("tagged.mov");
        write(&copy, b"same bytes");
        write(&tagged, b"same bytes");
        let mut recorded = clip("s/t1", &dir.path().join("gone.mov"));
        stream_tag::write_stream_tag(&tagged, &recorded.tag.to_string()).unwrap();
        recorded.size_bytes = Some(10);
        recorded.content_hash = fingerprint::content_hash(&copy).ok();
        let report = run(&[recorded], &[dir.path().to_path_buf()]);
        assert_eq!(report.clips[0].found_by, FoundBy::StreamTag);
        assert_eq!(report.clips[0].path, Some(tagged.display().to_string()));
    }

    #[test]
    fn a_file_tagged_for_another_project_is_left_alone() {
        let dir = Scratch::new("relink_other_project");
        let other = dir.path().join("other.mov");
        write(&other, b"same bytes");
        stream_tag::write_stream_tag(&other, &ClipTag::new("x/y", 5, "all").to_string()).unwrap();
        let mut recorded = clip("s/t1", &dir.path().join("gone.mov"));
        recorded.size_bytes = Some(10);
        recorded.content_hash = fingerprint::content_hash(&other).ok();
        let report = run(&[recorded], &[dir.path().to_path_buf()]);
        assert_eq!(report.clips[0].found_by, FoundBy::Missing);
        assert_eq!(report.files_checked, 1);
        assert_eq!(
            stream_tag::read_stream_tag(&other),
            Some("v1/x/y/5/all".to_string())
        );
    }

    #[test]
    fn a_file_at_the_recorded_path_tagged_as_another_render_is_not_the_clip() {
        let dir = Scratch::new("relink_name_taken");
        let path = dir.path().join("clip.mov");
        write(&path, b"another render");
        stream_tag::write_stream_tag(&path, "v1/s/t1/2000/all").unwrap();
        let report = run(&[clip("s/t1", &path)], &[]);
        assert_eq!(report.clips[0].found_by, FoundBy::Missing);
    }

    #[test]
    fn two_files_with_one_fingerprint_find_nothing() {
        let dir = Scratch::new("relink_two_copies");
        write(&dir.path().join("a.mov"), b"same bytes");
        write(&dir.path().join("b.mov"), b"same bytes");
        let mut recorded = clip("s/t1", &dir.path().join("gone").join("clip.mov"));
        recorded.size_bytes = Some(10);
        recorded.content_hash = fingerprint::content_hash(&dir.path().join("a.mov")).ok();
        let report = run(&[recorded], &[dir.path().to_path_buf()]);
        assert_eq!(report.clips[0].found_by, FoundBy::Missing);
    }

    #[test]
    fn progress_counts_the_clips_then_the_files() {
        let dir = Scratch::new("relink_progress");
        write(&dir.path().join("x.mov"), b"x");
        write(&dir.path().join("notes.txt"), b"not a video");
        let clips = [clip("s/t1", &dir.path().join("gone.mov"))];
        let mut steps = Vec::new();
        relink_clips(
            &clips,
            &[dir.path().to_path_buf()],
            &AtomicBool::new(false),
            &mut |s| steps.push(s),
        );
        assert_eq!(
            steps,
            vec![
                RelinkStep::Tagging { done: 0, total: 1 },
                RelinkStep::Listing { files: 1 },
                RelinkStep::Checking { done: 0, total: 1 },
            ]
        );
    }

    #[test]
    fn an_untagged_file_two_renders_share_goes_to_the_newest_and_the_older_is_superseded() {
        let dir = Scratch::new("relink_shared_path");
        let video = dir.path().join("clip.mov");
        write(&video, b"re-rendered video");
        let older = clip("s/t1", &video);
        let newer = RecordedClip {
            tag: ClipTag::new("s/t1", 2_000, "all"),
            ..clip("s/t1", &video)
        };
        let report = run(&[older, newer.clone()], &[dir.path().to_path_buf()]);
        assert_eq!(report.clips[0].found_by, FoundBy::Superseded);
        assert_eq!(report.clips[0].path, None);
        assert_eq!(report.clips[1].found_by, FoundBy::InPlace);
        assert_eq!(
            stream_tag::read_stream_tag(&video),
            Some(newer.tag.to_string())
        );
    }

    #[test]
    fn an_untagged_file_of_another_size_at_the_recorded_path_is_not_the_clip() {
        let dir = Scratch::new("relink_other_size");
        let video = dir.path().join("clip.mov");
        write(&video, b"some other video");
        let mut recorded = clip("s/t1", &video);
        recorded.size_bytes = Some(3);
        let report = run(&[recorded], &[]);
        assert_eq!(report.clips[0].found_by, FoundBy::Missing);
        assert_eq!(stream_tag::read_stream_tag(&video), None);
    }

    #[test]
    fn a_tagged_clip_whose_size_changed_is_fingerprinted_again() {
        let dir = Scratch::new("relink_size_changed");
        let video = dir.path().join("clip.mov");
        write(&video, b"video, trimmed");
        let mut recorded = clip("s/t1", &video);
        stream_tag::write_stream_tag(&video, &recorded.tag.to_string()).unwrap();
        recorded.size_bytes = Some(99);
        recorded.content_hash = Some("stale".into());
        let report = run(&[recorded], &[]);
        assert_eq!(report.clips[0].found_by, FoundBy::InPlace);
        assert_eq!(report.clips[0].size_bytes, Some(14));
        assert_eq!(
            report.clips[0].content_hash,
            fingerprint::content_hash(&video).ok()
        );
    }

    #[test]
    fn a_same_size_clip_keeps_its_recorded_fingerprint() {
        let dir = Scratch::new("relink_same_size");
        let video = dir.path().join("clip.mov");
        write(&video, b"video");
        let mut recorded = clip("s/t1", &video);
        recorded.size_bytes = Some(5);
        recorded.content_hash = Some("kept".into());
        let report = run(&[recorded], &[]);
        assert_eq!(report.clips[0].content_hash.as_deref(), Some("kept"));
    }

    /// Marks `path` offline, as a cloud drive does a file whose data isn't on
    /// this PC, so the scan must not read it.
    #[cfg(windows)]
    fn mark_offline(path: &Path) {
        use std::os::windows::ffi::OsStrExt;
        unsafe extern "system" {
            fn SetFileAttributesW(file_name: *const u16, attributes: u32) -> i32;
        }
        const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: `wide` is a NUL-terminated path that outlives the call.
        assert_ne!(
            unsafe { SetFileAttributesW(wide.as_ptr(), FILE_ATTRIBUTE_OFFLINE) },
            0
        );
    }

    #[cfg(windows)]
    #[test]
    fn an_online_only_file_is_counted_and_never_read() {
        let dir = Scratch::new("relink_cloud");
        let moved = dir.path().join("sorted").join("renamed.mov");
        write(&moved, b"video");
        let recorded = clip("s/t1", &dir.path().join("gone.mov"));
        stream_tag::write_stream_tag(&moved, &recorded.tag.to_string()).unwrap();
        mark_offline(&moved);
        let report = run(&[recorded], &[dir.path().to_path_buf()]);
        assert_eq!(report.clips[0].found_by, FoundBy::Missing);
        assert_eq!(report.cloud_files_skipped, 1);
        assert_eq!(report.files_checked, 0);
    }

    #[test]
    fn a_cancelled_scan_stops_and_says_so() {
        let dir = Scratch::new("relink_cancel");
        write(&dir.path().join("x.mov"), b"x");
        let clips = [clip("s/t1", &dir.path().join("gone.mov"))];
        let report = relink_clips(
            &clips,
            &[dir.path().to_path_buf()],
            &AtomicBool::new(true),
            &mut |_| {},
        );
        assert!(report.cancelled);
        assert_eq!(report.clips[0].found_by, FoundBy::Missing);
        assert_eq!(report.files_checked, 0);
    }

    #[test]
    fn a_scan_cancelled_while_reading_counts_only_the_files_it_read() {
        let dir = Scratch::new("relink_cancel_reading");
        for name in ["a.mov", "b.mov", "c.mov"] {
            write(&dir.path().join(name), b"x");
        }
        let clips = [clip("s/t1", &dir.path().join("gone.mov"))];
        let cancel = AtomicBool::new(false);
        let report = relink_clips(&clips, &[dir.path().to_path_buf()], &cancel, &mut |step| {
            if matches!(step, RelinkStep::Checking { done: 0, .. }) {
                cancel.store(true, Ordering::Relaxed);
            }
        });
        assert!(report.cancelled);
        assert_eq!(report.files_checked, 1);
    }

    #[test]
    fn a_render_still_being_written_is_never_a_candidate() {
        assert!(is_video(Path::new("D:/exports/Kill.mov")));
        assert!(!is_video(Path::new("D:/exports/Kill.partial.mov")));
        assert!(!is_video(Path::new("D:/exports/Kill.PARTIAL.mp4")));
        assert!(!is_video(Path::new("D:/exports/notes.txt")));
    }
}
