//! Frees the copy of each map's `.bsp` file that DoD's client leaks on every
//! map load (#1232).
//!
//! ## The leak
//!
//! When a map loads, DoD's `client.dll` reads the whole `.bsp` to pull out its
//! entity lump (`DoD_LoadEntityLump`, `client.dll+0x2c050`, called through
//! `+0x2c110` with `gEngfuncs.pfnGetLevelName()`):
//!
//! ```text
//! push 0; push 5; push ebx
//! call [gEngfuncs+0x13c]      ; COM_LoadFile("maps/<map>.bsp", 5, NULL)
//! mov  ebp, eax               ; the whole file
//! ...checks the version, mallocs the lump's length + 1, copies the lump...
//! ret                         ; ebp dropped: COM_FreeFile is never called
//! ```
//!
//! `usehunk` 5 makes the engine `malloc` the file (`hw.dll+0x2b92a` on the
//! pre-Anniversary build), so each map load keeps a whole map file on the
//! heap for the rest of the session: 3 MB for railroad2_s9a, 12 MB for
//! railroad2_test, 16.5 MB for thunder2. Every return path drops it, the
//! error paths too. The client's two other `COM_LoadFile` calls
//! (`scripts/voicemodel.txt` at `+0x26341`, `overviews/<map>.txt` at
//! `+0x38bc6`) free theirs through `pfnCOM_FreeFile` (`+0x2637f`, `+0x3900d`);
//! this is the only one that loads a `.bsp`.
//!
//! On the pre-Anniversary build `hl.exe` is not large-address-aware, so the
//! game has 2 GB of address space. Measured there (#1232) with plain HLAE and
//! no DoD Studio DLL, `viewdemo` of five demos on five big maps in turn: one
//! whole `.bsp` image per map load was still on the heap, found by its header
//! in the game's memory, and the process grew about 20 MB per load until,
//! with the hook DLL, load 33 died reading `maps/dod_railroad2_test.bsp` (the
//! largest free block was 8 MB). The engine reports it as `COM_LoadFile: not
//! enough space for maps/<map>.bsp` when the engine's own load is the one that
//! fails. The 25th Anniversary `hl.exe` is large-address-aware (4 GB), so it
//! takes about twice as many map loads to get there. `client.dll` is
//! byte-identical on both installs.
//!
//! ## The fix
//!
//! `pfnCOM_LoadFile` in client.dll's own `gEngfuncs` copy (slot 79) points at
//! [`hooked_load_file`]. A load of a `.bsp` file is remembered, and freed
//! through `pfnCOM_FreeFile` (slot 81) when the next one comes in: by then
//! `DoD_LoadEntityLump` has long returned, and nothing else holds the
//! pointer. Freeing at the next load rather than on a later frame keeps the
//! whole thing on the engine's thread, inside client.dll's own calls. At most
//! one map file stays on the heap, the current map's.
//!
//! It is on by default, like `tempent_fix`: it changes nothing the game draws
//! or plays, only gives back memory the game never uses again.
//! `GOLDSRC_HOOKS_MAP_FILE_LEAK_FIX=0` turns it off for a session.

// The hook only installs in the 32-bit game; host builds compile it for the tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::ffi::{CStr, c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};

/// `pfnCOM_LoadFile`: confirmed by client.dll's three calls through
/// `gEngfuncs+0x13c`, each `(path, 5, NULL)`.
const ENGFUNCS_SLOT_COM_LOAD_FILE: usize = 79;
/// `pfnCOM_FreeFile`: confirmed by client.dll's two calls through
/// `gEngfuncs+0x144`, each with the buffer the load returned.
const ENGFUNCS_SLOT_COM_FREE_FILE: usize = 81;

type ComLoadFileFn = unsafe extern "C" fn(*const c_char, i32, *mut i32) -> *mut u8;
type ComFreeFileFn = unsafe extern "C" fn(*mut c_void);

/// Off with `GOLDSRC_HOOKS_MAP_FILE_LEAK_FIX=0`.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

