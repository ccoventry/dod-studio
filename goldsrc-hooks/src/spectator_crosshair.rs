//! `dodtools_match_pov_crosshair`: draw the spectator crosshair from the
//! same sprite and the same tile the player's own `cl_xhair_style` picks.
//!
//! ## The two crosshairs are drawn by two different code paths
//!
//! `CHudDoDCrossHair::Draw` branches on the observer-mode global, and the
//! branches do not share a sprite (see `crosshair.rs` for the same fork read
//! from the other side):
//!
//! ```text
//!     mode 0        POV. Reads cl_xhair_style. Non-zero -> client+0x2ced0,
//!                   which draws a 64x64 tile out of customXHair.spr.
//!                   Zero -> client+0x2cda0, the HUD sprite list's crosshair.
//!     mode 3 or 4   client+0x2d1f0. Hardcodes crosshairs.spr and a 24x24
//!                   rect, and reads no cvar at all.
//! ```
//!
//! So a custom crosshair set up for play simply does not appear while
//! spectating, and the spectator one is a 24x24 tile of a 128x128 sprite --
//! which is the "very little to work with" in #291.
//!
//! ## `VidInit` already loaded the sprite
//!
//! Both are loaded unconditionally, into two handles on the same object:
//!
//! ```text
//!     client+0x2cc82  call pfnSPR_Load     ; "sprites/crosshairs.spr"
//!     client+0x2cc88  mov [esi+0x60], eax
//!     client+0x2cca0  call pfnSPR_Load     ; "sprites/customXHair.spr"
//!     client+0x2cca6  mov [esi+0x74], eax
//! ```
//!
//! Nothing needs loading, and no file path is involved. The custom sprite's
//! handle is already sitting at `+0x74`.
//!
//! ## What is patched
//!
//! Five places inside one 57-byte span, `client+0x2d205 .. +0x2d23e`:
//!
//! ```text
//!     +0x2d205  mov dword [ecx+0x64], 0x18   ; left   = 24
//!     +0x2d20c  mov dword [ecx+0x6c], 0      ; top    = 0
//!     +0x2d213  mov dword [ecx+0x68], 0x30   ; right  = 48
//!     +0x2d21a  mov dword [ecx+0x70], 0x18   ; bottom = 24
//!     ...
//!     +0x2d23b  mov eax, [ecx+0x60]          ; <- the sprite handle
//! ```
//!
//! The displacement byte `0x60` becomes `0x74`, and the four rect immediates
//! become the tile `cl_xhair_style` selects. Swapping the handle without the
//! rect would sample a 24x24 corner out of a 256x256 sprite; changing the rect
//! without the handle would sample off the end of a 128x128 one. They are one
//! change, which is why they are one patch.
//!
//! ## The tile arithmetic is DoD's, not ours
//!
//! `client+0x2ced0` -- the POV path, with `cl_xhair_style` in `eax` -- computes
//! its rect like this:
//!
//! ```text
//!     if (style > 16) style = 16;
//!     style--;
//!     col = style % 4;  row = style / 4;
//!     left = col << 6;  right  = (col + 1) << 6;
//!     top  = row << 6;  bottom = (row + 1) << 6;
//! ```
//!
//! A 4x4 grid of 64x64 tiles, which is exactly how the shipped
//! `customXHair.spr` is laid out (256x256, one frame, sixteen crosshairs).
//! [`tile_rect`] reproduces that arithmetic rather than inventing a layout, so
//! the spectator view gets the *same* tile the player sees, whatever they set.
//!
//! ## `cl_xhair_style 0` is not a tile, and is not implemented here
//!
//! The dispatcher at `client+0x2cd5c` checks `style == 0` *before* calling
//! `client+0x2ced0` at all: zero calls `client+0x2cda0` instead, a
//! completely different function that draws the classic four-segment dynamic
//! crosshair (its gap driven by per-frame weapon accuracy fields on the HUD
//! object, and gated by the separate `cl_dynamic_xhair` cvar) rather than
//! blitting a sprite tile. `client+0x2ced0` -- the function this module
//! mirrors -- is never reached for style 0, confirmed by disassembly.
//!
//! Replicating that for a spectated player would mean re-deriving their
//! weapon-accuracy state the same way the animation fix re-derives body
//! animation, and it is not known whether that state is replicated in a demo
//! at all. Out of scope here: this leaves the stock spectator rect in place
//! for style 0 and says so in `dodtools_status` rather than inventing a
//! tile. Filed as a follow-up.
//!
//! ## `cl_xhair_style < 0` draws the whole sheet
//!
//! Unlike the zero case, *every* nonzero style -- negative included -- does
//! reach `client+0x2ced0`, which clamps only at the top (`style > 16` becomes
//! 16) and has no lower clamp at all. For a negative style the resulting
//! `style - 1` feeds a signed mod/div-by-4 that produces rect coordinates
//! outside the sprite. Confirmed live: DoD's own POV view renders that as the
//! entire 256x256 sheet, all sixteen crosshairs at once, rather than clamping
//! or refusing it. [`tile_rect`] reproduces that outcome directly for any
//! negative style, rather than replicating the specific out-of-range
//! arithmetic that happens to produce it.
//!
//! ## It loses to `dodtools_hide_crosshair`
//!
//! That setting stubs `CHudDoDCrossHair::Draw`'s prologue, so neither branch
//! runs and nothing is drawn. Hiding wins by construction, with no interlock
//! needed here: this patches instructions inside a function that is no longer
//! reached. #291 asked for that ordering and this is why it holds.

