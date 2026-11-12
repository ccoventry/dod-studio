//! Walking a demo's frames by their headers alone, without decoding a single
//! message (#15). This is how a repair finds where the recording breaks and
//! where it resumes: a hole is bytes no frame header makes sense of.
//!
//! A frame is a 9-byte header (type, time, ordinal) and a body whose length is
//! fixed by the type or read from it. The walk stops at the first header that
//! cannot be a frame, or whose body runs past the end.

/// The demo header in front of the first frame.
pub(crate) const DEMO_HEADER_SIZE: usize = 544;
/// Where the header keeps the directory's offset.
pub(crate) const DIRECTORY_OFFSET_POS: usize = 540;
const FRAME_HEADER_SIZE: usize = 9;
/// A network frame's fixed part: the 464-byte `demo_info`, the sequence info,
/// then the message length.
const NETMSG_INFO_SIZE: usize = 464;
const NETWORK_HEADER_ALIGNMENT: usize = 468;
/// No real message is longer than this; a longer length is a damaged header.
const MAX_PAYLOAD: usize = 2_097_152;
/// A frame of a fixed-size type (anything but a network message) this long has
/// swallowed bytes that are not its own: a damaged length field.
const SWALLOW_BYTES: usize = 4096;
/// The `NextSection` frame type: the end of a directory entry.
pub(crate) const NEXT_SECTION: u8 = 5;
/// The frame types that carry no time of their own worth comparing.
const LAST_IN_SEGMENT: u8 = 255;

/// What a walk over a run of frames saw.
#[derive(Clone, Debug, Default)]
pub(crate) struct Run {
    pub frames: usize,
    /// Start of the frame the walk could not read: the byte to cut at. Never
    /// the offset part-way through it that the walk gave up at; cutting there
    /// leaves an orphan header whose length is read out of whatever follows.
    pub cut: usize,
    /// Fixed-size frames that swallowed more than [`SWALLOW_BYTES`].
    pub swallows: usize,
    /// Frames whose time steps back from the one before.
    pub backwards: usize,
    /// Client-data (4) and demo-buffer (9) frames, which pair one to one in
    /// every healthy demo measured.
    pub client_data: usize,
    pub demo_buffers: usize,
    pub bytes: usize,
    pub first_time: Option<f32>,
    pub last_time: Option<f32>,
}

/// How much clean recording a resume point must produce to be believed. Long
/// enough that a false alignment will not hold together, short enough that two
/// holes close to each other still leave a usable segment between them.
pub(crate) const MIN_RUN_FRAMES: usize = 5000;
/// The bytes a frame a plausible run averages. Wide on purpose: a resume
/// point is confirmed by parsing its run, and the shape only saves parses.
const BYTES_PER_FRAME_LOW: f64 = 30.0;
const BYTES_PER_FRAME_HIGH: f64 = 400.0;

