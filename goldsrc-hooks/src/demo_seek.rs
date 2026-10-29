//! `ds_demo_seek_to <seconds>` / `ds_demo_seek_by <seconds>`: jump
//! `viewdemo` playback to a time, the way the demo editor's **Goto** button
//! does, instead of fast-forwarding to it with `host_framerate` (issue #405).
//!
//! ## What Goto is
//!
//! `viewdemo` is not `hw.dll`'s own demo reader. It hands the file to
//! `DemoPlayer.dll`, which loads the *whole* demo into an HLTV world
//! (`Core.dll`) and then plays that world back by its clock. The events list's
//! Goto button (`GameUI.dll`, `CDemoPlayerDialog`) is three calls on the
//! player's `IDemoPlayer` interface:
//!
//! ```text
//!   SetWorldTime(event->time, false)   slot 22
//!   ExecuteDirectorCmd(event)          slot 33
//!   SetPaused(true)                    slot 24
//! ```
//!
//! and `SetWorldTime` only stores the new clock (`this+0x3a8`). Everything else
//! happens on the player's next frame: `WriteDatagram` sends the world frame at
//! the new time, as a delta from the last one it sent.
//!
//! ## A forward seek is not a teleport
//!
//! The next frame also runs everything between the old position and the new
//! one, in one burst:
//!
//! - **Every director event** in `(last frame time, new time]`, because
//!   `WriteCommands(this+0x3c0, this+0x3a8)` walks the events list between the
//!   two clocks and `SetWorldTime` moved only one of them.
//! - **Every `ConsoleCommand` (type 3) frame** in the frames skipped, because
//!   `ReadDemoMessage` executes each world frame's demo data from the last one
//!   it sent (`this+0x3d8`) up to the new one.
//!
//! A backward seek runs nothing: both walks start past their end. So "events
//! between the old and new position do not fire" holds only going back
//! (an HLTV demo goes back differently, see below).
//!
//! By default these commands do exactly what the engine does. Set
//! `ds_demo_seek_skip_between 1` to make a seek land clean instead: it moves
//! the two "last sent" marks to the landing frame, so the next frame runs that
//! frame's own commands and nothing before it. That is what a capture batch
//! wants -- the commands in between belong to other clips -- but it is wrong
//! for a POV demo, whose type-3 frames are the player's own key presses: skip a
//! `-showscores` and the scoreboard stays up. HLTV demos record none.
//!
//! ## Long forward seeks go in steps (#596)
//!
//! The burst above goes out as **one** network message, built in the player's
//! 64 KB `m_DemoStream`. `World::WriteFrame` adds every skipped frame's
//! reliable data and user messages (`svc_updateuserinfo`, team and score
//! messages, ...) while they fit, then `WriteCommands` adds the director events
//! without checking; a stream that overflows is cleared whole ("Demo data
//! stream overflow." in the console) and the player carries on from the landing
//! frame alone. A 20-minute jump in an HLTV demo did exactly that (2026-10-04):
//! a player who joined after the recording began had no name and no team, the
//! scoreboard read 0/0, and `ds_spec_target` found no such player.
//!
//! So a forward seek of more than [`STEP_SECONDS`] moves the clock that far per
//! frame instead, from [`poll`], until it lands. Each step's catch-up is a few
//! seconds of the demo, which always fits, so everything in between reaches the
//! client in order, as if played very fast. A whole HLTV demo takes a few
//! hundred frames. `ds_demo_seek_skip_between 1` seeks still jump at once:
//! they skip the in-between on purpose.
//!
//! The console commands the steps replay are the exception: the engine runs
//! only one of those a frame, so a POV demo's key presses from the whole jump
//! would play on for tens of seconds after the landing. A stepped seek drops
//! them and lets go of the keys once it lands (`filtered_commands.rs`, #702).
//!
//! ## A backward seek in an HLTV demo starts over (#830)
//!
//! Going back replays nothing, so the client keeps what the later position
//! left it. In an HLTV demo that includes each player's zoom, which only the
//! `HLTV` user message carries (`hltv_fov.rs`): a sniper scoped in at the
//! landing showed unzoomed after Review's Replay. So a backward seek in an
//! HLTV demo ([`rewinds`]) clears the client's zoom table, sets the clock to
//! the demo's start, waits for that frame to be sent, and steps forward to
//! the target like a long forward seek. Every message before the landing
//! arrives in order, so teams, scores and names are right too. It costs a
//! step per 5 s of the demo before the landing. POV demos, which record the
//! zoom in every frame, and skip seeks still jump back at once.
//!
//! A command after the time (`ds_demo_seek_to <seconds> <command ...>`) runs
//! one frame after the seek lands, once the landing frame has reached the
//! client: the Highlights tab's Go uses it to put the camera on a player who
//! may only exist once the jump has caught up.
//!
//! ## Why only `viewdemo`
//!
//! `playdemo` never loads `DemoPlayer.dll`'s player; `hw.dll` streams the file
//! itself and has no clock to set. Under `playdemo` the player reports itself
//! inactive and these commands say so.
//!
//! The engine also has `dem_jump <seconds>` (relative, and pauses) from the same
//! DLL; it is `SetWorldTime(t, true)` followed by `SetPaused(true)`, so it
//! replays like Goto. These commands differ in taking an absolute time, never
//! pausing, refusing while the demo is still loading, and the skip option.
//!
//! ## Still loading
//!
//! The world fills in the background -- `Core.dll`'s `Server::RunFrame` reads at
//! most 33 packets per engine frame, or for a time budget per frame with
//! `ds_demo_fast_load` (#798) -- while playback has already started. A
//! seek past what is loaded lands on the last loaded frame. `IsLoading()`
//! (slot 28, "is the loader still connected") is the gate; the console prints
//! `Demo file completely loaded.` when it clears.
//!
//! ## The gap before a POV demo's first frame (#893)
//!
//! A POV demo's LOADING segment holds the connection's sign-on, from when the
//! recorder joined the server; its playback starts when they typed `record`,
//! often minutes later (median 103 s over 469 POV demos). The player's clock
//! starts at the LOADING frames, so the first stretch of it has no frames.
//!
//! The player means to jump such a gap: `RunClocks` (PRE `DemoPlayer.dll`
//! `+0x42e0`, Anniversary `+0x3c40`) moves the clock to 0.01 s before the next
//! frame when the frame after the last one sent is more than 2 s on. But it
//! looks only from the last frame *sent*, and `WriteDatagram` sends the first
//! frame at or after the clock. The LOADING frames span about 14 ms of server
//! time, so an engine frame longer than that (a loading frame often is) steps
//! the clock past them, sends the first playback frame straight away, and the
//! check then compares that frame with the one after it: no gap. The clock
//! then crawls through the whole gap at normal speed while `GetDemoViewInfo`
//! interpolates the view from the LOADING frame's position to the first
//! playback frame's: the view floats from where the recorder joined to where
//! the recording starts, for as long as the gap.
//!
//! [`skip_gap`] does the jump the player missed: whenever the frame at or after
//! the clock is the one already sent and more than [`GAP_SECONDS`] ahead of it,
//! the clock goes to 0.01 s before that frame, as `RunClocks` would have put
//! it. Never while paused (the player's clock doesn't run then either) or
//! while a seek is on its way.
//!
//! ## Finding the player
//!
//! `DemoPlayer.dll` exports only `CreateInterface`, and `demoplayer001` is a
//! singleton: its factory returns the address of one static object, the same
//! one `hw.dll` drives. No engine address is needed. Each call looks the DLL up
//! afresh, so a player that is unloaded or never loaded is reported rather than
//! remembered.
//!
//! ## Per build
//!
//! The pre-Anniversary and 25th Anniversary `DemoPlayer.dll`s are different
//! compiles (138 KB and 48 KB) with the same interface: 47 slots in the HL SDK
//! `IDemoPlayer.h` order, and the same field offsets. [`BUILDS`] names each by
//! its PE timestamp and image size, and anything else is refused rather than
//! trusted. `tools/verify_demo_seek_offsets.py` checks every slot and offset
//! here against both DLLs.

