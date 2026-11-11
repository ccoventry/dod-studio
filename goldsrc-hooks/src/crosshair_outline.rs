//! `ds_hud_crosshair_outline_size` and `_color`: a border around any crosshair,
//! its thickness in the frame's own pixels whatever `ds_hud_crosshair_scale` is
//! (`docs/goldsrc_crosshair.md` §5).
//!
//! The kill-feed icons' outline is baked into their art
//! (`tools/hd/killfeed_icons.py`). A crosshair sheet has no room for that: the
//! creator's biggest crosshairs reach the edge of the 64x64 tile, and a border
//! in a sheet doubles with the scale. So the border is drawn: the same draw of
//! the crosshair, once for each pixel offset out to the size, in one flat
//! colour, and then the crosshair itself on top. The offsets fill the whole
//! square, not just its rim, or an arm thinner than the size would leave a gap
//! between it and its copy.
//!
//! The flat colour needs more than `SPR_Set`'s tint, which multiplies the
//! sprite's pixels: black works that way, but a tile's cyan under a white tint
//! stays cyan. Replacing the colour in the texture stage does nothing, since
//! the engine's draw sets that stage itself (measured on both builds). So the
//! copies draw under GL fog at full strength: it replaces a fragment's colour
//! after texturing and leaves its alpha, the crosshair's shape, alone.
//!
//! Only `SPR_DrawHoles` draws get a border. Its sprites are masked, so the
//! copies have the crosshair's shape; `SPR_DrawAdditive`'s have no mask, and
//! would draw the whole rect in the flat colour.
//!
//! The draw path reads atomics only; the cvars are read once a frame by
//! [`poll`].

use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use crate::crosshair_scale::Zoom;
use crate::hud_sprites::Wrect;
use crate::names::console_name;
use crate::player_crosshair::Sheet;
use crate::setting::Cvar;
use crate::spec_xray::colour::{self, Cached};

/// The cvars' names. Registered in `commands/install.rs`.
pub const SIZE_NAME: &str = console_name!("hud_crosshair_outline_size");
pub const COLOR_NAME: &str = console_name!("hud_crosshair_outline_color");
pub const COLOR_DEFAULT: &str = "0 0 0";

pub static SIZE: Cvar = Cvar::new();
pub static COLOR: Cvar = Cvar::new();
static COLOR_CACHE: Cached = Cached::new();

/// The thickest border, in pixels. Each pixel of size adds a ring of copies,
/// so 8 is 288 draws for each piece of the crosshair.
const MAX_SIZE: i32 = 8;

/// The border's thickness in pixels, 0 for none.
static PIXELS: AtomicI32 = AtomicI32::new(0);
/// The border's colour, `0x00bbggrr`.
static RGB: AtomicU32 = AtomicU32::new(0);

/// The border a draw gets: its thickness in pixels and its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outline {
    size: i32,
    rgb: [i32; 3],
}

/// The size a cvar's value asks for: the nearest whole number of pixels, at
/// most [`MAX_SIZE`]. Anything that isn't a positive number is no border.
fn wanted(raw: f32) -> i32 {
    if raw.is_finite() && raw > 0.0 {
        (raw.round() as i32).min(MAX_SIZE)
    } else {
        0
    }
}

/// Once a frame from `commands::poll`.
pub fn poll() {
    let Some(raw) = SIZE.value() else {
        return;
    };
    let size = wanted(raw);
    let [r, g, b] = colour::read(&COLOR, &COLOR_CACHE, COLOR_NAME, COLOR_DEFAULT);
    let rgb = u32::from_le_bytes([r, g, b, 0]);
    let old_size = PIXELS.swap(size, Ordering::Relaxed);
    let old_rgb = RGB.swap(rgb, Ordering::Relaxed);
    if old_size != size || old_rgb != rgb {
        unsafe {
            crate::debug::report(&format!(
                "commands: crosshair outline {size} px, colour {r} {g} {b}"
            ))
        };
    }
}

/// The offsets of the copies for a border `size` pixels thick: every whole
/// offset in the square, the centre left out, since that is the crosshair.
fn offsets(size: i32) -> impl Iterator<Item = (i32, i32)> {
    (-size..=size)
        .flat_map(move |y| (-size..=size).map(move |x| (x, y)))
        .filter(|&offset| offset != (0, 0))
}

