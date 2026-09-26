//! Console commands from DoD Studio, while the game runs (issue #413).
//!
//! Studio already knows when the game is open; this lets it do something
//! about it, like sending `viewdemo <demo>_preview` to the running game when
//! Launch Preview is clicked, instead of only saying the game is running.
//!
//! ## How
//!
//! A background thread serves a Windows named pipe named after this process,
//! [`pipe_name`] -- `\\.\pipe\dodstudio-hl-<pid>` -- so Studio, which finds the
//! running `hl.exe` by its process id, knows exactly which pipe to open. A
//! client writes newline-separated console commands and closes. Each line is
//! checked ([`commands_in`]) and queued; the engine's per-frame poll, on the
//! main thread, runs the queue through the engine's own `pfnClientCmd`. The
//! engine is not thread-safe, which is why the pipe thread only queues.
//!
//! ## Who can talk to it
//!
//! The pipe refuses remote clients (`PIPE_REJECT_REMOTE_CLIENTS`), and with
//! the default security descriptor only this user (and administrators and
//! SYSTEM) may write to it -- the same people who can type into the console.
//! No network port is opened.
//!
//! It only exists in a game launched by Studio, the only way this DLL is
//! loaded. On by default; `GOLDSRC_HOOKS_REMOTE=0` turns it off.

// The pipe itself is 32-bit only; a host build compiles the rest for the tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, channel};

/// Whether to open the pipe at all -- `GOLDSRC_HOOKS_REMOTE=0` turns it off.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

/// The longest command accepted, in bytes. The console's own line limit is
/// in this region, and anything longer is not something Studio sends.
const MAX_COMMAND: usize = 1024;
/// The most bytes read from one client connection.
const MAX_MESSAGE: usize = 64 * 1024;
/// The most commands run in one frame, so a burst can't stall a frame.
const PER_FRAME: usize = 32;

/// Where Studio finds this game: named after the process, so two games (or
/// a stale pipe) can never be confused.
pub fn pipe_name(pid: u32) -> String {
    format!(r"\\.\pipe\dodstudio-hl-{pid}")
}

/// The commands in one client's message: one per line, blank lines skipped.
/// A line that is too long or carries control characters is dropped rather
/// than truncated, since half a command is worse than none.
pub fn commands_in(message: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(message)
        .split('\n')
        .map(|line| line.trim_end_matches('\r').trim())
        .filter(|line| {
            !line.is_empty()
                && line.len() <= MAX_COMMAND
                && !line.chars().any(|c| c.is_control() && c != '\t')
        })
        .map(str::to_string)
        .collect()
}

/// The queue's receiving end, read only by the main thread. `try_lock` there
/// never waits: nothing else ever takes this lock.
static QUEUE: Mutex<Option<Receiver<String>>> = Mutex::new(None);

#[cfg(target_arch = "x86")]
mod server {
    use std::ffi::CString;
    use std::sync::mpsc::Sender;

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_PIPE_CONNECTED, GetLastError, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{PIPE_ACCESS_INBOUND, ReadFile};
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
        PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;

    use super::*;

