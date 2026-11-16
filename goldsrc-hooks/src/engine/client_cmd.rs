//! Watching what client.dll tells the engine to do, through its own copy of
//! the engine's table (`gEngfuncs`), and running a command the same way.

use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicPtr, Ordering};

use super::client_exports::REAL_INITIALIZE;
use super::engfuncs;
use super::loader::CLIENT_DLL;
use super::sdk::ClEngineFuncsPartial;

/// `pfnClientCmd`'s index in `cl_enginefunc_t`, verified against the binary --
/// DoD's client calls this slot 12 times.
const ENGFUNCS_SLOT_CLIENT_CMD: usize = 20;

type ClientCmdFn = unsafe extern "C" fn(*const c_char);
static REAL_CLIENT_CMD: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Runs `line` as a console command through the engine's `pfnClientCmd`, as
/// if `client.dll` had issued it. Main thread only, like every engine call.
/// False before the engine's function table is captured.
// Only 32-bit code (`remote`, `demo_reload`) calls it.
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
pub(crate) fn client_cmd(line: &std::ffi::CStr) -> bool {
    let Some(engfuncs) = engfuncs() else {
        return false;
    };
    // Safety: a slot of the engine's function table, the one this file already
    // redirects for its client-cmd watch.
    unsafe {
        let slot = *(engfuncs as *const ClEngineFuncsPartial as *const usize)
            .add(ENGFUNCS_SLOT_CLIENT_CMD);
        if slot == 0 {
            return false;
        }
        let client_cmd: ClientCmdFn = std::mem::transmute(slot);
        client_cmd(line.as_ptr());
    }
    true
}

/// Logs every console command `client.dll` issues to itself.
///
/// The motivating case is DoD's own cvar-enforcement routine, which runs from
/// `CHud::Redraw` -- every rendered frame -- and responds to `r_drawentities`
/// or `cl_lw` not being 1 by forcing the value back, printing a warning, and
/// then issuing `quit`. It assembles that `quit` byte-by-byte on the stack
/// rather than storing it as a literal, so nothing in the binary's strings
/// gives it away, and if the process exits before `qconsole.log` flushes there
/// is no trace at all. Catching it here is the only reliable way to tell "the
/// game quit on purpose" from "the game crashed". See issue #205.
unsafe extern "C" fn hook_client_cmd(text: *const c_char) {
    if !text.is_null() {
        let raw = unsafe { std::ffi::CStr::from_ptr(text) }
            .to_string_lossy()
            .into_owned();
        let trimmed = raw.trim();
        if trimmed.eq_ignore_ascii_case("quit") {
            unsafe {
                crate::debug::report(
                    "!!! client.dll issued 'quit' -- DoD is closing itself deliberately, not crashing. \
                     Its cvar enforcement does this when r_drawentities or cl_lw is not 1 (issue #205); \
                     the preceding console line names which one.",
                )
            };
        } else {
            unsafe {
                crate::debug::report(&format!(
                    "client.dll ClientCmd: \"{}\"",
                    trimmed.escape_debug()
                ))
            };
        }
    }

    let real = REAL_CLIENT_CMD.load(Ordering::Acquire);
    if !real.is_null() {
        let real: ClientCmdFn = unsafe { std::mem::transmute(real) };
        unsafe { real(text) };
    }
}

