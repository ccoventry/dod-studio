//! One summary event per captured clip, for Studio's activity log (#1221).
//!
//! ## Why
//!
//! Studio logs a batch's start and end, but not what each clip did: at what
//! demo time it started and stopped recording, and whether its markers
//! arrived. Those markers already pass through the hook ([`crate::events`]
//! wraps `echo`), so the hook can sum each clip up in one line rather than
//! Studio piecing it together from a dozen markers.
//!
//! ## How
//!
//! [`on_marker`] sees every `[dod-studio]` marker the game echoes, on the
//! engine's thread, as `echo` runs: never once per frame, only when the
//! schedule has a marker to print. It follows the batch's own markers
//! (`DEMO_START`, `NEXT_CLIP`, `AUDIO_SYNC`, `START_RECORD`, `STOP_RECORD`,
//! `BATCH_COMPLETE`) and, when a clip's recording stops, queues one
//! [`LABEL`] line for Studio through [`crate::events::send`] (the same queue
//! every marker uses) and writes the same line to the hook's own log, which
//! keeps it when no Studio is listening. A clip whose `STOP_RECORD` never
//! came is reported when the next clip starts, the next demo starts, the
//! batch ends or `ds_capture_batch stop` runs (a Cancel), with the stop left
//! out.
//!
//! The only per-frame cost is [`poll`]'s frame count: one atomic load, and
//! one atomic add while a clip records.
//!
//! ## The line
//!
//! `[dod-studio] CLIP_SUMMARY` then tab-separated `key=value` fields, any of
//! which may be missing: `clip` (1-based across the batch), `demo` and
//! `demo_clip` (`n/of`), `start` and `stop` (the demo player's clock, the one
//! `ds_demo_seek_to` takes, in seconds), `frames` (game frames drawn while
//! recording) and `markers` (which of `AUDIO_SYNC`, `START_RECORD`,
//! `STOP_RECORD` arrived, comma-separated). Studio skips keys it does not
//! know, and an older Studio ignores the whole line as a marker it does not
//! know. The label holds no `Tick `, so no reader takes a tick from it.

// The demo clock is 32-bit only; a host build compiles the rest for the tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// What the summary's label starts with (`native::capture_engine::markers`
/// reads it back).
pub(crate) const LABEL: &str = "CLIP_SUMMARY";

/// Whether a clip is recording, for [`poll`]'s frame count.
static RECORDING: AtomicBool = AtomicBool::new(false);
/// Frames drawn since the recording clip's `START_RECORD`.
static FRAMES: AtomicU32 = AtomicU32::new(0);

static STATE: Mutex<Clips> = Mutex::new(Clips::new());

/// Counts the frame while a clip records. Runs once per frame from
/// `commands::poll`.
pub fn poll() {
    if RECORDING.load(Ordering::Relaxed) {
        FRAMES.fetch_add(1, Ordering::Relaxed);
    }
}

/// Called by `events` for every marker line the game echoes.
pub(crate) fn on_marker(line: &str) {
    let Some((_, label)) = line.split_once(crate::events::TAG) else {
        return;
    };
    let label = label.trim();
    let now = crate::demo_seek::clock().map(|c| c.now);
    let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let frames = FRAMES.load(Ordering::Relaxed);
    let done = state.on_label(label, now, frames);
    if label.starts_with("START_RECORD") {
        FRAMES.store(0, Ordering::Relaxed);
    }
    RECORDING.store(state.recording(), Ordering::Relaxed);
    drop(state);
    report(done);
}

/// `ds_capture_batch stop` ran: a Cancel, or the next batch taking over a
/// game left open (#725). Neither echoes `BATCH_COMPLETE`, so a clip still
/// recording is reported now, without its stop, rather than at the next
/// batch's first `DEMO_START` with every frame drawn in between.
pub fn batch_stopped() {
    let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let done = state.stopped(FRAMES.load(Ordering::Relaxed));
    RECORDING.store(false, Ordering::Relaxed);
    drop(state);
    report(done);
}

/// Queues each ended clip's line for Studio and writes it to the hook's log.
/// Called with no lock held.
fn report(done: Vec<Clip>) {
    for clip in done {
        let label = clip.label();
        crate::events::send(&label);
        unsafe { crate::debug::report(&format!("{} {label}", crate::events::TAG)) };
    }
}

/// One clip, as its markers told it.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Clip {
    /// 1-based across the batch.
    pub number: u32,
    /// (demo, demos in the batch).
    pub demo: Option<(u32, u32)>,
    /// (clip in its demo, clips in that demo).
    pub demo_clip: Option<(u32, u32)>,
    pub start: Option<f64>,
    pub stop: Option<f64>,
    pub frames: Option<u32>,
    pub audio_sync: bool,
    pub start_record: bool,
    pub stop_record: bool,
}

