//! Stops `gl_spriteblend 0` at the session's first sprite load from
//! darkening sprites for the rest of the process (issue #467).
//!
//! ## The bug
//!
//! With `gl_spriteblend 0` already set when the first demo loads or the first
//! server is joined, the crosshair and other sprites draw dark, dotted and
//! pixelated -- and loading another demo does not fix it; only restarting
//! `hl.exe` with the value at 1 does. With 1 at that first load, switching to
//! 0 afterwards draws correctly.
//!
//! ## Why: one upload-time reader, and a texture cache
//!
//! `gl_spriteblend` has four readers in `hw.dll`. Three are draw-time
//! (blend state and texture environment around sprite draws) and read the
//! live value correctly. The fourth is in `GL_Upload32` (pre-Anniversary
//! `hw.dll+0x3e320`, 25th Anniversary `+0x23d260`):
//!
//! ```text
//!     fld   [gl_spriteblend.value]      ; Anniversary: movss/ucomiss/lahf
//!     fcomp [0.0]
//!     fnstsw ax
//!     test  ah, 0x44
//!     jnp   .no_bleed                   ; value == 0: skip   <- patched out
//!     cmp   esi, 1 / 3 / 4              ; the texture types that get it
//!     ...for every texel whose RGBA is all 0, average the RGB of its
//!        opaque 3x3 neighbours into it, alpha left at 0
//! ```
//!
//! That bleed is what lets filtered and blended sprite edges fade out
//! cleanly; without it the invisible texels stay black and every edge pulls
//! black in. `GL_LoadTexture` (pre-Anniversary `+0x3ec60`) returns the cached
//! texture by name and size without uploading again, so whatever
//! `gl_spriteblend` was at that first upload is baked in until the process
//! exits.
//!
//! ## The fix
//!
//! Two bytes: the `jnp` becomes two `nop`s, so the bleed always runs, as if
//! the value were 1 -- which is the engine's own default. It only changes the
//! RGB of texels whose alpha is 0, which alpha test discards anyway, so a
//! `gl_spriteblend 0` draw differs only at filtered edges, where it loses the
//! black fringe. The draw-time readers are untouched, so the cvar still means
//! what it does at draw time.
//!
//! Found by a byte pattern per build, each unique in its own `hw.dll` and
//! absent from the other. Before writing, the pattern's cvar operand must be
//! the address of `gl_spriteblend`'s value as the engine itself reports it
//! (`pfnGetCvarPointer`), so a match on some other float comparison can't be
//! patched. `goldsrc-hooks/tools/verify_spriteblend_offsets.py` re-derives
//! both sites offline.
//!
//! Installed from `install_fixes`, once `client.dll` has initialised and
//! before any map (and so any HUD sprite) loads. On by default;
//! `GOLDSRC_HOOKS_SPRITEBLEND_FIX=0` turns it off.

use std::ffi::CString;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::engine;
use crate::scan;

/// Whether to install at all -- `GOLDSRC_HOOKS_SPRITEBLEND_FIX=0` turns it off.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

/// The patched `jnp`, as an RVA, or 0 while not installed.
static PATCHED_RVA: AtomicUsize = AtomicUsize::new(0);

/// One build's view of the upload-time check.
struct Site {
    build: &'static str,
    pattern: &'static str,
    /// Offset of the 4-byte absolute address of `gl_spriteblend.value`.
    cvar_operand_at: usize,
    /// Offset of the `jnp rel8` (`7B xx`) that skips the bleed.
    jnp_at: usize,
}

#[rustfmt::skip]
const SITES: &[Site] = &[
    Site {
        build: "pre-Anniversary",
        //         fld [value]         mov eax,[..]   fcomp [0.0]          inc  mov [..],eax  fnstsw  test ah,44 jnp   cmp esi,1  je    cmp esi,3  je    cmp esi,4
        pattern: "D9 05 ?? ?? ?? ?? A1 ?? ?? ?? ?? D8 1D ?? ?? ?? ?? 40 A3 ?? ?? ?? ?? DF E0 F6 C4 44 7B ?? 83 FE 01 74 ?? 83 FE 03 74 ?? 83 FE 04",
        cvar_operand_at: 2,
        jnp_at: 28,
    },
    Site {
        build: "25th Anniversary",
        //         movss xmm0,[value]     ucomiss xmm0,[0.0]    lahf test ah,44 jnp   cmp esi,1  je    cmp esi,3  je    cmp esi,4
        pattern: "F3 0F 10 05 ?? ?? ?? ?? 0F 2E 05 ?? ?? ?? ?? 9F F6 C4 44 7B ?? 83 FE 01 74 ?? 83 FE 03 74 ?? 83 FE 04",
        cvar_operand_at: 4,
        jnp_at: 19,
    },
];

