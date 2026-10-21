//! `ds_hud_objectives scale <S>`: the objective icons, their capture bars and
//! player counts, and the objective timer drawn S times bigger (#891), the
//! way `ds_hud_deathmsg scale` does the kill feed (`deathmsg/scale.rs`).
//!
//! ## What the game draws, and from what
//!
//! `CObjectiveIcons::Draw` (`client+0x30030`) draws two elements in the top
//! left, each at its rects' exact pixel size (nothing in the engine scales a
//! HUD sprite):
//!
//! - **The timer** (`cl_hud_objtimer`): its background sprite with
//!   `pfnSPR_DrawHoles(0, 0, y, [this+0x28])` (`call [gEngfuncs+0x18]` at
//!   `+0x30168`), then four digits through the helper at `+0x30620`
//!   (`thiscall (digit, x, y)`, `ret 0xc`; `SPR_Set` + `SPR_DrawHoles` of the
//!   digit's own rect), called at `+0x301ab`, `+0x301b6`, `+0x301cd` and
//!   `+0x301d8` with the literal x 10, 24, 44, 58 and `y + 6`.
//! - **The icon row** (`cl_hud_objectives`): per objective, a capture-progress
//!   fill (the capturing team's flag, its rect cut to the progress, drawn at
//!   `(x, y - 1)`, `+0x3044c`), the flag itself (`(x + fill, y - 1)`,
//!   `+0x30491`), and while players stand in its area a `"%d/%d"` count with
//!   `CHud::DrawHudString(x + 4, y + icon height + 4, 1000, text, r, g, b)`
//!   (`thiscall`, `ret 0x1c`, `+0x37160`, called at `+0x30517`; it draws each
//!   character with `pfnDrawCharacter`). Then `x += [this+0x18] - [this+0x14]
//!   + 2`, a fixed per-icon step.
//!
//! Every size there is either a rect (the sprite's own) or a literal in the
//! instruction stream (the digit x, the 6, the 4, the 2), which is too many
//! constants to rewrite one by one. Instead every draw call goes through a
//! modelview zoom about its element's top-left corner (`crate::gl_zoom`):
//! the game lays everything out at its own size, and the zoom multiplies
//! every offset from the corner (the digits' x, the icon step, the count's
//! place under its icon) and every sprite and glyph by S together. Scale and
//! layout cannot disagree because there is only one number.
//!
//! ## The corners
//!
//! - The timer's: where its background is drawn, `(0, y)`, read from that
//!   draw (it is the first of the five). Its y is the game's, `timer`'s or
//!   the spectator layout's: the zoom keeps it, so `timer <y>` still says
//!   where the top goes.
//! - The row's: the row stub in `objicons.rs` (the convergence point at
//!   `+0x30289`, after `offset`/`xoffset`) records the row's x and y and the
//!   `spec_pip` flag every frame. The y is the anchor as it is. The x is too
//!   when `xoffset` set it or `spec_pip` placed it beside its picture; the
//!   game's own x (82 right of the timer, 10 without one) is a layout
//!   constant like the timer's digits, so it scales from the screen's left
//!   edge instead (anchor x 0), which keeps the row clear of a timer scaled
//!   by the same S.
//!
//! With `_cl_minimap 1` the loop draws each icon *on the overview map*, at
//! the objective's map position; those draws stay the game's. The mode is
//! `CHud::OverviewMapMode` (`+0x228e0`, `thiscall`), which `Draw` calls once
//! at `+0x3029a` before the loop; that call goes through [`hook::map_mode`],
//! which keeps the result.
//!
//! ## The patch
//!
//! Ten call operands in `Draw`, each to something the same length: the three
//! `call [gEngfuncs+0x18]` become `call rel32; nop` to our `SPR_DrawHoles`
//! stand-ins, and the seven `call rel32` (four digits, the map mode, the
//! count) point at our `thiscall` stand-ins. Each forwards to exactly what
//! the game would have called -- the `SPR_DrawHoles` slot is read from
//! `gEngfuncs` at every call, so `hud_sprites`' slot hook and HLAE's still
//! see every draw -- with the same arguments. At S = 1 that is all they do:
//! today's drawing, plus one indirect call per draw. The reinforcement timer
//! at the bottom of the screen (`+0x305d2`.., the same digit helper) is a
//! different element and keeps its own calls.
//!
//! Installed on the first `scale` other than 1, never undone (`scale 1`
//! leaves the calls forwarding), re-applied should `client.dll` be loaded
//! afresh. Sites and their offsets from `objicons.rs`'s two patterns are
//! checked against a real `client.dll` by
//! `tools/verify_objicons_scale_offsets.py`.
//!
//! ## Not reached
//!
//! `[client+0x17dd2c]`, which `Draw` sets to the last icon's bottom
//! (`+0x304a8`) and the picture-in-picture layout reads to sit below the row
//! while `spec_pip` is on (`+0x30899`), stays the unscaled bottom.