impl Clip {
    /// The summary's label, without the tag. One line, tabs between fields.
    pub fn label(&self) -> String {
        let mut out = format!("{LABEL}\tclip={}", self.number);
        if let Some((n, of)) = self.demo {
            out.push_str(&format!("\tdemo={n}/{of}"));
        }
        if let Some((n, of)) = self.demo_clip {
            out.push_str(&format!("\tdemo_clip={n}/{of}"));
        }
        if let Some(t) = self.start {
            out.push_str(&format!("\tstart={t:.3}"));
        }
        if let Some(t) = self.stop {
            out.push_str(&format!("\tstop={t:.3}"));
        }
        if let Some(f) = self.frames {
            out.push_str(&format!("\tframes={f}"));
        }
        let seen: Vec<&str> = [
            (self.audio_sync, "AUDIO_SYNC"),
            (self.start_record, "START_RECORD"),
            (self.stop_record, "STOP_RECORD"),
        ]
        .into_iter()
        .filter_map(|(seen, name)| seen.then_some(name))
        .collect();
        out.push_str(&format!("\tmarkers={}", seen.join(",")));
        out
    }
}

/// Where the batch is, from its markers.
#[derive(Debug)]
pub(crate) struct Clips {
    /// Clips summed up so far in this batch.
    count: u32,
    demo: Option<(u32, u32)>,
    /// Clips in the current demo.
    demo_clips: Option<u32>,
    /// The next clip's number in its demo.
    next_in_demo: Option<u32>,
    /// `AUDIO_SYNC` came since the last clip ended.
    audio_sync: bool,
    /// The clip recording now: its start time, if the clock was there.
    open: Option<Option<f64>>,
}

impl Clips {
    pub const fn new() -> Self {
        Self {
            count: 0,
            demo: None,
            demo_clips: None,
            next_in_demo: None,
            audio_sync: false,
            open: None,
        }
    }

    pub fn recording(&self) -> bool {
        self.open.is_some()
    }

    /// Follows one marker's label; returns the clips it ended (at most two:
    /// an unstopped one and, never at once, a stopped one).
    pub fn on_label(&mut self, label: &str, now: Option<f64>, frames: u32) -> Vec<Clip> {
        let mut words = label.split_whitespace();
        let head = words.next().unwrap_or("");
        let numbers: Vec<u32> = words.filter_map(|w| w.parse().ok()).collect();
        let mut done = Vec::new();
        match head {
            "DEMO_START" => {
                done.extend(self.close(None, frames, false));
                if let [demo, demos, clips, ..] = numbers[..] {
                    if demo == 1 {
                        self.count = 0;
                    }
                    self.demo = Some((demo, demos));
                    self.demo_clips = Some(clips);
                }
                self.next_in_demo = Some(1);
                self.audio_sync = false;
            }
            "NEXT_CLIP" => {
                if let [_, _, clip, clips, ..] = numbers[..] {
                    self.next_in_demo = Some(clip);
                    self.demo_clips = Some(clips);
                }
            }
            "AUDIO_SYNC" => self.audio_sync = true,
            "START_RECORD" => {
                done.extend(self.close(None, frames, false));
                self.open = Some(now);
            }
            "STOP_RECORD" => done.extend(self.close(now, frames, true)),
            "BATCH_COMPLETE" => done.extend(self.close(None, frames, false)),
            _ => {}
        }
        done
    }

    /// The batch was stopped: ends the clip recording now, if any, without
    /// its stop.
    pub fn stopped(&mut self, frames: u32) -> Vec<Clip> {
        self.close(None, frames, false).into_iter().collect()
    }

