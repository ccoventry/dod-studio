//! Reading just enough of a capture's files to answer the scanner's
//! questions: an AVI's frame count and size, a BMP's size, and whether a
//! video carries an audio stream. Header parsing only, no decoding.

use std::path::Path;

/// How much of an AVI to read looking for its header. The `hdrl` list sits at
/// the very front of the file, so this never has to grow — and it must not, as
/// the videos themselves run to gigabytes.
pub(super) const AVI_HEADER_SCAN_BYTES: usize = 64 * 1024;

/// Frame count read out of an AVI's own header, without decoding it.
///
/// The take's frame count drives the render progress percentage, and it used to
/// come from counting `.bmp` files — which returns 0 for a video take and left
/// video renders showing no progress at all. The count is in the header, so
/// this costs one bounded read rather than a decode pass or an ffprobe spawn
/// per take.
///
/// **The video stream's `strh.dwLength` is the authority, not
/// `avih.dwTotalFrames`.** AVI's legacy RIFF chunk tops out around 1 GiB, so
/// FFmpeg's muxer continues past that into OpenDML `AVIX` segments — and
/// `avih.dwTotalFrames` then counts only the frames in the *first* chunk, while
/// `strh.dwLength` carries the true total.
///
/// Measured across one capture, which shows the split exactly: the ~1.2 GB and
/// ~1.5 GB `all` streams read 1067/1218 and 859/1229 (avih/strh), and in both
/// cases the first chunk works out to almost exactly 1 GiB of the file. The
/// 135 MB HUD streams fit in a single chunk and agree with themselves. So the
/// error only appears on long or high-fps takes — the ones where an accurate
/// progress bar actually matters. `avih` is kept only as a fallback for a file
/// with no video `strh` at all.
///
/// Zero from both means the file was never finalised — a capture killed
/// mid-write — which is worth surfacing rather than papering over.
pub(super) fn avi_frame_count(path: &Path) -> Option<usize> {
    use std::io::Read;

    let mut file = std::fs::File::open(path).ok()?;
    // Heap-allocated and explicitly bounded: these files are gigabytes.
    let mut buf = vec![0u8; AVI_HEADER_SCAN_BYTES];
    let read = file.read(&mut buf).ok()?;
    buf.truncate(read);

    if buf.len() < 12 || &buf[0..4] != b"RIFF" || &buf[8..12] != b"AVI " {
        return None;
    }

    let u32_at = |b: &[u8], at: usize| -> Option<u32> {
        b.get(at..at + 4)
            .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    };

    // Walk the chunks inside the RIFF body. `hdrl` is a LIST, so descend into
    // it rather than skipping past; everything wanted lives inside it.
    let mut pos = 12usize;
    let mut total_frames = 0u32;
    let mut stream_length = 0u32;
    while pos + 8 <= buf.len() {
        let id = &buf[pos..pos + 4];
        let size = u32_at(&buf, pos + 4)? as usize;
        let body = pos + 8;

        match id {
            b"LIST" => {
                // Step inside: body starts with the list type, then chunks.
                pos = body + 4;
                continue;
            }
            b"avih" => {
                total_frames = u32_at(&buf, body + 16).unwrap_or(0);
            }
            b"strh"
                // Only the video stream's length is meaningful here; an audio
                // stream's dwLength counts samples or blocks, not frames.
                if buf.get(body..body + 4) == Some(b"vids") && stream_length == 0 => {
                    stream_length = u32_at(&buf, body + 32).unwrap_or(0);
                }
            _ => {}
        }

        // Chunks are word-aligned: an odd size is followed by a pad byte.
        pos = body + size + (size & 1);
    }

    let frames = if stream_length > 0 {
        stream_length
    } else {
        total_frames
    };
    Some(frames as usize)
}

/// An AVI's frame resolution, read from the video stream's `strf`
/// (`BITMAPINFOHEADER`) chunk — a separate chunk walk from `avi_frame_count`
/// rather than folding into it, so that function's existing return shape and
/// tests stay untouched. `biHeight` is signed and negative for a top-down
/// bitmap, hence the `.abs()`.
pub(super) fn avi_dimensions(path: &Path) -> Option<(u32, u32)> {
    use std::io::Read;

    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; AVI_HEADER_SCAN_BYTES];
    let read = file.read(&mut buf).ok()?;
    buf.truncate(read);

    if buf.len() < 12 || &buf[0..4] != b"RIFF" || &buf[8..12] != b"AVI " {
        return None;
    }

    let u32_at = |b: &[u8], at: usize| -> Option<u32> {
        b.get(at..at + 4)
            .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    };
    let i32_at = |b: &[u8], at: usize| -> Option<i32> {
        b.get(at..at + 4)
            .map(|s| i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    };

    let mut pos = 12usize;
    // `strf` describes whatever `strl`/`strh` it follows — this is only ever
    // set true right after a video `strh`, so the very next `strf` this
    // walk sees is guaranteed to be the video stream's, not audio's.
    let mut awaiting_video_strf = false;
    while pos + 8 <= buf.len() {
        let id = &buf[pos..pos + 4];
        let size = u32_at(&buf, pos + 4)? as usize;
        let body = pos + 8;

        match id {
            b"LIST" => {
                pos = body + 4;
                continue;
            }
            b"strh" => {
                awaiting_video_strf = buf.get(body..body + 4) == Some(b"vids");
            }
            b"strf" if awaiting_video_strf => {
                let width = u32_at(&buf, body + 4)?;
                let height = i32_at(&buf, body + 8)?.unsigned_abs();
                return Some((width, height));
            }
            _ => {}
        }

        pos = body + size + (size & 1);
    }
    None
}

