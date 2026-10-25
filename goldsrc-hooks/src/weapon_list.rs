//! `hud_fastswitch 1` keeps the weapon-selection list (the column of weapon
//! icons on the middle right) off screen while a demo plays (#1032). With
//! `hud_fastswitch 0` playback shows the list as the recording player saw
//! it, and live play is left as shipped either way. The weapon still
//! switches as recorded; only the list is not drawn.
//!
//! ## What brings the list up in playback
//!
//! Read from the shipped `client.dll` (one file, byte-identical on the PRE and
//! POST installs). A POV demo records the player's `slot1`..`slot10`,
//! `invnext` and `invprev` presses as console-command frames, and playback runs
//! them through the viewer's own `CHudAmmo` handlers:
//!
//! - `WeaponsResource::SelectSlot` (`+0x28190`, behind every `slotN`) honours
//!   `hud_fastswitch` only when the slot's bucket holds a single weapon, and
//!   DoD's `HasAmmo` (`+0x26f90`) is a bare `return 1`, so every carried
//!   weapon counts; a grenade count is ammo on one entry and doesn't. With two
//!   entries in the bucket it opens the list whatever `hud_fastswitch` says,
//!   as it did on the recording player's own screen. The usual second entry
//!   is a primed grenade: a player who rolls a grenade out and catches it
//!   again with USE holds it as a weapon of its own (id 15 for the stick,
//!   16 for the hand grenade, both in bucket 5 beside 14 and 13), while the
//!   class's grenade weapon stays for the one still in the pouch. Their next
//!   `slot5` shows the list until the recorded `+attack` picks one.
//! - With `hud_fastswitch 0`, every `slotN` opens it.
//! - `invnext`/`invprev` (`+0x288d0`, `+0x289c0`) switch straight away with
//!   `hud_fastswitch 1`; with 0 they open it.
//!
//! The list then stays up until a recorded `+attack` picks a weapon, or a
//! death or `HideWeapon` closes it. A recorder who plays with fast switch
//! presses no `+attack` after a `slotN`, so with `hud_fastswitch 0` the list
//! can stay up for many seconds -- which is what that viewer asked for.
//!
//! So with `hud_fastswitch 1` the only list playback can show is the
//! grenade bucket's, which a viewer who chose fast switch never wants over
//! the shot; this takes that one away too.
//!
//! ## What this patches: one displacement byte
//!
//! `CHudAmmo::DrawWeaponList` (`+0x29530`, called only from
//! `CHudAmmo::Draw`, which ignores its result) begins:
//!
//! ```text
//!     mov  eax, [gpActiveSel]
//!     sub  esp, 0x1c
//!     test eax, eax
//!     jne  .draw           ; 75 08
//!     xor  eax, eax        ; nothing selected: draw nothing
//!     add  esp, 0x1c
//!     ret  4
//! .draw:
//! ```
//!
//! Writing `00` over the `jne`'s displacement makes both of its outcomes land
//! on the `xor`, so the function always returns without drawing. The opcode
//! stays, and nothing that selects a weapon is touched: `gpActiveSel`,
//! `SelectSlot`, the `+attack` pick in `CHudAmmo::Think` all run as shipped.
//! [`poll`] writes it only while a demo plays with `hud_fastswitch` on, and
//! puts the shipped byte back the frame either stops being true, so live
//! play is left as shipped.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize, Ordering};

use crate::engine::{self, CvarSPartial};
use crate::scan;

/// The stock cvar this follows. Registered by `client.dll` (`gHUD+0x6e2b8`
/// caches its pointer), looked up by name.
const FASTSWITCH: &std::ffi::CStr = c"hud_fastswitch";

/// `CHudAmmo::DrawWeaponList`'s whole early-out, wildcarded over
/// `&gpActiveSel`. Unique in `client.dll`.
const PATTERN: &str = "A1 ?? ?? ?? ?? 83 EC 1C 85 C0 75 08 33 C0 83 C4 1C C2 04 00";

/// The `jne`'s displacement byte within [`PATTERN`].
const DISP_AT: usize = 11;