// The offsets are only read by the 32-bit build; a host build compiles them
// for the tests alone.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::ffi::{CStr, c_char};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use crate::commands::console_print;
use crate::engine::{self, CvarSPartial};
use crate::names::console_name;

pub const SEEK_TO_NAME: &str = console_name!("demo_seek_to");
pub const SEEK_BY_NAME: &str = console_name!("demo_seek_by");
pub const SKIP_BETWEEN_NAME: &str = console_name!("demo_seek_skip_between");

/// How far one step of a long forward seek moves the clock. Measured on four
/// HLTV demos (`analysis/examples/seek_burst.rs`, 2026-10-04): no 5-second
/// window held more than 55 KB of network data, entities included, and the
/// catch-up leaves entities out. `m_DemoStream` holds 64 KB.
pub const STEP_SECONDS: f64 = 5.0;

/// A seek still on its way, stepped by [`poll`].
struct Pending {
    target: f64,
    /// Run one frame after landing.
    then: Option<String>,
    /// The clock has been set to `target`; once that frame is sent, `then`
    /// runs.
    landed: bool,
    /// The clock value the last step set.
    last_set: f64,
    /// A backward seek in an HLTV demo first goes back to the start
    /// ([`rewinds`]): `m_LastFrameTime` before it, until the player has sent
    /// the start frame.
    rewind_from: Option<f64>,
    /// Frames spent waiting for the player to send the last step.
    waited: u32,
    steps: u32,
    /// Bytes of replayed console commands a stepped seek dropped
    /// (`filtered_commands`).
    dropped: usize,
    started: std::time::Instant,
}

/// How many frames a step waits for the player to send the one before it
/// before moving on regardless: a frame the world has no new data for (a gap,
/// or the end of the demo) is never sent.
const MAX_WAIT_FRAMES: u32 = 10;

/// Whether the player has sent the step that set the clock to `last_set`:
/// `WriteDatagram` records the clock it sent at in `m_LastFrameTime`.
fn sent(last_frame_time: f64, last_set: f64) -> bool {
    last_frame_time >= last_set - 0.001
}

static PENDING: std::sync::Mutex<Option<Pending>> = std::sync::Mutex::new(None);

/// Drops a seek still on its way, returning its target. A newly loaded demo
/// must not inherit the last one's steps: they would move the clock of a demo
/// still loading, where a seek is refused, and run the old seek's follow-up
/// in it. An HLTV backward seek steps for a second or two (#830), so a review
/// moving on to another demo can land inside one. `try_lock`: only `poll` and
/// a seek starting hold it, both on this thread and never across this call.
fn cancel_pending() -> Option<f64> {
    PENDING
        .try_lock()
        .ok()
        .and_then(|mut p| p.take())
        .map(|p| p.target)
}

/// One frame of a stepped seek: move the clock to `To`, or set it to the
/// target and land.
#[derive(Debug, PartialEq)]
enum Step {
    To(f64),
    Land(f64),
}

fn next_step(now: f64, target: f64) -> Step {
    if target - now > STEP_SECONDS {
        Step::To(now + STEP_SECONDS)
    } else {
        Step::Land(target)
    }
}

/// Whether a seek from `now` to `to` goes in steps.
fn stepped(now: f64, to: f64, skip_between: bool) -> bool {
    !skip_between && to - now > STEP_SECONDS
}

/// Whether a seek from `now` to `to` goes back to the demo's start first and
/// steps forward from there (#830): a backward seek in an HLTV demo, whose
/// zoom per player comes only from the `HLTV` messages a backward jump never
/// replays (`hltv_fov.rs`). Stepping from the start replays them all, in
/// order, along with every team, score and name change before the landing.
/// A POV demo carries its zoom in every frame, and a
/// `ds_demo_seek_skip_between` seek skips the in-between on purpose.
fn rewinds(now: f64, to: f64, skip_between: bool, hltv: bool) -> bool {
    hltv && !skip_between && to < now
}

/// Whether the player has sent the frame a rewind set the clock to: it
/// records the clock it sent at, which falls below where it was.
fn rewound(last_frame_time: f64, from: f64) -> bool {
    last_frame_time < from - 0.001
}

/// `ds_demo_seek_skip_between`: 1 lands a seek without running the director
/// events and console commands it jumps over. Off by default, so a seek does
/// what the engine's own Goto does until asked otherwise.
///
/// Read from the cvar when a seek runs, so it needs no per-frame poll. The flag
/// is the fallback path's, set by a plain toggle command when cvars could not
/// be registered.
pub static SKIP_BETWEEN: AtomicBool = AtomicBool::new(false);
static SKIP_BETWEEN_CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());

/// Called by `commands.rs` once `ds_demo_seek_skip_between` is registered.
pub fn set_skip_between_cvar(cvar: *mut CvarSPartial) {
    SKIP_BETWEEN_CVAR.store(cvar, Ordering::Release);
}

fn skip_between() -> bool {
    let cvar = SKIP_BETWEEN_CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        SKIP_BETWEEN.load(Ordering::Relaxed)
    } else {
        // Safety: the engine owns the cvar for the session.
        unsafe { (*cvar).value != 0.0 }
    }
}

/// For the fallback toggle command's bare-name query.
pub fn status() -> String {
    if skip_between() {
        "seeks skip the events and commands between the old and new position".to_string()
    } else {
        "seeks run the events and commands they jump over, like the editor's Goto".to_string()
    }
}

/// One `DemoPlayer.dll` build this module was checked against.
pub struct Build {
    pub name: &'static str,
    /// `IMAGE_FILE_HEADER::TimeDateStamp`.
    pub time_date_stamp: u32,
    /// `IMAGE_OPTIONAL_HEADER::SizeOfImage`.
    pub size_of_image: u32,
}

pub const BUILDS: [Build; 2] = [
    Build {
        name: "pre-Anniversary",
        time_date_stamp: 0x5f28_cf08,
        size_of_image: 0x2_8000,
    },
    Build {
        name: "25th Anniversary",
        time_date_stamp: 0x6704_9a3c,
        size_of_image: 0xc000,
    },
];

/// `IDemoPlayer` vftable slots, HL SDK order (after `ISystemModule`'s 15).
const SLOT_SET_WORLD_TIME: usize = 22;
/// `SetTimeScale(float)`, `SetPaused(bool)`, `IsPaused()` and
/// `GetTimeScale()`: the review mode (#623) plays each highlight at normal
/// speed and pauses at its end.
const SLOT_SET_TIME_SCALE: usize = 23;
const SLOT_SET_PAUSED: usize = 24;
const SLOT_IS_PAUSED: usize = 27;
const SLOT_GET_TIME_SCALE: usize = 37;
const SLOT_IS_LOADING: usize = 28;
const SLOT_IS_ACTIVE: usize = 29;
const SLOT_GET_WORLD_TIME: usize = 34;
const SLOT_GET_START_TIME: usize = 35;
const SLOT_GET_END_TIME: usize = 36;

/// `IWorld::GetFrameByTime(double)`, the call `WriteDatagram` makes first.
const WORLD_SLOT_GET_FRAME_BY_TIME: usize = 19;

/// `DemoPlayer` fields, the same in both builds.
const FIELD_WORLD: usize = 0x14c;
const FIELD_LAST_FRAME_TIME: usize = 0x3c0;
const FIELD_LAST_FRAME_SEQ_NR: usize = 0x3d8;

/// `frame_t` fields: the server time as a float, then the sequence number.
const FRAME_SEQ_NR: usize = 4;

/// What a seek's time may look like, for the refusal line.
const TIME_FORMS: &str = "seconds (610), m:ss (10:10) or h:mm:ss (1:02:03)";

