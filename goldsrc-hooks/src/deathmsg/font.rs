//! `ds_hud_deathmsg font`: the kill feed's names in a font of our own, set
//! from the console and shown on the next frame (#562).
//!
//! ## What the game does
//!
//! `CHudDeathNotice::Draw` draws each name with the engine's console-string
//! calls, not a sprite font: `pfnDrawConsoleStringLen` for the layout
//! (`client+0x2b00d` the victim's width, `+0x2b04e` the killer's),
//! `pfnDrawSetTextColor` for each name's team colour (`+0x2b079`, `+0x2b125`)
//! and `pfnDrawConsoleString` for the text itself (`+0x2b089`, `+0x2b135`).
//! All six are `call [gEngfuncs + slot*4]`, slots 29, 28 and 27. The weapon
//! icon between the names is a sprite (`SPR_DrawHoles` at `+0x2b0f1`), and
//! the next line starts `rect.bottom - rect.top` lower: the icon's height,
//! not a constant (`+0x2b13e`).
//!
//! In `hw.dll`, `pfnDrawConsoleString` draws through the engine's own vgui2
//! surface (`VGUI_Surface026`, class `BaseUISurface`, served by `hw.dll`'s
//! `CreateInterface` on both builds) with one font: `"EngineFont"` from the
//! default scheme, `dod/resource/TrackerScheme.res`, fetched once and kept.
//! The call sets the font, the position and the colour, converts the string
//! to UTF-16 (`ILocalize::ConvertANSIToUnicode`, CP_UTF8), prints it, and then
//! puts the colour back to `con_color`. Its width is the sum of each
//! character's `GetCharABCwide`, its height `GetFontTall`.
//!
//! So changing the font means editing the user's `.res`, which this project
//! never does, and it would change every other console-string user with it.
//!
//! ## How
//!
//! The six `call [gEngfuncs+N]` instructions (6 bytes each) become
//! `call rel32; nop` to three cdecl functions here with the same signatures.
//! While no font is set they forward to the engine's own, so nothing changes.
//! With one set, [`hook::draw_string`] draws the name through the same
//! surface with our handle, [`hook::string_len`] measures it with the same
//! handle (so the right-edge anchoring the layout computes stays right), and
//! [`hook::set_text_color`] records the colour the game asked for, since each
//! name is coloured by a call just before it. The victim's width call goes to
//! [`hook::victim_len`] instead, which adds a right margin while a font or a
//! scale is in force, so a big last glyph or a drop shadow is not cut by the
//! screen's edge (`scale::right_margin`). A 16-byte detour at the line
//! step makes it `max(icon height, font height)`, so taller text does not
//! overlap the next line. The icon's size and its place on the line are
//! [`super::scale`]'s (#728), which rewrites three more operands and adds a
//! line-start detour in the same span, sharing this module's pattern match.
//! Through that detour the name draws here know the line's icon height, so
//! both names are centred on the line whenever a font or a scale is set
//! (`scale::name_draw_y`); at scale 1 with no font they stay where the game
//! puts them.
//!
//! The font is `ISurface::CreateFont()` plus `AddGlyphSetToFont`. Each change
//! makes a **new** handle: `CFontManager::AddGlyphSetToFont` returns false
//! for a handle that already has a glyph set, and `VGUI_Surface026` has no
//! way to free one. The old handle costs one table entry; the Windows font
//! behind it is cached by name, size and flags, so switching back and forth
//! allocates nothing more. The next frame draws with the new handle: no
//! demo reload.
//!
//! Installed on the first `font` or `scale` other than 1, and never undone
//! (`detour::Detour`):
//! `font default` makes the calls forward again. Checked every frame like
//! the other patches, and re-applied should `client.dll` be loaded afresh.
//!
//! ## Flags
//!
//! vgui2's scheme loader maps `dropshadow` to bit 0x80 when the surface says
//! it `SupportsFeature(2)`, which both `hw.dll` builds do, and the PRE
//! `EngineFont`'s own shadow shows the renderer honours it. Neither build's
//! font manager knows an outline bit (0x200), and only the scheme loader of
//! a later vgui2 does, so `outline` is accepted and said to do nothing.

// The patching runs only in the 32-bit DLL; a host build (workspace clippy,
// tests) keeps the tables and helpers for the unit tests alone.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code, unused_imports))]

use std::fmt;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering};

/// `Draw` from the victim's width call on (`client+0x2b002`): the two `lea`s
/// and three pushes of its arguments, the call, and the x arithmetic after
/// it. Wildcards cover the absolute addresses. Unique in `client.dll`.
const PATTERN: &str = "8D 54 24 40 8D 44 24 30 52 50 51 FF 15 ?? ?? ?? ?? 8B 5C 24 28 \
                       A1 ?? ?? ?? ?? 8B 35 ?? ?? ?? ?? 8B 4C 24 3C C1 E3 04 83 C4 0C \
                       8B 54 03 04 2B F2 8B 14 03 2B F1";

/// Which engine call a redirected site makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Call {
    /// `pfnDrawConsoleString`, slot 27.
    Draw,
    /// `pfnDrawSetTextColor`, slot 28.
    Color,
    /// `pfnDrawConsoleStringLen`, slot 29.
    Len,
    /// The same call for the victim, the width the line is anchored by: it
    /// also reserves the right margin (`scale::right_margin`).
    VictimLen,
}

impl Call {
    /// Its `cl_enginefuncs_t` slot; `engine::ClEngineFuncsPartial` types the
    /// same three.
    fn slot(self) -> usize {
        match self {
            Call::Draw => 27,
            Call::Color => 28,
            Call::Len | Call::VictimLen => 29,
        }
    }
}

