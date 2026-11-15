//! Which source's markers go on to the capture loop (#434): `qconsole.log`
//! until the game's events pipe connects, the pipe from then on.
//!
//! The log carries every tagged `echo` the engine printed, and the pipe
//! carries the same ones in the same order, with the hook's own lines in
//! between (`CLIP_SUMMARY`, `KILLFEED_LAYOUT`, `BATCH_*`, `REVIEW_*`), which
//! never reach the console. The pipe sends every line queued since its last
//! reader, so its first lines include ones the log already sent. Those are
//! skipped by label, not by count: a count lands on one of the hook's own
//! lines and lets a `START_RECORD` through twice (#1358).

use std::collections::VecDeque;
use std::sync::Mutex;

/// Log markers kept for the pipe to skip: the hook's own queue holds no more
/// (`goldsrc-hooks` `events::MAX_PENDING`), so an older one can't come back.
const LOG_DELIVERED_KEPT: usize = 4096;

/// Hands the marker stream from the log to the pipe, once.
#[derive(Default)]
pub struct MarkerGate {
    inner: Mutex<GateState>,
}

#[derive(Default)]
struct GateState {
    pipe_active: bool,
    log_delivered: VecDeque<String>,
}

impl MarkerGate {
    /// Whether a marker read from the log goes on, noting it if it does.
    pub fn admit_from_log(&self, label: &str) -> bool {
        let Ok(mut state) = self.inner.lock() else {
            return true;
        };
        if state.pipe_active {
            return false;
        }
        if state.log_delivered.len() == LOG_DELIVERED_KEPT {
            state.log_delivered.pop_front();
        }
        state.log_delivered.push_back(label_key(label));
        true
    }

    /// Hands the stream to the pipe. Returns the markers the log already
    /// delivered, for the pipe to skip (none on a reconnect).
    pub fn switch_to_pipe(&self) -> AlreadyDelivered {
        let Ok(mut state) = self.inner.lock() else {
            return AlreadyDelivered::default();
        };
        if state.pipe_active {
            return AlreadyDelivered::default();
        }
        state.pipe_active = true;
        AlreadyDelivered(std::mem::take(&mut state.log_delivered))
    }

    pub fn pipe_active(&self) -> bool {
        self.inner.lock().map(|s| s.pipe_active).unwrap_or(false)
    }
}

/// The markers the log delivered before the pipe connected, oldest first.
#[derive(Debug, Default)]
pub struct AlreadyDelivered(VecDeque<String>);

impl AlreadyDelivered {
    /// Whether the pipe's `label` is one the log already sent. A match also
    /// drops every older entry: the order is the same on both, so one the
    /// pipe never had (an `echo` from before the hook wrapped `echo`) is not
    /// waited for.
    pub fn seen(&mut self, label: &str) -> bool {
        if self.0.is_empty() {
            return false;
        }
        let key = label_key(label);
        let Some(at) = self.0.iter().position(|l| *l == key) else {
            return false;
        };
        self.0.drain(..=at);
        true
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A label as compared across the two sources, which may space an `echo`'s
/// arguments differently.
fn label_key(label: &str) -> String {
    label.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate_with(labels: &[&str]) -> AlreadyDelivered {
        let gate = MarkerGate::default();
        for label in labels {
            assert!(gate.admit_from_log(label));
        }
        gate.switch_to_pipe()
    }

    #[test]
    fn the_log_is_dropped_once_the_pipe_has_the_stream() {
        let gate = MarkerGate::default();
        assert!(gate.admit_from_log("DEMO_START 1 1 2"));
        assert_eq!(gate.switch_to_pipe().len(), 1);
        assert!(gate.pipe_active());
        assert!(!gate.admit_from_log("NEXT_CLIP 1 1 1 2"));
    }

    #[test]
    fn a_reconnect_skips_nothing() {
        let gate = MarkerGate::default();
        gate.admit_from_log("DEMO_START 1 1 2");
        assert_eq!(gate.switch_to_pipe().len(), 1);
        assert!(gate.switch_to_pipe().is_empty());
    }

    /// #1358: the hook's own line between two console markers is not taken
    /// for one the log sent, so `START_RECORD` is skipped and the line isn't.
    #[test]
    fn a_hook_only_line_is_not_counted_as_delivered() {
        let mut skip = gate_with(&[
            "DEMO_START 1 1 2",
            "NEXT_CLIP 1 1 1 2",
            "START_RECORD - Tick 900",
        ]);
        assert!(skip.seen("DEMO_START 1 1 2"));
        assert!(skip.seen("NEXT_CLIP 1 1 1 2"));
        assert!(!skip.seen("KILLFEED_LAYOUT\teven"));
        assert!(skip.seen("START_RECORD - Tick 900"));
        assert!(skip.is_empty());
        assert!(!skip.seen("STOP_RECORD - Tick 1200"));
    }

    /// A marker the pipe never had (echoed before the hook wrapped `echo`)
    /// doesn't hold up the ones after it.
    #[test]
    fn a_marker_missing_from_the_pipe_is_passed_over() {
        let mut skip = gate_with(&["BREADCRUMB - Tick 10", "DEMO_START 1 1 2"]);
        assert!(skip.seen("DEMO_START 1 1 2"));
        assert!(skip.is_empty());
    }

    #[test]
    fn spacing_does_not_stop_a_match() {
        let mut skip = gate_with(&["START_RECORD -  Tick 900 "]);
        assert!(skip.seen("START_RECORD - Tick 900"));
    }

    /// Two identical markers in a row are skipped one each.
    #[test]
    fn a_repeated_marker_is_skipped_once_per_delivery() {
        let mut skip = gate_with(&["BREADCRUMB - Tick 5000"]);
        assert!(skip.seen("BREADCRUMB - Tick 5000"));
        assert!(!skip.seen("BREADCRUMB - Tick 5000"));
    }
}
