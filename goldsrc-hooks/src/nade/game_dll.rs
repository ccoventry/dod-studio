//! The game's own server, read from the side: `dod.dll`'s copy of the engine
//! function table and its `gpGlobals`, found by reading `GiveFnptrsToDll`.
//!
//! On a local server (`map <name>`), `dod.dll` runs inside the same `hl.exe`
//! as this DLL. The engine hands it two things at startup through the export
//! `GiveFnptrsToDll(enginefuncs_t *, globalvars_t *)` (stdcall, `ret 8`):
//! it copies the table into a static and keeps the globals pointer in another.
//! DoD 1.3's export (`dod.dll+0x58d80`, byte-identical in every install) is:
//!
//! ```text
//! push 0x27c              ; 159 slots -- the table's size in this engine
//! mov  eax, [ebp+8]
//! push eax
//! push 0x1013b360         ; &g_engfuncs
//! call memcpy
//! ...
//! mov  eax, [ebp+0xc]
//! mov  [0x1013b5dc], eax  ; gpGlobals
//! ```
//!
//! So rather than wrapping the export (the engine loads `dod.dll` at startup,
//! possibly before our hooks are in), this reads both addresses out of those
//! two instructions in the loaded image, relocations already applied. Anything
//! that doesn't match the pattern is refused, and the practice tools stay off.
//!
//! Slot numbers and `entvars_t` offsets are the Half-Life SDK's
//! (`engine/eiface.h`, `engine/progdefs.h`); `dod.dll` itself writes the
//! grenade's `movetype` at `+0x108`, `gravity` at `+0x11c` (0.5) and
//! `friction` at `+0x120` (0.8), which matches.
//!
//! Main thread only: every call here is an engine call.

#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::ffi::{CStr, c_char};
use std::sync::atomic::{AtomicUsize, Ordering};

use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

/// `enginefuncs_t` slots (`engine/eiface.h`).
const SLOT_SET_ORIGIN: usize = 28;
const SLOT_TRACE_HULL: usize = 34;
const SLOT_CVAR_GET_FLOAT: usize = 57;
const SLOT_SZ_FROM_INDEX: usize = 66;
const SLOT_PENTITY_OF_ENT_INDEX: usize = 72;
/// What `GiveFnptrsToDll` copies: 0x27c bytes.
const TABLE_SLOTS: usize = 159;

/// `edict_t`'s header before its `entvars_t` (`engine/edict.h`): `free`,
/// `serialnumber`, `area`, `headnode`, `num_leafs`, `leafnums[48]`,
/// `freetime`, `pvPrivateData`.
const EDICT_PRIVATE_DATA: usize = 0x7c;
const EDICT_VARS: usize = 0x80;

/// `entvars_t` offsets (`engine/progdefs.h`).
pub const EV_CLASSNAME: usize = 0x00;
pub const EV_ORIGIN: usize = 0x08;
pub const EV_VELOCITY: usize = 0x20;
pub const EV_ANGLES: usize = 0x50;
pub const EV_V_ANGLE: usize = 0x74;
pub const EV_FIXANGLE: usize = 0xa0;
pub const EV_MODEL: usize = 0xb8;
pub const EV_MOVETYPE: usize = 0x108;
pub const EV_EFFECTS: usize = 0x118;
pub const EV_GRAVITY: usize = 0x11c;
pub const EV_DEADFLAG: usize = 0x170;
pub const EV_FLAGS: usize = 0x1a4;

/// `globalvars_t` offsets.
const GV_TIME: usize = 0x00;
const GV_MAX_CLIENTS: usize = 0x90;
const GV_MAX_ENTITIES: usize = 0x94;

/// `common/const.h`.
pub const FL_CLIENT: i32 = 1 << 3;
pub const FL_ONGROUND: i32 = 1 << 9;
pub const FL_DUCKING: i32 = 1 << 14;
pub const FL_FAKECLIENT: i32 = 1 << 13;
pub const EF_NODRAW: i32 = 128;
pub const MOVETYPE_BOUNCE: i32 = 10;

/// The two addresses read out of `GiveFnptrsToDll`, 0 until found.
static TABLE: AtomicUsize = AtomicUsize::new(0);
static GLOBALS_SLOT: AtomicUsize = AtomicUsize::new(0);