/// As shipped: `jne` skips the 8-byte early return to the drawing code.
const DRAW: u8 = 0x08;
/// Patched: `jne` lands on the next instruction, the early return.
const SKIP: u8 = 0x00;

/// `hud_fastswitch`, once the client has registered it.
static FASTSWITCH_CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());

/// The resolved displacement byte and the module base it belongs to.
static DISP_ADDRESS: AtomicUsize = AtomicUsize::new(0);
static RESOLVED_BASE: AtomicUsize = AtomicUsize::new(0);

/// Whether the list is hidden in the loaded module.
static HIDDEN: AtomicBool = AtomicBool::new(false);
/// Frames the list would have been drawn on while hidden, for the status line.
static SUPPRESSED_FRAMES: AtomicU32 = AtomicU32::new(0);
/// `&gpActiveSel`, read from the pattern, for that count.
static ACTIVE_SEL: AtomicUsize = AtomicUsize::new(0);
/// So a failure is reported once, not every frame.
static COMPLAINED: AtomicBool = AtomicBool::new(false);

/// Whether the list should be kept off screen: only in demo playback, and
/// only for a viewer who chose fast switch.
fn should_hide(playing_demo: bool, fastswitch: bool) -> bool {
    playing_demo && fastswitch
}

/// The byte `apply` wants at the displacement, or an error naming a byte
/// neither this module nor the shipped game put there.
fn wanted_byte(present: u8, hide: bool) -> Result<Option<u8>, String> {
    let want = if hide { SKIP } else { DRAW };
    if present == want {
        return Ok(None);
    }
    if present != DRAW && present != SKIP {
        return Err(format!(
            "DrawWeaponList's jne holds displacement {present:#04x}, which is neither {DRAW:#04x} nor {SKIP:#04x} -- something else has patched it"
        ));
    }
    Ok(Some(want))
}

/// `hud_fastswitch`'s value now, or `None` until the client registers it.
fn fastswitch_on() -> Option<bool> {
    let mut cvar = FASTSWITCH_CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        let engfuncs = engine::engfuncs()?;
        // Safety: a plain lookup by name; null when not registered.
        cvar = unsafe { (engfuncs.pfn_get_cvar_pointer)(FASTSWITCH.as_ptr()) };
        if cvar.is_null() {
            return None;
        }
        FASTSWITCH_CVAR.store(cvar, Ordering::Release);
    }
    // Safety: the engine owns the cvar for the session.
    Some(unsafe { (*cvar).value } != 0.0)
}

/// Resolves the displacement byte for the loaded `client.dll`.
fn resolve() -> Result<usize, String> {
    let base = engine::client_module_base().ok_or("client.dll is not loaded yet")?;
    if RESOLVED_BASE.load(Ordering::Acquire) == base {
        let cached = DISP_ADDRESS.load(Ordering::Acquire);
        if cached != 0 {
            return Ok(cached);
        }
    }
    // Safety: `client_module_base` only returns a base for a mapped module.
    let found = unsafe { scan::find_unique(base, PATTERN) }
        .map_err(|why| format!("could not find CHudAmmo::DrawWeaponList -- {why}"))?;
    let active_sel = unsafe { std::ptr::read_unaligned((found + 1) as *const u32) } as usize;
    if !(base..base + 0x0100_0000).contains(&active_sel) {
        return Err(format!("gpActiveSel {active_sel:#x} is outside client.dll"));
    }
    let address = found + DISP_AT;
    ACTIVE_SEL.store(active_sel, Ordering::Release);
    DISP_ADDRESS.store(address, Ordering::Release);
    RESOLVED_BASE.store(base, Ordering::Release);
    Ok(address)
}

