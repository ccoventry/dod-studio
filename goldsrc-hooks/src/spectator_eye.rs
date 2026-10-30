//! The eye-height part of `ds_spec_match_pov`: the in-eye camera drops
//! to the ground when the spectated player goes prone, as his own view does
//! (issue #329), and goes into and out of a crouch at his own view's speed
//! (issue #1144, [`crouch`]).
//!
//! ## DoD's in-eye camera has no prone case
//!
//! `V_GetInEyePos` (`client+0x50c90`) puts the camera at the spectated
//! player's origin plus a height picked from two replicated fields:
//!
//! ```text
//!     +0x50d2f  cmp  word [eax+0x2ea], 0     ; curstate.solid == SOLID_NOT
//!               ...                          ;   dead: roll 80, z -= 8
//!     +0x50d4d  mov  ecx, [eax+0x378]        ; curstate.usehull
//!     +0x50d56  cmp  ecx, 1
//!     +0x50d59  jne  +0x50d65
//!     +0x50d5b  fadd dword [18.0]            ;   small hull: z += 18
//!     +0x50d64  ret
//!     +0x50d65  fadd dword [22.0]            ;   otherwise:  z += 22
//! ```
//!
//! A prone player is in the small hull, the same one as a crouched player, so
//! he gets the crouch height. His own client knows better because the server
//! sends it `view_ofs` and `iuser3` in `clientdata`, and neither is sent for
//! other players (`dod/delta.lst`: `entity_state_player_t` has no `iuser*`,
//! and `usehull` is one bit).
//!
//! ## What the player's own view does
//!
//! Read out of POV demos (`analysis/examples/crosshair_pov_probe.rs`, the
//! `view_ofs[2]`, `eye`, `camera_z` and `origin_z` fields):
//!
//! - standing the eye is 22 above the origin, crouched 18, **prone 6 below**;
//! - going prone, `view_ofs[2]` becomes -6 on the same update that starts the
//!   body's `get_down` and sets `usehull` to 1. The camera is 28 lower on the
//!   very next frame, and then falls another 18 over 0.2s as the origin drops
//!   into the small hull;
//! - getting up, `usehull` goes back to 0 on the update that starts `get_up`,
//!   and the camera is at standing height 0.12s later.
//!
//! So nothing follows the 1.3s and 2.0s body animations: the view is prone
//! from the first frame of `get_down` and standing from the first of `get_up`.
//!
//! ## What a spectator has
//!
//! `usehull` 1, plus a body or gait sequence that only a prone player has:
//! `get_down`, `prone_*`, `bipod_*` (a machine gun deployed prone),
//! `hs_prone_*` (hand signals), or gait `prone_idle` / `prone_forward`.
//! `anim_fix` already reads those labels and calls [`set_prone`] each frame.
//! `get_up` is not in the list: `usehull` is 0 by then, and the game's own
//! +22 is right.
//!
//! ## What is patched
//!
//! Two operands. `fadd dword [18.0]` at `+0x50d5b` and `fadd dword [22.0]` at
//! `+0x50d65` are `D8 05 <address>`; the addresses are changed to point at
//! [`EYE_HEIGHT`] and [`STAND_HEIGHT`], floats in this DLL. The code is
//! written once when the switch turns on and once when it turns off; frame to
//! frame only the floats change.
//!
//! The small hull's float holds 18.0, -6.0 for a prone player, or the crouch
//! ease's height; the standing one 22.0 or the ease's height
//! ([`crouch`], issue #1144). The dead branch is untouched.

mod crouch;

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::engine::{self, RefParamsPartial};
use crate::scan;

/// `usehull` test, both `fadd`s and both returns, with the two addresses
/// wildcarded. Unique in `client.dll`, and the same in both builds.
const PATTERN: &str = "8B 88 78 03 00 00 D9 42 08 83 F9 01 75 0A D8 05 ?? ?? ?? ?? \
                       D9 5A 08 C3 D8 05 ?? ?? ?? ?? D9 5A 08 C3";