use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

use crate::crosshair;
use crate::engine;
use crate::names::console_name;
use crate::scan;

/// The cvar name, for status and error text. Registered in `commands.rs`.
pub const NAME: &str = console_name!("match_pov_crosshair");

/// DoD's own cvar, read here rather than mirrored: whatever the player sets for
/// their POV crosshair is what the spectator view should show.
pub const STYLE_CVAR: &str = "cl_xhair_style";

/// The span, with the four rect immediates and the handle displacement
/// wildcarded so one pattern matches both the stock and the patched form.
/// Unique in `client.dll`.
const PATTERN: &str = "C7 41 64 ?? ?? ?? ?? C7 41 6C ?? ?? ?? ?? C7 41 68 ?? ?? ?? ?? \
                       C7 41 70 ?? ?? ?? ?? 8B 41 68 8B 51 70 56 8B 71 64 89 44 24 08 \
                       2B C6 57 8B 79 6C 89 74 24 08 8B F0 8B 41 ??";

/// How long the patched span is.
const SPAN: usize = 57;

/// Where each patched field sits inside the span.
const LEFT_AT: usize = 3;
const TOP_AT: usize = 10;
const RIGHT_AT: usize = 17;
const BOTTOM_AT: usize = 24;
const HANDLE_AT: usize = 56;

/// `[ecx+0x60]` -- `crosshairs.spr`, what the stock code draws.
const STOCK_HANDLE: u8 = 0x60;
/// `[ecx+0x74]` -- `customXHair.spr`, loaded and unused on this path.
const CUSTOM_HANDLE: u8 = 0x74;

/// The stock 24x24 rect, in the span's own field order.
const STOCK_RECT: [i32; 4] = [24, 0, 48, 24];

/// `customXHair.spr` is a 4x4 grid of 64x64 tiles. Both numbers are DoD's --
/// see the module docs -- and the sprite measures 256x256, which is 4 x 64.
const TILE: i32 = 64;
const COLUMNS: i32 = 4;
/// The sprite's full side length, `TILE * COLUMNS`.
const SPRITE_SIDE: i32 = TILE * COLUMNS;
/// The highest style the POV path will honour; it clamps anything above this.
pub const MAX_STYLE: i32 = 16;
/// The canonical `ACTIVE_STYLE` for "whole sheet" -- any negative
/// `cl_xhair_style` collapses to this on read-back, since the rect they
/// produce is identical (see [`tile_rect`]) and there is nothing to
/// distinguish them once written.
pub const WHOLE_SHEET: i32 = -1;
/// The whole sprite, `[left, top, right, bottom]`.
const WHOLE_SHEET_RECT: [i32; 4] = [0, 0, SPRITE_SIDE, SPRITE_SIDE];

