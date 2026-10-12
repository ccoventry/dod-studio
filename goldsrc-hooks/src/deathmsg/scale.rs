//! `dodstudio_deathmsg scale <S>`: the kill feed's weapon icons S times
//! bigger, with the lines spaced and right-anchored to match (#728).
//!
//! ## What the game does
//!
//! Every number in a kill-feed line comes out of `CHudDeathNotice::Draw`
//! (`client+0x2ae90`) from two sources: the engine's text calls (`font`'s
//! business) and one `wrect_t` -- the icon's entry in `gHUD.m_rgrcRects`,
//! `{left, right, top, bottom}` per sprite id, 16 bytes each, allocated once
//! by `CHud::VidInit` (`m_rgrcRects = new wrect_t[m_iSpriteCount]`,
//! `client+0x21c9c`). `Draw` reads that array exactly twice, both
//! `mov eax, [gHUD+0x18c]` (`+0x2b017` and `+0x2b097`): once for the anchor
//! `x = ScreenWidth - (right - left) - victim width [- 5 - killer width]`,
//! once to copy the rect onto its stack, which it then hands to
//! `pfnSPR_DrawHoles(0, x, y, &rect)` (`call [gEngfuncs+0x18]`, slot 6, at
//! `+0x2b0f1`), and reads back for the x advance (`right - left`) and the
//! line step (`bottom - top`; DoD's `d_*` icons are 20..58 px tall, so the
//! step already varies per line). Nothing in the engine or either client
//! scales a HUD sprite: `pfnSPR_Draw*` draws exactly `rect` pixels, with
//! texture coordinates `(rect +- 0.5) / the frame's logical size`.
//!
//! ## How
//!
//! Three operands in `Draw` change, each to something the same length, the
//! way `max` relocates the array and `font` redirects the text calls:
//!
//! 1. Both `mov eax, [gHUD.m_rgrcRects]` become `mov eax, [RECTS]`, a pointer
//!    this module owns. At S = 1 it is the game's own array, re-read every
//!    frame. At S != 1 it is a copy with `right = left + round(S * width)`
//!    and `bottom = top + round(S * height)`: the anchor, the advance and the
//!    line step then come out S times bigger with no further help, and
//!    `font`'s step detour makes the step `max(S * icon height, font height)`.
//! 2. The `SPR_DrawHoles` call becomes `call rel32; nop` to [`hook::draw_holes`].
//!    At S = 1 it forwards to whatever the table held when it was redirected
//!    (the engine's, or HLAE's wrapper if HLAE hooked the slot). At S != 1 it
//!    finds the unscaled rect the scaled one was made from, drops the icon by
//!    half the difference when the font is taller than the scaled icon, and
//!    calls that same function with the *unscaled* rect under a modelview of
//!    `translate(x+.5, y+.5) * scale(S, S) * translate(-(x+.5), -(y+.5))`.
//!    The engine then draws its own quad -- its own texture coordinates, its
//!    alpha test, `gl_spriteblend`, the scissor, `GL_Bind`'s cache -- S times
//!    bigger about the icon's corner. `glTranslatef`/`glScalef` post-multiply
//!    the current matrix, so this is the engine's pixel space whatever matrix
//!    the HUD pass has current; the push/pop leave nothing behind.
//!
//! A vftable wrapper around `Draw` with a per-call swap of the table slot
//! would do the same, but `hudelement` owns that vftable slot (it refuses a
//! value that is neither the stock `Draw` nor `CHudBase::Draw`, and writes
//! both), and a swap needs a restore on every exit. Operand rewrites need
//! neither, and they compose with `max`, `offset` and `font`, which all leave
//! these three operands alone.
//!
//! Installed on the first `scale` other than 1, never undone (`scale 1` keeps
//! the game's pointer in `RECTS` and forwards the call), re-checked every
//! frame like `font`, re-applied should `client.dll` be loaded afresh. At S =
//! 1 a frame costs one pointer copy and a forwarded call per line.

use std::sync::atomic::{AtomicU32, Ordering};

use super::font;

/// `wrect_t`, as `hud.txt` and `CHud::m_rgrcRects` hold it: `left, right,
/// top, bottom`, in that order.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct WRect {
    pub left: i32,
    pub right: i32,
    pub top: i32,
    pub bottom: i32,
}