/// Parses a seek's time (#928): seconds (`610`, `610.5`, `-30`), `m:ss`
/// (`10:10`, `-0:30`) or `h:mm:ss` (`1:02:03`). The last field may have a
/// fraction; a field after the first is below 60.
fn parse_time(raw: &str) -> Result<f64, String> {
    let refuse = || format!("expected {TIME_FORMS}, got \"{raw}\"");
    let text = raw.trim();
    let value = if !text.contains(':') {
        text.parse::<f64>().map_err(|_| refuse())?
    } else {
        let (negative, body) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let fields: Vec<&str> = body.split(':').collect();
        if fields.len() > 3 {
            return Err(refuse());
        }
        let (last, whole) = fields.split_last().ok_or_else(refuse)?;
        let mut total = 0.0;
        for (i, field) in whole.iter().enumerate() {
            if field.is_empty() || !field.bytes().all(|b| b.is_ascii_digit()) {
                return Err(refuse());
            }
            let n: f64 = field.parse().map_err(|_| refuse())?;
            // Minutes under hours are below 60; the first field is any size.
            if i > 0 && n >= 60.0 {
                return Err(refuse());
            }
            total = total * 60.0 + n;
        }
        // Seconds: digits, then maybe a fraction; no sign, no exponent.
        let (int, frac) = last.split_once('.').unwrap_or((last, "0"));
        if int.is_empty()
            || frac.is_empty()
            || !int.bytes().all(|b| b.is_ascii_digit())
            || !frac.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(refuse());
        }
        let seconds: f64 = last.parse().map_err(|_| refuse())?;
        if seconds >= 60.0 {
            return Err(refuse());
        }
        let total = total * 60.0 + seconds;
        if negative { -total } else { total }
    };
    if value.is_finite() {
        Ok(value)
    } else {
        Err(refuse())
    }
}

/// The time at the front of a seek's arguments, and the words after it. The
/// console splits unquoted text at `:`, so `10:10` arrives as `10`, `:`, `10`
/// and is joined back here; a quoted `"10:10"` arrives whole.
fn split_time(words: &[String]) -> Option<(String, &[String])> {
    let (first, mut rest) = words.split_first()?;
    let mut time = first.clone();
    while let [colon, field, after @ ..] = rest
        && colon == ":"
    {
        time.push(':');
        time.push_str(field);
        rest = after;
    }
    Some((time, rest))
}

/// A time as the demo bar shows it: `m:ss`, `h:mm:ss` from an hour, with
/// hundredths when it has them, and a minus when negative.
fn clock_text(seconds: f64) -> String {
    let sign = if seconds < 0.0 { "-" } else { "" };
    let hundredths = (seconds.abs() * 100.0).round() as u64;
    let (h, m, s, cs) = (
        hundredths / 360_000,
        hundredths / 6000 % 60,
        hundredths / 100 % 60,
        hundredths % 100,
    );
    let secs = if cs == 0 {
        format!("{s:02}")
    } else {
        format!("{s:02}.{cs:02}")
    };
    if h > 0 {
        format!("{sign}{h}:{m:02}:{secs}")
    } else {
        format!("{sign}{m}:{secs}")
    }
}

/// Where a seek lands: `arg` from the start of the world clock, or from `now`
/// when relative, kept inside what has been loaded.
fn landing(now: f64, arg: f64, relative: bool, start: f64, end: f64) -> f64 {
    let wanted = if relative { now + arg } else { arg };
    if end < start {
        return wanted;
    }
    wanted.clamp(start, end)
}

/// The "last sent" frame to leave behind for a clean landing on `landing_seq`,
/// or `None` when the seek stays on the frame already sent -- moving the mark
/// back would run that frame's commands a second time.
fn skip_mark(last_sent_seq: u32, landing_seq: u32) -> Option<u32> {
    (landing_seq != last_sent_seq).then(|| landing_seq.saturating_sub(1))
}

/// What the console command reads: the time, and the command to run once the
/// seek lands, if one follows it.
fn arguments(name: &str) -> Result<(String, Option<String>), String> {
    let usage =
        || format!("usage: {name} <time> [command to run once there]; the time is {TIME_FORMS}");
    let engfuncs = engine::engfuncs().ok_or_else(|| "the engine is not ready".to_string())?;
    // Cmd_Argc counts the command name itself.
    let argc = unsafe { (engfuncs.cmd_argc)() };
    if argc < 2 {
        return Err(usage());
    }
    let mut args = Vec::new();
    for i in 1..argc {
        let raw = unsafe { (engfuncs.cmd_argv)(i) };
        if raw.is_null() {
            return Err(usage());
        }
        args.push(
            unsafe { CStr::from_ptr(raw as *const c_char) }
                .to_string_lossy()
                .into_owned(),
        );
    }
    let (time, rest) = split_time(&args).ok_or_else(usage)?;
    Ok((time, follow_up(rest)))
}

/// The words after the time, as one command line, or `None` when there are none.
fn follow_up(words: &[String]) -> Option<String> {
    (!words.is_empty()).then(|| words.join(" "))
}

fn run(name: &'static str, relative: bool) {
    let result = arguments(name).and_then(|(raw, then)| {
        parse_time(&raw).and_then(|seconds| start(name, seconds, relative, then))
    });
    let line = match result {
        Ok(done) => format!("{name}: {done}"),
        Err(why) => format!("{name}: {why}"),
    };
    console_print(&format!("{line}\n"));
    unsafe { crate::debug::report(&format!("demo_seek: {line}")) };
}

/// Starts a typed seek: drops one still waiting for its demo, then waits
/// with this one when it came on the same line as `viewdemo`, or runs it.
fn start(
    name: &'static str,
    seconds: f64,
    relative: bool,
    then: Option<String>,
) -> Result<String, String> {
    cancel_queued("you started another seek");
    match ARMED.load(Ordering::Relaxed) {
        ARMED_VIEWDEMO => {
            let line = format!(
                "will seek {} {} once the demo has loaded",
                if relative { "by" } else { "to" },
                clock_text(seconds)
            );
            *QUEUED.lock().unwrap_or_else(|e| e.into_inner()) = Some(Queued {
                name,
                seconds,
                relative,
                then,
                watch: Watch::default(),
                queued: std::time::Instant::now(),
                last_look: std::time::Instant::now(),
            });
            Ok(line)
        }
        ARMED_PLAYDEMO => Err("not viewing a demo (works under viewdemo, not playdemo)".into()),
        _ => player::seek(seconds, relative, skip_between(), then),
    }
}

/// ## A seek typed with the demo (#928)
///
/// `viewdemo <demo>; ds_demo_seek_to 10:05` reaches the seek while the demo
/// has only begun to load, which takes 10-25 s while it already plays. Such a
/// seek waits ([`Queued`]) and runs once the demo has loaded. "Typed with the
/// demo" is "on the same command line": `viewdemo`'s wrapper
/// (`demo_reload.rs`) sets [`ARMED`] after the engine's handler returns, the
/// rest of the line runs in the same `Cbuf_Execute`, and the next [`poll`]
/// clears it. A seek typed later during loading is refused as before.
///
/// Anything the user does to the demo first cancels the wait, with a console
/// line: another seek, loading or closing a demo, pausing, a speed change, or
/// a move of the clock that playback doesn't explain (a demo-bar click or
/// drag). Only the last queued seek counts.
const ARMED_NONE: u8 = 0;
const ARMED_VIEWDEMO: u8 = 1;
const ARMED_PLAYDEMO: u8 = 2;
static ARMED: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(ARMED_NONE);

/// Which demo command just ran, for [`demo_started`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DemoCommand {
    Viewdemo,
    Playdemo,
}

/// Called by the `viewdemo` / `playdemo` wrappers once the engine's handler
/// has run with a demo name: a seek waiting for the last demo is dropped, and
/// a seek later on this line waits for this one.
pub fn demo_started(command: DemoCommand) {
    cancel_queued("you loaded another demo");
    if let Some(target) = cancel_pending() {
        say(&format!(
            "{SEEK_TO_NAME}: stopped the seek to {} (you loaded another demo)",
            clock_text(target)
        ));
    }
    // The client keeps the last demo's zooms otherwise (#830); none of this
    // demo's messages have arrived yet.
    crate::hltv_fov::clear_logged("a demo was loaded");
    let armed = match command {
        DemoCommand::Viewdemo => ARMED_VIEWDEMO,
        DemoCommand::Playdemo => ARMED_PLAYDEMO,
    };
    ARMED.store(armed, Ordering::Relaxed);
}

