//! `dodstudio_reload_demo`: play the current demo again from the start (#330).
//!
//! ## Nothing in the engine keeps the name
//!
//! `playdemo`'s handler (pre-Anniversary `hw.dll+0x105fd`) copies `Cmd_Argv(1)`
//! into a 256-byte buffer on its own stack, adds `.dem`, prints "Playing demo
//! from %s." and opens the file. The only thing it keeps is the open file
//! handle and the demo's header; the name is gone when the function returns.
//! So the DLL remembers it: it wraps the engine's own `playdemo` and
//! `viewdemo` commands, notes the name each time one runs, and hands straight
//! on to the real handler. `dodstudio_reload_demo` then runs the same command
//! with the same name.
//!
//! ## Wrapping an engine command without an offset
//!
//! `cl_enginefunc_t` slots 102-104 walk the engine's command list
//! (`pfnGetFirstCmdFunctionHandle`, `pfnGetNextCmdFunctionHandle`,
//! `pfnGetCmdFunctionName`). Each node is `Cmd_AddCommand`'s 16-byte
//! allocation, `{ next, name, function, flags }`, the same in both builds
//! (pre-Anniversary `hw.dll+0x28090`, 25th Anniversary `+0x1b50c0`), so the
//! handler is swapped by writing the node's `function` field. Nodes live on
//! the engine's heap, so no page protection needs changing, and no per-build
//! address is involved. `tools/verify_cmd_list_slots.py` checks the slots and
//! the layout against both `hw.dll`s.
//!
//! The first attempt is at [`crate::commands::install`], right after
//! `client.dll`'s `Initialize`. In case the engine has not registered
//! `playdemo` by then, [`poll`] retries every frame for a few seconds.

// Only the 32-bit build installs anything; a host check still compiles it.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::engine::{self, ClEngineFuncsPartial, ConsoleCommandFn};
use crate::names::console_name;

pub const NAME: &str = console_name!("reload_demo");

/// `cl_enginefunc_t` slots, from the public SDK's `APIProxy.h`.
const SLOT_CLIENT_CMD: usize = 20;
const SLOT_GET_FIRST_CMD_FUNCTION_HANDLE: usize = 102;

/// A node of the engine's command list, as `Cmd_AddCommand` builds it.
#[repr(C)]
struct CmdFunction {
    next: *mut CmdFunction,
    name: *const c_char,
    function: Option<ConsoleCommandFn>,
    flags: i32,
}

type GetFirstCmdFn = unsafe extern "C" fn() -> *mut CmdFunction;
type ClientCmdFn = unsafe extern "C" fn(*const c_char);

/// The engine's own handlers, called by the wrappers.
static REAL_PLAYDEMO: AtomicUsize = AtomicUsize::new(0);
static REAL_VIEWDEMO: AtomicUsize = AtomicUsize::new(0);
static WRAPPED: AtomicBool = AtomicBool::new(false);

/// How many more frames `poll` retries a wrap that found nothing.
static RETRIES_LEFT: AtomicU32 = AtomicU32::new(300);

/// The list is a few hundred nodes; this only stops a corrupted one looping.
const MAX_NODES: usize = 8192;

