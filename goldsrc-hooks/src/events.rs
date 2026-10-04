//! Game -> Studio events over a named pipe (issue #434, step 1).
//!
//! ## Why
//!
//! Studio learns what a capture batch is doing from `[dod-studio]` markers the
//! patched demo `echo`es: `DEMO_START`, `NEXT_CLIP`, `START_RECORD`,
//! `BATCH_COMPLETE` and the rest. Until now they reached Studio only through
//! `qconsole.log`, which needs `-condebug`, a writable game folder, a file
//! nothing else holds open, and polling -- and the batch's end was a folder
//! HLAE creates that Studio watched for.
//!
//! ## How
//!
//! The engine's `echo` command is wrapped through its command-list node
//! ([`crate::cmd_list`], no per-build address), so every `echo` -- typed, from
//! a `.cfg`, or read from a demo's own `ConsoleCommand` frames -- passes through
//! [`wrapped_echo`] before the engine prints it. A line carrying the
//! `[dod-studio]` tag is copied to a queue; everything else is left alone.
//!
//! A background thread serves an outbound named pipe,
//! `\\.\pipe\dodstudio-hl-<pid>-events` ([`events_pipe_name`]), and writes
//! the queue to it one line at a time. Lines queued before Studio connects
//! are kept and sent on connect, and a line that fails to write is sent again
//! after the next connect, so a slow or reconnecting Studio misses nothing.
//! The queue is capped ([`MAX_PENDING`]); past that, new lines are dropped
//! rather than growing without bound in a game nobody is listening to.
//!
//! On each connect the first line is a hello ([`HELLO`]), which carries no
//! tag, so a marker parser ignores it and a logger can note it.
//!
//! Same access rules as the inbound pipe (`remote.rs`): no remote clients,
//! the default security descriptor. On by default;
//! `GOLDSRC_HOOKS_EVENTS=0` turns it off.

// The pipe itself is 32-bit only; a host build compiles the rest for the tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc::{Sender, channel};

use crate::cmd_list::{self, call_real, wrap};

/// Whether to serve the pipe at all -- `GOLDSRC_HOOKS_EVENTS=0` turns it off.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

/// What the pipeline puts in front of every marker it echoes (Studio's
/// `native::obs::log_tail::LOG_TAG`).
const TAG: &str = "[dod-studio]";

/// The first line on every connection. Untagged, so it is never a marker.
pub const HELLO: &str = "dodstudio-hooks events 1";

/// Lines queued and not yet written. A batch echoes a few per clip plus a
/// breadcrumb every 5000 frames, so this is days of markers.
const MAX_PENDING: usize = 4096;

static PENDING: AtomicUsize = AtomicUsize::new(0);
static SENDER: OnceLock<Sender<String>> = OnceLock::new();

/// The engine's own `echo` handler.
static REAL_ECHO: AtomicUsize = AtomicUsize::new(0);
static WRAPPED: AtomicBool = AtomicBool::new(false);
/// How many more frames `poll` retries a wrap that found nothing.
static RETRIES_LEFT: AtomicU32 = AtomicU32::new(300);

/// Where Studio finds this game's events: named after the process, beside the
/// inbound `\\.\pipe\dodstudio-hl-<pid>`.
pub fn events_pipe_name(pid: u32) -> String {
    format!(r"\\.\pipe\dodstudio-hl-{pid}-events")
}

/// The line an `echo` prints, if it is one of the pipeline's markers.
fn marker_line(args: &[String]) -> Option<String> {
    let line = args.join(" ");
    let line = line.trim();
    (line.contains(TAG) && !line.contains(['\n', '\r'])).then(|| line.to_string())
}

/// Queues `line` for Studio. Never blocks, and drops the line past
/// [`MAX_PENDING`].
fn emit(line: String) {
    let Some(sender) = SENDER.get() else {
        return;
    };
    if PENDING.fetch_add(1, Ordering::AcqRel) >= MAX_PENDING {
        PENDING.fetch_sub(1, Ordering::AcqRel);
        return;
    }
    if sender.send(line).is_err() {
        PENDING.fetch_sub(1, Ordering::AcqRel);
    }
}

unsafe extern "C" fn wrapped_echo() {
    if let Some(line) = marker_line(&cmd_list::args()) {
        emit(line);
    }
    unsafe { call_real(&REAL_ECHO) };
}