struct Queued {
    name: &'static str,
    seconds: f64,
    relative: bool,
    then: Option<String>,
    watch: Watch,
    queued: std::time::Instant,
    last_look: std::time::Instant,
}

static QUEUED: std::sync::Mutex<Option<Queued>> = std::sync::Mutex::new(None);

/// Drops a waiting seek, saying why.
fn cancel_queued(why: &str) {
    let queued = QUEUED.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(q) = queued {
        say(&cancelled_line(&q, why));
    }
}

fn cancelled_line(q: &Queued, why: &str) -> String {
    format!(
        "{}: cancelled the seek {} {} ({why})",
        q.name,
        if q.relative { "by" } else { "to" },
        clock_text(q.seconds)
    )
}

fn say(line: &str) {
    console_print(&format!("{line}\n"));
    unsafe { crate::debug::report(&format!("demo_seek: {line}")) };
}

/// What a waiting seek sees of the player on one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Look {
    active: bool,
    loading: bool,
    paused: bool,
    now: f64,
    scale: f32,
    /// The clock sits just before a frame, where the player's own gap jump
    /// (`RunClocks`) and [`skip_gap`] put it.
    before_frame: bool,
}

#[derive(Debug, Default)]
struct Watch {
    /// The new demo has been seen loading; until then the player may still
    /// hold the last one.
    seen_loading: bool,
    frames: u32,
    last: Option<Look>,
}

#[derive(Debug, PartialEq)]
enum Verdict {
    Wait,
    Fire,
    Cancel(&'static str),
}

/// Frames after which a demo never seen loading counts as loaded: a small
/// one can be read between two looks (as in `review.rs`).
const UNSEEN_LOAD_FRAMES: u32 = 300;
/// How long a seek waits for a demo player that never becomes active (a
/// demo that failed to open).
const GIVE_UP_SECONDS: f64 = 60.0;
/// How far the clock may move back between two frames before it counts as
/// the user moving it.
const BACK_SLACK: f64 = 0.5;
/// Frames after the seek is typed before a move of the clock counts: right
/// after a second `viewdemo` the player still shows the last demo's clock,
/// and the new one's jumps several seconds in its first frames (measured on
/// PRE: 368.5 s, then 375.4 s a frame later).
const SETTLE_FRAMES: u32 = 30;

/// One frame of a waiting seek: `look` is the player now, `since_last` the
/// seconds since the last look, `waited` since the seek was typed.
fn judge(w: &mut Watch, look: Option<Look>, since_last: f64, waited: f64) -> Verdict {
    w.frames = w.frames.saturating_add(1);
    let Some(look) = look.filter(|l| l.active) else {
        if w.seen_loading {
            return Verdict::Cancel("you closed the demo");
        }
        if waited > GIVE_UP_SECONDS {
            return Verdict::Cancel("the demo never started");
        }
        return Verdict::Wait;
    };
    if !w.seen_loading {
        if look.loading {
            w.seen_loading = true;
            w.last = Some(look);
            return Verdict::Wait;
        }
        return if w.frames > UNSEEN_LOAD_FRAMES {
            Verdict::Fire
        } else {
            Verdict::Wait
        };
    }
    if let Some(last) = w.last {
        if look.paused && !last.paused {
            return Verdict::Cancel("you paused");
        }
        if look.scale != last.scale {
            return Verdict::Cancel("you changed the speed");
        }
        let moved = look.now - last.now;
        let played = 1.0 + 2.0 * since_last * f64::from(look.scale.max(0.0));
        if w.frames > SETTLE_FRAMES
            && (moved < -BACK_SLACK || (moved > played && !look.before_frame))
        {
            return Verdict::Cancel("you moved the demo's clock");
        }
    }
    w.last = Some(look);
    if look.loading {
        Verdict::Wait
    } else {
        Verdict::Fire
    }
}

fn look() -> Option<Look> {
    #[cfg(target_arch = "x86")]
    return player::look();
    #[cfg(not(target_arch = "x86"))]
    None
}

/// One frame of a seek waiting for its demo: watches, cancels or runs it.
fn poll_queued() {
    let Ok(mut queued) = QUEUED.try_lock() else {
        return;
    };
    let Some(q) = queued.as_mut() else {
        return;
    };
    let since_last = q.last_look.elapsed().as_secs_f64();
    q.last_look = std::time::Instant::now();
    let verdict = judge(
        &mut q.watch,
        look(),
        since_last,
        q.queued.elapsed().as_secs_f64(),
    );
    match verdict {
        Verdict::Wait => {}
        Verdict::Cancel(why) => {
            let line = cancelled_line(q, why);
            *queued = None;
            drop(queued);
            say(&line);
        }
        Verdict::Fire => {
            let Some(q) = queued.take() else { return };
            drop(queued);
            let what = format!(
                "the demo has loaded; seeking {} {}",
                if q.relative { "by" } else { "to" },
                clock_text(q.seconds)
            );
            say(&format!("{}: {what}", q.name));
            let line = match player::seek(q.seconds, q.relative, skip_between(), q.then) {
                Ok(done) => format!("{}: {done}", q.name),
                Err(why) => format!("{}: {why}", q.name),
            };
            say(&line);
        }
    }
}

/// While a `viewdemo` demo is still being read (#465): how many seconds of
/// it the player holds so far. `None` once loaded, or with no demo player.
pub fn buffered_while_loading() -> Option<f64> {
    #[cfg(target_arch = "x86")]
    return player::buffered_while_loading();
    #[cfg(not(target_arch = "x86"))]
    None
}

/// Steps a long forward seek on by one frame, runs a landed seek's
/// follow-up command, and watches a seek waiting for its demo to load.
/// Called every frame; two uncontended locks when idle.
pub fn poll() {
    poll_pending();
    poll_queued();
    // The command line that ran viewdemo has run whole by now (see `start`).
    ARMED.store(ARMED_NONE, Ordering::Relaxed);
}

fn poll_pending() {
    let Ok(mut pending) = PENDING.try_lock() else {
        return;
    };
    let Some(seek) = pending.as_mut() else {
        drop(pending);
        skip_gap();
        return;
    };
    // A stepped seek drops the console commands each step replays: the
    // engine runs only one a frame, so they would play on long after the
    // landing (`filtered_commands`).
    let stepped = seek.steps > 0 || !seek.landed;
    if stepped {
        seek.dropped += crate::filtered_commands::clear().unwrap_or(0);
    }
    #[cfg(target_arch = "x86")]
    let done = player::step(seek);
    #[cfg(not(target_arch = "x86"))]
    let done: Result<bool, String> = Err("only a 32-bit x86 build can drive DemoPlayer.dll".into());
    let line = match done {
        Ok(false) => return,
        Ok(true) => {
            // What the dropped commands pressed would stay down.
            if stepped
                && let Ok(line) =
                    std::ffi::CString::new(format!("{}\n", crate::filtered_commands::RELEASE_KEYS))
            {
                engine::client_cmd(&line);
            }
            if let Some(then) = &seek.then
                && let Ok(line) = std::ffi::CString::new(format!("{then}\n"))
            {
                // Typed during a review, `ds_demo_seek_to` itself is
                // refused, so a follow-up now is the review's camera line.
                crate::review_lock::let_through(then);
                engine::client_cmd(&line);
            }
            format!(
                "{SEEK_TO_NAME}: at {:.2} s after {} step(s), {} ms, {} bytes of replayed commands dropped; client clock {:.2}",
                seek.target,
                seek.steps,
                seek.started.elapsed().as_millis(),
                seek.dropped,
                engine::engine_client_time().unwrap_or(-1.0)
            )
        }
        Err(why) => format!("{SEEK_TO_NAME}: stopped stepping: {why}"),
    };
    *pending = None;
    unsafe { crate::debug::report(&format!("demo_seek: {line}")) };
}

/// The demo player's own gap threshold (`RunClocks`): a stretch longer than
/// this with no frames is jumped.
const GAP_SECONDS: f64 = 2.0;
/// Where `RunClocks` puts the clock ahead of the frame after a gap.
const GAP_LANDING_BEFORE: f32 = 0.01;

/// Where the clock should go when it sits in a gap the player failed to jump
/// (see the module doc): `frame` is the frame at or after the clock, `sent`
/// the last frame the player sent.
fn gap_landing(now: f64, frame_time: f32, frame_seq: u32, sent_seq: u32) -> Option<f64> {
    (frame_seq == sent_seq && f64::from(frame_time) - now > GAP_SECONDS)
        .then(|| f64::from(frame_time - GAP_LANDING_BEFORE))
}

/// Jumps the clock over a gap the demo player missed (#893). Only once
/// `Core.dll` is in: `viewdemo` loads it, and the player's world is null
/// before.
fn skip_gap() {
    #[cfg(target_arch = "x86")]
    if let Some((from, to)) = player::skip_gap() {
        unsafe {
            crate::debug::report(&format!(
                "demo_seek: jumped {:.1} s with no frames ({from:.2} -> {to:.2} s), the gap the demo player meant to skip (#893)",
                to - from
            ))
        };
    }
}

/// Whether a seek is still on its way (a stepped one, or one waiting to land),
/// for the capture batch (#434), which runs nothing until it has.
pub fn seeking() -> bool {
    match PENDING.try_lock() {
        Ok(pending) => pending.is_some(),
        // Held only by `poll` and a seek starting, both mid-seek.
        Err(_) => true,
    }
}

/// Where the demo player is, for the review mode (#623).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clock {
    /// A `viewdemo` demo is in the player.
    pub active: bool,
    /// It is still being read.
    pub loading: bool,
    pub paused: bool,
    /// The world clock, the one [`SEEK_TO_NAME`] takes.
    pub now: f64,
    pub start: f64,
    pub end: f64,
    pub time_scale: f32,
}

