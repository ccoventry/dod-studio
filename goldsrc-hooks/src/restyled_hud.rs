//! R&D proof for #849: one stock HUD readout redrawn in a style of our own.
//!
//! `ds_debug_restyled_health 1` draws the health as a TF2-like plus in
//! the bottom-left corner, filled from the bottom by the health left, with the
//! number on it. Pair it with the stock `cl_hud_health 0`, which hides DoD's
//! own health block (`CHud::ShouldDraw(1)`, `docs/goldsrc_client_dll_survey.md`
//! §2): `hud_main`, the stamina bar, the health bar and the heart.
//!
//! ## Where it draws
//!
//! After `HUD_Redraw` (`cldll_func_t` slot 3, `client+0x2a5b0`), which the
//! engine calls in its 2D pass: the projection is the screen in pixels, y
//! down, the same space `SPR_Draw*` and `FillRGBA` use. The shapes are plain
//! untextured GL quads from `opengl32.dll`, with the enable, colour and
//! current state pushed and popped around them, so the engine's own state is
//! left as it was. `pfnFillRGBA` can't do this: it blends additively, so dark
//! colours draw nothing (`demo_scope/fill.rs` says the same).
//!
//! ## Where the health comes from
//!
//! - **POV demo or live play:** `CHudDodIcons::m_iHealth`, `+0x14` on the
//!   element. `Draw` reads it for the health bar's height
//!   (`fimul [esi+0x14]` at `client+0x2de48`). The element is found by
//!   walking `gHUD`'s element list (`client+0x1080b0`) for its vftable
//!   (`client+0xac350`, checked by `verify_hudelements.py`).
//! - **HLTV in-eye:** the per-player health the `HLTV` user message keeps
//!   (`client+0xe913c + player * 0x30`, written at `client+0x222c9`; #830's
//!   findings), for the in-eye target.
//!
//! The digits are seven-segment quads, enough to judge the look; a real
//! version would draw text through the vgui2 surface with a font of our own,
//! as `deathmsg/font.rs` does.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code, unused_imports))]

use std::sync::OnceLock;
use std::sync::atomic::{AtomicPtr, Ordering};

use crate::engine::{self, CvarSPartial, ScreenInfo};
use crate::names::console_name;

pub const NAME: &str = console_name!("debug_restyled_health");

/// `gHUD`; its first field is the element list's head.
const GHUD_RVA: usize = 0x10_80b0;
/// `CHudDodIcons`' vftable (`hudelement.rs`' `icons`).
const DOD_ICONS_VFTABLE_RVA: usize = 0xac350;
/// `CHudDodIcons::m_iHealth`.
const HEALTH_FIELD: usize = 0x14;
/// The `HLTV` message's per-player health, 0x30 bytes per player.
const HLTV_HEALTH_RVA: usize = 0xe_913c;
const HLTV_HEALTH_STRIDE: usize = 0x30;
const HLTV_PLAYERS: i32 = 64;

static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());

pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

fn on() -> bool {
    let cvar = CVAR.load(Ordering::Acquire);
    // Safety: the engine owns the cvar for the session.
    !cvar.is_null() && unsafe { (*cvar).value != 0.0 }
}

/// The health to show: the in-eye target's in a spectated demo, the HUD's own
/// otherwise; `None` while spectating in any other camera mode.
fn health() -> Option<i32> {
    let base = engine::client_module_base()?;
    if let Some(target) = crate::spectator_target::in_eye_target() {
        if !(1..HLTV_PLAYERS).contains(&target) {
            return None;
        }
        let at = base + HLTV_HEALTH_RVA + target as usize * HLTV_HEALTH_STRIDE;
        // Safety: inside client.dll's .data table of 64 entries.
        return Some(unsafe { *(at as *const i32) });
    }
    if crate::spectator_target::mode().unwrap_or(0) != 0 {
        return None;
    }
    let vftable = base + DOD_ICONS_VFTABLE_RVA;
    // Safety: gHUD's list of 8-byte {element, next} nodes, built by
    // CHud::AddHudElem; at most 22 long. Bounded anyway.
    let mut node = unsafe { *((base + GHUD_RVA) as *const usize) };
    for _ in 0..64 {
        if node == 0 {
            return None;
        }
        let element = unsafe { *(node as *const usize) };
        if element != 0 && unsafe { *(element as *const usize) } == vftable {
            return Some(unsafe { *((element + HEALTH_FIELD) as *const i32) });
        }
        node = unsafe { *((node + 4) as *const usize) };
    }
    None
}

/// A rectangle in screen pixels, `x, y, w, h`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect(pub f32, pub f32, pub f32, pub f32);

/// The plus' two bars for a plus of `size` centred on `(cx, cy)`, each arm a
/// third of the size wide.
pub fn plus_bars(cx: f32, cy: f32, size: f32) -> [Rect; 2] {
    let arm = size / 3.0;
    [
        Rect(cx - size / 2.0, cy - arm / 2.0, size, arm),
        Rect(cx - arm / 2.0, cy - size / 2.0, arm, size),
    ]
}