/// The scale range. 1 is the game's own; 8 makes a 58 px icon 464 px tall,
/// which is already most of a 1080p screen for four lines. Below 1 would
/// shrink a texture that has no mipmaps (shimmer), so it is not offered.
pub(super) const MIN_SCALE: f32 = 1.0;
pub(super) const MAX_SCALE: f32 = 8.0;

/// `m_rgrcRects` and `m_iSpriteCount` inside `CHud`: `VidInit` allocates
/// `[this+0x18c] = new[16 * [this+0x10]]` (`client+0x21c9c..+0x21cae`).
const RECTS_IN_HUD: usize = 0x18c;
const SPRITE_COUNT_IN_HUD: usize = 0x10;

/// The two `mov eax, [gHUD.m_rgrcRects]` in `Draw`, as offsets from
/// `font::PATTERN`'s match (`client+0x2b002`): `+0x2b017` and `+0x2b097`.
const RECTS_READS: &[usize] = &[0x15, 0x95];
/// `A1 imm32`.
const READ_LEN: usize = 5;
/// `mov edx, [gHUD.m_rghSprites]` (`8B 15 imm32`, `+0x2b0c5`): the handles
/// sit four bytes before the rects in `CHud`, which pins the field.
const SPRITES_READ: usize = 0xc3;
/// `call [gEngfuncs+0x18]`, `pfnSPR_DrawHoles` (`+0x2b0f1`).
const DRAW_HOLES_CALL: usize = 0xef;
const DRAW_HOLES_SLOT: usize = 6;
/// `call [gEngfuncs+0x10]`, `pfnSPR_Set` (`+0x2b0de`): left alone, read to
/// confirm the two calls agree on `gEngfuncs`.
const SPR_SET_CALL: usize = 0xdc;
const SPR_SET_SLOT: usize = 4;

/// A sprite count past this is not `CHud`'s: `hud.txt` has ~200 entries.
const MAX_SPRITES: usize = 4096;

// The sites in `Draw`'s order, and the count before the array in `CHud`.
const _: () = assert!(RECTS_READS[0] < RECTS_READS[1]);
const _: () = assert!(RECTS_READS[1] < SPRITES_READ);
const _: () = assert!(SPRITES_READ < SPR_SET_CALL);
const _: () = assert!(SPR_SET_CALL < DRAW_HOLES_CALL);
const _: () = assert!(SPRITE_COUNT_IN_HUD < RECTS_IN_HUD);

/// The scale in force, as `f32` bits. Read on the game thread per icon.
static SCALE_BITS: AtomicU32 = AtomicU32::new(1.0f32.to_bits());

/// The scale in force.
pub(super) fn scale() -> f32 {
    f32::from_bits(SCALE_BITS.load(Ordering::Relaxed))
}

/// Whether `s` means "the game's own": exactly 1.
fn is_unity(s: f32) -> bool {
    s == 1.0
}

/// `S * len`, rounded to whole pixels, never below 1 for a visible rect.
fn scaled_len(len: i32, s: f32) -> i32 {
    if len <= 0 {
        return len;
    }
    ((len as f32) * s).round().max(1.0) as i32
}

/// A rect S times bigger, growing right and down from its own corner, so
/// `left`/`top` -- the texture position -- stay the game's.
pub(super) fn scale_rect(r: WRect, s: f32) -> WRect {
    WRect {
        left: r.left,
        right: r.left + scaled_len(r.right - r.left, s),
        top: r.top,
        bottom: r.top + scaled_len(r.bottom - r.top, s),
    }
}

/// The rect a scaled one was made from, when no table lookup is possible:
/// the inverse of [`scale_rect`] up to its rounding.
fn unscale_rect(r: WRect, s: f32) -> WRect {
    let inverse = 1.0 / s;
    WRect {
        left: r.left,
        right: r.left + scaled_len(r.right - r.left, inverse),
        top: r.top,
        bottom: r.top + scaled_len(r.bottom - r.top, inverse),
    }
}

/// The unscaled rect behind `scaled`: the stock entry at the index where the
/// copy holds exactly `scaled`. Two sprites with the same rect (DoD has
/// `d_mortar` and `d_germortar`) have the same stock rect, so the first hit
/// is right.
fn find_unscaled(scaled: WRect, copy: &[WRect], stock: &[WRect]) -> Option<WRect> {
    copy.iter()
        .zip(stock)
        .find(|(c, _)| **c == scaled)
        .map(|(_, s)| *s)
}