/// From a `SPR_DrawHoles` hook: the border this draw gets, if it is one of the
/// crosshair's and a border is on. Atomics only.
///
/// # Safety
///
/// `prc` is the rect pointer the game passed to the draw, null or valid.
pub unsafe fn of_draw(sheet: Sheet, prc: *const Wrect) -> Option<Outline> {
    let size = PIXELS.load(Ordering::Relaxed);
    // Safety: the caller's.
    if size == 0 || !unsafe { crate::crosshair_scale::is_crosshair(sheet, prc) } {
        return None;
    }
    let [r, g, b, _] = RGB.load(Ordering::Relaxed).to_le_bytes();
    Some(Outline {
        size,
        rgb: [i32::from(r), i32::from(g), i32::from(b)],
    })
}

/// Draws the border under `zoom`, the crosshair's own zoom
/// (`crosshair_scale::zoom`, which every crosshair draw has): `draw` is the
/// crosshair's draw, given the tint to use, and runs once for each copy. An
/// offset is in the frame's pixels, since it is applied after the zoom, so
/// each copy lands on the same whole pixels as the crosshair, moved.
///
/// # Safety
///
/// Only from a HUD draw, on the engine's thread with its GL context current.
pub unsafe fn draw(outline: Outline, zoom: Zoom, draw: impl Fn([i32; 3])) {
    // Safety: the caller's.
    unsafe {
        flat_colour(outline.rgb, || {
            for (x, y) in offsets(outline.size) {
                if !zoom.shifted(x, y).run(|| draw(outline.rgb)) {
                    return;
                }
            }
        })
    };
}

/// Runs `draw` with every fragment's colour replaced by `rgb` and its alpha
/// kept, then puts GL's fog back. Without the GL calls it runs `draw` as it
/// is, tinted: a black border still draws.
///
/// # Safety
///
/// As [`draw`].
unsafe fn flat_colour(rgb: [i32; 3], draw: impl FnOnce()) {
    #[cfg(target_arch = "x86")]
    {
        if let Ok(gl) = gl::get() {
            // Safety: the caller's.
            unsafe { gl.flat(rgb, draw) };
            return;
        }
    }
    let _ = rgb;
    draw();
}

/// The `ds_debug_status` line, while a border is on.
pub fn status_line() -> Option<String> {
    let size = PIXELS.load(Ordering::Relaxed);
    let [r, g, b, _] = RGB.load(Ordering::Relaxed).to_le_bytes();
    (size != 0).then(|| {
        format!("{SIZE_NAME} {size}: the crosshair has a {size} px border, colour {r} {g} {b}")
    })
}

#[cfg(target_arch = "x86")]
mod gl {
    use std::sync::OnceLock;

    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    const GL_FOG: u32 = 0x0b60;
    const GL_FOG_MODE: u32 = 0x0b65;
    const GL_FOG_START: u32 = 0x0b63;
    const GL_FOG_END: u32 = 0x0b64;
    const GL_FOG_COLOR: u32 = 0x0b66;
    const GL_LINEAR: i32 = 0x2601;

    type IsEnabledFn = unsafe extern "system" fn(u32) -> u8;
    type CapFn = unsafe extern "system" fn(u32);
    type GetIntegervFn = unsafe extern "system" fn(u32, *mut i32);
    type GetFloatvFn = unsafe extern "system" fn(u32, *mut f32);
    type FogiFn = unsafe extern "system" fn(u32, i32);
    type FogfFn = unsafe extern "system" fn(u32, f32);
    type FogfvFn = unsafe extern "system" fn(u32, *const f32);

    pub(super) struct Gl {
        is_enabled: IsEnabledFn,
        enable: CapFn,
        disable: CapFn,
        get_integerv: GetIntegervFn,
        get_floatv: GetFloatvFn,
        fogi: FogiFn,
        fogf: FogfFn,
        fogfv: FogfvFn,
    }

