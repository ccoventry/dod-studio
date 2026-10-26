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
//! a later vgui2 does, so `outline` is accepted and said to do nothing; the
//! `outline` subcommand draws one by hand (`backdrop.rs`).
//!
//! ## Characters past Latin-1 (#1052)
//!
//! The font manager (`CFontManager::AddGlyphSetToFont`, both builds) gives
//! the face only U+0000..U+00FF and draws everything above from `Tahoma`,
//! unless the face is `Tahoma` itself. And `GetTextSize` is not what
//! `DrawPrintText` draws: it skips `&` and every character `iswprint`
//! rejects (`⅞`, `★`, `™`, `€`, emoji halves), which `DrawUnicodeChar`
//! still draws and advances past by its `GetCharABCwide` a+b+c. A name
//! holding one measured short by that character, so the killer's ran into
//! the icon and the victim's off the screen.
//!
//! So the width is that same a+b+c sum over the characters printed
//! ([`drawn_width`]), and before that a character with a plain Latin-1
//! compatibility form becomes it ([`push_compat_form`]: `⅞` is `7/8`), so it
//! is drawn in the chosen face rather than Tahoma's. The game's own font path
//! gets the same forms while the sites are ours: its draw turns anything
//! `iswprint` rejects into a space, so `⅞` showed as a blank there.
//!
//! The font manager's split is its own choice, not the face's: the surface's
//! `AddGlyphSetToFont` drops the range arguments, and the manager makes the
//! face's handle two ranges, 0..0xff from the face and 0x100..0xffff from
//! Tahoma (only `Marlett` and `Tahoma` get the whole range). So once our
//! handle is made, `hook::widen` raises its first range to 0xffff (one u32
//! in the manager's data, and only when the two ranges are exactly that
//! split), and the names draw in runs ([`font_runs`]): the face's handle for
//! what the face has (`GetFontUnicodeRanges`, [`Coverage`]), a Tahoma handle
//! for the rest, as before (#1054). A compatibility form is then used only
//! for a character the face lacks: Bahnschrift's own `™` stays `™`.

// The patching runs only in the 32-bit DLL; a host build (workspace clippy,
// tests) keeps the tables and helpers for the unit tests alone.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code, unused_imports))]

use std::borrow::Cow;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Mutex, RwLock};

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

/// The face the font manager draws U+0100 and up from, whatever the face
/// asked for; given it, a face covers every character itself.
const FALLBACK_FACE: &str = "Tahoma";

const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
const ROMAN: [&str; 16] = [
    "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII", "L", "C", "D", "M",
];
const ROMAN_SMALL: [&str; 16] = [
    "i", "ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x", "xi", "xii", "l", "c", "d", "m",
];

/// Appends `c`'s Unicode compatibility form (NFKD) when that is plain
/// Latin-1, which the chosen face draws itself, and says whether it did. Only
/// forms that keep the meaning: fractions (`⅞` is `7/8`, the fraction slash
/// read as `/`), super- and subscript digits, `™`, Roman numerals, Latin
/// ligatures, fullwidth ASCII and the wide spaces. A letter with an accent
/// keeps it: Tahoma draws `ł` well, and `l` would be another name.
fn push_compat_form(c: char, out: &mut String) -> bool {
    let cp = c as u32;
    let at = |first: u32| (cp - first) as usize;
    let form: &str = match cp {
        0x2000..=0x200a | 0x202f | 0x205f | 0x3000 => " ",
        0x2024 => ".",
        0x2025 => "..",
        0x2026 => "...",
        0x2044 | 0x2215 => "/",
        0x2070 => "0",
        0x2071 => "i",
        0x2074..=0x2079 => DIGITS[at(0x2070)],
        0x207a | 0x208a => "+",
        0x207b | 0x208b => "-",
        0x207c | 0x208c => "=",
        0x207d | 0x208d => "(",
        0x207e | 0x208e => ")",
        0x207f => "n",
        0x2080..=0x2089 => DIGITS[at(0x2080)],
        0x2116 => "No",
        0x2120 => "SM",
        0x2122 => "TM",
        0x2150 => "1/7",
        0x2151 => "1/9",
        0x2152 => "1/10",
        0x2153 => "1/3",
        0x2154 => "2/3",
        0x2155 => "1/5",
        0x2156 => "2/5",
        0x2157 => "3/5",
        0x2158 => "4/5",
        0x2159 => "1/6",
        0x215a => "5/6",
        0x215b => "1/8",
        0x215c => "3/8",
        0x215d => "5/8",
        0x215e => "7/8",
        0x215f => "1/",
        0x2160..=0x216f => ROMAN[at(0x2160)],
        0x2170..=0x217f => ROMAN_SMALL[at(0x2170)],
        0x2189 => "0/3",
        0xfb00 => "ff",
        0xfb01 => "fi",
        0xfb02 => "fl",
        0xfb03 => "ffi",
        0xfb04 => "ffl",
        0xfb05 | 0xfb06 => "st",
        // Fullwidth `!`..`~`, 0xfee0 above their ASCII.
        0xff01..=0xff5e => {
            out.push(char::from_u32(cp - 0xfee0).unwrap_or(c));
            return true;
        }
        _ => return false,
    };
    out.push_str(form);
    true
}

/// `text` with every character above Latin-1 that has a compatibility form
/// ([`push_compat_form`]) replaced by it; borrowed when there is none.
fn with_compat_forms(text: &str) -> Cow<'_, str> {
    with_compat_forms_where(text, |_| false)
}

