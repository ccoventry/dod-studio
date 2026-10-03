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

/// What the console command reads: the one argument, or why there isn't one.
fn argument(name: &str) -> Result<String, String> {
    let engfuncs = engine::engfuncs().ok_or_else(|| "the engine is not ready".to_string())?;
    // Cmd_Argc counts the command name itself.
    if unsafe { (engfuncs.cmd_argc)() } != 2 {
        return Err(format!("usage: {name} <seconds>"));
    }
    let raw = unsafe { (engfuncs.cmd_argv)(1) };
    if raw.is_null() {
        return Err(format!("usage: {name} <seconds>"));
    }
    Ok(unsafe { CStr::from_ptr(raw as *const c_char) }
        .to_string_lossy()
        .into_owned())
}

fn run(name: &str, relative: bool) {
    let result = argument(name)
        .and_then(|raw| parse_seconds(&raw))
        .and_then(|seconds| player::seek(seconds, relative, skip_between()));
    let line = match result {
        Ok(done) => format!("{name}: {done}"),
        Err(why) => format!("{name}: {why}"),
    };
    console_print(&format!("{line}\n"));
    unsafe { crate::debug::report(&format!("demo_seek: {line}")) };
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

    pub(super) fn seek(arg: f64, relative: bool, skip_between: bool) -> Result<String, String> {
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
            set_world_time(player, to, 0);

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
    pub(super) fn seek(_arg: f64, _relative: bool, _skip_between: bool) -> Result<String, String> {
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
