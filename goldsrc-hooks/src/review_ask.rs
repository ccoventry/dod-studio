//! Asking DoD Studio to review the demo the game has loaded (#1142).
//!
//! The DoD Studio window's Review tab has **From the start** and **From
//! here** while no review runs (`ds_demo_review ask start` / `ask here`).
//! Each sends DoD Studio a request over its game pipe (`studio_pipe`,
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
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
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
    /// Studio sent the review, at this time; the game starts it in a moment.
    Sent(Instant),
    Failed(String),
}

static STATUS: RwLock<Status> = RwLock::new(Status::Idle);

/// Bumped by every `ds_demo_review start`. Studio's `done` can reach the
/// request's thread after the game has already run the `start` it announced
/// (the command pipe and this one race), so a request that ends after a start
/// ends idle instead of saying the review is on its way ([`settled`]).
static STARTS: AtomicU32 = AtomicU32::new(0);

/// How long a sent review keeps the buttons greyed: the game runs Studio's
/// `start` within a frame or two, so one that hasn't come by then was lost,
/// and the buttons may ask again.
const SENT_GRACE: Duration = Duration::from_secs(10);

/// What the Review tab says about the last request.
pub(crate) fn status() -> Status {
    shown(STATUS.try_read().map_or(Status::Idle, |s| s.clone()))
}

/// `status` as the tab shows it: a sent review that never started, past
/// [`SENT_GRACE`], is idle again.
fn shown(status: Status) -> Status {
    match status {
        Status::Sent(at) if at.elapsed() >= SENT_GRACE => Status::Idle,
        other => other,
    }
}

fn set_status(status: Status) {
    if let Ok(mut s) = STATUS.write() {
        *s = status;
    }
}

/// The review started (or a queue failed to): whatever the last request said
/// is done with.
pub(crate) fn review_started() {
    if let Ok(mut s) = STATUS.write() {
        STARTS.fetch_add(1, Ordering::Relaxed);
        *s = Status::Idle;
    }
}

/// The status a request ends on: idle when a review started while it was on
/// its way (`started`), since its `done` then announces one already running.
fn settled(end: Status, started: bool) -> Status {
    if started { Status::Idle } else { end }
}

/// Sets the request's last status, against the [`STARTS`] it saw when sent;
/// under the status lock, so a start can't slip between the check and the set.
fn set_end(end: Status, starts_then: u32) {
    if let Ok(mut s) = STATUS.write() {
        *s = settled(end, STARTS.load(Ordering::Relaxed) != starts_then);
    }
}

/// Whether a request is on its way, so a second click doesn't send another.
static IN_FLIGHT: AtomicBool = AtomicBool::new(false);

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
        RUNNING.store(crate::studio_pipe::studio_serves(), Ordering::Relaxed);
    }
    RUNNING.load(Ordering::Relaxed)
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
    let starts_then = STARTS.load(Ordering::Relaxed);
    let spawned = std::thread::Builder::new()
        .name("review-ask".into())
        .spawn(move || {
            let end = ask(&line);
            log(&format!("{end:?}"));
            set_end(end, starts_then);
            IN_FLIGHT.store(false, Ordering::Release);
        });
    if let Err(e) = spawned {
        IN_FLIGHT.store(false, Ordering::Release);
        set_status(Status::Idle);
        return Err(format!("could not start asking DoD Studio: {e}"));
    }
    Ok(())
}

/// The demo player as an ask waiting for it sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum DemoNow {
    /// A demo is in the player and read.
    Playing,
    /// A demo is still being read.
    Loading,
    /// No demo.
    Absent,
}

/// What an ask waiting for its demo does this frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum WaitStep {
    Ask,
    Wait,
    GiveUp(String),
}

/// How long an ask waits for its demo to load: as long as the review waits
/// for one it loads itself.
const WAIT_FOR_DEMO: Duration = Duration::from_secs(180);

/// The ask waiting for the demo to finish loading: 0 for none, 1 for
/// [`Start::First`], 2 for [`Start::At`] the time in [`WAITING_AT_BITS`]. An
/// atomic, since the review's poll looks every frame.
static WAITING: AtomicU8 = AtomicU8::new(0);
/// The clock time of a waiting From here, as `f64` bits: taken when it was
/// asked, since the demo plays while it still loads and the clock moves on
/// meanwhile.
static WAITING_AT_BITS: AtomicU64 = AtomicU64::new(0);
/// When it started waiting, as milliseconds since [`epoch`].
static WAITING_SINCE_MS: AtomicU64 = AtomicU64::new(0);

