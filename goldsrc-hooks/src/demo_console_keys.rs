//! Keys don't open the console while a POV demo plays (#894): only the
//! console key does, as in an HLTV demo.
//!
//! ## What the engine does
//!
//! `Key_Event` still carries Quake's "during demo playback, most keys bring
//! up the menu", with the menu swapped for the console:
//!
//! ```text
//!     cmp  [cls.demoplayback], 0 / je .normal
//!     cmp  [cls.spectator], 0    / jne .normal   ; <- 75 xx, made EB xx
//!     test down                  / je .normal
//!     cmp  [consolekeys + key*4], 0 / je .normal
//!     cmp  key_dest, key_game    / jne .normal
//!     call Con_ToggleConsole_f                   ; the `toggleconsole` handler
//!     ret
//! .normal:
//!     ...the key's binding runs
//! ```
//!
//! `consolekeys` (`Key_Init`) holds every printable key, Enter, Tab,
//! Backspace, the arrows, the keypad, Shift, Ctrl and Alt; the console key
//! itself (`` ` `` and `~`) is left out. So in a POV demo, pressing any of
//! them opens the console. The one people hit is Alt: Alt+Tab away from the
//! game or back to it, and game_probe's own focus trick (a synthetic Alt
//! press), opened the console over the HUD being measured. In an HLTV demo
//! the proxy's `svc_hltv` sets `cls.spectator` (`CL_Parse_HLTV`'s
//! `HLTV_ACTIVE`), so the keys already run their binds instead.
//!
//! The call is direct, not through the command list, so the `toggleconsole`
//! wrap for `ds_ui_console_in_panel` (`studio_panel.rs`) never sees it: Alt
//! opened the plain console window even with that setting on.
//!
//! Pre-Anniversary `hw.dll`: `Key_Event`'s check at `+0x61911`, the patched
//! byte `+0x6191f`, `Con_ToggleConsole_f` `+0x2c690`. 25th Anniversary:
//! `+0x1d7765`, byte `+0x1d7775`, `+0x1bc360`.
//!
//! ## The fix
//!
//! One byte: the `jne` taken when `cls.spectator` is set becomes `jmp`, so a
//! POV demo takes the path an HLTV demo takes and each key runs its own
//! binding (Tab shows the scores, Alt runs `lastinv`, which a demo drops).
//! The console key still opens the console, and with
//! `ds_ui_console_in_panel 1` the DoD Studio window, since it goes through
//! its binding. Nothing changes outside demo playback, where the first test
//! already skips the block.
//!
//! Before writing, all five jumps must land on one place, and the call must
//! reach the handler the engine registered as `toggleconsole`, so a match on
//! other code can't be patched. `tools/verify_demo_console_keys_offsets.py`
//! re-derives both builds offline. On by default;
//! `GOLDSRC_HOOKS_DEMO_CONSOLE_KEYS=0` turns it off.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::engine;
use crate::scan;

/// Whether to install at all: `GOLDSRC_HOOKS_DEMO_CONSOLE_KEYS=0` turns it off.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

const JE_REL8: u8 = 0x74;
const JNE_REL8: u8 = 0x75;
const JMP_REL8: u8 = 0xeb;
const CALL_REL32: u8 = 0xe8;

/// One build's view of the check.
struct Site {
    build: &'static str,
    pattern: &'static str,
    /// Offsets of the five short jumps to `.normal`, in order.
    jumps_at: [usize; 5],
    /// Which of [`Site::jumps_at`] is the `cls.spectator` test: the patched
    /// byte.
    spectator: usize,
    /// Offset of the `call Con_ToggleConsole_f`.
    call_at: usize,
}

#[rustfmt::skip]
const SITES: &[Site] = &[
    Site {
        build: "pre-Anniversary",
        //         cmp [demoplayback],edi je  cmp [spectator],edi jne  cmp esi,edi je  cmp [consolekeys+eax*4],edi je  cmp ecx,edi jne  call Con_ToggleConsole_f
        pattern: "39 3D ?? ?? ?? ?? 74 ?? 39 3D ?? ?? ?? ?? 75 ?? 3B F7 74 ?? 39 3C 85 ?? ?? ?? ?? 74 ?? 3B CF 75 ?? E8 ?? ?? ?? ??",
        jumps_at: [6, 14, 18, 27, 31],
        spectator: 1,
        call_at: 33,
    },
    Site {
        build: "25th Anniversary",
        //         cmp [demoplayback],0  je  cmp [spectator],0     jne  test edx,edx je  cmp [consolekeys+eax*4],0 je  test ecx,ecx jne  call Con_ToggleConsole_f
        pattern: "83 3D ?? ?? ?? ?? 00 74 ?? 83 3D ?? ?? ?? ?? 00 75 ?? 85 D2 74 ?? 83 3C 85 ?? ?? ?? ?? 00 74 ?? 85 C9 75 ?? E8 ?? ?? ?? ??",
        jumps_at: [7, 16, 20, 30, 34],
        spectator: 1,
        call_at: 36,
    },
];