/// The six call sites, as offsets from the [`PATTERN`] match: the victim's
/// and killer's width, then the killer's colour and name, then the victim's.
const SITES: &[(usize, Call)] = &[
    (0x0b, Call::VictimLen),
    (0x4c, Call::Len),
    (0x77, Call::Color),
    (0x87, Call::Draw),
    (0x123, Call::Color),
    (0x133, Call::Draw),
];

/// One more `call [gEngfuncs+N]` in the same span, left alone but read: the
/// icon's `SPR_Set` (slot 4, `+0x2b0de`). It pins `gEngfuncs` independently
/// of the six above. The `SPR_DrawHoles` call after it (slot 6, `+0x2b0f1`)
/// is not an anchor: `scale` redirects it (#728).
const ANCHORS: &[(usize, usize)] = &[(0xdc, 4)];

/// `FF 15 imm32`, the same length as `E8 rel32` plus one `nop`.
pub(super) const CALL_LEN: usize = 6;

/// Where the line step is computed, from the match: `+0x2b13e`.
const STEP_AT: usize = 0x13c;

/// `mov ecx, [esp+0x54]` (rect.bottom), `mov edx, [esp+0x50]` (rect.top),
/// `mov esi, [esp+0x20]`, `sub ecx, edx`, `add ebx, ecx` (y += height). The
/// only branch into this span lands on its first byte (`jne` at `+0x2b10e`).
const STEP_STOLEN: &[u8] = &[
    0x8b, 0x4c, 0x24, 0x54, 0x8b, 0x54, 0x24, 0x50, 0x8b, 0x74, 0x24, 0x20, 0x2b, 0xca, 0x03, 0xd9,
];

/// The height range `font` takes, in pixels. Below 4 nothing is legible, and
/// 200 is already most of a 1080p screen for four lines.
pub(super) const MIN_TALL: i32 = 4;
pub(super) const MAX_TALL: i32 = 200;
/// `LF_FACESIZE` less the terminator: what `CreateFontA` can take.
const MAX_FACE_LEN: usize = 31;

/// vgui2 `FONTFLAG_DROPSHADOW`.
const FLAG_DROPSHADOW: i32 = 0x80;
/// vgui2 `FONTFLAG_OUTLINE`, which this engine's font manager never tests.
const FLAG_OUTLINE: i32 = 0x200;
/// `FW_BOLD`; the scheme loader passes 0 (`FW_DONTCARE`) for a plain face.
const WEIGHT_BOLD: i32 = 700;

/// A font as typed: `font <face...> <size> [bold] [shadow] [outline]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FontSpec {
    pub face: String,
    pub tall: i32,
    pub bold: bool,
    pub shadow: bool,
    pub outline: bool,
}

impl FontSpec {
    fn flags(&self) -> i32 {
        (if self.shadow { FLAG_DROPSHADOW } else { 0 })
            | (if self.outline { FLAG_OUTLINE } else { 0 })
    }

    fn weight(&self) -> i32 {
        if self.bold { WEIGHT_BOLD } else { 0 }
    }
}

impl fmt::Display for FontSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\"{}\" {}", self.face, self.tall)?;
        for (on, word) in [
            (self.bold, "bold"),
            (self.shadow, "shadow"),
            (self.outline, "outline"),
        ] {
            if on {
                write!(f, " {word}")?;
            }
        }
        Ok(())
    }
}

/// The arguments after `font`: `default`, or a face, a size and flag words.
/// The face is every token before the first number, so `Times New Roman`
/// works unquoted; `Ok(None)` is `default`.
pub(super) fn parse_args(rest: &[String]) -> Result<Option<FontSpec>, String> {
    if rest.len() == 1 && rest[0].eq_ignore_ascii_case("default") {
        return Ok(None);
    }
    let Some(size_at) = rest.iter().position(|t| t.parse::<i32>().is_ok()) else {
        return Err("need a face and a size, e.g. font \"Verdana\" 28 bold shadow".to_string());
    };
    let face = rest[..size_at].join(" ");
    if face.is_empty() {
        return Err("the face comes before the size, e.g. font \"Verdana\" 28".to_string());
    }
    if face.len() > MAX_FACE_LEN {
        return Err(format!(
            "face names are at most {MAX_FACE_LEN} characters, got {face:?}"
        ));
    }
    let tall: i32 = rest[size_at]
        .parse()
        .map_err(|_| "size is not a number".to_string())?;
    if !(MIN_TALL..=MAX_TALL).contains(&tall) {
        return Err(format!(
            "size: expected {MIN_TALL}..={MAX_TALL}, got {tall}"
        ));
    }
    let mut spec = FontSpec {
        face,
        tall,
        bold: false,
        shadow: false,
        outline: false,
    };
    for word in &rest[size_at + 1..] {
        match word.to_ascii_lowercase().as_str() {
            "bold" => spec.bold = true,
            "shadow" | "dropshadow" => spec.shadow = true,
            "outline" => spec.outline = true,
            other => {
                return Err(format!("{other:?} is not one of bold, shadow, outline"));
            }
        }
    }
    Ok(Some(spec))
}

/// `call rel32; nop` from `site` to `target`: the six bytes that replace a
/// `call [gEngfuncs+N]`. On 32-bit the displacement wraps, so any target is
/// in reach.
pub(super) fn redirect_bytes(site: usize, target: usize) -> [u8; CALL_LEN] {
    let rel = (target as u32)
        .wrapping_sub((site + 5) as u32)
        .to_le_bytes();
    [0xe8, rel[0], rel[1], rel[2], rel[3], 0x90]
}

