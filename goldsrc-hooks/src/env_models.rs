//! The map's own models -- sandbags, banisters, a tank -- drawn in a POV demo
//! played as HLTV (`dem_forcehltv 1`), where none of them showed.
//!
//! ## How DoD draws them
//!
//! A map's `env_model`s are not networked entities: no demo holds a baseline
//! or a packet entity for one. DoD's `client.dll` draws them itself.
//! `InitHUD` makes it read the map's entity lump and keep every `env_model`
//! in `CClientEnvModel` (`client+0x10cccc`, at most 192: model, origin,
//! angles, body, spawnflags; a spawnflags-2 model is dropped to the floor
//! there). `CClientEnvModel::Think` (`client+0x29b10`) then keeps one temp
//! entity alive, `models/p_stick.mdl` with `entityType` 4, at the camera
//! (`v_origin` while spectating, else the local player), and DoD's
//! `StudioDrawModel` (its loop at `client+0x4a3a9`) draws every stored model,
//! PVS-culled, each time it is asked to draw that carrier. No carrier, no map models. Names from the
//! Linux `client.so`'s symbols; the addresses are the Windows build's.
//!
//! ## Why they vanished under `dem_forcehltv 1`
//!
//! `Think` runs from `CHud::Think`, called from `HUD_UpdateClientData`,
//! which the engine does not call for a POV demo played as HLTV (the same
//! gap `hltv_fov` fills for the HUD's FOV, #1462). `InitHUD` still arrives
//! and empties the carrier pointer, so nothing ever creates the carrier
//! again. Measured on `tr2.dem` (PRE): the table held the same 20 models at
//! the same heights in a normal and a forcehltv session, the carrier pointer
//! was set throughout the normal one and stayed NULL in the forcehltv one,
//! and every sandbag and banister on `dod_avalanche` was missing there.
//!
//! ## The fix
//!
//! [`poll`], once a frame on the engine's thread: in a spectate-only demo
//! session, while the table holds models and the carrier pointer is NULL,
//! call the game's own `Think` on the game's own object -- exactly the call
//! `CHud::Think` makes. It only acts on a NULL pointer, which `InitHUD`
//! leaves and `Think` replaces, so in an HLTV demo, where `CHud::Think` runs
//! every frame, it finds nothing to do or makes the same call a moment
//! earlier. A pointer left over from an earlier level is never touched:
//! `InitHUD` clears it before the new level's models are stored.
//!
//! `client.dll` is byte-identical in both movie installs, so one set of
//! patterns covers both; `tools/verify_env_models_offsets.py` checks them.
//! On by default; `GOLDSRC_HOOKS_ENV_MODELS=0` turns it off.

// The call is a thiscall, which only the 32-bit build has; the host build
// (clippy, tests) compiles the rest.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::engine;
use crate::scan;

/// Whether to act at all -- `GOLDSRC_HOOKS_ENV_MODELS=0` turns it off.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

/// `Parse_env_model`'s call to `CClientEnvModel::AddEnvModel`:
/// `lea eax, [esp+0xc]; mov ecx, <object>; push eax; call AddEnvModel`.
const ADD_CALL_PATTERN: &str = "8D 44 24 0C B9 ?? ?? ?? ?? 50 E8 ?? ?? ?? ??";
/// Where the object's address sits in [`ADD_CALL_PATTERN`].
const OBJECT_AT: usize = 5;
/// The call's `rel32`, and the end of the call it is measured from.
const CALL_REL_AT: usize = 11;
const CALL_END: usize = 15;
/// `CClientEnvModel::AddEnvModel` (`client+0x298e0`): `this` in `ebx`, the
/// count at `this+0x8418` compared with the 192 limit.
const ADD_PATTERN: &str = "83 EC 50 53 8B D9 8B 83 18 84 00 00 3D C0 00 00 00 7C";
/// `CClientEnvModel::Think` (`client+0x29b10`): `this` in `esi`, the
/// carrier at `this+0x14`, its `die` compared with 0.
const THINK_PATTERN: &str =
    "83 EC 10 56 8B F1 8B 4E 14 85 C9 74 14 D9 41 04 D8 1D ?? ?? ?? ?? DF E0";