/// Where the small-hull `fadd`'s address sits inside the pattern.
const OPERAND_AT: usize = 16;
/// Where the standing `fadd`'s address sits inside the pattern.
const STAND_OPERAND_AT: usize = 26;

/// The game's own height for the small hull, which is right for crouching.
const CROUCH_EYE: f32 = 18.0;
/// The game's own standing height.
const STAND_EYE: f32 = 22.0;
/// `view_ofs[2]` while prone, from POV demos.
const PRONE_EYE: f32 = -6.0;

/// What the patched `fadd` adds. An `AtomicU32` is four plain bytes, which
/// is all the instruction reads; it holds an `f32`'s bits.
static EYE_HEIGHT: AtomicU32 = AtomicU32::new(CROUCH_EYE.to_bits());
/// What the patched standing `fadd` adds, the same way.
static STAND_HEIGHT: AtomicU32 = AtomicU32::new(STAND_EYE.to_bits());

/// Resolved address of the pattern, or 0 before the first successful scan.
static SPAN_ADDRESS: AtomicUsize = AtomicUsize::new(0);
/// The module base it was resolved against (see
/// `spectator_crosshair::SCANNED_BASE` for why this is kept).
static SCANNED_BASE: AtomicUsize = AtomicUsize::new(0);
/// The addresses the game shipped in the two operands, to put back.
static STOCK_OPERAND: AtomicUsize = AtomicUsize::new(0);
static STOCK_STAND_OPERAND: AtomicUsize = AtomicUsize::new(0);
/// Whether the operand points at [`EYE_HEIGHT`] now.
static PATCHED: AtomicBool = AtomicBool::new(false);
/// Whether the spectated player is prone this frame.
static PRONE: AtomicBool = AtomicBool::new(false);
/// Whether the crouch ease moved the camera away from the game's own
/// height on the last frame (for `ds_debug_status`).
static EASING: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// The crouch ease. Only `before_calc_refdef` touches it, on the
    /// engine's thread, so a thread-local rather than a lock.
    static CROUCH: RefCell<crouch::CrouchEase> = const { RefCell::new(crouch::CrouchEase::new()) };
}

/// Whether these replicated fields say the player is prone.
pub(crate) fn is_prone(usehull: i32, body: &str, gait: &str) -> bool {
    usehull == 1
        && (body == "get_down"
            || body.starts_with("prone_")
            || body.starts_with("bipod_")
            || body.starts_with("hs_prone_")
            || gait.starts_with("prone_"))
}

/// Says whether the spectated player is prone. Called every frame by
/// `anim_fix::apply()`, with `false` whenever there is no such player.
pub fn set_prone(prone: bool) {
    let height = if prone { PRONE_EYE } else { CROUCH_EYE };
    EYE_HEIGHT.store(height.to_bits(), Ordering::Relaxed);
    if PRONE.swap(prone, Ordering::Relaxed) != prone
        && crate::anim_fix::LOG_HELD_MODELS.load(Ordering::Relaxed)
    {
        unsafe {
            crate::debug::report(&format!(
                "spectator_eye: the spectated player is {}",
                if prone {
                    "prone -- eye 6 below their origin"
                } else {
                    "not prone -- the game's own eye height"
                }
            ))
        };
    }
}

/// The pattern's address in the loaded `client.dll`, scanned once per load.
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
        .map_err(|why| format!("could not find the in-eye camera's height -- {why}"))?;
    SPAN_ADDRESS.store(address, Ordering::Release);
    SCANNED_BASE.store(base, Ordering::Release);
    // A reloaded module comes back stock, at whatever address it likes.
    PATCHED.store(false, Ordering::Release);
    STOCK_OPERAND.store(0, Ordering::Release);
    STOCK_STAND_OPERAND.store(0, Ordering::Release);
    Ok(address)
}

