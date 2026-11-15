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

use super::{ClipTag, METADATA_KEY, fingerprint, mov_tags, stream_tag};
use crate::sys::folder_walk::{Kind, Step, Walker};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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
    Missing,
}

/// Where one recorded clip is now, in the order the clips were given.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RelinkedClip {
    /// The file it is at now; `None` when it is missing.
    pub path: Option<String>,
    pub found_by: FoundBy,
    /// The file's size and fingerprint when found (taken now when the
    /// project had none), else what the project had.
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

/// Finds every clip of `clips` under its recorded path or under `roots`,
/// tagging the ones it places. Writes only stream tags (never a video's
/// bytes); reads every video file under `roots` up to [`SEARCH_DEPTH`].
pub fn relink_clips(
    clips: &[RecordedClip],
    roots: &[PathBuf],
    progress: &mut dyn FnMut(RelinkStep),
) -> RelinkReport {
    let mut report = RelinkReport {
        clips: clips.iter().map(missing).collect(),
        files_checked: 0,
        search_complete: true,
        stream_tags_written: 0,
        stream_tags_failed: 0,
    };
    for (i, clip) in clips.iter().enumerate() {
        progress(RelinkStep::Tagging {
            done: i,
            total: clips.len(),
        });
        let path = PathBuf::from(&clip.path);
        if is_clip_at(clip, &path) {
            report.clips[i] = place(clip, &path, FoundBy::InPlace, &mut report);
        }
    }
    let wanted: Vec<usize> = (0..clips.len())
        .filter(|&i| report.clips[i].found_by == FoundBy::Missing)
        .collect();
    if wanted.is_empty() {
        return report;
    }
    let in_place: HashSet<String> = report
        .clips
        .iter()
        .filter_map(|c| c.path.as_deref().map(str::to_lowercase))
        .collect();
    let (candidates, complete) = list_videos(roots, &in_place, progress);
    report.search_complete = complete;
    for (index, path, found_by) in match_candidates(clips, &wanted, &candidates, progress) {
        report.clips[index] = place(&clips[index], &path, found_by, &mut report);
    }
    report.files_checked = candidates.len();
    report
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

/// Whether the file at `path` is `clip`: a file is there and it carries no
/// tag, or `clip`'s. A file tagged as another render is a different video
/// that took the name.
fn is_clip_at(clip: &RecordedClip, path: &Path) -> bool {
    !clip.path.is_empty() && path.is_file() && tag_on(path).is_none_or(|(tag, _)| tag == clip.tag)
}

/// `clip`'s entry once found at `path`: the stream tag written when the
/// file lacks it, and the size and fingerprint filled in when the project
/// has none.
fn place(
    clip: &RecordedClip,
    path: &Path,
    found_by: FoundBy,
    report: &mut RelinkReport,
) -> RelinkedClip {
    let tag = clip.tag.to_string();
    if stream_tag::read_stream_tag(path).as_deref() != Some(tag.as_str()) {
        match stream_tag::write_stream_tag(path, &tag) {
            Ok(()) => report.stream_tags_written += 1,
            Err(_) => report.stream_tags_failed += 1,
        }
    }
    let size_bytes = std::fs::metadata(path)
        .ok()
        .map(|m| m.len())
        .or(clip.size_bytes);
    let content_hash = clip
        .content_hash
        .clone()
        .or_else(|| fingerprint::content_hash(path).ok());
    RelinkedClip {
        path: Some(path.display().to_string()),
        found_by,
        size_bytes,
        content_hash,
    }
}

/// Every video file under `roots` that is not one of the `in_place` clips'
/// own files (lowercase paths), and whether the listing finished.
fn list_videos(
    roots: &[PathBuf],
    in_place: &HashSet<String>,
    progress: &mut dyn FnMut(RelinkStep),
) -> (Vec<PathBuf>, bool) {
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    let outcome = Walker::new()
        .breadth_first()
        .skip_system_folders()
        .max_depth(SEARCH_DEPTH)
        .max_entries(SEARCH_ENTRY_BUDGET)
        .walk_roots(roots, &mut |entry| {
            if entry.kind == Kind::File && is_video(&entry.path) {
                let key = entry.path.to_string_lossy().to_lowercase();
                // Two search folders where one holds the other list a file twice.
                if !in_place.contains(&key) && seen.insert(key) {
                    files.push(entry.path.clone());
                    progress(RelinkStep::Listing { files: files.len() });
                }
            }
            Step::Enter
        });
    (files, outcome.complete)
}

fn is_video(path: &Path) -> bool {
    path.extension().is_some_and(|ext| {
        VIDEO_EXTENSIONS
            .iter()
            .any(|known| ext.eq_ignore_ascii_case(known))
    })
}

/// Which of `candidates` is which wanted clip: `(clip index, file, how)`.
/// Tags win outright; a file with no tag matches by size and fingerprint
/// only when exactly one wanted clip has that pair.
fn match_candidates(
    clips: &[RecordedClip],
    wanted: &[usize],
    candidates: &[PathBuf],
    progress: &mut dyn FnMut(RelinkStep),
) -> Vec<(usize, PathBuf, FoundBy)> {
    let mut by_tag: HashMap<&ClipTag, usize> = wanted.iter().map(|&i| (&clips[i].tag, i)).collect();
    let mut by_size: HashMap<u64, Vec<usize>> = HashMap::new();
    for &i in wanted {
        if let (Some(size), Some(_)) = (clips[i].size_bytes, &clips[i].content_hash) {
            by_size.entry(size).or_default().push(i);
        }
    }
    let mut found = Vec::new();
    let mut fingerprinted: HashMap<usize, Vec<PathBuf>> = HashMap::new();
    for (done, file) in candidates.iter().enumerate() {
        progress(RelinkStep::Checking {
            done,
            total: candidates.len(),
        });
        if by_tag.is_empty() {
            break;
        }
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
        relink_clips(clips, roots, &mut |_| {})
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
        relink_clips(&clips, &[dir.path().to_path_buf()], &mut |s| steps.push(s));
        assert_eq!(
            steps,
            vec![
                RelinkStep::Tagging { done: 0, total: 1 },
                RelinkStep::Listing { files: 1 },
                RelinkStep::Checking { done: 0, total: 1 },
            ]
        );
    }
}