/// The patched `jne`, as an RVA, or 0 while not installed.
static PATCHED_RVA: AtomicUsize = AtomicUsize::new(0);
/// Index into [`SITES`] of the build it was installed on.
static BUILD: AtomicUsize = AtomicUsize::new(usize::MAX);

/// Makes keys other than the console key run their binds in a POV demo. A
/// second call does nothing. Runs on the engine's thread, from
/// `install_fixes`, before any demo plays.
pub fn install() {
    if !ENABLED.load(Ordering::Relaxed) {
        unsafe {
            crate::debug::report(
                "demo_console_keys: off (GOLDSRC_HOOKS_DEMO_CONSOLE_KEYS=0) -- Alt and most other keys open the console in a POV demo (#894)",
            )
        };
        return;
    }
    if PATCHED_RVA.load(Ordering::Acquire) != 0 {
        return;
    }
    let report = match install_inner() {
        Ok((index, rva)) => {
            BUILD.store(index, Ordering::Release);
            PATCHED_RVA.store(rva, Ordering::Release);
            format!(
                "demo_console_keys: {} hw.dll -- only the console key opens the console in a POV demo (Key_Event's demo check at +{rva:#x} skipped, #894)",
                SITES[index].build
            )
        }
        Err(why) => format!(
            "demo_console_keys: not installed -- Alt and most other keys still open the console in a POV demo: {why}"
        ),
    };
    unsafe { crate::debug::report(&report) };
}

fn install_inner() -> Result<(usize, usize), String> {
    let base = engine::engine_module_base().ok_or("hw.dll is not loaded yet")?;
    let toggle = crate::studio_panel::engine_toggleconsole()
        .ok_or("the engine's toggleconsole command was not found")?;

    let mut errors = Vec::new();
    for (index, site) in SITES.iter().enumerate() {
        // Safety: `base` is a module handle the loader gave us.
        let found = match unsafe { scan::find_unique(base, site.pattern) } {
            Ok(found) => found,
            Err(why) => {
                errors.push(format!("{}: {why}", site.build));
                continue;
            }
        };
        let len = site.pattern.split_whitespace().count();
        // Safety: the match is `len` mapped bytes of the code section.
        let span = unsafe { std::slice::from_raw_parts(found as *const u8, len) };
        let at = |why: String| {
            format!(
                "the {} pattern matched at +{:#x}, but {why}",
                site.build,
                found - base
            )
        };
        check_jumps(span, site).map_err(at)?;
        let called = call_target(span, site.call_at, found).map_err(at)?;
        if called != toggle {
            return Err(at(format!(
                "it calls +{:#x}, not toggleconsole's handler +{:#x}",
                called.wrapping_sub(base),
                toggle.wrapping_sub(base)
            )));
        }
        let jne = found + site.jumps_at[site.spectator];
        // Safety: one byte of the `jne` the checks above identified; this
        // runs on the engine's thread, the only one that patches the game.
        if !unsafe { crate::patch::write_code_bytes(jne, &[JMP_REL8]) } {
            return Err(format!("could not make +{:#x} writable", jne - base));
        }
        return Ok((index, jne - base));
    }
    Err(format!("no known build matched ({})", errors.join("; ")))
}

/// The target of the short jump at `at`, as an offset into `span`, if its
/// opcode is `opcode`.
fn jump_target(span: &[u8], at: usize, opcode: u8) -> Result<isize, String> {
    match span.get(at..at + 2) {
        Some(&[op, rel]) if op == opcode => Ok(at as isize + 2 + rel as i8 as isize),
        Some(other) => Err(format!(
            "{other:02x?} at offset {at} is not the expected {opcode:02x} rel8"
        )),
        None => Err(format!("offset {at} is past the match")),
    }
}

/// The opcode each jump must have: every one is a `je` but the `jne`s on
/// `cls.spectator` and on `key_dest`.
fn expected_opcode(site: &Site, which: usize) -> u8 {
    if which == site.spectator || which == site.jumps_at.len() - 1 {
        JNE_REL8
    } else {
        JE_REL8
    }
}

/// Checks the matched bytes: the five jumps have their opcodes, all land on
/// one place, and that place is past the call.
fn check_jumps(span: &[u8], site: &Site) -> Result<(), String> {
    let mut landing = None;
    for (which, &at) in site.jumps_at.iter().enumerate() {
        let target = jump_target(span, at, expected_opcode(site, which))?;
        match landing {
            None => landing = Some(target),
            Some(first) if first != target => {
                return Err(format!(
                    "the jump at offset {at} lands at {target}, the first at {first}"
                ));
            }
            Some(_) => {}
        }
    }
    let past_call = site.call_at as isize + 5;
    match landing {
        Some(target) if target >= past_call => Ok(()),
        Some(target) => Err(format!(
            "the jumps land at {target}, before the end of the call ({past_call})"
        )),
        None => Err("no jumps".to_string()),
    }
}

