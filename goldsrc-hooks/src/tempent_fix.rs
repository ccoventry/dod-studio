//! Stops DoD 1.3's client crashing when the engine has no temp entity to give
//! it (issue #374).
//!
//! ## The bug
//!
//! Six places in DoD's `client.dll` ask the engine for a temporary effect
//! entity -- `pEfxAPI->R_TempSprite` or `R_TempModel` -- and write into the
//! result without checking it. The engine returns NULL when it cannot hand one
//! out: the temp-entity pool is full, it was just reset by a level load, or the
//! sprite or model is not precached (`EV_FindModelIndex` returned 0). The
//! client then writes to `NULL + offset` and the game dies:
//!
//! ```text
//! CRASH: access violation (0xc0000005) at client.dll+0x225cc -- writing 0x354
//!   eax=0x00000000 ... esi=0x00000000
//! ```
//!
//! That one is `EV_BloodPuff`'s dust sprite, seen twice on 2026-09-24 right
//! after a demo loaded. It has happened for years, with or without this DLL.
//! A survey of every `pEfxAPI` call in `client.dll` (every load of
//! `gEngfuncs.pEfxAPI`, followed to the call it feeds, including the ones that
//! cache the function pointer in a register first) found these six unchecked:
//!
//! | site | function | effect |
//! | --- | --- | --- |
//! | `+0x225c0` | `EV_BloodPuff` | dust puff where a bullet hits a player (the crash above) |
//! | `+0x2269c` | `EV_BloodPuff` | blood sprite beside it |
//! | `+0xb23c`  | `EV_BloodStream` | dust |
//! | `+0xb2dd`  | `EV_BloodStream` | blood |
//! | `+0x3156b` | `Event_EjectBrassP` | shell casing from a third-person model |
//! | `+0x316a4` | `Event_EjectBrassV` | shell casing from the viewmodel |
//!
//! The other fourteen `pEfxAPI` calls that return a pointer are fine: seven
//! check it, five are `CL_AllocDlight`, which never returns NULL (it reuses
//! the oldest light), and two ignore the result.
//!
//! ## The fix: hand it somewhere harmless to write
//!
//! Each site gets a detour on the instruction right after the call. If the
//! engine returned a real entity nothing changes. If it returned NULL, the stub
//! swaps in a pointer to a scratch buffer of our own and lets the game's code
//! run on as normal, so it fills in a "temp entity" the engine never sees. The
//! effect is simply not drawn, which is what the engine meant by NULL.
//!
//! Substituting a buffer rather than jumping past the writes is deliberate.
//! These functions balance their stack with one `add esp` that covers several
//! calls' arguments at once (`EV_BloodPuff` pops the sprite call's and the next
//! `RandomLong`'s together), so a skip would have to reproduce a different
//! stack adjustment per site. With a buffer, every site's stub is the same four
//! instructions plus its own stolen bytes. It is only safe because, at all six
//! sites, the code after the call only *writes* through the result, never
//! reads a pointer back out of it and follows it -- which
//! `tools/verify_tempent_offsets.py` checks against the binary, along with
//! every offset written staying inside [`SCRATCH_SIZE`].
//!
//! It is on by default, unlike every `dodstudio_*` setting, because it is a
//! crash fix rather than a change to what a capture looks like: it only ever
//! does anything in the case that would otherwise end the game.
//! `GOLDSRC_HOOKS_TEMPENT_FIX=0` turns it off for a session.
//!
//! Analysis subject: `dod/cl_dlls/client.dll`, 977,816 bytes, byte-identical
//! across the stock, pre-Anniversary and post-Anniversary installs.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::detour;
use crate::scan;

