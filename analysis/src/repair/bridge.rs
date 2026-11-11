//! Cutting the holes out of a damaged demo (#15): walk the frames, cut where
//! the walk breaks, find the next place the recording resumes, and repeat to
//! the end. The kept stretches are spliced behind the original header with a
//! fresh two-entry directory. Moved from the `multi_bridge` probe.
//!
//! Demos usually have several holes. A search that wants each resume point to
//! walk to the end of the file assumes one hole, and every honest resume point
//! before a later hole then fails it (on `m1_h1` that skipped 790 seconds of
//! the match). So each resume point is judged on the shape of the run it
//! starts ([`Run::is_plausible`]) and confirmed by parsing that run.

use super::frame_walk::{
    DEMO_HEADER_SIZE, DIRECTORY_OFFSET_POS, MIN_RUN_FRAMES, NEXT_SECTION, Run, entry0_end,
    frame_end, frame_starts, frame_time, frame_type, frames_end, walk,
};

/// A walk that stops this close to the end of the frames has reached it: a
/// recording cut off mid-frame by the game closing is not a hole.
pub(crate) const END_SLACK_BYTES: usize = 64;
/// Guard against a pathological file turning the search into an endless one.
const MAX_HOLES: usize = 64;
/// A segment trimmed back further than this many frames is given up on.
const MAX_TRIM_FRAMES: usize = 4096;
/// Frames at each side of a hole whose times are checked against the
/// recording around them ([`drop_misplaced_edge_frames`]).
const EDGE_FRAMES: usize = 8;
/// How far an edge frame's time may stray from its side of the hole.
const EDGE_TIME_SLACK_SECS: f32 = 1.0;

/// A stretch of the original file kept as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Segment {
    pub start: usize,
    pub stop: usize,
}

/// A hole between two kept segments.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Cut {
    /// Where the kept recording stops in the original file (after any damaged
    /// frames trimmed off).
    pub stop: usize,
    /// Where it resumes.
    pub resume: usize,
    /// The recording time of the last frame kept before the hole, seconds.
    pub time_before: f32,
    /// The recording time of the first frame kept after it.
    pub time_after: f32,
}

/// What [`bridge`] made of a file.
#[derive(Debug, Default)]
pub(crate) struct Bridged {
    /// The spliced demo; empty when no hole was found.
    pub bytes: Vec<u8>,
    pub cuts: Vec<Cut>,
    /// Frames dropped at the edges of holes for being damaged.
    pub edge_frames_dropped: usize,
    /// Bytes of the original's frame area kept.
    pub bytes_kept: usize,
    /// The original's frame area.
    pub bytes_total: usize,
    /// The end of the file was not reached: the recording never resumed
    /// after the last hole.
    pub stopped_early: bool,
}

/// Why a demo cannot be bridged at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BridgeError {
    /// The signon (directory entry 0) does not walk: nothing to build on.
    NoSignon,
    /// The recording before the first hole never parses.
    NothingParses,
    Cancelled,
}

/// Parses `probe` and says whether it parsed. The parser is bounds-checked at
/// its read sites (#225), so damaged input is an `Err`, never a panic.
fn parses(probe: &[u8]) -> bool {
    dem::open_demo_from_bytes(probe).is_ok()
}

/// What the bridge needs from its caller.
pub(crate) struct BridgeHooks<'a> {
    /// Called with how far through the file the search is, 0.0 to 1.0.
    pub progress: &'a mut dyn FnMut(f32),
    pub is_cancelled: &'a dyn Fn() -> bool,
}

