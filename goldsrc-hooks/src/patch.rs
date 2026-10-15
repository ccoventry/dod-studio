//! Writing to a loaded module's code, and putting the page back.
//!
//! Lifted out of `deathmsg` when `detour` needed the same three steps. Issue
//! #204 had recorded `client.dll` behaving as though it were hardened against
//! exactly this; a live session settled that it is not.

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

use windows_sys::Win32::System::Memory::{
    PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS, VirtualProtect,
};

/// Repoints one vftable slot to `ours`, saving what it held into `original`
/// **before** the slot changes. The other order leaves a window where the
/// slot already points at our thunk but `original` still reads 0, and a
/// call landing in that window (any thread) would jump to address 0.
///
/// Safety: `vftable_slot` must be the address of a slot in a loaded module's
/// vftable, and `ours` a function with that slot's signature.
// Its callers are the 32-bit hook modules; a host build has none.
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
pub unsafe fn swap_vftable_slot(
    vftable_slot: usize,
    ours: usize,
    original: &AtomicUsize,
) -> Result<(), String> {
    let old = unsafe { *(vftable_slot as *const usize) };
    original.store(old, Ordering::Release);
    if unsafe { write_code_bytes(vftable_slot, &(ours as u32).to_le_bytes()) } {
        Ok(())
    } else {
        Err(format!(
            "could not make the vftable at {vftable_slot:#x} writable"
        ))
    }
}

/// Overwrites `bytes.len()` bytes at `address`, restoring the original page
/// protection afterwards. `false` means nothing was written.
///
/// Safety: `address` must be a valid, mapped span of at least `bytes.len()`
/// bytes, and the caller must know that overwriting it is meaningful -- this
/// does no checking of its own.
pub unsafe fn write_code_bytes(address: usize, bytes: &[u8]) -> bool {
    let target = address as *mut u8;
    let mut old: PAGE_PROTECTION_FLAGS = 0;
    let ok = unsafe {
        VirtualProtect(
            target as *mut c_void,
            bytes.len(),
            PAGE_EXECUTE_READWRITE,
            &mut old,
        )
    };
    if ok == 0 {
        return false;
    }
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), target, bytes.len()) };
    // Restoring matters more than it looks: leaving the page writable would
    // quietly remove a protection the game shipped with.
    let mut discard: PAGE_PROTECTION_FLAGS = 0;
    unsafe { VirtualProtect(target as *mut c_void, bytes.len(), old, &mut discard) };
    true
}