/// One patched call site.
struct Site {
    /// Named in the log and in `dodstudio_debug_status`.
    what: &'static str,
    /// Unique across `client.dll`'s code, and ends with the stolen bytes, so a
    /// build where they differ does not match at all.
    pattern: &'static str,
    /// Distance from the match to the instruction after the call: where the
    /// engine's function returns to, and where the jump goes.
    detour_at: usize,
    /// The instructions the jump overwrites, reproduced verbatim in the stub.
    /// None is relative, so they run unchanged from anywhere; none reads the
    /// flags, so the stub's own `test` in front of them changes nothing.
    stolen: &'static [u8],
}

/// Every site, in address order within each function. The comments are the
/// disassembly the pattern covers.
const SITES: [Site; 6] = [
    // lea ecx, [esp+0x50]; lea edx, [esp+0x2c]; push ecx; push edx
    // call [eax+0xc8]           ; R_TempSprite("sprites/shot-dust.spr")
    // mov esi, eax; push 0x5a; push 0
    Site {
        what: "hit puff dust",
        pattern: "8D 4C 24 50 8D 54 24 2C 51 52 FF 90 C8 00 00 00 8B F0 6A 5A 6A 00",
        detour_at: 0x10,
        stolen: &[0x8b, 0xf0, 0x6a, 0x5a, 0x6a, 0x00],
    },
    // lea ecx, [esp+0x5c]; push eax; push ecx
    // call [edx+0xc8]           ; R_TempSprite("sprites/blood-narrow.spr")
    // mov esi, eax; mov dword ptr [esi+0x354], 20.0
    Site {
        what: "hit puff blood",
        pattern: "8D 4C 24 5C 50 51 FF 92 C8 00 00 00 8B F0 C7 86 54 03 00 00 00 00 A0 41",
        detour_at: 0xc,
        stolen: &[
            0x8b, 0xf0, 0xc7, 0x86, 0x54, 0x03, 0x00, 0x00, 0x00, 0x00, 0xa0, 0x41,
        ],
    },
    // lea ecx, [esp+0x20]; fstp dword ptr [esp]; push eax; push ecx
    // call [esi]                ; R_TempSprite, cached in esi
    // mov dword ptr [eax+0x18], 4.0
    Site {
        what: "blood stream dust",
        pattern: "8D 4C 24 20 D9 1C 24 50 51 FF 16 C7 40 18 00 00 80 40",
        detour_at: 0xb,
        stolen: &[0xc7, 0x40, 0x18, 0x00, 0x00, 0x80, 0x40],
    },
    // lea eax, [esp+0x20]; fstp dword ptr [esp]; push edx; push eax
    // call [esi]                ; R_TempSprite, cached in esi
    // add esp, 0x24; mov dword ptr [eax+0x18], 4.0
    Site {
        what: "blood stream blood",
        pattern: "8D 44 24 20 D9 1C 24 52 50 FF 16 83 C4 24 C7 40 18 00 00 80 40",
        detour_at: 0xb,
        stolen: &[0x83, 0xc4, 0x24, 0xc7, 0x40, 0x18, 0x00, 0x00, 0x80, 0x40],
    },
    // mov dword ptr [esp+0x50], 0; mov dword ptr [esp+0x58], 0
    // call [eax+0xc0]           ; R_TempModel("models/shells.mdl")
    // add esp, 0x38; mov [eax+0x358], edi   ; curstate.body = shell type
    Site {
        what: "shell casing (player model)",
        pattern: "C7 44 24 50 00 00 00 00 C7 44 24 58 00 00 00 00 FF 90 C0 00 00 00 \
                  83 C4 38 89 B8 58 03 00 00",
        detour_at: 0x16,
        stolen: &[0x83, 0xc4, 0x38, 0x89, 0xb8, 0x58, 0x03, 0x00, 0x00],
    },
    // mov dword ptr [esp+0x44], 0; mov dword ptr [esp+0x4c], 0
    // fstp dword ptr [esp+0x34]
    // call [edx+0xc0]           ; R_TempModel("models/shells.mdl")
    // add esp, 0x24; mov [eax+0x358], esi   ; curstate.body = shell type
    Site {
        what: "shell casing (viewmodel)",
        pattern: "C7 44 24 44 00 00 00 00 C7 44 24 4C 00 00 00 00 D9 5C 24 34 \
                  FF 92 C0 00 00 00 83 C4 24 89 B0 58 03 00 00",
        detour_at: 0x1a,
        stolen: &[0x83, 0xc4, 0x24, 0x89, 0xb0, 0x58, 0x03, 0x00, 0x00],
    },
];