/// Finds every hole in `bytes` and splices the recording around them. A demo
/// whose frames walk to the end has no hole: `cuts` comes back empty and no
/// bytes are built.
pub(crate) fn bridge(bytes: &[u8], mut hooks: BridgeHooks) -> Result<Bridged, BridgeError> {
    let end = frames_end(bytes);
    let e0 = entry0_end(bytes, end).ok_or(BridgeError::NoSignon)?;
    let mut segments: Vec<Segment> = Vec::new();
    let mut pos = DEMO_HEADER_SIZE;
    let mut last_time = 0.0f32;
    let mut stopped_early = false;
    for hole in 0..=MAX_HOLES {
        if (hooks.is_cancelled)() {
            return Err(BridgeError::Cancelled);
        }
        (hooks.progress)(pos as f32 / end as f32);
        let (starts, cut) = frame_starts(bytes, pos, end);
        let reached_end = cut >= end.saturating_sub(END_SLACK_BYTES);
        if hole == 0 && reached_end {
            return Ok(Bridged::default());
        }
        if !starts.is_empty() {
            match trim_to_parse(
                bytes,
                e0,
                Segment {
                    start: pos,
                    stop: cut,
                },
                &starts,
            ) {
                Some(seg) => {
                    segments.push(seg);
                    // The median of the kept frames' last times, never the
                    // last frame's own: a frame the damage reached can be
                    // stamped far ahead of the recording, and the resume
                    // search would then skip every honest frame before it.
                    if let Some(t) = time_at_end(bytes, seg.start, seg.stop).filter(|t| *t > 0.0) {
                        last_time = t;
                    }
                }
                None if segments.is_empty() => return Err(BridgeError::NothingParses),
                // A stretch between two holes that never parses is dropped.
                None => {}
            }
        }
        if reached_end {
            break;
        }
        if hole == MAX_HOLES {
            stopped_early = true;
            break;
        }
        match next_resume(bytes, e0, cut + 1, end, last_time, &mut hooks) {
            Some(next) => pos = next,
            None if (hooks.is_cancelled)() => return Err(BridgeError::Cancelled),
            None => {
                stopped_early = true;
                break;
            }
        }
    }
    let (frames, edge_frames_dropped) = drop_misplaced_edge_frames(bytes, e0, &segments);
    let cuts = cuts_between(bytes, &segments, &frames);
    let bytes_kept =
        frames.iter().map(|s| s.stop - s.start).sum::<usize>() - (e0 - DEMO_HEADER_SIZE);
    Ok(Bridged {
        bytes: assemble(bytes, e0, &frames),
        cuts,
        edge_frames_dropped,
        bytes_kept,
        bytes_total: end - e0,
        stopped_early,
    })
}

/// The first offset at or after `from` where the recording plausibly resumes:
/// a run of real-looking frames, no earlier in time than `after_time`, that
/// parses. Shape alone is not enough: on `m3_h2` the first shape-plausible
/// offset is one the parser rejects, and only the third is real.
fn next_resume(
    bytes: &[u8],
    e0: usize,
    from: usize,
    end: usize,
    after_time: f32,
    hooks: &mut BridgeHooks,
) -> Option<usize> {
    const QUICK_FRAMES: usize = 400;
    // How often the byte-by-byte search reports and checks for a cancel.
    const REPORT_EVERY_BYTES: usize = 65_536;
    for scan in from..end {
        if scan % REPORT_EVERY_BYTES == 0 {
            if (hooks.is_cancelled)() {
                return None;
            }
            (hooks.progress)(scan as f32 / end as f32);
        }
        if frame_type(bytes, scan).is_none()
            || frame_time(bytes, scan).is_none_or(|t| t < after_time)
        {
            continue;
        }
        let quick = walk(bytes, scan, end, QUICK_FRAMES);
        if quick.frames < QUICK_FRAMES || quick.swallows > 0 || quick.backwards > 2 {
            continue;
        }
        let run: Run = walk(bytes, scan, end, MIN_RUN_FRAMES * 4);
        if !run.is_plausible() {
            continue;
        }
        let probe = probe_demo(
            bytes,
            e0,
            Segment {
                start: scan,
                stop: run.cut,
            },
        );
        // Each parse is a few milliseconds and `m3_h1-1` needs 2,675 of them
        // at one hole, so a cancel is checked before each.
        if (hooks.is_cancelled)() {
            return None;
        }
        if parses(&probe) {
            return Some(scan);
        }
    }
    None
}