/// `Think`'s store of the new carrier and the flags it gives it
/// (`FTENT_PERSIST | FTENT_CLIENTCUSTOM | 0x100000`), at [`THINK_STORE_AT`].
const THINK_STORE_PATTERN: &str = "89 46 14 85 C0 74 21 8B 08 81 C9 00 20 18 00";
const THINK_STORE_AT: usize = 0x82;
/// The carrier temp entity's pointer, and the number of stored models.
pub const CARRIER_OFFSET: usize = 0x14;
pub const COUNT_OFFSET: usize = 0x8418;
/// `AddEnvModel`'s limit.
const MAX_MODELS: i32 = 192;

/// What [`locate`] found: `CClientEnvModel` and its `Think`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Code {
    object: usize,
    think: usize,
}

/// The `client.dll` base [`OBJECT`] and [`THINK`] were found at; a refusal
/// is remembered by [`REFUSED_BASE`] and logged once.
static FOUND_BASE: AtomicUsize = AtomicUsize::new(0);
static OBJECT: AtomicUsize = AtomicUsize::new(0);
static THINK: AtomicUsize = AtomicUsize::new(0);
static REFUSED_BASE: AtomicUsize = AtomicUsize::new(0);
/// How many carriers [`poll`] has had `Think` create this session.
static CREATED: AtomicU32 = AtomicU32::new(0);
/// Whether [`poll`] has logged a `Think` that made no carrier.
static NO_FREE_LOGGED: AtomicBool = AtomicBool::new(false);
/// Lines [`poll`] writes to the log before it only counts.
const LOG_LINES: u32 = 4;

/// The object's address from the call site's bytes as loaded at `site`,
/// after checking the call lands on `add` and the object lies inside the
/// image (`base`, `image_size`) with room for the count.
fn object_from_call(
    site: usize,
    call: &[u8],
    add: usize,
    base: usize,
    image_size: usize,
) -> Result<usize, String> {
    if call.len() < CALL_END {
        return Err("the AddEnvModel call site is cut short".to_string());
    }
    let word = |at: usize| u32::from_le_bytes([call[at], call[at + 1], call[at + 2], call[at + 3]]);
    let target = (site + CALL_END).wrapping_add_signed(word(CALL_REL_AT) as i32 as isize);
    if target != add {
        return Err(format!(
            "the env_model call at +{:#x} goes to +{:#x}, not AddEnvModel at +{:#x}",
            site.wrapping_sub(base),
            target.wrapping_sub(base),
            add.wrapping_sub(base)
        ));
    }
    let object = word(OBJECT_AT) as usize;
    if object < base || object + COUNT_OFFSET + 4 > base + image_size {
        return Err(format!(
            "the env_model call passes {object:#x}, outside client.dll"
        ));
    }
    Ok(object)
}

/// The scans, for the module at `base`.
fn scan_client(base: usize) -> Result<Code, String> {
    // Safety: `client_module_base` only returns a mapped module's base.
    let image_size = unsafe { crate::pe::image_size(base as *mut u8) }
        .ok_or_else(|| "client.dll has no readable PE header".to_string())?;
    let site = unsafe { scan::find_unique(base, ADD_CALL_PATTERN) }
        .map_err(|why| format!("could not find Parse_env_model's AddEnvModel call -- {why}"))?;
    let add = unsafe { scan::find_unique(base, ADD_PATTERN) }
        .map_err(|why| format!("could not find CClientEnvModel::AddEnvModel -- {why}"))?;
    let think = unsafe { scan::find_unique(base, THINK_PATTERN) }
        .map_err(|why| format!("could not find CClientEnvModel::Think -- {why}"))?;
    let store = unsafe { scan::find_unique(base, THINK_STORE_PATTERN) }
        .map_err(|why| format!("could not find Think's carrier store -- {why}"))?;
    if store != think + THINK_STORE_AT {
        return Err(format!(
            "Think's carrier store is at +{:#x}, not +{:#x}",
            store - base,
            think + THINK_STORE_AT - base
        ));
    }
    // Safety: the pattern matched there, so its bytes are mapped code.
    let call = unsafe { std::slice::from_raw_parts(site as *const u8, CALL_END) };
    let object = object_from_call(site, call, add, base, image_size)?;
    Ok(Code { object, think })
}

