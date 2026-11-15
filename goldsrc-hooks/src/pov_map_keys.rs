//! A POV demo's recorded `+map` and `+mapzoom` presses do nothing during
//! playback, so they can't change the viewer's `_cl_minimap` (#1441).
//!
//! ## What went wrong
//!
//! DoD's `+map` (`IN_MapDown`, client.dll `+0x3c110`) presses the `in_map`
//! button and calls `CHudDoDMap::HandleMapButton` (`+0x2f520`), which cycles
//! `_cl_minimap`: 0 → 1 (full map) → 2 (minimap, with `cl_dmsmallmap 1`) → 0.
//! It is the only code in client.dll that writes the cvar (`SetMinimapState`,
//! `+0x22910`). A POV demo records the recorder's presses as `ConsoleCommand`
//! frames and playback runs them again in the viewer's game, so the viewer's
//! `_cl_minimap` ends wherever the recorder's presses leave it, and stays
//! there for every later demo of the session: it is an ordinary cvar.
//!
//! In POV playback the map itself never draws (the engine cancels its second
//! render view, #897), so nothing shows the change except its side effects.
//! At 1, the full map: `ShouldDrawCrossHair` returns false, and
//! `CObjectiveIcons::Draw` puts each flag at its spot on the (undrawn) map, so
//! the flag row is gone but for one icon in the corner. At 2 the kill feed
//! moves down under the minimap's place.
//!
//! A capture batch made it worse. A stepped seek drops the commands each step
//! replays but the engine still runs one a frame between polls
//! (`filtered_commands`), so of three presses the seek crosses, any number
//! may run. In the WSOD25 batch of 2026-11-14 (POST), seeking across
//! `wsod25_grp1_h2_dyelife`'s three `+map` presses (1:51:25-1:59:54) left
//! the full map on, and every clip from there to the end of the batch, eight
//! of them, had no crosshair and no flag row.
//!
//! ## The fix
//!
//! `+map` and `+mapzoom` are wrapped through the engine's command list
//! ([`crate::cmd_list`]); while a POV demo plays the wrapper drops them,
//! whoever pressed them, since the map they would open can't draw there.
//! `-map` and `-mapzoom` only let go of a button and pass as before. Live
//! play and HLTV demos, where the map draws, are unchanged. A `_cl_minimap`
//! the viewer set themselves stays as they set it. On by default;
//! `GOLDSRC_HOOKS_POV_MAP_KEYS=0` turns it off.

// Only the 32-bit build installs anything; a host check still compiles it.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::cmd_list::{self, call_real, wrap};

/// Whether to wrap at all: `GOLDSRC_HOOKS_POV_MAP_KEYS=0` turns it off.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

/// client.dll's own handlers, called by the wrappers outside POV playback.
static REAL_MAP: AtomicUsize = AtomicUsize::new(0);
static REAL_MAPZOOM: AtomicUsize = AtomicUsize::new(0);
static WRAPPED: AtomicBool = AtomicBool::new(false);

/// How many more frames `poll` looks for the commands: client.dll registers
/// them in its input setup, which can come after `commands::install`.
static RETRIES_LEFT: AtomicU32 = AtomicU32::new(600);

/// Presses dropped this session; only the first few are logged.
static DROPPED: AtomicU32 = AtomicU32::new(0);
const LOGGED_DROPS: u32 = 5;

/// Wraps the commands once they exist; a no-op once done or given up. Runs
/// every frame on the engine's thread.
pub fn poll() {
    if RETRIES_LEFT.load(Ordering::Relaxed) == 0 {
        return;
    }
    if !ENABLED.load(Ordering::Relaxed) {
        RETRIES_LEFT.store(0, Ordering::Relaxed);
        unsafe {
            crate::debug::report(
                "pov_map_keys: off (GOLDSRC_HOOKS_POV_MAP_KEYS=0) -- a POV demo's +map presses change _cl_minimap (#1441)",
            )
        };
        return;
    }
    if try_wrap() {
        RETRIES_LEFT.store(0, Ordering::Relaxed);
        unsafe {
            crate::debug::report(
                "pov_map_keys: wrapped +map and +mapzoom -- they do nothing while a POV demo plays, so a recorder's presses can't hide the crosshair and flags (#1441)",
            )
        };
    } else if RETRIES_LEFT.fetch_sub(1, Ordering::Relaxed) == 1 {
        unsafe {
            crate::debug::report(
                "pov_map_keys: no +map command in the engine's command list -- a POV demo's +map presses still change _cl_minimap",
            )
        };
    }
}

fn try_wrap() -> bool {
    if WRAPPED.load(Ordering::Relaxed) {
        return true;
    }
    let mut found_map = false;
    let mut found_zoom = false;
    let listed = cmd_list::for_each(|name, entry| {
        if name.eq_ignore_ascii_case(b"+map") {
            found_map |= wrap(entry, &REAL_MAP, wrapped_map);
        } else if name.eq_ignore_ascii_case(b"+mapzoom") {
            found_zoom |= wrap(entry, &REAL_MAPZOOM, wrapped_mapzoom);
        }
    });
    let done = listed && found_map && found_zoom;
    if done {
        WRAPPED.store(true, Ordering::Relaxed);
    }
    done
}

/// Whether a press goes through: anywhere but a POV demo's playback.
fn lets_through(pov_demo_playing: bool) -> bool {
    !pov_demo_playing
}

fn press(real: &AtomicUsize, name: &str) {
    if lets_through(crate::anim_fix::pov_demo_playing()) {
        unsafe { call_real(real) };
        return;
    }
    let n = DROPPED.fetch_add(1, Ordering::Relaxed) + 1;
    if n <= LOGGED_DROPS {
        unsafe {
            crate::debug::report(&format!(
                "pov_map_keys: dropped {name} during POV playback ({n} so far{}); the map can't draw there and it would hide the crosshair and flags",
                if n == LOGGED_DROPS {
                    ", no more logged"
                } else {
                    ""
                }
            ))
        };
    }
}

unsafe extern "C" fn wrapped_map() {
    press(&REAL_MAP, "+map");
}

unsafe extern "C" fn wrapped_mapzoom() {
    press(&REAL_MAPZOOM, "+mapzoom");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presses_go_through_outside_pov_playback() {
        assert!(lets_through(false));
    }

    #[test]
    fn presses_are_dropped_while_a_pov_demo_plays() {
        assert!(!lets_through(true));
    }
}
