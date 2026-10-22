//! Empties the engine's filtered command buffer, so a long forward seek in a
//! POV demo doesn't leave the recorder's old key presses replaying after it
//! lands (#702).
//!
//! ## What the engine does
//!
//! `hw.dll` keeps two 16 KB command buffers (`Cbuf_Init`): `cmd_text`, for
//! the console and the client, and `filteredcmd_text`, for text the game
//! didn't type itself -- a server's `stufftext` and a demo's recorded console
//! commands. `Cbuf_Execute` runs all of `cmd_text` every frame but **one**
//! command of `filteredcmd_text` (its executor's line limit is 1 on both
//! builds; pre-Anniversary `hw.dll+0x27570`, 25th Anniversary `+0x1b4cf0`).
//!
//! In playback that is never noticed: a POV demo's type-3 frames (`+forward`,
//! `-moveleft`, `+speed`, ...) come a few per second. A stepped seek
//! (`demo_seek.rs`) replays every frame it jumps over, so a 30-minute jump
//! hands the buffer some 15,000 commands in a few seconds: it overflows
//! ("Cbuf_AddTextToBuffer: overflow", about 6,600 times for one jump) and
//! then drains one command a frame for the next 40 seconds of the demo. Each
//! of those is a key press from minutes earlier, so `CL_ButtonBits` changes
//! almost every frame; DoD's gun lowering reads it (a sprint key plus a move
//! key) and the crosshair blinks. Measured on wsod25_grp2_h1_m00cat (PRE),
//! the 31:01 clip after a 1,826 s jump: 18 blinks shorter than 0.1 s, against
//! none when the same clip was reached by fast-forwarding from the start.
//!
//! So a stepped seek drops what it hands the buffer ([`clear`]) and, once it
//! lands, lets go of every key the dropped commands may have left down
//! ([`RELEASE_KEYS`]). What the demo presses after the landing plays as
//! recorded; a key held across the landing reads as up until its next press,
//! as with `ds_demo_seek_skip_between`.
//!
//! Found by a byte pattern of `Cbuf_Execute` per build, each unique in its own
//! `hw.dll`; `tools/verify_filtered_commands_offsets.py` re-derives both.

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::engine;
use crate::scan;

/// One build's `Cbuf_Execute`: `cmd_text` with no line limit, then
/// `filteredcmd_text` with a limit of one.
struct Site {
    build: &'static str,
    pattern: &'static str,
    /// Offset of the `filteredcmd_text` address (the last `push imm32`).
    buffer_at: usize,
}

#[rustfmt::skip]
const SITES: &[Site] = &[
    Site {
        build: "pre-Anniversary",
        //         push 1 push cmd_text   call            push 1 push 0 push filteredcmd_text call            add esp,14 ret
        pattern: "6A 01 68 ?? ?? ?? ?? E8 ?? ?? ?? ?? 6A 01 6A 00 68 ?? ?? ?? ?? E8 ?? ?? ?? ?? 83 C4 14 C3",
        buffer_at: 17,
    },
    Site {
        build: "25th Anniversary",
        //         push -1 push 1 push cmd_text   call            push 1 push 0 push filteredcmd_text call            add esp,18 ret
        pattern: "6A FF 6A 01 68 ?? ?? ?? ?? E8 ?? ?? ?? ?? 6A 01 6A 00 68 ?? ?? ?? ?? E8 ?? ?? ?? ?? 83 C4 18 C3",
        buffer_at: 19,
    },
];

/// `sizebuf_t::cursize`, the same in both builds.
const CURSIZE: usize = 0x10;

/// Every button `CL_ButtonBits` reads. Typed with no key number, each `-`
/// clears the button whatever pressed it.
pub const RELEASE_KEYS: &str = "-attack;-attack2;-duck;-jump;-forward;-back;-use;-left;-right;-moveleft;-moveright;-reload;-alt1;-showscores;-speed";

/// The buffer's `sizebuf_t`, found once per session (hw.dll never reloads);
/// 1 when it can't be found.
static BUFFER: AtomicUsize = AtomicUsize::new(0);

fn buffer() -> Option<usize> {
    match BUFFER.load(Ordering::Acquire) {
        0 => {}
        1 => return None,
        found => return Some(found),
    }
    let base = engine::engine_module_base()?;
    let mut errors = Vec::new();
    for site in SITES {
        // Safety: `base` is a module handle the loader gave us.
        match unsafe { scan::find_unique(base, site.pattern) } {
            Ok(at) => {
                // Safety: inside the code the scan just matched.
                let address =
                    unsafe { std::ptr::read_unaligned((at + site.buffer_at) as *const u32) }
                        as usize;
                BUFFER.store(address, Ordering::Release);
                return Some(address);
            }
            Err(why) => errors.push(format!("{}: {why}", site.build)),
        }
    }
    BUFFER.store(1, Ordering::Release);
    unsafe {
        crate::debug::report(&format!(
            "filtered_commands: Cbuf_Execute wasn't found, so a long seek can't drop the commands it replays ({})",
            errors.join("; ")
        ))
    };
    None
}

/// Drops every command waiting in the filtered buffer; returns how many bytes
/// it held, or `None` when the buffer couldn't be found. Main thread only.
pub fn clear() -> Option<usize> {
    let cursize = (buffer()? + CURSIZE) as *mut i32;
    // Safety: `sizebuf_t` in hw.dll's data, which lives for the session; the
    // engine reads it only on this thread.
    unsafe {
        let held = cursize.read_unaligned();
        cursize.write_unaligned(0);
        Some(held.max(0) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each pattern's buffer offset is the operand of its last `push imm32`.
    #[test]
    fn each_buffer_offset_follows_the_last_push() {
        for site in SITES {
            let bytes: Vec<&str> = site.pattern.split(' ').collect();
            assert_eq!(bytes[site.buffer_at - 1], "68", "{}", site.build);
            assert!(
                bytes[site.buffer_at..site.buffer_at + 4]
                    .iter()
                    .all(|b| *b == "??")
            );
            assert_eq!(bytes[site.buffer_at + 4], "E8", "{}", site.build);
        }
    }

    #[test]
    fn every_release_is_a_minus_command() {
        assert!(
            RELEASE_KEYS
                .split(';')
                .all(|c| c.starts_with('-') && !c.contains(' '))
        );
    }
}
