//! Asking DoD Studio to review the demo the game has loaded (#1142).
//!
//! The DoD Studio window's Review tab has **From the start** and **From
//! here** while no review runs (`ds_demo_review ask start` / `ask here`).
//! Each sends DoD Studio a request over its game pipe ([`STUDIO_PIPE`],
//! `native::sys::game_review_requests` on Studio's side): the loaded demo's
//! full path, and for From here the demo player's clock now. Studio reviews
//! that demo as its Review highlights button would, scanning it first if
//! needed, and sends `ds_demo_review start` back over the command pipe as it
//! always does; meanwhile it answers on the request's connection with what
//! it is doing (`progress <text>`), then `done` or `failed <why>`, which the
//! tab shows ([`status`]).
//!
//! The request runs on a thread of its own: Studio may take a while
//! (scanning, or starting the review), and the game's thread never waits on
//! a pipe. The buttons are greyed while nothing serves the pipe (DoD Studio
//! isn't running), looked up at most once a second while the tab shows
//! ([`studio_running`]).

// The pipe is 32-bit only in practice; a host build compiles the rest for
// the tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::path::{Component, Path, PathBuf};
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Where the review starts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Start {
    /// The demo's first highlight.
    First,
    /// The first highlight whose first kill is at or after this time on the
    /// demo player's clock.
    At(f64),
}

/// The line Studio reads: `review start <path>` or `review at <seconds>
/// <path>` (`native::sys::game_review_requests::parse_request`).
pub(crate) fn request_line(demo: &Path, start: Start) -> String {
    let path = demo.to_string_lossy();
    match start {
        Start::First => format!("review start {path}\n"),
        Start::At(seconds) => format!("review at {seconds:.3} {path}\n"),
    }
}

/// The loaded demo's full path: the name `viewdemo` was given, from the
/// game's `dod` folder, with `.dem` added when missing and `..` worked out
/// (`viewdemo "../../demos/a"` names `<Steam>\demos\a.dem`).
pub(crate) fn demo_path(dod_dir: &Path, name: &str) -> PathBuf {
    let joined = crate::streaks::demo_path(dod_dir, name);
    let mut out = PathBuf::new();
    for part in joined.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// What the last request is doing, for the Review tab.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Status {
    /// Nothing asked, or the review it asked for has started.
    Idle,
    /// Asked; Studio's latest word on it.
    Asking(String),
    /// Studio sent the review; the game starts it in a moment.
    Sent,
    Failed(String),
}

static STATUS: RwLock<Status> = RwLock::new(Status::Idle);

/// What the Review tab says about the last request.
pub(crate) fn status() -> Status {
    STATUS.try_read().map_or(Status::Idle, |s| s.clone())
}

fn set_status(status: Status) {
    if let Ok(mut s) = STATUS.write() {
        *s = status;
    }
}

/// The review started: whatever the last request said is done with.
pub(crate) fn review_started() {
    set_status(Status::Idle);
}

/// Whether a request is on its way, so a second click doesn't send another.
static IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// DoD Studio's game pipe; the same as `streaks::STUDIO_PIPE`.
const STUDIO_PIPE: &str = crate::streaks::STUDIO_PIPE;

/// How often [`studio_running`] looks again.
const LOOK_EVERY: Duration = Duration::from_secs(1);
/// When it last looked, as milliseconds since [`epoch`], and what it saw.
static LOOKED_MS: AtomicU64 = AtomicU64::new(0);
static RUNNING: AtomicBool = AtomicBool::new(false);