/// [`with_compat_forms`], leaving alone the characters `drawn` says the
/// chosen face draws itself (Bahnschrift's own `™` stays `™`).
fn with_compat_forms_where(text: &str, drawn: impl Fn(char) -> bool) -> Cow<'_, str> {
    let swapped = |c: char| c > '\u{ff}' && !drawn(c) && push_compat_form(c, &mut String::new());
    let Some(first) = text
        .char_indices()
        .find(|&(_, c)| swapped(c))
        .map(|(i, _)| i)
    else {
        return Cow::Borrowed(text);
    };
    let mut out = String::with_capacity(text.len() + 8);
    out.push_str(&text[..first]);
    for c in text[first..].chars() {
        if c <= '\u{ff}' || drawn(c) || !push_compat_form(c, &mut out) {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

/// Which UTF-16 code units the chosen face has a glyph for, one bit each:
/// what `GetFontUnicodeRanges` reports for it (#1054). Surrogates are never
/// in it, so half an emoji goes to the fallback face as it always did.
pub(super) struct Coverage(Box<[u64; 1024]>);

impl Coverage {
    /// From `GetFontUnicodeRanges`' `(first, count)` pairs.
    fn from_ranges(ranges: impl IntoIterator<Item = (u16, u16)>) -> Self {
        let mut bits = Box::new([0u64; 1024]);
        for (low, count) in ranges {
            let end = (u32::from(low) + u32::from(count)).min(0x1_0000);
            for unit in u32::from(low)..end {
                if !(0xd800..=0xdfff).contains(&unit) {
                    bits[(unit >> 6) as usize] |= 1 << (unit & 63);
                }
            }
        }
        Coverage(bits)
    }

    fn has(&self, unit: u16) -> bool {
        self.0[usize::from(unit >> 6)] & (1 << (unit & 63)) != 0
    }
}

/// Splits `chars` (up to a terminator) into runs drawn with one font each:
/// `true` for the chosen face, `false` for the fallback. Latin-1 is always
/// the face's, as the font manager has it; above that, the face's when it has
/// the glyph (`has`), else the fallback's, which is what drew it before.
fn font_runs(chars: &[u16], has: impl Fn(u16) -> bool) -> Vec<(bool, std::ops::Range<usize>)> {
    let mut runs: Vec<(bool, std::ops::Range<usize>)> = Vec::new();
    for (i, &unit) in chars.iter().take_while(|&&c| c != 0).enumerate() {
        let face = unit <= 0xff || has(unit);
        match runs.last_mut() {
            Some((f, range)) if *f == face => range.end = i + 1,
            _ => runs.push((face, i..i + 1)),
        }
    }
    runs
}

/// `CFontManager`'s data, the same on both builds (the verify script checks
/// it): a `CUtlVector<CFontAmalgam>` first, whose element count is at +0xc;
/// each amalgam 0x3c bytes with its own `CUtlVector<TFontRange>` first; each
/// range `{ low, high, CWin32Font* }`, and `GetFontForChar` takes the first
/// range holding the character.
const VECTOR_COUNT_AT: usize = 0xc;
const AMALGAM_SIZE: usize = 0x3c;
const RANGE_SIZE: usize = 12;

/// Where the `AddGlyphSetToFont` slot calls `FontManager()`: the thunk on
/// both builds is `call FontManager; mov ecx, eax; call
/// CFontManager::AddGlyphSetToFont`. The offset of that first `call` and its
/// displacement.
fn manager_call(thunk: &[u8]) -> Option<(usize, i32)> {
    thunk
        .windows(8)
        .position(|w| w[0] == 0xe8 && w[5..8] == [0x8b, 0xc8, 0xe8])
        .map(|at| {
            let rel =
                i32::from_le_bytes([thunk[at + 1], thunk[at + 2], thunk[at + 3], thunk[at + 4]]);
            (at, rel)
        })
}

/// `FontManager()` is `mov eax, imm32; ret` on both builds: the manager.
fn returns_constant(code: &[u8]) -> Option<u32> {
    match code {
        [0xb8, a, b, c, d, 0xc3, ..] => Some(u32::from_le_bytes([*a, *b, *c, *d])),
        _ => None,
    }
}

/// Whether a fresh handle's two ranges are what `AddGlyphSetToFont` makes
/// of a face other than Tahoma: the face over 0..0xff, Tahoma over
/// 0x100..0xffff. Anything else is left alone.
fn split_as_expected(ranges: [u32; 6]) -> bool {
    let [low0, high0, face, low1, high1, fallback] = ranges;
    (low0, high0, low1, high1) == (0, 0xff, 0x100, 0xffff)
        && face != 0
        && fallback != 0
        && face != fallback
}

/// A name as the engine's own font gets it while the sites are ours: the
/// compatibility forms, or `None` to pass the game's bytes on unchanged.
/// Bytes that are not UTF-8 go on as they are; the engine converts them.
fn engine_name(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.is_ascii() {
        return None;
    }
    match with_compat_forms(std::str::from_utf8(bytes).ok()?) {
        Cow::Borrowed(_) => None,
        Cow::Owned(text) => Some(text.into_bytes()),
    }
}

/// The engine's conversion, near enough: UTF-8 to UTF-16 (`ConvertANSIToUnicode`
/// is `MultiByteToWideChar(CP_UTF8, ...)` on both builds), control characters
/// drawn as spaces as the engine does with anything `iswprint` rejects, and a
/// terminator. With `compat`, the characters with a Latin-1 compatibility
/// form become it first ([`with_compat_forms`]).
fn to_wide(bytes: &[u8], compat: bool) -> Vec<u16> {
    to_wide_where(bytes, compat, |_| false)
}

/// [`to_wide`], keeping the characters `drawn` says the face draws itself.
fn to_wide_where(bytes: &[u8], compat: bool, drawn: impl Fn(char) -> bool) -> Vec<u16> {
    let text = String::from_utf8_lossy(bytes);
    let text = if compat {
        with_compat_forms_where(&text, drawn)
    } else {
        Cow::Borrowed(&*text)
    };
    let mut out: Vec<u16> = text
        .encode_utf16()
        .map(|c| if c < 0x20 { 0x20 } else { c })
        .collect();
    out.push(0);
    out
}

/// What `DrawPrintText` advances past for `chars` (up to a terminator):
/// `DrawUnicodeChar` moves on by each character's `GetCharABCwide` a+b+c on
/// both builds, `&` and characters `iswprint` rejects included. `GetTextSize`
/// skips those two, so it measured such a name short (#1052).
fn drawn_width(chars: &[u16], mut abc: impl FnMut(u16) -> (i32, i32, i32)) -> i32 {
    chars
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| {
            let (a, b, c) = abc(c);
            a.saturating_add(b).saturating_add(c)
        })
        .fold(0i32, i32::saturating_add)
        .max(0)
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
/// The same colour as the game passed it, as `f32` bits, so the engine's
/// own font can be given it again exactly after the outline's passes.
static COLOR_F: [AtomicU32; 3] = [const { AtomicU32::new(0) }; 3];
/// Whether our font's names get their compatibility forms: false for a face
/// that draws every character itself ([`FALLBACK_FACE`]).
static COMPAT: AtomicBool = AtomicBool::new(true);
/// The fallback face's handle while the chosen face covers more than
/// Latin-1 (#1054), 0 otherwise: names then draw in runs, the face's own
/// glyphs with [`FONT`] and the rest with this.
static FALLBACK: AtomicU32 = AtomicU32::new(0);
/// What the chosen face has, while [`FALLBACK`] is set.
static COVERAGE: RwLock<Option<Coverage>> = RwLock::new(None);
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

/// The names' height: our font's, or the engine font's as its length call
/// last reported it (0 before the first line).
pub(super) fn text_tall() -> i32 {
    match FONT_TALL.load(Ordering::Relaxed) {
        0 => ENGINE_TALL.load(Ordering::Relaxed),
        tall => tall,
    }
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
    /// What `DrawUnicodeChar` advances by (`call [eax+0xfc]` in it, both
    /// builds); see [`drawn_width`].
    const SURFACE_GET_CHAR_ABC_WIDE: usize = 63;

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
    /// `(font, ch, &a, &b, &c)`.
    type CharAbcFn =
        unsafe extern "thiscall" fn(*mut c_void, u32, i32, *mut i32, *mut i32, *mut i32);

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
    /// a scale or a font is in force. `Draw` anchors the line at
    /// `ScreenWidth - icon - victim width`, so the extra width moves the whole line left
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
        let screen = screen_width();
        // An outline or a box reaches past the name: keep that clear too.
        let margin =
            super::super::scale::right_margin(screen, tall) + super::super::backdrop::edge_extra();
        // Safety: the caller's out-parameter, which `string_len` just wrote.
        unsafe { *width += margin };
        super::super::backdrop::line_started(screen - margin);
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
        use super::super::{backdrop, scale};

        let font = FONT.load(Ordering::Acquire);
        // The colour the game gave this name, if it gave one; the engine
        // resets to `con_color` after every string either way.
        let asked = COLOR.swap(-1, Ordering::AcqRel);
        let name_rgb = || match asked {
            -1 => con_color(),
            packed => ((packed >> 16) & 0xff, (packed >> 8) & 0xff, packed & 0xff),
        };
        let line_y = y;
        let text_tall = text_tall();
        let y = scale::name_draw_y(y, text_tall);
        let backdrop_on = backdrop::active();
        if backdrop_on {
            // The killer's name is a line's first draw; the box goes under it.
            // Safety: inside the HUD draw, on the engine's thread.
            unsafe { backdrop::line_box_at(x, line_y, scale::line_height(text_tall)) };
        }
        if font == 0 {
            let Some(e) = engine::engfuncs() else {
                return x;
            };
            // Kept alive past every engine call below that reads it.
            let shown = engine_text(text);
            let text = shown.as_ref().map_or(text, |s| s.as_ptr());
            if backdrop_on && !text.is_null() {
                if backdrop::name_boxes() {
                    let (mut w, mut h) = (0, 0);
                    unsafe { (e.pfn_draw_console_string_len)(text, &mut w, &mut h) };
                    let line = (line_y, scale::line_height(text_tall));
                    unsafe { backdrop::name_box_at(name_rgb(), x, w, y, text_tall, line) };
                }
                let (px, (r, g, b), _alpha) = backdrop::outline();
                if px > 0 {
                    // The engine's call takes no alpha: the outline is opaque.
                    let to_f = |v: i32| v as f32 / 255.0;
                    for dy in -px..=px {
                        for dx in -px..=px {
                            if backdrop::in_outline(dx, dy, px) {
                                unsafe {
                                    (e.pfn_draw_set_text_color)(to_f(r), to_f(g), to_f(b));
                                    (e.pfn_draw_console_string)(x + dx, y + dy, text);
                                }
                            }
                        }
                    }
                    if asked != -1 {
                        let f = |i: usize| f32::from_bits(COLOR_F[i].load(Ordering::Relaxed));
                        unsafe { (e.pfn_draw_set_text_color)(f(0), f(1), f(2)) };
                    }
                }
            }
            return unsafe { (e.pfn_draw_console_string)(x, y, text) };
        }
        let Ok(surface) = surface() else {
            return x;
        };
        if text.is_null() {
            return x;
        }
        // Safety: the game's own `szKiller`/`szVictim`, NUL-terminated by `MsgFunc_DeathMsg`.
        let wide = name_wide(unsafe { CStr::from_ptr(text) }.to_bytes());
        let (r, g, b) = name_rgb();
        let runs = runs_of(surface, font, &wide);
        let width = runs.iter().map(|run| run.2).fold(0i32, i32::saturating_add);
        if backdrop_on && backdrop::name_boxes() {
            let line = (line_y, scale::line_height(text_tall));
            unsafe { backdrop::name_box_at((r, g, b), x, width, y, text_tall, line) };
        }
        let (outline_px, (or, og, ob), outline_alpha) = if backdrop_on {
            backdrop::outline()
        } else {
            (0, (0, 0, 0), 0)
        };
        // Safety: the engine's own text path makes these same calls from
        // `HUD_Redraw`; the slots are checked by ret size on both builds.
        unsafe {
            let set_font: SetFontFn = slot(surface, SURFACE_DRAW_SET_TEXT_FONT);
            let set_pos: SetPosFn = slot(surface, SURFACE_DRAW_SET_TEXT_POS);
            let set_color: SetColorFn = slot(surface, SURFACE_DRAW_SET_TEXT_COLOR_RGBA);
            let print: PrintTextFn = slot(surface, SURFACE_DRAW_PRINT_TEXT);
            let flush: FlushFn = slot(surface, SURFACE_DRAW_FLUSH_TEXT);
            // Each run in its own font, starting where the last one ended.
            let print_at = |x: i32, y: i32| {
                let mut at = x;
                for (handle, range, run_width) in &runs {
                    set_font(surface, *handle);
                    set_pos(surface, at, y);
                    print(surface, wide[range.start..].as_ptr(), range.len() as i32);
                    at = at.saturating_add(*run_width);
                }
            };
            if outline_px > 0 {
                set_color(surface, or, og, ob, outline_alpha);
                for dy in -outline_px..=outline_px {
                    for dx in -outline_px..=outline_px {
                        if backdrop::in_outline(dx, dy, outline_px) {
                            print_at(x + dx, y + dy);
                        }
                    }
                }
            }
            set_color(surface, r, g, b, 255);
            print_at(x, y);
            flush(surface);
        }
        x + width
    }

    /// Stands in for `pfnDrawSetTextColor` before each name: remembers the
    /// colour for [`draw_string`] (our font's colour, a team box's shade, and
    /// the engine font's colour again after an outline) and still tells the
    /// engine, so its own state is what it would have been.
    unsafe extern "C" fn set_text_color(r: f32, g: f32, b: f32) {
        for (slot, v) in COLOR_F.iter().zip([r, g, b]) {
            slot.store(v.to_bits(), Ordering::Relaxed);
        }
        let packed = (color_byte(r) << 16) | (color_byte(g) << 8) | color_byte(b);
        COLOR.store(packed, Ordering::Release);
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
                let shown = engine_text(text);
                let text = shown.as_ref().map_or(text, |s| s.as_ptr());
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
            name_wide(unsafe { CStr::from_ptr(text) }.to_bytes())
        };
        let w = runs_of(surface, font, &wide)
            .iter()
            .map(|run| run.2)
            .fold(0i32, i32::saturating_add);
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

    /// How far `DrawPrintText` moves on for `wide` in `font`: [`drawn_width`]
    /// over the surface's own `GetCharABCwide`, the call `DrawUnicodeChar`
    /// advances by. Not `GetTextSize`, which skips characters it still draws.
    fn text_width(surface: *mut c_void, font: u32, wide: &[u16]) -> i32 {
        // Safety: slot checked by ret size on both builds; the out-parameters are locals.
        let abc: CharAbcFn = unsafe { slot(surface, SURFACE_GET_CHAR_ABC_WIDE) };
        drawn_width(wide, |ch| {
            let (mut a, mut b, mut c) = (0i32, 0i32, 0i32);
            unsafe { abc(surface, font, i32::from(ch), &mut a, &mut b, &mut c) };
            (a, b, c)
        })
    }

    /// A name as our font draws it: UTF-16 with the compatibility forms of
    /// the characters the face lacks ([`to_wide_where`]).
    fn name_wide(bytes: &[u8]) -> Vec<u16> {
        let compat = COMPAT.load(Ordering::Relaxed);
        if FALLBACK.load(Ordering::Acquire) == 0 {
            return to_wide(bytes, compat);
        }
        let Ok(coverage) = COVERAGE.read() else {
            return to_wide(bytes, compat);
        };
        to_wide_where(bytes, compat, |c| {
            u16::try_from(u32::from(c))
                .is_ok_and(|unit| coverage.as_ref().is_some_and(|cov| cov.has(unit)))
        })
    }

    /// `wide`'s runs with the handle each is drawn with and its width: one
    /// run in `font` unless the face covers more than Latin-1 (#1054).
    fn runs_of(
        surface: *mut c_void,
        font: u32,
        wide: &[u16],
    ) -> Vec<(u32, std::ops::Range<usize>, i32)> {
        let chars = wide.iter().take_while(|&&c| c != 0).count();
        let fallback = FALLBACK.load(Ordering::Acquire);
        let split = if fallback == 0 {
            None
        } else {
            COVERAGE.read().ok().and_then(|cov| {
                cov.as_ref()
                    .map(|cov| font_runs(wide, |unit| cov.has(unit)))
            })
        };
        let runs = split.unwrap_or_else(|| vec![(true, 0..chars)]);
        runs.into_iter()
            .map(|(face, range)| {
                let handle = if face { font } else { fallback };
                let width = text_width(surface, handle, &wide[range.clone()]);
                (handle, range, width)
            })
            .collect()
    }

    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn CreateCompatibleDC(hdc: *mut c_void) -> *mut c_void;
        fn DeleteDC(hdc: *mut c_void) -> i32;
        #[allow(clippy::too_many_arguments)]
        fn CreateFontA(
            height: i32,
            width: i32,
            escapement: i32,
            orientation: i32,
            weight: i32,
            italic: u32,
            underline: u32,
            strike_out: u32,
            charset: u32,
            out_precision: u32,
            clip_precision: u32,
            quality: u32,
            pitch_and_family: u32,
            face: *const c_char,
        ) -> *mut c_void;
        fn SelectObject(hdc: *mut c_void, object: *mut c_void) -> *mut c_void;
        fn DeleteObject(object: *mut c_void) -> i32;
        fn GetFontUnicodeRanges(hdc: *mut c_void, glyph_set: *mut u32) -> u32;
    }

    /// What the face has, from Windows (`GetFontUnicodeRanges`): the same
    /// face the engine's font manager asked `CreateFontA` for, so the same
    /// substitute when Windows has no such face.
    fn face_coverage(spec: &FontSpec) -> Result<Coverage, String> {
        const DEFAULT_CHARSET: u32 = 1;
        let face = CString::new(spec.face.as_str()).map_err(|_| "the face has a NUL in it")?;
        // Safety: a memory DC and a font of our own, freed before returning.
        unsafe {
            let dc = CreateCompatibleDC(std::ptr::null_mut());
            if dc.is_null() {
                return Err("no memory DC".to_string());
            }
            let font = CreateFontA(
                spec.tall,
                0,
                0,
                0,
                spec.weight(),
                0,
                0,
                0,
                DEFAULT_CHARSET,
                0,
                0,
                0,
                0,
                face.as_ptr(),
            );
            if font.is_null() {
                DeleteDC(dc);
                return Err(format!("Windows could not create {:?}", spec.face));
            }
            let old = SelectObject(dc, font);
            let size = GetFontUnicodeRanges(dc, std::ptr::null_mut()) as usize;
            // `GLYPHSET`: four u32s, then one `WCRANGE` (u16 low, u16 count) per range.
            let mut set = vec![0u32; size.div_ceil(4).max(4)];
            let got = if size >= 16 {
                GetFontUnicodeRanges(dc, set.as_mut_ptr())
            } else {
                0
            };
            SelectObject(dc, old);
            DeleteObject(font);
            DeleteDC(dc);
            if got == 0 {
                return Err("GetFontUnicodeRanges failed".to_string());
            }
            let ranges = (set[3] as usize).min(set.len() - 4);
            Ok(Coverage::from_ranges(
                set[4..4 + ranges]
                    .iter()
                    .map(|&w| ((w & 0xffff) as u16, (w >> 16) as u16)),
            ))
        }
    }

    /// The engine's `CFontManager`, found through the `AddGlyphSetToFont`
    /// slot's thunk ([`manager_call`], [`returns_constant`]).
    fn font_manager(surface: *mut c_void) -> Result<usize, String> {
        // Safety: the surface's vftable, checked on both builds.
        let thunk: usize = unsafe { slot(surface, SURFACE_ADD_GLYPH_SET_TO_FONT) };
        const THUNK_LEN: usize = 0x40;
        if !crate::crash::readable(thunk, THUNK_LEN) {
            return Err("the AddGlyphSetToFont thunk is not readable".to_string());
        }
        // Safety: checked readable just above.
        let code = unsafe { std::slice::from_raw_parts(thunk as *const u8, THUNK_LEN) };
        let (at, rel) =
            manager_call(code).ok_or("the AddGlyphSetToFont thunk has no FontManager() call")?;
        let target = (thunk + at + 5).wrapping_add(rel as usize);
        if !crate::crash::readable(target, 6) {
            return Err(format!("FontManager() at {target:#x} is not readable"));
        }
        // Safety: checked readable just above.
        let body = unsafe { std::slice::from_raw_parts(target as *const u8, 6) };
        returns_constant(body).map(|m| m as usize).ok_or_else(|| {
            format!("FontManager() at {target:#x} is not `mov eax, imm32; ret`: {body:02x?}")
        })
    }

    /// Reads a u32 the engine owns, refusing memory that is not committed.
    fn read_u32(address: usize) -> Result<u32, String> {
        if !crate::crash::readable(address, 4) {
            return Err(format!("{address:#x} is not readable"));
        }
        // Safety: committed, checked just above.
        Ok(unsafe { std::ptr::read_unaligned(address as *const u32) })
    }

    /// Gives the face in `handle` the whole of U+0000..U+FFFF: its first
    /// range's upper bound, 0xff, becomes 0xffff, so `GetFontForChar` never
    /// reaches Tahoma's. Only for a handle whose ranges are exactly what the
    /// font manager makes ([`split_as_expected`]); anything else is left
    /// alone and the reason returned.
    fn widen(manager: usize, handle: u32) -> Result<(), String> {
        let amalgams = read_u32(manager)? as usize;
        let count = read_u32(manager + VECTOR_COUNT_AT)?;
        if handle >= count || amalgams == 0 {
            return Err(format!(
                "handle {handle} is not among the manager's {count} fonts"
            ));
        }
        let amalgam = amalgams + handle as usize * AMALGAM_SIZE;
        let ranges_at = read_u32(amalgam)? as usize;
        let ranges = read_u32(amalgam + VECTOR_COUNT_AT)?;
        if ranges != 2 || !crate::crash::readable(ranges_at, 2 * RANGE_SIZE) {
            return Err(format!(
                "handle {handle} has {ranges} ranges, not the face's and Tahoma's"
            ));
        }
        let mut found = [0u32; 6];
        for (i, v) in found.iter_mut().enumerate() {
            *v = read_u32(ranges_at + 4 * i)?;
        }
        if !split_as_expected(found) {
            return Err(format!(
                "handle {handle}'s ranges are {found:x?}, not 0..ff face, 100..ffff Tahoma"
            ));
        }
        // Safety: the first range's `high`, inside the vector just read; the
        // game's thread, the only one that draws or makes fonts.
        unsafe { std::ptr::write_volatile((ranges_at + 4) as *mut u32, 0xffff) };
        Ok(())
    }

    /// Makes `handle` (the chosen face, just made) draw every character the
    /// face has, with a Tahoma handle for the rest (#1054). Returns how many
    /// characters past Latin-1 the face covers.
    fn widen_to_face(surface: *mut c_void, handle: u32, spec: &FontSpec) -> Result<usize, String> {
        let coverage = face_coverage(spec)?;
        let manager = font_manager(surface)?;
        // Safety: the surface's own font calls, slots checked on both builds.
        let fallback = unsafe {
            let create: CreateFontFn = slot(surface, SURFACE_CREATE_FONT);
            let add: AddGlyphSetFn = slot(surface, SURFACE_ADD_GLYPH_SET_TO_FONT);
            let fallback = create(surface);
            let face = CString::new(FALLBACK_FACE).unwrap_or_default();
            let ok = fallback != 0
                && add(
                    surface,
                    fallback,
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
                    "the engine would not make a {FALLBACK_FACE} font to fall back on"
                ));
            }
            fallback
        };
        widen(manager, handle)?;
        let past_latin1 = (0x100u32..0x1_0000)
            .filter(|&u| coverage.has(u as u16))
            .count();
        if let Ok(mut held) = COVERAGE.write() {
            *held = Some(coverage);
        }
        FALLBACK.store(fallback, Ordering::Release);
        Ok(past_latin1)
    }

    /// A name for the engine's own font with its compatibility forms
    /// ([`engine_name`]), or `None` to pass the game's pointer on.
    fn engine_text(text: *const c_char) -> Option<CString> {
        if text.is_null() {
            return None;
        }
        // Safety: the game's own `szKiller`/`szVictim`, NUL-terminated by `MsgFunc_DeathMsg`.
        let bytes = engine_name(unsafe { CStr::from_ptr(text) }.to_bytes())?;
        CString::new(bytes).ok()
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
            FALLBACK.store(0, Ordering::Release);
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
        let compat = !spec.face.eq_ignore_ascii_case(FALLBACK_FACE);
        // The old handle's runs must not be drawn with the new one's coverage.
        FONT.store(0, Ordering::Release);
        FALLBACK.store(0, Ordering::Release);
        let beyond = if compat {
            match widen_to_face(surface, handle, &spec) {
                Ok(n) => format!(
                    " Past Latin-1 it draws the {n} characters it has itself, the rest in {FALLBACK_FACE}."
                ),
                Err(why) => {
                    unsafe {
                        crate::debug::report(&format!(
                            "deathmsg font: characters past Latin-1 stay {FALLBACK_FACE}'s -- {why}"
                        ))
                    };
                    format!(" Past Latin-1 it draws in {FALLBACK_FACE} ({why}).")
                }
            }
        } else {
            String::new()
        };
        COMPAT.store(compat, Ordering::Relaxed);
        // Height first: the stub reads it, and a draw with the new handle
        // must not step by the old one.
        FONT_TALL.store(tall.max(0), Ordering::Release);
        FONT.store(handle, Ordering::Release);
        let outline = if spec.outline {
            " (outline: this engine's font renderer has none, so it is drawn without; `outline <px>` draws one)"
        } else {
            ""
        };
        let reply = format!(
            "font = {spec}, {tall} px tall, handle {handle}{outline}.{beyond} A face Windows does not have falls back to its default; `font default` puts the game's back."
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
            to_wide(b"m00cat", true),
            vec![0x6d, 0x30, 0x30, 0x63, 0x61, 0x74, 0]
        );
        // UTF-8, as the engine converts it.
        assert_eq!(to_wide("é".as_bytes(), true), vec![0xe9, 0]);
        assert_eq!(to_wide(b"a\tb\x01", true), vec![0x61, 0x20, 0x62, 0x20, 0]);
        assert_eq!(to_wide(b"", true), vec![0]);
    }

    fn wide(text: &str) -> String {
        let units = to_wide(text.as_bytes(), true);
        String::from_utf16(&units[..units.len() - 1]).unwrap()
    }

    #[test]
    fn the_seven_eighths_name_is_drawn_in_the_chosen_face() {
        // The WSOD25 player whose name broke the line (#1052): U+215E, e2 85 9e.
        let name = "NoGo ! kroD- \u{215e}";
        assert_eq!(&name.as_bytes()[name.len() - 3..], &[0xe2, 0x85, 0x9e]);
        assert_eq!(wide(name), "NoGo ! kroD- 7/8");
        assert!(wide(name).chars().all(|c| c <= '\u{ff}'));
        // A Tahoma face draws it itself.
        let units = to_wide(name.as_bytes(), false);
        assert_eq!(units[units.len() - 2], 0x215e);
        // And the engine's own font gets the same bytes.
        assert_eq!(
            engine_name(name.as_bytes()).as_deref(),
            Some(&b"NoGo ! kroD- 7/8"[..])
        );
    }

    #[test]
    fn compatibility_forms_keep_the_meaning() {
        for (from, to) in [
            ("\u{215b}\u{215c}\u{215d}", "1/83/85/8"),
            ("1\u{2044}2", "1/2"),
            ("x\u{00b2}\u{2074}\u{2089}", "x\u{00b2}49"),
            ("Brand\u{2122}", "BrandTM"),
            ("\u{2163} \u{216b} \u{2170}", "IV XII i"),
            ("\u{fb01}re", "fire"),
            ("\u{ff21}\u{ff42}\u{ff10}\u{ff01}\u{3000}x", "Ab0! x"),
            ("wait\u{2026}", "wait..."),
            ("No\u{2116}1", "NoNo1"),
        ] {
            assert_eq!(wide(from), to, "{from:?}");
        }
    }

    #[test]
    fn characters_without_a_plain_form_are_left_alone() {
        // Accented letters stay themselves (Tahoma draws them), as do symbols
        // with no Latin-1 form, CJK and emoji: measured as drawn instead.
        for name in [
            "\u{0141}ukasz",
            "\u{2605}ace\u{2605}",
            "\u{4e2d}\u{6587}",
            "\u{1f600}",
            "\u{20ac}100",
            "caf\u{e9} & bar \u{00bd}",
        ] {
            assert_eq!(wide(name), name, "{name:?}");
            assert_eq!(engine_name(name.as_bytes()), None, "{name:?}");
        }
        assert!(matches!(with_compat_forms("m00cat"), Cow::Borrowed(_)));
        assert_eq!(engine_name(b"m00cat"), None);
        // Not UTF-8: the engine converts it as it always did.
        assert_eq!(engine_name(b"bad \xe2\x85"), None);
    }

    #[test]
    fn every_form_is_latin1_and_every_listed_character_has_one() {
        for cp in (0x100..0x1_0000).filter_map(char::from_u32) {
            let mut out = String::new();
            if push_compat_form(cp, &mut out) {
                assert!(!out.is_empty(), "{cp:?}");
                assert!(
                    out.chars().all(|c| (' '..='\u{ff}').contains(&c)),
                    "{cp:?} -> {out:?}"
                );
            }
        }
        for c in ['\u{215e}', '\u{2044}', '\u{2122}', '\u{ff5e}', '\u{217f}'] {
            assert!(push_compat_form(c, &mut String::new()), "{c:?}");
        }
        // Latin-1 itself is never rewritten.
        for c in (0x20u8..=0xff).map(char::from) {
            assert_eq!(wide(&c.to_string()), c.to_string());
        }
    }

    #[test]
    fn the_width_is_what_draw_print_text_advances_past() {
        // A made-up font: 10 px a character, `&` and `⅞` included, with a
        // negative a and c to show all three parts count.
        let abc = |c: u16| {
            if c == u16::from(b'i') {
                (-1, 4, -1)
            } else {
                (1, 8, 1)
            }
        };
        let units = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
        assert_eq!(drawn_width(&units("ab"), abc), 20);
        assert_eq!(drawn_width(&units("a&b"), abc), 30, "GetTextSize skips &");
        assert_eq!(
            drawn_width(&units("kroD- \u{215e}"), abc),
            70,
            "nor a character iswprint rejects"
        );
        assert_eq!(drawn_width(&units("ii"), abc), 4);
        assert_eq!(drawn_width(&units(""), abc), 0);
        // Stops at the terminator, as the draw does.
        assert_eq!(drawn_width(&[0x61, 0, 0x62], abc), 10);
        assert_eq!(drawn_width(&units("i"), |_| (-5, 1, -5)), 0);
    }

    #[test]
    fn the_font_manager_is_found_through_both_builds_thunks() {
        // `AddGlyphSetToFont`'s slot as shipped (hw.dll +0x3d90 PRE,
        // +0x22cf60 Anniversary): `call FontManager; mov ecx, eax; call ...`.
        let pre = [
            0x55, 0x8b, 0xec, 0x8b, 0x45, 0x20, 0x8b, 0x4d, 0x1c, 0x8b, 0x55, 0x18, 0x50, 0x8b,
            0x45, 0x14, 0x51, 0x8b, 0x4d, 0x10, 0x52, 0x8b, 0x55, 0x0c, 0x50, 0x8b, 0x45, 0x08,
            0x51, 0x52, 0x50, 0xe8, 0x4c, 0xbc, 0x0c, 0x00, 0x8b, 0xc8, 0xe8, 0xb5, 0xbf, 0x0c,
            0x00, 0x5d, 0xc2, 0x24,
        ];
        let post = [
            0x55, 0x8b, 0xec, 0xff, 0x75, 0x20, 0xff, 0x75, 0x1c, 0xff, 0x75, 0x18, 0xff, 0x75,
            0x14, 0xff, 0x75, 0x10, 0xff, 0x75, 0x0c, 0xff, 0x75, 0x08, 0xe8, 0x43, 0x02, 0x04,
            0x00, 0x8b, 0xc8, 0xe8, 0x8c, 0xfb, 0x03, 0x00, 0x5d, 0xc2, 0x24, 0x00,
        ];
        // PRE: thunk at 0x1d03d90 calls FontManager() at 0x1dcfa00.
        let (at, rel) = manager_call(&pre).unwrap();
        assert_eq!((0x1d0_3d90 + at + 5).wrapping_add(rel as usize), 0x1dc_fa00);
        // Anniversary: 0x1022cf60 calls 0x1026d1c0.
        let (at, rel) = manager_call(&post).unwrap();
        assert_eq!(
            (0x1022_cf60 + at + 5).wrapping_add(rel as usize),
            0x1026_d1c0
        );
        assert_eq!(manager_call(&pre[..0x20]), None, "cut short");
        // `mov eax, 0x24e4ab8; ret` (PRE).
        assert_eq!(
            returns_constant(&[0xb8, 0xb8, 0x4a, 0x4e, 0x02, 0xc3]),
            Some(0x024e_4ab8)
        );
        assert_eq!(
            returns_constant(&[0xb8, 0xb8, 0x4a, 0x4e, 0x02, 0x90]),
            None
        );
        assert_eq!(returns_constant(&[0x55, 0x8b]), None);
    }

    #[test]
    fn only_the_managers_own_split_is_widened() {
        assert!(split_as_expected([0, 0xff, 0x1000, 0x100, 0xffff, 0x2000]));
        // Tahoma itself (one range), a face that failed, the same font twice,
        // or ranges someone else changed: left alone.
        assert!(!split_as_expected([
            0, 0xffff, 0x1000, 0x100, 0xffff, 0x2000
        ]));
        assert!(!split_as_expected([0, 0xff, 0, 0x100, 0xffff, 0x2000]));
        assert!(!split_as_expected([0, 0xff, 0x1000, 0x100, 0xffff, 0x1000]));
        assert!(!split_as_expected([0, 0xff, 0x1000, 0x100, 0xfffe, 0x2000]));
    }

    #[test]
    fn coverage_holds_the_reported_ranges_and_never_a_surrogate() {
        let cov = Coverage::from_ranges([
            (0x20, 0x5f),
            (0x400, 0x100),
            (0xd800, 0x800),
            (0xfff0, 0x20),
        ]);
        assert!(cov.has(u16::from(b'A')) && cov.has(0x7e) && !cov.has(0x7f));
        assert!(cov.has(0x414) && cov.has(0x4ff) && !cov.has(0x500));
        assert!(!cov.has(0xd83d) && !cov.has(0xde00), "half an emoji");
        assert!(cov.has(0xffff), "clipped at the end of the BMP");
    }

    #[test]
    fn names_split_into_runs_by_who_has_the_glyph() {
        let units = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
        // Bahnschrift-like: Latin Extended and Cyrillic, not ★ or emoji.
        let has = |u: u16| (0x100..0x500).contains(&u);
        assert_eq!(font_runs(&units("m00cat"), has), vec![(true, 0..6)]);
        assert_eq!(font_runs(&units("\u{141}ukasz"), has), vec![(true, 0..6)]);
        assert_eq!(
            font_runs(&units("\u{2605}Ace\u{2605}"), has),
            vec![(false, 0..1), (true, 1..4), (false, 4..5)]
        );
        // Both halves of a surrogate pair go to the fallback together.
        assert_eq!(
            font_runs(&units("a\u{1f600}b"), has),
            vec![(true, 0..1), (false, 1..3), (true, 3..4)]
        );
        assert_eq!(font_runs(&units(""), has), vec![]);
        assert_eq!(
            font_runs(&[0x61, 0, 0x2605], has),
            vec![(true, 0..1)],
            "stops at the terminator"
        );
    }

    #[test]
    fn a_character_the_face_has_keeps_its_own_glyph() {
        // Bahnschrift has ™ and €, not ⅞: only ⅞ is swapped.
        let face_has = |c: char| matches!(c, '\u{2122}' | '\u{20ac}');
        assert_eq!(
            with_compat_forms_where("x\u{2122} \u{215e}", face_has),
            "x\u{2122} 7/8"
        );
        assert_eq!(
            with_compat_forms_where("x\u{2122} \u{215e}", |_| false),
            "xTM 7/8"
        );
        assert!(matches!(
            with_compat_forms_where("\u{2122}", face_has),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn status_says_the_games_font_until_one_is_set() {
        assert_eq!(describe(), "font = the game's");
    }
}
