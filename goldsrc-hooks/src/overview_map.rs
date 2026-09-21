//! `dodtools_overviewmap`: move and resize DoD's overview map.
//!
//! ## Four dwords, not a patch
//!
//! `CHud::ComputeOverviewMapRects` runs from `CHudDoDMap::VidInit` and **caches**
//! two rectangles in `gHUD` rather than recomputing them per frame:
//!
//! ```text
//!     gHUD+0x238 / +0x23c / +0x240 / +0x244   x, y, w, h
//!     gHUD+0x248 / +0x24c / +0x250 / +0x254   x, y, w, h
//! ```
//!
//! `CHud::GetOverviewMapBounds` hands one of the two back depending on
//! `CHud::OverviewMapMode` (`_cl_minimap`'s value, while a spectator predicate
//! holds). So placing the map is writing four integers into `.data`. Nothing
//! executable is touched, no signature is overwritten, and putting it back is
//! letting `VidInit` recompute.
//!
//! ## The survey has the two rectangles the wrong way round
//!
//! #268 (quoting `docs/goldsrc_client_dll_survey.md` §5) calls `+0x238` "the
//! small map" and `+0x248` "the full map", and then observes that "at 0.625 x
//! screen width the small map is huge". It is huge because it is not the small
//! one. Read off the arithmetic:
//!
//! ```text
//!     +0x238 block   w = 0.625 * ScreenWidth,  h = 0.625 * ScreenHeight
//!                    x = (ScreenWidth  - w) / 2      <- centred
//!                    y = (ScreenHeight - h) / 2
//!                    returned for OverviewMapMode 1
//!
//!     +0x248 block   w = 0.24 * ScreenWidth,   h = 0.24 * ScreenHeight
//!                    x = ScreenWidth - ScreenWidth/320 - w   <- right edge
//!                    y = ScreenHeight/240                    <- top edge
//!                    returned for OverviewMapMode 2, plus the
//!                    54 * ScreenHeight/480 spectator-bar offset on y
//! ```
//!
//! A 0.625-of-screen box centred on the screen is the **full** map; a
//! 0.24-of-screen box tucked into the top-right corner with a one-pixel-ish
//! margin is the **mini**map. The spectator-bar offset settles it too: pushing
//! something down past the bar makes sense for an element at the top of the
//! screen, not for one already in the middle of it.
//!
//! So this module names them `full` (`+0x238`, mode 1) and `mini` (`+0x248`,
//! mode 2), and the survey's §5 needs the same correction.
//!
//! ## Re-applied every frame
//!
//! The rects are recomputed on `VidInit` -- a resolution change or a level load
//! -- so a placement has to be re-asserted. `commands::poll` already runs every
//! frame and four dword compares cost nothing, which is the same "re-apply
//! rather than patch once" shape the rest of this DLL uses.
//!
//! ## What else moves with it
//!
//! `CHudDeathNotice::Draw` and `CObjectiveIcons::Draw` take their y from the
//! map's bounds while the full map is up. Moving the map moves them. That is
//! the engine's own coupling, not something introduced here, but it is worth
//! knowing before rather than after.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};

use crate::engine;
use crate::names::console_name;
use crate::scan;

/// The command name. Registered in `commands.rs`.
pub const NAME: &str = console_name!("overviewmap");

/// `mov ecx, gHUD; call CHud::ComputeOverviewMapRects` at the end of
/// `CHudDoDMap::VidInit`. Both operands are wildcards: the first gives `gHUD`,
/// the second gives the function, and neither is written down here.
const CALL_SITE: &str = "B9 ?? ?? ?? ?? E8 ?? ?? ?? ?? 5F B8 01 00 00 00 5E C3";

/// `CHud::ComputeOverviewMapRects`'s own opening, ending on the write to
/// `[esi+0x240]` -- the width of the first rectangle. Matching this
/// independently is what turns "the call site's target" into "the function that
/// writes the field this module writes".
const COMPUTE: &str =
    "DB 05 ?? ?? ?? ?? 56 8B F1 DC 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 89 86 40 02 00 00";