fn epoch() -> Instant {
    static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// Whether DoD Studio serves its game pipe, as last seen: looked up again
/// when the last look is over [`LOOK_EVERY`] old. The look is
/// `WaitNamedPipeW` with a 1 ms wait, which returns at once whether the pipe
/// exists or not (Studio always keeps an instance listening).
pub(crate) fn studio_running() -> bool {
    let now = epoch().elapsed().as_millis() as u64 + 1;
    let last = LOOKED_MS.load(Ordering::Relaxed);
    if last == 0 || now.saturating_sub(last) >= LOOK_EVERY.as_millis() as u64 {
        LOOKED_MS.store(now, Ordering::Relaxed);
        RUNNING.store(pipe_exists(), Ordering::Relaxed);
    }
    RUNNING.load(Ordering::Relaxed)
}

#[cfg(windows)]
fn pipe_exists() -> bool {
    use windows_sys::Win32::System::Pipes::WaitNamedPipeW;
    let wide: Vec<u16> = STUDIO_PIPE.encode_utf16().chain([0]).collect();
    // Safety: a NUL-terminated name; a 1 ms wait at most.
    unsafe { WaitNamedPipeW(wide.as_ptr(), 1) != 0 }
}

#[cfg(not(windows))]
fn pipe_exists() -> bool {
    false
}

/// One line of Studio's answer.
#[derive(Debug, PartialEq)]
enum Reply {
    Progress(String),
    Done,
    Failed(String),
    /// Anything else, ignored, so a later Studio can add lines.
    Other,
}

fn parse_reply(line: &str) -> Reply {
    let line = line.trim_end_matches('\r');
    if let Some(text) = line.strip_prefix("progress ") {
        Reply::Progress(text.to_string())
    } else if line == "done" {
        Reply::Done
    } else if let Some(why) = line.strip_prefix("failed ") {
        Reply::Failed(why.to_string())
    } else {
        Reply::Other
    }
}

/// Sends the request for `demo` from `start` on a thread of its own, and
/// follows Studio's answer into [`status`]. Says why instead when one is
/// already on its way.
pub(crate) fn send(demo: PathBuf, start: Start) -> Result<(), String> {
    if IN_FLIGHT.swap(true, Ordering::AcqRel) {
        return Err("DoD Studio is still working on the last request".to_string());
    }
    set_status(Status::Asking("Asking DoD Studio...".to_string()));
    let line = request_line(&demo, start);
    let spawned = std::thread::Builder::new()
        .name("review-ask".into())
        .spawn(move || {
            let end = ask(&line);
            log(&format!("{end:?}"));
            set_status(end);
            IN_FLIGHT.store(false, Ordering::Release);
        });
    if let Err(e) = spawned {
        IN_FLIGHT.store(false, Ordering::Release);
        set_status(Status::Idle);
        return Err(format!("could not start asking DoD Studio: {e}"));
    }
    Ok(())
}

/// To the hook's log only: this runs off the game's thread, where the
/// console can't be printed to.
fn log(line: &str) {
    // Safety: `report` only appends to the log file.
    unsafe { crate::debug::report(&format!("review ask: {line}")) };
}

/// One request, start to end: the status it ends on.
fn ask(line: &str) -> Status {
    use std::io::{BufRead, BufReader, Write};
    let Ok(mut pipe) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(STUDIO_PIPE)
    else {
        return Status::Failed("DoD Studio isn't running".to_string());
    };
    if let Err(e) = pipe.write_all(line.as_bytes()) {
        return Status::Failed(format!("could not ask DoD Studio: {e}"));
    }
    for line in BufReader::new(pipe).lines() {
        let Ok(line) = line else { break };
        match parse_reply(&line) {
            Reply::Progress(text) => {
                log(&format!("DoD Studio: {text}"));
                set_status(Status::Asking(text));
            }
            Reply::Done => return Status::Sent,
            Reply::Failed(why) => return Status::Failed(why),
            Reply::Other => {}
        }
    }
    Status::Failed("DoD Studio stopped answering".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_request_names_where_to_start_and_the_demo() {
        let demo = Path::new(r"C:\Steam\dod\a b.dem");
        assert_eq!(
            request_line(demo, Start::First),
            "review start C:\\Steam\\dod\\a b.dem\n"
        );
        assert_eq!(
            request_line(demo, Start::At(356.25)),
            "review at 356.250 C:\\Steam\\dod\\a b.dem\n"
        );
    }

    #[test]
    fn a_demo_named_from_the_game_folder_becomes_a_plain_full_path() {
        let dod = Path::new(r"C:\Steam\common\Half-Life\dod");
        assert_eq!(
            demo_path(dod, "../../../demos/./m"),
            PathBuf::from(r"C:\Steam\demos\m.dem")
        );
        assert_eq!(
            demo_path(dod, "ktps8w11sf.dem"),
            PathBuf::from(r"C:\Steam\common\Half-Life\dod\ktps8w11sf.dem")
        );
    }

    #[test]
    fn studios_answer_is_read_line_by_line() {
        assert_eq!(
            parse_reply("progress Scanning the demo...\r"),
            Reply::Progress("Scanning the demo...".into())
        );
        assert_eq!(parse_reply("done"), Reply::Done);
        assert_eq!(parse_reply("failed nope"), Reply::Failed("nope".into()));
        assert_eq!(parse_reply("cache v14"), Reply::Other);
    }

    #[test]
    fn the_pipe_is_studios() {
        let server = include_str!("../../native/src/sys/analysis_server.rs");
        assert!(server.contains(&format!("pub const PIPE_NAME: &str = r\"{STUDIO_PIPE}\";")));
        let requests = include_str!("../../native/src/sys/game_review_requests.rs");
        assert!(requests.contains("`review start <absolute path to the .dem>`"));
        assert!(requests.contains("`review at <seconds> <absolute path>`"));
    }
}
