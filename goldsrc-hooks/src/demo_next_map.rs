//! `ds_debug_demo_play_next_map`: `viewdemo` plays on into a demo's next map
//! instead of stopping at the level change (R&D, #1372).
//!
//! ## Why `viewdemo` stops
//!
//! `viewdemo` reads the file with `Core.dll` (`DemoFile` -> `Server` ->
//! `World`) as fast as the frame allows, while `DemoPlayer.dll` plays the
//! world back by its clock (`docs/goldsrc_viewdemo.md`). The loader runs far
//! ahead of playback and the world holds one map, so the loader ends at a
//! level change, in two places (`docs/goldsrc_viewdemo_map_change.md`):
//!
//! ```text
//! Server::ParseStuffText, on the server's "reconnect":
//!     if (IsDemoFile()) Disconnect();            // closes the file
//! Server::ParseServerinfo:
//!     if (IsDemoFile() && m_ServerState == SERVER_RUNNING) { Disconnect(); return; }
//! ```
//!
//! Playback then runs to the world's last frame and the clock stays there.
//! `playdemo` has no world: `hw.dll` streams the file, runs the `reconnect`
//! and loads the next map from the signon that follows it.
//!
//! ## What this does
//!
//! Everything past the stop already handles a new map on the same objects, as
//! an HLTV proxy needs when its server changes level: `ParseServerinfo` calls
//! `World::NewGame`, the new map's first frame fires the signal
//! `DemoPlayer::ReceiveSignal` answers with `DemoPlayer::NewGame`, and the
//! player then sends the engine a fresh `svc_serverinfo` and spawn, as it did
//! for the first map. Only the timing is missing: the loader must not read
//! the next map until playback has reached the end of this one.
//!
//! 1. **The `reconnect` calls `Server::Reconnect` instead of
//!    `Server::Disconnect`**: one byte, the vftable offset in the call
//!    (`0x48` -> `0x44`, [`BUILDS`]). `Reconnect` is what a live proxy runs
//!    there: back to the connected state, so the signon that follows passes
//!    `ParseServerinfo`'s check.
//! 2. **`Server::Reconnect` (vftable slot 17) is wrapped** so the hook hears
//!    it, and holds the loader: `DemoFile::ReadDemoPacket` returns at once
//!    while `m_nextReadTime > now && m_Continuous`, its own throttle for a
//!    demo played in real time, so setting those two fields stops the reading
//!    with no code patch. The channel is a fake one and never times out.
//! 3. **`IDemoPlayer::IsLoading` (slot 28) says no while the loader is
//!    held**, since this map is all there: the demo bar and
//!    `ds_demo_seek_to` treat it as loaded.
//! 4. **[`poll`] lets go** once playback has sent the world's last frame
//!    ([`crate::demo_seek::played_to_end`]) and isn't paused. The loader
//!    reads the next signon, the world starts over, and the player reconnects
//!    the engine to the next map.
//!
//! `0` puts the call back; a demo already past its `reconnect` keeps the
//! state it has. The value `2` also runs the engine's own `reconnect` when it
//! lets go, as `playdemo` does at that point (loading plaque, `cls.signon`
//! reset), to compare.
//!
//! ## What it doesn't do
//!
//! The world drops the map it leaves, so the clock, the demo bar and
//! `ds_demo_seek_to` cover one map at a time, and going back to an earlier
//! map means loading the demo again. A level change with no `reconnect`
//! before its `svc_serverinfo` still stops at `ParseServerinfo`'s check.
//!
//! `tools/verify_demo_next_map_offsets.py` checks every address, slot and
//! field here against both builds' `Core.dll` and `DemoPlayer.dll`.

// The patch and the vftable wraps are only built for the 32-bit DLL; a host
// build compiles the pure parts for the tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code, unused_imports))]

use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

use crate::names::console_name;
use crate::setting::Setting;

pub const NAME: &str = console_name!("debug_demo_play_next_map");

/// Off by default: R&D, and it changes what a multi-map demo does at the end
/// of its first map.
pub static SETTING: Setting = Setting::new(NAME, false);

