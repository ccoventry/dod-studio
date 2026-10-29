//! Closes the game at the end of a capture batch when DoD Studio is no longer
//! watching (issue #545, part 3).
//!
//! Studio normally ends a batch itself: it sees `BATCH_COMPLETE` (over the
//! events pipe, #434) or the exit-trigger folder, and ends `hl.exe`. If
//! Studio was closed mid-batch, the game still plays the schedule to the end
//! -- it lives in the patched demos -- and then sits there. So when the
//! `BATCH_COMPLETE` marker goes by and no Studio is connected to the events
//! pipe a few seconds later, the game quits itself.
//!
//! "Connected" is the events pipe's own view: it notices a reader has gone
//! when a write fails, and `BATCH_COMPLETE` is itself written to the pipe, so
//! by the time the grace period ends a vanished Studio has been noticed. A
//! Studio that is still connected keeps the last word, as before.
//!
//! Only on while the events pipe is (`GOLDSRC_HOOKS_EVENTS=0` turns both off).
//!
//! A Studio that was there when the batch ended has had its say, and the
//! marker is forgotten: the game may be one Studio left open for the next
//! batch (#725), and a reader that goes away later (the batch's pipe reader
//! stops at the end, which the pipe only notices at its next write) must not
//! close it then. A new batch forgets an old marker too.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// How long after `BATCH_COMPLETE` a still-open game waits for Studio. Studio
/// ends it within half a second when it is there; the extra lets the
/// exit-trigger folder be written first, for a Studio that reads it.
const GRACE: Duration = Duration::from_secs(5);

/// When `BATCH_COMPLETE` was seen, in ms since [`start`], plus one (0 = not seen).
static SEEN_AT_MS: AtomicU64 = AtomicU64::new(0);
/// Set once the quit has been sent.
static QUIT_SENT: AtomicBool = AtomicBool::new(false);

fn start() -> Instant {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *START.get_or_init(Instant::now)
}

fn now_ms() -> u64 {
    start().elapsed().as_millis() as u64
}

/// Whether `marker` (a whole marker line) is the batch's last.
pub fn is_batch_complete(marker: &str) -> bool {
    marker
        .split_once(crate::events::TAG)
        .map(|(_, rest)| rest.split_whitespace().next() == Some("BATCH_COMPLETE"))
        .unwrap_or(false)
}

/// Called by `events` for every marker line the game echoes.
pub fn on_marker(marker: &str) {
    if is_batch_complete(marker) {
        batch_ended();
    }
}

/// The batch ended without its `BATCH_COMPLETE` line: a held batch whose
/// Studio went away (#861). The same quit follows when no Studio is there.
pub fn batch_ended() {
    let _ = SEEN_AT_MS.compare_exchange(0, now_ms() + 1, Ordering::AcqRel, Ordering::Relaxed);
}

/// What to do about a batch's end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Next {
    /// No batch ended, or the grace period is still running.
    Wait,
    /// The grace period is over and nobody reads the events pipe.
    Quit,
    /// The grace period is over and Studio is reading: its call, not ours.
    Forget,
}

/// What to do now, the batch having ended at `seen_at_ms` (0 = it hasn't).
fn next(seen_at_ms: u64, now_ms: u64, studio_connected: bool) -> Next {
    if seen_at_ms == 0 || now_ms + 1 < seen_at_ms + GRACE.as_millis() as u64 {
        Next::Wait
    } else if studio_connected {
        Next::Forget
    } else {
        Next::Quit
    }
}

/// Forgets an earlier batch's end: `batch.rs` calls it when a batch starts.
pub fn reset() {
    SEEN_AT_MS.store(0, Ordering::Release);
}

/// Runs every frame from `commands::poll`. One atomic load until a batch ends.
pub fn poll() {
    let seen = SEEN_AT_MS.load(Ordering::Acquire);
    if seen == 0 || QUIT_SENT.load(Ordering::Relaxed) {
        return;
    }
    match next(seen, now_ms(), crate::events::studio_connected()) {
        Next::Wait => return,
        Next::Forget => {
            // Only the marker this look saw: a newer one keeps its own grace.
            let _ = SEEN_AT_MS.compare_exchange(seen, 0, Ordering::AcqRel, Ordering::Relaxed);
            return;
        }
        Next::Quit => {}
    }
    QUIT_SENT.store(true, Ordering::Relaxed);
    unsafe {
        crate::debug::report(
            "batch_end: BATCH_COMPLETE and no Studio on the events pipe -- quitting the game (#545)",
        )
    };
    crate::engine::client_cmd(c"quit\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_batch_complete_marker_counts() {
        assert!(is_batch_complete("[dod-studio] BATCH_COMPLETE"));
        assert!(is_batch_complete("[dod-studio]  BATCH_COMPLETE extra"));
        assert!(!is_batch_complete("[dod-studio] NEXT_CLIP 1 2 3 4"));
        assert!(
            !is_batch_complete("BATCH_COMPLETE"),
            "untagged text is not a marker"
        );
        assert!(
            !is_batch_complete("[dodstudio] -> BATCH_COMPLETE"),
            "a chunk continuation"
        );
    }

    #[test]
    fn quits_only_after_the_grace_period_and_without_studio() {
        let seen = 1_001; // seen at 1000 ms
        let grace = GRACE.as_millis() as u64;
        assert_eq!(next(0, 60_000, false), Next::Wait, "no batch ended");
        assert_eq!(
            next(seen, 1000 + grace - 1, false),
            Next::Wait,
            "still in the grace period"
        );
        assert_eq!(next(seen, 1000 + grace, false), Next::Quit);
        assert_eq!(
            next(seen, 1000 + grace * 10, true),
            Next::Forget,
            "Studio is watching: its call"
        );
    }

    /// #725: Studio leaves the game open after a batch and its pipe reader
    /// stops; the game must not quit when the pipe notices that later.
    #[test]
    fn a_batch_studio_saw_end_is_forgotten() {
        SEEN_AT_MS.store(1_001, Ordering::Release);
        assert_eq!(
            next(
                SEEN_AT_MS.load(Ordering::Acquire),
                1000 + GRACE.as_millis() as u64,
                true
            ),
            Next::Forget
        );
        reset();
        assert_eq!(
            next(SEEN_AT_MS.load(Ordering::Acquire), u64::MAX / 2, false),
            Next::Wait
        );
    }
}