type PEntityOfEntIndexFn = unsafe extern "C" fn(i32) -> *mut u8;
type SzFromIndexFn = unsafe extern "C" fn(i32) -> *const c_char;
type SetOriginFn = unsafe extern "C" fn(*mut u8, *const f32);
type CVarGetFloatFn = unsafe extern "C" fn(*const c_char) -> f32;
type TraceHullFn =
    unsafe extern "C" fn(*const f32, *const f32, i32, i32, *mut u8, *mut TraceResult);

/// `TraceResult` (`engine/eiface.h`).
#[repr(C)]
#[derive(Default)]
pub struct TraceResult {
    pub all_solid: i32,
    pub start_solid: i32,
    pub in_open: i32,
    pub in_water: i32,
    pub fraction: f32,
    pub end_pos: [f32; 3],
    pub plane_dist: f32,
    pub plane_normal: [f32; 3],
    pub hit: u32,
    pub hitgroup: i32,
}

/// Reads `push imm32` after `push 0x27c` and `mov [imm32], eax`, in that
/// order, out of the first bytes of `GiveFnptrsToDll`. Pure, so the pattern
/// is tested against the real export's bytes.
pub fn parse_give_fnptrs(code: &[u8]) -> Option<(u32, u32)> {
    let size = code
        .windows(5)
        .position(|w| w == [0x68, 0x7c, 0x02, 0x00, 0x00])?;
    let mut table = None;
    let mut i = size + 5;
    while i + 5 <= code.len() {
        match code[i] {
            0x68 if table.is_none() => {
                table = Some(u32::from_le_bytes(code[i + 1..i + 5].try_into().ok()?));
                i += 5;
            }
            0xa3 if table.is_some() => {
                let globals = u32::from_le_bytes(code[i + 1..i + 5].try_into().ok()?);
                return Some((table?, globals));
            }
            _ => i += 1,
        }
    }
    None
}

/// Finds the table and globals once `dod.dll` is loaded. Cheap to call every
/// frame: it does the work once.
fn locate() -> Option<(usize, usize)> {
    let table = TABLE.load(Ordering::Relaxed);
    if table != 0 {
        return Some((table, GLOBALS_SLOT.load(Ordering::Relaxed)));
    }
    let module = unsafe { GetModuleHandleA(c"dod.dll".as_ptr() as *const u8) };
    if module.is_null() {
        return None;
    }
    let export = unsafe { GetProcAddress(module, c"GiveFnptrsToDll".as_ptr() as *const u8) }?;
    // Safety: the export's own code, mapped for the life of the module.
    let code = unsafe { std::slice::from_raw_parts(export as usize as *const u8, 64) };
    let Some((table, globals)) = parse_give_fnptrs(code) else {
        static SAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !SAID.swap(true, Ordering::Relaxed) {
            unsafe {
                crate::debug::report(
                    "nade: dod.dll's GiveFnptrsToDll isn't the expected code -- grenade practice is off",
                )
            };
        }
        return None;
    };
    TABLE.store(table as usize, Ordering::Relaxed);
    GLOBALS_SLOT.store(globals as usize, Ordering::Relaxed);
    unsafe {
        crate::debug::report(&format!(
            "nade: dod.dll's engine table at {table:#x}, gpGlobals at {globals:#x}"
        ))
    };
    Some((table as usize, globals as usize))
}

fn slot(index: usize) -> Option<usize> {
    debug_assert!(index < TABLE_SLOTS);
    let (table, _) = locate()?;
    // Safety: inside the 159-slot copy GiveFnptrsToDll made.
    let f = unsafe { *((table + index * 4) as *const u32) } as usize;
    (f != 0).then_some(f)
}

fn globals() -> Option<usize> {
    let (_, slot) = locate()?;
    let g = unsafe { *(slot as *const u32) } as usize;
    (g != 0).then_some(g)
}

pub fn read_f32(base: usize, offset: usize) -> f32 {
    unsafe { *((base + offset) as *const f32) }
}

pub fn read_i32(base: usize, offset: usize) -> i32 {
    unsafe { *((base + offset) as *const i32) }
}

pub fn read_vec(base: usize, offset: usize) -> [f32; 3] {
    [
        read_f32(base, offset),
        read_f32(base, offset + 4),
        read_f32(base, offset + 8),
    ]
}

pub fn write_vec(base: usize, offset: usize, v: [f32; 3]) {
    for (k, x) in v.iter().enumerate() {
        unsafe { *((base + offset + k * 4) as *mut f32) = *x };
    }
}

pub fn write_i32(base: usize, offset: usize, v: i32) {
    unsafe { *((base + offset) as *mut i32) = v };
}

/// The server's clock, `gpGlobals->time`.
pub fn time() -> Option<f32> {
    Some(read_f32(globals()?, GV_TIME))
}