/// The demo player's clock, or `None` with no demo player (or one of a
/// build this was not checked against).
/// The demo player's `IDemoPlayer` interface, for checking a GameUI dialog
/// points at the same player (#663).
pub fn player_object() -> Option<*mut std::ffi::c_void> {
    #[cfg(target_arch = "x86")]
    return player::find().ok().map(|(p, _)| p);
    #[cfg(not(target_arch = "x86"))]
    None
}

pub fn clock() -> Option<Clock> {
    #[cfg(target_arch = "x86")]
    return player::clock();
    #[cfg(not(target_arch = "x86"))]
    None
}

/// Pauses the demo player, or plays on.
pub fn set_paused(paused: bool) -> Result<(), String> {
    #[cfg(target_arch = "x86")]
    return player::set_paused(paused);
    #[cfg(not(target_arch = "x86"))]
    {
        let _ = paused;
        Err("only the 32-bit build has a demo player".to_string())
    }
}

/// Sets the demo player's speed (1 is normal).
pub fn set_time_scale(scale: f32) -> Result<(), String> {
    #[cfg(target_arch = "x86")]
    return player::set_time_scale(scale);
    #[cfg(not(target_arch = "x86"))]
    {
        let _ = scale;
        Err("only the 32-bit build has a demo player".to_string())
    }
}

/// [`SEEK_TO_NAME`]'s seek, for code: lands on `seconds` of the world clock.
pub fn seek_to_seconds(seconds: f64) -> Result<String, String> {
    seek_to_seconds_then(seconds, None)
}

/// [`seek_to_seconds`], then runs `then` (a console line) once the seek has
/// landed, as `ds_demo_seek_to <seconds> <command>` does.
pub fn seek_to_seconds_then(seconds: f64, then: Option<String>) -> Result<String, String> {
    cancel_queued("you started another seek");
    #[cfg(target_arch = "x86")]
    return player::seek(seconds, false, skip_between(), then);
    #[cfg(not(target_arch = "x86"))]
    {
        let _ = (seconds, then);
        Err("only the 32-bit build has a demo player".to_string())
    }
}

/// `ds_demo_seek_to <seconds>`: an absolute world time, the clock the events
/// list shows and the analysis calls `viewdemo_offset`.
pub unsafe extern "C" fn seek_to() {
    run(SEEK_TO_NAME, false);
}

/// `ds_demo_seek_by <seconds>`: forward, or back when negative.
pub unsafe extern "C" fn seek_by() {
    run(SEEK_BY_NAME, true);
}

#[cfg(target_arch = "x86")]
mod player {
    use std::ffi::c_void;

    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    use super::*;

    type CreateInterfaceFn = unsafe extern "C" fn(*const c_char, *mut i32) -> *mut c_void;
    // `bool` crosses as a byte in a 4-byte slot. On return MSVC sets only `al`
    // (the Anniversary `IsActive` is `cmp; setne al; ret`), while rustc takes a
    // `u8`/`bool` return as already widened to 32 bits -- so read all of `eax`
    // and keep the low byte, see [`is_set`].
    type SetWorldTimeFn = unsafe extern "thiscall" fn(*mut c_void, f64, u32);
    type ByteFn = unsafe extern "thiscall" fn(*mut c_void) -> u32;
    type TimeFn = unsafe extern "thiscall" fn(*mut c_void) -> f64;
    type SetPausedFn = unsafe extern "thiscall" fn(*mut c_void, u32);
    type SetTimeScaleFn = unsafe extern "thiscall" fn(*mut c_void, f32);
    type TimeScaleFn = unsafe extern "thiscall" fn(*mut c_void) -> f32;
    type FrameByTimeFn = unsafe extern "thiscall" fn(*mut c_void, f64) -> *const u8;

    /// The `IDemoPlayer` singleton, once `DemoPlayer.dll` is loaded and is a
    /// build in [`BUILDS`].
    pub(super) fn find() -> Result<(*mut c_void, &'static Build), String> {
        let module = unsafe { GetModuleHandleA(c"DemoPlayer.dll".as_ptr() as *const u8) };
        if module.is_null() {
            return Err("DemoPlayer.dll is not loaded; start the demo with viewdemo".into());
        }
        let Some((stamp, size)) = (unsafe { crate::pe::image_identity(module as *mut u8) }) else {
            return Err("DemoPlayer.dll has no readable PE header".into());
        };
        let Some(build) = BUILDS
            .iter()
            .find(|b| b.time_date_stamp == stamp && b.size_of_image == size)
        else {
            return Err(format!(
                "DemoPlayer.dll is a build this was not checked against (timestamp {stamp:#x}, size {size:#x})"
            ));
        };
        let Some(create) =
            (unsafe { GetProcAddress(module, c"CreateInterface".as_ptr() as *const u8) })
        else {
            return Err("DemoPlayer.dll exports no CreateInterface".into());
        };
        // Safety: the export's signature is the Source/GoldSrc interface
        // factory's, and this build is one the verify script checked.
        let create: CreateInterfaceFn = unsafe { std::mem::transmute(create) };
        let player = unsafe { create(c"demoplayer001".as_ptr(), std::ptr::null_mut()) };
        if player.is_null() {
            return Err("DemoPlayer.dll did not hand out demoplayer001".into());
        }
        Ok((player, build))
    }

    /// A `bool` an MSVC method returned: `al`, whatever the rest of `eax` holds.
    fn is_set(eax: u32) -> bool {
        eax & 0xff != 0
    }

    /// Slot `index` of `object`'s vftable.
    unsafe fn slot<F: Copy>(object: *mut c_void, index: usize) -> F {
        unsafe {
            let vftable = *(object as *const *const usize);
            std::mem::transmute_copy(&*vftable.add(index))
        }
    }

    /// While a `viewdemo` demo is still being read: how much of it the
    /// player holds, in seconds (end minus start of the buffered world).
    pub(super) fn buffered_while_loading() -> Option<f64> {
        let (player, _) = find().ok()?;
        // Safety: the same slots `seek` uses, checked against both builds.
        unsafe {
            let is_active: ByteFn = slot(player, SLOT_IS_ACTIVE);
            let is_loading: ByteFn = slot(player, SLOT_IS_LOADING);
            if !is_set(is_active(player)) || !is_set(is_loading(player)) {
                return None;
            }
            let get_start: TimeFn = slot(player, SLOT_GET_START_TIME);
            let get_end: TimeFn = slot(player, SLOT_GET_END_TIME);
            Some(get_end(player) - get_start(player))
        }
    }

