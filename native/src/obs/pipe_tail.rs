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

use std::io::{BufRead, BufReader, Read};
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

    /// Runs until `cancel` is raised or the receiver hangs up. While
    /// connected it waits for the game's next line, the game closing its end
    /// (it exits, or the batch kills it), or `cancel`, and lets go of the
    /// pipe on any of them.
    pub fn run(self, tx: Sender<Marker>, cancel: Arc<AtomicBool>, gate: Arc<MarkerGate>) {
        while !cancel.load(Ordering::Relaxed) {
            let Some((pid, pipe)) = open_any_game() else {
                std::thread::sleep(RETRY);
                continue;
            };
            let skip = gate.switch_to_pipe();
            let pipe = UntilCancelled {
                pipe,
                cancel: Arc::clone(&cancel),
            };
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

/// How often a connected reader looks for new bytes and for `cancel`. Short:
/// OBS mode starts and stops recording on these markers.
const POLL: Duration = Duration::from_millis(5);

/// The events pipe, read so that `cancel` ends the read rather than a line
/// that may never come. A batch run in a game that stays open (#725) ends
/// with the game still serving the pipe, which takes one reader at a time
/// and only notices a reader gone at its next write: a reader left waiting
/// in a read would hold the pipe until the next batch's first marker, take
/// that marker and drop it, and keep the next batch's reader out until then.
struct UntilCancelled {
    pipe: std::fs::File,
    cancel: Arc<AtomicBool>,
}

impl Read for UntilCancelled {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                return Ok(0);
            }
            match bytes_waiting(&self.pipe) {
                Some(0) => std::thread::sleep(POLL),
                // Bytes waiting, so the read returns at once; or the pipe is
                // broken, and the read says how.
                _ => return self.pipe.read(buf),
            }
        }
    }
}

/// Bytes waiting in `pipe`, or `None` when it can't say (the game closed its
/// end, or this isn't a pipe).
#[cfg(windows)]
fn bytes_waiting(pipe: &std::fs::File) -> Option<u32> {
    use std::os::windows::io::AsRawHandle;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn PeekNamedPipe(
            pipe: *mut std::ffi::c_void,
            buffer: *mut std::ffi::c_void,
            buffer_size: u32,
            bytes_read: *mut u32,
            total_available: *mut u32,
            left_this_message: *mut u32,
        ) -> i32;
    }
    let mut available = 0u32;
    // SAFETY: the handle `pipe` owns; with no buffer only the count is written.
    let ok = unsafe {
        PeekNamedPipe(
            pipe.as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut available,
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(available)
}

/// No named pipes to peek at: the read blocks as a plain one does.
#[cfg(not(windows))]
fn bytes_waiting(_pipe: &std::fs::File) -> Option<u32> {
    None
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
    crate::sys::process::game_pids()
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

    /// #725: a cancelled reader lets go of the pipe at once, so the game's
    /// next write fails and is kept for the next reader, instead of the old
    /// reader taking it.
    #[cfg(windows)]
    #[test]
    fn a_cancelled_reader_lets_go_of_the_pipe() {
        use std::io::Write;
        use std::os::windows::io::FromRawHandle;

        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn CreateNamedPipeW(
                name: *const u16,
                open_mode: u32,
                pipe_mode: u32,
                max_instances: u32,
                out_buffer: u32,
                in_buffer: u32,
                default_timeout: u32,
                security: *mut std::ffi::c_void,
            ) -> *mut std::ffi::c_void;
            fn ConnectNamedPipe(
                pipe: *mut std::ffi::c_void,
                overlapped: *mut std::ffi::c_void,
            ) -> i32;
        }
        const PIPE_ACCESS_OUTBOUND: u32 = 0x2;
        let name = format!(r"\\.\pipe\dodstudio-pipe-tail-test-{}", std::process::id());
        let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
        // SAFETY: a NUL-terminated name, byte mode, no security attributes.
        let handle = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                PIPE_ACCESS_OUTBOUND,
                0,
                1,
                4096,
                0,
                0,
                std::ptr::null_mut(),
            )
        };
        assert_ne!(handle as isize, -1, "could not create {name}");
        let handle = handle as usize;
        let (wrote_tx, wrote_rx) = std::sync::mpsc::channel();
        let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
        let server = std::thread::spawn(move || {
            let handle = handle as *mut std::ffi::c_void;
            // SAFETY: the handle just created; blocks until the client opens it.
            unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) };
            // SAFETY: an owned pipe handle; the File closes it.
            let mut pipe = unsafe { std::fs::File::from_raw_handle(handle) };
            pipe.write_all(b"hello\n").unwrap();
            go_rx.recv().unwrap();
            wrote_tx.send(pipe.write_all(b"next\n").is_ok()).unwrap();
        });

        let mut client = None;
        for _ in 0..50 {
            match std::fs::OpenOptions::new().read(true).open(&name) {
                Ok(pipe) => {
                    client = Some(pipe);
                    break;
                }
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let reader = UntilCancelled {
            pipe: client.expect("the test pipe never opened"),
            cancel: Arc::clone(&cancel),
        };
        let reading = std::thread::spawn(move || {
            let mut lines = BufReader::new(reader).lines().map_while(Result::ok);
            let hello = lines.next();
            // Nothing more comes: only `cancel` ends this.
            let rest: Vec<String> = lines.collect();
            (hello, rest)
        });
        std::thread::sleep(Duration::from_millis(100));
        cancel.store(true, Ordering::Relaxed);
        let (hello, rest) = reading.join().unwrap();
        assert_eq!(hello.as_deref(), Some("hello"));
        assert!(rest.is_empty(), "{rest:?}");
        // The reader is gone: the game's next line fails, so it keeps it.
        go_tx.send(()).unwrap();
        assert!(!wrote_rx.recv().unwrap());
        server.join().unwrap();
    }
}
