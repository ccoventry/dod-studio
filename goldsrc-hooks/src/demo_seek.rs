//! `dodstudio_seek_to <seconds>` / `dodstudio_seek_by <seconds>`: jump
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
//! between the old and new position do not fire" holds only going back.
//!
//! By default these commands do exactly what the engine does. Set
//! `dodstudio_seek_skip_between 1` to make a seek land clean instead: it moves
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
//! scoreboard read 0/0, and `dodstudio_spec_target` found no such player.
//!
//! So a forward seek of more than [`STEP_SECONDS`] moves the clock that far per
//! frame instead, from [`poll`], until it lands. Each step's catch-up is a few
//! seconds of the demo, which always fits, so everything in between reaches the
//! client in order, as if played very fast. A whole HLTV demo takes a few
//! hundred frames. `dodstudio_seek_skip_between 1` seeks still jump at once:
//! they skip the in-between on purpose.
//!
//! A command after the time (`dodstudio_seek_to <seconds> <command ...>`) runs
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
//! most 33 packets per engine frame -- while playback has already started. A
//! seek past what is loaded lands on the last loaded frame. `IsLoading()`
//! (slot 28, "is the loader still connected") is the gate; the console prints
//! `Demo file completely loaded.` when it clears.
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

pub const SEEK_TO_NAME: &str = console_name!("seek_to");
pub const SEEK_BY_NAME: &str = console_name!("seek_by");
pub const SKIP_BETWEEN_NAME: &str = console_name!("seek_skip_between");

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
    /// Frames spent waiting for the player to send the last step.
    waited: u32,
    steps: u32,
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

/// `dodstudio_seek_skip_between`: 1 lands a seek without running the director
/// events and console commands it jumps over. Off by default, so a seek does
/// what the engine's own Goto does until asked otherwise.
///
/// Read from the cvar when a seek runs, so it needs no per-frame poll. The flag
/// is the fallback path's, set by a plain toggle command when cvars could not
/// be registered.
pub static SKIP_BETWEEN: AtomicBool = AtomicBool::new(false);
static SKIP_BETWEEN_CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());

/// Called by `commands.rs` once `dodstudio_seek_skip_between` is registered.
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

/// Parses a seek argument: seconds, fractional or negative.
fn parse_seconds(raw: &str) -> Result<f64, String> {
    let value: f64 = raw
        .trim()
        .parse()
        .map_err(|_| format!("expected a number of seconds, got \"{raw}\""))?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(format!("expected a number of seconds, got \"{raw}\""))
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
    let usage = || format!("usage: {name} <seconds> [command to run once there]");
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
    let then = follow_up(&args[1..]);
    Ok((args.swap_remove(0), then))
}

/// The words after the time, as one command line, or `None` when there are none.
fn follow_up(words: &[String]) -> Option<String> {
    (!words.is_empty()).then(|| words.join(" "))
}

fn run(name: &str, relative: bool) {
    let result = arguments(name).and_then(|(raw, then)| {
        parse_seconds(&raw)
            .and_then(|seconds| player::seek(seconds, relative, skip_between(), then))
    });
    let line = match result {
        Ok(done) => format!("{name}: {done}"),
        Err(why) => format!("{name}: {why}"),
    };
    console_print(&format!("{line}\n"));
    unsafe { crate::debug::report(&format!("demo_seek: {line}")) };
}

/// While a `viewdemo` demo is still being read (#465): how many seconds of
/// it the player holds so far. `None` once loaded, or with no demo player.
pub fn buffered_while_loading() -> Option<f64> {
    #[cfg(target_arch = "x86")]
    return player::buffered_while_loading();
    #[cfg(not(target_arch = "x86"))]
    None
}

/// Steps a long forward seek on by one frame, and runs a landed seek's
/// follow-up command. Called every frame; one uncontended lock when idle.
pub fn poll() {
    let Ok(mut pending) = PENDING.try_lock() else {
        return;
    };
    let Some(seek) = pending.as_mut() else {
        return;
    };
    #[cfg(target_arch = "x86")]
    let done = player::step(seek);
    #[cfg(not(target_arch = "x86"))]
    let done: Result<bool, String> = Err("only a 32-bit x86 build can drive DemoPlayer.dll".into());
    let line = match done {
        Ok(false) => return,
        Ok(true) => {
            if let Some(then) = &seek.then
                && let Ok(line) = std::ffi::CString::new(format!("{then}\n"))
            {
                engine::client_cmd(&line);
            }
            format!(
                "{SEEK_TO_NAME}: at {:.2} s after {} step(s), {} ms",
                seek.target,
                seek.steps,
                seek.started.elapsed().as_millis()
            )
        }
        Err(why) => format!("{SEEK_TO_NAME}: stopped stepping: {why}"),
    };
    *pending = None;
    unsafe { crate::debug::report(&format!("demo_seek: {line}")) };
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
    #[cfg(target_arch = "x86")]
    return player::seek(seconds, false, skip_between(), None);
    #[cfg(not(target_arch = "x86"))]
    {
        let _ = seconds;
        Err("only the 32-bit build has a demo player".to_string())
    }
}

/// `dodstudio_seek_to <seconds>`: an absolute world time, the clock the events
/// list shows and the analysis calls `viewdemo_offset`.
pub unsafe extern "C" fn seek_to() {
    run(SEEK_TO_NAME, false);
}

/// `dodstudio_seek_by <seconds>`: forward, or back when negative.
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
    fn find() -> Result<(*mut c_void, &'static Build), String> {
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
            let last_frame_time =
                ((player as *const u8).add(FIELD_LAST_FRAME_TIME) as *const f64).read_unaligned();
            if !sent(last_frame_time, seek.last_set) && seek.waited < MAX_WAIT_FRAMES {
                seek.waited += 1;
                return Ok(false);
            }
            seek.waited = 0;
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
            if stepped(now, to, skip_between) {
                // Each frame from here moves the clock one step (`step`).
                *pending = Some(Pending {
                    target: to,
                    then,
                    landed: false,
                    last_set: now,
                    waited: 0,
                    steps: 0,
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
                waited: 0,
                steps: 0,
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
    fn seconds_parse_as_numbers_and_nothing_else() {
        assert_eq!(parse_seconds("1234.5"), Ok(1234.5));
        assert_eq!(parse_seconds(" -30 "), Ok(-30.0));
        assert!(parse_seconds("1:30").is_err());
        assert!(parse_seconds("inf").is_err());
        assert!(parse_seconds("NaN").is_err());
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

    #[test]
    fn the_words_after_the_time_are_one_command() {
        let words = |w: &[&str]| w.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(follow_up(&words(&[])), None);
        assert_eq!(
            follow_up(&words(&["dodstudio_spec_target", "13"])),
            Some("dodstudio_spec_target 13".to_string())
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