/// Hides or shows the list, writing only when the byte differs.
fn apply(hide: bool) -> Result<(), String> {
    let address = resolve()?;
    // Safety: code inside the mapped module, found by `resolve`.
    let present = unsafe { *(address as *const u8) };
    if let Some(want) = wanted_byte(present, hide)?
        && !unsafe { crate::patch::write_code_bytes(address, &[want]) }
    {
        return Err("could not make DrawWeaponList writable".to_string());
    }
    if hide != HIDDEN.swap(hide, Ordering::Relaxed) {
        unsafe {
            crate::debug::report(&format!(
                "weapon_list: {}",
                if hide {
                    "demo playback with hud_fastswitch 1, the weapon list is not drawn"
                } else {
                    "the weapon list draws as shipped"
                }
            ))
        };
    }
    Ok(())
}

/// Called every frame from `commands::poll`: hides the list while a demo
/// plays with `hud_fastswitch` on, and puts it back otherwise. Until the
/// first time that holds, `client.dll` stays exactly as shipped.
pub fn poll() {
    if engine::client_module_base().is_none() {
        return;
    }
    let Some(fastswitch) = fastswitch_on() else {
        return;
    };
    let hide = should_hide(crate::spray_hltv::playing_demo(), fastswitch);
    if (hide || HIDDEN.load(Ordering::Relaxed))
        && let Err(why) = apply(hide)
        && !COMPLAINED.swap(true, Ordering::Relaxed)
    {
        unsafe { crate::debug::report(&format!("weapon_list: {why}")) };
    }
    let active_sel = ACTIVE_SEL.load(Ordering::Acquire);
    if HIDDEN.load(Ordering::Relaxed)
        && active_sel != 0
        // Safety: a global inside client.dll's image, checked in `resolve`.
        && unsafe { std::ptr::read_volatile(active_sel as *const u32) } != 0
    {
        SUPPRESSED_FRAMES.fetch_add(1, Ordering::Relaxed);
    }
}

/// A `ds_debug_status` line, or `None` until the list was first hidden.
pub fn status_line() -> Option<String> {
    let frames = SUPPRESSED_FRAMES.load(Ordering::Relaxed);
    let hidden = HIDDEN.load(Ordering::Relaxed);
    if !hidden && frames == 0 {
        return None;
    }
    Some(format!(
        "weapon list: {}; it was up, unseen, for {frames} frame(s) this session",
        if hidden {
            "not drawn (demo playback, hud_fastswitch 1)"
        } else {
            "drawn as shipped"
        }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens() -> Vec<&'static str> {
        PATTERN.split_whitespace().collect()
    }

    #[test]
    fn hidden_only_in_playback_with_fastswitch() {
        assert!(should_hide(true, true));
        assert!(
            !should_hide(true, false),
            "fastswitch 0: as the recorder saw it"
        );
        assert!(!should_hide(false, true), "live play is left as shipped");
        assert!(!should_hide(false, false));
    }

    #[test]
    fn disp_at_indexes_the_jne_displacement() {
        let t = tokens();
        assert_eq!(t[DISP_AT - 1], "75", "the byte before is the jne opcode");
        assert_eq!(u8::from_str_radix(t[DISP_AT], 16).unwrap(), DRAW);
    }

    /// The displacement skips exactly the early return, so 0 lands on it.
    #[test]
    fn the_shipped_displacement_skips_exactly_the_early_return() {
        let t = tokens();
        let early_return = &t[DISP_AT + 1..];
        assert_eq!(
            early_return,
            ["33", "C0", "83", "C4", "1C", "C2", "04", "00"]
        );
        assert_eq!(early_return.len(), DRAW as usize);
    }

    #[test]
    fn the_pattern_is_well_formed() {
        for token in tokens() {
            assert!(
                token == "??" || u8::from_str_radix(token, 16).is_ok(),
                "{token:?}"
            );
        }
        assert_eq!(tokens()[0], "A1", "starts with mov eax, [gpActiveSel]");
    }

    #[test]
    fn writes_only_what_differs_and_refuses_a_foreign_byte() {
        assert_eq!(wanted_byte(DRAW, false), Ok(None));
        assert_eq!(wanted_byte(SKIP, true), Ok(None));
        assert_eq!(wanted_byte(DRAW, true), Ok(Some(SKIP)));
        assert_eq!(wanted_byte(SKIP, false), Ok(Some(DRAW)));
        assert!(wanted_byte(0x10, true).is_err());
    }
}