// The patching runs only in the 32-bit DLL; a host build (workspace clippy,
// tests) keeps the layout helpers for the unit tests alone.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code, unused_imports))]

use std::sync::atomic::{AtomicU32, Ordering};

/// The subcommand's name.
pub(super) const SUBCOMMAND: &str = "scale";

/// The scale range, as the kill feed's: 1 is the game's own; below it a
/// texture with no mipmaps would shimmer.
pub(super) const MIN_SCALE: f32 = 1.0;
pub(super) const MAX_SCALE: f32 = 8.0;

// ── The sites, as offsets from objicons.rs' pattern matches ──────────────────

/// From `TIMER_PATTERN`'s match (`+0x30119`): the background's `SPR_Set`
/// (`call [gEngfuncs+0x10]`, `+0x30159`, read to confirm the table) and
/// `SPR_DrawHoles` (`+0x30168`).
const TIMER_SPR_SET: usize = 0x40;
const TIMER_BACKGROUND_DRAW: usize = 0x4f;
/// The four digit-helper calls (`+0x301ab`, `+0x301b6`, `+0x301cd`,
/// `+0x301d8`).
const TIMER_DIGIT_CALLS: [usize; 4] = [0x92, 0x9d, 0xb4, 0xbf];

/// From `ROW_PATTERN`'s match (`+0x30248`): the overview-map mode call
/// (`+0x3029a`), the capture fill's and the flag's `SPR_DrawHoles`
/// (`+0x3044c`, `+0x30491`), and the player count's `DrawHudString`
/// (`+0x30517`).
const ROW_MAP_MODE_CALL: usize = 0x52;
const ROW_FILL_DRAW: usize = 0x204;
const ROW_ICON_DRAW: usize = 0x249;
const ROW_TEXT_CALL: usize = 0x2cf;

/// `cl_enginefunc_t` slots: `pfnSPR_Set`, `pfnSPR_DrawHoles`.
const SPR_SET_SLOT: usize = 4;
const DRAW_HOLES_SLOT: usize = 6;

/// The first bytes of each `call rel32` target, so a site that calls
/// something else is refused: the digit helper (`push esi; push edi;
/// mov edi, [esp+0xc]; mov esi, ecx`), `CHud::OverviewMapMode` (`push esi;
/// mov esi, ecx; call [..]`) and `CHud::DrawHudString` (`push ebx; push ebp;
/// push esi; mov eax, [esp+0x10]; push edi`).
const DIGIT_PROLOGUE: &[u8] = &[0x56, 0x57, 0x8b, 0x7c, 0x24, 0x0c, 0x8b, 0xf1];
const MAP_MODE_PROLOGUE: &[u8] = &[0x56, 0x8b, 0xf1, 0xff, 0x15];
const TEXT_PROLOGUE: &[u8] = &[0x53, 0x55, 0x56, 0x8b, 0x44, 0x24, 0x10, 0x57];

