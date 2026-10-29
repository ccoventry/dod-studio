//! Whether a take's video runs as long as its audio (#1105).
//!
//! HLAE's FFmpeg can stop writing mid-clip (out of memory, a disk error, a
//! crash) while the game plays the clip to its end. The take folder is then
//! full of files and Render Studio admits it, so "captured" and "renderable"
//! both pass; only its length gives it away. HLAE writes the clip's audio
//! itself, to `sound.wav` beside the video, so the audio runs the whole clip
//! even when the video stops: a video well short of its audio is a partial
//! take. Not captured, so never rendered or deleted automatically, the same
//! as a clip Cancel cut short (#1081).
//!
//! The audio is the yardstick rather than the batch's planned record span:
//! the plan converts frames to seconds by the demo's average frame rate
//! (the scanner only times the demo's opening frames), which is off by the
//! recorder's frame-rate swings. Measured on a real 300 fps take: planned
//! 18.49 s, video and audio both 19.33 s.

use std::io::Read;
use std::path::{Path, PathBuf};

/// How much shorter than its audio a take's video may be and still count as
/// whole: a tenth, and never less than half a second, so a frame or two at
/// either end of a short clip never trips it. A video that stopped early is
/// short by far more (the #885 live test: 1.83 s of 10 s).
pub const SHORT_SHARE: f64 = 0.1;
pub const SHORT_MIN_SECONDS: f64 = 0.5;

/// A take whose video stops well before its audio, in seconds.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct ShortTake {
    pub video_secs: f64,
    pub audio_secs: f64,
}

/// Whether `video` seconds against `audio` seconds is short. An unknown or
/// empty audio length never is.
pub fn is_short(video: f64, audio: f64) -> bool {
    if !(audio.is_finite() && audio > 0.0 && video.is_finite()) {
        return false;
    }
    audio - video > (audio * SHORT_SHARE).max(SHORT_MIN_SECONDS)
}

/// The take folders under a block folder: the block itself and HLAE's
/// numbered `take*` folders inside it.
fn take_folders(block_folder: &Path) -> Vec<PathBuf> {
    let mut folders = vec![block_folder.to_path_buf()];
    if let Ok(entries) = std::fs::read_dir(block_folder) {
        folders.extend(
            entries
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .to_lowercase()
                        .starts_with("take")
                })
                .map(|e| e.path()),
        );
    }
    folders
}

/// The take under `block_folder` (or the take folder itself) checked
/// against its own audio, its video counted at `capture_fps` frames a
/// second. `None` when it's whole or either length is unknown: an OBS take
/// (audio inside a video whose frames aren't counted), an AGR take, no wav,
/// or no capture rate.
pub fn short_take(block_folder: &Path, capture_fps: i32) -> Option<ShortTake> {
    if capture_fps <= 0 {
        return None;
    }
    take_folders(block_folder).into_iter().find_map(|folder| {
        let frames = super::scanner::take_frame_count(&folder)?;
        let audio_secs = wav_seconds(&super::scanner::take_wav(&folder)?)?;
        let video_secs = frames as f64 / f64::from(capture_fps);
        is_short(video_secs, audio_secs).then_some(ShortTake {
            video_secs,
            audio_secs,
        })
    })
}

/// How much of the file to read looking for the `fmt ` and `data` chunks:
/// HLAE writes them first.
const WAV_HEADER_SCAN_BYTES: usize = 4096;

/// A wav's length in seconds from its header: the `data` chunk's size over
/// the `fmt ` chunk's byte rate. A `data` size the header never filled in
/// (0, or past the end of the file: the writer was killed) gives way to the
/// bytes the file holds after it. `None` for anything that isn't a PCM wav
/// header.
pub fn wav_seconds(path: &Path) -> Option<f64> {
    let file_len = std::fs::metadata(path).ok()?.len();
    let mut buf = vec![0u8; WAV_HEADER_SCAN_BYTES];
    let read = std::fs::File::open(path).ok()?.read(&mut buf).ok()?;
    buf.truncate(read);
    wav_seconds_in(&buf, file_len)
}

