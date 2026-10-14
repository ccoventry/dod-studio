//! Lets a demo recorded in `dod` play in a copy of the game folder, such as
//! `-game dod_movies` (issues #402, #711).
//!
//! ## The check
//!
//! Every demo's `svc_serverinfo` carries the recording server's game folder,
//! `dod` for every DoD demo. `CL_ParseServerInfo` compares it, ignoring case,
//! with the base name of the running game's folder (`com_gamedir`), and on a
//! mismatch prints
//!
//! ```text
//! Server is running game dod.  Restart in that game to connect.
//! ```
//!
//! disconnects and stops parsing, so in a copy of `dod` no demo plays at all.
//! The copy holds the same mod, so nothing the check guards against (another
//! mod's `client.dll` and assets meeting this server's messages) applies.
//!
//! ```text
//!     test esi, esi / je .none         ; no game folder sent
//!     cmp  byte [esi], 0 / je .none
//!     COM_FileBase(com_gamedir, buf)
//!     Q_stricmp(buf, esi)
//!     test eax, eax
//!     je   .same                       ; <- becomes jmp
//!     Con_Printf("Server is running game %s. ...", esi)
//!     disconnect; return false
//! .none:
//!     Con_Printf("Server didn't specify a gamedir, assuming no change\n")
//! .same:                               ; 13 bytes on: push, call, add esp
//!     ...the parse goes on
//! ```
//!
//! The pre-Anniversary `hw.dll` keeps this in its own function,
//! `CL_CheckGameDirectory` (`+0x1c2a0`, patched byte `+0x1c2d9`), which
//! `CL_ParseServerInfo` (call at `+0x1c755`) and the `gamedir` console
//! command call; that command disconnects before it checks, so skipping the
//! check there only drops its message. The 25th Anniversary `hw.dll` inlines
//! it into `CL_ParseServerInfo` (patched byte `+0x1a7b46`), and its
//! standalone copy (`+0x1a6900`, called only by `gamedir`) is left alone.
//!
//! ## The fix
//!
//! One byte: the `je` taken when the names match becomes `jmp` (`74` ->
//! `EB`), so a mismatch continues exactly as a match does. The game folder
//! name is still read off the message, so the rest of it parses as before;
//! nothing else in either build uses it. Before writing, the jump must land
//! exactly where the "didn't specify" print rejoins, and the message right
//! after it must be the mismatch message, so a match on other code can't be
//! patched. `tools/verify_game_dir_check_offsets.py` re-derives both builds
//! offline.
//!
//! Installed only when the game runs a folder other than `dod`
//! ([`crate::game_folder::name`]), since in `dod` the check already passes
//! for every DoD demo. It then skips the check for every demo, whatever folder
//! it names: a demo of another mod fails to play as it would in `dod`.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::engine;
use crate::scan;

/// The mismatch message, NUL included, as `Con_Printf` is handed it.
const MESSAGE: &[u8] = b"Server is running game %s.  Restart in that game to connect.\n\0";

/// `push imm32; call rel32; add esp, 4`: the "didn't specify" print, between
/// the no-folder path's target and the match path's.
const NO_GAMEDIR_PRINT_LEN: usize = 13;

const JE_REL8: u8 = 0x74;
const JMP_REL8: u8 = 0xeb;
const PUSH_IMM32: u8 = 0x68;

/// One build's view of the check.
struct Site {
    build: &'static str,
    pattern: &'static str,
    /// Offset of the `je rel8` taken when the string is empty.
    empty_je_at: usize,
    /// Offset of the `je rel8` taken when the names match: the patched byte.
    match_je_at: usize,
    /// Offset of the mismatch message's address in its `push imm32`.
    message_at: usize,
}