    /// One client at a time: accept, read to the end, queue, repeat.
    pub(super) fn serve(sender: Sender<String>) {
        let name = pipe_name(unsafe { GetCurrentProcessId() });
        let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
        unsafe { crate::debug::report(&format!("remote: listening on {name} (#413)")) };
        loop {
            // Safety: a NUL-terminated name and plain flags; the default
            // security descriptor.
            let pipe = unsafe {
                CreateNamedPipeW(
                    wide.as_ptr(),
                    PIPE_ACCESS_INBOUND,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    1,
                    0,
                    4096,
                    0,
                    std::ptr::null(),
                )
            };
            if pipe == INVALID_HANDLE_VALUE {
                unsafe {
                    crate::debug::report(&format!(
                        "remote: could not create {name} (error {}); no commands from Studio this session",
                        GetLastError()
                    ))
                };
                return;
            }
            // Safety: our own handle; blocks until a client opens the pipe.
            let connected = unsafe {
                ConnectNamedPipe(pipe, std::ptr::null_mut()) != 0
                    || GetLastError() == ERROR_PIPE_CONNECTED
            };
            if connected {
                let mut message = Vec::new();
                let mut buffer = [0u8; 4096];
                loop {
                    let mut read = 0u32;
                    // Safety: reading into a local buffer of the stated size.
                    let ok = unsafe {
                        ReadFile(
                            pipe,
                            buffer.as_mut_ptr(),
                            buffer.len() as u32,
                            &mut read,
                            std::ptr::null_mut(),
                        )
                    };
                    if ok == 0 || read == 0 {
                        break; // the client closed its end
                    }
                    message.extend_from_slice(&buffer[..read as usize]);
                    if message.len() > MAX_MESSAGE {
                        break;
                    }
                }
                for command in commands_in(&message) {
                    if sender.send(command).is_err() {
                        return;
                    }
                }
            }
            // Safety: our own handle, done with.
            unsafe {
                DisconnectNamedPipe(pipe);
                CloseHandle(pipe);
            }
        }
    }

    /// Runs one command on the main thread.
    pub(super) fn run(command: &str) {
        let Ok(line) = CString::new(format!("{command}\n")) else {
            return;
        };
        if crate::engine::client_cmd(&line) {
            unsafe {
                crate::debug::report(&format!(
                    "remote: Studio ran \"{}\"",
                    command.escape_debug()
                ))
            };
        }
    }
}

/// Opens the pipe on a thread of its own, when the engine is ready to run
/// commands. Only the first call does anything: the engine-ready callback runs
/// again if `client.dll` is ever reloaded, and a second server could not
/// create the (single-instance) pipe the first one already holds.
pub fn start() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return;
    }
    if !ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
        unsafe {
            crate::debug::report(
                "remote: off (GOLDSRC_HOOKS_REMOTE=0) -- Studio can't send commands (#413)",
            )
        };
        return;
    }
    let (sender, receiver) = channel();
    if let Ok(mut queue) = QUEUE.lock() {
        *queue = Some(receiver);
    }
    #[cfg(target_arch = "x86")]
    std::thread::spawn(move || server::serve(sender));
    #[cfg(not(target_arch = "x86"))]
    drop(sender);
}

/// Runs what Studio has sent, from `commands::poll` on the main thread.
pub fn poll() {
    let Ok(queue) = QUEUE.try_lock() else {
        return;
    };
    let Some(receiver) = queue.as_ref() else {
        return;
    };
    for _ in 0..PER_FRAME {
        let Ok(command) = receiver.try_recv() else {
            return;
        };
        #[cfg(target_arch = "x86")]
        server::run(&command);
        #[cfg(not(target_arch = "x86"))]
        drop(command);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Studio builds the same name (`native::sys::game_remote::pipe_name`);
    /// the two must agree to the character.
    #[test]
    fn the_pipe_is_named_after_the_process() {
        assert_eq!(pipe_name(4242), r"\\.\pipe\dodstudio-hl-4242");
    }

    #[test]
    fn a_message_splits_into_commands() {
        assert_eq!(
            commands_in(
                b"viewdemo \"temp demos/a\"\r\n\n  dodstudio_clear_decals  \necho a;echo b"
            ),
            vec![
                "viewdemo \"temp demos/a\"",
                "dodstudio_clear_decals",
                "echo a;echo b"
            ]
        );
    }

    #[test]
    fn bad_lines_are_dropped_not_cut() {
        let long = "x".repeat(MAX_COMMAND + 1);
        let message = format!("echo ok\n{long}\necho \u{7}bell\n");
        assert_eq!(commands_in(message.as_bytes()), vec!["echo ok"]);
    }
}