/// Trims frames off the end of `seg` until it parses, doubling how many each
/// time. The frames right before a hole keep their headers while their
/// messages are damaged, so a segment can walk perfectly and still not parse.
fn trim_to_parse(bytes: &[u8], e0: usize, seg: Segment, starts: &[usize]) -> Option<Segment> {
    let mut trimmed = 0usize;
    loop {
        if trimmed >= starts.len() || trimmed > MAX_TRIM_FRAMES {
            return None;
        }
        let stop = if trimmed == 0 {
            seg.stop
        } else {
            starts[starts.len() - trimmed]
        };
        let candidate = Segment {
            start: seg.start,
            stop,
        };
        if parses(&probe_demo(bytes, e0, candidate)) {
            return Some(candidate);
        }
        trimmed = if trimmed == 0 { 1 } else { trimmed * 2 };
    }
}

/// A demo of one segment for a parse check: entry 0 then the segment. The
/// first segment already holds entry 0; every later one needs it in front, or
/// there are no delta descriptions to decode against.
fn probe_demo(bytes: &[u8], e0: usize, seg: Segment) -> Vec<u8> {
    let signon = Segment {
        start: DEMO_HEADER_SIZE,
        stop: e0,
    };
    if seg.start == DEMO_HEADER_SIZE {
        assemble(bytes, e0, &[seg])
    } else {
        assemble(bytes, e0, &[signon, seg])
    }
}

/// The segments with the damaged frames at the edges of each hole left out:
/// a frame within [`EDGE_FRAMES`] of a hole whose time is out of line with
/// its own side of the hole. Such a frame is a header the damage reached:
/// a `Sound` frame stamped 0.000 s at `m1_h2`'s first hole, a client-data
/// frame stamped 2031 s at its third (512 s past the frames around it), and
/// at `m3_h1-1`'s third a network frame stamped 877 s among frames at 893 s
/// whose `incoming_sequence` is 2^17 too high, which read as two more joins.
/// The engine plays frames in file order and waits for each one's time, so a
/// frame stamped far ahead holds playback there (#15). The signon (before
/// `e0`) is never judged: its frames' times spread over the seconds the map
/// took to load, and the directory's first entry must end exactly at `e0`.
/// Returns the pieces to keep and how many frames were left out.
fn drop_misplaced_edge_frames(
    bytes: &[u8],
    e0: usize,
    segments: &[Segment],
) -> (Vec<Segment>, usize) {
    let mut drop_at: Vec<(usize, usize)> = Vec::new();
    let mut misplaced = |side: &[&TimedFrame], edge: &[&TimedFrame]| {
        let Some(reference) = median_time(side) else {
            return;
        };
        for f in edge {
            if (f.time - reference).abs() > EDGE_TIME_SLACK_SECS {
                drop_at.push((f.start, f.stop));
            }
        }
    };
    for pair in segments.windows(2) {
        let before = timed_frames(bytes, pair[0]);
        let after = timed_frames(bytes, pair[1]);
        let before_side: Vec<&TimedFrame> = before
            .iter()
            .rev()
            .take_while(|f| f.start >= e0)
            .take(EDGE_FRAMES * 3)
            .collect();
        let after_side: Vec<&TimedFrame> = after.iter().take(EDGE_FRAMES * 3).collect();
        misplaced(
            &before_side,
            &before_side[..EDGE_FRAMES.min(before_side.len())],
        );
        misplaced(
            &after_side,
            &after_side[..EDGE_FRAMES.min(after_side.len())],
        );
    }
    drop_at.sort_unstable();
    let mut kept = Vec::with_capacity(segments.len() + drop_at.len());
    for seg in segments {
        let mut start = seg.start;
        for &(from, to) in drop_at
            .iter()
            .filter(|(f, _)| *f >= seg.start && *f < seg.stop)
        {
            if from > start {
                kept.push(Segment { start, stop: from });
            }
            start = to;
        }
        if start < seg.stop {
            kept.push(Segment {
                start,
                stop: seg.stop,
            });
        }
    }
    (kept, drop_at.len())
}