/// The absolute target of the `call rel32` at `at`, for a span mapped at
/// `address`.
fn call_target(span: &[u8], at: usize, address: usize) -> Result<usize, String> {
    match span.get(at..at + 5) {
        Some(&[CALL_REL32, a, b, c, d]) => {
            let rel = i32::from_le_bytes([a, b, c, d]);
            Ok((address + at + 5).wrapping_add_signed(rel as isize))
        }
        Some(other) => Err(format!("{other:02x?} at offset {at} is not a call rel32")),
        None => Err(format!("offset {at} is past the match")),
    }
}

/// One `ds_debug_status` line, once installed.
pub fn status_line() -> Option<String> {
    let rva = PATCHED_RVA.load(Ordering::Relaxed);
    if rva == 0 {
        return None;
    }
    let build = SITES
        .get(BUILD.load(Ordering::Relaxed))
        .map_or("", |site| site.build);
    Some(format!(
        "POV demo keys: only the console key opens the console ({build} hw.dll, +{rva:#x}, #894)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(pattern: &str) -> Vec<&str> {
        pattern.split_whitespace().collect()
    }

    /// Each build's real bytes, copied from its `hw.dll`: the jumps' and the
    /// call's operands; the other wildcards don't matter to the checks.
    fn real(site: &Site) -> Vec<u8> {
        let (rels, call): (&[u8], [u8; 4]) = match site.build {
            "pre-Anniversary" => (&[0x24, 0x1c, 0x18, 0x0f, 0x0b], [0x59, 0xad, 0xfc, 0xff]),
            _ => (&[0x2f, 0x26, 0x22, 0x18, 0x14], [0xd2, 0x4b, 0xfe, 0xff]),
        };
        tokens(site.pattern)
            .iter()
            .enumerate()
            .map(|(i, t)| {
                if let Some(which) = site.jumps_at.iter().position(|&at| at + 1 == i) {
                    rels[which]
                } else if (site.call_at + 1..site.call_at + 5).contains(&i) {
                    call[i - site.call_at - 1]
                } else if *t == "??" {
                    0
                } else {
                    u8::from_str_radix(t, 16).unwrap()
                }
            })
            .collect()
    }

    #[test]
    fn each_site_points_at_its_jumps_and_call() {
        for site in SITES {
            let t = tokens(site.pattern);
            for (which, &at) in site.jumps_at.iter().enumerate() {
                let op = format!("{:02X}", expected_opcode(site, which));
                assert_eq!(t[at..at + 2], [op.as_str(), "??"], "{}", site.build);
            }
            assert_eq!(t[site.call_at], "E8", "{}", site.build);
            assert_eq!(
                t.len(),
                site.call_at + 5,
                "{}: ends on the call",
                site.build
            );
            assert!(scan::Pattern::parse(site.pattern).is_ok(), "{}", site.build);
        }
    }

    #[test]
    fn the_real_jumps_pass() {
        for site in SITES {
            assert_eq!(check_jumps(&real(site), site), Ok(()), "{}", site.build);
        }
    }

    #[test]
    fn the_real_calls_reach_con_toggleconsole_f() {
        // Image bases and RVAs from each build's hw.dll.
        let cases = [
            (
                "pre-Anniversary",
                0x01d0_0000 + 0x6191f - 14,
                0x01d0_0000 + 0x2c690,
            ),
            (
                "25th Anniversary",
                0x1000_0000 + 0x1d7775 - 16,
                0x1000_0000 + 0x1bc360,
            ),
        ];
        for (site, (build, address, toggle)) in SITES.iter().zip(cases) {
            assert_eq!(site.build, build);
            assert_eq!(
                call_target(&real(site), site.call_at, address),
                Ok(toggle),
                "{build}"
            );
        }
    }

    #[test]
    fn a_jump_anywhere_else_is_refused() {
        for site in SITES {
            for &at in &site.jumps_at {
                let mut span = real(site);
                span[at + 1] += 1;
                assert!(check_jumps(&span, site).is_err(), "{} at {at}", site.build);
            }
            let mut span = real(site);
            span[site.jumps_at[site.spectator]] = JMP_REL8;
            assert!(
                check_jumps(&span, site).is_err(),
                "{}: already patched",
                site.build
            );
            let short = real(site);
            assert!(check_jumps(&short[..site.jumps_at[4] + 1], site).is_err());
        }
    }

    #[test]
    fn the_patterns_differ_so_one_build_cannot_match_the_other() {
        assert_ne!(SITES[0].pattern, SITES[1].pattern);
    }
}