    /// Ends the clip recording now, or one stopped without a start: `stop` is
    /// whether a `STOP_RECORD` ended it. Nothing to end, nothing returned.
    fn close(&mut self, now: Option<f64>, frames: u32, stop: bool) -> Option<Clip> {
        let start = self.open.take();
        if start.is_none() && !stop {
            return None;
        }
        self.count += 1;
        let demo_clip = self.next_in_demo.map(|n| (n, self.demo_clips.unwrap_or(0)));
        self.next_in_demo = self.next_in_demo.map(|n| n + 1);
        let clip = Clip {
            number: self.count,
            demo: self.demo,
            demo_clip,
            start: start.flatten(),
            stop: if stop { now } else { None },
            frames: start.is_some().then_some(frames),
            audio_sync: std::mem::take(&mut self.audio_sync),
            start_record: start.is_some(),
            stop_record: stop,
        };
        Some(clip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(clips: &mut Clips, label: &str, now: f64, frames: u32) -> Vec<Clip> {
        clips.on_label(label, Some(now), frames)
    }

    #[test]
    fn a_two_clip_demo_gives_one_summary_per_clip() {
        let mut c = Clips::new();
        assert!(run(&mut c, "DEMO_START 1 1 2", 0.0, 0).is_empty());
        run(&mut c, "NEXT_CLIP 1 1 1 2", 0.0, 0);
        run(&mut c, "SPEED_FLUSH - Tick 100", 9.0, 0);
        run(&mut c, "AUDIO_SYNC - Tick 110", 10.0, 0);
        run(&mut c, "START_RECORD - Tick 170", 11.0, 0);
        assert!(c.recording());
        let first = run(&mut c, "STOP_RECORD - Tick 900", 23.5, 750);
        assert!(!c.recording());
        assert_eq!(
            first[0].label(),
            "CLIP_SUMMARY\tclip=1\tdemo=1/1\tdemo_clip=1/2\tstart=11.000\tstop=23.500\t\
             frames=750\tmarkers=AUDIO_SYNC,START_RECORD,STOP_RECORD"
        );
        // A chained clip: no NEXT_CLIP, no AUDIO_SYNC outside OBS mode.
        run(&mut c, "START_RECORD - Tick 960", 25.0, 0);
        let second = run(&mut c, "STOP_RECORD - Tick 1200", 29.0, 240);
        assert_eq!(second[0].number, 2);
        assert_eq!(second[0].demo_clip, Some((2, 2)));
        assert!(!second[0].audio_sync);
        assert!(run(&mut c, "BATCH_COMPLETE", 30.0, 0).is_empty());
    }

    #[test]
    fn a_clip_that_never_stopped_is_reported_without_its_stop() {
        let mut c = Clips::new();
        run(&mut c, "DEMO_START 1 2 1", 0.0, 0);
        run(&mut c, "START_RECORD - Tick 170", 11.0, 0);
        // The demo was skipped mid-clip; the next one starts.
        let done = run(&mut c, "DEMO_START 2 2 1", 0.0, 300);
        assert_eq!(
            done[0].label(),
            "CLIP_SUMMARY\tclip=1\tdemo=1/2\tdemo_clip=1/1\tstart=11.000\tframes=300\t\
             markers=START_RECORD"
        );
        run(&mut c, "START_RECORD - Tick 170", 5.0, 0);
        let done = run(&mut c, "BATCH_COMPLETE", 9.0, 120);
        assert_eq!(done[0].number, 2);
        assert_eq!(done[0].demo, Some((2, 2)));
        assert!(!done[0].stop_record);
    }

    /// A Cancel mid-clip (`ds_capture_batch stop`, no `BATCH_COMPLETE`)
    /// reports the clip then, once, and the next batch starts clean.
    #[test]
    fn a_batch_stopped_mid_clip_reports_it_once_at_the_stop() {
        let mut c = Clips::new();
        run(&mut c, "DEMO_START 1 1 2", 0.0, 0);
        run(&mut c, "START_RECORD - Tick 170", 11.0, 0);
        let done = c.stopped(90);
        assert_eq!(
            done[0].label(),
            "CLIP_SUMMARY\tclip=1\tdemo=1/1\tdemo_clip=1/2\tstart=11.000\tframes=90\t\
             markers=START_RECORD"
        );
        assert!(!c.recording());
        assert!(
            c.stopped(500).is_empty(),
            "a second stop has nothing to report"
        );
        assert!(run(&mut c, "DEMO_START 1 1 1", 0.0, 5000).is_empty());
    }

    #[test]
    fn a_stop_without_a_start_still_counts_and_says_so() {
        let mut c = Clips::new();
        let done = run(&mut c, "STOP_RECORD - Tick 900", 23.5, 0);
        assert_eq!(
            done[0].label(),
            "CLIP_SUMMARY\tclip=1\tstop=23.500\tmarkers=STOP_RECORD"
        );
    }

    #[test]
    fn a_new_batch_numbers_its_clips_from_one() {
        let mut c = Clips::new();
        run(&mut c, "DEMO_START 1 1 1", 0.0, 0);
        run(&mut c, "START_RECORD - Tick 1", 1.0, 0);
        run(&mut c, "STOP_RECORD - Tick 2", 2.0, 1);
        run(&mut c, "DEMO_START 1 1 1", 0.0, 0);
        run(&mut c, "START_RECORD - Tick 1", 1.0, 0);
        assert_eq!(run(&mut c, "STOP_RECORD - Tick 2", 2.0, 1)[0].number, 1);
    }

    /// The label never names a tick, nor starts with a marker Studio acts on.
    #[test]
    fn the_label_is_one_unknown_marker_line() {
        let label = Clip {
            number: 1,
            ..Clip::default()
        }
        .label();
        assert!(!label.contains("Tick ") && !label.contains(['\n', '\r']));
        assert!(label.starts_with("CLIP_SUMMARY\t"));
    }
}