static REAL_LOAD: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static REAL_FREE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
/// The `gEngfuncs` copy the hook went into; 0 until then.
static INSTALLED_AT: AtomicUsize = AtomicUsize::new(0);
/// The last `.bsp` the client loaded, and its length; freed at the next.
static PENDING: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static PENDING_LEN: AtomicUsize = AtomicUsize::new(0);
static FREED: AtomicU32 = AtomicU32::new(0);
static FREED_BYTES: AtomicU64 = AtomicU64::new(0);
static LOGGED: AtomicU32 = AtomicU32::new(0);

/// True for the path DoD's entity-lump loader asks for (`maps/<map>.bsp`,
/// from `pfnGetLevelName`), and for no other file the client loads.
fn is_map_file(path: &[u8]) -> bool {
    path.len() > 4 && path[path.len() - 4..].eq_ignore_ascii_case(b".bsp")
}

/// Takes the remembered map file, if any, so the caller can free it.
fn take_pending() -> Option<(*mut u8, usize)> {
    let buffer = PENDING.swap(std::ptr::null_mut(), Ordering::AcqRel);
    (!buffer.is_null()).then(|| (buffer, PENDING_LEN.swap(0, Ordering::AcqRel)))
}

/// Remembers a map file the client will never free.
fn remember(buffer: *mut u8, length: usize) {
    PENDING_LEN.store(length, Ordering::Release);
    PENDING.store(buffer, Ordering::Release);
}

/// Counts a freed map file; true when it's worth a log line (the first, then
/// every tenfold, as `tempent_fix` logs).
fn count_freed(length: usize) -> bool {
    let now = FREED.fetch_add(1, Ordering::Relaxed) + 1;
    FREED_BYTES.fetch_add(length as u64, Ordering::Relaxed);
    let logged = LOGGED.load(Ordering::Relaxed);
    if crate::tempent_fix::worth_logging(logged, now) {
        LOGGED.store(now, Ordering::Relaxed);
        return true;
    }
    false
}

/// client.dll's `pfnCOM_LoadFile`. Called on the engine's thread, from
/// client.dll's own code.
unsafe extern "C" fn hooked_load_file(
    path: *const c_char,
    usehunk: i32,
    length: *mut i32,
) -> *mut u8 {
    let real = REAL_LOAD.load(Ordering::Acquire);
    if real.is_null() {
        return std::ptr::null_mut();
    }
    let real: ComLoadFileFn = unsafe { std::mem::transmute(real) };
    // Safety: the client passes a NUL-terminated path, or NULL.
    let map_file = !path.is_null() && is_map_file(unsafe { CStr::from_ptr(path) }.to_bytes());
    if !map_file || !ENABLED.load(Ordering::Relaxed) {
        return unsafe { real(path, usehunk, length) };
    }
    // The previous map's copy first, so the new one can reuse its space.
    if let Some((buffer, bytes)) = take_pending() {
        free_map_file(buffer, bytes);
    }
    let mut own_length = 0i32;
    let length_out = if length.is_null() {
        &mut own_length as *mut i32
    } else {
        length
    };
    let buffer = unsafe { real(path, usehunk, length_out) };
    if !buffer.is_null() {
        // Safety: the engine wrote the length through the pointer it was given.
        remember(buffer, usize::try_from(unsafe { *length_out }).unwrap_or(0));
    }
    buffer
}

fn free_map_file(buffer: *mut u8, bytes: usize) {
    let free = REAL_FREE.load(Ordering::Acquire);
    if free.is_null() {
        return;
    }
    let free: ComFreeFileFn = unsafe { std::mem::transmute(free) };
    // Safety: a buffer the engine's COM_LoadFile returned, freed once.
    unsafe { free(buffer as *mut c_void) };
    if count_freed(bytes) {
        unsafe {
            crate::debug::report(&format!(
                "map_file_leak: freed the previous map's .bsp copy client.dll leaks ({} so far, {:.1} MB) (#1232)",
                FREED.load(Ordering::Relaxed),
                FREED_BYTES.load(Ordering::Relaxed) as f64 / 1_048_576.0
            ))
        };
    }
}

