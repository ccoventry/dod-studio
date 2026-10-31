//! The gap call in POV's red crosshair (`cl_xhair_style 0`): who answers it.
//!
//! `CHudDoDCrossHair`'s style-0 draw (`client+0x2cda0`) asks
//! `client+0x2cfb0` for the gap between the dot and the four arms, once a
//! frame, then blits the five pieces around the screen centre. Read out of
//! the binary (and matched against dod13-client's `GetCrossHairWidth`):
//!
//! ```text
//!     cl_dynamic_xhair < 1                    -> 4, fixed
//!     weapon 13-16 (grenades), 29-31 (rockets) -> 4, fixed
//!     spread = deployed ? alt : base           (IsInMGDeploy: bipod or sandbag)
//!     speed > 105 u/s: spread += moving
//!     target = 2 + trunc(200 * spread)
//!     [this+0x98] steps toward target by at most 3 px; return it
//! ```
//!
//! The weapon is the HUD's current one (the `CurWeapon` message, recorded in
//! a demo), deployed is `g_iUser3 == 2` or the sandbag flag (both from the
//! player's entity state, recorded too), and the speed is the length of the
//! velocity at `client+0x1080f8`. That last one is the only input a demo
//! lacks: `HUD_WeaponsPostThink` (`client+0x12360`) is its one writer,
//! copying the predicted player's velocity in, and prediction does not run
//! while a demo plays. So a POV demo's crosshair stays at the resting gap
//! however fast its recorder runs (#1187's test, 377 frames at 5 px).
//!
//! Two modules answer the call instead of the game, each only in its own
//! case; this one points the call at [`gap_hook`] while either wants it and
//! back at the game's own otherwise:
//!
//! - `spectator_crosshair_gap`, in a player's eyes while spectating
//!   (`g_iUser1` not 0), when `spectator_crosshair` routes the spectator
//!   draw to the style-0 draw (#308): the watched player's gap, worked out
//!   from what a spectator sees.
//! - `pov_crosshair_gap`, in a POV demo's own view (`g_iUser1` 0): the
//!   game's own function, with the recorded velocity in place for the call.
//!
//! Anything else -- live play above all -- gets the game's own function,
//! and while neither wants it the call is the game's bytes.

// The hook is a thiscall, which only the 32-bit build has; the host build
// (clippy, tests) compiles the rest.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::engine;
use crate::hw_call_site::{CALL_LEN, call_bytes, call_target};
use crate::scan;

/// The style-0 draw, `client+0x2cda0`, up to and past its gap call. The
/// call's `rel32` is a wildcard so the hooked form matches too.
const DRAW_PATTERN: &str = "83 EC 0C 53 55 56 57 8B F1 E8 ?? ?? ?? ?? 8B E8 A1";
/// Where the gap call's `E8` sits in [`DRAW_PATTERN`].
const GAP_CALL_AT: usize = 9;
/// The gap function, `client+0x2cfb0`: the `cl_dynamic_xhair` test it opens
/// with.
const GAP_PATTERN: &str = "A1 ?? ?? ?? ?? 56 85 C0 8B F1 74 17 D9 40 0C D8 1D";
/// The spread function, `client+0x2d070`, up to `push velocity` before the
/// call that takes its length.
const SPREAD_PATTERN: &str = "51 56 B9 ?? ?? ?? ?? E8 ?? ?? ?? ?? 8D 0C C5 00 00 00 00 \
                              2B C8 8D 04 88 B9 ?? ?? ?? ?? 8D 34 40 C1 E6 02 8B 96 ?? ?? ?? ?? \
                              89 54 24 04 E8 ?? ?? ?? ?? 84 C0 74 0A 8B 86 ?? ?? ?? ?? \
                              89 44 24 04 68 ?? ?? ?? ??";
/// Where the weapon table's address (its `base` float for weapon 0) sits in
/// [`SPREAD_PATTERN`].
const TABLE_AT: usize = 37;
/// Where the velocity's address sits in [`SPREAD_PATTERN`].
const VELOCITY_AT: usize = 65;

/// What the game returns without easing: `cl_dynamic_xhair 0`, grenades and
/// rockets.
pub const FIXED_GAP: i32 = 4;

/// What the scans found in one `client.dll`.
#[derive(Clone, Copy)]
struct Code {
    draw: usize,
    call_site: usize,
    original: usize,
    table: usize,
    velocity: usize,
}