/// What a waiting ask does with the demo as it is, `waited` after it was
/// asked (#1152's follow-up: an ask sent while the demo still loaded was
/// answered "no demo is playing").
pub(crate) fn wait_step(demo: DemoNow, waited: Duration) -> WaitStep {
    match demo {
        DemoNow::Playing => WaitStep::Ask,
        DemoNow::Loading if waited < WAIT_FOR_DEMO => WaitStep::Wait,
        DemoNow::Loading => {
            WaitStep::GiveUp("the demo did not finish loading in three minutes".to_string())
        }
        DemoNow::Absent => WaitStep::GiveUp("the demo stopped loading".to_string()),
    }
}

/// Asks once the loading demo plays; the tab says so meanwhile, with its
/// buttons greyed.
pub(crate) fn wait_for_demo(start: Start) {
    WAITING_SINCE_MS.store(epoch().elapsed().as_millis() as u64, Ordering::Relaxed);
    let mode = match start {
        Start::First => 1,
        Start::At(seconds) => {
            WAITING_AT_BITS.store(seconds.to_bits(), Ordering::Relaxed);
            2
        }
    };
    WAITING.store(mode, Ordering::Release);
    set_status(Status::Asking(
        "Waiting for the demo to load, then asking DoD Studio...".to_string(),
    ));
}

/// The ask waiting for its demo, and how long it has waited.
pub(crate) fn waiting() -> Option<(Start, Duration)> {
    let start = match WAITING.load(Ordering::Acquire) {
        1 => Start::First,
        2 => Start::At(f64::from_bits(WAITING_AT_BITS.load(Ordering::Relaxed))),
        _ => return None,
    };
    let since = WAITING_SINCE_MS.load(Ordering::Relaxed);
    let now = epoch().elapsed().as_millis() as u64;
    Some((start, Duration::from_millis(now.saturating_sub(since))))
}

/// Ends the wait: `why` it gave up, or none when it asks now or a review
/// started meanwhile.
pub(crate) fn stop_waiting(why: Option<String>) {
    WAITING.store(0, Ordering::Release);
    set_status(why.map_or(Status::Idle, Status::Failed));
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
    let Ok(mut pipe) = crate::studio_pipe::connect() else {
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
            Reply::Done => return Status::Sent(Instant::now()),
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
    fn a_done_that_comes_after_its_review_started_ends_idle() {
        let sent = Status::Sent(Instant::now());
        assert_eq!(settled(sent.clone(), false), sent);
        assert_eq!(settled(sent, true), Status::Idle);
    }

    #[test]
    fn a_sent_review_that_never_starts_frees_the_buttons() {
        let fresh = Status::Sent(Instant::now());
        assert_eq!(shown(fresh.clone()), fresh);
        if let Some(old) = Instant::now().checked_sub(SENT_GRACE) {
            assert_eq!(shown(Status::Sent(old)), Status::Idle);
        }
    }

    #[test]
    fn the_requests_are_the_ones_studio_reads() {
        let requests = include_str!("../../native/src/sys/game_review_requests.rs");
        assert!(requests.contains("`review start <absolute path to the .dem>`"));
        assert!(requests.contains("`review at <seconds> <absolute path>`"));
    }

    #[test]
    fn an_ask_while_the_demo_loads_waits_for_it_to_play() {
        let s = Duration::from_secs;
        assert_eq!(wait_step(DemoNow::Loading, s(6)), WaitStep::Wait);
        assert_eq!(wait_step(DemoNow::Playing, s(6)), WaitStep::Ask);
        assert!(matches!(
            wait_step(DemoNow::Loading, s(181)),
            WaitStep::GiveUp(_)
        ));
        assert!(matches!(
            wait_step(DemoNow::Absent, s(1)),
            WaitStep::GiveUp(_)
        ));
    }

    #[test]
    fn a_waiting_ask_greys_the_buttons_until_it_ends() {
        // From here keeps the clock it was asked at: the demo plays on while
        // it loads.
        wait_for_demo(Start::At(42.5));
        assert_eq!(waiting().map(|(start, _)| start), Some(Start::At(42.5)));
        assert!(matches!(status(), Status::Asking(_)));
        stop_waiting(Some("the demo stopped loading".into()));
        assert_eq!(waiting(), None);
        assert_eq!(status(), Status::Failed("the demo stopped loading".into()));
        stop_waiting(None);
        assert_eq!(status(), Status::Idle);
    }
}