const JNP_REL8: u8 = 0x7b;

/// Makes sprite uploads always bleed colour into transparent texels. A second
/// call does nothing.
pub fn install() {
    if !ENABLED.load(Ordering::Relaxed) {
        unsafe {
            crate::debug::report(
                "sprite_blend: off (GOLDSRC_HOOKS_SPRITEBLEND_FIX=0) -- gl_spriteblend 0 at the first sprite load still darkens sprites for the session (#467)",
            )
        };
        return;
    }
    if PATCHED_RVA.load(Ordering::Acquire) != 0 {
        return;
    }
    match install_inner() {
        Ok((build, rva)) => {
            PATCHED_RVA.store(rva, Ordering::Release);
            unsafe {
                crate::debug::report(&format!(
                    "sprite_blend: {build} hw.dll -- sprite uploads always bleed edge colour (jnp at +{rva:#x} removed, #467)"
                ))
            };
        }
        Err(why) => unsafe {
            crate::debug::report(&format!("sprite_blend: not installed -- {why}"))
        },
    }
}

fn install_inner() -> Result<(&'static str, usize), String> {
    let base = engine::engine_module_base().ok_or("hw.dll is not loaded yet")?;
    let value_address = spriteblend_value_address()?;

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
        // Safety: both offsets are inside the matched span.
        let operand =
            unsafe { ((found + site.cvar_operand_at) as *const u32).read_unaligned() } as usize;
        if operand != value_address {
            return Err(format!(
                "the {} pattern matched at +{:#x}, but it reads {operand:#x}, not gl_spriteblend's value at {value_address:#x}",
                site.build,
                found - base
            ));
        }
        let jnp = found + site.jnp_at;
        // Safety: inside the matched span.
        let present = unsafe { *(jnp as *const u8) };
        if present != JNP_REL8 {
            return Err(format!(
                "expected jnp (7b) at +{:#x}, found {present:02x}",
                jnp - base
            ));
        }
        // Safety: two bytes of an instruction the pattern and the operand
        // check have just identified.
        if !unsafe { crate::patch::write_code_bytes(jnp, &[0x90, 0x90]) } {
            return Err(format!("could not make +{:#x} writable", jnp - base));
        }
        return Ok((site.build, jnp - base));
    }
    Err(format!("no known build matched ({})", errors.join("; ")))
}

/// `&gl_spriteblend.value`, from the engine's own cvar list.
fn spriteblend_value_address() -> Result<usize, String> {
    let engfuncs = engine::engfuncs().ok_or("the engine function table is not available yet")?;
    let name = CString::new("gl_spriteblend").map_err(|e| e.to_string())?;
    // Safety: the engine's own lookup, main thread.
    let cvar = unsafe { (engfuncs.pfn_get_cvar_pointer)(name.as_ptr()) };
    if cvar.is_null() {
        return Err("the engine has no gl_spriteblend cvar".to_string());
    }
    Ok(cvar as usize + std::mem::offset_of!(engine::CvarSPartial, value))
}

/// One `dodstudio_debug_status` line, once installed.
pub fn status_line() -> Option<String> {
    (PATCHED_RVA.load(Ordering::Relaxed) != 0)
        .then(|| "sprite uploads: edge colour always bled (gl_spriteblend at load no longer sticks, #467)".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(pattern: &str) -> Vec<&str> {
        pattern.split_whitespace().collect()
    }

    #[test]
    fn each_site_points_at_its_jnp_and_a_wildcarded_operand() {
        for site in SITES {
            let t = tokens(site.pattern);
            assert_eq!(t[site.jnp_at], "7B", "{}: jnp offset", site.build);
            assert_eq!(
                t[site.jnp_at + 1],
                "??",
                "{}: jnp target is wildcarded",
                site.build
            );
            assert!(
                t[site.cvar_operand_at..site.cvar_operand_at + 4]
                    .iter()
                    .all(|b| *b == "??"),
                "{}: the cvar operand is four wildcard bytes",
                site.build
            );
            // The comparison right before the jnp is `test ah, 0x44`: equal
            // or unordered, i.e. "value is 0".
            assert_eq!(t[site.jnp_at - 3..site.jnp_at], ["F6", "C4", "44"]);
        }
    }

    #[test]
    fn the_patterns_differ_so_one_build_cannot_match_the_other() {
        assert_ne!(SITES[0].pattern, SITES[1].pattern);
    }
}
