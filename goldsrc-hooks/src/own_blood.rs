//! `ds_view_show_own_blood <0..4>`: POV demos show the recording player's own
//! blood when they're hit, which the game skips this close to the camera
//! (#1035, findings on #1018).
//!
//! ## What the game does
//!
//! A `BloodPuff` message is the puff's position, three coords. A POV demo
//! keeps every one, the recorder's own wounds too (25-58 per demo closer than
//! 32 units to the view, in 6 demos from 6 recorders). `EV_BloodPuff`
//! (`client+0x22470`, the same `client.dll` in both installs) works out
//! `|v_origin - puff|` and skips the puff when that is under 32:
//!
//! ```text
//!     client+0x22530  d8 1d <+0xa8894>   fcomp dword ptr [32.0]
//!     client+0x22536  df e0              fnstsw ax
//!     client+0x22538  f6 c4 05           test ah, 5
//!     client+0x2253b  0f 8b ...          jnp  <end>   ; distance < 32: skip
//! ```
//!
//! So the recorder's own puffs, which land on his body a few units below
//! the eye, never draw. (Whether one 27 units away drew was a coin flip on the
//! client's last-frame `v_origin`, which is why the stock game showed the
//! #727 spray at 60 fps and not at 300.)
//!
//! ## The change
//!
//! The setting picks a lower cut: 0 is the game's 32, then 24, 16, 8 and 0.
//! The `fcomp`'s absolute operand is pointed at [`CUT`], a float of our own,
//! rather than the 32.0 itself being written: that constant is shared by 10
//! other reads in `client.dll`. Level 0 puts the stock operand back, and a
//! game that never asks for more than 0 is never written to.
//!
//! The patch runs from `commands::poll` on the engine thread. A frame where
//! the cvar hasn't changed costs one float compare. A value past 4 or below 0
//! is set back to the nearest level, with a console line. Under about 16 units
//! a puff covers a third of the screen with blocky sprite pixels and puts a
//! pale haze over the view, hence the levels in between (#1018).
//!
//! `tools/verify_own_blood_offsets.py` checks the pattern is unique and its
//! operand is the 32.0 in both installs' `client.dll`.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize, Ordering};

use crate::engine::{self, CvarSPartial};
use crate::names::console_name;
use crate::scan;

pub const NAME: &str = console_name!("view_show_own_blood");

/// The distance under which `EV_BloodPuff` skips a puff, for each level.
const CUTS: [f32; 5] = [32.0, 24.0, 16.0, 8.0, 0.0];
pub const MAX_LEVEL: i32 = CUTS.len() as i32 - 1;

/// From the distance's `fsqrt` to the skip's `jnp`; the `fcomp` operand is
/// a wildcard, so a rescan finds the site patched or not.
const PATTERN: &str = "D9 FA D9 5C 24 50 DD D8 DD D8 DD D8 D9 44 24 50 \
                       D8 1D ?? ?? ?? ?? DF E0 F6 C4 05 0F 8B";
/// The `fcomp m32fp`'s absolute operand within [`PATTERN`].
const OPERAND_AT: usize = 18;

/// The cut the patched `fcomp` reads, as f32 bits. The client code holds its
/// address, so it stays a plain 4-byte static.
static CUT: AtomicU32 = AtomicU32::new(0x4200_0000);
static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
/// The cvar's raw f32 bits the last time it was acted on; NaN until then.
static SEEN: AtomicU32 = AtomicU32::new(0x7fc0_0000);
/// The operand's address, for the `client.dll` base in [`SCANNED_BASE`].
static ADDRESS: AtomicUsize = AtomicUsize::new(0);
static SCANNED_BASE: AtomicUsize = AtomicUsize::new(0);
/// The game's own operand (the 32.0's address), read before the first write.
static STOCK_OPERAND: AtomicU32 = AtomicU32::new(0);
static GAVE_UP: AtomicBool = AtomicBool::new(false);

pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

/// The level a cvar value asks for, and whether it had to be clamped to get
/// there. A fraction rounds down; NaN and infinities read as 0.
pub(crate) fn level_for(raw: f32) -> (i32, bool) {
    if !raw.is_finite() {
        return (0, true);
    }
    let floor = raw.floor();
    if floor < 0.0 {
        (0, true)
    } else if floor > MAX_LEVEL as f32 {
        (MAX_LEVEL, true)
    } else {
        (floor as i32, false)
    }
}

/// The cut for a level already clamped by [`level_for`].
pub(crate) fn cut_for(level: i32) -> f32 {
    CUTS[level.clamp(0, MAX_LEVEL) as usize]
}

/// The operand's address in the loaded `client.dll`, scanning once per base.
fn operand_address(base: usize) -> Result<usize, String> {
    if SCANNED_BASE.load(Ordering::Acquire) == base {
        let cached = ADDRESS.load(Ordering::Acquire);
        if cached != 0 {
            return Ok(cached);
        }
    }
    // Safety: a mapped module, for the session.
    let found = unsafe { scan::find_unique(base, PATTERN) }
        .map_err(|why| format!("could not find EV_BloodPuff's distance cut -- {why}"))?;
    let address = found + OPERAND_AT;
    // Safety: inside the pattern just matched.
    let operand = unsafe { (address as *const u32).read_unaligned() };
    let ours = &CUT as *const AtomicU32 as usize as u32;
    if operand != ours {
        // Safety: the operand of a `fcomp` that reads it on every puff.
        let stock = unsafe { *(operand as usize as *const f32) };
        if stock != CUTS[0] {
            return Err(format!(
                "EV_BloodPuff's cut reads {stock} at {operand:#x}, not 32 -- something else has patched it"
            ));
        }
        STOCK_OPERAND.store(operand, Ordering::Release);
    }
    ADDRESS.store(address, Ordering::Release);
    SCANNED_BASE.store(base, Ordering::Release);
    Ok(address)
}