/// Addresses found by the scans, 0 before them. `client.dll` does not reload
/// between demos; [`RESOLVED_BASE`] notices whatever else would.
static CALL_SITE: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static DRAW: AtomicUsize = AtomicUsize::new(0);
static TABLE: AtomicUsize = AtomicUsize::new(0);
static VELOCITY: AtomicUsize = AtomicUsize::new(0);
static RESOLVED_BASE: AtomicUsize = AtomicUsize::new(0);
/// The module base a scan failed against, so it is not repeated each frame.
static FAILED_BASE: AtomicUsize = AtomicUsize::new(0);

/// Who wants the call pointed at [`gap_hook`].
static SPECTATOR_WANTS: AtomicBool = AtomicBool::new(false);
static POV_DEMO_WANTS: AtomicBool = AtomicBool::new(false);

/// The scans' results, scanning once per `client.dll`. The draw scan matches
/// the hooked call too (its `rel32` is a wildcard), and the gap function is
/// found by its own signature, not through the call.
fn resolve() -> Result<Code, String> {
    let base = engine::client_module_base().ok_or("client.dll is not loaded yet")?;
    if RESOLVED_BASE.load(Ordering::Acquire) == base {
        return Ok(Code {
            draw: DRAW.load(Ordering::Acquire),
            call_site: CALL_SITE.load(Ordering::Acquire),
            original: ORIGINAL.load(Ordering::Acquire),
            table: TABLE.load(Ordering::Acquire),
            velocity: VELOCITY.load(Ordering::Acquire),
        });
    }
    // Asked every frame while a user wants it: one failed scan is enough.
    if FAILED_BASE.load(Ordering::Acquire) == base {
        return Err("the red crosshair's code was not found in this client.dll".to_string());
    }
    scan_client(base).inspect_err(|_| FAILED_BASE.store(base, Ordering::Release))
}

/// [`resolve`]'s scans, for the module at `base`.
fn scan_client(base: usize) -> Result<Code, String> {
    // Safety: `client_module_base` only returns a mapped module's base.
    let draw = unsafe { scan::find_unique(base, DRAW_PATTERN) }
        .map_err(|why| format!("could not find the style-0 crosshair draw -- {why}"))?;
    let original = unsafe { scan::find_unique(base, GAP_PATTERN) }
        .map_err(|why| format!("could not find the crosshair gap function -- {why}"))?;
    let spread = unsafe { scan::find_unique(base, SPREAD_PATTERN) }
        .map_err(|why| format!("could not find the weapon spread function -- {why}"))?;
    // Safety: the pattern matched, and these are two of its operands.
    let operand = |at: usize| unsafe { ((spread + at) as *const u32).read_unaligned() } as usize;
    let code = Code {
        draw,
        call_site: draw + GAP_CALL_AT,
        original,
        table: operand(TABLE_AT),
        velocity: operand(VELOCITY_AT),
    };
    if !crate::crash::readable(code.velocity, 12) {
        return Err(format!(
            "the velocity the crosshair reads ({:#x}) is not readable memory",
            code.velocity
        ));
    }
    DRAW.store(code.draw, Ordering::Release);
    CALL_SITE.store(code.call_site, Ordering::Release);
    ORIGINAL.store(code.original, Ordering::Release);
    TABLE.store(code.table, Ordering::Release);
    VELOCITY.store(code.velocity, Ordering::Release);
    RESOLVED_BASE.store(base, Ordering::Release);
    Ok(code)
}

/// The style-0 draw's address, for `spectator_crosshair`'s jump.
pub fn style0_draw() -> Result<usize, String> {
    resolve().map(|code| code.draw)
}

/// The address the spread function indexes `P_WpnInfo` by: weapon 0's
/// `base` float.
pub fn table_address() -> Result<usize, String> {
    resolve().map(|code| code.table)
}

/// The three floats the spread function takes the speed from, which
/// `HUD_WeaponsPostThink` fills while the client predicts; `None` before
/// the scans. An atomic load: the gap hook asks every frame.
pub fn velocity_slot() -> Option<usize> {
    let slot = VELOCITY.load(Ordering::Acquire);
    (slot != 0).then_some(slot)
}

/// The spectator side's wish (`spectator_crosshair`, while it routes the
/// spectator draw to the red one). Returns whether the call was written.
pub fn set_spectator_hook(on: bool) -> Result<bool, String> {
    SPECTATOR_WANTS.store(on, Ordering::Release);
    apply()
}

