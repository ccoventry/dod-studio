//! Reading the pipeline's markers from the game's events pipe (#434, step 1).
//!
//! The hook DLL (`goldsrc-hooks/src/events.rs`) forwards every `[dod-studio]`
//! `echo` to `\\.\pipe\dodstudio-hl-<pid>-events` as the engine runs it. That
//! is the same line `qconsole.log` would get, without `-condebug`, the file,
//! or polling it. This reader finds the running `hl.exe`, connects, and turns
//! each line into a [`Marker`] with `via_pipe` set.
//!
//! [`MarkerGate`] decides which source feeds the capture loop: the log until
//! this connects, the pipe after. A game without the hook (or with
//! `GOLDSRC_HOOKS_EVENTS=0`) never serves the pipe, so the log simply stays
//! the source, exactly as before.

use std::io::{BufRead, BufReader};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::Duration;

use super::log_tail::{Marker, MarkerGate, parse_marker};

/// How often to look for the game and its pipe before the first connection.
const RETRY: Duration = Duration::from_millis(250);

/// Connects to the game's events pipe and forwards its markers.
pub struct PipeTailer {
    /// Called once per connection, with the game's pid and its hello line.
    on_connect: Box<dyn Fn(u32, &str) + Send>,
}

impl PipeTailer {
    pub fn new(on_connect: impl Fn(u32, &str) + Send + 'static) -> Self {
        Self {
            on_connect: Box::new(on_connect),
        }
    }

    /// Runs until `cancel` is raised or the receiver hangs up. Blocks in a
    /// read while connected; the game closing its end (it exits, or the
    /// batch kills it) ends that read.
    pub fn run(self, tx: Sender<Marker>, cancel: Arc<AtomicBool>, gate: Arc<MarkerGate>) {
        while !cancel.load(Ordering::Relaxed) {
            let Some((pid, pipe)) = open_any_game() else {
                std::thread::sleep(RETRY);
                continue;
            };
            let skip = gate.switch_to_pipe();
            // Lossy, like the log: a mangled byte must never stop the stream.
            let mut lines = BufReader::new(pipe)
                .split(b'\n')
                .map_while(Result::ok)
                .map(|bytes| {
                    String::from_utf8_lossy(&bytes)
                        .trim_end_matches('\r')
                        .to_string()
                });
            match lines.next() {
                Some(hello) => (self.on_connect)(pid, &hello),
                None => continue,
            }
            if !forward(lines, skip, &tx) {
                return;
            }
        }
    }
}

/// Sends every marker in `lines` after the first `skip`, marked as from the
/// pipe. False when the receiver has hung up.
fn forward(lines: impl Iterator<Item = String>, mut skip: usize, tx: &Sender<Marker>) -> bool {
    for line in lines {
        let Some(mut marker) = parse_marker(&line) else {
            continue;
        };
        if skip > 0 {
            skip -= 1;
            continue;
        }
        marker.via_pipe = true;
        if tx.send(marker).is_err() {
            return false;
        }
    }
    true
}

/// The first running `hl.exe` whose events pipe opens.
fn open_any_game() -> Option<(u32, std::fs::File)> {
    crate::sys::process::pids_named(&["hl.exe"])
        .into_iter()
        .find_map(|pid| {
            let name = crate::sys::game_remote::events_pipe_name(pid);
            std::fs::OpenOptions::new()
                .read(true)
                .open(name)
                .ok()
                .map(|pipe| (pid, pipe))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::obs::MarkerKind;

    fn lines(text: &str) -> impl Iterator<Item = String> + '_ {
        text.lines().map(str::to_string)
    }

    #[test]
    fn markers_are_forwarded_and_marked_as_from_the_pipe() {
        let (tx, rx) = std::sync::mpsc::channel();
        assert!(forward(
            lines("[dod-studio] DEMO_START 1 2 3\nnoise\n[dod-studio] BATCH_COMPLETE"),
            0,
            &tx
        ));
        let got: Vec<Marker> = rx.try_iter().collect();
        assert_eq!(got.len(), 2);
        assert!(got.iter().all(|m| m.via_pipe));
        assert_eq!(got[0].kind, MarkerKind::DemoStart);
        assert_eq!(got[0].demo_progress, Some((1, 2, 3)));
        assert_eq!(got[1].kind, MarkerKind::BatchComplete);
    }

    #[test]
    fn markers_the_log_already_delivered_are_skipped() {
        // The log sent the first two before the pipe connected; the pipe's
        // replay starts from the game's first marker.
        let gate = MarkerGate::default();
        assert!(gate.admit_from_log());
        assert!(gate.admit_from_log());
        let skip = gate.switch_to_pipe();
        assert_eq!(skip, 2);
        assert!(
            !gate.admit_from_log(),
            "the log is dropped once the pipe has it"
        );
        assert!(gate.pipe_active());

        let (tx, rx) = std::sync::mpsc::channel();
        forward(
            lines(
                "[dod-studio] DEMO_START 1 1 2\n[dod-studio] NEXT_CLIP 1 1 1 2\n[dod-studio] AUDIO_SYNC - Tick 900",
            ),
            skip,
            &tx,
        );
        let got: Vec<Marker> = rx.try_iter().collect();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].kind, MarkerKind::AudioSync);
    }

    #[test]
    fn a_reconnect_skips_nothing() {
        let gate = MarkerGate::default();
        gate.admit_from_log();
        assert_eq!(gate.switch_to_pipe(), 1);
        assert_eq!(gate.switch_to_pipe(), 0);
    }
}