/// One `Core.dll` build's `reconnect` branch in `Server::ParseStuffText`.
struct Build {
    name: &'static str,
    /// From the `IsDemoFile()` test through the `Disconnect()` call and the
    /// store that follows it: unique in that build's `Core.dll`.
    pattern: &'static str,
    /// Where the call's vftable offset byte is within `pattern`.
    site: usize,
    /// `Server::m_DemoFile`: what `Server::IsDemoFile` (slot 24) adds to
    /// `this` before jumping to `DemoFile::IsPlaying`.
    demo_file: usize,
}

/// The builds the patch knows, tried in order.
const BUILDS: [Build; 2] = [
    Build {
        name: "pre-Anniversary",
        // test al, al; je; mov eax, [esi]; mov ecx, esi; call [eax+0x48];
        // lea ecx, [esp+0x3c]; mov byte [esi+m_IsPaused], 0
        pattern: "84 C0 74 21 8B 06 8B CE FF 50 48 8D 4C 24 3C C6 86 ?? ?? ?? ?? 00",
        site: 10,
        demo_file: 0x16de0,
    },
    Build {
        name: "25th Anniversary",
        // test al, al; je; mov eax, [esi]; mov ecx, esi; call [eax+0x48];
        // mov byte [esi+m_IsPaused], 0; jmp
        pattern: "84 C0 74 13 8B 06 8B CE FF 50 48 C6 86 ?? ?? ?? ?? 00 E9",
        site: 10,
        demo_file: 0x569e0,
    },
];

/// `call [eax+0x48]`: `Server::Disconnect`, vftable slot 18.
const CALL_DISCONNECT: u8 = 0x48;
/// `call [eax+0x44]`: `Server::Reconnect`, slot 17.
const CALL_RECONNECT: u8 = 0x44;

/// `IServer` vftable slots, the same in both builds.
const SERVER_SLOT_RECONNECT: usize = 17;
const SERVER_SLOT_IS_DEMO_FILE: usize = 24;
/// `IDemoPlayer::IsLoading`.
const PLAYER_SLOT_IS_LOADING: usize = 28;
/// `DemoPlayer::m_Server`, the same in both builds.
const PLAYER_FIELD_SERVER: usize = 0x150;

/// `DemoFile` fields, the same in both builds.
const DEMO_FILE_HANDLE: usize = 0x6c8;
const DEMO_FILE_NEXT_READ_TIME: usize = 0xb5c;
const DEMO_FILE_CONTINUOUS: usize = 0xb74;

/// What a cvar value asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// The engine's own stop.
    Off,
    /// Play on into the next map.
    On,
    /// Play on, and run the engine's `reconnect` when the loader is let go.
    OnWithReconnect,
}

fn mode(value: Option<f32>, fallback_on: bool) -> Mode {
    match value {
        Some(v) if v.is_finite() && v >= 2.0 => Mode::OnWithReconnect,
        Some(v) if v.is_finite() && v >= 1.0 => Mode::On,
        Some(_) => Mode::Off,
        None if fallback_on => Mode::On,
        None => Mode::Off,
    }
}

fn wanted() -> Mode {
    mode(SETTING.value(), SETTING.on())
}

/// The vftable offset the `reconnect` branch should call for `mode`.
fn call_byte(mode: Mode) -> u8 {
    match mode {
        Mode::Off => CALL_DISCONNECT,
        Mode::On | Mode::OnWithReconnect => CALL_RECONNECT,
    }
}

/// `m_DemoFile`'s offset as `Server::IsDemoFile`'s code gives it:
/// `add ecx, imm32; jmp DemoFile::IsPlaying`.
fn demo_file_from_thunk(code: &[u8]) -> Option<usize> {
    match code {
        [0x81, 0xc1, a, b, c, d, 0xe9, ..] => Some(u32::from_le_bytes([*a, *b, *c, *d]) as usize),
        _ => None,
    }
}

/// The `Core.dll` the call was found in, the address of its vftable offset
/// byte, and that build's `m_DemoFile` offset; 0 while not found.
static CORE_BASE: AtomicUsize = AtomicUsize::new(0);
static SITE: AtomicUsize = AtomicUsize::new(0);
static DEMO_FILE: AtomicUsize = AtomicUsize::new(0);
/// The `Core.dll` the last failed look was on, so a failure is logged once.
static FAILED_BASE: AtomicUsize = AtomicUsize::new(0);
static BUILD_NAME: std::sync::Mutex<&str> = std::sync::Mutex::new("");