/// Points client.dll's `pfnCOM_LoadFile` at the hook. Once per `gEngfuncs`
/// copy; a second call is free. Runs right after the client's `Initialize`,
/// before the first map loads.
#[cfg(target_arch = "x86")]
pub fn install() {
    if !ENABLED.load(Ordering::Relaxed) {
        unsafe { crate::debug::report("map_file_leak: off (GOLDSRC_HOOKS_MAP_FILE_LEAK_FIX=0)") };
        return;
    }
    let Some(gengfuncs) = crate::engine::client_gengfuncs() else {
        unsafe {
            crate::debug::report(
                "map_file_leak: client.dll's gEngfuncs copy not found; map files leak as before",
            )
        };
        return;
    };
    if INSTALLED_AT.load(Ordering::Acquire) == gengfuncs as usize {
        return;
    }
    // Safety: the live copy; slot 81 is read, not written.
    let free = unsafe { *gengfuncs.add(ENGFUNCS_SLOT_COM_FREE_FILE) };
    if free.is_null() {
        unsafe { crate::debug::report("map_file_leak: pfnCOM_FreeFile is null; not installed") };
        return;
    }
    REAL_FREE.store(free, Ordering::Release);
    let hook = hooked_load_file as *mut c_void;
    // Safety: the live copy; slot 79 has COM_LoadFile's signature.
    match unsafe {
        crate::engine::patch_gengfuncs_slot(gengfuncs, ENGFUNCS_SLOT_COM_LOAD_FILE, hook)
    } {
        Ok(was) => {
            REAL_LOAD.store(was, Ordering::Release);
            INSTALLED_AT.store(gengfuncs as usize, Ordering::Release);
            unsafe {
                crate::debug::report(&format!(
                    "map_file_leak: client.dll's pfnCOM_LoadFile (gEngfuncs at {gengfuncs:p}, slot 79) now frees the .bsp copy each map load leaks (#1232)"
                ))
            };
        }
        Err(why) => unsafe {
            crate::debug::report(&format!("map_file_leak: not installed -- {why}"))
        },
    }
}

#[cfg(not(target_arch = "x86"))]
pub fn install() {}

/// The `ds_debug_status` line, or `None` when it was never installed.
pub fn status_line() -> Option<String> {
    if INSTALLED_AT.load(Ordering::Relaxed) == 0 {
        return None;
    }
    Some(format!(
        "map file leak fix: {} leaked .bsp cop{} freed, {:.1} MB",
        FREED.load(Ordering::Relaxed),
        if FREED.load(Ordering::Relaxed) == 1 {
            "y"
        } else {
            "ies"
        },
        FREED_BYTES.load(Ordering::Relaxed) as f64 / 1_048_576.0
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // The tests share the module's statics.
    static STATICS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn only_bsp_paths_are_map_files() {
        assert!(is_map_file(b"maps/dod_railroad2_test.bsp"));
        assert!(is_map_file(b"maps/dod_saints2_B2.BSP"));
        assert!(!is_map_file(b"scripts/voicemodel.txt"));
        assert!(!is_map_file(b"overviews/dod_anzio.txt"));
        assert!(!is_map_file(b".bsp"));
        assert!(!is_map_file(b""));
    }

    #[test]
    fn a_remembered_file_is_taken_once() {
        let _guard = STATICS.lock().unwrap_or_else(|e| e.into_inner());
        let mut a = [0u8; 4];
        remember(a.as_mut_ptr(), 4);
        assert_eq!(take_pending(), Some((a.as_mut_ptr(), 4)));
        assert_eq!(take_pending(), None);
    }

    #[test]
    fn freed_files_are_logged_first_then_every_tenfold() {
        let _guard = STATICS.lock().unwrap_or_else(|e| e.into_inner());
        FREED.store(0, Ordering::Relaxed);
        FREED_BYTES.store(0, Ordering::Relaxed);
        LOGGED.store(0, Ordering::Relaxed);
        let logged: Vec<u32> = (1..=25)
            .filter(|_| count_freed(1 << 20))
            .map(|_| FREED.load(Ordering::Relaxed))
            .collect();
        assert_eq!(logged, vec![1, 10]);
        assert_eq!(FREED_BYTES.load(Ordering::Relaxed), 25 << 20);
    }
}