/// A BMP frame sequence's resolution, read from the first frame's own
/// 54-byte header (`BITMAPFILEHEADER` + `BITMAPINFOHEADER`) — width/height
/// are little-endian `i32` at offsets 18/22. Same file `get_clip_date`
/// already opens for its metadata, and the same bounded-read shape as
/// `avi_frame_count` above.
pub(super) fn read_bmp_dimensions(folder: &Path) -> Option<(u32, u32)> {
    use std::io::Read;

    let mut file = std::fs::File::open(folder.join("00000.bmp")).ok()?;
    let mut buf = [0u8; 54];
    file.read_exact(&mut buf).ok()?;

    let i32_at =
        |at: usize| -> i32 { i32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]]) };
    let width = i32_at(18);
    let height = i32_at(22);
    if width <= 0 || height == 0 {
        return None;
    }
    Some((width as u32, height.unsigned_abs()))
}

/// How much of a container to read looking for an audio track.
///
/// Both ends, because the two formats disagree about where the index lives: an
/// AVI's `hdrl` is at the front, while an MP4 written without `+faststart` —
/// which is every OBS recording — puts `moov` at the end. A window at each end
/// finds both without reading a multi-gigabyte file.
pub(super) const AUDIO_SCAN_BYTES: usize = 512 * 1024;

/// Whether a video file carries an audio stream.
///
/// Deliberately a byte scan rather than an FFprobe call. This runs inside the
/// scanner, which walks every folder the user has pointed Render Studio at, and
/// spawning a process per candidate take would make a library scan unusable.
///
/// Signature per container:
///
/// - **AVI** — a stream header (`strh`) whose `fccType` is `auds`.
/// - **MP4/MOV** — a handler box (`hdlr`) declaring the `soun` handler type.
/// - **MKV** — the Matroska `TrackType` element (`0x83`) with value 2, audio.
///
/// A false negative means a take is reported unrenderable and the user is told
/// why; a false positive means a silent clip renders as if fine. So when the
/// scan cannot tell, the answer is no.
pub(super) fn video_has_audio(path: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};

    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let Ok(len) = file.metadata().map(|m| m.len()) else {
        return false;
    };

    let mut windows: Vec<Vec<u8>> = Vec::new();
    let mut head = vec![0u8; AUDIO_SCAN_BYTES.min(len as usize)];
    if file.read_exact(&mut head).is_ok() {
        windows.push(head);
    }
    if len > AUDIO_SCAN_BYTES as u64 {
        let tail_start = len.saturating_sub(AUDIO_SCAN_BYTES as u64);
        if file.seek(SeekFrom::Start(tail_start)).is_ok() {
            let mut tail = Vec::new();
            if file
                .take(AUDIO_SCAN_BYTES as u64)
                .read_to_end(&mut tail)
                .is_ok()
            {
                windows.push(tail);
            }
        }
    }

    windows.iter().any(|w| {
        contains_pair(w, b"strh", b"auds")
            || contains_pair(w, b"hdlr", b"soun")
            || contains_mkv_audio_track(w)
    })
}

/// `needle` followed by `tag` within a short distance.
///
/// Both AVI and MP4 place the type immediately after the box name — 4 bytes for
/// `strh`, 8 for `hdlr` after its version/flags — so a small window is enough,
/// and keeping it small is what stops an unrelated occurrence of `soun`
/// elsewhere in the file reading as an audio track.
pub(super) fn contains_pair(haystack: &[u8], needle: &[u8; 4], tag: &[u8; 4]) -> bool {
    haystack.windows(4).enumerate().any(|(i, w)| {
        if w != needle {
            return false;
        }
        let from = i + 4;
        let to = (from + 16).min(haystack.len());
        haystack[from..to].windows(4).any(|c| c == tag)
    })
}

/// Matroska `TrackType` (element id `0x83`), size 1, value 2 (audio).
pub(super) fn contains_mkv_audio_track(haystack: &[u8]) -> bool {
    haystack.windows(3).any(|w| w == [0x83, 0x81, 0x02])
}