/// The part of `rect` at or below `cut_y`.
pub fn below(rect: Rect, cut_y: f32) -> Option<Rect> {
    let Rect(x, y, w, h) = rect;
    let top = y.max(cut_y);
    (top < y + h).then_some(Rect(x, top, w, y + h - top))
}

/// Seven-segment bits for 0..9: a b c d e f g, a = top, clockwise, g middle.
const SEGMENTS: [u8; 10] = [
    0b011_1111, 0b000_0110, 0b101_1011, 0b100_1111, 0b110_0110, 0b110_1101, 0b111_1101, 0b000_0111,
    0b111_1111, 0b110_1111,
];

/// The segments of one digit `height` tall with its top-left at `(x, y)`.
pub fn digit_rects(digit: u32, x: f32, y: f32, height: f32) -> Vec<Rect> {
    let w = height * 0.5;
    let t = height * 0.14;
    let half = height / 2.0;
    let all = [
        Rect(x, y, w, t),                   // a
        Rect(x + w - t, y, t, half),        // b
        Rect(x + w - t, y + half, t, half), // c
        Rect(x, y + height - t, w, t),      // d
        Rect(x, y + half, t, half),         // e
        Rect(x, y, t, half),                // f
        Rect(x, y + half - t / 2.0, w, t),  // g
    ];
    let bits = SEGMENTS[(digit % 10) as usize];
    (0..7)
        .filter(|i| bits & (1 << i) != 0)
        .map(|i| all[i])
        .collect()
}

/// The fill colour for `health`: TF2's cream when healthy, amber at half,
/// red when low.
pub fn fill_colour(health: i32) -> (f32, f32, f32) {
    match health {
        h if h > 50 => (0.93, 0.89, 0.80),
        h if h > 25 => (1.0, 0.70, 0.24),
        _ => (0.86, 0.20, 0.16),
    }
}

type EnumFn = unsafe extern "system" fn(u32);
type NoArgFn = unsafe extern "system" fn();
type TwoEnumsFn = unsafe extern "system" fn(u32, u32);
type Color4fFn = unsafe extern "system" fn(f32, f32, f32, f32);
type Vertex2fFn = unsafe extern "system" fn(f32, f32);

struct Gl {
    push_attrib: EnumFn,
    pop_attrib: NoArgFn,
    enable: EnumFn,
    disable: EnumFn,
    blend_func: TwoEnumsFn,
    color4f: Color4fFn,
    begin: EnumFn,
    end: NoArgFn,
    vertex2f: Vertex2fFn,
}

const GL_CURRENT_BIT: u32 = 0x0001;
const GL_ENABLE_BIT: u32 = 0x2000;
const GL_COLOR_BUFFER_BIT: u32 = 0x4000;
const GL_TEXTURE_2D: u32 = 0x0de1;
const GL_BLEND: u32 = 0x0be2;
const GL_ALPHA_TEST: u32 = 0x0bc0;
const GL_DEPTH_TEST: u32 = 0x0b71;
const GL_SRC_ALPHA: u32 = 0x0302;
const GL_ONE_MINUS_SRC_ALPHA: u32 = 0x0303;
const GL_QUADS: u32 = 0x0007;

static GL: OnceLock<Option<Gl>> = OnceLock::new();

#[cfg(target_arch = "x86")]
fn load_gl() -> Option<Gl> {
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
    let module = unsafe { GetModuleHandleA(c"opengl32.dll".as_ptr() as *const u8) };
    if module.is_null() {
        return None;
    }
    let proc = |name: &std::ffi::CStr| unsafe {
        GetProcAddress(module, name.as_ptr() as *const u8).map(|p| p as usize)
    };
    // Safety: opengl32's exports, stdcall, with these signatures.
    unsafe {
        Some(Gl {
            push_attrib: std::mem::transmute::<usize, EnumFn>(proc(c"glPushAttrib")?),
            pop_attrib: std::mem::transmute::<usize, NoArgFn>(proc(c"glPopAttrib")?),
            enable: std::mem::transmute::<usize, EnumFn>(proc(c"glEnable")?),
            disable: std::mem::transmute::<usize, EnumFn>(proc(c"glDisable")?),
            blend_func: std::mem::transmute::<usize, TwoEnumsFn>(proc(c"glBlendFunc")?),
            color4f: std::mem::transmute::<usize, Color4fFn>(proc(c"glColor4f")?),
            begin: std::mem::transmute::<usize, EnumFn>(proc(c"glBegin")?),
            end: std::mem::transmute::<usize, NoArgFn>(proc(c"glEnd")?),
            vertex2f: std::mem::transmute::<usize, Vertex2fFn>(proc(c"glVertex2f")?),
        })
    }
}

#[cfg(not(target_arch = "x86"))]
fn load_gl() -> Option<Gl> {
    None
}