/// The handlers beneath the wraps, and the vftables they are in.
static REAL_RECONNECT: AtomicUsize = AtomicUsize::new(0);
static REAL_IS_LOADING: AtomicUsize = AtomicUsize::new(0);
static WRAPPED_SERVER_VFTABLE: AtomicUsize = AtomicUsize::new(0);
static WRAPPED_PLAYER_VFTABLE: AtomicUsize = AtomicUsize::new(0);

/// The `Server` whose loader is held at a map change; 0 while none is.
/// Written by the `Reconnect` wrap (inside `Server::RunFrame`) and [`poll`],
/// both on the engine's thread; read by the `IsLoading` wrap.
static HELD_SERVER: AtomicUsize = AtomicUsize::new(0);
/// Map changes this session: held, and let go.
static HELD_COUNT: AtomicU32 = AtomicU32::new(0);
static RELEASED_COUNT: AtomicU32 = AtomicU32::new(0);
/// The player's end time (`f64` bits) when the loader was last let go, until
/// the next map's clock shows; 0 while not waiting.
static WAITING_FROM_END: AtomicU64 = AtomicU64::new(0);

/// `ds_debug_status`'s line, once `Core.dll` has been seen.
pub fn status_line() -> Option<String> {
    if CORE_BASE.load(Ordering::Relaxed) == 0 {
        return None;
    }
    let build = BUILD_NAME.lock().map(|name| *name).unwrap_or_default();
    let state = match wanted() {
        Mode::Off => "off (viewdemo stops at a map change)",
        Mode::On => "on",
        Mode::OnWithReconnect => "on, with the engine's reconnect",
    };
    Some(format!(
        "demo next map: {state} ({build} Core.dll); {} map changes reached, {} played into{}",
        HELD_COUNT.load(Ordering::Relaxed),
        RELEASED_COUNT.load(Ordering::Relaxed),
        if HELD_SERVER.load(Ordering::Relaxed) != 0 {
            "; the loader is waiting at one now"
        } else {
            ""
        }
    ))
}

/// Each frame: finds the call once `Core.dll` is loaded, keeps it and the two
/// wraps as the setting asks, and lets a held loader go when playback has
/// reached the end of its map.
pub fn poll() {
    #[cfg(target_arch = "x86")]
    {
        installed::keep();
        installed::let_go_at_the_end();
        installed::note_next_map();
    }
}

#[cfg(target_arch = "x86")]
mod installed {
    use std::ffi::c_void;

    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleA;

    use super::*;

    type ServerFn = unsafe extern "thiscall" fn(*mut c_void);
    // An MSVC `bool` return sets only `al`; read all of `eax` and keep the
    // low byte, as `demo_seek` does.
    type ByteFn = unsafe extern "thiscall" fn(*mut c_void) -> u32;

    fn core_dll() -> usize {
        // Safety: a plain lookup of a loaded module by name.
        unsafe { GetModuleHandleA(c"Core.dll".as_ptr() as *const u8) as usize }
    }

    /// The demo player's `Server`, null before the player has one.
    fn server_of_player() -> Option<*mut c_void> {
        let player = crate::demo_seek::player_object()?;
        // Safety: `demo_seek` only hands out a player of a build it knows,
        // and the field is checked against both by the verify script.
        let server =
            unsafe { *((player as *const u8).add(PLAYER_FIELD_SERVER) as *const *mut c_void) };
        (!server.is_null()).then_some(server)
    }

    unsafe fn vftable_slot(object: *mut c_void, index: usize) -> usize {
        unsafe { (*(object as *const usize)) + index * size_of::<usize>() }
    }

    /// Finds the `reconnect` branch's call in the `Core.dll` at `base`.
    fn find(base: usize) -> Result<(&'static Build, usize), String> {
        let mut misses = Vec::new();
        for build in &BUILDS {
            // Safety: `base` is a module handle the loader gave us.
            match unsafe { crate::scan::find_unique(base, build.pattern) } {
                Ok(found) => return Ok((build, found + build.site)),
                Err(why) => misses.push(format!("{} signature: {why}", build.name)),
            }
        }
        Err(misses.join("; "))
    }