/// Resolved address of the span, or 0 before the first successful scan.
static SPAN_ADDRESS: AtomicUsize = AtomicUsize::new(0);

/// The module base it was resolved against. `client.dll` is unloaded and
/// reloaded between demos, so a changed base means rescan.
static SCANNED_BASE: AtomicUsize = AtomicUsize::new(0);

/// The style currently written into the code, or 0 for the stock rect. Read by
/// [`status`] and by `dodtools_status`.
static ACTIVE_STYLE: AtomicI32 = AtomicI32::new(0);

/// The rect DoD's POV path would use for `style`, as `[left, top, right,
/// bottom]` in the order the patched span writes them.
///
/// Mirrors `client+0x2ced0`'s clamp at the top (`style > 16` becomes 16) and
/// its *absence* of one at the bottom -- confirmed by disassembly, not
/// assumed. `client+0x2ced0` is never even called for `style == 0` (the
/// caller at `client+0x2cd5c` branches on that before making the call, taking
/// a completely different function -- see the module docs), so `style == 0`
/// has no tile here either. But every *other* value, including negative
/// ones, does reach `client+0x2ced0`, and its unclamped `style - 1` feeds a
/// signed mod/div-by-4 that produces a rect with negative coordinates for
/// `style < 0`. What that rect renders as in practice was confirmed live:
/// the whole 256x256 sheet, all sixteen crosshairs at once -- consistent with
/// the sprite draw call wrapping on out-of-range texture coordinates rather
/// than clamping. This function reproduces that outcome directly, as
/// [`WHOLE_SHEET_RECT`], rather than replicating the exact out-of-bounds
/// arithmetic that happens to produce it -- the wrap is downstream of the
/// rect this module writes, not something writing the rect can influence.
pub fn tile_rect(style: i32) -> Option<[i32; 4]> {
    if style == 0 {
        return None;
    }
    if style < 0 {
        return Some(WHOLE_SHEET_RECT);
    }
    let index = style.min(MAX_STYLE) - 1;
    let column = index % COLUMNS;
    let row = index / COLUMNS;
    Some([
        column * TILE,
        row * TILE,
        (column + 1) * TILE,
        (row + 1) * TILE,
    ])
}

fn span_address() -> Result<usize, String> {
    let Some(base) = engine::client_module_base() else {
        return Err("client.dll is not loaded yet".to_string());
    };

    if SCANNED_BASE.load(Ordering::Acquire) == base {
        let cached = SPAN_ADDRESS.load(Ordering::Acquire);
        if cached != 0 {
            return Ok(cached);
        }
    }

    // Safety: `client_module_base` only returns a base for a mapped module,
    // and it stays mapped for the session.
    let address = unsafe { scan::find_unique(base, PATTERN) }
        .map_err(|why| format!("could not find the spectator crosshair rect -- {why}"))?;

    SPAN_ADDRESS.store(address, Ordering::Release);
    SCANNED_BASE.store(base, Ordering::Release);
    Ok(address)
}

/// What the bytes currently in the span say. `None` for anything this module
/// did not write and the game did not ship.
fn read_state(present: &[u8]) -> Option<i32> {
    let rect = [
        i32::from_le_bytes(present[LEFT_AT..LEFT_AT + 4].try_into().ok()?),
        i32::from_le_bytes(present[TOP_AT..TOP_AT + 4].try_into().ok()?),
        i32::from_le_bytes(present[RIGHT_AT..RIGHT_AT + 4].try_into().ok()?),
        i32::from_le_bytes(present[BOTTOM_AT..BOTTOM_AT + 4].try_into().ok()?),
    ];
    match present[HANDLE_AT] {
        STOCK_HANDLE if rect == STOCK_RECT => Some(0),
        CUSTOM_HANDLE if rect == WHOLE_SHEET_RECT => Some(WHOLE_SHEET),
        CUSTOM_HANDLE => (1..=MAX_STYLE).find(|style| tile_rect(*style) == Some(rect)),
        _ => None,
    }
}