/// Offsets into a [`CALL_SITE`] match.
const GHUD_AT: usize = 1;
const CALL_AT: usize = 5;

/// The full map: 0.625 of the screen, centred. `OverviewMapMode` 1.
const FULL_AT: usize = 0x238;
/// The minimap: 0.24 of the screen, top-right. `OverviewMapMode` 2.
const MINI_AT: usize = 0x248;

/// Rejects a rectangle that could not be a screen rectangle. The fields are
/// plain `int`s the engine hands to the sprite drawing code, so nothing stops a
/// nonsense value except this.
const COORD_LIMIT: i32 = 1 << 15;

/// Which of the two cached rectangles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    Full,
    Mini,
}

impl Which {
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "full" => Some(Self::Full),
            "mini" | "minimap" | "small" => Some(Self::Mini),
            _ => None,
        }
    }

    fn offset(self) -> usize {
        match self {
            Self::Full => FULL_AT,
            Self::Mini => MINI_AT,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Mini => "mini",
        }
    }

    /// What `_cl_minimap` has to be for this rectangle to be the one drawn.
    pub fn mode(self) -> i32 {
        match self {
            Self::Full => 1,
            Self::Mini => 2,
        }
    }

    fn held(self) -> &'static [AtomicI32; 4] {
        match self {
            Self::Full => &HELD_FULL,
            Self::Mini => &HELD_MINI,
        }
    }

    fn holding(self) -> &'static AtomicBool {
        match self {
            Self::Full => &HOLDING_FULL,
            Self::Mini => &HOLDING_MINI,
        }
    }
}

/// One cached rectangle, in the order the engine stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    fn fields(&self) -> [i32; 4] {
        [self.x, self.y, self.w, self.h]
    }

    fn from_fields(f: [i32; 4]) -> Self {
        Self { x: f[0], y: f[1], w: f[2], h: f[3] }
    }

    /// A rectangle with no area would not draw, and one with a negative size
    /// is how a sprite routine ends up reading backwards.
    pub fn validate(&self) -> Result<(), String> {
        if self.w <= 0 || self.h <= 0 {
            return Err(format!("{}x{} has no area", self.w, self.h));
        }
        for (label, value) in [("x", self.x), ("y", self.y), ("w", self.w), ("h", self.h)] {
            if value.abs() >= COORD_LIMIT {
                return Err(format!("{label} = {value} is not a screen coordinate"));
            }
        }
        Ok(())
    }
}

impl std::fmt::Display for Rect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "x {} y {} w {} h {}", self.x, self.y, self.w, self.h)
    }
}

static HELD_FULL: [AtomicI32; 4] = [const { AtomicI32::new(0) }; 4];
static HELD_MINI: [AtomicI32; 4] = [const { AtomicI32::new(0) }; 4];
static HOLDING_FULL: AtomicBool = AtomicBool::new(false);
static HOLDING_MINI: AtomicBool = AtomicBool::new(false);

static GHUD: AtomicUsize = AtomicUsize::new(0);
static SCANNED_BASE: AtomicUsize = AtomicUsize::new(0);