/// Whether four bytes at `address` are inside the loaded `client.dll`.
fn inside_client(address: usize) -> bool {
    let Some(base) = engine::client_module_base() else {
        return false;
    };
    // Safety: `client_module_base` only returns a base for a mapped module.
    let Some(size) = (unsafe { crate::pe::image_size(base as *mut u8) }) else {
        return false;
    };
    address >= base && address.saturating_add(4) <= base + size
}

/// One of the two `fadd` operands and what it should read.
struct Operand {
    /// Where the operand's four bytes are.
    at: usize,
    /// This module's float.
    ours: usize,
    /// The game's constant it reads when stock.
    stock_value: f32,
    /// Where the game's own address is kept once seen.
    stock_slot: &'static AtomicUsize,
    /// "small-hull" or "standing", for the error text.
    name: &'static str,
}

/// Checks that an operand reads either this module's float or the game's
/// constant (remembering the game's address), so a foreign patch is refused
/// before anything is written.
fn check_operand(operand: &Operand) -> Result<(), String> {
    // Safety: the scan proved the pattern's bytes are mapped code.
    let present = unsafe { std::ptr::read_unaligned(operand.at as *const u32) } as usize;
    if present == operand.ours {
        return Ok(());
    }
    // Whatever is there must be the game's own constant. Anything else
    // adding a different height here is another patch, and not ours to
    // overwrite or to "restore" later.
    // Safety: read only after checking it lies inside the mapped module.
    let stock = inside_client(present)
        && unsafe { std::ptr::read_unaligned(present as *const f32) } == operand.stock_value;
    if !stock {
        return Err(format!(
            "the in-eye camera's {} height reads from {present:#x}, which is not the game's {:.1} -- something else has patched it",
            operand.name, operand.stock_value
        ));
    }
    operand.stock_slot.store(present, Ordering::Release);
    Ok(())
}

/// Points a checked operand at this module's float, or back at the game's
/// constant, returning whether anything was written.
fn point_operand(operand: &Operand, matching: bool) -> Result<bool, String> {
    let want = if matching {
        operand.ours
    } else {
        match operand.stock_slot.load(Ordering::Acquire) {
            // Ours is there and the game's address was never seen: only
            // possible if another copy of this DLL patched it. Leave it.
            0 => return Ok(false),
            stock => stock,
        }
    };
    // Safety: the scan proved the pattern's bytes are mapped code.
    let present = unsafe { std::ptr::read_unaligned(operand.at as *const u32) } as usize;
    if present == want {
        return Ok(false);
    }
    // Safety: writing to code the scan vouched for, through the same
    // protect/write/restore used everywhere else in this DLL.
    if !unsafe { crate::patch::write_code_bytes(operand.at, &(want as u32).to_le_bytes()) } {
        return Err(format!(
            "could not make the in-eye camera's {} height writable",
            operand.name
        ));
    }
    Ok(true)
}

/// Points both heights at this module's floats, or back at the game's
/// constants, returning whether anything was written. Idempotent and cheap
/// to call every frame, which is how `commands::poll` uses it.
pub fn set_matching(matching: bool) -> Result<bool, String> {
    let span = span_address()?;
    let operands = [
        Operand {
            at: span + OPERAND_AT,
            ours: EYE_HEIGHT.as_ptr() as usize,
            stock_value: CROUCH_EYE,
            stock_slot: &STOCK_OPERAND,
            name: "small-hull",
        },
        Operand {
            at: span + STAND_OPERAND_AT,
            ours: STAND_HEIGHT.as_ptr() as usize,
            stock_value: STAND_EYE,
            stock_slot: &STOCK_STAND_OPERAND,
            name: "standing",
        },
    ];
    // Both are checked before either is written, so a foreign patch on one
    // leaves the pair as it was rather than half ours.
    for operand in &operands {
        check_operand(operand)?;
    }
    let mut wrote = false;
    for operand in &operands {
        wrote |= point_operand(operand, matching)?;
    }
    PATCHED.store(matching, Ordering::Release);
    Ok(wrote)
}