/// Starts the pipe thread and wraps `echo`. Called once engfuncs are
/// captured; only the first call does anything.
pub fn install() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    if !ENABLED.load(Ordering::Relaxed) {
        RETRIES_LEFT.store(0, Ordering::Relaxed);
        unsafe {
            crate::debug::report(
                "events: off (GOLDSRC_HOOKS_EVENTS=0) -- Studio reads markers from qconsole.log (#434)",
            )
        };
        return;
    }
    let (sender, receiver) = channel::<String>();
    if SENDER.set(sender).is_err() {
        return;
    }
    #[cfg(target_arch = "x86")]
    std::thread::spawn(move || server::serve(receiver));
    #[cfg(not(target_arch = "x86"))]
    drop(receiver);
    if try_wrap() {
        RETRIES_LEFT.store(0, Ordering::Relaxed);
    }
}

/// Retries a wrap that has not found `echo` yet, for a few seconds of frames.
pub fn poll() {
    if RETRIES_LEFT.load(Ordering::Relaxed) == 0 {
        return;
    }
    if try_wrap() {
        RETRIES_LEFT.store(0, Ordering::Relaxed);
    } else if RETRIES_LEFT.fetch_sub(1, Ordering::Relaxed) == 1 {
        unsafe {
            crate::debug::report(
                "events: no echo command in the engine's command list -- Studio falls back to qconsole.log",
            )
        };
    }
}

fn try_wrap() -> bool {
    if WRAPPED.load(Ordering::Relaxed) {
        return true;
    }
    let mut found = false;
    cmd_list::for_each(|name, entry| {
        if name.eq_ignore_ascii_case(b"echo") {
            found |= wrap(entry, &REAL_ECHO, wrapped_echo);
        }
    });
    if found {
        WRAPPED.store(true, Ordering::Relaxed);
        unsafe {
            crate::debug::report(
                "events: wrapped echo; markers go to Studio over the events pipe (#434)",
            )
        };
    }
    found
}

/// One `dodstudio_debug_status` line.
pub fn status_line() -> Option<String> {
    WRAPPED.load(Ordering::Relaxed).then(|| {
        format!(
            "events pipe: {}, {} line(s) waiting",
            if server::CONNECTED.load(Ordering::Relaxed) {
                "Studio connected"
            } else {
                "no reader"
            },
            PENDING.load(Ordering::Relaxed)
        )
    })
}

mod server {
    use std::sync::atomic::AtomicBool;

    /// Whether a reader is connected right now.
    pub static CONNECTED: AtomicBool = AtomicBool::new(false);

    #[cfg(target_arch = "x86")]
    pub(super) use imp::serve;

    #[cfg(target_arch = "x86")]
    mod imp {
        use std::collections::VecDeque;
        use std::sync::atomic::Ordering;
        use std::sync::mpsc::{Receiver, RecvTimeoutError};
        use std::time::Duration;

        use windows_sys::Win32::Foundation::{
            CloseHandle, ERROR_PIPE_CONNECTED, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
        };
        use windows_sys::Win32::Storage::FileSystem::{PIPE_ACCESS_OUTBOUND, WriteFile};
        use windows_sys::Win32::System::Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
            PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcessId;

        use super::super::{HELLO, PENDING, events_pipe_name};
        use super::CONNECTED;

        fn write_line(pipe: HANDLE, line: &str) -> bool {
            let bytes = format!("{line}\n");
            let mut written = 0u32;
            // Safety: writing a local buffer to our own handle.
            let ok = unsafe {
                WriteFile(
                    pipe,
                    bytes.as_ptr(),
                    bytes.len() as u32,
                    &mut written,
                    std::ptr::null_mut(),
                )
            };
            ok != 0 && written as usize == bytes.len()
        }

