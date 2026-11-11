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

/// Points `entry` at `wrapper`, keeping its handler in `real`, once per
/// `real`: a slot that already holds a handler means `wrapper` is in the
/// node's chain, and it stays there, so callers can re-run this every poll.
///
/// Several modules wrap the same command (`connect_guard` and the review
/// lock both wrap `connect`, `retry` and `reconnect`; `demo_reload` and the
/// review lock both wrap `playdemo` and `viewdemo`). Re-wrapping whenever
/// the node's handler isn't our own would store the other module's
/// wrapper as our real handler while it already holds ours, a loop that
/// overflows the stack the first time the command runs: a demo's level
/// change crashed the game in its `reconnect`. So whoever wraps second sits
/// on top and calls down to the first, which calls the engine.
///
/// The one case this misses is a node the engine frees and registers again
/// with a new handler: the commands wrapped here live in `hw.dll`, HLAE and
/// this DLL, which never reload, and the engine refuses a second
/// registration of a name it has (`Cmd_AddCommand`: "already defined").
pub fn wrap(entry: &mut CmdFunction, real: &AtomicUsize, wrapper: ConsoleCommandFn) -> bool {
    if real.load(Ordering::Relaxed) != 0 {
        return true;
    }
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
    use std::sync::Mutex;
    use std::sync::atomic::AtomicU32;

    /// The fake handlers below share these statics.
    static FAKES: Mutex<()> = Mutex::new(());
    static REAL_A: AtomicUsize = AtomicUsize::new(0);
    static REAL_B: AtomicUsize = AtomicUsize::new(0);
    static RAN_ENGINE: AtomicU32 = AtomicU32::new(0);
    static RAN_A: AtomicU32 = AtomicU32::new(0);
    static RAN_B: AtomicU32 = AtomicU32::new(0);
    /// How deep the wrappers are nested; a loop stops at a few instead of
    /// overflowing the test's stack.
    static DEPTH: AtomicU32 = AtomicU32::new(0);
    const LOOPED: u32 = 8;

    unsafe extern "C" fn engine_handler() {
        RAN_ENGINE.fetch_add(1, Ordering::Relaxed);
    }

    unsafe fn nested(ran: &AtomicU32, real: &AtomicUsize) {
        ran.fetch_add(1, Ordering::Relaxed);
        if DEPTH.fetch_add(1, Ordering::Relaxed) < LOOPED {
            unsafe { call_real(real) };
        }
        DEPTH.fetch_sub(1, Ordering::Relaxed);
    }

    unsafe extern "C" fn wrapper_a() {
        unsafe { nested(&RAN_A, &REAL_A) };
    }

    unsafe extern "C" fn wrapper_b() {
        unsafe { nested(&RAN_B, &REAL_B) };
    }

    fn fake_node() -> CmdFunction {
        for counter in [&REAL_A, &REAL_B] {
            counter.store(0, Ordering::Relaxed);
        }
        for counter in [&RAN_ENGINE, &RAN_A, &RAN_B, &DEPTH] {
            counter.store(0, Ordering::Relaxed);
        }
        CmdFunction {
            next: std::ptr::null_mut(),
            name: c"reconnect".as_ptr(),
            function: Some(engine_handler),
            flags: 0,
        }
    }

    fn run(node: &CmdFunction) {
        unsafe { (node.function.unwrap())() };
    }

    fn wrap_a(node: &mut CmdFunction) -> bool {
        wrap(node, &REAL_A, wrapper_a)
    }

    fn wrap_b(node: &mut CmdFunction) -> bool {
        wrap(node, &REAL_B, wrapper_b)
    }

    #[test]
    fn two_wrappers_re_polled_in_either_order_never_loop_and_both_run() {
        let _fakes = FAKES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        type Wrap = fn(&mut CmdFunction) -> bool;
        for (first, second) in [(wrap_a as Wrap, wrap_b as Wrap), (wrap_b, wrap_a)] {
            let mut node = fake_node();
            assert!(first(&mut node));
            assert!(second(&mut node));
            let on_top = node.function.map(|f| f as usize);
            // Both modules re-run their wrap every poll, in both orders.
            for i in 0..100 {
                if i % 2 == 0 {
                    assert!(first(&mut node));
                    assert!(second(&mut node));
                } else {
                    assert!(second(&mut node));
                    assert!(first(&mut node));
                }
            }
            assert_eq!(node.function.map(|f| f as usize), on_top);
            run(&node);
            assert_eq!(
                RAN_A.load(Ordering::Relaxed),
                1,
                "a wrapper ran twice: a loop"
            );
            assert_eq!(
                RAN_B.load(Ordering::Relaxed),
                1,
                "a wrapper ran twice: a loop"
            );
            assert_eq!(RAN_ENGINE.load(Ordering::Relaxed), 1);
        }
    }

    #[test]
    fn the_second_wrapper_sits_on_top_and_calls_down_to_the_first() {
        let _fakes = FAKES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut node = fake_node();
        wrap_a(&mut node);
        wrap_b(&mut node);
        let b: ConsoleCommandFn = wrapper_b;
        let a: ConsoleCommandFn = wrapper_a;
        let engine: ConsoleCommandFn = engine_handler;
        assert_eq!(node.function.map(|f| f as usize), Some(b as usize));
        assert_eq!(REAL_B.load(Ordering::Relaxed), a as usize);
        assert_eq!(REAL_A.load(Ordering::Relaxed), engine as usize);
    }

    #[test]
    fn a_wrapper_on_its_own_is_put_in_once() {
        let _fakes = FAKES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut node = fake_node();
        for _ in 0..10 {
            assert!(wrap_a(&mut node));
        }
        run(&node);
        assert_eq!(RAN_A.load(Ordering::Relaxed), 1);
        assert_eq!(RAN_ENGINE.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn a_node_with_no_handler_is_not_wrapped() {
        let _fakes = FAKES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut node = fake_node();
        node.function = None;
        assert!(!wrap_a(&mut node));
        assert_eq!(REAL_A.load(Ordering::Relaxed), 0);
        assert!(node.function.is_none());
    }

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