/// Sets the crouch ease's height for this frame, before `client.dll`'s own
/// `V_CalcRefdef` runs `V_GetInEyePos` with it (issue #1144). Called from the
/// `V_CalcRefdef` trampoline, on the engine's thread, once this frame's
/// interpolation has placed every entity.
///
/// Stands down (the game's heights, the prone one included) when the switch
/// is off, in a POV demo, outside a player's eyes, and while he is prone.
///
/// Safety: `pparams` is the engine's `ref_params_s` for this view, valid for
/// the call, or null.
pub unsafe fn before_calc_refdef(pparams: *const RefParamsPartial) {
    // The inset view of picture-in-picture renders second; the main view's
    // height stands for the frame.
    if !unsafe { crate::spectator_chase_pvs::is_main_view(pparams) } {
        return;
    }
    // Safety: non-null, checked by `is_main_view`.
    let time = f64::from(unsafe { (*pparams).time });
    let height = crouch_frame(time);
    match height {
        Some(height) => {
            EYE_HEIGHT.store(height.to_bits(), Ordering::Relaxed);
            STAND_HEIGHT.store(height.to_bits(), Ordering::Relaxed);
        }
        // The small hull's float is `set_prone`'s, which ran this frame.
        None => STAND_HEIGHT.store(STAND_EYE.to_bits(), Ordering::Relaxed),
    }
    let easing = height.is_some_and(|h| h != CROUCH_EYE && h != STAND_EYE);
    if EASING.swap(easing, Ordering::Relaxed) != easing
        && crate::anim_fix::LOG_HELD_MODELS.load(Ordering::Relaxed)
    {
        unsafe {
            crate::debug::report(&format!(
                "spectator_eye: crouch ease {} at {time:.3}",
                if easing { "started" } else { "ended" }
            ))
        };
    }
}

/// The crouch ease's height for this frame, or `None` when the game's own
/// heights stand (and the ease is forgotten, so it starts fresh).
fn crouch_frame(time: f64) -> Option<f32> {
    let frame = crate::anim_fix::active()
        .then(crate::spectator_target::in_eye_target)
        .flatten()
        .filter(|_| !PRONE.load(Ordering::Relaxed))
        .and_then(|target| watched_frame(time, target));
    CROUCH.with(|ease| {
        let mut ease = ease.borrow_mut();
        match frame {
            Some(frame) => Some(ease.eye_height(&frame)),
            None => {
                *ease = crouch::CrouchEase::new();
                None
            }
        }
    })
}

/// What the ease needs from the watched player's entity this frame.
fn watched_frame(time: f64, target: i32) -> Option<crouch::Frame> {
    let engfuncs = engine::engfuncs()?;
    // Safety: the engine's own entity table; null for a bad index.
    let player = unsafe { (engfuncs.get_entity_by_index)(target) };
    if player.is_null() {
        return None;
    }
    // Safety: non-null, and the engine keeps it for the frame.
    let player = unsafe { &*player };
    let snapshot = |state: &engine::EntityStateS| crouch::Snapshot {
        msg_time: state.msg_time,
        z: state.origin.z,
        small_hull: state.usehull == 1,
    };
    let newest = player.curstate.msg_time;
    let started = std::time::Instant::now();
    let timeline = crate::lookahead::timeline(f64::from(newest));
    let upcoming_down = timeline
        .as_ref()
        .and_then(|t| t.next_crouch_down(target, newest, crouch::LOOK_AHEAD_SECONDS));
    crate::lookahead::note_frame(
        started.elapsed(),
        timeline.as_ref(),
        target,
        time,
        newest,
        player.curstate.usehull == 1,
        upcoming_down,
    );
    Some(crouch::Frame {
        time,
        target,
        drawn_z: player.origin.z,
        snapshot: snapshot(&player.curstate),
        previous: snapshot(&player.prevstate),
        upcoming_down,
    })
}

