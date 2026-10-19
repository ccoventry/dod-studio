//! A LOD bias of -1 on replaced HUD sprites (#841), so a kill feed drawn at
//! `dodstudio_deathmsg scale 2` or `3` samples the full-detail mip level.
//!
//! A replaced HUD sheet goes up 4x with mipmaps (`texture_hires.rs`, #728).
//! The GPU picks the level from texels per screen pixel: at scale 3 a 4x
//! sheet has 4/3 texels per pixel, so trilinear filtering mixes level 0 with
//! the 2x level 1, about 40 % of the way to it, and a rendered kill-feed icon
//! (#841) loses half its sharpness. A bias of -1 moves every lookup one level
//! finer: scale 3 then reads level 0 alone, scale 2 level 0 too, and scale 1
//! (one sheet texel per four texels) level 1 sampled 2:1, which bilinear
//! filtering at those pixel centres averages into the same 2x2 box level 2
//! holds -- so the 1:1 HUD draws as before.
//!
//! `GL_TEXTURE_LOD_BIAS` is a texture parameter (OpenGL 1.4), kept by the
//! texture object, so it is set once, right after `GL_Upload32` has filled
//! the texture (the texture `glTexImage2D` just filled is the bound one),
//! through `opengl32.dll`'s own export: no engine offsets, the same on both
//! `hw.dll` builds. Only replaced HUD sprites get it; world textures, model
//! skins, world sprites and the stock HUD sheets keep the engine's state.
//! It is read back, so `dodstudio_debug_status` counts only biases the
//! driver took.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::atomic::{AtomicUsize, Ordering};

use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

pub const GL_TEXTURE_2D: u32 = 0x0DE1;
pub const GL_TEXTURE_LOD_BIAS: u32 = 0x8501;
/// The bias a replaced HUD sprite gets.
pub const HUD_LOD_BIAS: f32 = -1.0;

type TexParameterF = unsafe extern "system" fn(u32, u32, f32);
type GetTexParameterFv = unsafe extern "system" fn(u32, u32, *mut f32);

/// `glTexParameterf` and `glGetTexParameterfv`, looked up on first use; 1
/// once a lookup failed.
static SET: AtomicUsize = AtomicUsize::new(0);
static GET: AtomicUsize = AtomicUsize::new(0);

/// Whether a replacement of this texture type gets the bias: HUD sprites
/// (`GLT_HUDSPRITE`, 2) uploaded with mipmaps, nothing else.
pub fn wants_bias(texture_type: u32, mipmap: u32) -> bool {
    texture_type == 2 && mipmap != 0
}

/// Whether the read-back bias is the one set (a driver without the
/// parameter leaves 0).
pub fn took(read_back: f32) -> bool {
    (read_back - HUD_LOD_BIAS).abs() < 1e-3
}

fn procs() -> Option<(TexParameterF, GetTexParameterFv)> {
    if SET.load(Ordering::Acquire) == 0 {
        let gl = unsafe { GetModuleHandleA(c"opengl32.dll".as_ptr() as *const u8) };
        let (set, get) = if gl.is_null() {
            (None, None)
        } else {
            unsafe {
                (
                    GetProcAddress(gl, c"glTexParameterf".as_ptr() as *const u8),
                    GetProcAddress(gl, c"glGetTexParameterfv".as_ptr() as *const u8),
                )
            }
        };
        match (set, get) {
            (Some(s), Some(g)) => {
                GET.store(g as usize, Ordering::Release);
                SET.store(s as usize, Ordering::Release);
            }
            _ => SET.store(1, Ordering::Release),
        }
    }
    let (set, get) = (SET.load(Ordering::Acquire), GET.load(Ordering::Acquire));
    if set <= 1 || get == 0 {
        return None;
    }
    // Safety: both are opengl32.dll exports with these signatures.
    unsafe {
        Some((
            std::mem::transmute::<usize, TexParameterF>(set),
            std::mem::transmute::<usize, GetTexParameterFv>(get),
        ))
    }
}

/// Sets [`HUD_LOD_BIAS`] on the bound 2D texture; whether the driver kept it.
///
/// # Safety
///
/// Only on the engine's GL thread, with its context current: right after
/// `GL_Upload32` returned.
pub unsafe fn apply() -> bool {
    let Some((set, get)) = procs() else {
        return false;
    };
    let mut read_back = 0.0f32;
    unsafe {
        set(GL_TEXTURE_2D, GL_TEXTURE_LOD_BIAS, HUD_LOD_BIAS);
        get(GL_TEXTURE_2D, GL_TEXTURE_LOD_BIAS, &mut read_back);
    }
    took(read_back)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_mipmapped_hud_sprites_get_the_bias() {
        assert!(wants_bias(2, 1));
        assert!(!wants_bias(2, 0));
        // decals, model skins, world textures, world sprites
        for kind in [1, 3, 4, 5] {
            assert!(!wants_bias(kind, 1), "type {kind}");
        }
    }

    #[test]
    fn read_back_must_match() {
        assert!(took(-1.0));
        assert!(!took(0.0));
        assert!(!took(-0.5));
    }

    #[test]
    fn gl_enums() {
        assert_eq!(GL_TEXTURE_2D, 0x0DE1);
        assert_eq!(GL_TEXTURE_LOD_BIAS, 0x8501);
    }
}