/// The holes between `segments`, timed by the frames kept around each one
/// (`pieces`: the segments less the damaged edge frames).
fn cuts_between(bytes: &[u8], segments: &[Segment], pieces: &[Segment]) -> Vec<Cut> {
    let frames_of = |seg: &Segment| -> Vec<TimedFrame> {
        pieces
            .iter()
            .filter(|p| p.start >= seg.start && p.stop <= seg.stop)
            .flat_map(|p| timed_frames(bytes, *p))
            .collect()
    };
    segments
        .windows(2)
        .map(|pair| Cut {
            stop: pair[0].stop,
            resume: pair[1].start,
            time_before: frames_of(&pair[0]).last().map_or(0.0, |f| f.time),
            time_after: frames_of(&pair[1]).first().map_or(0.0, |f| f.time),
        })
        .collect()
}

/// The recording time where the frames from `start` to `stop` end: the
/// median of the last few, so a damaged frame right before a hole (stamped
/// 0.000 s at `m1_h2`'s first) doesn't speak for the rest.
pub(crate) fn time_at_end(bytes: &[u8], start: usize, stop: usize) -> Option<f32> {
    let frames = timed_frames(bytes, Segment { start, stop });
    let last: Vec<&TimedFrame> = frames.iter().rev().take(EDGE_FRAMES * 3).collect();
    median_time(&last)
}

/// The median of the frames' times: what a side of a hole reads, whatever a
/// few damaged frames among them say.
fn median_time(frames: &[&TimedFrame]) -> Option<f32> {
    let mut times: Vec<f32> = frames.iter().map(|f| f.time).collect();
    times.sort_by(f32::total_cmp);
    times.get(times.len() / 2).copied()
}

/// One frame's place and time, from its header.
struct TimedFrame {
    start: usize,
    stop: usize,
    time: f32,
}

/// The frames of `seg` that carry a time (not section ends).
fn timed_frames(bytes: &[u8], seg: Segment) -> Vec<TimedFrame> {
    let mut frames = Vec::new();
    let mut pos = seg.start;
    while let Some(next) = frame_end(bytes, pos, seg.stop) {
        let t = bytes[pos];
        if t != NEXT_SECTION && t != 255 {
            frames.push(TimedFrame {
                start: pos,
                stop: next,
                time: frame_time(bytes, pos).unwrap_or(0.0),
            });
        }
        pos = next;
    }
    frames
}

/// Splices `segments` behind the original file header and wraps them in a
/// two-entry directory. The first segment always starts with entry 0 (the
/// signon), which ends at `e0`.
fn assemble(bytes: &[u8], e0: usize, segments: &[Segment]) -> Vec<u8> {
    let mut out =
        Vec::with_capacity(segments.iter().map(|s| s.stop - s.start).sum::<usize>() + 1024);
    out.extend_from_slice(&bytes[..DEMO_HEADER_SIZE]);
    for s in segments {
        out.extend_from_slice(&bytes[s.start..s.stop]);
    }
    // The playback entry's closing NextSection frame: type, time, ordinal.
    out.push(NEXT_SECTION);
    out.extend_from_slice(&0f32.to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes());
    let dir = out.len();
    out.extend_from_slice(&2i32.to_le_bytes());
    let entry = |out: &mut Vec<u8>, type_: i32, name: &str, offset: usize, length: usize| {
        out.extend_from_slice(&type_.to_le_bytes());
        let mut description = [0u8; 64];
        for (d, c) in description.iter_mut().zip(name.bytes().take(63)) {
            *d = c;
        }
        out.extend_from_slice(&description);
        out.extend_from_slice(&0i32.to_le_bytes()); // flags
        out.extend_from_slice(&(-1i32).to_le_bytes()); // cd track
        out.extend_from_slice(&0f32.to_le_bytes()); // track time
        out.extend_from_slice(&0i32.to_le_bytes()); // frame count
        out.extend_from_slice(&(offset as i32).to_le_bytes());
        out.extend_from_slice(&(length as i32).to_le_bytes());
    };
    entry(
        &mut out,
        0,
        "LOADING",
        DEMO_HEADER_SIZE,
        e0 - DEMO_HEADER_SIZE,
    );
    entry(&mut out, 1, "Playback", e0, dir - e0);
    out[DIRECTORY_OFFSET_POS..DEMO_HEADER_SIZE].copy_from_slice(&(dir as i32).to_le_bytes());
    out
}