/// The object and `Think`, scanning once per `client.dll`.
fn locate() -> Option<Code> {
    let base = engine::client_module_base()?;
    if FOUND_BASE.load(Ordering::Acquire) == base {
        return Some(Code {
            object: OBJECT.load(Ordering::Acquire),
            think: THINK.load(Ordering::Acquire),
        });
    }
    if REFUSED_BASE.load(Ordering::Acquire) == base {
        return None;
    }
    match scan_client(base) {
        Ok(code) => {
            OBJECT.store(code.object, Ordering::Release);
            THINK.store(code.think, Ordering::Release);
            FOUND_BASE.store(base, Ordering::Release);
            Some(code)
        }
        Err(why) => {
            REFUSED_BASE.store(base, Ordering::Release);
            unsafe {
                crate::debug::report(&format!(
                    "env_models: the map models fix is off this session: {why}"
                ))
            };
            None
        }
    }
}

/// Whether `Think` should be called: the table holds models and no carrier
/// draws them.
fn needs_carrier(count: i32, carrier: usize) -> bool {
    carrier == 0 && (1..=MAX_MODELS).contains(&count)
}

/// Whether a demo is playing as HLTV (a real HLTV demo or a POV demo with
/// `dem_forcehltv 1`).
fn spectating_a_demo() -> bool {
    let Some(engfuncs) = engine::engfuncs() else {
        return false;
    };
    let api = engfuncs.p_demo_api;
    // Safety: the engine's own functions, called on its thread.
    unsafe {
        (engfuncs.is_spectate_only)() != 0 && !api.is_null() && ((*api).is_playingback)() != 0
    }
}

#[cfg(target_arch = "x86")]
type ThinkFn = unsafe extern "thiscall" fn(*mut u8);

/// Calls the game's `Think` on its object.
///
/// # Safety
///
/// `code` is [`locate`]'s, and this runs on the engine's thread.
#[cfg(target_arch = "x86")]
unsafe fn call_think(code: Code) {
    // Safety: the pattern-checked `CClientEnvModel::Think`, a thiscall with
    // no arguments, on the object `Parse_env_model` passes it.
    let think: ThinkFn = unsafe { std::mem::transmute::<usize, ThinkFn>(code.think) };
    unsafe { think(code.object as *mut u8) };
}

#[cfg(not(target_arch = "x86"))]
unsafe fn call_think(_code: Code) {}

/// Has the game's `Think` create the map models' carrier when a demo played
/// as HLTV left it missing. Runs once a frame from the command poll; only
/// acts on the engine's thread (`HUD_Frame`), never the fallback timer.
pub fn poll() {
    if !ENABLED.load(Ordering::Relaxed) || !engine::hud_frame_driving() || !spectating_a_demo() {
        return;
    }
    let Some(code) = locate() else {
        return;
    };
    // Safety: `object_from_call` checked the object and its count lie inside
    // client.dll's image: plain `.data` the engine's thread owns.
    let (count, carrier) = unsafe {
        (
            *((code.object + COUNT_OFFSET) as *const i32),
            *((code.object + CARRIER_OFFSET) as *const usize),
        )
    };
    if !needs_carrier(count, carrier) {
        return;
    }
    // Safety: as `call_think` asks; `poll` runs on the engine's thread.
    unsafe { call_think(code) };
    let made = unsafe { *((code.object + CARRIER_OFFSET) as *const usize) } != 0;
    if !made {
        // Asked again next frame, as `CHud::Think` would; logged once.
        if !NO_FREE_LOGGED.swap(true, Ordering::Relaxed) {
            unsafe {
                crate::debug::report(&format!(
                    "env_models: {count} map model(s) have no carrier and CClientEnvModel::Think made none: the game had no temp entity free; trying again each frame"
                ))
            };
        }
        return;
    }
    let created = CREATED.fetch_add(1, Ordering::Relaxed) + 1;
    if created <= LOG_LINES {
        unsafe {
            crate::debug::report(&format!(
                "env_models: {count} map model(s) had no carrier to draw them (a demo played as HLTV); CClientEnvModel::Think made one ({created} so far)"
            ))
        };
    }
}

