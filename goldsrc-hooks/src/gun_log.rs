//! `ds_debug_log_gun_lowered`: logs each time the first-person gun goes
//! down or comes back up, with the demo's clock, so a POV demo and the HLTV
//! demo of the same match can be lined up and compared (issue #704).
//!
//! - **POV** (the game's own view): `client.dll`'s `g_ihidexhair`, which
//!   `DoDGunGoOnOffScreen` sets to 1 on every frame the gun is lowered and to
//!   0 otherwise (`+0x2c9c6` / `+0x2c9da` / `+0x2ca0a`; the same file on both
//!   installs).
//! - **HLTV in-eye** (`ds_spec_match_pov`'s copy of it): whether
//!   `spectator_gun` was told the watched player's gun is lowered this frame.
//!
//! Each line carries the demo player's clock under `viewdemo` (the same clock
//! for a POV and an HLTV demo of one match), else the hook's own clock.
//!
//! **Limit:** the POV side is the client's own lowering sum while the demo
//! plays, not the gun that is drawn, so it misses the lowering a POV demo
//! shows on a jump or a crouch-jump. A POV-vs-HLTV difference it reports is
//! a lead to check against the drawn frames, not a finding (#704).

use std::sync::atomic::{AtomicPtr, AtomicU8, AtomicUsize, Ordering};

use crate::engine::{self, CvarSPartial};
use crate::names::console_name;

pub const NAME: &str = console_name!("debug_log_gun_lowered");

/// The end of `DoDGunGoOnOffScreen`'s lowered branch: `cmp eax, 0x37; jge;
/// fadd [one]; mov [g_ihidexhair], 1`. Unique in `client.dll`.
const PATTERN: &str = "83 F8 37 7D 06 D8 05 ?? ?? ?? ?? C7 05 ?? ?? ?? ?? 01 00 00 00";
/// Where `g_ihidexhair`'s address sits in the pattern.
const FLAG_OPERAND_AT: usize = 13;

static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
/// `g_ihidexhair`'s address, 1 when it can't be found, 0 before a look.
static FLAG: AtomicUsize = AtomicUsize::new(0);
static FLAG_BASE: AtomicUsize = AtomicUsize::new(0);
/// The last state logged: 0 none, then `1 + source * 2 + lowered`.
static LAST: AtomicU8 = AtomicU8::new(0);
/// Why `anim_fix` lowered the watched player's gun this frame
/// (`crosshair_rule::Hidden` as u8, 0 when it didn't).
static SPEC_REASON: AtomicU8 = AtomicU8::new(0);

/// `crosshair_rule::Hidden`'s names, by value (8 to 12 are the lowered ones).
const REASONS: [&str; 13] = [
    "-",
    "dead",
    "no weapon",
    "no crosshair weapon",
    "undeployed mg",
    "just drew",
    "reloading",
    "cycling bolt",
    "sprinting",
    "prone transition",
    "crawling",
    "in the air",
    "on a ladder",
];

/// Called every frame by `anim_fix` with why the gun is lowered, or 0.
pub fn note_spec_reason(code: u8) {
    SPEC_REASON.store(code, Ordering::Relaxed);
}

pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

fn report(line: &str) {
    unsafe { crate::debug::report(&format!("gun_log: {line}")) };
}

fn flag_address() -> Option<usize> {
    let base = engine::client_module_base()?;
    if FLAG_BASE.load(Ordering::Acquire) == base {
        return match FLAG.load(Ordering::Acquire) {
            0 | 1 => None,
            found => Some(found),
        };
    }
    FLAG_BASE.store(base, Ordering::Release);
    // Safety: a module base the engine gave us, mapped for the session.
    let found = unsafe { crate::scan::find_unique(base, PATTERN) }.map(|at| unsafe {
        std::ptr::read_unaligned((at + FLAG_OPERAND_AT) as *const u32)
    } as usize);
    match found {
        Ok(address) if (base..base + 0x0100_0000).contains(&address) => {
            FLAG.store(address, Ordering::Release);
            Some(address)
        }
        Ok(address) => {
            report(&format!("g_ihidexhair {address:#x} is outside client.dll"));
            FLAG.store(1, Ordering::Release);
            None
        }
        Err(why) => {
            report(&format!("could not find g_ihidexhair -- {why}"));
            FLAG.store(1, Ordering::Release);
            None
        }
    }
}

fn clock() -> f64 {
    crate::demo_seek::clock()
        .filter(|c| c.active && !c.loading)
        .map_or_else(engine::client_time, |c| c.now)
}

/// Runs every frame.
pub fn poll() {
    let cvar = CVAR.load(Ordering::Acquire);
    if cvar.is_null() || unsafe { (*cvar).value } == 0.0 {
        LAST.store(0, Ordering::Relaxed);
        return;
    }
    let (source, lowered) = match crate::spectator_target::in_eye_target() {
        Some(target) => {
            let why = REASONS
                .get(usize::from(SPEC_REASON.load(Ordering::Relaxed)))
                .copied()
                .unwrap_or("?");
            (
                format!("spec {target}: {why}"),
                crate::spectator_gun::lowered(),
            )
        }
        None => {
            let Some(address) = flag_address() else {
                return;
            };
            // Safety: a static int inside client.dll, written by the game on
            // this thread.
            (
                "pov".to_string(),
                unsafe { std::ptr::read_volatile(address as *const i32) } != 0,
            )
        }
    };
    let state = 1 + u8::from(source != "pov") * 2 + u8::from(lowered);
    // A change of reason while lowered is worth a line too.
    let state = state.wrapping_add(SPEC_REASON.load(Ordering::Relaxed).wrapping_mul(4));
    if LAST.swap(state, Ordering::Relaxed) != state {
        report(&format!(
            "{} at {:.3} ({source})",
            if lowered { "down" } else { "up" },
            clock()
        ));
    }
}