    /// Wraps `Server::Reconnect` and `IDemoPlayer::IsLoading`, once each per
    /// vftable. `Ok(false)` while the player has no server yet.
    fn wrap(build: &Build) -> Result<bool, String> {
        let (Some(player), Some(server)) = (crate::demo_seek::player_object(), server_of_player())
        else {
            return Ok(false);
        };
        // Safety: both objects are live, and the slots are checked against
        // both builds by the verify script.
        unsafe {
            let is_demo_file = *(vftable_slot(server, SERVER_SLOT_IS_DEMO_FILE) as *const usize);
            let thunk = std::slice::from_raw_parts(is_demo_file as *const u8, 7);
            if demo_file_from_thunk(thunk) != Some(build.demo_file) {
                return Err(format!(
                    "Server::IsDemoFile is {thunk:02x?}, not the {} build's m_DemoFile at +{:#x}",
                    build.name, build.demo_file
                ));
            }
            let server_vftable = *(server as *const usize);
            if WRAPPED_SERVER_VFTABLE.load(Ordering::Relaxed) != server_vftable {
                let slot = vftable_slot(server, SERVER_SLOT_RECONNECT);
                let ours = reconnect as ServerFn as usize;
                if *(slot as *const usize) != ours {
                    crate::patch::swap_vftable_slot(slot, ours, &REAL_RECONNECT)?;
                }
                WRAPPED_SERVER_VFTABLE.store(server_vftable, Ordering::Relaxed);
            }
            let player_vftable = *(player as *const usize);
            if WRAPPED_PLAYER_VFTABLE.load(Ordering::Relaxed) != player_vftable {
                let slot = vftable_slot(player, PLAYER_SLOT_IS_LOADING);
                let ours = is_loading as ByteFn as usize;
                if *(slot as *const usize) != ours {
                    crate::patch::swap_vftable_slot(slot, ours, &REAL_IS_LOADING)?;
                }
                WRAPPED_PLAYER_VFTABLE.store(player_vftable, Ordering::Relaxed);
            }
        }
        Ok(true)
    }

    /// Finds the call in a newly loaded `Core.dll`, and keeps its byte as the
    /// setting asks.
    pub(super) fn keep() {
        let base = core_dll();
        if base == 0 {
            CORE_BASE.store(0, Ordering::Relaxed);
            HELD_SERVER.store(0, Ordering::Relaxed);
            return;
        }
        if FAILED_BASE.load(Ordering::Relaxed) == base {
            return;
        }
        if CORE_BASE.load(Ordering::Relaxed) != base {
            let found = find(base).and_then(|(build, site)| Ok((build, site, wrap(build)?)));
            let (build, site) = match found {
                Ok((build, site, true)) => (build, site),
                // The player has no server yet: look again next frame.
                Ok((_, _, false)) => return,
                Err(why) => {
                    FAILED_BASE.store(base, Ordering::Relaxed);
                    // Safety: `report` only appends to the hook log.
                    unsafe {
                        crate::debug::report(&format!(
                            "demo_next_map: not installed -- {why}; viewdemo stops at a map change"
                        ))
                    };
                    return;
                }
            };
            SITE.store(site, Ordering::Relaxed);
            DEMO_FILE.store(build.demo_file, Ordering::Relaxed);
            CORE_BASE.store(base, Ordering::Relaxed);
            if let Ok(mut name) = BUILD_NAME.lock() {
                *name = build.name;
            }
            // Safety: as above.
            unsafe {
                crate::debug::report(&format!(
                    "demo_next_map: found ({} Core.dll, the reconnect branch's call at +{:#x}; Server::Reconnect and IsLoading wrapped) (#1372)",
                    build.name,
                    site - 2 - base
                ))
            };
        }
        let site = SITE.load(Ordering::Relaxed);
        let want = call_byte(wanted());
        // Safety: `site` is inside the signature matched in the module still
        // loaded at `base`.
        let have = unsafe { *(site as *const u8) };
        if have == want || (have != CALL_DISCONNECT && have != CALL_RECONNECT) {
            return;
        }
        // Safety: one byte of an instruction's displacement, both values a
        // slot of the same vftable taking no arguments.
        let written = unsafe { crate::patch::write_code_bytes(site, &[want]) };
        // Safety: as above.
        unsafe {
            crate::debug::report(&format!(
                "demo_next_map: {} -- a demo's reconnect {}",
                if written { "set" } else { "could not write the call" },
                if want == CALL_RECONNECT {
                    "holds the loader until playback gets there, then plays the next map"
                } else {
                    "ends the loading, as the engine has it"
                }
            ))
        };
    }