/// The 57 bytes the span should hold for `style` (0 meaning the stock rect),
/// built by editing what is there rather than from a literal, so nothing
/// between the five patched fields is ever rewritten.
fn wanted(present: &[u8], style: i32) -> Option<Vec<u8>> {
    let (handle, rect) = match style {
        0 => (STOCK_HANDLE, STOCK_RECT),
        _ => (CUSTOM_HANDLE, tile_rect(style)?),
    };
    let mut want = present.to_vec();
    for (at, value) in [LEFT_AT, TOP_AT, RIGHT_AT, BOTTOM_AT].iter().zip(rect) {
        want[*at..*at + 4].copy_from_slice(&value.to_le_bytes());
    }
    want[HANDLE_AT] = handle;
    Some(want)
}

/// DoD's `cl_xhair_style`, truncated and clamped the way the POV path does.
///
/// Only an upper clamp -- `client+0x2ced0` has none at the bottom, so
/// negative values are passed through rather than raised to 0. Clamping them
/// away here would silently show the stock rect for a style that POV renders
/// as the whole sheet.
fn requested_style() -> i32 {
    let Some(engfuncs) = engine::engfuncs() else {
        return 0;
    };
    let Ok(name) = std::ffi::CString::new(STYLE_CVAR) else {
        return 0;
    };
    let value = unsafe { (engfuncs.pfn_get_cvar_float)(name.as_ptr()) };
    if !value.is_finite() {
        return 0;
    }
    (value as i32).min(MAX_STYLE)
}

/// Points the spectator crosshair at `customXHair.spr`, or back at the stock
/// sprite, returning whether anything was written.
///
/// `matching` is the cvar's own sense: `true` makes the spectator crosshair
/// follow `cl_xhair_style`, `false` is the game's stock behaviour.
///
/// Idempotent and cheap to call every frame, which is how `commands::poll` uses
/// it. That is also what makes it track `cl_xhair_style` live, and what makes
/// the setting survive `client.dll` being unloaded and reloaded between demos —
/// a reloaded module comes back stock, and the next frame writes it again.
pub fn set_matching(matching: bool) -> Result<bool, String> {
    let address = span_address()?;
    // Safety: the scan proved `SPAN` bytes of mapped code start here.
    let present = unsafe { std::slice::from_raw_parts(address as *const u8, SPAN) };

    if read_state(present).is_none() {
        return Err(format!(
            "the spectator crosshair rect holds an encoding that is neither DoD's nor one of this module's tiles (sprite displacement {:#04x}) -- something else has patched it",
            present[HANDLE_AT]
        ));
    }

    let style = if matching { requested_style() } else { 0 };
    let Some(want) = wanted(present, style) else {
        return Err(format!("{STYLE_CVAR} resolved to {style}, which is not a tile"));
    };

    // Compared by bytes, not by `style` against `read_state`'s canonical
    // value: every negative style writes the identical whole-sheet rect (see
    // `tile_rect`), so `cl_xhair_style` moving between -1 and -5 must not
    // count as a change needing a write, even though the two numbers differ.
    if present == want.as_slice() {
        ACTIVE_STYLE.store(style, Ordering::Release);
        return Ok(false);
    }

    // Safety: writing to code the scan vouched for, through the same
    // protect/write/restore used everywhere else in this DLL.
    if !unsafe { crate::patch::write_code_bytes(address, &want) } {
        return Err("could not make the spectator crosshair rect writable".to_string());
    }
    ACTIVE_STYLE.store(style, Ordering::Release);
    Ok(true)
}

/// Whether the spectator crosshair is currently drawn from `customXHair.spr`.
pub fn matching() -> bool {
    ACTIVE_STYLE.load(Ordering::Relaxed) != 0
}

