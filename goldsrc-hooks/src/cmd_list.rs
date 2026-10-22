//! Wrapping an engine console command without a per-build offset.
//!
//! `cl_enginefunc_t` slots 102-104 walk the engine's command list
//! (`pfnGetFirstCmdFunctionHandle`, `pfnGetNextCmdFunctionHandle`,
//! `pfnGetCmdFunctionName`). Each node is `Cmd_AddCommand`'s 16-byte
//! allocation, `{ next, name, function, flags }`, the same in both builds
//! (pre-Anniversary `hw.dll+0x28090`, 25th Anniversary `+0x1b50c0`), so a
//! handler is swapped by writing the node's `function` field. Nodes live on
//! the engine's heap, so no page protection needs changing.
//! `tools/verify_cmd_list_slots.py` checks the slots and the layout against
//! both `hw.dll`s.
//!
//! Every command runs through its node, whether typed, bound, in a `.cfg`, or
//! read from a demo's own `ConsoleCommand` frames, so a wrapper sees them all.
//! First used by `demo_reload` (#330); `events` wraps `echo` the same way.

// Only the 32-bit build wraps anything; a host check still compiles it.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::ffi::{CStr, c_char};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::engine::{self, ClEngineFuncsPartial, ConsoleCommandFn};

/// `cl_enginefunc_t` slot, from the public SDK's `APIProxy.h`.
const SLOT_GET_FIRST_CMD_FUNCTION_HANDLE: usize = 102;

/// The list is a few hundred nodes; this only stops a corrupted one looping.
const MAX_NODES: usize = 8192;

/// A node of the engine's command list, as `Cmd_AddCommand` builds it.
#[repr(C)]
pub struct CmdFunction {
    pub next: *mut CmdFunction,
    pub name: *const c_char,
    pub function: Option<ConsoleCommandFn>,
    pub flags: i32,
}

type GetFirstCmdFn = unsafe extern "C" fn() -> *mut CmdFunction;

/// Calls `visit` with each command's lowercase-comparable name and its node.
/// False when the engine's function table or list is not available yet.
pub fn for_each(mut visit: impl FnMut(&[u8], &mut CmdFunction)) -> bool {
    let Some(engfuncs) = engine::engfuncs() else {
        return false;
    };
    // Safety: slot 102 of the engine's own table, checked in both builds by
    // tools/verify_cmd_list_slots.py.
    let first = unsafe {
        let slot = *(engfuncs as *const ClEngineFuncsPartial as *const usize)
            .add(SLOT_GET_FIRST_CMD_FUNCTION_HANDLE);
        if slot == 0 {
            return false;
        }
        let get_first: GetFirstCmdFn = std::mem::transmute(slot);
        get_first()
    };
    let mut node = first;
    let mut seen = 0;
    while !node.is_null() && seen < MAX_NODES {
        seen += 1;
        // Safety: a live node of the engine's list; see the module doc.
        let entry = unsafe { &mut *node };
        if !entry.name.is_null() {
            let name = unsafe { CStr::from_ptr(entry.name) }.to_bytes();
            visit(name, entry);
        }
        node = entry.next;
    }
    true
}

/// Points `entry` at `wrapper`, keeping its handler in `real`. Idempotent.
pub fn wrap(entry: &mut CmdFunction, real: &AtomicUsize, wrapper: ConsoleCommandFn) -> bool {
    let Some(current) = entry.function else {
        return false;
    };
    if current as usize == wrapper as usize {
        return true;
    }
    real.store(current as usize, Ordering::Relaxed);
    entry.function = Some(wrapper);
    true
}

/// Calls the handler `wrap` kept, if there is one.
///
/// # Safety
///
/// `real` must hold 0 or an engine command handler `wrap` stored.
pub unsafe fn call_real(real: &AtomicUsize) {
    let address = real.load(Ordering::Relaxed);
    if address != 0 {
        let real: ConsoleCommandFn = unsafe { std::mem::transmute(address) };
        unsafe { real() };
    }
}

/// The current command's arguments after its name, as the engine tokenised
/// them. Main thread only, inside a command handler.
pub fn args() -> Vec<String> {
    tokens_from(1)
}

/// The whole current command line as the engine tokenised it, name first
/// (`argv[0]`), for handlers that echo the full line into their log. Main
/// thread only, inside a command handler. Empty while engfuncs isn't resolved
/// yet, so callers must not assume `argv[0]` exists.
pub fn argv() -> Vec<String> {
    tokens_from(0)
}

fn tokens_from(first: i32) -> Vec<String> {
    let Some(engfuncs) = engine::engfuncs() else {
        return Vec::new();
    };
    let argc = unsafe { (engfuncs.cmd_argc)() };
    (first..argc)
        .filter_map(|i| {
            let ptr = unsafe { (engfuncs.cmd_argv)(i) };
            (!ptr.is_null()).then(|| {
                unsafe { CStr::from_ptr(ptr) }
                    .to_string_lossy()
                    .into_owned()
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_node_is_laid_out_as_cmd_add_command_builds_it() {
        assert_eq!(
            std::mem::size_of::<CmdFunction>(),
            4 * std::mem::size_of::<usize>()
        );
        assert_eq!(
            std::mem::offset_of!(CmdFunction, name),
            std::mem::size_of::<usize>()
        );
        assert_eq!(
            std::mem::offset_of!(CmdFunction, function),
            2 * std::mem::size_of::<usize>()
        );
    }
}
