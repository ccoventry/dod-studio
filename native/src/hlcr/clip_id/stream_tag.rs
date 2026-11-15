//! The clip tag kept in an NTFS alternate data stream, `clip.mov:dodstudio`
//! (#1442).
//!
//! Writing it never touches the video's own bytes, so clips rendered before
//! tags existed can be tagged in place. It survives a rename or a move on
//! NTFS and an Explorer copy between NTFS drives; FAT/exFAT drives, most
//! cloud sync and some tools drop it, which is why the in-file tag and the
//! fingerprint exist too. Writing a stream bumps the file's modified time on
//! NTFS, which an editor keying its cache on the time (VEGAS's peak files)
//! would take as a changed video, so [`write_stream_tag`] keeps it: the
//! stream's handle is told not to update the times before anything is
//! written, and the time is put back through an attributes-only handle if
//! it moved anyway. Neither needs the video itself opened for writing, so it
//! works while an editor has the video open (share modes are per stream).

use std::ffi::OsString;
use std::io::{self, Write};
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
/// leaves the file's modified time as it was. Fails on a drive with no
/// streams (FAT, exFAT) or a read-only file; works while another program has
/// the video open.
pub fn write_stream_tag(file: &Path, tag: &str) -> io::Result<()> {
    let modified = std::fs::metadata(file)?.modified()?;
    // Opened without truncating, so nothing changes before the times are
    // frozen; emptied after.
    let mut stream = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(stream_path(file))?;
    times::freeze(&stream)?;
    stream.set_len(0)?;
    stream.write_all(tag.as_bytes())?;
    drop(stream);
    if std::fs::metadata(file)?.modified()? != modified {
        times::restore_modified(file, modified)?;
    }
    Ok(())
}

#[cfg(windows)]
mod times {
    use std::ffi::c_void;
    use std::fs::File;
    use std::io;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;
    use std::time::SystemTime;

    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    unsafe extern "system" {
        fn SetFileTime(
            file: *mut c_void,
            creation: *const FileTime,
            last_access: *const FileTime,
            last_write: *const FileTime,
        ) -> i32;
    }

    /// All ones: "stop updating this time through this handle" (SetFileTime).
    const NO_UPDATES: FileTime = FileTime {
        low: u32::MAX,
        high: u32::MAX,
    };
    const FILE_WRITE_ATTRIBUTES: u32 = 0x100;
    const SHARE_ALL: u32 = 0x1 | 0x2 | 0x4;

    /// Stops writes through `handle` from updating the file's access and
    /// modified times.
    pub(super) fn freeze(handle: &File) -> io::Result<()> {
        // SAFETY: the handle is open for the call, and the pointers name a
        // live constant or are null (leave that time alone).
        let ok = unsafe {
            SetFileTime(
                handle.as_raw_handle(),
                std::ptr::null(),
                &NO_UPDATES,
                &NO_UPDATES,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Sets `file`'s modified time through a handle that may write only its
    /// attributes, which no other program's share mode refuses.
    pub(super) fn restore_modified(file: &Path, modified: SystemTime) -> io::Result<()> {
        std::fs::OpenOptions::new()
            .access_mode(FILE_WRITE_ATTRIBUTES)
            .share_mode(SHARE_ALL)
            .open(file)?
            .set_modified(modified)
    }
}

#[cfg(not(windows))]
mod times {
    use std::fs::File;
    use std::io;
    use std::path::Path;
    use std::time::SystemTime;

    pub(super) fn freeze(_handle: &File) -> io::Result<()> {
        Ok(())
    }

    pub(super) fn restore_modified(file: &Path, modified: SystemTime) -> io::Result<()> {
        std::fs::OpenOptions::new()
            .write(true)
            .open(file)?
            .set_modified(modified)
    }
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

    /// The video open in an editor that lets others only read it (NTFS share
    /// modes are per stream, so the stream still opens): the tag goes on and
    /// the modified time stays.
    #[cfg(windows)]
    #[test]
    fn a_video_held_open_read_only_gets_its_tag_and_keeps_its_time() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = Scratch::new("stream_tag_held_open");
        let video = dir.path().join("clip.mov");
        std::fs::write(&video, b"video bytes").unwrap();
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        std::fs::OpenOptions::new()
            .write(true)
            .open(&video)
            .unwrap()
            .set_modified(old)
            .unwrap();
        const FILE_SHARE_READ: u32 = 0x1;
        let _editor = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&video)
            .unwrap();
        write_stream_tag(&video, "v1/s/t/1/all").unwrap();
        write_stream_tag(&video, "v1/s/t/2/all").unwrap();
        assert_eq!(read_stream_tag(&video).as_deref(), Some("v1/s/t/2/all"));
        assert_eq!(std::fs::metadata(&video).unwrap().modified().unwrap(), old);
    }

    #[test]
    fn a_shorter_tag_replaces_a_longer_one_whole() {
        let dir = Scratch::new("stream_tag_shorter");
        let video = dir.path().join("clip.mov");
        std::fs::write(&video, b"x").unwrap();
        write_stream_tag(&video, "v1/session/take/123456/all").unwrap();
        write_stream_tag(&video, "v1/s/t/1/all").unwrap();
        assert_eq!(read_stream_tag(&video).as_deref(), Some("v1/s/t/1/all"));
    }
}
