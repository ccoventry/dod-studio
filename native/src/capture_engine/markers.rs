//! The game's console markers during a batch: where they come from (the
//! console log, then the hook's events pipe), what the listener is told
//! about each, and OBS mode's go-ahead to the hook's batch.

mod clip_summary;

use super::EngineEvent;
use crate::log_markdown;
use std::path::Path;
use std::sync::{Arc, atomic::AtomicBool, mpsc::Sender};

/// The listener's event for one console marker, if it has one: progress,
/// clip recordings, holds. The hook's per-clip summary (#1221) has none: it
/// goes straight to the activity log, here, since every marker the batch
/// reads, late ones too, passes through.
pub(super) fn marker_event(marker: &crate::obs::Marker) -> Option<EngineEvent> {
    use crate::obs::MarkerKind;
    if marker.kind == MarkerKind::Other
        && let Some(clip) = clip_summary::parse(&marker.label)
    {
        log_markdown(&clip.log_line());
        return None;
    }
    match marker.kind {
        MarkerKind::DemoStart => marker
            .demo_progress
            .map(|(job_idx, total, clips)| EngineEvent::DemoLoading(job_idx, total, clips)),
        MarkerKind::NextClip => {
            marker
                .next_clip_progress
                .map(|(job_idx, total, clip_idx, clip_count)| {
                    EngineEvent::FastForwardToClip(job_idx, total, clip_idx, clip_count)
                })
        }
        MarkerKind::StartRecord => Some(EngineEvent::ClipStarted(marker.tick)),
        MarkerKind::StopRecord => Some(EngineEvent::ClipRecorded(marker.tick)),
        MarkerKind::Other if marker.label == "BATCH_HELD" => Some(EngineEvent::Held),
        MarkerKind::Other if marker.label == "BATCH_GOING" => Some(EngineEvent::Going),
        _ => None,
    }
}

/// How long the capture loop's end waits for markers still on their way:
/// the log delivers them 21-40 ms after the game runs them, the pipe within
/// its 5 ms poll.
pub(super) const LATE_MARKERS: std::time::Duration = std::time::Duration::from_millis(100);

/// Every marker that arrives within `wait`. The readers keep reading until
/// the teardown stops them, so what the game ran just before the batch ended
/// lands here rather than being dropped with the channel.
pub(super) fn late_markers(
    rx: &std::sync::mpsc::Receiver<crate::obs::Marker>,
    wait: std::time::Duration,
) -> Vec<crate::obs::Marker> {
    let deadline = std::time::Instant::now() + wait;
    let mut late = Vec::new();
    while let Ok(marker) =
        rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
    {
        late.push(marker);
    }
    late
}

/// The hook's word that a clip's lead-in is paused until OBS records (#1109).
const WAITING_FOR_RECORDING: &str = "BATCH_WAITING_FOR_RECORDING";

/// OBS mode's go-ahead to the hook's batch (#1109): each clip's lead-in
/// waits at its `AUDIO_SYNC` (`BATCH_WAITING_FOR_RECORDING`) until Studio
/// has started OBS on that same marker. Counted rather than paired by clip,
/// since the hook's word can come before or after the marker: a go-ahead is
/// owed for every clip that has both. A hook DLL without the wait never asks,
/// so it never gets one.
#[derive(Debug, Default)]
pub(super) struct RecordingGoAhead {
    waits: usize,
    starts: usize,
    sent: usize,
    /// When the game said it waits, for the activity log's per-clip cost.
    waiting_since: Option<std::time::Instant>,
}

impl RecordingGoAhead {
    pub(super) fn on_marker(&mut self, marker: &crate::obs::Marker) {
        if marker.kind == crate::obs::TRIGGER_MARKER {
            self.starts += 1;
        } else if marker.kind == crate::obs::MarkerKind::Other
            && marker.label == WAITING_FOR_RECORDING
        {
            self.waits += 1;
            self.waiting_since.get_or_insert(marker.at);
        }
    }

    pub(super) fn owed(&self) -> bool {
        self.sent < self.waits.min(self.starts)
    }

    /// One go-ahead went out; how long the game waited for it.
    pub(super) fn sent(&mut self) -> Option<std::time::Duration> {
        self.sent += 1;
        self.waiting_since.take().map(|t| t.elapsed())
    }
}

/// Size of the engine's console log, or 0 if it is not there.
///
/// Read through an *open handle* rather than `fs::metadata(path)`: on Windows a
/// file's directory entry is not necessarily updated while a handle to it is
/// open, and the engine holds this one open for the whole session. `LogTailer`
/// reads its length the same way, for the same reason.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn console_log_len(path: &Path) -> u64 {
    std::fs::File::open(path)
        .and_then(|f| f.metadata())
        .map(|m| m.len())
        .unwrap_or(0)
}