fn quads(gl: &Gl, rects: &[Rect], (r, g, b, a): (f32, f32, f32, f32)) {
    // Safety: the engine's GL context, on its thread, inside its 2D pass.
    unsafe {
        (gl.color4f)(r, g, b, a);
        (gl.begin)(GL_QUADS);
        for &Rect(x, y, w, h) in rects {
            (gl.vertex2f)(x, y);
            (gl.vertex2f)(x + w, y);
            (gl.vertex2f)(x + w, y + h);
            (gl.vertex2f)(x, y + h);
        }
        (gl.end)();
    }
}

/// Called by `engine.rs` after the client's own `HUD_Redraw`.
pub fn after_redraw() {
    if !on() {
        return;
    }
    let Some(health) = health() else { return };
    let Some(gl) = GL.get_or_init(load_gl).as_ref() else {
        return;
    };
    let Some(funcs) = engine::engfuncs() else {
        return;
    };
    let mut info: ScreenInfo = unsafe { std::mem::zeroed() };
    info.size = size_of::<ScreenInfo>() as i32;
    // Safety: the engine's own call, with the size it expects.
    unsafe { (funcs.pfn_get_screen_info)(&mut info) };
    if info.height <= 0 {
        return;
    }
    let health = health.clamp(0, 999);
    let height = info.height as f32;
    let size = (height * 0.13).round();
    let margin = (height * 0.03).round();
    let (cx, cy) = (margin + size / 2.0, height - margin - size / 2.0);

    let outline = plus_bars(cx, cy, size + size * 0.08);
    let bars = plus_bars(cx, cy, size);
    let cut = cy + size / 2.0 - size * (health.min(100) as f32 / 100.0);
    let filled: Vec<Rect> = bars.iter().filter_map(|&bar| below(bar, cut)).collect();
    let (r, g, b) = fill_colour(health);

    let text = health.to_string();
    let digit_h = (size * 0.36).round();
    let gap = digit_h * 0.18;
    let text_w = text.len() as f32 * digit_h * 0.5 + (text.len() as f32 - 1.0) * gap;
    let mut digits = Vec::new();
    let mut shadow = Vec::new();
    let (mut x, y) = (cx - text_w / 2.0, cy - digit_h / 2.0);
    let drop = (digit_h * 0.06).max(1.0);
    for ch in text.chars() {
        let d = ch.to_digit(10).unwrap_or(0);
        digits.extend(digit_rects(d, x, y, digit_h));
        shadow.extend(digit_rects(d, x + drop, y + drop, digit_h));
        x += digit_h * 0.5 + gap;
    }

    // Safety: as in `quads`.
    unsafe {
        (gl.push_attrib)(GL_ENABLE_BIT | GL_COLOR_BUFFER_BIT | GL_CURRENT_BIT);
        (gl.disable)(GL_TEXTURE_2D);
        (gl.disable)(GL_ALPHA_TEST);
        (gl.disable)(GL_DEPTH_TEST);
        (gl.enable)(GL_BLEND);
        (gl.blend_func)(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA);
    }
    quads(gl, &outline, (0.08, 0.07, 0.06, 0.75));
    quads(gl, &bars, (0.25, 0.23, 0.21, 0.85));
    quads(gl, &filled, (r, g, b, 1.0));
    quads(gl, &shadow, (0.0, 0.0, 0.0, 0.6));
    quads(gl, &digits, (1.0, 1.0, 1.0, 1.0));
    unsafe { (gl.pop_attrib)() };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plus_is_two_centred_bars_a_third_wide() {
        let [h, v] = plus_bars(50.0, 50.0, 30.0);
        assert_eq!(h, Rect(35.0, 45.0, 30.0, 10.0));
        assert_eq!(v, Rect(45.0, 35.0, 10.0, 30.0));
    }

    #[test]
    fn below_keeps_only_the_part_under_the_cut() {
        assert_eq!(
            below(Rect(0.0, 0.0, 10.0, 10.0), 4.0),
            Some(Rect(0.0, 4.0, 10.0, 6.0))
        );
        assert_eq!(
            below(Rect(0.0, 0.0, 10.0, 10.0), -5.0),
            Some(Rect(0.0, 0.0, 10.0, 10.0))
        );
        assert_eq!(below(Rect(0.0, 0.0, 10.0, 10.0), 10.0), None);
    }

    #[test]
    fn digits_light_the_right_segment_counts() {
        let counts: Vec<usize> = (0..10)
            .map(|d| digit_rects(d, 0.0, 0.0, 10.0).len())
            .collect();
        assert_eq!(counts, vec![6, 2, 5, 5, 4, 5, 6, 3, 7, 6]);
    }

    #[test]
    fn colour_steps_down_with_health() {
        assert_eq!(fill_colour(100), (0.93, 0.89, 0.80));
        assert_eq!(fill_colour(40), (1.0, 0.70, 0.24));
        assert_eq!(fill_colour(10), (0.86, 0.20, 0.16));
    }
}