/// Finds `client.dll`'s own copy of `cl_enginefunc_t` by reading it straight
/// out of the real `Initialize`, which begins by memcpy-ing the engine's table
/// into a fixed global:
///
/// ```text
///   b9 87 00 00 00     mov ecx, 0x87        ; 135 dwords = sizeof(cl_enginefunc_t)
///   ...
///   bf <imm32>         mov edi, gEngfuncs   ; <- the address we want
///   50                 push eax
///   f3 a5              rep movsd
/// ```
///
/// The immediate is read from the *loaded* module, so the loader has already
/// applied relocations to it and it is the runtime address.
///
/// Safety: `real_initialize` must point at the real, mapped `Initialize`.
unsafe fn find_gengfuncs(real_initialize: *const u8) -> Option<*mut *mut c_void> {
    let window = unsafe { std::slice::from_raw_parts(real_initialize, 0x60) };
    for k in 0..window.len().saturating_sub(8) {
        if window[k] == 0xBF
            && window[k + 5] == 0x50
            && window[k + 6] == 0xF3
            && window[k + 7] == 0xA5
        {
            let addr =
                u32::from_le_bytes([window[k + 1], window[k + 2], window[k + 3], window[k + 4]])
                    as usize;
            // Must land inside client.dll's own image, or the pattern matched
            // something that only looked like the prologue.
            let base = CLIENT_DLL.load(Ordering::Acquire) as usize;
            if base != 0 && addr > base && addr < base + 0x0400_0000 {
                return Some(addr as *mut *mut c_void);
            }
        }
    }
    None
}

/// `client.dll`'s own `cl_enginefunc_t` copy, once the real `Initialize` is
/// known (see [`find_gengfuncs`]); `hud_sprites` patches its `SPR_*`
/// pointers the way [`watch_client_commands`] patches `pfnClientCmd`.
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
pub(crate) fn client_gengfuncs() -> Option<*mut *mut c_void> {
    let real_initialize = REAL_INITIALIZE.load(Ordering::Acquire) as *const u8;
    if real_initialize.is_null() {
        return None;
    }
    unsafe { find_gengfuncs(real_initialize) }
}

/// Writes `hook` into slot `slot` of client.dll's `gEngfuncs` copy and
/// returns what was there.
///
/// # Safety
/// `gengfuncs` is the live copy from [`client_gengfuncs`], and `hook` has
/// the slot's signature.
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
pub(crate) unsafe fn patch_gengfuncs_slot(
    gengfuncs: *mut *mut c_void,
    slot: usize,
    hook: *mut c_void,
) -> Result<*mut c_void, String> {
    unsafe {
        let at = gengfuncs.add(slot);
        let was = *at;
        if was.is_null() {
            return Err(format!("slot {slot} is null"));
        }
        if !crate::patch::write_pointer(at, hook, crate::patch::After::Restore) {
            return Err(format!("slot {slot} could not be made writable"));
        }
        Ok(was)
    }
}

/// Redirects `client.dll`'s own `pfnClientCmd` through `hook_client_cmd`.
///
/// This patches one function pointer inside a struct client.dll owns, rather
/// than swapping a `cldll_func_t` slot the engine also uses -- deliberately,
/// after hooking `V_CalcRefdef` that way closed the game.
pub(super) unsafe fn watch_client_commands() {
    let real_initialize = REAL_INITIALIZE.load(Ordering::Acquire) as *const u8;
    if real_initialize.is_null() {
        return;
    }
    let Some(gengfuncs) = (unsafe { find_gengfuncs(real_initialize) }) else {
        unsafe {
            crate::debug::report(
                "client-cmd watch: couldn't locate client.dll's gEngfuncs copy; not watching",
            )
        };
        return;
    };

    let slot = unsafe { gengfuncs.add(ENGFUNCS_SLOT_CLIENT_CMD) };
    let real = unsafe { *slot };
    if real.is_null() {
        unsafe {
            crate::debug::report("client-cmd watch: pfnClientCmd slot is null; not watching")
        };
        return;
    }
    REAL_CLIENT_CMD.store(real, Ordering::Release);

    unsafe {
        if !crate::patch::write_pointer(
            slot,
            hook_client_cmd as *mut c_void,
            crate::patch::After::Restore,
        ) {
            crate::debug::report("client-cmd watch: couldn't make the slot writable; not watching");
            return;
        }
        crate::debug::report(&format!(
            "client-cmd watch: gEngfuncs at {gengfuncs:p}, pfnClientCmd was {real:p} -- now logging what client.dll runs"
        ));
    }
}