/// Starts the two marker readers: the console log, and the game's events
/// pipe once its hook DLL serves one. The log reader is the one the batch
/// can't run without, so only its failure is returned; a pipe reader that
/// can't start is logged, and the log carries on alone.
///
/// The console-log marker stream isn't OBS-specific: OBS mode needs it to
/// drive recording, but `DEMO_START` (per-demo progress, see
/// `EngineEvent::DemoLoading`) matters in every mode. `-condebug` is passed
/// on every launch (`build_hlae_process`), so the log is always being
/// written and this always runs.
///
/// The log is deleted at the end of a batch, not the start, so a file left
/// by a previous run is normal and its markers are history.
/// `LogTailer::at_end` is what stops a stale `START_RECORD` firing a
/// recording the moment this begins.
pub(super) fn start_readers(
    log_path: &Path,
    marker_tx: Sender<crate::obs::Marker>,
    tail_cancel: &Arc<AtomicBool>,
    marker_gate: &Arc<crate::obs::MarkerGate>,
) -> std::io::Result<()> {
    let tailer = crate::obs::LogTailer::at_end(log_path);
    let cancel = Arc::clone(tail_cancel);
    let gate = Arc::clone(marker_gate);
    let log_marker_tx = marker_tx.clone();
    std::thread::Builder::new()
        .name("obs_log_tail".into())
        .spawn(move || tailer.run(log_marker_tx, cancel, gate))?;
    // The same markers straight from the game, when its hook DLL
    // serves the events pipe. Not fatal if it can't start: the log
    // above is still reading.
    let pipe_tailer = crate::obs::PipeTailer::new(|pid, hello| {
        log_markdown(&format!(
            "[HLAE] Game events pipe connected (hl.exe PID {pid}, `{hello}`) — markers now come from the game directly, not qconsole.log"
        ));
    });
    let cancel = Arc::clone(tail_cancel);
    let gate = Arc::clone(marker_gate);
    if let Err(e) = std::thread::Builder::new()
        .name("events_pipe_tail".into())
        .spawn(move || pipe_tailer.run(marker_tx, cancel, gate))
    {
        log_markdown(&format!(
            "[HLAE] Could not start the game events reader ({e}); using qconsole.log only"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{RecordingGoAhead, late_markers, marker_event};
    use crate::capture_engine::EngineEvent;
    use crate::obs::log_tail::parse_marker;

    fn event(line: &str) -> Option<EngineEvent> {
        marker_event(&parse_marker(line).expect("a marker"))
    }

    #[test]
    fn each_marker_becomes_the_listeners_event() {
        assert!(matches!(
            event("[dod-studio] DEMO_START 2 3 4"),
            Some(EngineEvent::DemoLoading(2, 3, 4))
        ));
        assert!(matches!(
            event("[dod-studio] STOP_RECORD - Tick 93481"),
            Some(EngineEvent::ClipRecorded(Some(93481)))
        ));
        assert!(matches!(
            event("[dod-studio] START_RECORD - Tick 900"),
            Some(EngineEvent::ClipStarted(Some(900)))
        ));
        assert!(event("[dod-studio] AUDIO_SYNC - Tick 900").is_none());
    }

    /// #1081: a `STOP_RECORD` the game ran just before Cancel reaches the
    /// reader a few tens of ms later. The loop's end still collects it, so
    /// the finished clip isn't called cut short.
    #[test]
    fn a_marker_still_on_its_way_when_the_loop_ends_is_kept() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(parse_marker("[dod-studio] START_RECORD - Tick 1").unwrap())
            .unwrap();
        let late_tx = tx.clone();
        let sender = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(40));
            late_tx
                .send(parse_marker("[dod-studio] STOP_RECORD - Tick 2").unwrap())
                .unwrap();
        });
        let late = late_markers(&rx, std::time::Duration::from_millis(400));
        sender.join().unwrap();
        let events: Vec<_> = late.iter().filter_map(marker_event).collect();
        assert!(matches!(
            events.as_slice(),
            [
                EngineEvent::ClipStarted(Some(1)),
                EngineEvent::ClipRecorded(Some(2))
            ]
        ));
    }

    #[test]
    fn a_clip_gets_its_go_ahead_once_obs_started_and_the_game_waits() {
        let marker = |line: &str| parse_marker(line).unwrap();
        let wait = marker("[dod-studio] BATCH_WAITING_FOR_RECORDING");
        let sync = marker("[dod-studio] AUDIO_SYNC - Tick 900");
        let mut go = RecordingGoAhead::default();
        go.on_marker(&wait);
        assert!(!go.owed(), "OBS isn't recording yet");
        go.on_marker(&sync);
        assert!(go.owed());
        assert!(go.sent().is_some());
        assert!(!go.owed(), "one go-ahead a clip");

        // The game's word after the marker, on the next clip.
        go.on_marker(&marker("[dod-studio] START_RECORD - Tick 1000"));
        go.on_marker(&sync);
        assert!(!go.owed(), "the game hasn't said it waits");
        go.on_marker(&wait);
        assert!(go.owed());

        // A hook DLL without the wait never says it waits: no go-ahead.
        let mut old = RecordingGoAhead::default();
        old.on_marker(&sync);
        old.on_marker(&sync);
        assert!(!old.owed());
    }

    /// A batch with no console log to read has no markers at all, so its
    /// size reads as 0, the same as an empty log.
    #[test]
    fn a_console_log_that_isnt_there_reads_as_empty() {
        let dir = crate::test_support::Scratch::new("capture_engine_log_len");
        let log = dir.path().join("qconsole.log");
        assert_eq!(super::console_log_len(&log), 0);
        std::fs::write(&log, b"hello\n").unwrap();
        assert_eq!(super::console_log_len(&log), 6);
    }
}