    unsafe fn demo_file(server: *mut c_void) -> *mut u8 {
        unsafe { (server as *mut u8).add(DEMO_FILE.load(Ordering::Relaxed)) }
    }

    /// `Server::Reconnect`, which only the patched `reconnect` branch of a
    /// demo reaches in the game (the engine's own callers are a live proxy's).
    /// Runs inside `Server::RunFrame`, on the engine's thread.
    unsafe extern "thiscall" fn reconnect(server: *mut c_void) {
        let real = REAL_RECONNECT.load(Ordering::Acquire);
        if real != 0 {
            // Safety: the slot's own handler, saved before the swap.
            unsafe { std::mem::transmute::<usize, ServerFn>(real)(server) };
        }
        if DEMO_FILE.load(Ordering::Relaxed) == 0 {
            return;
        }
        // Safety: `server` is the object the engine called us on, and the
        // slot and fields are checked against both builds.
        unsafe {
            let is_demo_file: ByteFn = std::mem::transmute(
                *(vftable_slot(server, SERVER_SLOT_IS_DEMO_FILE) as *const usize),
            );
            if is_demo_file(server) & 0xff == 0 {
                return;
            }
            let file = demo_file(server);
            (file.add(DEMO_FILE_NEXT_READ_TIME) as *mut f32).write_unaligned(f32::MAX);
            file.add(DEMO_FILE_CONTINUOUS).write(1);
        }
        HELD_SERVER.store(server as usize, Ordering::Release);
        let count = HELD_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        let end = crate::demo_seek::clock().map(|c| c.end).unwrap_or(0.0);
        // Safety: `report` only appends to the hook log.
        unsafe {
            crate::debug::report(&format!(
                "demo_next_map: map change {count} in the demo after {end:.2} s of this map; the loader waits there until playback reaches it"
            ))
        };
    }

    /// `IDemoPlayer::IsLoading`: no while the loader is held, since the map
    /// being played is all there.
    unsafe extern "thiscall" fn is_loading(player: *mut c_void) -> u32 {
        if HELD_SERVER.load(Ordering::Acquire) != 0 {
            return 0;
        }
        let real = REAL_IS_LOADING.load(Ordering::Acquire);
        if real == 0 {
            return 0;
        }
        // Safety: the slot's own handler, saved before the swap.
        unsafe { std::mem::transmute::<usize, ByteFn>(real)(player) }
    }

    /// Lets a held loader read on once playback has reached the end of the
    /// map it holds.
    pub(super) fn let_go_at_the_end() {
        let held = HELD_SERVER.load(Ordering::Relaxed);
        if held == 0 {
            return;
        }
        let server = held as *mut c_void;
        // Safety: `held` was the player's server when it was stored, and
        // Core.dll is still loaded (`keep` clears the hold when it isn't).
        let (handle, continuous) = unsafe {
            let file = demo_file(server);
            (
                (file.add(DEMO_FILE_HANDLE) as *const usize).read_unaligned(),
                file.add(DEMO_FILE_CONTINUOUS).read(),
            )
        };
        // A demo stopped or loaded since: the file is closed, or its reader
        // was set up afresh (`Server::LoadDemo` clears `m_Continuous`).
        if server_of_player() != Some(server) || handle == 0 || continuous == 0 {
            HELD_SERVER.store(0, Ordering::Release);
            return;
        }
        let mode = wanted();
        if mode == Mode::Off || !crate::demo_seek::played_to_end() {
            return;
        }
        let end = crate::demo_seek::clock().map(|c| c.end).unwrap_or(0.0);
        // Safety: as above. `m_Continuous` off is the reader's own state for
        // a demo under `viewdemo`.
        unsafe {
            let file = demo_file(server);
            file.add(DEMO_FILE_CONTINUOUS).write(0);
            (file.add(DEMO_FILE_NEXT_READ_TIME) as *mut f32).write_unaligned(0.0);
        }
        HELD_SERVER.store(0, Ordering::Release);
        WAITING_FROM_END.store(end.to_bits().max(1), Ordering::Relaxed);
        let count = RELEASED_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        let reconnected =
            mode == Mode::OnWithReconnect && crate::engine::client_cmd(c"reconnect");
        // Safety: `report` only appends to the hook log.
        unsafe {
            crate::debug::report(&format!(
                "demo_next_map: playback reached the end of the map at {end:.2} s; loading the demo's next map ({count} this session){}",
                if reconnected { ", after the engine's reconnect" } else { "" }
            ))
        };
    }