/// Finds `gHUD`, and proves it is `gHUD` rather than some other pointer.
///
/// The call site gives both the object and the function it is passed to; the
/// function is then matched independently by its own opening, which ends on the
/// write to `[esi+0x240]`. So the address this module writes into is the one
/// the engine passes to the code that writes the same field.
fn ghud() -> Result<usize, String> {
    let Some(base) = engine::client_module_base() else {
        return Err("client.dll is not loaded yet".to_string());
    };
    if SCANNED_BASE.load(Ordering::Acquire) == base {
        let cached = GHUD.load(Ordering::Acquire);
        if cached != 0 {
            return Ok(cached);
        }
    }

    // Safety: `client_module_base` only returns a base for a mapped module.
    let call_site = unsafe { scan::find_unique(base, CALL_SITE) }
        .map_err(|why| format!("could not find the overview-map call site -- {why}"))?;
    let compute = unsafe { scan::find_unique(base, COMPUTE) }
        .map_err(|why| format!("could not find CHud::ComputeOverviewMapRects -- {why}"))?;

    // Safety: both addresses are inside spans the scan proved are mapped.
    let (object, called) = unsafe {
        let object = ((call_site + GHUD_AT) as *const u32).read_unaligned() as usize;
        let displacement = ((call_site + CALL_AT + 1) as *const i32).read_unaligned();
        (object, call_site.wrapping_add(CALL_AT + 5).wrapping_add(displacement as usize))
    };
    if called != compute {
        return Err(format!(
            "the call site targets +{:#x}, but ComputeOverviewMapRects matched at +{:#x}",
            called.wrapping_sub(base),
            compute - base
        ));
    }
    if object < base || object - base > 0x0400_0000 {
        return Err(format!("{object:#x} is not an address inside client.dll"));
    }

    GHUD.store(object, Ordering::Release);
    SCANNED_BASE.store(base, Ordering::Release);
    Ok(object)
}

/// The rectangle the engine currently has cached.
pub fn read(which: Which) -> Result<Rect, String> {
    let ghud = ghud()?;
    // Safety: four dwords inside the object the engine itself writes here.
    let fields = unsafe {
        let at = (ghud + which.offset()) as *const i32;
        [
            at.read_unaligned(),
            at.add(1).read_unaligned(),
            at.add(2).read_unaligned(),
            at.add(3).read_unaligned(),
        ]
    };
    Ok(Rect::from_fields(fields))
}

/// Holds `rect` for `which`, so every frame re-asserts it.
pub fn hold(which: Which, rect: Rect) -> Result<(), String> {
    rect.validate()?;
    // Resolve before promising to hold it, so a bad build is reported at the
    // moment the user types the command rather than silently every frame.
    ghud()?;
    for (slot, value) in which.held().iter().zip(rect.fields()) {
        slot.store(value, Ordering::Relaxed);
    }
    which.holding().store(true, Ordering::Release);
    Ok(())
}

/// Stops holding both rectangles. The next `VidInit` recomputes them; until
/// then whatever is cached stays, which is what the engine would have had.
pub fn release() {
    HOLDING_FULL.store(false, Ordering::Release);
    HOLDING_MINI.store(false, Ordering::Release);
}

pub fn is_held(which: Which) -> bool {
    which.holding().load(Ordering::Relaxed)
}

pub fn any_held() -> bool {
    is_held(Which::Full) || is_held(Which::Mini)
}

/// The rectangle being held for `which`, if any.
pub fn held(which: Which) -> Option<Rect> {
    if !is_held(which) {
        return None;
    }
    let slots = which.held();
    Some(Rect::from_fields([
        slots[0].load(Ordering::Relaxed),
        slots[1].load(Ordering::Relaxed),
        slots[2].load(Ordering::Relaxed),
        slots[3].load(Ordering::Relaxed),
    ]))
}

/// Re-asserts every held rectangle, returning how many dwords were written.
///
/// Called every frame. Nothing is written when the cache already matches, so
/// the steady-state cost is eight dword compares.
pub fn apply() -> Result<usize, String> {
    if !any_held() {
        return Ok(0);
    }
    let ghud = ghud()?;
    let mut written = 0;
    for which in [Which::Full, Which::Mini] {
        let Some(rect) = held(which) else { continue };
        for (index, value) in rect.fields().into_iter().enumerate() {
            let at = ghud + which.offset() + index * 4;
            // Safety: a dword inside the object the engine writes here itself.
            if unsafe { (at as *const i32).read_unaligned() } == value {
                continue;
            }
            if !unsafe { crate::patch::write_code_bytes(at, &value.to_le_bytes()) } {
                return Err(format!("could not write the {} map's rect", which.name()));
            }
            written += 1;
        }
    }
    Ok(written)
}