    impl Gl {
        pub(super) unsafe fn flat(&self, rgb: [i32; 3], draw: impl FnOnce()) {
            let colour = [
                rgb[0] as f32 / 255.0,
                rgb[1] as f32 / 255.0,
                rgb[2] as f32 / 255.0,
                1.0,
            ];
            let (mut mode, mut start, mut end) = (0i32, 0f32, 0f32);
            let mut old_colour = [0f32; 4];
            // Safety: the caller's (engine thread, context current).
            unsafe {
                let was_on = (self.is_enabled)(GL_FOG) != 0;
                (self.get_integerv)(GL_FOG_MODE, &mut mode);
                (self.get_floatv)(GL_FOG_START, &mut start);
                (self.get_floatv)(GL_FOG_END, &mut end);
                (self.get_floatv)(GL_FOG_COLOR, old_colour.as_mut_ptr());
                // Linear fog ends behind the eye, so every fragment is past
                // its end: all fog and none of the sprite's colour.
                (self.fogi)(GL_FOG_MODE, GL_LINEAR);
                (self.fogf)(GL_FOG_START, -2.0);
                (self.fogf)(GL_FOG_END, -1.0);
                (self.fogfv)(GL_FOG_COLOR, colour.as_ptr());
                (self.enable)(GL_FOG);
                draw();
                if !was_on {
                    (self.disable)(GL_FOG);
                }
                (self.fogi)(GL_FOG_MODE, mode);
                (self.fogf)(GL_FOG_START, start);
                (self.fogf)(GL_FOG_END, end);
                (self.fogfv)(GL_FOG_COLOR, old_colour.as_ptr());
            }
        }
    }

    static GL: OnceLock<Gl> = OnceLock::new();

    fn load() -> Result<Gl, String> {
        let module = unsafe { GetModuleHandleA(c"opengl32.dll".as_ptr() as *const u8) };
        if module.is_null() {
            return Err("opengl32.dll is not loaded".to_string());
        }
        let proc = |name: &std::ffi::CStr| -> Result<usize, String> {
            unsafe { GetProcAddress(module, name.as_ptr() as *const u8) }
                .map(|p| p as usize)
                .ok_or_else(|| format!("opengl32.dll has no {}", name.to_string_lossy()))
        };
        // Safety: the exported functions' own signatures (stdcall).
        unsafe {
            Ok(Gl {
                is_enabled: std::mem::transmute::<usize, IsEnabledFn>(proc(c"glIsEnabled")?),
                enable: std::mem::transmute::<usize, CapFn>(proc(c"glEnable")?),
                disable: std::mem::transmute::<usize, CapFn>(proc(c"glDisable")?),
                get_integerv: std::mem::transmute::<usize, GetIntegervFn>(proc(c"glGetIntegerv")?),
                get_floatv: std::mem::transmute::<usize, GetFloatvFn>(proc(c"glGetFloatv")?),
                fogi: std::mem::transmute::<usize, FogiFn>(proc(c"glFogi")?),
                fogf: std::mem::transmute::<usize, FogfFn>(proc(c"glFogf")?),
                fogfv: std::mem::transmute::<usize, FogfvFn>(proc(c"glFogfv")?),
            })
        }
    }

    pub(super) fn get() -> Result<&'static Gl, String> {
        if let Some(gl) = GL.get() {
            return Ok(gl);
        }
        let loaded = load()?;
        let _ = GL.set(loaded);
        GL.get().ok_or_else(|| "GL functions not kept".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bad_value_is_no_border_and_a_big_one_is_capped() {
        assert_eq!(wanted(2.0), 2);
        assert_eq!(wanted(1.4), 1);
        assert_eq!(wanted(0.0), 0);
        assert_eq!(wanted(-3.0), 0);
        assert_eq!(wanted(f32::NAN), 0);
        assert_eq!(wanted(f32::INFINITY), 0);
        assert_eq!(wanted(100.0), MAX_SIZE);
    }

    #[test]
    fn the_copies_fill_the_square_and_leave_the_crosshair_out() {
        assert_eq!(offsets(1).count(), 8);
        assert_eq!(offsets(MAX_SIZE).count(), 288);
        let two: Vec<_> = offsets(2).collect();
        assert_eq!(two.len(), 24);
        assert!(!two.contains(&(0, 0)));
        // An arm one pixel wide stays joined to its border.
        assert!(two.contains(&(1, 0)) && two.contains(&(2, 0)));
        assert!(two.contains(&(2, 2)) && two.contains(&(-2, -2)));
    }

    #[test]
    fn no_border_means_no_outline_for_any_draw() {
        PIXELS.store(0, Ordering::Relaxed);
        let rect = Wrect {
            left: 0,
            right: 64,
            top: 0,
            bottom: 64,
        };
        assert_eq!(unsafe { of_draw(Sheet::Custom, &rect) }, None);
    }
}