/// `call rel32`, and `call [abs32]`.
const CALL_REL_LEN: usize = 5;
const CALL_ABS_LEN: usize = 6;

// The sites in `Draw`'s order, none overlapping the next.
const _: () = assert!(TIMER_SPR_SET + CALL_ABS_LEN <= TIMER_BACKGROUND_DRAW);
const _: () = assert!(TIMER_BACKGROUND_DRAW + CALL_ABS_LEN <= TIMER_DIGIT_CALLS[0]);
const _: () = assert!(TIMER_DIGIT_CALLS[0] + CALL_REL_LEN <= TIMER_DIGIT_CALLS[1]);
const _: () = assert!(TIMER_DIGIT_CALLS[1] + CALL_REL_LEN <= TIMER_DIGIT_CALLS[2]);
const _: () = assert!(TIMER_DIGIT_CALLS[2] + CALL_REL_LEN <= TIMER_DIGIT_CALLS[3]);
const _: () = assert!(ROW_MAP_MODE_CALL + CALL_REL_LEN <= ROW_FILL_DRAW);
const _: () = assert!(ROW_FILL_DRAW + CALL_ABS_LEN <= ROW_ICON_DRAW);
const _: () = assert!(ROW_ICON_DRAW + CALL_ABS_LEN <= ROW_TEXT_CALL);

/// The scale in force, as `f32` bits. Read on the game thread per draw.
static SCALE_BITS: AtomicU32 = AtomicU32::new(1.0f32.to_bits());

/// The scale in force.
fn scale() -> f32 {
    f32::from_bits(SCALE_BITS.load(Ordering::Relaxed))
}

/// Whether `s` means "the game's own": exactly 1.
fn is_unity(s: f32) -> bool {
    s == 1.0
}

/// `_cl_minimap 1`'s value of `CHud::OverviewMapMode`: the icons are drawn on
/// the full overview map, at map positions.
const MAP_FULL: i32 = 1;

/// The row's zoom anchor x: the row's own x when `xoffset` or `spec_pip`
/// placed it, else the screen's left edge, so the game's fixed 82 (or 10)
/// scales with the timer it sits beside.
fn row_anchor_x(row_x: i32, x_typed: bool, pip: bool) -> i32 {
    if x_typed || pip { row_x } else { 0 }
}

/// The anchor to zoom a row draw about, or `None` to draw it as the game
/// does (the full overview map has the icons at map positions).
fn row_anchor(map_mode: i32, row: (i32, i32), x_typed: bool, pip: bool) -> Option<(i32, i32)> {
    (map_mode != MAP_FULL).then(|| (row_anchor_x(row.0, x_typed, pip), row.1))
}

/// The zoom's fixed point for an anchor: its pixel's corner as the engine's
/// quads have it (`x + 0.5`; see `deathmsg/scale.rs`' `quad_extent`).
fn fixed_point(anchor: (i32, i32)) -> (f32, f32) {
    (anchor.0 as f32 + 0.5, anchor.1 as f32 + 0.5)
}

/// Where a draw the game makes at `p` lands under the zoom about `anchor`:
/// the arithmetic the matrix performs, spelled out for the tests.
#[cfg(test)]
fn zoomed(anchor: (i32, i32), s: f32, p: (i32, i32)) -> (f32, f32) {
    let (fx, fy) = fixed_point(anchor);
    (
        fx + s * (p.0 as f32 + 0.5 - fx),
        fy + s * (p.1 as f32 + 0.5 - fy),
    )
}

/// A scale as the console shows it: `2`, `1.5`, never `2.0`.
fn format_scale(s: f32) -> String {
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
    format!("scale = {}", format_scale(scale()))
}

/// The reply for a scale now in force.
fn applied(s: f32) -> String {
    if is_unity(s) {
        "scale = 1, the game's own".to_string()
    } else {
        format!(
            "scale = {}: the objective icons and the timer {} times their size, from the next frame. `scale 1` puts the game's back.",
            format_scale(s),
            format_scale(s)
        )
    }
}