/// `call [gengfuncs + slot*4]`, the shipped bytes at a site; what
/// [`gengfuncs_of`] reads back.
#[cfg(test)]
pub(super) fn stock_bytes(gengfuncs: usize, slot: usize) -> [u8; CALL_LEN] {
    let abs = ((gengfuncs + slot * 4) as u32).to_le_bytes();
    [0xff, 0x15, abs[0], abs[1], abs[2], abs[3]]
}

/// `gEngfuncs` from a stock `call [gEngfuncs + slot*4]`, or `None` for
/// anything else.
pub(super) fn gengfuncs_of(bytes: &[u8; CALL_LEN], slot: usize) -> Option<usize> {
    if bytes[..2] != [0xff, 0x15] {
        return None;
    }
    let abs = u32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]) as usize;
    abs.checked_sub(slot * 4)
}

/// The step stub, hand-assembled:
///
/// ```asm
/// mov ecx, [esp+0x54] / mov edx, [esp+0x50] / mov esi, [esp+0x20]
/// sub ecx, edx                 ; the icon's height, as the game computes it
/// cmp ecx, [MIN]               ; our font's height, 0 while unset
/// jge .keep
/// mov ecx, [MIN]
/// .keep:
/// add ebx, ecx                 ; y += the larger
/// jmp dword ptr [RESUME]
/// ```
///
/// `ecx` is dead after the span (the next read is a fresh load at the loop
/// top) and the flags `sub` leaves are overwritten before use, so the stub
/// may change both.
fn step_stub(min: usize, resume: usize) -> Vec<u8> {
    let (loads_and_sub, add) = STEP_STOLEN.split_at(STEP_STOLEN.len() - 2);
    let mut code = loads_and_sub.to_vec();
    code.extend_from_slice(&[0x3b, 0x0d]); // cmp ecx, [abs32]
    code.extend_from_slice(&(min as u32).to_le_bytes());
    code.extend_from_slice(&[0x7d, 0x06]); // jge over the substitution
    code.extend_from_slice(&[0x8b, 0x0d]); // mov ecx, [abs32]
    code.extend_from_slice(&(min as u32).to_le_bytes());
    code.extend_from_slice(add); // add ebx, ecx
    code.extend_from_slice(&[0xff, 0x25]); // jmp dword ptr [abs32]
    code.extend_from_slice(&(resume as u32).to_le_bytes());
    code
}