    pub(super) fn clock() -> Option<Clock> {
        let (player, _) = find().ok()?;
        // Safety: every slot is checked against both builds by
        // tools/verify_demo_seek_offsets.py, and `find` refused any other.
        unsafe {
            let is_active: ByteFn = slot(player, SLOT_IS_ACTIVE);
            let is_loading: ByteFn = slot(player, SLOT_IS_LOADING);
            let is_paused: ByteFn = slot(player, SLOT_IS_PAUSED);
            let get_now: TimeFn = slot(player, SLOT_GET_WORLD_TIME);
            let get_start: TimeFn = slot(player, SLOT_GET_START_TIME);
            let get_end: TimeFn = slot(player, SLOT_GET_END_TIME);
            let get_scale: TimeScaleFn = slot(player, SLOT_GET_TIME_SCALE);
            Some(Clock {
                active: is_set(is_active(player)),
                loading: is_set(is_loading(player)),
                paused: is_set(is_paused(player)),
                now: get_now(player),
                start: get_start(player),
                end: get_end(player),
                time_scale: get_scale(player),
            })
        }
    }

    /// What a waiting seek watches (#928): the clock, and whether it sits
    /// just before a frame, where a gap jump lands it.
    pub(super) fn look() -> Option<Look> {
        let c = clock()?;
        let mut before_frame = false;
        if let Ok((player, _)) = find() {
            // Safety: the field and slot `skip_gap` reads, checked against
            // both builds by tools/verify_demo_seek_offsets.py.
            unsafe {
                let world = *((player as *const u8).add(FIELD_WORLD) as *const *mut c_void);
                if !world.is_null() {
                    let frame_by_time: FrameByTimeFn = slot(world, WORLD_SLOT_GET_FRAME_BY_TIME);
                    let frame = frame_by_time(world, c.now);
                    if !frame.is_null() {
                        let ahead = f64::from((frame as *const f32).read_unaligned()) - c.now;
                        before_frame = (0.0..=0.02).contains(&ahead);
                    }
                }
            }
        }
        Some(Look {
            active: c.active,
            loading: c.loading,
            paused: c.paused,
            now: c.now,
            scale: c.time_scale,
            before_frame,
        })
    }

    /// [`super::skip_gap`]'s look and jump: the clock before and after.
    pub(super) fn skip_gap() -> Option<(f64, f64)> {
        // Safety: a plain lookup of a loaded module by name.
        if unsafe { GetModuleHandleA(c"Core.dll".as_ptr() as *const u8) }.is_null() {
            return None;
        }
        let (player, _) = find().ok()?;
        // Safety: the slots and fields `seek` uses, checked against both
        // builds by tools/verify_demo_seek_offsets.py.
        unsafe {
            let is_active: ByteFn = slot(player, SLOT_IS_ACTIVE);
            let is_paused: ByteFn = slot(player, SLOT_IS_PAUSED);
            if !is_set(is_active(player)) || is_set(is_paused(player)) {
                return None;
            }
            let world = *((player as *const u8).add(FIELD_WORLD) as *const *mut c_void);
            if world.is_null() {
                return None;
            }
            let get_now: TimeFn = slot(player, SLOT_GET_WORLD_TIME);
            let now = get_now(player);
            let frame_by_time: FrameByTimeFn = slot(world, WORLD_SLOT_GET_FRAME_BY_TIME);
            let frame = frame_by_time(world, now);
            if frame.is_null() {
                return None;
            }
            let frame_time = (frame as *const f32).read_unaligned();
            let frame_seq = (frame.add(FRAME_SEQ_NR) as *const u32).read_unaligned();
            let sent_seq =
                ((player as *const u8).add(FIELD_LAST_FRAME_SEQ_NR) as *const u32).read_unaligned();
            let to = gap_landing(now, frame_time, frame_seq, sent_seq)?;
            let set_world_time: SetWorldTimeFn = slot(player, SLOT_SET_WORLD_TIME);
            set_world_time(player, to, 0);
            Some((now, to))
        }
    }

    pub(super) fn set_paused(paused: bool) -> Result<(), String> {
        let (player, _) = find()?;
        // Safety: as in `clock`.
        unsafe {
            let set: SetPausedFn = slot(player, SLOT_SET_PAUSED);
            set(player, paused as u32);
        }
        Ok(())
    }

    pub(super) fn set_time_scale(scale: f32) -> Result<(), String> {
        let (player, _) = find()?;
        // Safety: as in `clock`.
        unsafe {
            let set: SetTimeScaleFn = slot(player, SLOT_SET_TIME_SCALE);
            set(player, scale);
        }
        Ok(())
    }

    /// One frame of a stepped seek: `Ok(true)` once it has landed and a frame
    /// has passed since.
    pub(super) fn step(seek: &mut Pending) -> Result<bool, String> {
        let (player, _) = find()?;
        // Safety: the slots `seek` uses, checked against both builds, and
        // `m_LastFrameTime`, which `seek`'s skip already writes.
        unsafe {
            let is_active: ByteFn = slot(player, SLOT_IS_ACTIVE);
            if !is_set(is_active(player)) {
                return Err("the demo stopped".into());
            }
            // One step per datagram: two steps sent together could overflow.
            // A rewind's start frame goes first, or the first step would be
            // a backward jump that replays nothing.
            let last_frame_time =
                ((player as *const u8).add(FIELD_LAST_FRAME_TIME) as *const f64).read_unaligned();
            let ready = match seek.rewind_from {
                Some(from) => rewound(last_frame_time, from),
                None => sent(last_frame_time, seek.last_set),
            };
            if !ready && seek.waited < MAX_WAIT_FRAMES {
                seek.waited += 1;
                return Ok(false);
            }
            seek.waited = 0;
            seek.rewind_from = None;
            if seek.landed {
                return Ok(true);
            }
            let get_now: TimeFn = slot(player, SLOT_GET_WORLD_TIME);
            let set_world_time: SetWorldTimeFn = slot(player, SLOT_SET_WORLD_TIME);
            let t = match next_step(get_now(player), seek.target) {
                Step::To(t) => t,
                Step::Land(t) => {
                    seek.landed = true;
                    t
                }
            };
            set_world_time(player, t, 0);
            seek.last_set = t;
        }
        seek.steps += 1;
        Ok(false)
    }