#[cfg(target_arch = "x86")]
mod hook {
    use std::ffi::{c_char, c_void};
    use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

    use super::*;
    use crate::{engine, scan};

    /// `pfnSPR_DrawHoles(frame, x, y, prc)`.
    type DrawHolesFn = unsafe extern "C" fn(i32, i32, i32, *const c_void);
    /// The timer's digit helper: `(this, digit, x, y)`.
    type DigitFn = unsafe extern "thiscall" fn(*mut c_void, i32, i32, i32);
    /// `CHud::OverviewMapMode(this)`.
    type MapModeFn = unsafe extern "thiscall" fn(*mut c_void) -> i32;
    /// `CHud::DrawHudString(this, x, y, max x, text, r, g, b)`.
    type TextFn = unsafe extern "thiscall" fn(
        *mut c_void,
        i32,
        i32,
        i32,
        *const c_char,
        i32,
        i32,
        i32,
    ) -> i32;

    /// `gEngfuncs`, from the stock operands; 0 until installed.
    static GENGFUNCS: AtomicUsize = AtomicUsize::new(0);
    /// The `call rel32` targets, as the stock operands had them.
    static DIGIT: AtomicUsize = AtomicUsize::new(0);
    static MAP_MODE: AtomicUsize = AtomicUsize::new(0);
    static TEXT: AtomicUsize = AtomicUsize::new(0);
    /// The `client.dll` the calls were redirected in, 0 before.
    static INSTALLED_BASE: AtomicUsize = AtomicUsize::new(0);

    /// The timer's corner as this frame's background draw had it.
    static TIMER_X: AtomicI32 = AtomicI32::new(0);
    static TIMER_Y: AtomicI32 = AtomicI32::new(0);
    /// `CHud::OverviewMapMode` as this frame's `Draw` got it.
    static MAP_MODE_NOW: AtomicI32 = AtomicI32::new(0);

    /// The `SPR_DrawHoles` the game's `call [gEngfuncs+0x18]` would reach now.
    fn draw_holes_now() -> Option<DrawHolesFn> {
        let g = GENGFUNCS.load(Ordering::Acquire);
        if g == 0 {
            return None;
        }
        // Safety: client.dll's `gEngfuncs`, a 135-slot table in .data.
        let f = unsafe { *((g + DRAW_HOLES_SLOT * 4) as *const usize) };
        // Safety: the slot's own signature.
        (f != 0).then(|| unsafe { std::mem::transmute::<usize, DrawHolesFn>(f) })
    }

    /// Runs `draw` zoomed about `anchor` at the scale in force, or as it is
    /// at 1 (or with no GL calls to be had).
    fn draw_about(anchor: (i32, i32), draw: impl Fn()) {
        let s = scale();
        if is_unity(s) {
            draw();
            return;
        }
        let (fx, fy) = fixed_point(anchor);
        // Safety: inside `Draw`, on the engine's thread.
        if !unsafe { crate::gl_zoom::about(fx, fy, s, &draw) } {
            draw();
        }
    }

    /// The row's anchor now, or `None` on the full overview map.
    fn row_now() -> Option<(i32, i32)> {
        row_anchor(
            MAP_MODE_NOW.load(Ordering::Relaxed),
            (
                super::super::ROW_X_DRAWN.load(Ordering::Relaxed),
                super::super::ROW_Y_DRAWN.load(Ordering::Relaxed),
            ),
            super::super::X_ACTIVE.load(Ordering::Relaxed) != 0,
            super::super::ROW_PIP.load(Ordering::Relaxed) != 0,
        )
    }

    /// The timer's background: records its corner, then draws about it.
    unsafe extern "C" fn timer_background(frame: i32, x: i32, y: i32, prc: *const c_void) {
        let Some(original) = draw_holes_now() else {
            return;
        };
        TIMER_X.store(x, Ordering::Relaxed);
        TIMER_Y.store(y, Ordering::Relaxed);
        // Safety: the engine's call with the game's arguments.
        draw_about((x, y), || unsafe { original(frame, x, y, prc) });
    }

