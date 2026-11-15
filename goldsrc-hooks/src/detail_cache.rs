//! Keeps detail textures from dropping out or going wrong on a section of
//! floor or wall depending on where the camera is (#463 on pre-Anniversary,
//! #703 on the 25th Anniversary build).
//!
//! ## The engine's cache
//!
//! `DT_SetRenderState` (pre-Anniversary `hw.dll+0x33ca0`, 25th Anniversary
//! `+0x1c3e50`), which `R_DrawSequentialPoly` calls before each run of
//! surfaces, binds the surface's detail texture on texture unit 2 and sets
//! that unit's texture matrix to the detail scale. It remembers the last
//! texture it bound and the last scale it set, and skips the bind or the
//! matrix when the next surface wants the same. Nothing ever resets those
//! three values, not even on a map change, and the engine assumes unit 2
//! still holds what it left there.
//!
//! When that assumption breaks, the runs that trust the cache draw with no
//! detail (flat and blurry) or with another surface's. Which runs those are
//! depends on draw order, and draw order on where the camera stands, so a
//! surface changes from one spot to the next. Measured on the 25th
//! Anniversary build (`whiskers-draft-teamnicholson-harrington-allies`, the
//! double doors leaving the Allied spawn, paused frame): with the cache the
//! door to the left drew blocky with no wood grain; with it bypassed, the
//! grain was there. The pre-Anniversary frames there came out the same both
//! ways.
//!
//! ## The fix
//!
//! The cache's two "same as last time?" branches become plain jumps to the
//! "different" side, so every run binds its own detail texture and sets its
//! own scale. That costs one bind and one matrix set per detailed run of
//! surfaces, which the engine did anyway before it cached them.
//!
//! `ds_debug_keep_detail_cache 1` puts the engine's bytes back, to
//! compare.
//!
//! Found by a byte pattern per build, each unique in its own `hw.dll`;
//! `tools/verify_detail_cache_offsets.py` re-derives both.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

use crate::engine::{self};
use crate::names::console_name;
use crate::scan;
use crate::setting::Cvar;

pub const KEEP_NAME: &str = console_name!("debug_keep_detail_cache");

/// One build's cache check in `DT_SetRenderState`.
struct Site {
    build: &'static str,
    pattern: &'static str,
    /// Offsets of the absolute addresses of the cached texture, horizontal
    /// scale and vertical scale. Only the tests and the verify script read
    /// them; they document what the pattern pins.
    #[allow(dead_code)]
    texture_at: usize,
    #[allow(dead_code)]
    hscale_at: usize,
    #[allow(dead_code)]
    vscale_at: usize,
    /// The `je` taken when nothing is cached yet (texture -1): made a `jmp`,
    /// the texture is always bound.
    bind_jump_at: usize,
    /// The first `jp` taken when the horizontal scale differs: made a `jmp`,
    /// the scale is always set.
    scale_jump_at: usize,
}

#[rustfmt::skip]
const SITES: &[Site] = &[
    Site {
        build: "pre-Anniversary",
        //         mov eax,[texture] mov dl,1 cmp eax,-1 mov bl,dl je    mov ecx,[esi+2C] cmp ecx,eax jne   xor dl,dl jmp   mov ecx,[esi+2C] mov [texture],ecx fld [esi+20] fcomp [hscale]  fnstsw test ah,44 jp    fld [esi+24] fcomp [vscale]
        pattern: "A1 ?? ?? ?? ?? B2 01 83 F8 FF 8A DA 74 0B 8B 4E 2C 3B C8 75 04 32 D2 EB 09 8B 4E 2C 89 0D ?? ?? ?? ?? D9 46 20 D8 1D ?? ?? ?? ?? DF E0 F6 C4 44 7A 14 D9 46 24 D8 1D ?? ?? ?? ??",
        texture_at: 1,
        hscale_at: 39,
        vscale_at: 55,
        bind_jump_at: 12,
        scale_jump_at: 48,
    },
    Site {
        build: "25th Anniversary",
        //         mov eax,[texture] mov dl,1 mov bl,dl cmp eax,-1 je    mov ecx,[esi+3C] cmp ecx,eax jne   xor dl,dl jmp   call [invalid_parameter]  mov ecx,[esi+3C] mov [texture],ecx movss xmm1,[esi+30] lea edi,[esi+34] movss xmm0,[hscale]       ucomiss lahf test ah,44 jp    movss xmm0,[vscale]
        pattern: "A1 ?? ?? ?? ?? B2 01 8A DA 83 F8 FF 74 11 8B 4E 3C 3B C8 75 0A 32 D2 EB 0F FF 15 ?? ?? ?? ?? 8B 4E 3C 89 0D ?? ?? ?? ?? F3 0F 10 4E 30 8D 7E 34 F3 0F 10 05 ?? ?? ?? ?? 0F 2E C1 9F F6 C4 44 7A 15 F3 0F 10 05 ?? ?? ?? ??",
        texture_at: 1,
        hscale_at: 52,
        vscale_at: 69,
        bind_jump_at: 12,
        scale_jump_at: 63,
    },
];

const JE: u8 = 0x74;
const JP: u8 = 0x7a;
const JMP: u8 = 0xeb;