/// `con_color`'s "r g b", the colour the engine draws a console string in
/// when nothing set one. `None` for anything else.
fn con_color_from(text: &str) -> Option<(i32, i32, i32)> {
    let mut parts = text.split_whitespace().map(|p| p.parse::<i32>().ok());
    let (Some(Some(r)), Some(Some(g)), Some(Some(b)), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    Some((r.clamp(0, 255), g.clamp(0, 255), b.clamp(0, 255)))
}

/// The colour `con_color` has when the user never changed it.
const CON_COLOR_DEFAULT: (i32, i32, i32) = (255, 180, 30);

/// A `pfnDrawSetTextColor` component as the engine stores it: `(int)(f*255)`,
/// truncated, held to a byte.
fn color_byte(component: f32) -> i32 {
    if !component.is_finite() {
        return 0;
    }
    ((component * 255.0) as i32).clamp(0, 255)
}

/// The engine's conversion, near enough: UTF-8 to UTF-16 (`ConvertANSIToUnicode`
/// is `MultiByteToWideChar(CP_UTF8, ...)` on both builds), control characters
/// drawn as spaces as the engine does with anything `iswprint` rejects, and a
/// terminator for `GetTextSize`.
fn to_wide(bytes: &[u8]) -> Vec<u16> {
    let text = String::from_utf8_lossy(bytes);
    let mut out: Vec<u16> = text
        .encode_utf16()
        .map(|c| if c < 0x20 { 0x20 } else { c })
        .collect();
    out.push(0);
    out
}

/// The font in force, for `status` and the usage text.
static SPEC: Mutex<Option<FontSpec>> = Mutex::new(None);

/// Our `HFont`, or 0 while the game's font is in use. Read by the redirected
/// calls on the game thread.
static FONT: AtomicU32 = AtomicU32::new(0);
/// Our font's height, the floor the step stub applies; 0 while unset. Read by
/// game code, so an address rather than a value: [`AtomicI32::as_ptr`].
static FONT_TALL: AtomicI32 = AtomicI32::new(0);
/// The colour the last redirected `pfnDrawSetTextColor` asked for, as
/// `0x00rrggbb`, or -1 once a draw has used it (the engine resets to
/// `con_color` after each string).
static COLOR: AtomicI32 = AtomicI32::new(-1);
/// Whether a failure has been logged, so the per-frame check says it once.
static COMPLAINED: AtomicBool = AtomicBool::new(false);
/// The `client.dll` the sites were rewritten in, 0 before.
static INSTALLED_BASE: AtomicUsize = AtomicUsize::new(0);
/// The engine font's height as its length call last reported it, for
/// centring the names drawn in it; 0 until a line was measured.
static ENGINE_TALL: AtomicI32 = AtomicI32::new(0);

/// Our font's height in pixels, 0 while the game's font is in use: the floor
/// the step stub applies, so `scale` centres the icon on the same line height
/// the step uses.
pub(super) fn tall() -> i32 {
    FONT_TALL.load(Ordering::Relaxed)
}

/// What `status` says about the font.
pub(super) fn describe() -> String {
    match SPEC.lock().map(|s| s.clone()) {
        Ok(Some(spec)) => format!("font = {spec}"),
        _ => "font = the game's".to_string(),
    }
}

#[cfg(target_arch = "x86")]
mod hook {
    use std::ffi::{CStr, CString, c_char, c_void};

    use super::*;
    use crate::engine;
    use crate::scan;

    /// `ISurface` (`VGUI_Surface026`) slots, slot 0 being the interface's
    /// destructor. MSVC lays adjacent overloads out in reverse order, so the
    /// four-int `DrawSetTextColor` is the second of its pair. Every one is
    /// checked by `ret` size on both builds in
    /// `tools/verify_killfeed_font_offsets.py`.
    const SURFACE_DRAW_SET_TEXT_FONT: usize = 13;
    const SURFACE_DRAW_SET_TEXT_COLOR_RGBA: usize = 15;
    const SURFACE_DRAW_SET_TEXT_POS: usize = 16;
    const SURFACE_DRAW_PRINT_TEXT: usize = 18;
    const SURFACE_DRAW_FLUSH_TEXT: usize = 21;
    const SURFACE_CREATE_FONT: usize = 59;
    const SURFACE_ADD_GLYPH_SET_TO_FONT: usize = 60;
    const SURFACE_GET_FONT_TALL: usize = 62;
    const SURFACE_GET_TEXT_SIZE: usize = 65;

    type SetFontFn = unsafe extern "thiscall" fn(*mut c_void, u32);
    type SetPosFn = unsafe extern "thiscall" fn(*mut c_void, i32, i32);
    type SetColorFn = unsafe extern "thiscall" fn(*mut c_void, i32, i32, i32, i32);
    type PrintTextFn = unsafe extern "thiscall" fn(*mut c_void, *const u16, i32);
    type FlushFn = unsafe extern "thiscall" fn(*mut c_void);
    type CreateFontFn = unsafe extern "thiscall" fn(*mut c_void) -> u32;
    /// `(font, windowsFontName, tall, weight, blur, scanlines, flags,
    /// lowRange, highRange)`; a `bool` back in `al`. The engine's wrapper
    /// hands only the first seven to its font manager, which picks the
    /// ranges itself.
    type AddGlyphSetFn = unsafe extern "thiscall" fn(
        *mut c_void,
        u32,
        *const c_char,
        i32,
        i32,
        i32,
        i32,
        i32,
        i32,
        i32,
    ) -> u32;
    type FontTallFn = unsafe extern "thiscall" fn(*mut c_void, u32) -> i32;
    type TextSizeFn = unsafe extern "thiscall" fn(*mut c_void, u32, *const u16, *mut i32, *mut i32);

    /// The engine's `BaseUISurface`, once looked up.
    static SURFACE: AtomicUsize = AtomicUsize::new(0);
    /// `gEngfuncs` in `client.dll`, read out of the first site.
    static GENGFUNCS: AtomicUsize = AtomicUsize::new(0);
    /// The [`PATTERN`] match, and the base it was found in.
    static MATCH: AtomicUsize = AtomicUsize::new(0);
    static MATCH_BASE: AtomicUsize = AtomicUsize::new(0);
    /// Where the step stub jumps back to.
    static STEP_RESUME: AtomicUsize = AtomicUsize::new(0);
    /// The base the step detour went into, 0 before.
    static STEP_BASE: AtomicUsize = AtomicUsize::new(0);
    static STEP_DETOUR: Mutex<Option<crate::detour::Detour>> = Mutex::new(None);
    /// `con_color`'s `cvar_s`, once found; stable for the session.
    static CON_COLOR: AtomicUsize = AtomicUsize::new(0);

    unsafe fn slot<F: Copy>(object: *mut c_void, index: usize) -> F {
        unsafe {
            let vftable = *(object as *const *const usize);
            std::mem::transmute_copy(&*vftable.add(index))
        }
    }

    fn surface() -> Result<*mut c_void, String> {
        let cached = SURFACE.load(Ordering::Acquire);
        if cached != 0 {
            return Ok(cached as *mut c_void);
        }
        let object = engine::create_interface(c"hw.dll", c"VGUI_Surface026")
            .ok_or("hw.dll does not serve VGUI_Surface026")?;
        SURFACE.store(object as usize, Ordering::Release);
        Ok(object)
    }

    /// Where a redirected site calls.
    fn target_of(call: Call) -> usize {
        match call {
            Call::Draw => draw_string as engine::DrawConsoleStringFn as usize,
            Call::Color => set_text_color as engine::DrawSetTextColorFn as usize,
            Call::Len => string_len as engine::DrawConsoleStringLenFn as usize,
            Call::VictimLen => victim_len as engine::DrawConsoleStringLenFn as usize,
        }
    }

    /// The screen's width as the engine reports it, 0 when it cannot.
    fn screen_width() -> i32 {
        let Some(e) = engine::engfuncs() else {
            return 0;
        };
        let mut info = engine::ScreenInfo {
            size: size_of::<engine::ScreenInfo>() as i32,
            width: 0,
            height: 0,
            flags: 0,
            char_height: 0,
            char_widths: [0; 256],
        };
        // Safety: the engine fills the struct whose size it is told.
        unsafe { (e.pfn_get_screen_info)(&mut info) };
        info.width.max(0)
    }

    /// Stands in for `pfnDrawConsoleStringLen` at the victim's width, the
    /// first call of every line: [`string_len`], plus the right margin while
    /// a scale or a font is in force. `Draw` anchors the line at `ScreenWidth
    /// - icon - victim width`, so the extra width moves the whole line left
    /// and leaves the victim's name that far from the edge; the name itself is
    /// drawn where the icon's advance puts it, at its own width.
    unsafe extern "C" fn victim_len(text: *const c_char, width: *mut i32, height: *mut i32) {
        unsafe { string_len(text, width, height) };
        if width.is_null() || !super::super::scale::layout_is_ours() {
            return;
        }
        let tall = if height.is_null() {
            0
        } else {
            unsafe { *height }
        };
        let margin = super::super::scale::right_margin(screen_width(), tall);
        // Safety: the caller's out-parameter, which `string_len` just wrote.
        unsafe { *width += margin };
    }

    /// `con_color` as (r, g, b), the colour a string gets with none set.
    fn con_color() -> (i32, i32, i32) {
        let mut cvar = CON_COLOR.load(Ordering::Relaxed) as *mut engine::CvarSPartial;
        if cvar.is_null() {
            let Some(engfuncs) = engine::engfuncs() else {
                return CON_COLOR_DEFAULT;
            };
            cvar = unsafe { (engfuncs.pfn_get_cvar_pointer)(c"con_color".as_ptr()) };
            if cvar.is_null() {
                return CON_COLOR_DEFAULT;
            }
            CON_COLOR.store(cvar as usize, Ordering::Relaxed);
        }
        // Safety: a registered cvar lives for the session; its string is NUL-terminated.
        let text = unsafe { (*cvar).string };
        if text.is_null() {
            return CON_COLOR_DEFAULT;
        }
        unsafe { CStr::from_ptr(text) }
            .to_str()
            .ok()
            .and_then(con_color_from)
            .unwrap_or(CON_COLOR_DEFAULT)
    }

    /// Stands in for `pfnDrawConsoleString` at the two name draws. Both are
    /// centred on the line by [`super::super::scale::name_draw_y`], which
    /// leaves `y` alone at scale 1 with no font.
    unsafe extern "C" fn draw_string(x: i32, y: i32, text: *const c_char) -> i32 {
        let font = FONT.load(Ordering::Acquire);
        if font == 0 {
            let y = super::super::scale::name_draw_y(y, ENGINE_TALL.load(Ordering::Relaxed));
            return match engine::engfuncs() {
                Some(e) => unsafe { (e.pfn_draw_console_string)(x, y, text) },
                None => x,
            };
        }
        let y = super::super::scale::name_draw_y(y, FONT_TALL.load(Ordering::Relaxed));
        let Ok(surface) = surface() else {
            return x;
        };
        if text.is_null() {
            return x;
        }
        // Safety: the game's own `szKiller`/`szVictim`, NUL-terminated by `MsgFunc_DeathMsg`.
        let wide = to_wide(unsafe { CStr::from_ptr(text) }.to_bytes());
        let (r, g, b) = match COLOR.swap(-1, Ordering::AcqRel) {
            -1 => con_color(),
            packed => ((packed >> 16) & 0xff, (packed >> 8) & 0xff, packed & 0xff),
        };
        let chars = (wide.len() - 1) as i32;
        // Safety: the engine's own text path makes these same calls from
        // `HUD_Redraw`; the slots are checked by ret size on both builds.
        unsafe {
            let set_font: SetFontFn = slot(surface, SURFACE_DRAW_SET_TEXT_FONT);
            let set_pos: SetPosFn = slot(surface, SURFACE_DRAW_SET_TEXT_POS);
            let set_color: SetColorFn = slot(surface, SURFACE_DRAW_SET_TEXT_COLOR_RGBA);
            let print: PrintTextFn = slot(surface, SURFACE_DRAW_PRINT_TEXT);
            let flush: FlushFn = slot(surface, SURFACE_DRAW_FLUSH_TEXT);
            set_font(surface, font);
            set_pos(surface, x, y);
            set_color(surface, r, g, b, 255);
            print(surface, wide.as_ptr(), chars);
            flush(surface);
        }
        x + text_width(surface, font, &wide)
    }

    /// Stands in for `pfnDrawSetTextColor` before each name: remembers the
    /// colour for [`draw_string`] and still tells the engine, so its own
    /// state is what it would have been.
    unsafe extern "C" fn set_text_color(r: f32, g: f32, b: f32) {
        if FONT.load(Ordering::Acquire) != 0 {
            let packed = (color_byte(r) << 16) | (color_byte(g) << 8) | color_byte(b);
            COLOR.store(packed, Ordering::Release);
        }
        if let Some(e) = engine::engfuncs() {
            unsafe { (e.pfn_draw_set_text_color)(r, g, b) };
        }
    }

    /// Stands in for `pfnDrawConsoleStringLen`: the size our font gives the
    /// name, so the layout's right-edge arithmetic lines up with the draw.
    unsafe extern "C" fn string_len(text: *const c_char, width: *mut i32, height: *mut i32) {
        let font = FONT.load(Ordering::Acquire);
        if font == 0 {
            if let Some(e) = engine::engfuncs() {
                unsafe { (e.pfn_draw_console_string_len)(text, width, height) };
                // The engine font's height, for centring its names: the
                // victim's width call starts every line, before either draw.
                if !height.is_null() {
                    ENGINE_TALL.store(unsafe { *height }, Ordering::Relaxed);
                }
            }
            return;
        }
        let Ok(surface) = surface() else {
            return;
        };
        let wide = if text.is_null() {
            vec![0u16]
        } else {
            to_wide(unsafe { CStr::from_ptr(text) }.to_bytes())
        };
        let w = text_width(surface, font, &wide);
        let h = FONT_TALL.load(Ordering::Relaxed);
        // Safety: the caller's two out-parameters, as the engine writes them.
        unsafe {
            if !width.is_null() {
                *width = w;
            }
            if !height.is_null() {
                *height = h;
            }
        }
    }

    /// `GetTextSize`'s width: the same per-character `GetCharABCwide` sum the
    /// engine's own length call makes.
    fn text_width(surface: *mut c_void, font: u32, wide: &[u16]) -> i32 {
        let (mut w, mut h) = (0i32, 0i32);
        unsafe {
            let size: TextSizeFn = slot(surface, SURFACE_GET_TEXT_SIZE);
            size(surface, font, wide.as_ptr(), &mut w, &mut h);
        }
        w.max(0)
    }

    /// Finds `Draw`'s span in the module at `base`, once per base.
    fn site(base: usize) -> Result<usize, String> {
        if MATCH_BASE.load(Ordering::Acquire) == base {
            let found = MATCH.load(Ordering::Acquire);
            if found != 0 {
                return Ok(found);
            }
        }
        // Safety: a mapped module, for the session.
        let found = unsafe { scan::find_unique(base, PATTERN) }
            .map_err(|why| format!("could not find the kill feed's name draws -- {why}"))?;
        MATCH.store(found, Ordering::Release);
        MATCH_BASE.store(base, Ordering::Release);
        Ok(found)
    }

    /// Reads the six bytes at `address`.
    unsafe fn bytes_at(address: usize) -> [u8; CALL_LEN] {
        let mut out = [0u8; CALL_LEN];
        // Safety: inside the matched span of a mapped module.
        out.copy_from_slice(unsafe { std::slice::from_raw_parts(address as *const u8, CALL_LEN) });
        out
    }

    /// The [`PATTERN`] match in the module at `base`, for `scale`, whose
    /// sites are offsets from the same match.
    pub(in crate::deathmsg) fn draw_site(base: usize) -> Result<usize, String> {
        site(base)
    }

    /// Rewrites the six sites and installs the step detour in the module at
    /// `base`, whatever of that is still to do. Returns whether anything was
    /// written. Called every frame while a font is wanted or was ever set, so
    /// it reads before it writes and is cheap when all is in place.
    fn install(base: usize) -> Result<bool, String> {
        let found = site(base)?;
        // Every site must be stock (and agree on one gEngfuncs with the two
        // sprite calls) or already ours; anything else means another patch,
        // and nothing is written.
        let mut gengfuncs: Option<usize> = None;
        let mut pending: Vec<(usize, Call)> = Vec::new();
        for &(offset, call) in SITES {
            let address = found + offset;
            let present = unsafe { bytes_at(address) };
            if present == redirect_bytes(address, target_of(call)) {
                continue;
            }
            let Some(g) = gengfuncs_of(&present, call.slot()) else {
                return Err(format!(
                    "+{:#x} holds {present:02x?}, neither the game's call nor ours -- something else has patched it",
                    address - base
                ));
            };
            if gengfuncs.is_some_and(|known| known != g) {
                return Err(format!(
                    "the calls at +{:#x} do not share one gEngfuncs",
                    address - base
                ));
            }
            gengfuncs = Some(g);
            pending.push((address, call));
        }
        if let Some(g) = gengfuncs {
            for &(offset, slot) in ANCHORS {
                let address = found + offset;
                let present = unsafe { bytes_at(address) };
                if gengfuncs_of(&present, slot) != Some(g) {
                    return Err(format!(
                        "the sprite call at +{:#x} does not use the gEngfuncs the name calls do",
                        address - base
                    ));
                }
            }
            GENGFUNCS.store(g, Ordering::Release);
        }

        let mut wrote = false;
        if STEP_BASE.load(Ordering::Acquire) != base {
            let step = found + STEP_AT;
            // Safety: inside the matched span.
            let present =
                unsafe { std::slice::from_raw_parts(step as *const u8, STEP_STOLEN.len()) };
            if present != STEP_STOLEN {
                return Err(format!(
                    "expected the line step {STEP_STOLEN:02x?} at +{:#x}, found {present:02x?}",
                    step - base
                ));
            }
            STEP_RESUME.store(step + STEP_STOLEN.len(), Ordering::Release);
            let stub = step_stub(FONT_TALL.as_ptr() as usize, STEP_RESUME.as_ptr() as usize);
            // Safety: the span was checked byte for byte, and nothing branches
            // into it (module doc; the verify script checks the whole of Draw).
            let detour = unsafe { crate::detour::install(step, STEP_STOLEN.len(), &stub) }?;
            unsafe {
                crate::debug::report(&format!(
                    "deathmsg font: line-step detour at client+{:#x}, stub at {:#x}",
                    step - base,
                    detour.stub_address()
                ))
            };
            if let Ok(mut held) = STEP_DETOUR.lock() {
                *held = Some(detour);
            }
            STEP_BASE.store(base, Ordering::Release);
            wrote = true;
        }

        for (address, call) in pending {
            let bytes = redirect_bytes(address, target_of(call));
            // Safety: a stock call in the matched span; the same six bytes
            // long, so no instruction boundary moves.
            if !unsafe { crate::patch::write_code_bytes(address, &bytes) } {
                return Err(format!("could not make +{:#x} writable", address - base));
            }
            wrote = true;
        }
        if INSTALLED_BASE.swap(base, Ordering::AcqRel) != base {
            unsafe {
                crate::debug::report(&format!(
                    "deathmsg font: the kill feed's six text calls at client+{:#x}.. now go through the hook (gEngfuncs at {:#x})",
                    found - base,
                    GENGFUNCS.load(Ordering::Relaxed)
                ))
            };
        }
        Ok(wrote)
    }

    /// Makes `spec` the font, or `None` the game's own.
    pub(super) fn apply(spec: Option<FontSpec>) -> Result<String, String> {
        let Some(spec) = spec else {
            FONT.store(0, Ordering::Release);
            FONT_TALL.store(0, Ordering::Release);
            if let Ok(mut held) = SPEC.lock() {
                *held = None;
            }
            return Ok("font back to the game's (EngineFont in TrackerScheme.res)".to_string());
        };
        let base = engine::client_module_base().ok_or("client.dll is not loaded yet")?;
        install(base)?;
        // Centring the icon and the names on a line goes through `scale`'s
        // sites, which draw the game's own at scale 1. The font works
        // without them, top-aligned, so a failure is logged, not refused.
        if let Err(why) = super::super::scale::ensure_installed() {
            unsafe {
                crate::debug::report(&format!(
                    "deathmsg font: the names and icons stay top-aligned -- {why}"
                ))
            };
        }
        let surface = surface()?;
        let face = CString::new(spec.face.as_str()).map_err(|_| "the face has a NUL in it")?;
        // Safety: the surface's own font calls, slots checked on both builds.
        let (handle, tall) = unsafe {
            let create: CreateFontFn = slot(surface, SURFACE_CREATE_FONT);
            let add: AddGlyphSetFn = slot(surface, SURFACE_ADD_GLYPH_SET_TO_FONT);
            let font_tall: FontTallFn = slot(surface, SURFACE_GET_FONT_TALL);
            let handle = create(surface);
            if handle == 0 {
                return Err("the engine gave no font handle".to_string());
            }
            let ok = add(
                surface,
                handle,
                face.as_ptr(),
                spec.tall,
                spec.weight(),
                0,
                0,
                spec.flags(),
                0,
                0xffff,
            ) & 0xff
                != 0;
            if !ok {
                return Err(format!(
                    "the engine refused {spec} -- could Windows not create that font?"
                ));
            }
            (handle, font_tall(surface, handle))
        };
        // Height first: the stub reads it, and a draw with the new handle
        // must not step by the old one.
        FONT_TALL.store(tall.max(0), Ordering::Release);
        FONT.store(handle, Ordering::Release);
        let outline = if spec.outline {
            " (outline: this engine's font renderer has none, so it is drawn without)"
        } else {
            ""
        };
        let reply = format!(
            "font = {spec}, {tall} px tall, handle {handle}{outline}. A face Windows does not have falls back to its default; `font default` puts the game's back."
        );
        if let Ok(mut held) = SPEC.lock() {
            *held = Some(spec);
        }
        Ok(reply)
    }

    /// Installs the six sites and the step detour with the font in force
    /// (forwarding to the engine while none is set), for `scale`, which
    /// centres the names through [`draw_string`].
    pub(in crate::deathmsg) fn ensure_installed() -> Result<(), String> {
        let base = engine::client_module_base().ok_or("client.dll is not loaded yet")?;
        install(base).map(|_| ())
    }

    /// Keeps the sites ours once they ever were. Called every frame.
    pub(super) fn poll() {
        if FONT.load(Ordering::Relaxed) == 0 && INSTALLED_BASE.load(Ordering::Relaxed) == 0 {
            return;
        }
        let Some(base) = engine::client_module_base() else {
            return;
        };
        match install(base) {
            Ok(_) => COMPLAINED.store(false, Ordering::Relaxed),
            Err(why) => {
                if !COMPLAINED.swap(true, Ordering::Relaxed) {
                    unsafe {
                        crate::debug::report(&format!("deathmsg font: not in place -- {why}"))
                    };
                }
            }
        }
    }
}

/// Makes `spec` the kill feed's font, or `None` the game's own. The reply is
/// what the console prints.
pub(super) fn apply(spec: Option<FontSpec>) -> Result<String, String> {
    #[cfg(target_arch = "x86")]
    {
        hook::apply(spec)
    }
    #[cfg(not(target_arch = "x86"))]
    {
        let _ = spec;
        Err("only in the 32-bit game".to_string())
    }
}

/// Re-checks the rewritten calls every frame, from `deathmsg::poll`.
pub(super) fn poll() {
    #[cfg(target_arch = "x86")]
    hook::poll();
}

/// `Draw`'s span in the module at `base` -- the [`PATTERN`] match -- for
/// `scale`, whose three operands are offsets from the same match.
#[cfg(target_arch = "x86")]
pub(super) use hook::{draw_site, ensure_installed};

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens() -> Vec<&'static str> {
        PATTERN.split_whitespace().collect()
    }

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn the_pattern_holds_the_first_site_where_the_table_says() {
        let t = tokens();
        let (first, _) = SITES[0];
        assert_eq!(&t[first..first + 2], &["FF", "15"], "call [imm32]");
        assert_eq!(
            &t[first + CALL_LEN..first + CALL_LEN + 2],
            &["8B", "5C"],
            "the next instruction"
        );
        // The rest lie past the pattern, inside Draw; the verify script
        // checks them against the binary, and `install` checks the bytes
        // before writing.
        for &(offset, _) in &SITES[1..] {
            assert!(offset > t.len());
        }
        assert!(STEP_AT > SITES[SITES.len() - 1].0 + CALL_LEN);
    }

    #[test]
    fn the_slots_are_the_ones_engine_rs_types() {
        use crate::engine::ClEngineFuncsPartial;
        let slot = |offset: usize| offset / size_of::<*const ()>();
        assert_eq!(
            slot(std::mem::offset_of!(
                ClEngineFuncsPartial,
                pfn_draw_console_string
            )),
            Call::Draw.slot()
        );
        assert_eq!(
            slot(std::mem::offset_of!(
                ClEngineFuncsPartial,
                pfn_draw_set_text_color
            )),
            Call::Color.slot()
        );
        assert_eq!(
            slot(std::mem::offset_of!(
                ClEngineFuncsPartial,
                pfn_draw_console_string_len
            )),
            Call::Len.slot()
        );
    }

    #[test]
    fn a_redirect_is_the_same_length_as_the_call_it_replaces() {
        let site = 0x0192_b089;
        let target = 0x6a00_1000;
        let bytes = redirect_bytes(site, target);
        assert_eq!(bytes[0], 0xe8);
        assert_eq!(bytes[5], 0x90);
        let rel = i32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
        assert_eq!((site + 5).wrapping_add(rel as usize), target);
        // And back the other way, as a DLL below client.dll would be.
        let bytes = redirect_bytes(site, 0x0040_1000);
        let rel = i32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
        assert_eq!((site + 5).wrapping_add(rel as usize), 0x0040_1000);
        assert_eq!(
            stock_bytes(0x01a7_6388, Call::Draw.slot()),
            [0xff, 0x15, 0xf4, 0x63, 0xa7, 0x01],
            "the bytes at client+0x2b089"
        );
    }

    #[test]
    fn the_step_stub_reproduces_the_stolen_bytes_and_skips_only_the_substitution() {
        let code = step_stub(0x1111_1111, 0x2222_2222);
        assert_eq!(
            &code[..STEP_STOLEN.len() - 2],
            &STEP_STOLEN[..STEP_STOLEN.len() - 2]
        );
        let jge = code
            .windows(2)
            .position(|w| w == [0x7d, 0x06])
            .expect("a jge");
        let landing = jge + 2 + 6;
        assert_eq!(
            &code[landing..landing + 2],
            &[0x03, 0xd9],
            "jge lands on add ebx, ecx"
        );
        assert_eq!(
            &code[jge + 2..jge + 8],
            &[0x8b, 0x0d, 0x11, 0x11, 0x11, 0x11]
        );
        assert_eq!(&code[landing + 2..], &[0xff, 0x25, 0x22, 0x22, 0x22, 0x22]);
        assert!(STEP_STOLEN.len() >= 5, "room for the jump");
    }

    #[test]
    fn font_arguments_parse_as_typed() {
        assert_eq!(parse_args(&argv(&["default"])), Ok(None));
        assert_eq!(
            parse_args(&argv(&["Verdana", "28", "bold", "shadow"])),
            Ok(Some(FontSpec {
                face: "Verdana".into(),
                tall: 28,
                bold: true,
                shadow: true,
                outline: false,
            }))
        );
        // An unquoted multi-word face.
        let spec = parse_args(&argv(&["Times", "New", "Roman", "20"]))
            .unwrap()
            .unwrap();
        assert_eq!(spec.face, "Times New Roman");
        assert_eq!(spec.to_string(), "\"Times New Roman\" 20");
        assert_eq!(
            parse_args(&argv(&["Verdana", "28", "BOLD", "Outline"]))
                .unwrap()
                .unwrap()
                .to_string(),
            "\"Verdana\" 28 bold outline"
        );
        assert!(parse_args(&argv(&["Verdana"])).is_err(), "no size");
        assert!(parse_args(&argv(&["28"])).is_err(), "no face");
        assert!(parse_args(&argv(&["Verdana", "3"])).is_err(), "too small");
        assert!(parse_args(&argv(&["Verdana", "201"])).is_err(), "too big");
        assert!(
            parse_args(&argv(&["Verdana", "28", "italic"])).is_err(),
            "not a flag"
        );
        assert!(parse_args(&argv(&[])).is_err());
    }

    #[test]
    fn flags_and_weight_are_vgui2s() {
        let plain = parse_args(&argv(&["Verdana", "20"])).unwrap().unwrap();
        assert_eq!((plain.flags(), plain.weight()), (0, 0));
        let all = parse_args(&argv(&["Verdana", "20", "bold", "shadow", "outline"]))
            .unwrap()
            .unwrap();
        assert_eq!(all.flags(), 0x80 | 0x200);
        assert_eq!(all.weight(), 700);
    }

    #[test]
    fn con_color_parses_as_the_engine_reads_it() {
        assert_eq!(con_color_from("255 180 30"), Some((255, 180, 30)));
        assert_eq!(con_color_from(" 10  20  300 "), Some((10, 20, 255)));
        assert_eq!(con_color_from("255 180"), None);
        assert_eq!(con_color_from("255 180 30 1"), None);
        assert_eq!(con_color_from("red"), None);
        assert_eq!(CON_COLOR_DEFAULT, (255, 180, 30));
    }

    #[test]
    fn colour_components_truncate_to_bytes() {
        assert_eq!(color_byte(1.0), 255);
        assert_eq!(color_byte(0.0), 0);
        assert_eq!(color_byte(0.5), 127);
        assert_eq!(color_byte(2.0), 255);
        assert_eq!(color_byte(-1.0), 0);
        assert_eq!(color_byte(f32::NAN), 0);
    }

    #[test]
    fn names_become_terminated_utf16_with_controls_as_spaces() {
        assert_eq!(
            to_wide(b"m00cat"),
            vec![0x6d, 0x30, 0x30, 0x63, 0x61, 0x74, 0]
        );
        // UTF-8, as the engine converts it.
        assert_eq!(to_wide("é".as_bytes()), vec![0xe9, 0]);
        assert_eq!(to_wide(b"a\tb\x01"), vec![0x61, 0x20, 0x62, 0x20, 0]);
        assert_eq!(to_wide(b""), vec![0]);
    }

    #[test]
    fn status_says_the_games_font_until_one_is_set() {
        assert_eq!(describe(), "font = the game's");
    }
}