#[rustfmt::skip]
const SITES: &[Site] = &[
    Site {
        build: "pre-Anniversary",
        //         push ebp;mov ebp,esp;sub esp,0x104 push esi;mov esi,[ebp+8] test esi,esi;je  cmp [esi],0;je  lea eax,[ebp-0x104]  push  push com_gamedir  call COM_FileBase  lea ecx,[ebp-0x104]  push esi;push ecx  call Q_stricmp  add esp,0x10 test eax,eax je  push esi push message
        pattern: "55 8B EC 81 EC 04 01 00 00 56 8B 75 08 85 F6 74 ?? 80 3E 00 74 ?? 8D 85 FC FE FF FF 50 68 ?? ?? ?? ?? E8 ?? ?? ?? ?? 8D 8D FC FE FF FF 56 51 E8 ?? ?? ?? ?? 83 C4 10 85 C0 74 ?? 56 68 ?? ?? ?? ??",
        empty_je_at: 20,
        match_je_at: 57,
        message_at: 61,
    },
    // Inside CL_ParseServerInfo, from the MSG_ReadString that reads the
    // folder. The standalone copy loads esi from [ebp+8] instead.
    Site {
        build: "25th Anniversary",
        //         call MSG_ReadString  mov esi,eax test esi,esi;je  cmp [esi],0;je  lea eax,[ebp-x]  push  push com_gamedir  call COM_FileBase  lea eax,[ebp-x]  push esi;push eax  call Q_stricmp  add esp,0x10 test eax,eax je  push esi push message
        pattern: "E8 ?? ?? ?? ?? 8B F0 85 F6 74 ?? 80 3E 00 74 ?? 8D 85 ?? ?? ?? ?? 50 68 ?? ?? ?? ?? E8 ?? ?? ?? ?? 8D 85 ?? ?? ?? ?? 56 50 E8 ?? ?? ?? ?? 83 C4 10 85 C0 74 ?? 56 68 ?? ?? ?? ??",
        empty_je_at: 14,
        match_je_at: 51,
        message_at: 55,
    },
];

/// The patched `je`, as an RVA, or 0 while not installed.
static PATCHED_RVA: AtomicUsize = AtomicUsize::new(0);

/// The build it was installed on, for the status line.
static BUILD_NAME: Mutex<&str> = Mutex::new("");

/// Whether a game running `folder` needs the check skipped: any folder but
/// `dod`, which every DoD demo names.
fn needed(folder: &str) -> bool {
    !folder.eq_ignore_ascii_case(crate::game_folder::DEFAULT)
}

/// Skips the demo game-folder check when the game runs a copy of `dod`. A
/// second call does nothing.
pub fn install() {
    let folder = crate::game_folder::name();
    if !needed(folder) || PATCHED_RVA.load(Ordering::Acquire) != 0 {
        return;
    }
    match install_inner() {
        Ok((build, rva)) => {
            if let Ok(mut name) = BUILD_NAME.lock() {
                *name = build;
            }
            PATCHED_RVA.store(rva, Ordering::Release);
            unsafe {
                crate::debug::report(&format!(
                    "game_dir_check: {build} hw.dll -- demos recorded in dod play in {folder} (game folder check at +{rva:#x} skipped, #402)"
                ))
            };
        }
        Err(why) => unsafe {
            crate::debug::report(&format!(
                "game_dir_check: not installed -- demos recorded in dod will not play in {folder}: {why}"
            ))
        },
    }
}

fn install_inner() -> Result<(&'static str, usize), String> {
    let base = engine::engine_module_base().ok_or("hw.dll is not loaded yet")?;
    let size =
        unsafe { crate::pe::image_size(base as *mut u8) }.ok_or("hw.dll has no PE header")?;

    let mut errors = Vec::new();
    for site in SITES {
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
        check_jumps(span, site).map_err(|why| {
            format!(
                "the {} pattern matched at +{:#x}, but {why}",
                site.build,
                found - base
            )
        })?;
        let message = read_u32(span, site.message_at)
            .ok_or("the message operand is outside the match")? as usize;
        let inside = message >= base
            && message
                .checked_add(MESSAGE.len())
                .is_some_and(|end| end <= base + size);
        // Safety: inside the mapped image, checked first.
        if !inside
            || unsafe { std::slice::from_raw_parts(message as *const u8, MESSAGE.len()) } != MESSAGE
        {
            return Err(format!(
                "the {} pattern matched at +{:#x}, but its message {message:#x} is not the mismatch message",
                site.build,
                found - base
            ));
        }
        let je = found + site.match_je_at;
        // Safety: one byte of the `je` the checks above identified.
        if !unsafe { crate::patch::write_code_bytes(je, &[JMP_REL8]) } {
            return Err(format!("could not make +{:#x} writable", je - base));
        }
        return Ok((site.build, je - base));
    }
    Err(format!("no known build matched ({})", errors.join("; ")))
}