    /// One of the timer's four digits, about the timer's corner.
    unsafe extern "thiscall" fn timer_digit(this: *mut c_void, digit: i32, x: i32, y: i32) {
        let f = DIGIT.load(Ordering::Acquire);
        if f == 0 {
            return;
        }
        // Safety: the helper the stock call reached, its own signature.
        let original: DigitFn = unsafe { std::mem::transmute(f) };
        let anchor = (
            TIMER_X.load(Ordering::Relaxed),
            TIMER_Y.load(Ordering::Relaxed),
        );
        draw_about(anchor, || unsafe { original(this, digit, x, y) });
    }

    /// `CHud::OverviewMapMode`, keeping what it said for the row's draws.
    unsafe extern "thiscall" fn map_mode(this: *mut c_void) -> i32 {
        let f = MAP_MODE.load(Ordering::Acquire);
        if f == 0 {
            return 0;
        }
        // Safety: the function the stock call reached.
        let original: MapModeFn = unsafe { std::mem::transmute(f) };
        let mode = unsafe { original(this) };
        MAP_MODE_NOW.store(mode, Ordering::Relaxed);
        mode
    }

    /// A flag or its capture fill, about the row's corner.
    unsafe extern "C" fn row_draw_holes(frame: i32, x: i32, y: i32, prc: *const c_void) {
        let Some(original) = draw_holes_now() else {
            return;
        };
        match row_now() {
            Some(anchor) => draw_about(anchor, || unsafe { original(frame, x, y, prc) }),
            None => unsafe { original(frame, x, y, prc) },
        }
    }

    /// The `"%d/%d"` player count under a flag, about the row's corner.
    #[allow(clippy::too_many_arguments)]
    unsafe extern "thiscall" fn row_text(
        this: *mut c_void,
        x: i32,
        y: i32,
        max_x: i32,
        text: *const c_char,
        r: i32,
        g: i32,
        b: i32,
    ) -> i32 {
        let f = TEXT.load(Ordering::Acquire);
        if f == 0 {
            return x;
        }
        // Safety: the function the stock call reached.
        let original: TextFn = unsafe { std::mem::transmute(f) };
        let out = std::cell::Cell::new(x);
        let draw = || out.set(unsafe { original(this, x, y, max_x, text, r, g, b) });
        match row_now() {
            Some(anchor) => draw_about(anchor, draw),
            None => draw(),
        }
        out.get()
    }

    unsafe fn bytes<const N: usize>(address: usize) -> [u8; N] {
        let mut out = [0u8; N];
        // Safety: inside the matched span of a mapped module.
        out.copy_from_slice(unsafe { std::slice::from_raw_parts(address as *const u8, N) });
        out
    }