        /// One reader at a time: accept, write the backlog and then every
        /// new line until a write fails, repeat.
        pub fn serve(receiver: Receiver<String>) {
            let name = events_pipe_name(unsafe { GetCurrentProcessId() });
            let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
            unsafe { crate::debug::report(&format!("events: serving {name} (#434)")) };
            // A line taken off the channel but not yet written.
            let mut retry: VecDeque<String> = VecDeque::new();
            loop {
                // Safety: a NUL-terminated name and plain flags; the default
                // security descriptor.
                let pipe = unsafe {
                    CreateNamedPipeW(
                        wide.as_ptr(),
                        PIPE_ACCESS_OUTBOUND,
                        PIPE_TYPE_BYTE
                            | PIPE_READMODE_BYTE
                            | PIPE_WAIT
                            | PIPE_REJECT_REMOTE_CLIENTS,
                        1,
                        64 * 1024,
                        0,
                        0,
                        std::ptr::null(),
                    )
                };
                if pipe == INVALID_HANDLE_VALUE {
                    unsafe {
                        crate::debug::report(&format!(
                            "events: could not create {name} (error {}); Studio reads qconsole.log instead",
                            GetLastError()
                        ))
                    };
                    return;
                }
                // Safety: our own handle; blocks until a reader opens the pipe.
                let connected = unsafe {
                    ConnectNamedPipe(pipe, std::ptr::null_mut()) != 0
                        || GetLastError() == ERROR_PIPE_CONNECTED
                };
                if connected && write_line(pipe, HELLO) {
                    CONNECTED.store(true, Ordering::Relaxed);
                    unsafe { crate::debug::report("events: Studio connected") };
                    loop {
                        let line = match retry.pop_front() {
                            Some(line) => line,
                            None => match receiver.recv_timeout(Duration::from_millis(500)) {
                                Ok(line) => line,
                                Err(RecvTimeoutError::Timeout) => continue,
                                Err(RecvTimeoutError::Disconnected) => break,
                            },
                        };
                        if !write_line(pipe, &line) {
                            retry.push_front(line);
                            break;
                        }
                        PENDING.fetch_sub(1, Ordering::AcqRel);
                    }
                    CONNECTED.store(false, Ordering::Relaxed);
                    unsafe { crate::debug::report("events: Studio disconnected") };
                }
                // Safety: our own handle, done with.
                unsafe {
                    DisconnectNamedPipe(pipe);
                    CloseHandle(pipe);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_string).collect()
    }

    /// Studio builds the same name (`native::sys::game_remote::events_pipe_name`).
    #[test]
    fn the_events_pipe_is_named_after_the_process() {
        assert_eq!(events_pipe_name(4242), r"\\.\pipe\dodstudio-hl-4242-events");
    }

    #[test]
    fn only_tagged_echoes_are_markers() {
        assert_eq!(
            marker_line(&args("[dod-studio] START_RECORD - Tick 41450")),
            Some("[dod-studio] START_RECORD - Tick 41450".to_string())
        );
        assert_eq!(
            marker_line(&args("[dod-studio] DEMO_START 5 14 2")),
            Some("[dod-studio] DEMO_START 5 14 2".to_string())
        );
        assert_eq!(marker_line(&args("hello there")), None);
        assert_eq!(marker_line(&[]), None);
    }

    /// The real pipe, end to end: lines queued before a reader connects are
    /// kept, and arrive after the hello, in order. 32-bit only, like the server.
    #[cfg(target_arch = "x86")]
    #[test]
    fn a_reader_gets_the_hello_then_everything_queued_before_it() {
        use std::io::BufRead;
        let (sender, receiver) = channel::<String>();
        for line in [
            "[dod-studio] DEMO_START 1 1 2",
            "[dod-studio] NEXT_CLIP 1 1 1 2",
        ] {
            PENDING.fetch_add(1, Ordering::AcqRel);
            sender.send(line.to_string()).unwrap();
        }
        std::thread::spawn(move || server::serve(receiver));

        let name = events_pipe_name(std::process::id());
        let started = std::time::Instant::now();
        let pipe = loop {
            match std::fs::OpenOptions::new().read(true).open(&name) {
                Ok(pipe) => break pipe,
                Err(_) if started.elapsed().as_secs() < 5 => {
                    std::thread::sleep(std::time::Duration::from_millis(20))
                }
                Err(e) => panic!("could not open {name}: {e}"),
            }
        };
        let mut lines = std::io::BufReader::new(pipe).lines();
        assert_eq!(lines.next().unwrap().unwrap(), HELLO);
        assert_eq!(
            lines.next().unwrap().unwrap(),
            "[dod-studio] DEMO_START 1 1 2"
        );
        sender
            .send("[dod-studio] BATCH_COMPLETE".to_string())
            .unwrap();
        PENDING.fetch_add(1, Ordering::AcqRel);
        assert_eq!(
            lines.next().unwrap().unwrap(),
            "[dod-studio] NEXT_CLIP 1 1 1 2"
        );
        assert_eq!(
            lines.next().unwrap().unwrap(),
            "[dod-studio] BATCH_COMPLETE"
        );
    }

    #[test]
    fn the_hello_is_never_a_marker() {
        assert!(!HELLO.contains(TAG));
    }
}