pub static KEEP: Cvar = Cvar::new();
/// The matched pattern's address; 1 when it can't be found, 0 before a look.
static FOUND: AtomicUsize = AtomicUsize::new(0);
static SITE: AtomicUsize = AtomicUsize::new(0);
/// 0 not decided yet, 1 bypassing, 2 keeping the engine's bytes.
static STATE: AtomicU8 = AtomicU8::new(0);
static COMPLAINED: AtomicBool = AtomicBool::new(false);

fn keep() -> bool {
    KEEP.on()
}

/// The cache check, found once per session (hw.dll never reloads).
fn find() -> Option<(usize, &'static Site)> {
    match FOUND.load(Ordering::Acquire) {
        0 => {}
        1 => return None,
        at => return Some((at, &SITES[SITE.load(Ordering::Acquire)])),
    }
    let base = engine::engine_module_base()?;
    let mut errors = Vec::new();
    for (i, site) in SITES.iter().enumerate() {
        // Safety: `base` is a module handle the loader gave us.
        match unsafe { scan::find_unique(base, site.pattern) } {
            Ok(at) => {
                SITE.store(i, Ordering::Release);
                FOUND.store(at, Ordering::Release);
                return Some((at, site));
            }
            Err(why) => errors.push(format!("{}: {why}", site.build)),
        }
    }
    FOUND.store(1, Ordering::Release);
    unsafe {
        crate::debug::report(&format!(
            "detail_cache: DT_SetRenderState's cache wasn't found, so detail textures keep the engine's behaviour ({})",
            errors.join("; ")
        ))
    };
    None
}

/// Writes one branch opcode, checking it holds one of the two expected bytes.
fn set_opcode(address: usize, want: u8, other: u8) -> Result<(), String> {
    // Safety: inside the code the scan matched.
    let present = unsafe { *(address as *const u8) };
    if present == want {
        return Ok(());
    }
    if present != other {
        return Err(format!(
            "the byte at {address:#x} holds {present:#04x}, neither {want:#04x} nor {other:#04x} -- something else has patched it"
        ));
    }
    if unsafe { crate::patch::write_code_bytes(address, &[want]) } {
        Ok(())
    } else {
        Err(format!("could not make {address:#x} writable"))
    }
}

/// Every frame: bypasses the cache, or restores it while the debug cvar is 1.
pub fn poll() {
    let want = if keep() { 2 } else { 1 };
    if STATE.load(Ordering::Relaxed) == want {
        return;
    }
    let Some((at, site)) = find() else {
        return;
    };
    let bypass = want == 1;
    let (bind, scale) = if bypass {
        ((JMP, JE), (JMP, JP))
    } else {
        ((JE, JMP), (JP, JMP))
    };
    let result = set_opcode(at + site.bind_jump_at, bind.0, bind.1)
        .and_then(|_| set_opcode(at + site.scale_jump_at, scale.0, scale.1));
    match result {
        Ok(()) => {
            STATE.store(want, Ordering::Relaxed);
            let line = if bypass {
                format!(
                    "detail_cache: {} hw.dll -- every run of surfaces binds its own detail texture (#463)",
                    site.build
                )
            } else {
                format!(
                    "detail_cache: {} hw.dll -- the engine's detail cache is back ({KEEP_NAME} 1)",
                    site.build
                )
            };
            unsafe { crate::debug::report(&line) };
        }
        Err(why) => {
            if !COMPLAINED.swap(true, Ordering::Relaxed) {
                unsafe { crate::debug::report(&format!("detail_cache: {why}")) };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each address offset is the operand of the instruction the pattern says
    /// reads that value: `A1` for the texture, `D8 1D` / `F3 0F 10 05` for a
    /// scale.
    #[test]
    fn each_offset_follows_its_instruction() {
        for site in SITES {
            let b: Vec<&str> = site.pattern.split(' ').collect();
            assert_eq!(b[site.texture_at - 1], "A1", "{}", site.build);
            for at in [site.hscale_at, site.vscale_at] {
                let op = b[at - 2..at].join(" ");
                assert!(op == "D8 1D" || op == "10 05", "{}: {op}", site.build);
                assert!(b[at..at + 4].iter().all(|x| *x == "??"), "{}", site.build);
            }
        }
    }

    /// The two patched bytes are the `je` after `cmp eax,-1` and the `jp`
    /// after the first scale compare.
    #[test]
    fn each_jump_offset_lands_on_its_branch() {
        for site in SITES {
            let b: Vec<&str> = site.pattern.split(' ').collect();
            assert_eq!(b[site.bind_jump_at], "74", "{}", site.build);
            assert!(
                b[site.bind_jump_at - 4..site.bind_jump_at].contains(&"FF"),
                "{}",
                site.build
            );
            assert_eq!(b[site.scale_jump_at], "7A", "{}", site.build);
            assert_eq!(
                &b[site.scale_jump_at - 3..site.scale_jump_at],
                ["F6", "C4", "44"],
                "{}",
                site.build
            );
            assert!(site.scale_jump_at > site.hscale_at && site.scale_jump_at < site.vscale_at);
        }
    }
}
