//! Keeps detail textures from switching off on a section of floor or wall
//! depending on where the player stands (#463, and likely #703 on the 25th
//! Anniversary build).
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
//! When that assumption breaks -- the detail textures reloaded for a new map
//! under the same GL name, or anything else touching unit 2 between frames --
//! the first run of surfaces whose detail matches the remembered one draws
//! without it: flat and blurry. Which run that is depends on draw order, and
//! draw order on where the camera stands, which is why a surface loses its
//! detail from one spot and gets it back a few steps away.
//!
//! ## The fix
//!
//! Once a frame, before anything is drawn, [`poll`] forgets the cache: the
//! texture to -1 (the engine's own "nothing bound" value) and the scales to
//! NaN, which never compares equal. The first detailed run of each frame
//! then binds and sets its matrix again; the rest still use the cache. One
//! extra bind and matrix set a frame.
//!
//! `dodstudio_debug_keep_detail_cache 1` leaves the cache alone, to compare.
//!
//! Found by a byte pattern per build, each unique in its own `hw.dll`;
//! `tools/verify_detail_cache_offsets.py` re-derives both.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

use crate::engine::{self, CvarSPartial};
use crate::names::console_name;
use crate::scan;

pub const KEEP_NAME: &str = console_name!("debug_keep_detail_cache");

/// One build's cache check in `DT_SetRenderState`.
struct Site {
    build: &'static str,
    pattern: &'static str,
    /// Offsets of the absolute addresses of the cached texture, horizontal
    /// scale and vertical scale.
    texture_at: usize,
    hscale_at: usize,
    vscale_at: usize,
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
    },
    Site {
        build: "25th Anniversary",
        //         mov eax,[texture] mov dl,1 mov bl,dl cmp eax,-1 je    mov ecx,[esi+3C] cmp ecx,eax jne   xor dl,dl jmp   call [invalid_parameter]  mov ecx,[esi+3C] mov [texture],ecx movss xmm1,[esi+30] lea edi,[esi+34] movss xmm0,[hscale]       ucomiss lahf test ah,44 jp    movss xmm0,[vscale]
        pattern: "A1 ?? ?? ?? ?? B2 01 8A DA 83 F8 FF 74 11 8B 4E 3C 3B C8 75 0A 32 D2 EB 0F FF 15 ?? ?? ?? ?? 8B 4E 3C 89 0D ?? ?? ?? ?? F3 0F 10 4E 30 8D 7E 34 F3 0F 10 05 ?? ?? ?? ?? 0F 2E C1 9F F6 C4 44 7A 15 F3 0F 10 05 ?? ?? ?? ??",
        texture_at: 1,
        hscale_at: 52,
        vscale_at: 69,
    },
];

static KEEP: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
/// The cached texture's address; the two scales follow from [`SCALES`].
static TEXTURE: AtomicUsize = AtomicUsize::new(0);
static SCALES: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];
static GAVE_UP: AtomicBool = AtomicBool::new(false);

pub fn set_keep_cvar(cvar: *mut CvarSPartial) {
    KEEP.store(cvar, Ordering::Release);
}

fn keep() -> bool {
    let cvar = KEEP.load(Ordering::Acquire);
    // Safety: the engine owns the cvar for the session.
    !cvar.is_null() && unsafe { (*cvar).value } != 0.0
}

/// Finds the three cached values, once per session (hw.dll never reloads).
fn find() -> Option<usize> {
    let found = TEXTURE.load(Ordering::Acquire);
    if found != 0 {
        return Some(found);
    }
    if GAVE_UP.load(Ordering::Relaxed) {
        return None;
    }
    let base = engine::engine_module_base()?;
    let read = |at: usize| unsafe { std::ptr::read_unaligned(at as *const u32) } as usize;
    let mut errors = Vec::new();
    for site in SITES {
        // Safety: `base` is a module handle the loader gave us.
        match unsafe { scan::find_unique(base, site.pattern) } {
            Ok(at) => {
                let (texture, h, v) = (
                    read(at + site.texture_at),
                    read(at + site.hscale_at),
                    read(at + site.vscale_at),
                );
                SCALES[0].store(h, Ordering::Release);
                SCALES[1].store(v, Ordering::Release);
                TEXTURE.store(texture, Ordering::Release);
                unsafe {
                    crate::debug::report(&format!(
                        "detail_cache: {} hw.dll -- the detail texture cache is forgotten once a frame (#463)",
                        site.build
                    ))
                };
                return Some(texture);
            }
            Err(why) => errors.push(format!("{}: {why}", site.build)),
        }
    }
    GAVE_UP.store(true, Ordering::Relaxed);
    unsafe {
        crate::debug::report(&format!(
            "detail_cache: DT_SetRenderState's cache wasn't found, so detail textures keep the engine's behaviour ({})",
            errors.join("; ")
        ))
    };
    None
}

/// Every frame, before the world is drawn.
pub fn poll() {
    if keep() {
        return;
    }
    let Some(texture) = find() else {
        return;
    };
    // Safety: hw.dll globals the scan located, written on the engine's thread
    // between frames, where the engine itself reads them.
    unsafe {
        (texture as *mut i32).write_unaligned(-1);
        for scale in &SCALES {
            (scale.load(Ordering::Relaxed) as *mut f32).write_unaligned(f32::NAN);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each offset is the operand of the instruction the pattern says reads
    /// that value: `A1` for the texture, `D8 1D` / `F3 0F 10 05` for a scale.
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
}