/// The line step `font`'s stub applies: the taller of the icon and the font.
pub(super) fn line_step(icon_height: i32, font_tall: i32) -> i32 {
    icon_height.max(font_tall)
}

/// Where the icon's top goes on a line that starts at `y`: centred on the
/// line when the font is taller than the icon, else at the top as the game
/// draws it.
pub(super) fn icon_y(y: i32, icon_height: i32, font_tall: i32) -> i32 {
    y + (line_step(icon_height, font_tall) - icon_height) / 2
}

/// The screen span the engine's quad covers once scaled about `(x + 0.5)`:
/// `(x + 0.5, x + 0.5 + S * width)`, which is what the scaled rect's
/// `right - left` advance expects up to the rounding in [`scaled_len`]. The
/// arithmetic `hook::draw_holes`'s matrix performs, spelled out for the test.
#[cfg(test)]
fn quad_extent(x: i32, width: i32, s: f32) -> (f32, f32) {
    let start = x as f32 + 0.5;
    (start, start + s * width as f32)
}

/// A scale as the console shows it: `2`, `1.5`, never `2.0`.
pub(super) fn format_scale(s: f32) -> String {
    if s.fract() == 0.0 {
        format!("{}", s as i32)
    } else {
        format!("{s}")
    }
}

/// The argument after `scale`: a number in [`MIN_SCALE`]`..=`[`MAX_SCALE`],
/// or `default` (1).
pub(super) fn parse_args(rest: &[String]) -> Result<f32, String> {
    let [arg] = rest else {
        return Err(format!(
            "need one number, {MIN_SCALE}..={MAX_SCALE} (1 is the game's own), or default"
        ));
    };
    if arg.eq_ignore_ascii_case("default") {
        return Ok(1.0);
    }
    let s: f32 = arg
        .parse()
        .map_err(|_| format!("{arg:?} is not a number"))?;
    if !s.is_finite() || !(MIN_SCALE..=MAX_SCALE).contains(&s) {
        return Err(format!("expected {MIN_SCALE}..={MAX_SCALE}, got {arg}"));
    }
    Ok(s)
}

/// What `status` says about the scale.
pub(super) fn describe() -> String {
    let s = scale();
    if is_unity(s) {
        "scale = 1".to_string()
    } else {
        format!("scale = {}", format_scale(s))
    }
}

#[cfg(target_arch = "x86")]
mod hook {
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    use super::*;
    use crate::engine;

    /// `pfnSPR_DrawHoles(frame, x, y, prc)`.
    type DrawHolesFn = unsafe extern "C" fn(i32, i32, i32, *const WRect);

    /// What `Draw` reads where it read `gHUD.m_rgrcRects`: the game's array
    /// at S = 1, the scaled copy otherwise. Read by game code, so an address
    /// rather than a value: [`AtomicUsize::as_ptr`] goes into the operands.
    static RECTS: AtomicUsize = AtomicUsize::new(0);
    /// `&gHUD.m_rgrcRects`, from the stock operand, 0 until installed.
    static RECTS_FIELD: AtomicUsize = AtomicUsize::new(0);
    /// The game's array and count the copy was built from, and the scale.
    static SEEN_STOCK: AtomicUsize = AtomicUsize::new(0);
    static SEEN_COUNT: AtomicUsize = AtomicUsize::new(0);
    static SEEN_SCALE: AtomicU32 = AtomicU32::new(0);
    /// The scaled copy: its first rect, and how many it has room for. Never
    /// freed while a smaller one might still be `RECTS` (a count change
    /// leaks the old copy, as `max`'s buffer does; `VidInit` allocates once).
    static COPY: AtomicUsize = AtomicUsize::new(0);
    static COPY_CAP: AtomicUsize = AtomicUsize::new(0);
    /// `pfnSPR_DrawHoles` as the table held it when the call was redirected.
    static ORIGINAL_DRAW_HOLES: AtomicUsize = AtomicUsize::new(0);
    /// The `client.dll` the operands were rewritten in, 0 before.
    static INSTALLED_BASE: AtomicUsize = AtomicUsize::new(0);
    /// Whether a failure has been logged, so the per-frame check says it once.
    static COMPLAINED: AtomicBool = AtomicBool::new(false);
    /// Whether a bad sprite count has been logged.
    static COUNT_COMPLAINED: AtomicBool = AtomicBool::new(false);