/// One line for `ds_debug_status`.
pub fn status() -> String {
    if !PATCHED.load(Ordering::Relaxed) {
        return "the in-eye camera uses the game's own heights (a prone player is seen from crouch height, and a crouch drops at once)".to_string();
    }
    format!(
        "the in-eye camera drops to the ground for a prone player and eases through a crouch; right now they are {}{}",
        if PRONE.load(Ordering::Relaxed) {
            "prone"
        } else {
            "not prone"
        },
        if EASING.load(Ordering::Relaxed) {
            ", mid-crouch"
        } else {
            ""
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The operand offset is read off the disassembly in the module docs.
    /// Anchoring it to the pattern means a mistyped pattern cannot quietly
    /// move it onto an opcode.
    #[test]
    fn the_patched_operand_lands_on_the_first_wildcards() {
        let tokens: Vec<&str> = PATTERN.split_whitespace().collect();
        assert_eq!(&tokens[OPERAND_AT - 2..OPERAND_AT], ["D8", "05"]);
        assert!(
            tokens[OPERAND_AT..OPERAND_AT + 4]
                .iter()
                .all(|t| *t == "??")
        );
        assert_eq!(tokens[OPERAND_AT + 4], "D9");
        // And it is the first `fadd`, the small-hull one, not the 22.0 one.
        assert_eq!(tokens.iter().position(|t| *t == "??"), Some(OPERAND_AT));
    }

    /// The standing `fadd`'s operand is the second run of wildcards, after
    /// the small-hull branch's `fstp` and `ret`.
    #[test]
    fn the_standing_operand_lands_on_the_second_wildcards() {
        let tokens: Vec<&str> = PATTERN.split_whitespace().collect();
        assert_eq!(
            &tokens[STAND_OPERAND_AT - 2..STAND_OPERAND_AT],
            ["D8", "05"]
        );
        assert!(
            tokens[STAND_OPERAND_AT..STAND_OPERAND_AT + 4]
                .iter()
                .all(|t| *t == "??")
        );
        assert_eq!(tokens.len(), STAND_OPERAND_AT + 8);
        assert_eq!(
            &tokens[STAND_OPERAND_AT - 6..STAND_OPERAND_AT - 2],
            ["D9", "5A", "08", "C3"]
        );
    }

    #[test]
    fn prone_is_the_small_hull_with_a_prone_sequence() {
        for (body, gait) in [
            ("get_down", "look_idle"),
            ("prone_rifle_aim", "prone_idle"),
            ("prone_forward", "prone_forward"),
            ("prone_garand_reload", "prone_idle"),
            ("bipod_mg_aim", "prone_idle"),
            ("hs_prone_grenade", "prone_idle"),
            // The body mid-change, the gait already there.
            ("look_idle", "prone_forward"),
        ] {
            assert!(is_prone(1, body, gait), "{body} / {gait}");
        }
    }

    #[test]
    fn crouching_standing_and_getting_up_are_not_prone() {
        for (usehull, body, gait) in [
            (1, "crouch_rifle_aim", "dod_crouch_idle"),
            (1, "crouch_rifle_aim", "dod_crawl"),
            (0, "stand_rifle_aim", "dod_idle1"),
            // `usehull` is back to 0 on the update that starts `get_up`.
            (0, "get_up", "look_idle"),
            // A sandbag deploy is standing or crouched, never prone.
            (1, "sandbag_mg_aim", "dod_crouch_idle"),
            // Not the small hull, whatever a stale sequence says.
            (0, "prone_rifle_aim", "prone_idle"),
        ] {
            assert!(!is_prone(usehull, body, gait), "{usehull} {body} / {gait}");
        }
    }

    #[test]
    fn the_float_follows_the_stance() {
        set_prone(true);
        assert_eq!(f32::from_bits(EYE_HEIGHT.load(Ordering::Relaxed)), -6.0);
        set_prone(false);
        assert_eq!(f32::from_bits(EYE_HEIGHT.load(Ordering::Relaxed)), 18.0);
    }
}