/// Points the `fcomp` at the stock 32.0 for level 0 and at [`CUT`] otherwise.
/// Returns whether it wrote.
fn apply(level: i32) -> Result<bool, String> {
    let base = engine::client_module_base().ok_or("client.dll is not loaded yet")?;
    let address = operand_address(base)?;
    let ours = &CUT as *const AtomicU32 as usize as u32;
    CUT.store(cut_for(level).to_bits(), Ordering::Release);
    let stock = STOCK_OPERAND.load(Ordering::Acquire);
    let want = if level == 0 { stock } else { ours };
    // Safety: `operand_address` proved this is the operand's 4 bytes.
    let present = unsafe { (address as *const u32).read_unaligned() };
    if present == want {
        return Ok(false);
    }
    if present != stock && present != ours {
        return Err(format!(
            "EV_BloodPuff's cut operand holds {present:#x} -- something else has patched it"
        ));
    }
    if !unsafe { crate::patch::write_code_bytes(address, &want.to_le_bytes()) } {
        return Err("could not make EV_BloodPuff's cut writable".to_string());
    }
    Ok(true)
}

/// For `ds_debug_status`: what the client reads now, once this module has
/// looked at the code (a setting above 0 was asked for) or given up.
pub fn status_line() -> Option<String> {
    if GAVE_UP.load(Ordering::Relaxed) {
        return Some(format!(
            "own blood: {NAME} could not be applied (see the log)"
        ));
    }
    let address = ADDRESS.load(Ordering::Acquire);
    if address == 0 {
        return None;
    }
    // Safety: the operand found by `operand_address`, for this session's client.dll.
    let present = unsafe { (address as *const u32).read_unaligned() };
    let ours = &CUT as *const AtomicU32 as usize as u32;
    Some(if present == ours {
        format!(
            "own blood: puffs closer than {} units skipped (stock 32)",
            f32::from_bits(CUT.load(Ordering::Relaxed))
        )
    } else {
        "own blood: the game's own 32-unit cut".to_string()
    })
}

/// Once a frame from `commands::poll`, on the engine thread.
pub fn poll() {
    let ptr = CVAR.load(Ordering::Acquire);
    if ptr.is_null() || GAVE_UP.load(Ordering::Relaxed) {
        return;
    }
    // Safety: the engine owns the cvar for the session.
    let raw = unsafe { (*ptr).value };
    if SEEN.load(Ordering::Relaxed) == raw.to_bits() {
        return;
    }
    let (level, clamped) = level_for(raw);
    if clamped {
        crate::commands::console_print(&format!(
            "{NAME} takes 0 to {MAX_LEVEL}; {raw} gives {level}\n"
        ));
        // The cvar then reads the level in effect; the next frame applies it.
        if let Ok(line) = std::ffi::CString::new(format!("{NAME} {level}")) {
            engine::client_cmd(&line);
        }
        SEEN.store(raw.to_bits(), Ordering::Relaxed);
        return;
    }
    // Level 0 in a game that was never patched: nothing to put back.
    if level == 0 && STOCK_OPERAND.load(Ordering::Relaxed) == 0 {
        SEEN.store(raw.to_bits(), Ordering::Relaxed);
        return;
    }
    match apply(level) {
        Ok(wrote) => {
            SEEN.store(raw.to_bits(), Ordering::Relaxed);
            if wrote {
                unsafe {
                    crate::debug::report(&format!(
                        "own_blood: {NAME} = {level} (blood puffs closer than {} units skipped)",
                        cut_for(level)
                    ))
                };
            }
        }
        // Retry next frame until client.dll is there.
        Err(_) if engine::client_module_base().is_none() => {}
        Err(why) => {
            GAVE_UP.store(true, Ordering::Relaxed);
            crate::commands::console_print(&format!("{NAME}: not applied -- {why}\n"));
            unsafe { crate::debug::report(&format!("own_blood: {why}")) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_level_sets_its_cut() {
        let cuts: Vec<f32> = (0..=MAX_LEVEL).map(cut_for).collect();
        assert_eq!(cuts, [32.0, 24.0, 16.0, 8.0, 0.0]);
        assert_eq!(MAX_LEVEL, 4);
    }

    #[test]
    fn level_zero_is_the_game_s_own_cut() {
        assert_eq!(cut_for(0), 32.0);
        assert_eq!(CUT.load(Ordering::Relaxed), 32.0f32.to_bits());
    }

    #[test]
    fn values_in_range_are_kept_and_fractions_round_down() {
        for (raw, level) in [(0.0, 0), (1.0, 1), (2.0, 2), (3.0, 3), (4.0, 4), (2.9, 2)] {
            assert_eq!(level_for(raw), (level, false), "{raw}");
        }
    }

    #[test]
    fn values_out_of_range_clamp_and_say_so() {
        assert_eq!(level_for(5.0), (4, true));
        assert_eq!(level_for(100.0), (4, true));
        assert_eq!(level_for(-1.0), (0, true));
        assert_eq!(level_for(-0.5), (0, true));
        assert_eq!(level_for(f32::NAN), (0, true));
        assert_eq!(level_for(f32::INFINITY), (0, true));
        // 4.5 rounds down to 4, in range.
        assert_eq!(level_for(4.5), (4, false));
    }

    #[test]
    fn a_clamped_level_still_has_a_cut() {
        assert_eq!(cut_for(-3), 32.0);
        assert_eq!(cut_for(9), 0.0);
    }
}