    const GL_MATRIX_MODE: u32 = 0x0ba0;
    const GL_MODELVIEW: u32 = 0x1700;

    /// The matrix calls, from `opengl32.dll`: the engine's own GL, loaded
    /// before anything draws. Not the engine's `qgl*` slots (per-build
    /// addresses) and not TriAPI (no matrix calls).
    struct Gl {
        get_integerv: GetIntegervFn,
        matrix_mode: MatrixModeFn,
        push_matrix: NoArgFn,
        pop_matrix: NoArgFn,
        translatef: ThreeFloatsFn,
        scalef: ThreeFloatsFn,
    }

    type GetIntegervFn = unsafe extern "system" fn(u32, *mut i32);
    type MatrixModeFn = unsafe extern "system" fn(u32);
    type NoArgFn = unsafe extern "system" fn();
    type ThreeFloatsFn = unsafe extern "system" fn(f32, f32, f32);

    static GL: OnceLock<Gl> = OnceLock::new();

    fn load_gl() -> Result<Gl, String> {
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
                get_integerv: std::mem::transmute::<usize, GetIntegervFn>(proc(c"glGetIntegerv")?),
                matrix_mode: std::mem::transmute::<usize, MatrixModeFn>(proc(c"glMatrixMode")?),
                push_matrix: std::mem::transmute::<usize, NoArgFn>(proc(c"glPushMatrix")?),
                pop_matrix: std::mem::transmute::<usize, NoArgFn>(proc(c"glPopMatrix")?),
                translatef: std::mem::transmute::<usize, ThreeFloatsFn>(proc(c"glTranslatef")?),
                scalef: std::mem::transmute::<usize, ThreeFloatsFn>(proc(c"glScalef")?),
            })
        }
    }

    /// The GL calls, loaded on first use; a failure is not cached, so a
    /// later try can succeed.
    fn gl() -> Result<&'static Gl, String> {
        if let Some(gl) = GL.get() {
            return Ok(gl);
        }
        let loaded = load_gl()?;
        let _ = GL.set(loaded);
        GL.get().ok_or_else(|| "GL functions not kept".to_string())
    }

    /// `mov eax, [imm32]` reading `at`.
    fn read_bytes(at: usize) -> [u8; READ_LEN] {
        let abs = (at as u32).to_le_bytes();
        [0xa1, abs[0], abs[1], abs[2], abs[3]]
    }

    /// The operand of a `mov eax, [imm32]` (`A1`), or `None` for any other
    /// instruction.
    fn read_operand(bytes: &[u8; READ_LEN]) -> Option<usize> {
        (bytes[0] == 0xa1)
            .then(|| u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize)
    }

    /// The operand of a `mov edx, [imm32]` (`8B 15`), or `None`.
    fn read_edx_operand(bytes: &[u8; font::CALL_LEN]) -> Option<usize> {
        (bytes[..2] == [0x8b, 0x15])
            .then(|| u32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]) as usize)
    }

    unsafe fn bytes<const N: usize>(address: usize) -> [u8; N] {
        let mut out = [0u8; N];
        // Safety: inside the matched span of a mapped module.
        out.copy_from_slice(unsafe { std::slice::from_raw_parts(address as *const u8, N) });
        out
    }

    /// The game's array and its count, as `CHud` holds them now.
    fn stock() -> Option<(usize, usize)> {
        let field = RECTS_FIELD.load(Ordering::Acquire);
        if field == 0 {
            return None;
        }
        // Safety: `field` is `&gHUD.m_rgrcRects`, read out of `Draw`'s own
        // operand; the count is 0x17c bytes before it in the same object.
        let array = unsafe { *(field as *const usize) };
        let count = unsafe { *((field - RECTS_IN_HUD + SPRITE_COUNT_IN_HUD) as *const i32) };
        Some((array, count.max(0) as usize))
    }

    /// Points `RECTS` at what `Draw` should read for the scale in force,
    /// rebuilding the copy when the game's array, its count or the scale
    /// changed. Cheap when nothing did: three loads and compares.
    pub(super) fn sync_rects() {
        let Some((array, count)) = stock() else {
            return;
        };
        let s = scale();
        if is_unity(s) || array == 0 || count == 0 {
            RECTS.store(array, Ordering::Release);
            return;
        }
        if count > MAX_SPRITES {
            if !COUNT_COMPLAINED.swap(true, Ordering::Relaxed) {
                unsafe {
                    crate::debug::report(&format!(
                        "deathmsg scale: gHUD.m_iSpriteCount is {count}, not a sprite count -- drawing the game's rects"
                    ))
                };
            }
            RECTS.store(array, Ordering::Release);
            return;
        }
        let unchanged = SEEN_STOCK.load(Ordering::Relaxed) == array
            && SEEN_COUNT.load(Ordering::Relaxed) == count
            && SEEN_SCALE.load(Ordering::Relaxed) == s.to_bits()
            && COPY.load(Ordering::Relaxed) != 0;
        if !unchanged {
            rebuild(array, count, s);
        }
        RECTS.store(COPY.load(Ordering::Acquire), Ordering::Release);
    }

    /// Fills the copy with the game's rects scaled by `s`. The same thread
    /// that runs `Draw` (the engine's) calls this, from the console command
    /// or the per-frame poll, so no draw is reading the copy meanwhile.
    fn rebuild(array: usize, count: usize, s: f32) {
        if COPY_CAP.load(Ordering::Relaxed) < count || COPY.load(Ordering::Relaxed) == 0 {
            let block = vec![WRect::default(); count].into_boxed_slice();
            let ptr = Box::leak(block).as_mut_ptr() as usize;
            COPY.store(ptr, Ordering::Release);
            COPY_CAP.store(count, Ordering::Release);
        }
        // Safety: `array` is `CHud`'s live `wrect_t[count]`; the copy has
        // room for `count`.
        let stock = unsafe { std::slice::from_raw_parts(array as *const WRect, count) };
        let copy = unsafe {
            std::slice::from_raw_parts_mut(COPY.load(Ordering::Relaxed) as *mut WRect, count)
        };
        for (c, r) in copy.iter_mut().zip(stock) {
            *c = scale_rect(*r, s);
        }
        SEEN_STOCK.store(array, Ordering::Relaxed);
        SEEN_COUNT.store(count, Ordering::Relaxed);
        SEEN_SCALE.store(s.to_bits(), Ordering::Relaxed);
    }

    /// Stands in for `pfnSPR_DrawHoles` at the icon draw.
    pub(super) unsafe extern "C" fn draw_holes(frame: i32, x: i32, y: i32, prc: *const WRect) {
        let original = ORIGINAL_DRAW_HOLES.load(Ordering::Acquire);
        if original == 0 {
            return;
        }
        // Safety: the function the table held at install, the engine's own
        // signature.
        let original: DrawHolesFn = unsafe { std::mem::transmute(original) };
        let s = scale();
        if is_unity(s) || prc.is_null() {
            unsafe { original(frame, x, y, prc) };
            return;
        }
        // Safety: `Draw`'s stack copy of one of our rects.
        let scaled = unsafe { *prc };
        let unscaled = {
            let count = SEEN_COUNT.load(Ordering::Relaxed);
            let copy_ptr = COPY.load(Ordering::Acquire);
            let stock_ptr = SEEN_STOCK.load(Ordering::Relaxed);
            if copy_ptr != 0 && stock_ptr != 0 && count != 0 {
                // Safety: both arrays hold `count` rects (see `rebuild`).
                let copy = unsafe { std::slice::from_raw_parts(copy_ptr as *const WRect, count) };
                let stock = unsafe { std::slice::from_raw_parts(stock_ptr as *const WRect, count) };
                find_unscaled(scaled, copy, stock)
            } else {
                None
            }
        }
        .unwrap_or_else(|| unscale_rect(scaled, s));
        let y = icon_y(y, scaled.bottom - scaled.top, font::tall());
        let Ok(gl) = gl() else {
            // No matrix calls: the right pixels at the game's size beat the
            // wrong pixels at ours.
            unsafe { original(frame, x, y, &unscaled) };
            return;
        };
        let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
        // Safety: GL calls on the engine's thread with its context current,
        // balanced push/pop, the matrix mode put back if it was changed.
        unsafe {
            let mut mode: i32 = GL_MODELVIEW as i32;
            (gl.get_integerv)(GL_MATRIX_MODE, &mut mode);
            if mode != GL_MODELVIEW as i32 {
                (gl.matrix_mode)(GL_MODELVIEW);
            }
            (gl.push_matrix)();
            (gl.translatef)(cx, cy, 0.0);
            (gl.scalef)(s, s, 1.0);
            (gl.translatef)(-cx, -cy, 0.0);
            original(frame, x, y, &unscaled);
            (gl.pop_matrix)();
            if mode != GL_MODELVIEW as i32 {
                (gl.matrix_mode)(mode as u32);
            }
        }
    }

    /// Rewrites the three operands in the module at `base`, whatever of that
    /// is still to do. Returns whether anything was written. Reads before it
    /// writes, so the per-frame call is cheap once all is in place.
    fn install(base: usize) -> Result<bool, String> {
        let found = font::draw_site(base)?;

        // The two rect reads: stock (both naming one field) or already ours.
        let ours_read = read_bytes(RECTS.as_ptr() as usize);
        let mut field: Option<usize> = None;
        let mut pending_reads: Vec<usize> = Vec::new();
        for &offset in RECTS_READS {
            let address = found + offset;
            let present: [u8; READ_LEN] = unsafe { bytes(address) };
            if present == ours_read {
                continue;
            }
            let Some(operand) = read_operand(&present) else {
                return Err(format!(
                    "+{:#x} holds {present:02x?}, neither the game's rect read nor ours -- something else has patched it",
                    address - base
                ));
            };
            if field.is_some_and(|known| known != operand) {
                return Err(format!(
                    "the rect reads at +{:#x} do not read one field",
                    address - base
                ));
            }
            field = Some(operand);
            pending_reads.push(address);
        }
        if let Some(field) = field {
            // `m_rghSprites` is the dword before `m_rgrcRects` in `CHud`.
            let sprites: [u8; font::CALL_LEN] = unsafe { bytes(found + SPRITES_READ) };
            if read_edx_operand(&sprites) != Some(field - 4) {
                return Err(format!(
                    "the sprite-handle read at +{:#x} ({sprites:02x?}) is not four bytes before the rect field {field:#x}",
                    found + SPRITES_READ - base
                ));
            }
            RECTS_FIELD.store(field, Ordering::Release);
        } else if RECTS_FIELD.load(Ordering::Acquire) == 0 {
            return Err(
                "the rect reads are already redirected, but the game's field was never recorded"
                    .to_string(),
            );
        }

        // The icon draw: stock (agreeing with SPR_Set on gEngfuncs) or ours.
        let call = found + DRAW_HOLES_CALL;
        let ours_call = font::redirect_bytes(call, draw_holes as DrawHolesFn as usize);
        let present: [u8; font::CALL_LEN] = unsafe { bytes(call) };
        let pending_call = if present == ours_call {
            false
        } else {
            let Some(g) = font::gengfuncs_of(&present, DRAW_HOLES_SLOT) else {
                return Err(format!(
                    "+{:#x} holds {present:02x?}, neither the game's SPR_DrawHoles call nor ours -- something else has patched it",
                    call - base
                ));
            };
            let anchor: [u8; font::CALL_LEN] = unsafe { bytes(found + SPR_SET_CALL) };
            if font::gengfuncs_of(&anchor, SPR_SET_SLOT) != Some(g) {
                return Err(format!(
                    "the SPR_Set call at +{:#x} does not use the gEngfuncs the SPR_DrawHoles call does",
                    found + SPR_SET_CALL - base
                ));
            }
            // Safety: `g` is client.dll's `gEngfuncs`, a 135-slot table in .data.
            let original = unsafe { *((g + DRAW_HOLES_SLOT * 4) as *const usize) };
            if original == 0 {
                return Err("gEngfuncs' SPR_DrawHoles slot is empty".to_string());
            }
            ORIGINAL_DRAW_HOLES.store(original, Ordering::Release);
            true
        };

        // `RECTS` must hold the game's pointer before `Draw` reads it from us.
        sync_rects();

        let mut wrote = false;
        for address in pending_reads {
            // Safety: a stock `A1 imm32` in the matched span; the same five
            // bytes long, so no instruction boundary moves.
            if !unsafe { crate::patch::write_code_bytes(address, &ours_read) } {
                return Err(format!("could not make +{:#x} writable", address - base));
            }
            wrote = true;
        }
        if pending_call {
            // Safety: a stock `FF 15 imm32`, replaced by six bytes.
            if !unsafe { crate::patch::write_code_bytes(call, &ours_call) } {
                return Err(format!("could not make +{:#x} writable", call - base));
            }
            wrote = true;
        }
        if INSTALLED_BASE.swap(base, Ordering::AcqRel) != base {
            unsafe {
                crate::debug::report(&format!(
                    "deathmsg scale: the kill feed's two rect reads and its SPR_DrawHoles call at client+{:#x}.. now go through the hook (rects at {:#x}, SPR_DrawHoles was {:#x})",
                    found - base,
                    RECTS_FIELD.load(Ordering::Relaxed),
                    ORIGINAL_DRAW_HOLES.load(Ordering::Relaxed)
                ))
            };
        }
        Ok(wrote)
    }

    /// Makes `s` the scale. The reply is what the console prints.
    pub(super) fn apply(s: f32) -> Result<String, String> {
        if is_unity(s) && INSTALLED_BASE.load(Ordering::Acquire) == 0 {
            SCALE_BITS.store(s.to_bits(), Ordering::Release);
            return Ok("scale = 1, the game's own".to_string());
        }
        let base = engine::client_module_base().ok_or("client.dll is not loaded yet")?;
        install(base)?;
        if !is_unity(s) {
            gl().map_err(|why| format!("cannot scale the icons: {why}"))?;
        }
        SCALE_BITS.store(s.to_bits(), Ordering::Release);
        sync_rects();
        if is_unity(s) {
            return Ok("scale = 1, the game's own".to_string());
        }
        Ok(format!(
            "scale = {}: icons {} times their size, lines spaced to match, from the next frame. `scale 1` puts the game's back.",
            format_scale(s),
            format_scale(s)
        ))
    }

    /// Keeps the operands ours and the rect pointer current, once the module
    /// ever installed. Called every frame.
    pub(super) fn poll() {
        if INSTALLED_BASE.load(Ordering::Relaxed) == 0 {
            return;
        }
        let Some(base) = engine::client_module_base() else {
            return;
        };
        match install(base) {
            Ok(_) => {
                COMPLAINED.store(false, Ordering::Relaxed);
                sync_rects();
            }
            Err(why) => {
                if !COMPLAINED.swap(true, Ordering::Relaxed) {
                    unsafe {
                        crate::debug::report(&format!("deathmsg scale: not in place -- {why}"))
                    };
                }
            }
        }
    }
}