/// The POV demo side's wish (`pov_crosshair_gap`, while a POV demo plays).
/// Returns whether the call was written.
pub fn set_pov_demo_hook(on: bool) -> Result<bool, String> {
    POV_DEMO_WANTS.store(on, Ordering::Release);
    apply()
}

/// Points the style-0 draw's gap call at [`gap_hook`] while either side
/// wants it, back at the game's own otherwise. Returns whether anything was
/// written. Off costs nothing before the first time it was on.
fn apply() -> Result<bool, String> {
    let on = SPECTATOR_WANTS.load(Ordering::Acquire) || POV_DEMO_WANTS.load(Ordering::Acquire);
    if !on && RESOLVED_BASE.load(Ordering::Acquire) == 0 {
        return Ok(false);
    }
    let code = resolve()?;
    let stock = call_bytes(code.call_site, code.original);
    let ours = call_bytes(code.call_site, hook_address());
    let want = if on { ours } else { stock };
    // Safety: the draw's pattern covers the call's five bytes.
    let present = unsafe { std::slice::from_raw_parts(code.call_site as *const u8, CALL_LEN) };
    if present == want {
        return Ok(false);
    }
    if present != stock && present != ours {
        let mut bytes = [0u8; CALL_LEN];
        bytes.copy_from_slice(present);
        return Err(format!(
            "the crosshair gap call goes to {:#x}, neither the game's gap function nor ours -- something else has patched it",
            call_target(code.call_site, &bytes)
        ));
    }
    // Safety: as above; written from the game's thread between frames.
    if !unsafe { crate::patch::write_code_bytes(code.call_site, &want) } {
        return Err("could not make the crosshair gap call writable".to_string());
    }
    Ok(true)
}

#[cfg(target_arch = "x86")]
type GapFn = unsafe extern "thiscall" fn(*mut u8) -> i32;

/// The game's own gap function, `client+0x2cfb0`, on `this`; the fixed gap
/// before the scans have found it.
///
/// # Safety
///
/// `this` is the `CHudDoDCrossHair` the style-0 draw passed, and this runs
/// on the HUD's thread, inside that draw.
#[cfg(target_arch = "x86")]
pub unsafe fn original_gap(this: *mut u8) -> i32 {
    let original = ORIGINAL.load(Ordering::Relaxed);
    if original == 0 {
        return FIXED_GAP;
    }
    // Safety: the address the call went to before it was pointed here, a
    // thiscall with no arguments returning the gap.
    let original: GapFn = unsafe { std::mem::transmute::<usize, GapFn>(original) };
    unsafe { original(this) }
}

/// Stands in for `client+0x2cfb0` at the style-0 draw's call. `this` is the
/// `CHudDoDCrossHair` the draw was called on.
#[cfg(target_arch = "x86")]
unsafe extern "thiscall" fn gap_hook(this: *mut u8) -> i32 {
    if crate::spectator_target::mode().unwrap_or(0) != 0 {
        return unsafe { crate::spectator_crosshair_gap::gap(this) };
    }
    if crate::pov_crosshair_gap::playing() {
        return unsafe { crate::pov_crosshair_gap::gap(this) };
    }
    unsafe { original_gap(this) }
}

/// Where the gap call goes while hooked.
#[cfg(target_arch = "x86")]
fn hook_address() -> usize {
    gap_hook as *const () as usize
}

#[cfg(not(target_arch = "x86"))]
fn hook_address() -> usize {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_patterns_parse_and_reach_their_operands() {
        for pattern in [DRAW_PATTERN, GAP_PATTERN, SPREAD_PATTERN] {
            assert!(scan::Pattern::parse(pattern).is_ok(), "{pattern}");
        }
        let draw: Vec<&str> = DRAW_PATTERN.split_whitespace().collect();
        assert_eq!(draw[GAP_CALL_AT], "E8");
        assert!(draw.len() >= GAP_CALL_AT + CALL_LEN);
        let spread: Vec<&str> = SPREAD_PATTERN.split_whitespace().collect();
        assert_eq!(&spread[TABLE_AT - 2..TABLE_AT], &["8B", "96"]);
        assert_eq!(spread[VELOCITY_AT - 1], "68");
        assert_eq!(spread.len(), VELOCITY_AT + 4);
    }
}