    pub(super) fn seek(
        arg: f64,
        relative: bool,
        skip_between: bool,
        then: Option<String>,
    ) -> Result<String, String> {
        let (player, build) = find()?;
        unsafe { crate::debug::report(&format!("demo_seek: {} DemoPlayer.dll", build.name)) };
        // Safety: every slot and field below is checked against both builds by
        // tools/verify_demo_seek_offsets.py, and `find` refused any other build.
        unsafe {
            let is_active: ByteFn = slot(player, SLOT_IS_ACTIVE);
            if !is_set(is_active(player)) {
                return Err("not viewing a demo (works under viewdemo, not playdemo)".into());
            }
            let get_end: TimeFn = slot(player, SLOT_GET_END_TIME);
            let is_loading: ByteFn = slot(player, SLOT_IS_LOADING);
            if is_set(is_loading(player)) {
                return Err(format!(
                    "the demo is still loading (up to {:.1} s so far); wait for \"Demo file completely loaded.\"",
                    get_end(player)
                ));
            }
            let get_now: TimeFn = slot(player, SLOT_GET_WORLD_TIME);
            let get_start: TimeFn = slot(player, SLOT_GET_START_TIME);
            let (now, start, end) = (get_now(player), get_start(player), get_end(player));
            let to = landing(now, arg, relative, start, end);

            let set_world_time: SetWorldTimeFn = slot(player, SLOT_SET_WORLD_TIME);
            let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
            let hltv = engine::engfuncs().is_some_and(|e| (e.is_spectate_only)() != 0);
            if rewinds(now, to, skip_between, hltv) {
                // Cleared first: a player with no `HLTV` message before the
                // landing has no zoom there.
                crate::hltv_fov::clear_logged("a backward seek");
                let from = ((player as *const u8).add(FIELD_LAST_FRAME_TIME) as *const f64)
                    .read_unaligned();
                set_world_time(player, start, 0);
                *pending = Some(Pending {
                    target: to,
                    then,
                    landed: false,
                    last_set: start,
                    rewind_from: Some(from),
                    waited: 0,
                    steps: 0,
                    dropped: 0,
                    started: std::time::Instant::now(),
                });
                return Ok(format!(
                    "{now:.2} -> {to:.2} s, an HLTV demo going back: from the start ({start:.2}) in steps of {STEP_SECONDS} s, so each player's zoom is right (demo runs {start:.2} to {end:.2})"
                ));
            }
            if stepped(now, to, skip_between) {
                // Each frame from here moves the clock one step (`step`).
                *pending = Some(Pending {
                    target: to,
                    then,
                    landed: false,
                    last_set: now,
                    rewind_from: None,
                    waited: 0,
                    steps: 0,
                    dropped: 0,
                    started: std::time::Instant::now(),
                });
                return Ok(format!(
                    "{now:.2} -> {to:.2} s in steps of {STEP_SECONDS} s (demo runs {start:.2} to {end:.2})"
                ));
            }
            set_world_time(player, to, 0);
            // A follow-up still waits a frame, for the landing frame to reach
            // the client; a seek that was stepping is replaced by this one.
            *pending = then.map(|then| Pending {
                target: to,
                then: Some(then),
                landed: true,
                last_set: to,
                rewind_from: None,
                waited: 0,
                steps: 0,
                dropped: 0,
                started: std::time::Instant::now(),
            });
            drop(pending);

            let mut skipped = "";
            if skip_between {
                let world = *((player as *const u8).add(FIELD_WORLD) as *const *mut c_void);
                if !world.is_null() {
                    let frame_by_time: FrameByTimeFn = slot(world, WORLD_SLOT_GET_FRAME_BY_TIME);
                    let frame = frame_by_time(world, to);
                    if !frame.is_null() {
                        let landing_seq = (frame.add(FRAME_SEQ_NR) as *const u32).read_unaligned();
                        let last_seq = (player as *mut u8).add(FIELD_LAST_FRAME_SEQ_NR) as *mut u32;
                        if let Some(mark) = skip_mark(last_seq.read_unaligned(), landing_seq) {
                            last_seq.write_unaligned(mark);
                            ((player as *mut u8).add(FIELD_LAST_FRAME_TIME) as *mut f64)
                                .write_unaligned(to);
                            skipped = ", skipping what lies between";
                        }
                    }
                }
            }
            Ok(format!(
                "{now:.2} -> {to:.2} s (demo runs {start:.2} to {end:.2}){skipped}"
            ))
        }
    }
}