/// Both rectangles and what is being held, for the bare command.
pub fn listing() -> String {
    let mut out = String::new();
    for which in [Which::Full, Which::Mini] {
        let cached = match read(which) {
            Ok(rect) => rect.to_string(),
            Err(why) => format!("unavailable -- {why}"),
        };
        let state = match held(which) {
            Some(rect) => format!("held at {rect}"),
            None => "the engine's own".to_string(),
        };
        out.push_str(&format!(
            "  {:<5} (_cl_minimap {})  {cached}   [{state}]\n",
            which.name(),
            which.mode()
        ));
    }
    out
}

/// One line for `dodtools_status`.
pub fn status() -> String {
    let held: Vec<&str> = [Which::Full, Which::Mini]
        .into_iter()
        .filter(|w| is_held(*w))
        .map(|w| w.name())
        .collect();
    if held.is_empty() {
        return format!("the overview map is where the engine puts it ({NAME} lists both rects)");
    }
    format!(
        "holding the {} overview rect(s) against VidInit -- note the kill feed and objective icons take their y from the full map's bounds",
        held.join(" and ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clear() {
        release();
    }

    /// The two offsets are 16 bytes apart because each rect is four `int`s.
    /// A wrong gap would write the second rect over the first one's tail.
    #[test]
    fn the_two_rects_are_four_ints_apart() {
        assert_eq!(MINI_AT - FULL_AT, 4 * std::mem::size_of::<i32>());
    }

    /// The names have to map to the offsets the module docs claim, and to the
    /// `_cl_minimap` values that select them. Getting this pair backwards is
    /// exactly the mistake the survey made.
    #[test]
    fn full_is_the_centred_rect_and_mini_is_the_corner_one() {
        assert_eq!(Which::Full.offset(), 0x238);
        assert_eq!(Which::Mini.offset(), 0x248);
        assert_eq!(Which::Full.mode(), 1);
        assert_eq!(Which::Mini.mode(), 2);
    }

    /// `small` is accepted for the minimap on purpose: the survey calls the
    /// 0.625 rect "small", so someone reading it will type that, and pointing
    /// them at the corner map is the less surprising of the two wrong answers.
    #[test]
    fn names_parse_including_the_surveys_misleading_one() {
        assert_eq!(Which::parse("full"), Some(Which::Full));
        assert_eq!(Which::parse("FULL"), Some(Which::Full));
        assert_eq!(Which::parse("mini"), Some(Which::Mini));
        assert_eq!(Which::parse("minimap"), Some(Which::Mini));
        assert_eq!(Which::parse("small"), Some(Which::Mini));
        assert_eq!(Which::parse("big"), None);
        assert_eq!(Which::parse(""), None);
    }

    #[test]
    fn a_rect_with_no_area_is_refused() {
        assert!(Rect { x: 0, y: 0, w: 0, h: 10 }.validate().is_err());
        assert!(Rect { x: 0, y: 0, w: 10, h: 0 }.validate().is_err());
        assert!(Rect { x: 0, y: 0, w: -10, h: 10 }.validate().is_err());
        assert!(Rect { x: 0, y: 0, w: 10, h: 10 }.validate().is_ok());
    }

    /// Negative x and y are legal -- sliding a rect off the left or top edge is
    /// a reasonable thing to ask for -- but a coordinate that could not be on
    /// any screen is a typo.
    #[test]
    fn offscreen_is_allowed_but_absurd_is_not() {
        assert!(Rect { x: -200, y: -50, w: 100, h: 100 }.validate().is_ok());
        assert!(Rect { x: COORD_LIMIT, y: 0, w: 10, h: 10 }.validate().is_err());
        assert!(Rect { x: 0, y: 0, w: COORD_LIMIT, h: 10 }.validate().is_err());
    }

    #[test]
    fn fields_round_trip_in_the_engines_order() {
        let rect = Rect { x: 1, y: 2, w: 3, h: 4 };
        assert_eq!(rect.fields(), [1, 2, 3, 4]);
        assert_eq!(Rect::from_fields(rect.fields()), rect);
    }

    #[test]
    fn holding_one_rect_leaves_the_other_alone() {
        clear();
        assert!(!any_held());

        // `hold` resolves gHUD, which needs a loaded client.dll, so the state
        // machine is exercised through the pieces it is made of.
        HELD_FULL[0].store(10, Ordering::Relaxed);
        HELD_FULL[1].store(20, Ordering::Relaxed);
        HELD_FULL[2].store(30, Ordering::Relaxed);
        HELD_FULL[3].store(40, Ordering::Relaxed);
        HOLDING_FULL.store(true, Ordering::Release);

        assert!(is_held(Which::Full));
        assert!(!is_held(Which::Mini));
        assert!(any_held());
        assert_eq!(held(Which::Full), Some(Rect { x: 10, y: 20, w: 30, h: 40 }));
        assert_eq!(held(Which::Mini), None);

        release();
        assert!(!any_held());
        assert_eq!(held(Which::Full), None);
    }

    #[test]
    fn status_warns_about_what_moves_with_the_map() {
        clear();
        assert!(status().contains("where the engine puts it"), "{}", status());

        HOLDING_FULL.store(true, Ordering::Release);
        let text = status();
        assert!(text.contains("full"), "{text}");
        assert!(text.contains("kill feed"), "{text}");
        clear();
    }

    #[test]
    fn the_patterns_are_well_formed() {
        for pattern in [CALL_SITE, COMPUTE] {
            let tokens: Vec<&str> = pattern.split_whitespace().collect();
            assert!(tokens.len() > 8);
            assert_ne!(tokens[0], "??", "a leading wildcard is rejected by the scanner");
            for token in &tokens {
                assert!(
                    *token == "??" || u8::from_str_radix(token, 16).is_ok(),
                    "{token:?} is not a hex byte or a wildcard"
                );
            }
        }
    }

    /// `GHUD_AT` must be the `mov ecx, imm32`'s operand and `CALL_AT` its
    /// `call`'s opcode. Reading one byte off would give a plausible pointer.
    #[test]
    fn the_call_site_offsets_land_on_the_right_operands() {
        let tokens: Vec<&str> = CALL_SITE.split_whitespace().collect();
        let wildcards = |at: usize, what: &str| {
            for (offset, token) in tokens.iter().skip(at).take(4).enumerate() {
                assert_eq!(*token, "??", "byte {} is {what}", at + offset);
            }
        };
        assert_eq!(u8::from_str_radix(tokens[0], 16).unwrap(), 0xb9, "mov ecx, imm32");
        wildcards(GHUD_AT, "gHUD");
        assert_eq!(u8::from_str_radix(tokens[CALL_AT], 16).unwrap(), 0xe8, "call rel32");
        wildcards(CALL_AT + 1, "the displacement");
    }

    /// The `COMPUTE` pattern has to end on the write this module's offsets are
    /// about, or matching it proves nothing about them.
    #[test]
    fn the_compute_pattern_ends_on_the_width_write() {
        let tokens: Vec<&str> = COMPUTE.split_whitespace().collect();
        let tail: Vec<u8> = tokens[tokens.len() - 6..]
            .iter()
            .map(|t| u8::from_str_radix(t, 16).expect("a fixed byte"))
            .collect();
        // `89 86 40 02 00 00` -- mov [esi+0x240], eax
        assert_eq!(tail[0], 0x89);
        assert_eq!(tail[1], 0x86);
        assert_eq!(
            u32::from_le_bytes([tail[2], tail[3], tail[4], tail[5]]) as usize,
            FULL_AT + 8,
            "the width field of the full rect"
        );
    }
}