const SITE_COUNT: usize = SITES.len();

/// The stand-in temp entity. Larger than every offset the six sites write
/// (the highest is `EV_BloodPuff`'s `+0xba0`), which the verify script
/// checks. Never read by anyone: the engine does not know it
/// exists.
const SCRATCH_SIZE: usize = 0x1000;

/// Whether to install at all -- `GOLDSRC_HOOKS_TEMPENT_FIX=0` turns it off.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

/// The scratch buffer's address, allocated once and never freed: game code
/// may be writing to it at any moment.
static SCRATCH: AtomicUsize = AtomicUsize::new(0);

/// Where each stub jumps back to, read by the stubs themselves.
static RESUME: [AtomicUsize; SITE_COUNT] = [const { AtomicUsize::new(0) }; SITE_COUNT];

/// How many times each site got NULL, incremented by the stubs.
static SKIPPED: [AtomicU32; SITE_COUNT] = [const { AtomicU32::new(0) }; SITE_COUNT];

/// The last count of each site written to the log, so [`poll`] only logs a
/// change.
static LOGGED: [AtomicU32; SITE_COUNT] = [const { AtomicU32::new(0) }; SITE_COUNT];

/// The `client.dll` base the detours were written into, or 0. On a later
/// install at the same base, each site's own bytes say whether the loaded copy
/// still has its jump (see [`already_guarded`]), so a reloaded `client.dll` is
/// patched again rather than skipped.
static INSTALLED_BASE: AtomicUsize = AtomicUsize::new(0);

/// How many of the six are in place, for the status line.
static INSTALLED_COUNT: AtomicUsize = AtomicUsize::new(0);

/// See [`detour::Detour`] on why these are never undone.
static DETOURS: Mutex<Vec<detour::Detour>> = Mutex::new(Vec::new());

/// A site's stub, hand-assembled.
///
/// ```asm
/// test eax, eax
/// jnz  .game
/// mov  eax, SCRATCH                ; no entity: write into ours instead
/// lock inc dword ptr [SKIPPED[i]]
/// .game:
/// <the stolen instructions>
/// jmp  dword ptr [RESUME[i]]
/// ```
fn stub(scratch: usize, counter: usize, stolen: &[u8], resume: usize) -> Vec<u8> {
    let mut substitute = vec![0xb8]; // mov eax, imm32
    substitute.extend_from_slice(&(scratch as u32).to_le_bytes());
    substitute.extend_from_slice(&[0xf0, 0xff, 0x05]); // lock inc dword ptr [abs32]
    substitute.extend_from_slice(&(counter as u32).to_le_bytes());

    let mut code = vec![0x85, 0xc0, 0x75, substitute.len() as u8]; // test eax, eax; jnz
    code.extend_from_slice(&substitute);
    code.extend_from_slice(stolen);
    code.extend_from_slice(&[0xff, 0x25]); // jmp dword ptr [abs32]
    code.extend_from_slice(&(resume as u32).to_le_bytes());
    code
}

