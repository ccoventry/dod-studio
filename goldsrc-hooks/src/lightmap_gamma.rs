//! Fixes the first demo of a session rendering with its lighting far too dark
//! (issue #365, "The First-Load Black Map Bug" in `docs/goldsrc_dod_quirks.md`),
//! which is why capture batches play a primer demo first.
//!
//! ## The bug
//!
//! A map's lightmaps are built once, at map load, by `GL_BuildLightmaps`
//! (`hw.dll` +0x49bc0), and `R_BuildLightMap` (+0x47390) passes every luxel
//! through `lightgammatable` (+0x24ebc80). That table is made by
//! `BuildGammaTable` (+0xc1770) from the `brightness` and `lightgamma` cvars.
//!
//! - `V_Init` builds it once at startup, while those cvars still hold their
//!   defaults (brightness 0, lightgamma 2.5).
//! - `config.cfg` and `movie.cfg` change the cvars later. The table only
//!   follows in `V_CheckGamma` (+0xc19b0), which runs at the start of every
//!   `SCR_UpdateScreen`, and it never rebuilds lightmaps when it does: the
//!   call it makes for that (+0x4703e) is an empty `ret`.
//! - A demo named on the command line loads its map before the first screen
//!   update. So the first map's lightmaps come from the default-cvar table:
//!   with the movie install's brightness 10 (clamped to 2 in multiplayer) and
//!   lightgamma 1.81, far too dark. The first rendered frame corrects the
//!   table, too late for that map; every later map is built right, which is
//!   why the primer demo works.
//!
//! ## The fix
//!
//! A detour on `GL_BuildLightmaps`' entry calls `V_CheckGamma` first. It is the
//! engine's own check, cdecl with no arguments, and does nothing unless a cvar
//! changed since it last ran, so every map load builds its lightmaps from the
//! cvars as they are. That covers every path to `GL_BuildLightmaps`: a map
//! load (`R_NewMap`), and the `sv_cheats` clamp's rebuild when it forces
//! `lightgamma` up to 1.8, which has the same stale-table problem.
//!
//! Textures are uploaded before the lightmaps, through `texgammatable`, and a
//! first map would still get those at the default `texgamma` (2.0). Nothing
//! here changes that; the movie install doesn't set `texgamma`.
//!
//! Pre-Anniversary `hw.dll` only (1,641,376 bytes); elsewhere the signatures
//! don't match and it logs "not installed". On by default, since it only
//! makes the first map match every later one;
//! `GOLDSRC_HOOKS_LIGHTMAP_GAMMA=0` turns it off.
//! `tools/verify_lightmap_gamma_offsets.py` checks every fact above against
//! the binary.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::detour;
use crate::scan;

/// `GL_BuildLightmaps`' entry: the prologue the stub reproduces, then the
/// `memset` of the lightmap allocation blocks (0x8000 bytes). Unique.
const BUILD_PATTERN: &str = "55 8B EC 51 53 56 57 68 00 80 00 00 6A 00 68 ?? ?? ?? ?? \
                             E8 ?? ?? ?? ?? A1 ?? ?? ?? ?? 83 C4 0C BF 01 00 00 00 85 C0 89 3D";

/// `push ebp; mov ebp, esp; push ecx; push ebx`: the five bytes the jump
/// overwrites. Not relative, and none reads the flags.
const STOLEN: [u8; 5] = [0x55, 0x8b, 0xec, 0x51, 0x53];

/// `V_CheckGamma`: its call to the cvar clamp, then the first two of its four
/// "changed since last time?" compares (gamma, lightgamma). Unique.
const CHECK_PATTERN: &str = "E8 ?? ?? ?? ?? D9 05 ?? ?? ?? ?? D8 1D ?? ?? ?? ?? DF E0 F6 C4 44 7A ?? \
                             D9 05 ?? ?? ?? ?? D8 1D ?? ?? ?? ?? DF E0 F6 C4 44 7A ?? D9 05";

/// Whether to install at all -- `GOLDSRC_HOOKS_LIGHTMAP_GAMMA=0` turns it off.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

/// `V_CheckGamma`'s address, which the stub calls through.
static CHECK_GAMMA: AtomicUsize = AtomicUsize::new(0);
/// Where the stub jumps back to: the instruction after the stolen bytes.
static RESUME: AtomicUsize = AtomicUsize::new(0);
/// Lightmap builds that found the tables stale and refreshed them first.
static REFRESHED: AtomicU32 = AtomicU32::new(0);
/// The last count written to the log.
static LOGGED: AtomicU32 = AtomicU32::new(0);

static INSTALLED: AtomicBool = AtomicBool::new(false);
/// See [`detour::Detour`] on why this is never undone.
static DETOUR: Mutex<Option<detour::Detour>> = Mutex::new(None);

/// The stub, hand-assembled. At entry nothing is in a register a cdecl
/// callee must keep, so `V_CheckGamma` may clobber eax/ecx/edx freely.
///
/// ```asm
///         call dword ptr [CHECK_GAMMA]   ; 1 when it rebuilt the tables
///         test eax, eax
///         jz   .build
///         lock inc dword ptr [REFRESHED]
/// .build: push ebp; mov ebp, esp; push ecx; push ebx
///         jmp  dword ptr [RESUME]
/// ```
fn stub(check_gamma: usize, refreshed: usize, resume: usize) -> Vec<u8> {
    const BUILD: usize = 17;
    let mut code = vec![0xff, 0x15]; // call dword ptr [abs32]
    code.extend_from_slice(&(check_gamma as u32).to_le_bytes());
    code.extend_from_slice(&[0x85, 0xc0]); // test eax, eax
    code.extend_from_slice(&[0x74, (BUILD - 10) as u8]); // jz .build
    code.extend_from_slice(&[0xf0, 0xff, 0x05]); // lock inc dword ptr [abs32]
    code.extend_from_slice(&(refreshed as u32).to_le_bytes());
    debug_assert_eq!(code.len(), BUILD);
    code.extend_from_slice(&STOLEN);
    code.extend_from_slice(&[0xff, 0x25]); // jmp dword ptr [abs32]
    code.extend_from_slice(&(resume as u32).to_le_bytes());
    code
}

