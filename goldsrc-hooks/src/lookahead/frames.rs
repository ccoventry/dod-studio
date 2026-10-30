//! A `.dem` file's frames, one at a time, through a window of the file
//! (issue #1154).
//!
//! The look-ahead reads a whole demo on a thread inside the game, a 32-bit
//! process: a 40 MB HLTV demo read at once held the file's 40 MB for the
//! second it took, and parsing it whole (`open_demo_from_bytes`) peaks above
//! 800 MB. This holds only [`WINDOW_BYTES`] of the file at a time and hands
//! each frame, parsed by `dem-patch`'s own frame parser, to the caller to
//! keep what it needs and drop the rest.

use std::io::{Read, Seek, SeekFrom};

use dem::demo_parser::parse_frame;
use dem::types::{AuxRefCell, Frame, MessageDataParseMode};

/// The most of the file held at once.
pub const WINDOW_BYTES: usize = 1 << 20;
/// The window is topped up whenever less than this is left in it, so a
/// frame that doesn't parse with this much ahead is not cut off by the
/// window: it is too big or broken. A network frame, the biggest that
/// matters, is at most 64 KB of message plus its 477-byte head
/// (`parse_network_messages` refuses longer); the rest are under 100
/// bytes, except the sound and demo-buffer frames, which carry sizes of
/// their own and are a few KB in real demos.
pub const MIN_AHEAD_BYTES: usize = 256 << 10;

/// Reads frames out of `source` from wherever [`Self::seek`] put it.
pub struct Frames<R> {
    source: R,
    window: Vec<u8>,
    /// Where the next frame starts in `window`.
    at: usize,
    source_ended: bool,
    window_bytes: usize,
    min_ahead_bytes: usize,
}

impl<R: Read + Seek> Frames<R> {
    /// Frames out of `source`, once [`Self::seek`] has said where from.
    pub fn new(source: R) -> Self {
        Self::with_window(source, WINDOW_BYTES, MIN_AHEAD_BYTES)
    }

    /// [`Self::new`] with another window, so tests can make the window
    /// refill often.
    pub(super) fn with_window(source: R, window_bytes: usize, min_ahead_bytes: usize) -> Self {
        Self {
            source,
            window: Vec::with_capacity(window_bytes),
            at: 0,
            source_ended: true,
            window_bytes,
            min_ahead_bytes: min_ahead_bytes.min(window_bytes),
        }
    }

    /// Reads from `offset` in the file on; what the window held is dropped.
    pub fn seek(&mut self, offset: u64) -> std::io::Result<()> {
        self.source.seek(SeekFrom::Start(offset))?;
        self.window.clear();
        self.at = 0;
        self.source_ended = false;
        Ok(())
    }

    /// The next frame, or `None` at the end of the file, at a frame that
    /// doesn't parse (a truncated or damaged file: the caller keeps what it
    /// had), or when the file can't be read. `aux` is the parser's state
    /// across frames: the same one for every frame of a demo.
    pub fn next_frame(&mut self, aux: &AuxRefCell) -> Option<Frame> {
        self.top_up().ok()?;
        let input = &self.window[self.at..];
        if input.is_empty() {
            return None;
        }
        let (rest, frame) = parse_frame(input, MessageDataParseMode::Parse, aux.clone()).ok()?;
        self.at += input.len() - rest.len();
        Some(frame)
    }

    /// Moves what is left of the window to its front and fills the rest
    /// from the file, when less than `min_ahead_bytes` is left.
    fn top_up(&mut self) -> std::io::Result<()> {
        if self.source_ended || self.window.len() - self.at >= self.min_ahead_bytes {
            return Ok(());
        }
        self.window.drain(..self.at);
        self.at = 0;
        let mut filled = self.window.len();
        self.window.resize(self.window_bytes, 0);
        while filled < self.window_bytes {
            match self.source.read(&mut self.window[filled..]) {
                Ok(0) => {
                    self.source_ended = true;
                    break;
                }
                Ok(n) => filled += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => {
                    self.window.truncate(filled);
                    return Err(e);
                }
            }
        }
        self.window.truncate(filled);
        Ok(())
    }

    /// The window's allocation, for the tests' memory check.
    #[cfg(test)]
    fn window_capacity(&self) -> usize {
        self.window.capacity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dem::types::{Aux, FrameData};
    use std::io::Cursor;

    /// A weapon-animation frame (type 7): type, time, frame number, then
    /// two `i32`s. 17 bytes.
    fn weapon_anim_frame(n: i32) -> Vec<u8> {
        let mut f = vec![7u8];
        f.extend_from_slice(&(n as f32).to_le_bytes());
        f.extend_from_slice(&n.to_le_bytes());
        f.extend_from_slice(&n.to_le_bytes());
        f.extend_from_slice(&0i32.to_le_bytes());
        f
    }

    fn file_of(frames: i32) -> Vec<u8> {
        let mut bytes: Vec<u8> = (0..frames).flat_map(weapon_anim_frame).collect();
        bytes.push(5); // NextSection
        bytes.extend_from_slice(&[0; 8]);
        bytes
    }

    fn anims(frames: &mut Frames<Cursor<Vec<u8>>>) -> Vec<i32> {
        let aux = Aux::new2();
        let mut seen = Vec::new();
        while let Some(frame) = frames.next_frame(&aux) {
            match frame.frame_data {
                FrameData::WeaponAnimation(w) => seen.push(w.anim),
                FrameData::NextSection => break,
                _ => panic!("unexpected frame"),
            }
        }
        seen
    }

    #[test]
    fn frames_come_out_whole_across_window_refills() {
        // A 40-byte window topped up below 20: refilled every frame or two,
        // with frames split across the old window's end.
        let mut frames = Frames::with_window(Cursor::new(file_of(100)), 40, 20);
        frames.seek(0).unwrap();
        assert_eq!(anims(&mut frames), (0..100).collect::<Vec<_>>());
        assert!(frames.window_capacity() <= 40);
    }

    #[test]
    fn a_seek_starts_reading_there() {
        let mut frames = Frames::with_window(Cursor::new(file_of(10)), 40, 20);
        frames.seek(17 * 7).unwrap();
        assert_eq!(anims(&mut frames), vec![7, 8, 9]);
    }

    #[test]
    fn a_file_cut_off_mid_frame_ends_at_the_last_whole_frame() {
        let mut bytes = file_of(5);
        bytes.truncate(17 * 3 + 6);
        let mut frames = Frames::with_window(Cursor::new(bytes), 40, 20);
        frames.seek(0).unwrap();
        assert_eq!(anims(&mut frames), vec![0, 1, 2]);
    }

    #[test]
    fn nothing_is_read_before_a_seek() {
        let mut frames = Frames::new(Cursor::new(file_of(3)));
        assert!(frames.next_frame(&Aux::new2()).is_none());
    }
}
