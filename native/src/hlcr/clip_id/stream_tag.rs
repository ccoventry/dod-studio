//! The clip tag kept in an NTFS alternate data stream, `clip.mov:dodstudio`
//! (#1442).
//!
//! Writing it never touches the video's own bytes, so clips rendered before
//! tags existed can be tagged in place. It survives a rename or a move on
//! NTFS and an Explorer copy between NTFS drives; FAT/exFAT drives, most
//! cloud sync and some tools drop it, which is why the in-file tag and the
//! fingerprint exist too. Writing a stream bumps the file's modified time on
//! NTFS, so [`write_stream_tag`] puts it back: an editor that keys its cache
//! on the time (VEGAS's peak files) sees the same file as before.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

/// The stream's name after the file's: `clip.mov:dodstudio`.
pub const STREAM_NAME: &str = "dodstudio";

/// A stream longer than this is not one Studio wrote.
const MAX_STREAM_BYTES: u64 = 1024;

/// The path that opens `file`'s tag stream.
fn stream_path(file: &Path) -> PathBuf {
    let mut path = OsString::from(file.as_os_str());
    path.push(":");
    path.push(STREAM_NAME);
    PathBuf::from(path)
}

/// The text in `file`'s tag stream, trimmed, or `None` when it has none, it
/// cannot be read or it is too big to be a tag.
pub fn read_stream_tag(file: &Path) -> Option<String> {
    let stream = stream_path(file);
    let size = std::fs::metadata(&stream).ok()?.len();
    if size == 0 || size > MAX_STREAM_BYTES {
        return None;
    }
    let text = std::fs::read_to_string(&stream).ok()?;
    Some(text.trim().to_string()).filter(|t| !t.is_empty())
}

/// Writes `tag` into `file`'s tag stream, replacing what was there, and
/// puts the file's modified time back. Fails on a drive with no streams
/// (FAT, exFAT) or a file that is read-only or locked.
pub fn write_stream_tag(file: &Path, tag: &str) -> io::Result<()> {
    let modified = std::fs::metadata(file)?.modified()?;
    std::fs::write(stream_path(file), tag.as_bytes())?;
    filetime::set_file_mtime(file, filetime::FileTime::from_system_time(modified))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    #[test]
    fn a_written_tag_reads_back_and_leaves_the_video_as_it_was() {
        let dir = Scratch::new("stream_tag_round_trip");
        let video = dir.path().join("clip.mov");
        std::fs::write(&video, b"video bytes").unwrap();
        let before = std::fs::metadata(&video).unwrap().modified().unwrap();
        write_stream_tag(&video, "v1/s/t/1/all").unwrap();
        assert_eq!(read_stream_tag(&video).as_deref(), Some("v1/s/t/1/all"));
        assert_eq!(std::fs::read(&video).unwrap(), b"video bytes");
        let after = std::fs::metadata(&video).unwrap().modified().unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn the_tag_follows_a_rename_into_a_subfolder() {
        let dir = Scratch::new("stream_tag_rename");
        let video = dir.path().join("clip.mov");
        std::fs::write(&video, b"x").unwrap();
        write_stream_tag(&video, "v1/s/t/1/all").unwrap();
        let sorted = dir.path().join("monday");
        std::fs::create_dir_all(&sorted).unwrap();
        let moved = sorted.join("renamed.mov");
        std::fs::rename(&video, &moved).unwrap();
        assert_eq!(read_stream_tag(&moved).as_deref(), Some("v1/s/t/1/all"));
    }

    #[test]
    fn a_file_with_no_stream_has_no_tag() {
        let dir = Scratch::new("stream_tag_none");
        let video = dir.path().join("clip.mov");
        std::fs::write(&video, b"x").unwrap();
        assert_eq!(read_stream_tag(&video), None);
        assert_eq!(read_stream_tag(&dir.path().join("gone.mov")), None);
    }
}