    /// Logs the next map's clock once the player shows it.
    pub(super) fn note_next_map() {
        let from = WAITING_FROM_END.load(Ordering::Relaxed);
        if from == 0 {
            return;
        }
        let Some(clock) = crate::demo_seek::clock() else {
            return;
        };
        if !clock.active || clock.end <= clock.start || clock.end.to_bits() == from {
            return;
        }
        if clock.now < clock.start - 1.0 || clock.now > clock.end {
            // The player's clock is still the last map's.
            return;
        }
        WAITING_FROM_END.store(0, Ordering::Relaxed);
        // Safety: `report` only appends to the hook log.
        unsafe {
            crate::debug::report(&format!(
                "demo_next_map: the next map is playing; its clock runs from {:.2} s, {:.2} s loaded so far",
                clock.start, clock.end
            ))
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cvar_value_picks_the_mode() {
        assert_eq!(mode(Some(0.0), false), Mode::Off);
        assert_eq!(mode(Some(1.0), false), Mode::On);
        assert_eq!(mode(Some(2.0), false), Mode::OnWithReconnect);
        assert_eq!(mode(Some(f32::NAN), true), Mode::Off);
        assert_eq!(mode(Some(-1.0), true), Mode::Off);
        // Unregistered: the setting's own default.
        assert_eq!(mode(None, false), Mode::Off);
        assert_eq!(mode(None, true), Mode::On);
    }

    #[test]
    fn off_calls_disconnect_and_on_calls_reconnect() {
        assert_eq!(call_byte(Mode::Off), 0x48);
        assert_eq!(call_byte(Mode::On), 0x44);
        assert_eq!(call_byte(Mode::OnWithReconnect), 0x44);
        // Slot numbers times the pointer size of the 32-bit game.
        assert_eq!(CALL_RECONNECT as usize, SERVER_SLOT_RECONNECT * 4);
        assert_eq!(CALL_DISCONNECT as usize, (SERVER_SLOT_RECONNECT + 1) * 4);
    }

    #[test]
    fn the_site_is_the_calls_vftable_offset_in_each_signature() {
        for build in &BUILDS {
            let bytes: Vec<&str> = build.pattern.split(' ').collect();
            assert_eq!(
                &bytes[build.site - 2..=build.site],
                ["FF", "50", "48"],
                "{}",
                build.name
            );
        }
    }

    #[test]
    fn is_demo_files_thunk_gives_the_demo_file_offset() {
        // add ecx, 0x16de0; jmp rel32 (pre-Anniversary Core.dll+0x141b0).
        let pre = [0x81, 0xc1, 0xe0, 0x6d, 0x01, 0x00, 0xe9, 0x05, 0x33];
        assert_eq!(demo_file_from_thunk(&pre), Some(BUILDS[0].demo_file));
        // add ecx, 0x569e0; jmp rel32 (25th Anniversary Core.dll+0xfad0).
        let post = [0x81, 0xc1, 0xe0, 0x69, 0x05, 0x00, 0xe9];
        assert_eq!(demo_file_from_thunk(&post), Some(BUILDS[1].demo_file));
        assert_eq!(demo_file_from_thunk(&[0x8b, 0x81, 0, 0, 0, 0, 0xc3]), None);
        assert_eq!(demo_file_from_thunk(&[0x81, 0xc1]), None);
    }
}