/// Patches every site it can find in the loaded `client.dll`. Called on the
/// engine thread once `client.dll` has initialised; a second call against the
/// same `client.dll` does nothing.
///
/// A site that does not match is logged and left alone, and the rest still go
/// in: each guards a different effect, and none depends on another.
pub fn install() {
    if !ENABLED.load(Ordering::Relaxed) {
        unsafe {
            crate::debug::report(
                "tempent_fix: off (GOLDSRC_HOOKS_TEMPENT_FIX=0) -- a missing temp entity can still crash the game",
            )
        };
        return;
    }
    let Some(base) = crate::engine::client_module_base() else {
        unsafe { crate::debug::report("tempent_fix: client.dll is not loaded yet; not installed") };
        return;
    };
    // client.dll opts out of ASLR, so a reloaded copy usually lands at the
    // same base: the base alone can't say whether this copy is patched. Only
    // at the same base is RESUME known to point into mapped code.
    let same_base = INSTALLED_BASE.load(Ordering::Acquire) == base;
    let Ok(mut detours) = DETOURS.lock() else {
        unsafe { crate::debug::report("tempent_fix: the detour lock is poisoned; not installed") };
        return;
    };

    let scratch = match SCRATCH.load(Ordering::Acquire) {
        0 => {
            let buffer = Box::leak(vec![0u8; SCRATCH_SIZE].into_boxed_slice());
            let address = buffer.as_mut_ptr() as usize;
            SCRATCH.store(address, Ordering::Release);
            address
        }
        address => address,
    };

    let mut installed = 0;
    let mut already = 0;
    for (index, site) in SITES.iter().enumerate() {
        if same_base && already_guarded(index, site) {
            already += 1;
            continue;
        }
        match install_site(index, site, base, scratch) {
            Ok(detour) => {
                detours.push(detour);
                installed += 1;
            }
            Err(why) => unsafe {
                crate::debug::report(&format!("tempent_fix: {} not guarded -- {why}", site.what))
            },
        }
    }
    INSTALLED_COUNT.store(installed + already, Ordering::Relaxed);
    INSTALLED_BASE.store(base, Ordering::Release);
    if installed == 0 && already > 0 {
        return;
    }
    let installed = installed + already;
    unsafe {
        crate::debug::report(&format!(
            "tempent_fix: {installed} of {SITE_COUNT} unchecked temp-entity calls guarded (scratch at {scratch:#x})"
        ))
    };
}

/// Whether site `index`'s jump is still in the loaded code: `E9` where the
/// stolen bytes began. Only called at the base the jump was written at.
fn already_guarded(index: usize, site: &Site) -> bool {
    let resume = RESUME[index].load(Ordering::Acquire);
    // Safety: the caller checked this is the base RESUME was computed
    // against, so the address is inside client.dll's mapped code.
    resume != 0 && unsafe { *((resume - site.stolen.len()) as *const u8) } == 0xe9
}

/// Scans for one site, checks its stolen bytes, and writes the jump.
fn install_site(
    index: usize,
    site: &Site,
    base: usize,
    scratch: usize,
) -> Result<detour::Detour, String> {
    // Safety: `base` is a module handle the loader gave us.
    let found = unsafe { scan::find_unique(base, site.pattern) }?;
    let target = found + site.detour_at;
    // Safety: `target` is inside the matched span, inside the code section.
    let present = unsafe { std::slice::from_raw_parts(target as *const u8, site.stolen.len()) };
    if present != site.stolen {
        return Err(format!(
            "expected {:02x?} at +{:#x}, found {present:02x?}",
            site.stolen,
            target - base
        ));
    }
    RESUME[index].store(target + site.stolen.len(), Ordering::Release);
    let code = stub(
        scratch,
        SKIPPED[index].as_ptr() as usize,
        site.stolen,
        RESUME[index].as_ptr() as usize,
    );
    // Safety: the span was checked byte for byte above, and nothing branches
    // into it -- checked by tools/verify_tempent_offsets.py.
    unsafe { detour::install(target, site.stolen.len(), &code) }
}

/// Logs any site that has skipped an effect since the last call. Runs every
/// frame from `commands::poll`; costs six atomic loads when nothing happened.
///
/// Logs the first skip per site and then every tenfold, so a map that runs the
/// pool dry all match long does not fill the log.
pub fn poll() {
    for (index, site) in SITES.iter().enumerate() {
        let now = SKIPPED[index].load(Ordering::Relaxed);
        let logged = LOGGED[index].load(Ordering::Relaxed);
        if now == logged || !worth_logging(logged, now) {
            continue;
        }
        LOGGED[index].store(now, Ordering::Relaxed);
        unsafe {
            crate::debug::report(&format!(
                "tempent_fix: the engine had no temp entity for a {} ({now} so far) -- effect skipped instead of crashing",
                site.what
            ))
        };
    }
}