    /// `gEngfuncs` from a stock `call [gEngfuncs + slot*4]`.
    fn gengfuncs_of(bytes: &[u8; CALL_ABS_LEN], slot: usize) -> Option<usize> {
        if bytes[..2] != [0xff, 0x15] {
            return None;
        }
        let abs = u32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]) as usize;
        abs.checked_sub(slot * 4)
    }

    /// The target of a stock `call rel32` at `site`.
    fn rel_target(site: usize, bytes: &[u8; CALL_REL_LEN]) -> Option<usize> {
        (bytes[0] == 0xe8).then(|| {
            let rel = i32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
            (site + CALL_REL_LEN).wrapping_add(rel as usize)
        })
    }

    /// `call rel32` from `site` to `target`.
    fn call_rel(site: usize, target: usize) -> [u8; CALL_REL_LEN] {
        let rel = (target as u32)
            .wrapping_sub((site + CALL_REL_LEN) as u32)
            .to_le_bytes();
        [0xe8, rel[0], rel[1], rel[2], rel[3]]
    }

    /// `call rel32; nop` from `site` to `target`, replacing a `call [abs32]`.
    fn call_abs_replacement(site: usize, target: usize) -> [u8; CALL_ABS_LEN] {
        let r = call_rel(site, target);
        [r[0], r[1], r[2], r[3], r[4], 0x90]
    }

    /// The `call rel32` target at `site`, checked against `prologue`.
    fn checked_rel(base: usize, site: usize, prologue: &[u8], what: &str) -> Result<usize, String> {
        let present: [u8; CALL_REL_LEN] = unsafe { bytes(site) };
        let target = rel_target(site, &present).ok_or_else(|| {
            format!(
                "+{:#x} holds {present:02x?}, not the game's call to {what} -- something else has patched it",
                site - base
            )
        })?;
        // Safety: a call target inside client.dll's code.
        let head = unsafe { std::slice::from_raw_parts(target as *const u8, prologue.len()) };
        if head != prologue {
            return Err(format!(
                "the call at +{:#x} reaches +{:#x}, which does not start like {what}",
                site - base,
                target.wrapping_sub(base)
            ));
        }
        Ok(target)
    }

    /// Redirects the ten calls in the module at `base`. Checks every site
    /// before writing any.
    fn install(base: usize) -> Result<(), String> {
        if INSTALLED_BASE.load(Ordering::Acquire) == base {
            return Ok(());
        }
        // The row stub records the row's corner for the anchor.
        super::super::Setting::IconX.ensure_detour(base)?;
        // Safety: `base` is a module handle the loader gave us.
        let timer = unsafe { scan::find_unique(base, super::super::TIMER_PATTERN) }
            .map_err(|why| format!("could not locate the timer -- {why}"))?;
        let row = unsafe { scan::find_unique(base, super::super::ROW_PATTERN) }
            .map_err(|why| format!("could not locate the icon row -- {why}"))?;

        let set: [u8; CALL_ABS_LEN] = unsafe { bytes(timer + TIMER_SPR_SET) };
        let g = gengfuncs_of(&set, SPR_SET_SLOT).ok_or_else(|| {
            format!(
                "+{:#x} holds {set:02x?}, not the timer's SPR_Set call",
                timer + TIMER_SPR_SET - base
            )
        })?;
        let mut abs_sites: Vec<(usize, usize)> = Vec::new();
        for (site, ours) in [
            (
                timer + TIMER_BACKGROUND_DRAW,
                timer_background as DrawHolesFn as usize,
            ),
            (row + ROW_FILL_DRAW, row_draw_holes as DrawHolesFn as usize),
            (row + ROW_ICON_DRAW, row_draw_holes as DrawHolesFn as usize),
        ] {
            let present: [u8; CALL_ABS_LEN] = unsafe { bytes(site) };
            if gengfuncs_of(&present, DRAW_HOLES_SLOT) != Some(g) {
                return Err(format!(
                    "+{:#x} holds {present:02x?}, not the game's SPR_DrawHoles call -- something else has patched it",
                    site - base
                ));
            }
            abs_sites.push((site, ours));
        }

        let mut digit = None;
        for offset in TIMER_DIGIT_CALLS {
            let target = checked_rel(
                base,
                timer + offset,
                DIGIT_PROLOGUE,
                "the timer's digit helper",
            )?;
            if digit.is_some_and(|known| known != target) {
                return Err(format!(
                    "the digit calls at +{:#x} do not all reach one helper",
                    timer + offset - base
                ));
            }
            digit = Some(target);
        }
        let digit = digit.ok_or("no digit calls")?;
        let map = checked_rel(
            base,
            row + ROW_MAP_MODE_CALL,
            MAP_MODE_PROLOGUE,
            "CHud::OverviewMapMode",
        )?;
        let text = checked_rel(
            base,
            row + ROW_TEXT_CALL,
            TEXT_PROLOGUE,
            "CHud::DrawHudString",
        )?;

        GENGFUNCS.store(g, Ordering::Release);
        DIGIT.store(digit, Ordering::Release);
        MAP_MODE.store(map, Ordering::Release);
        TEXT.store(text, Ordering::Release);

        let mut writes: Vec<(usize, Vec<u8>)> = abs_sites
            .into_iter()
            .map(|(site, ours)| (site, call_abs_replacement(site, ours).to_vec()))
            .collect();
        for offset in TIMER_DIGIT_CALLS {
            let site = timer + offset;
            writes.push((
                site,
                call_rel(site, timer_digit as DigitFn as usize).to_vec(),
            ));
        }
        let site = row + ROW_MAP_MODE_CALL;
        writes.push((
            site,
            call_rel(site, map_mode as MapModeFn as usize).to_vec(),
        ));
        let site = row + ROW_TEXT_CALL;
        writes.push((site, call_rel(site, row_text as TextFn as usize).to_vec()));
        for (site, code) in writes {
            // Safety: a call checked above, replaced by one of the same
            // length, so no instruction boundary moves.
            if !unsafe { crate::patch::write_code_bytes(site, &code) } {
                return Err(format!("could not make +{:#x} writable", site - base));
            }
        }
        INSTALLED_BASE.store(base, Ordering::Release);
        unsafe {
            crate::debug::report(&format!(
                "objicons scale: the timer's and the icon row's draw calls at client+{:#x}.. and +{:#x}.. now go through the hook",
                timer - base,
                row - base
            ))
        };
        Ok(())
    }

    /// Makes `s` the scale. The reply is what the console prints.
    pub(super) fn apply(s: f32) -> Result<String, String> {
        if is_unity(s) && INSTALLED_BASE.load(Ordering::Acquire) == 0 {
            SCALE_BITS.store(s.to_bits(), Ordering::Release);
            return Ok(applied(s));
        }
        let base = engine::client_module_base().ok_or("client.dll is not loaded yet")?;
        install(base)?;
        if !is_unity(s) {
            crate::gl_zoom::ready().map_err(|why| format!("cannot scale: {why}"))?;
        }
        SCALE_BITS.store(s.to_bits(), Ordering::Release);
        Ok(applied(s))
    }
}