fn wav_seconds_in(buf: &[u8], file_len: u64) -> Option<f64> {
    if buf.len() < 12 || &buf[0..4] != b"RIFF" || &buf[8..12] != b"WAVE" {
        return None;
    }
    let u32_at = |at: usize| {
        buf.get(at..at + 4)
            .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    };
    let mut pos = 12usize;
    let mut byte_rate = None;
    while pos + 8 <= buf.len() {
        let id = &buf[pos..pos + 4];
        let size = u32_at(pos + 4)?;
        let body = pos + 8;
        match id {
            b"fmt " => byte_rate = u32_at(body + 8).filter(|&r| r > 0),
            b"data" => {
                let rate = f64::from(byte_rate?);
                let after = file_len.saturating_sub(body as u64);
                let bytes = if size == 0 || u64::from(size) > after {
                    after
                } else {
                    u64::from(size)
                };
                return Some(bytes as f64 / rate);
            }
            _ => {}
        }
        pos = body
            .checked_add(size as usize)?
            .checked_add(size as usize & 1)?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    /// A PCM wav header for `secs` at 22050 Hz, 16-bit stereo (HLAE's
    /// format), with `data_size` written as given.
    fn wav_header(data_size: u32) -> Vec<u8> {
        let mut w = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes()); // PCM
        w.extend_from_slice(&2u16.to_le_bytes()); // stereo
        w.extend_from_slice(&22050u32.to_le_bytes());
        w.extend_from_slice(&(22050u32 * 4).to_le_bytes()); // byte rate
        w.extend_from_slice(&4u16.to_le_bytes());
        w.extend_from_slice(&16u16.to_le_bytes());
        w.extend_from_slice(b"data");
        w.extend_from_slice(&data_size.to_le_bytes());
        w
    }

    fn wav(secs: f64) -> Vec<u8> {
        let bytes = (secs * 22050.0 * 4.0) as u32;
        let mut w = wav_header(bytes);
        w.resize(w.len() + bytes as usize, 0);
        w
    }

    #[test]
    fn a_video_that_stopped_early_is_short() {
        // The #885 live test: 1.83 s of a 10 s clip.
        assert!(is_short(1.83, 10.0));
        assert!(is_short(0.0, 3.0));
    }

    #[test]
    fn a_whole_take_is_not_short() {
        assert!(!is_short(19.33, 19.33));
        assert!(!is_short(10.2, 10.0));
        assert!(!is_short(9.2, 10.0));
        // On a short clip the half-second floor is the margin.
        assert!(!is_short(1.6, 2.0));
        assert!(is_short(1.4, 2.0));
        // On a long one, a tenth.
        assert!(!is_short(55.0, 60.0));
        assert!(is_short(53.0, 60.0));
    }

    #[test]
    fn an_unknown_audio_length_is_never_short() {
        assert!(!is_short(1.0, 0.0));
        assert!(!is_short(1.0, -5.0));
        assert!(!is_short(1.0, f64::NAN));
        assert!(!is_short(f64::NAN, 10.0));
    }

    #[test]
    fn a_wav_is_timed_by_its_header_or_by_what_follows_it() {
        let w = wav(2.5);
        assert_eq!(wav_seconds_in(&w, w.len() as u64), Some(2.5));
        // A data size never filled in: the bytes after the header.
        let mut w = wav_header(0);
        let header = w.len();
        w.resize(header + 22050 * 4, 0);
        assert_eq!(wav_seconds_in(&w, w.len() as u64), Some(1.0));
        // Not a wav.
        assert_eq!(wav_seconds_in(b"RIFF\0\0\0\0AVI ", 12), None);
        assert_eq!(wav_seconds_in(b"", 0), None);
    }

    fn bmp_take(take: &Path, frames: usize, audio_secs: f64) {
        let stream = take.join("all");
        std::fs::create_dir_all(&stream).unwrap();
        for i in 0..frames {
            std::fs::write(stream.join(format!("{i:05}.bmp")), b"bmp").unwrap();
        }
        std::fs::write(take.join("sound.wav"), wav(audio_secs)).unwrap();
    }

    #[test]
    fn a_frame_sequence_is_checked_inside_hlaes_take_folder() {
        let block = Scratch::new("take_length_bmp");
        // 30 frames at 10 fps: 3 s of video against 10 s of audio.
        bmp_take(&block.path().join("take0000"), 30, 10.0);
        assert_eq!(
            short_take(block.path(), 10),
            Some(ShortTake {
                video_secs: 3.0,
                audio_secs: 10.0
            })
        );
        // No capture rate: not judged.
        assert_eq!(short_take(block.path(), 0), None);

        let whole = Scratch::new("take_length_bmp_whole");
        bmp_take(&whole.path().join("take0000"), 30, 3.0);
        assert_eq!(short_take(whole.path(), 10), None);
    }

    #[test]
    fn a_take_with_no_countable_stream_or_no_audio_is_not_judged() {
        // An OBS take: its MP4's frames aren't counted.
        let block = Scratch::new("take_length_obs");
        let stream = block.path().join("take0000").join("all");
        std::fs::create_dir_all(&stream).unwrap();
        std::fs::write(stream.join("video.mp4"), b"not counted").unwrap();
        assert_eq!(short_take(block.path(), 60), None);
        // Frames but no audio.
        let silent = Scratch::new("take_length_no_wav");
        let stream = silent.path().join("take0000").join("all");
        std::fs::create_dir_all(&stream).unwrap();
        std::fs::write(stream.join("00000.bmp"), b"bmp").unwrap();
        assert_eq!(short_take(silent.path(), 60), None);
        // Nothing there at all.
        let empty = Scratch::new("take_length_empty");
        assert_eq!(short_take(empty.path(), 60), None);
    }

    #[test]
    fn an_avi_never_finalised_counts_as_no_video() {
        let block = Scratch::new("take_length_avi");
        let take = block.path().join("take0000");
        std::fs::create_dir_all(take.join("all")).unwrap();
        // A RIFF AVI whose header was never filled in.
        let mut avi = b"RIFF\0\0\0\0AVI ".to_vec();
        avi.extend_from_slice(&[0u8; 64]);
        std::fs::write(take.join("all").join("video.avi"), avi).unwrap();
        std::fs::write(take.join("sound.wav"), wav(4.0)).unwrap();
        assert_eq!(
            short_take(block.path(), 300).map(|s| s.video_secs),
            Some(0.0)
        );
    }

    /// The live check (#1105): `DS_SHORT_TAKE_FULL` and
    /// `DS_SHORT_TAKE_TRUNCATED` name two block folders of the same 300 fps
    /// clip, whole and with its video cut.
    #[test]
    #[ignore = "needs a real take: set DS_SHORT_TAKE_FULL and DS_SHORT_TAKE_TRUNCATED"]
    fn a_real_take_and_its_truncated_copy() {
        let folder = |name: &str| {
            PathBuf::from(std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set")))
        };
        let (full, cut) = (
            folder("DS_SHORT_TAKE_FULL"),
            folder("DS_SHORT_TAKE_TRUNCATED"),
        );
        for (name, block) in [("full", &full), ("truncated", &cut)] {
            let take = block.join("take0000");
            println!(
                "{name}: {:?} frames, audio {:?} s, short: {:?}",
                super::super::scanner::take_frame_count(&take),
                super::super::scanner::take_wav(&take).and_then(|w| wav_seconds(&w)),
                short_take(block, 300)
            );
        }
        assert_eq!(short_take(&full, 300), None);
        assert!(short_take(&cut, 300).is_some());
    }
}