/// Patches `GL_BuildLightmaps` in the loaded `hw.dll`. A second call does
/// nothing.
pub fn install() {
    if !ENABLED.load(Ordering::Relaxed) {
        unsafe {
            crate::debug::report(
                "lightmap_gamma: off (GOLDSRC_HOOKS_LIGHTMAP_GAMMA=0) -- the first demo of a session can be too dark (#365)",
            )
        };
        return;
    }
    if INSTALLED.load(Ordering::Acquire) {
        return;
    }
    let result = crate::engine::engine_module_base()
        .ok_or_else(|| "hw.dll is not loaded yet".to_string())
        .and_then(install_at);
    match result {
        Ok((detour, build, check)) => {
            if let Ok(mut slot) = DETOUR.lock() {
                *slot = Some(detour);
            }
            INSTALLED.store(true, Ordering::Release);
            unsafe {
                crate::debug::report(&format!(
                    "lightmap_gamma: GL_BuildLightmaps (+{build:#x}) refreshes the gamma tables (V_CheckGamma +{check:#x}) first (#365)"
                ))
            };
        }
        Err(why) => unsafe {
            crate::debug::report(&format!("lightmap_gamma: not installed -- {why}"))
        },
    }
}

fn install_at(base: usize) -> Result<(detour::Detour, usize, usize), String> {
    // Safety: `base` is a module handle the loader gave us.
    let build = unsafe { scan::find_unique(base, BUILD_PATTERN) }
        .map_err(|e| format!("GL_BuildLightmaps: {e}"))?;
    let check = unsafe { scan::find_unique(base, CHECK_PATTERN) }
        .map_err(|e| format!("V_CheckGamma: {e}"))?;
    // Safety: the match is inside the code section and longer than STOLEN.
    let present = unsafe { std::slice::from_raw_parts(build as *const u8, STOLEN.len()) };
    if present != STOLEN {
        return Err(format!(
            "expected {STOLEN:02x?} at +{:#x}, found {present:02x?}",
            build - base
        ));
    }
    CHECK_GAMMA.store(check, Ordering::Release);
    RESUME.store(build + STOLEN.len(), Ordering::Release);
    let code = stub(
        CHECK_GAMMA.as_ptr() as usize,
        REFRESHED.as_ptr() as usize,
        RESUME.as_ptr() as usize,
    );
    // Safety: the span was checked byte for byte above, and nothing branches
    // into it -- checked by tools/verify_lightmap_gamma_offsets.py.
    let detour = unsafe { detour::install(build, STOLEN.len(), &code) }?;
    Ok((detour, build - base, check - base))
}

/// Logs when a lightmap build found the tables stale, the first time and
/// then every tenfold. Runs every frame from `commands::poll`.
pub fn poll() {
    let now = REFRESHED.load(Ordering::Relaxed);
    let logged = LOGGED.load(Ordering::Relaxed);
    if now == logged || !crate::tempent_fix::worth_logging(logged, now) {
        return;
    }
    LOGGED.store(now, Ordering::Relaxed);
    unsafe {
        crate::debug::report(&format!(
            "lightmap_gamma: the gamma tables were out of date at a map load ({now} time(s) so far) -- refreshed before building its lightmaps (#365)"
        ))
    };
}

/// The `dodstudio_debug_status` line, or `None` when it was never installed.
pub fn status_line() -> Option<String> {
    INSTALLED.load(Ordering::Relaxed).then(|| {
        format!(
            "first-load lighting fix: on, refreshed stale gamma tables before {} lightmap build(s)",
            REFRESHED.load(Ordering::Relaxed)
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stub_assembles_to_what_the_comment_claims() {
        #[rustfmt::skip]
        assert_eq!(
            stub(0x1111_1111, 0x2222_2222, 0x3333_3333),
            vec![
                0xff, 0x15, 0x11, 0x11, 0x11, 0x11,       //  0 call dword [CHECK_GAMMA]
                0x85, 0xc0,                               //  6 test eax, eax
                0x74, 0x07,                               //  8 jz   .build (17)
                0xf0, 0xff, 0x05, 0x22, 0x22, 0x22, 0x22, // 10 lock inc [REFRESHED]
                0x55, 0x8b, 0xec, 0x51, 0x53,             // 17 .build: the stolen bytes
                0xff, 0x25, 0x33, 0x33, 0x33, 0x33,       // 22 jmp dword [RESUME]
            ]
        );
    }

    #[test]
    fn the_build_pattern_starts_with_the_stolen_bytes() {
        let head: Vec<u8> = BUILD_PATTERN
            .split_whitespace()
            .take(STOLEN.len())
            .map(|t| u8::from_str_radix(t, 16).expect("no wildcards in the stolen bytes"))
            .collect();
        assert_eq!(head, STOLEN);
        assert!(scan::Pattern::parse(CHECK_PATTERN).is_ok());
    }
}