#[cfg(not(target_arch = "x86"))]
mod player {
    pub(super) fn seek(
        _arg: f64,
        _relative: bool,
        _skip_between: bool,
        _then: Option<String>,
    ) -> Result<String, String> {
        Err("only a 32-bit x86 build can drive DemoPlayer.dll".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_parse_as_numbers() {
        assert_eq!(parse_time("610"), Ok(610.0));
        assert_eq!(parse_time("1234.5"), Ok(1234.5));
        assert_eq!(parse_time(" -30 "), Ok(-30.0));
        assert!(parse_time("inf").is_err());
        assert!(parse_time("NaN").is_err());
        assert!(parse_time("").is_err());
    }

    #[test]
    fn minutes_and_hours_parse_as_the_demo_bar_shows_them() {
        assert_eq!(parse_time("10:10"), Ok(610.0));
        assert_eq!(parse_time("0:30"), Ok(30.0));
        assert_eq!(parse_time("-0:30"), Ok(-30.0));
        assert_eq!(parse_time("75:00"), Ok(4500.0));
        assert_eq!(parse_time("10:05.5"), Ok(605.5));
        assert_eq!(parse_time("1:02:03"), Ok(3723.0));
        assert_eq!(parse_time("0:00:05"), Ok(5.0));
    }

    #[test]
    fn malformed_times_are_refused_with_the_forms_expected() {
        for bad in [
            "10:", ":10", "10:60", "1:60:00", "1:2:3:4", "a:10", "10:b", "1e1:00", "10:1e1",
            "10:-5", "--1:00", "+1:00", "10::10", "10:05.", "1.5:00",
        ] {
            let err = parse_time(bad).expect_err(bad);
            assert!(err.contains("m:ss (10:10)"), "{bad}: {err}");
        }
    }

    #[test]
    fn the_console_s_split_at_colons_is_joined_back() {
        let words = |w: &[&str]| w.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let unquoted = words(&["10", ":", "10"]);
        assert_eq!(split_time(&unquoted), Some(("10:10".to_string(), &[][..])));
        let hours = words(&["1", ":", "02", ":", "03", "ds_spec_target", "13"]);
        let (time, rest) = split_time(&hours).unwrap();
        assert_eq!(time, "1:02:03");
        assert_eq!(follow_up(rest), Some("ds_spec_target 13".to_string()));
        let quoted = words(&["10:10"]);
        assert_eq!(split_time(&quoted).unwrap().0, "10:10");
        let plain = words(&["610", "ds_spec_target", "13"]);
        assert_eq!(split_time(&plain).unwrap().0, "610");
        // A trailing colon has nothing to join: it is left for the parser.
        let dangling = words(&["10", ":"]);
        assert_eq!(split_time(&dangling).unwrap().0, "10");
        assert_eq!(split_time(&[]), None);
    }

    #[test]
    fn times_print_as_the_demo_bar_shows_them() {
        assert_eq!(clock_text(605.0), "10:05");
        assert_eq!(clock_text(-30.0), "-0:30");
        assert_eq!(clock_text(605.5), "10:05.50");
        assert_eq!(clock_text(3723.0), "1:02:03");
        assert_eq!(clock_text(59.999), "1:00");
    }

    fn look(now: f64, loading: bool) -> Option<Look> {
        Some(Look {
            active: true,
            loading,
            paused: false,
            now,
            scale: 1.0,
            before_frame: false,
        })
    }

    #[test]
    fn a_queued_seek_waits_for_the_load_then_fires() {
        let mut w = Watch::default();
        // The player not up yet, then loading while it plays.
        assert_eq!(judge(&mut w, None, 0.01, 0.0), Verdict::Wait);
        assert_eq!(judge(&mut w, look(726.0, true), 0.01, 0.1), Verdict::Wait);
        assert_eq!(judge(&mut w, look(726.02, true), 0.02, 0.2), Verdict::Wait);
        // A long frame lets the clock run on further.
        assert_eq!(judge(&mut w, look(727.5, true), 1.0, 1.2), Verdict::Wait);
        assert_eq!(judge(&mut w, look(727.52, false), 0.02, 1.3), Verdict::Fire);
    }

    #[test]
    fn a_queued_seek_is_cancelled_by_what_the_user_does() {
        // Settled: the first frames' jumps are let through.
        let loading = |w: &mut Watch| {
            assert_eq!(judge(w, look(368.5, true), 0.01, 0.0), Verdict::Wait);
            assert_eq!(judge(w, look(375.4, true), 0.01, 0.0), Verdict::Wait);
            for _ in 0..SETTLE_FRAMES {
                assert_eq!(judge(w, look(100.0, true), 0.01, 0.0), Verdict::Wait);
            }
        };
        let mut w = Watch::default();
        loading(&mut w);
        let paused = look(100.01, true).map(|l| Look { paused: true, ..l });
        assert_eq!(
            judge(&mut w, paused, 0.01, 0.1),
            Verdict::Cancel("you paused")
        );

        let mut w = Watch::default();
        loading(&mut w);
        let faster = look(100.01, true).map(|l| Look { scale: 2.0, ..l });
        assert_eq!(
            judge(&mut w, faster, 0.01, 0.1),
            Verdict::Cancel("you changed the speed")
        );

        let mut w = Watch::default();
        loading(&mut w);
        assert_eq!(
            judge(&mut w, look(40.0, true), 0.01, 0.1),
            Verdict::Cancel("you moved the demo's clock")
        );

        let mut w = Watch::default();
        loading(&mut w);
        assert_eq!(
            judge(&mut w, look(160.0, true), 0.01, 0.1),
            Verdict::Cancel("you moved the demo's clock")
        );

        let mut w = Watch::default();
        loading(&mut w);
        assert_eq!(
            judge(&mut w, None, 0.01, 0.1),
            Verdict::Cancel("you closed the demo")
        );
    }

    /// The player's own gap jump and `skip_gap` land just before a frame;
    /// neither is the user.
    #[test]
    fn a_gap_jump_does_not_cancel_a_queued_seek() {
        let mut w = Watch::default();
        for _ in 0..=SETTLE_FRAMES {
            assert_eq!(judge(&mut w, look(8.5, true), 0.01, 0.0), Verdict::Wait);
        }
        let jumped = look(71.378, true).map(|l| Look {
            before_frame: true,
            ..l
        });
        assert_eq!(judge(&mut w, jumped, 0.01, 0.1), Verdict::Wait);
    }

    #[test]
    fn a_demo_that_never_starts_or_loads_unseen_is_not_waited_on_forever() {
        let mut w = Watch::default();
        assert_eq!(judge(&mut w, None, 0.01, 10.0), Verdict::Wait);
        assert_eq!(
            judge(&mut w, None, 0.01, GIVE_UP_SECONDS + 1.0),
            Verdict::Cancel("the demo never started")
        );
        // Loaded between two looks: never seen loading, then fires.
        let mut w = Watch::default();
        for _ in 0..UNSEEN_LOAD_FRAMES {
            assert_eq!(judge(&mut w, look(50.0, false), 0.01, 0.1), Verdict::Wait);
        }
        assert_eq!(judge(&mut w, look(50.0, false), 0.01, 3.0), Verdict::Fire);
    }

    #[test]
    fn a_seek_lands_inside_what_is_loaded() {
        assert_eq!(landing(100.0, 250.0, false, 50.0, 900.0), 250.0);
        assert_eq!(landing(100.0, -30.0, true, 50.0, 900.0), 70.0);
        assert_eq!(landing(100.0, 5000.0, false, 50.0, 900.0), 900.0);
        assert_eq!(landing(100.0, -500.0, true, 50.0, 900.0), 50.0);
        // An empty world reports 0..0 or worse; leave the engine to clamp.
        assert_eq!(landing(0.0, 42.0, false, 0.0, -1.0), 42.0);
    }

    /// The mark sits one frame before the landing, so `ReadDemoMessage`'s
    /// "run every frame after the last one sent" runs the landing frame alone.
    #[test]
    fn a_clean_landing_runs_the_landing_frame_and_nothing_before_it() {
        assert_eq!(skip_mark(100, 5000), Some(4999));
        assert_eq!(skip_mark(5000, 100), Some(99));
        assert_eq!(skip_mark(0, 1), Some(0));
        assert_eq!(skip_mark(5000, 5000), None);
    }

    /// A long forward seek moves the clock one step per frame, then lands on
    /// the target exactly, however the clock ran in between.
    #[test]
    fn a_long_forward_seek_steps_then_lands() {
        assert_eq!(next_step(90.0, 1335.3), Step::To(95.0));
        assert_eq!(next_step(1331.0, 1335.3), Step::Land(1335.3));
        assert_eq!(next_step(1335.3, 1335.3), Step::Land(1335.3));
        // The clock ran past the target while stepping: land anyway.
        assert_eq!(next_step(1336.0, 1335.3), Step::Land(1335.3));
        let (mut now, mut frames) = (90.1, 0);
        while let Step::To(t) = next_step(now, 1335.3) {
            now = t + 0.01; // the clock also runs a little each frame
            frames += 1;
        }
        assert_eq!(frames, 248);
    }

    /// The player records the clock it sent at, which has run on a little
    /// past what the step set.
    #[test]
    fn a_step_counts_as_sent_once_the_player_sent_at_or_after_it() {
        assert!(sent(95.004, 95.0));
        assert!(sent(95.0, 95.0));
        assert!(!sent(90.1, 95.0));
    }

    #[test]
    fn only_long_forward_seeks_without_the_skip_go_in_steps() {
        assert!(stepped(90.0, 1335.3, false));
        assert!(!stepped(90.0, 94.0, false), "a short hop fits in one frame");
        assert!(
            !stepped(1335.3, 90.0, false),
            "backward runs nothing in between"
        );
        assert!(
            !stepped(90.0, 1335.3, true),
            "the skip jumps at once on purpose"
        );
    }

    /// #830: stealth scoped at 940 s of `wsod25_grp2_h1_hltv`, seeked back to
    /// from 1000 s.
    #[test]
    fn only_backward_seeks_in_hltv_demos_go_back_to_the_start() {
        assert!(rewinds(1000.0, 940.0, false, true));
        assert!(
            !rewinds(1000.0, 940.0, false, false),
            "a POV demo records its zoom"
        );
        assert!(
            !rewinds(1000.0, 940.0, true, true),
            "the skip lands at once on purpose"
        );
        assert!(
            !rewinds(900.0, 940.0, false, true),
            "forward replays the messages"
        );
        assert!(!rewinds(940.0, 940.0, false, true));
    }

    /// The player records the clock it sent at: once the start frame is out,
    /// it is far below where playback was.
    #[test]
    fn a_rewind_counts_as_sent_once_the_clock_it_sent_at_falls() {
        assert!(rewound(0.02, 1000.0));
        assert!(
            !rewound(1000.0, 1000.0),
            "still the frame before the rewind"
        );
        assert!(
            !rewound(1000.3, 1000.0),
            "a frame sent before the rewind took"
        );
    }

    /// A review moving on to another demo mid-rewind: the new demo gets none
    /// of the old seek's steps or follow-up.
    #[test]
    fn loading_another_demo_drops_a_seek_still_on_its_way() {
        *PENDING.lock().unwrap() = Some(Pending {
            target: 939.0,
            then: Some("spec_mode 4".into()),
            landed: false,
            last_set: 300.0,
            rewind_from: None,
            waited: 0,
            steps: 60,
            dropped: 0,
            started: std::time::Instant::now(),
        });
        assert_eq!(cancel_pending(), Some(939.0));
        assert!(!seeking());
        assert_eq!(cancel_pending(), None);
    }

    /// The float of #893 on `ktps8w1-m00cat_soul_lenn_h2` (Anniversary): the
    /// clock at 8.52 s, the first playback frame (71.388 s) already sent.
    #[test]
    fn a_clock_stuck_behind_a_sent_frame_jumps_to_just_before_it() {
        let to = gap_landing(8.522, 71.388, 3, 3).expect("a gap");
        assert!((to - 71.378).abs() < 1e-3, "{to}");
        // Not sent yet: the player's own check still has its chance.
        assert_eq!(gap_landing(8.522, 71.388, 3, 2), None);
        // Normal playback: the next frame is a few milliseconds on.
        assert_eq!(gap_landing(226.663, 226.664, 14075, 14075), None);
        // A short gap the player plays through.
        assert_eq!(gap_landing(100.0, 101.5, 7, 7), None);
    }

    #[test]
    fn the_words_after_the_time_are_one_command() {
        let words = |w: &[&str]| w.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(follow_up(&words(&[])), None);
        assert_eq!(
            follow_up(&words(&["ds_spec_target", "13"])),
            Some("ds_spec_target 13".to_string())
        );
    }

    /// No name may be the whole start of another: the console's autocomplete
    /// swaps the shorter one for the longer when space is pressed.
    #[test]
    fn no_name_is_the_start_of_another() {
        let names = [SEEK_TO_NAME, SEEK_BY_NAME, SKIP_BETWEEN_NAME];
        for a in names {
            for b in names {
                assert!(a == b || !b.starts_with(a), "{a} is the start of {b}");
            }
        }
    }

    #[test]
    fn the_builds_are_told_apart() {
        assert_ne!(BUILDS[0].time_date_stamp, BUILDS[1].time_date_stamp);
    }
}