pub fn max_clients() -> i32 {
    globals().map_or(0, |g| read_i32(g, GV_MAX_CLIENTS))
}

pub fn max_entities() -> i32 {
    globals().map_or(0, |g| read_i32(g, GV_MAX_ENTITIES))
}

/// An entity that is in use: its edict address, or `None` for a free slot.
pub fn edict(index: i32) -> Option<usize> {
    let f: PEntityOfEntIndexFn = unsafe { std::mem::transmute(slot(SLOT_PENTITY_OF_ENT_INDEX)?) };
    let e = unsafe { f(index) } as usize;
    (e != 0).then_some(e)
}

/// The entity's `entvars_t`.
pub fn vars(edict: usize) -> usize {
    edict + EDICT_VARS
}

/// The game's object behind an edict, which changes when the slot is reused.
pub fn private_data(edict: usize) -> usize {
    unsafe { *((edict + EDICT_PRIVATE_DATA) as *const u32) as usize }
}

/// A `string_t` field as text.
pub fn string(vars: usize, offset: usize) -> Option<String> {
    let index = read_i32(vars, offset);
    if index == 0 {
        return None;
    }
    let f: SzFromIndexFn = unsafe { std::mem::transmute(slot(SLOT_SZ_FROM_INDEX)?) };
    let p = unsafe { f(index) };
    (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
}

pub fn cvar(name: &CStr) -> Option<f32> {
    let f: CVarGetFloatFn = unsafe { std::mem::transmute(slot(SLOT_CVAR_GET_FLOAT)?) };
    Some(unsafe { f(name.as_ptr()) })
}

pub fn set_origin(edict: usize, origin: [f32; 3]) -> bool {
    let Some(f) = slot(SLOT_SET_ORIGIN) else {
        return false;
    };
    let f: SetOriginFn = unsafe { std::mem::transmute(f) };
    unsafe { f(edict as *mut u8, origin.as_ptr()) };
    true
}

/// Whether a standing player (`hull` 1) or a crouched one (3) fits at `at`.
pub fn hull_fits(at: [f32; 3], hull: i32, ignore: usize) -> Option<bool> {
    let f: TraceHullFn = unsafe { std::mem::transmute(slot(SLOT_TRACE_HULL)?) };
    let mut tr = TraceResult::default();
    // fNoMonsters 0: other players count.
    unsafe {
        f(
            at.as_ptr(),
            at.as_ptr(),
            0,
            hull,
            ignore as *mut u8,
            &mut tr,
        )
    };
    Some(tr.all_solid == 0 && tr.start_solid == 0)
}

/// The local player on a local server where nobody else has joined: edict 1,
/// a real client, and no other human in the client slots. Bots don't count.
/// `None` otherwise, which is what keeps every practice tool off anywhere but
/// your own offline game.
pub fn local_player_alone() -> Option<usize> {
    let max = max_clients();
    if max < 1 {
        return None;
    }
    let mut me = None;
    for index in 1..=max {
        let Some(e) = edict(index) else { continue };
        let v = vars(e);
        let flags = read_i32(v, EV_FLAGS);
        if flags & FL_CLIENT == 0 || flags & FL_FAKECLIENT != 0 {
            continue;
        }
        if index != 1 {
            return None;
        }
        me = Some(e);
    }
    me
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_both_addresses_out_of_dods_give_fnptrs() {
        // dod.dll+0x58d80, as shipped.
        let code = [
            0x55, 0x8b, 0xec, 0x83, 0xec, 0x40, 0x53, 0x56, 0x57, 0x68, 0x7c, 0x02, 0x00, 0x00,
            0x8b, 0x45, 0x08, 0x50, 0x68, 0x60, 0xb3, 0x13, 0x10, 0xe8, 0x54, 0x38, 0x06, 0x00,
            0x83, 0xc4, 0x0c, 0x8b, 0x45, 0x0c, 0xa3, 0xdc, 0xb5, 0x13, 0x10, 0x5f, 0x5e, 0x5b,
            0x8b, 0xe5, 0x5d, 0xc2, 0x08, 0x00,
        ];
        assert_eq!(parse_give_fnptrs(&code), Some((0x1013b360, 0x1013b5dc)));
    }

    #[test]
    fn refuses_code_without_the_table_copy() {
        assert_eq!(
            parse_give_fnptrs(&[0x55, 0x8b, 0xec, 0xa3, 1, 2, 3, 4, 0xc3]),
            None
        );
    }
}