impl Run {
    /// Whether the run has the shape of real recording: a sane size a frame
    /// (about 160 bytes in a 12-player match, 77 in `ci_fixture.dem`'s game
    /// against five bots), nothing swallowed, and client-data and demo-buffer
    /// frames paired.
    pub fn is_plausible(&self) -> bool {
        if self.frames < MIN_RUN_FRAMES {
            return false;
        }
        let bytes_per_frame = self.bytes as f64 / self.frames as f64;
        self.swallows == 0
            && self.backwards <= 2
            && self.client_data.abs_diff(self.demo_buffers) <= 2
            && (BYTES_PER_FRAME_LOW..=BYTES_PER_FRAME_HIGH).contains(&bytes_per_frame)
    }
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// A frame's time from its header at `at`, when it is a plausible one.
pub(crate) fn frame_time(bytes: &[u8], at: usize) -> Option<f32> {
    let time = f32::from_bits(read_u32(bytes, at + 1)?);
    (time.is_finite() && (0.0..=100_000.0).contains(&time)).then_some(time)
}

/// The type byte at `at` when it is a frame type.
pub(crate) fn frame_type(bytes: &[u8], at: usize) -> Option<u8> {
    let t = *bytes.get(at)?;
    (t <= 9 || t == LAST_IN_SEGMENT).then_some(t)
}

/// The end of the frame starting at `at`, or `None` when no frame starts
/// there or it runs past `end`.
pub(crate) fn frame_end(bytes: &[u8], at: usize, end: usize) -> Option<usize> {
    if at + FRAME_HEADER_SIZE > end {
        return None;
    }
    let t = frame_type(bytes, at)?;
    frame_time(bytes, at)?;
    let body = at + FRAME_HEADER_SIZE;
    let next = match t {
        2 | NEXT_SECTION | LAST_IN_SEGMENT => body,
        0 | 1 => {
            let length = read_u32(bytes, body + NETMSG_INFO_SIZE)? as i32;
            if body + NETWORK_HEADER_ALIGNMENT > end || length < 0 || length as usize > MAX_PAYLOAD
            {
                return None;
            }
            body + NETWORK_HEADER_ALIGNMENT + length as usize
        }
        3 => body + 64,
        4 => body + 32,
        6 => body + 84,
        7 => body + 8,
        // Checked: a damaged length near `u32::MAX` overflows a 32-bit
        // `usize` (the wasm build).
        8 => (body + 24).checked_add(read_u32(bytes, body + 4)? as usize)?,
        9 => (body + 4).checked_add(read_u32(bytes, body)? as usize)?,
        _ => return None,
    };
    (next <= end).then_some(next)
}

/// Walks up to `max_frames` frames from `start`, stopping at the first one it
/// cannot read.
pub(crate) fn walk(bytes: &[u8], start: usize, end: usize, max_frames: usize) -> Run {
    let mut run = Run {
        cut: start,
        ..Default::default()
    };
    let mut pos = start;
    let mut prev_time: Option<f32> = None;
    while run.frames < max_frames {
        let Some(next) = frame_end(bytes, pos, end) else {
            break;
        };
        let t = bytes[pos];
        if t != LAST_IN_SEGMENT && t != NEXT_SECTION {
            let time = frame_time(bytes, pos).unwrap_or(0.0);
            run.first_time.get_or_insert(time);
            if prev_time.is_some_and(|p| time + 0.001 < p) {
                run.backwards += 1;
            }
            prev_time = Some(time);
            run.last_time = Some(time);
        }
        match t {
            4 => run.client_data += 1,
            9 => run.demo_buffers += 1,
            _ => {}
        }
        if next - pos > SWALLOW_BYTES && t != 0 && t != 1 {
            run.swallows += 1;
        }
        run.frames += 1;
        pos = next;
        run.cut = pos;
    }
    run.bytes = run.cut - start;
    run
}

/// Every frame's start from `start` up to where the walk breaks, and that
/// break: the segment can then be trimmed back frame by frame.
pub(crate) fn frame_starts(bytes: &[u8], start: usize, end: usize) -> (Vec<usize>, usize) {
    let mut starts = Vec::new();
    let mut pos = start;
    while let Some(next) = frame_end(bytes, pos, end) {
        starts.push(pos);
        pos = next;
    }
    (starts, pos)
}

/// Where the frames end: at the directory when the header names one inside
/// the file, else at the end of the file (an unfinalised demo, or a directory
/// lost with the end of the file).
pub(crate) fn frames_end(bytes: &[u8]) -> usize {
    match read_u32(bytes, DIRECTORY_OFFSET_POS) {
        Some(dir) if dir as usize > DEMO_HEADER_SIZE && dir as usize <= bytes.len() => dir as usize,
        _ => bytes.len(),
    }
}

/// End of the signon (directory entry 0). It finishes on a `NextSection`
/// frame, and some recordings write two back to back (`m3_h1-1` does, `m3_h2`
/// does not), so the whole run is consumed. Stopping after the first leaves
/// the second as entry 1's opening frame, which ends entry 1 at once: the
/// result parses, and holds 32 frames out of half a million.
pub(crate) fn entry0_end(bytes: &[u8], end: usize) -> Option<usize> {
    let mut pos = DEMO_HEADER_SIZE;
    loop {
        let t = *bytes.get(pos)?;
        pos = frame_end(bytes, pos, end)?;
        if t == NEXT_SECTION {
            while bytes.get(pos) == Some(&NEXT_SECTION) {
                match frame_end(bytes, pos, end) {
                    Some(next) => pos = next,
                    None => break,
                }
            }
            return Some(pos);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame of type 4 (client data, 32 bytes) at `time`.
    fn client_data(time: f32) -> Vec<u8> {
        let mut f = vec![4];
        f.extend_from_slice(&time.to_le_bytes());
        f.extend_from_slice(&0i32.to_le_bytes());
        f.extend_from_slice(&[0; 32]);
        f
    }

    #[test]
    fn a_walk_stops_at_the_start_of_the_first_frame_it_cannot_read() {
        let mut bytes = client_data(1.0);
        bytes.extend(client_data(1.5));
        let good = bytes.len();
        bytes.extend([42, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        let run = walk(&bytes, 0, bytes.len(), usize::MAX);
        assert_eq!(run.frames, 2);
        assert_eq!(run.cut, good);
        assert_eq!(run.last_time, Some(1.5));
    }

    #[test]
    fn a_frame_whose_body_runs_past_the_end_is_not_read() {
        let bytes = client_data(1.0);
        assert_eq!(frame_end(&bytes, 0, bytes.len() - 1), None);
        assert_eq!(frame_end(&bytes, 0, bytes.len()), Some(bytes.len()));
    }

    #[test]
    fn a_time_that_steps_back_is_counted() {
        let mut bytes = client_data(2.0);
        bytes.extend(client_data(1.0));
        assert_eq!(walk(&bytes, 0, bytes.len(), usize::MAX).backwards, 1);
    }

    #[test]
    fn a_header_with_a_nonsense_time_is_not_a_frame() {
        let bytes = client_data(f32::NAN);
        assert_eq!(frame_end(&bytes, 0, bytes.len()), None);
        let bytes = client_data(-3.0);
        assert_eq!(frame_end(&bytes, 0, bytes.len()), None);
    }
}