fn read_u32(span: &[u8], at: usize) -> Option<u32> {
    let bytes = span.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

/// The target of the `je rel8` at `at`, as an offset into `span`.
fn je_target(span: &[u8], at: usize) -> Result<isize, String> {
    match span.get(at..at + 2) {
        Some(&[JE_REL8, rel]) => Ok(at as isize + 2 + rel as i8 as isize),
        Some(other) => Err(format!("{other:02x?} at offset {at} is not a je rel8")),
        None => Err(format!("offset {at} is past the match")),
    }
}

/// Checks the matched bytes: both jumps are `je`, the match jump lands
/// exactly past the no-folder path's print, and the mismatch message is
/// pushed right after it.
fn check_jumps(span: &[u8], site: &Site) -> Result<(), String> {
    let empty = je_target(span, site.empty_je_at)?;
    let same = je_target(span, site.match_je_at)?;
    if same != empty + NO_GAMEDIR_PRINT_LEN as isize {
        return Err(format!(
            "its match jump lands at {same}, not {NO_GAMEDIR_PRINT_LEN} bytes past the no-folder path ({empty})"
        ));
    }
    if span.get(site.message_at.wrapping_sub(1)) != Some(&PUSH_IMM32) {
        return Err("the message is not pushed as an imm32".to_string());
    }
    Ok(())
}

/// One `dodstudio_debug_status` line, once installed.
pub fn status_line() -> Option<String> {
    let rva = PATCHED_RVA.load(Ordering::Relaxed);
    if rva == 0 {
        return None;
    }
    let build = BUILD_NAME.lock().map(|name| *name).unwrap_or_default();
    Some(format!(
        "demo game folder check: skipped, demos recorded in dod play in {} ({build} hw.dll, +{rva:#x}, #402)",
        crate::game_folder::name()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(pattern: &str) -> Vec<&str> {
        pattern.split_whitespace().collect()
    }

    /// The pattern with each wildcard replaced by `fill`.
    fn bytes(site: &Site, fill: impl Fn(usize) -> u8) -> Vec<u8> {
        tokens(site.pattern)
            .iter()
            .enumerate()
            .map(|(i, t)| match *t {
                "??" => fill(i),
                hex => u8::from_str_radix(hex, 16).unwrap(),
            })
            .collect()
    }

    /// Each build's real bytes, copied from its `hw.dll` (the wildcards'
    /// values only; everything else is the pattern).
    fn real(site: &Site) -> Vec<u8> {
        let (empty_rel, match_rel) = match site.build {
            "pre-Anniversary" => (0x3f, 0x27),
            _ => (0x49, 0x31),
        };
        bytes(site, |i| {
            if i == site.empty_je_at + 1 {
                empty_rel
            } else if i == site.match_je_at + 1 {
                match_rel
            } else {
                0x00
            }
        })
    }

    #[test]
    fn each_site_points_at_its_jumps_and_message() {
        for site in SITES {
            let t = tokens(site.pattern);
            assert_eq!(t[site.empty_je_at..site.empty_je_at + 2], ["74", "??"]);
            assert_eq!(
                t[site.empty_je_at - 3..site.empty_je_at],
                ["80", "3E", "00"]
            );
            assert_eq!(t[site.match_je_at..site.match_je_at + 2], ["74", "??"]);
            assert_eq!(t[site.match_je_at - 2..site.match_je_at], ["85", "C0"]);
            assert_eq!(t[site.message_at - 1], "68", "{}", site.build);
            assert_eq!(
                t.len(),
                site.message_at + 4,
                "{}: ends on the message",
                site.build
            );
            assert!(
                t[site.message_at..].iter().all(|b| *b == "??"),
                "{}: the message operand is wildcarded",
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
    fn a_jump_anywhere_else_is_refused() {
        for site in SITES {
            let mut span = real(site);
            span[site.match_je_at + 1] += 1;
            assert!(check_jumps(&span, site).is_err(), "{}", site.build);
            let mut span = real(site);
            span[site.match_je_at] = JMP_REL8;
            assert!(
                check_jumps(&span, site).is_err(),
                "{}: already patched",
                site.build
            );
            assert!(check_jumps(&span[..site.match_je_at + 1], site).is_err());
        }
    }

    #[test]
    fn the_patterns_differ_so_one_build_cannot_match_the_other() {
        assert_ne!(SITES[0].pattern, SITES[1].pattern);
    }

    #[test]
    fn only_a_folder_other_than_dod_needs_it() {
        assert!(!needed("dod"));
        assert!(!needed("DOD"));
        assert!(needed("dod_movies"));
        assert!(needed("valve"));
    }
}