/// One line for `dodtools_status`.
///
/// Reports what is patched into the code, which is not the same question as
/// what is on screen: `dodtools_hide_crosshair` stubs `Draw`'s prologue, so
/// none of this ever runs while it is on. Said here rather than left for the
/// player to work out from two settings that otherwise look unrelated.
pub fn status() -> String {
    let text = match ACTIVE_STYLE.load(Ordering::Relaxed) {
        0 => format!(
            "the spectator crosshair is DoD's own 24x24 tile of crosshairs.spr (set {STYLE_CVAR} to 1-{MAX_STYLE} and {NAME} to 1 to use your own)"
        ),
        style if style < 0 => format!(
            "the spectator crosshair draws the whole customXHair.spr sheet, the same as the POV view at {STYLE_CVAR} {style}"
        ),
        style => format!(
            "the spectator crosshair draws tile {style} of customXHair.spr, the same one {STYLE_CVAR} gives the POV view"
        ),
    };
    if crosshair::hidden() {
        format!("{text} (moot right now -- {} is 1, so nothing is drawn)", crosshair::NAME)
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The offsets are read off the disassembly in the module docs. Anchoring
    /// them to the pattern means a mistyped pattern cannot quietly move a
    /// field: every patched field must land on an operand, never on an opcode.
    #[test]
    fn every_patched_field_lands_on_a_wildcard() {
        let tokens: Vec<&str> = PATTERN.split_whitespace().collect();
        assert_eq!(tokens.len(), SPAN, "the pattern must describe the whole span");
        for at in [LEFT_AT, TOP_AT, RIGHT_AT, BOTTOM_AT] {
            for (offset, token) in tokens.iter().skip(at).take(4).enumerate() {
                assert_eq!(*token, "??", "byte {} should be a rect immediate", at + offset);
            }
        }
        assert_eq!(tokens[HANDLE_AT], "??", "the handle displacement");
    }

    /// Everything that is *not* patched has to be a fixed byte, or the pattern
    /// would match more than the one span -- and `find_unique` would refuse.
    #[test]
    fn nothing_else_in_the_pattern_is_a_wildcard() {
        let tokens: Vec<&str> = PATTERN.split_whitespace().collect();
        let patched: Vec<usize> = [LEFT_AT, TOP_AT, RIGHT_AT, BOTTOM_AT]
            .iter()
            .flat_map(|at| *at..*at + 4)
            .chain([HANDLE_AT])
            .collect();
        for (i, token) in tokens.iter().enumerate() {
            if patched.contains(&i) {
                continue;
            }
            assert!(
                u8::from_str_radix(token, 16).is_ok(),
                "byte {i} is {token:?}; only the patched fields may be wildcards"
            );
        }
    }

    /// The three `mov dword [ecx+disp], imm32` opcodes each patched rect field
    /// belongs to, and the displacement each one writes. Spelled out because a
    /// rect written in the wrong field order is a crosshair drawn from the
    /// wrong corner, which is the kind of bug that looks like bad art.
    #[test]
    fn each_rect_field_writes_the_displacement_it_claims() {
        let tokens: Vec<&str> = PATTERN.split_whitespace().collect();
        let field = |at: usize| -> (u8, u8, u8) {
            (
                u8::from_str_radix(tokens[at - 3], 16).unwrap(),
                u8::from_str_radix(tokens[at - 2], 16).unwrap(),
                u8::from_str_radix(tokens[at - 1], 16).unwrap(),
            )
        };
        // wrect_t is { left, right, top, bottom }, so the object's rect starts
        // at +0x64 and the four members are 0x64, 0x68, 0x6c, 0x70.
        assert_eq!(field(LEFT_AT), (0xc7, 0x41, 0x64), "left");
        assert_eq!(field(TOP_AT), (0xc7, 0x41, 0x6c), "top");
        assert_eq!(field(RIGHT_AT), (0xc7, 0x41, 0x68), "right");
        assert_eq!(field(BOTTOM_AT), (0xc7, 0x41, 0x70), "bottom");

        // `8B 41 ??` -- mov eax, [ecx+disp]
        assert_eq!(u8::from_str_radix(tokens[HANDLE_AT - 2], 16).unwrap(), 0x8b);
        assert_eq!(u8::from_str_radix(tokens[HANDLE_AT - 1], 16).unwrap(), 0x41);
    }

    /// The tile arithmetic reproduced from `client+0x2ced0`. Tile 1 is the top
    /// left and tile 16 the bottom right of a 256x256 sprite.
    #[test]
    fn tiles_walk_the_grid_in_the_order_dod_numbers_them() {
        assert_eq!(tile_rect(1), Some([0, 0, 64, 64]));
        assert_eq!(tile_rect(2), Some([64, 0, 128, 64]));
        assert_eq!(tile_rect(4), Some([192, 0, 256, 64]));
        assert_eq!(tile_rect(5), Some([0, 64, 64, 128]));
        assert_eq!(tile_rect(16), Some([192, 192, 256, 256]));
    }

    /// Every tile has to sit inside the sprite the handle points at. A rect
    /// that runs off the end is how a swapped handle goes wrong silently.
    #[test]
    fn no_tile_leaves_the_256x256_sprite() {
        for style in 1..=MAX_STYLE {
            let [left, top, right, bottom] = tile_rect(style).unwrap();
            assert!(left >= 0 && top >= 0, "tile {style}");
            assert!(right <= 256 && bottom <= 256, "tile {style}");
            assert_eq!(right - left, TILE, "tile {style} width");
            assert_eq!(bottom - top, TILE, "tile {style} height");
        }
    }

    /// DoD clamps anything above 16 down to 16 rather than wrapping. Zero has
    /// no tile -- it means "use the HUD sprite", a different function -- but
    /// every negative style, confirmed by disassembly, has no lower clamp at
    /// all and reaches the tile function regardless.
    #[test]
    fn styles_outside_the_grid_clamp_the_way_dod_clamps() {
        assert_eq!(tile_rect(99), tile_rect(16));
        assert_eq!(tile_rect(0), None);
    }

    /// Negative styles all collapse to the same whole-sheet rect -- there is
    /// no per-value distinction to preserve, since DoD's own out-of-range
    /// arithmetic doesn't produce one a spectator could tell apart on screen.
    #[test]
    fn any_negative_style_is_the_whole_sheet() {
        for style in [-1, -2, -3, -16, -1000] {
            assert_eq!(tile_rect(style), Some(WHOLE_SHEET_RECT), "style {style}");
        }
        let [left, top, right, bottom] = WHOLE_SHEET_RECT;
        assert_eq!((left, top), (0, 0));
        assert_eq!((right, bottom), (SPRITE_SIDE, SPRITE_SIDE));
    }

    /// A round trip through the bytes: what [`wanted`] writes is what
    /// [`read_state`] reads back. This is the property that lets `set_matching`
    /// decide from the code rather than from a flag, which is what makes it
    /// survive `client.dll` reloading between demos.
    #[test]
    fn every_state_reads_back_as_the_style_that_wrote_it() {
        let stock = stock_span();
        assert_eq!(read_state(&stock), Some(0), "the shipped bytes are style 0");

        for style in 0..=MAX_STYLE {
            let written = wanted(&stock, style).expect("a writable state");
            assert_eq!(written.len(), SPAN);
            assert_eq!(read_state(&written), Some(style), "style {style}");
        }
    }

    /// Negative styles are the one case where the property above doesn't hold
    /// literally: every negative value writes the identical whole-sheet rect
    /// (see [`any_negative_style_is_the_whole_sheet`]), so reading it back
    /// cannot recover which one wrote it -- only that it was some negative
    /// style. `read_state` reports that as [`WHOLE_SHEET`] regardless.
    #[test]
    fn negative_styles_all_read_back_as_whole_sheet() {
        let stock = stock_span();
        for style in [-1, -2, -16, -1000] {
            let written = wanted(&stock, style).expect("a writable state");
            assert_eq!(read_state(&written), Some(WHOLE_SHEET), "style {style}");
        }
    }

    /// Only the five fields move. Anything else changing would mean rewriting
    /// instructions this module does not own.
    #[test]
    fn nothing_outside_the_five_fields_is_touched() {
        let stock = stock_span();
        let written = wanted(&stock, 9).unwrap();
        let patched: Vec<usize> = [LEFT_AT, TOP_AT, RIGHT_AT, BOTTOM_AT]
            .iter()
            .flat_map(|at| *at..*at + 4)
            .chain([HANDLE_AT])
            .collect();
        for i in 0..SPAN {
            if patched.contains(&i) {
                continue;
            }
            assert_eq!(written[i], stock[i], "byte {i} moved and should not have");
        }
    }

    /// Bytes nobody here wrote are refused rather than overwritten -- the same
    /// discipline `crosshair.rs` and `scoreboard.rs` apply.
    #[test]
    fn an_unrecognised_encoding_is_refused() {
        let mut span = stock_span();
        span[HANDLE_AT] = 0x58; // the POV path's own handle: plausible, not ours
        assert_eq!(read_state(&span), None);

        let mut span = stock_span();
        span[LEFT_AT] = 7; // a rect that is neither stock nor a tile
        assert_eq!(read_state(&span), None);
    }

    #[test]
    fn status_says_which_sprite_is_in_use() {
        let saved = ACTIVE_STYLE.load(Ordering::Acquire);

        ACTIVE_STYLE.store(0, Ordering::Release);
        assert!(status().contains("crosshairs.spr"), "{}", status());
        assert!(!matching());

        ACTIVE_STYLE.store(7, Ordering::Release);
        assert!(status().contains("tile 7"), "{}", status());
        assert!(status().contains("customXHair.spr"), "{}", status());
        assert!(matching());

        ACTIVE_STYLE.store(-1, Ordering::Release);
        assert!(status().contains("whole"), "{}", status());
        assert!(status().contains("customXHair.spr"), "{}", status());
        assert!(matching());

        ACTIVE_STYLE.store(saved, Ordering::Release);
    }

    /// `dodtools_hide_crosshair` stubs `Draw`'s prologue, so nothing this
    /// module patches ever runs while it is on. `status()` has to say so,
    /// rather than describe a tile that is not actually visible.
    #[test]
    fn status_notes_when_hide_crosshair_makes_it_moot() {
        let saved_style = ACTIVE_STYLE.load(Ordering::Acquire);
        let saved_hidden = crosshair::HIDDEN_NOW.load(Ordering::Acquire);

        ACTIVE_STYLE.store(7, Ordering::Release);
        crosshair::HIDDEN_NOW.store(false, Ordering::Release);
        assert!(!status().contains("moot"), "{}", status());

        crosshair::HIDDEN_NOW.store(true, Ordering::Release);
        let text = status();
        assert!(text.contains("moot"), "{text}");
        assert!(text.contains(crosshair::NAME), "{text}");
        // The tile it would draw is still worth reporting alongside the note.
        assert!(text.contains("tile 7"), "{text}");

        ACTIVE_STYLE.store(saved_style, Ordering::Release);
        crosshair::HIDDEN_NOW.store(saved_hidden, Ordering::Release);
    }

    /// The span exactly as `client.dll` ships it, built from the pattern with
    /// the stock values filled into the wildcards.
    fn stock_span() -> Vec<u8> {
        let tokens: Vec<&str> = PATTERN.split_whitespace().collect();
        let mut span: Vec<u8> = tokens
            .iter()
            .map(|t| u8::from_str_radix(t, 16).unwrap_or(0))
            .collect();
        for (at, value) in [LEFT_AT, TOP_AT, RIGHT_AT, BOTTOM_AT].iter().zip(STOCK_RECT) {
            span[*at..*at + 4].copy_from_slice(&value.to_le_bytes());
        }
        span[HANDLE_AT] = STOCK_HANDLE;
        span
    }
}