/// One `ds_debug_status` line, once it has acted.
pub fn status_line() -> Option<String> {
    let created = CREATED.load(Ordering::Relaxed);
    (created > 0).then(|| {
        format!(
            "map models fix: the map's models drawn again in a demo played as HLTV, {created} time(s) this session"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: usize = 0x1900_0000;
    const SIZE: usize = 0x1b_4000;
    const SITE: usize = BASE + 0x2_c032;
    const ADD: usize = BASE + 0x2_98e0;

    /// The analysed build's call site, as loaded at `BASE`.
    fn call() -> Vec<u8> {
        let mut c = vec![0x8d, 0x44, 0x24, 0x0c, 0xb9];
        c.extend_from_slice(&((BASE + 0x10_cccc) as u32).to_le_bytes());
        c.extend_from_slice(&[0x50, 0xe8]);
        let rel = (ADD as i64 - (SITE + CALL_END) as i64) as i32;
        c.extend_from_slice(&rel.to_le_bytes());
        c
    }

    #[test]
    fn the_object_is_read_from_the_call_site() {
        assert_eq!(
            object_from_call(SITE, &call(), ADD, BASE, SIZE),
            Ok(BASE + 0x10_cccc)
        );
        // The bytes the analysis read: `e8 9f d8 ff ff`.
        assert_eq!(&call()[CALL_REL_AT..CALL_END], &[0x9f, 0xd8, 0xff, 0xff]);
    }

    #[test]
    fn a_call_elsewhere_or_an_object_outside_is_refused() {
        assert!(object_from_call(SITE, &call(), ADD + 0x10, BASE, SIZE).is_err());
        assert!(object_from_call(SITE, &call()[..CALL_END - 1], ADD, BASE, SIZE).is_err());
        // The count must fit inside the image too.
        assert!(object_from_call(SITE, &call(), ADD, BASE, 0x10_cccc + COUNT_OFFSET + 3).is_err());
        assert!(object_from_call(SITE, &call(), ADD, BASE, 0x10_cccc + COUNT_OFFSET + 4).is_ok());
        let mut below = call();
        below[OBJECT_AT..OBJECT_AT + 4].copy_from_slice(&0x1000u32.to_le_bytes());
        assert!(object_from_call(SITE, &below, ADD, BASE, SIZE).is_err());
    }

    /// Only a NULL carrier with models stored: a carrier already there (an
    /// HLTV demo's `CHud::Think` made it, or a pointer from an earlier level
    /// that `InitHUD` has yet to clear) and an empty or corrupt table are left
    /// alone.
    #[test]
    fn think_is_called_only_for_a_missing_carrier() {
        assert!(needs_carrier(20, 0));
        assert!(needs_carrier(1, 0));
        assert!(needs_carrier(MAX_MODELS, 0));
        assert!(!needs_carrier(20, 0x02fe_ead0));
        assert!(!needs_carrier(0, 0));
        assert!(!needs_carrier(-1, 0));
        assert!(!needs_carrier(MAX_MODELS + 1, 0));
    }

    #[test]
    fn the_patterns_parse() {
        for pattern in [
            ADD_CALL_PATTERN,
            ADD_PATTERN,
            THINK_PATTERN,
            THINK_STORE_PATTERN,
        ] {
            assert!(scan::Pattern::parse(pattern).is_ok(), "{pattern}");
        }
    }
}