/// True when the count has reached the next of 1, 10, 100, ... past `logged`.
pub(crate) fn worth_logging(logged: u32, now: u32) -> bool {
    let mut step = 1u32;
    while step <= logged {
        match step.checked_mul(10) {
            Some(next) => step = next,
            None => return false,
        }
    }
    now >= step
}

/// The `dodstudio_debug_status` line, or `None` when it was never installed.
pub fn status_line() -> Option<String> {
    if INSTALLED_BASE.load(Ordering::Relaxed) == 0 {
        return None;
    }
    let installed = INSTALLED_COUNT.load(Ordering::Relaxed);
    let skipped: Vec<String> = SITES
        .iter()
        .zip(&SKIPPED)
        .filter_map(|(site, count)| match count.load(Ordering::Relaxed) {
            0 => None,
            n => Some(format!("{} {n}", site.what)),
        })
        .collect();
    let skipped = if skipped.is_empty() {
        "none skipped".to_string()
    } else {
        format!("skipped: {}", skipped.join(", "))
    };
    Some(format!(
        "temp-entity crash fix: {installed} of {SITE_COUNT} sites guarded, {skipped}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stub_assembles_to_what_the_comment_claims() {
        let code = stub(0x1111_1111, 0x2222_2222, &[0xaa, 0xbb], 0x3333_3333);
        #[rustfmt::skip]
        assert_eq!(
            code,
            vec![
                0x85, 0xc0,                               // test eax, eax
                0x75, 0x0c,                               // jnz  .game
                0xb8, 0x11, 0x11, 0x11, 0x11,             // mov  eax, SCRATCH
                0xf0, 0xff, 0x05, 0x22, 0x22, 0x22, 0x22, // lock inc dword [SKIPPED]
                0xaa, 0xbb,                               // .game: the stolen bytes
                0xff, 0x25, 0x33, 0x33, 0x33, 0x33,       // jmp dword [RESUME]
            ]
        );
    }

    #[test]
    fn the_jnz_lands_on_the_stolen_bytes() {
        for site in &SITES {
            let code = stub(0, 0, site.stolen, 0);
            let lands = 4 + code[3] as usize;
            assert_eq!(
                &code[lands..lands + site.stolen.len()],
                site.stolen,
                "{}",
                site.what
            );
        }
    }

    #[test]
    fn every_site_is_long_enough_for_the_jump_and_ends_its_pattern() {
        for site in &SITES {
            assert!(
                site.stolen.len() >= 5,
                "{}: a jmp rel32 needs 5 bytes",
                site.what
            );
            let tokens: Vec<&str> = site.pattern.split_whitespace().collect();
            assert_eq!(
                tokens.len(),
                site.detour_at + site.stolen.len(),
                "{}: the stolen bytes should end the pattern",
                site.what
            );
            let tail: Vec<u8> = tokens[site.detour_at..]
                .iter()
                .map(|t| u8::from_str_radix(t, 16).expect("no wildcards in the stolen bytes"))
                .collect();
            assert_eq!(tail, site.stolen, "{}", site.what);
        }
    }

    #[test]
    fn logging_thins_out_by_tens() {
        let logged_at: Vec<u32> = (1..=1000)
            .scan(0u32, |logged, now| {
                Some(worth_logging(*logged, now).then(|| {
                    *logged = now;
                    now
                }))
            })
            .flatten()
            .collect();
        assert_eq!(logged_at, vec![1, 10, 100, 1000]);
        assert!(!worth_logging(u32::MAX, u32::MAX));
    }
}