/// Makes `s` the kill feed's scale (1 is the game's own). The reply is what
/// the console prints.
pub(super) fn apply(s: f32) -> Result<String, String> {
    #[cfg(target_arch = "x86")]
    {
        hook::apply(s)
    }
    #[cfg(not(target_arch = "x86"))]
    {
        let _ = s;
        Err("only in the 32-bit game".to_string())
    }
}

/// Re-checks the rewritten operands every frame, from `deathmsg::poll`.
pub(super) fn poll() {
    #[cfg(target_arch = "x86")]
    hook::poll();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    /// `d_skull` and `d_garand` from DoD's hud.txt (640 set).
    const SKULL: WRect = WRect {
        left: 328,
        right: 328 + 74,
        top: 326,
        bottom: 326 + 52,
    };
    const GARAND: WRect = WRect {
        left: 27,
        right: 27 + 201,
        top: 108,
        bottom: 108 + 34,
    };

    #[test]
    fn a_scaled_rect_keeps_its_corner_and_grows_right_and_down() {
        assert_eq!(scale_rect(SKULL, 1.0), SKULL);
        let twice = scale_rect(SKULL, 2.0);
        assert_eq!(
            twice,
            WRect {
                left: 328,
                right: 328 + 148,
                top: 326,
                bottom: 326 + 104
            }
        );
        // 1.5 x 201 = 301.5 rounds up; 1.5 x 34 = 51 exactly.
        let garand = scale_rect(GARAND, 1.5);
        assert_eq!(garand.right - garand.left, 302);
        assert_eq!(garand.bottom - garand.top, 51);
        // An empty rect stays empty rather than becoming one pixel.
        let empty = WRect::default();
        assert_eq!(scale_rect(empty, 3.0), empty);
    }

    #[test]
    fn the_layout_in_the_wrect_order_the_game_uses() {
        // `left, right, top, bottom`: the anchor reads +4 (right) then +0
        // (left), the step +0xc (bottom) minus +8 (top).
        assert_eq!(std::mem::offset_of!(WRect, left), 0);
        assert_eq!(std::mem::offset_of!(WRect, right), 4);
        assert_eq!(std::mem::offset_of!(WRect, top), 8);
        assert_eq!(std::mem::offset_of!(WRect, bottom), 12);
        assert_eq!(size_of::<WRect>(), 16, "VidInit's shl 4");
    }

    #[test]
    fn the_unscaled_rect_comes_from_the_table_and_the_division_agrees() {
        let stock = [SKULL, GARAND, SKULL];
        for s in [1.5, 2.0, 3.0] {
            let copy: Vec<WRect> = stock.iter().map(|r| scale_rect(*r, s)).collect();
            for (i, r) in stock.iter().enumerate() {
                assert_eq!(find_unscaled(copy[i], &copy, &stock), Some(*r));
                assert_eq!(unscale_rect(copy[i], s), *r, "scale {s}, rect {i}");
            }
        }
        assert_eq!(
            find_unscaled(WRect::default(), &[scale_rect(SKULL, 2.0)], &[SKULL]),
            None
        );
    }

    #[test]
    fn the_quad_lands_where_the_scaled_rect_advances() {
        // The engine draws (x+0.5)..(x+0.5+w); under the matrix it covers
        // S x that, which is the scaled rect's width up to rounding.
        for s in [1.0f32, 1.5, 2.0, 3.0] {
            let scaled = scale_rect(GARAND, s);
            let (start, end) = quad_extent(100, GARAND.right - GARAND.left, s);
            assert_eq!(start, 100.5);
            let advance = (scaled.right - scaled.left) as f32;
            assert!(
                (end - start - advance).abs() <= 0.5,
                "scale {s}: quad {} px, advance {advance}",
                end - start
            );
        }
    }

    #[test]
    fn the_step_is_the_taller_and_the_icon_is_centred_only_under_a_taller_font() {
        assert_eq!(line_step(104, 0), 104, "no font: the icon's own height");
        assert_eq!(line_step(104, 28), 104);
        assert_eq!(line_step(40, 60), 60);
        assert_eq!(
            icon_y(20, 104, 28),
            20,
            "icon taller: at the top, as the game"
        );
        assert_eq!(icon_y(20, 40, 60), 30, "font taller: centred");
        assert_eq!(icon_y(20, 41, 60), 29, "odd gap rounds toward the top");
        assert_eq!(icon_y(20, 60, 60), 20);
    }

    #[test]
    fn scale_arguments_parse_as_typed() {
        assert_eq!(parse_args(&argv(&["2"])), Ok(2.0));
        assert_eq!(parse_args(&argv(&["1.5"])), Ok(1.5));
        assert_eq!(parse_args(&argv(&["1"])), Ok(1.0));
        assert_eq!(parse_args(&argv(&["default"])), Ok(1.0));
        assert_eq!(parse_args(&argv(&["DEFAULT"])), Ok(1.0));
        assert_eq!(parse_args(&argv(&["8"])), Ok(8.0));
        assert!(parse_args(&argv(&["0.5"])).is_err(), "below 1");
        assert!(parse_args(&argv(&["9"])).is_err(), "above 8");
        assert!(parse_args(&argv(&["nan"])).is_err());
        assert!(parse_args(&argv(&["inf"])).is_err());
        assert!(parse_args(&argv(&["big"])).is_err());
        assert!(parse_args(&argv(&[])).is_err());
        assert!(parse_args(&argv(&["2", "3"])).is_err());
    }

    #[test]
    fn scales_print_without_a_trailing_zero() {
        assert_eq!(format_scale(2.0), "2");
        assert_eq!(format_scale(1.5), "1.5");
        assert_eq!(format_scale(1.0), "1");
        assert_eq!(describe(), "scale = 1");
    }

    #[test]
    fn the_sites_lie_past_the_pattern_inside_draw() {
        // The pattern (font.rs) covers `Draw` from `+0x2b002`; the first rect
        // read is inside it, right after the first text call.
        assert_eq!(
            RECTS_READS[0],
            0x0b + font::CALL_LEN + 4,
            "mov ebx, [esp+0x28] between"
        );
        assert_eq!(
            DRAW_HOLES_CALL,
            SPR_SET_CALL + 0x13,
            "SPR_Set, four pushes, DrawHoles"
        );
        assert_eq!(
            RECTS_IN_HUD - 4,
            0x188,
            "m_rghSprites just before m_rgrcRects"
        );
        assert_eq!(DRAW_HOLES_SLOT, 6);
        assert_eq!(SPR_SET_SLOT, 4);
    }
}