/// Makes `s` the objectives' scale (1 is the game's own). The reply is what
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

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    /// The timer's own layout: background at `(0, y)`, digits at these x and
    /// `y + 6` (`client+0x301a2`..`+0x301d8`).
    const DIGIT_X: [i32; 4] = [10, 24, 44, 58];
    const DIGIT_DY: i32 = 6;

    #[test]
    fn scale_1_draws_everything_where_the_game_does() {
        for anchor in [(0, 2), (82, 5), (300, -40)] {
            for p in [(0, 2), (10, 8), (82, 4), (200, 50)] {
                assert_eq!(zoomed(anchor, 1.0, p), (p.0 as f32 + 0.5, p.1 as f32 + 0.5));
            }
        }
        assert_eq!(scale(), 1.0, "the default is the game's own");
    }

    #[test]
    fn the_timer_grows_from_its_corner_and_its_digits_spread_with_it() {
        let y = 122; // spectating at 1080p
        for s in [1.5f32, 2.0, 3.0] {
            let corner = zoomed((0, y), s, (0, y));
            assert_eq!(corner, (0.5, y as f32 + 0.5), "the corner stays put");
            for x in DIGIT_X {
                let (dx, dy) = zoomed((0, y), s, (x, y + DIGIT_DY));
                assert_eq!(dx, 0.5 + s * x as f32);
                assert_eq!(dy, y as f32 + 0.5 + s * DIGIT_DY as f32);
            }
        }
    }

    #[test]
    fn the_game_s_row_x_scales_with_the_timer_and_a_typed_one_stays() {
        // The game's 82 sits right of the timer; scaled from the left edge it
        // stays right of a timer scaled the same.
        assert_eq!(row_anchor_x(82, false, false), 0);
        assert_eq!(zoomed((0, 5), 2.0, (82, 5)).0, 0.5 + 164.0);
        // `xoffset 300`: the row starts at 300 whatever the scale, and the
        // second icon (step 32 + 2) moves out by S times the step.
        assert_eq!(row_anchor_x(300, true, false), 300);
        assert_eq!(zoomed((300, 5), 2.0, (300, 5)).0, 300.5);
        assert_eq!(zoomed((300, 5), 2.0, (334, 5)).0, 300.5 + 68.0);
        // spec_pip's x beside its picture is kept too.
        assert_eq!(row_anchor_x(417, false, true), 417);
    }

    #[test]
    fn the_row_keeps_its_y_and_everything_under_it_moves_down_by_s() {
        // `offset 40`: the anchor y is 40; the flags are drawn at y - 1 and
        // the count at y + icon height + 4, both S times as far from it.
        let anchor = row_anchor(0, (82, 40), false, false).unwrap();
        assert_eq!(anchor, (0, 40));
        let s = 3.0;
        assert_eq!(zoomed(anchor, s, (82, 39)).1, 40.5 - 3.0);
        let icon = 32;
        assert_eq!(zoomed(anchor, s, (86, 40 + icon + 4)).1, 40.5 + 3.0 * 36.0);
    }

    #[test]
    fn the_full_overview_map_keeps_the_game_s_icons() {
        assert_eq!(row_anchor(MAP_FULL, (82, 5), false, false), None);
        assert_eq!(row_anchor(0, (82, 5), false, false), Some((0, 5)));
        assert_eq!(
            row_anchor(2, (82, 5), true, false),
            Some((82, 5)),
            "the minimap leaves the row in the corner"
        );
    }

    #[test]
    fn scale_arguments_parse_as_typed() {
        assert_eq!(parse_args(&argv(&["2"])), Ok(2.0));
        assert_eq!(parse_args(&argv(&["1.5"])), Ok(1.5));
        assert_eq!(parse_args(&argv(&["default"])), Ok(1.0));
        assert_eq!(parse_args(&argv(&["8"])), Ok(8.0));
        assert!(parse_args(&argv(&["0.5"])).is_err());
        assert!(parse_args(&argv(&["9"])).is_err());
        assert!(parse_args(&argv(&["nan"])).is_err());
        assert!(parse_args(&argv(&["big"])).is_err());
        assert!(parse_args(&argv(&[])).is_err());
        assert!(parse_args(&argv(&["2", "3"])).is_err());
    }

    #[test]
    fn the_status_and_replies_name_the_scale() {
        assert_eq!(describe(), "scale = 1");
        assert_eq!(format_scale(1.5), "1.5");
        assert!(applied(2.0).starts_with("scale = 2:"));
        assert_eq!(applied(1.0), "scale = 1, the game's own");
    }

    #[test]
    fn the_sites_sit_where_the_disassembly_has_them() {
        // `+0x30119` and `+0x30248` are the two patterns' matches.
        let (timer, row) = (0x30119usize, 0x30248usize);
        assert_eq!(timer + TIMER_SPR_SET, 0x30159);
        assert_eq!(timer + TIMER_BACKGROUND_DRAW, 0x30168);
        assert_eq!(
            TIMER_DIGIT_CALLS.map(|o| timer + o),
            [0x301ab, 0x301b6, 0x301cd, 0x301d8]
        );
        assert_eq!(row + ROW_MAP_MODE_CALL, 0x3029a);
        assert_eq!(row + ROW_FILL_DRAW, 0x3044c);
        assert_eq!(row + ROW_ICON_DRAW, 0x30491);
        assert_eq!(row + ROW_TEXT_CALL, 0x30517);
        // The timer's SPR_Set is the four pushes and a mov before its draw.
        assert_eq!(TIMER_BACKGROUND_DRAW - TIMER_SPR_SET, 0x0f);
    }
}