thread_local! {
    /// The last `playdemo`/`viewdemo` and the name it was given. Console
    /// commands run on the engine's main thread, like everything else here.
    static LAST: RefCell<Option<(&'static str, String)>> = const { RefCell::new(None) };
}

/// Wraps `playdemo` and `viewdemo`. Called once engfuncs are captured.
pub fn install() {
    if try_wrap() {
        RETRIES_LEFT.store(0, Ordering::Relaxed);
    }
}

/// Retries a wrap that has not found `playdemo` yet, for a few seconds of
/// frames, then says so once. A no-op after that, or once wrapped.
pub fn poll() {
    if RETRIES_LEFT.load(Ordering::Relaxed) == 0 {
        return;
    }
    if try_wrap() {
        RETRIES_LEFT.store(0, Ordering::Relaxed);
    } else if RETRIES_LEFT.fetch_sub(1, Ordering::Relaxed) == 1 {
        unsafe {
            crate::debug::report(&format!(
                "{NAME}: no playdemo command in the engine's command list -- {NAME} will not work"
            ))
        };
    }
}

fn try_wrap() -> bool {
    if WRAPPED.load(Ordering::Relaxed) {
        return true;
    }
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

    let mut found_playdemo = false;
    let mut node = first;
    let mut seen = 0;
    while !node.is_null() && seen < MAX_NODES {
        seen += 1;
        // Safety: a live node of the engine's list; see the module doc.
        let entry = unsafe { &mut *node };
        if !entry.name.is_null() {
            let name = unsafe { CStr::from_ptr(entry.name) }.to_bytes();
            if name.eq_ignore_ascii_case(b"playdemo") {
                found_playdemo |= wrap(entry, &REAL_PLAYDEMO, wrapped_playdemo);
            } else if name.eq_ignore_ascii_case(b"viewdemo") {
                wrap(entry, &REAL_VIEWDEMO, wrapped_viewdemo);
            }
        }
        node = entry.next;
    }
    if found_playdemo {
        WRAPPED.store(true, Ordering::Relaxed);
        unsafe {
            crate::debug::report(&format!(
                "{NAME}: wrapped playdemo{} to remember the demo name",
                if REAL_VIEWDEMO.load(Ordering::Relaxed) != 0 {
                    " and viewdemo"
                } else {
                    ""
                }
            ))
        };
    }
    found_playdemo
}

/// Points `entry` at `wrapper`, keeping its handler in `real`.
fn wrap(entry: &mut CmdFunction, real: &AtomicUsize, wrapper: ConsoleCommandFn) -> bool {
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

unsafe extern "C" fn wrapped_playdemo() {
    remember("playdemo");
    unsafe { call_real(&REAL_PLAYDEMO) };
}

unsafe extern "C" fn wrapped_viewdemo() {
    remember("viewdemo");
    unsafe { call_real(&REAL_VIEWDEMO) };
}

unsafe fn call_real(real: &AtomicUsize) {
    let address = real.load(Ordering::Relaxed);
    if address != 0 {
        let real: ConsoleCommandFn = unsafe { std::mem::transmute(address) };
        unsafe { real() };
    }
}

/// Notes the name the command was given. A bare `playdemo` (it prints its
/// usage) changes nothing.
fn remember(command: &'static str) {
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    let name = unsafe {
        if (engfuncs.cmd_argc)() < 2 {
            return;
        }
        let arg = (engfuncs.cmd_argv)(1);
        if arg.is_null() {
            return;
        }
        CStr::from_ptr(arg).to_string_lossy().trim().to_string()
    };
    if replay_line(command, &name).is_some() {
        LAST.with(|last| *last.borrow_mut() = Some((command, name)));
    }
}

/// The console line that plays `name` again with `command`, or `None` for a
/// name that cannot be replayed safely: empty, or carrying a quote or a line
/// break that would end the argument early or start a second command.
fn replay_line(command: &str, name: &str) -> Option<String> {
    if name.is_empty() || name.contains(['"', '\n', '\r', ';']) {
        return None;
    }
    Some(format!("{command} \"{name}\"\n"))
}

/// `dodstudio_reload_demo`: runs the last `playdemo`/`viewdemo` again.
pub unsafe extern "C" fn command() {
    let last = LAST.with(|last| last.borrow().clone());
    let Some((command, name)) = last else {
        let why = if WRAPPED.load(Ordering::Relaxed) {
            "no demo has been played this session yet -- start one with playdemo or viewdemo first"
        } else {
            "the engine's playdemo command was not found, so the demo name cannot be tracked"
        };
        crate::commands::console_print(&format!("{NAME}: {why}\n"));
        return;
    };
    let Some(line) = replay_line(command, &name) else {
        return;
    };
    crate::commands::console_print(&format!("{NAME}: {}\n", line.trim_end()));
    if !client_cmd(&line) {
        crate::commands::console_print(&format!(
            "{NAME}: the engine's function table is not available\n"
        ));
    }
}

/// Runs `line` through the engine's `pfnClientCmd`.
fn client_cmd(line: &str) -> bool {
    let Some(engfuncs) = engine::engfuncs() else {
        return false;
    };
    let Ok(line) = CString::new(line) else {
        return false;
    };
    // Safety: slot 20 of the engine's own table, the one engine.rs already
    // documents and watches.
    unsafe {
        let slot = *(engfuncs as *const ClEngineFuncsPartial as *const usize).add(SLOT_CLIENT_CMD);
        if slot == 0 {
            return false;
        }
        let client_cmd: ClientCmdFn = std::mem::transmute(slot as *const c_void);
        client_cmd(line.as_ptr());
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_replay_line_quotes_the_name() {
        assert_eq!(
            replay_line("playdemo", "test/my demo"),
            Some("playdemo \"test/my demo\"\n".to_string())
        );
        assert_eq!(
            replay_line("viewdemo", "../other/match.dem"),
            Some("viewdemo \"../other/match.dem\"\n".to_string())
        );
    }

    #[test]
    fn names_that_would_break_the_line_are_not_replayed() {
        assert_eq!(replay_line("playdemo", ""), None);
        assert_eq!(replay_line("playdemo", "a\"; quit"), None);
        assert_eq!(replay_line("playdemo", "a;quit"), None);
        assert_eq!(replay_line("playdemo", "a\nquit"), None);
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
