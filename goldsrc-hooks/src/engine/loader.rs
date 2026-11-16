//! hw.dll's IAT hooks on `LoadLibraryA` and `GetProcAddress`, which is how
//! every hook in `client_exports` gets in (see the module docs in
//! `engine.rs`), and the module lookups built on what they see.

use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicPtr, Ordering};

use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleA;
use windows_sys::Win32::System::Threading::Sleep;

use super::client_exports::{
    REAL_CALC_REFDEF, REAL_DRAW_TRANSPARENT_TRIANGLES, REAL_F, REAL_GET_STUDIO_MODEL_INTERFACE,
    REAL_HUD_ADD_ENTITY, REAL_HUD_DIRECTOR_MESSAGE, REAL_HUD_FRAME, REAL_HUD_KEY_EVENT,
    REAL_HUD_REDRAW, REAL_HUD_SHUTDOWN, REAL_INITIALIZE, hook_f, tramp_calc_refdef,
    tramp_draw_transparent_triangles, tramp_get_studio_model_interface, tramp_hud_add_entity,
    tramp_hud_director_message, tramp_hud_key_event, tramp_hud_redraw, tramp_hud_shutdown,
    tramp_initialize,
};
use super::frame::tramp_hud_frame;
use crate::pe;

/// `client.dll`'s load address, once the engine has loaded it. `None` before
/// that.
///
/// Every address in `deathmsg.rs` is recorded as an RVA against the analysed
/// image and rebased through this, rather than assumed: `client.dll` opts out
/// of ASLR so it usually lands at its preferred 0x1900000, but a base conflict
/// still relocates it, and all 34 of the death-notice call sites carry base
/// relocations that the loader would have rewritten.
pub fn client_module_base() -> Option<usize> {
    let base = CLIENT_DLL.load(Ordering::Acquire) as usize;
    (base != 0).then_some(base)
}

/// The loaded engine's base address, or `None` before `hw.dll` exists.
///
/// `hw.dll` is loaded once and stays for the session, so asking the loader
/// each time is both correct and cheap. (`client.dll` is tracked through the
/// IAT hook instead; it does not reload between demos either, but whatever
/// else might reload it is untested -- see CLAUDE.md.)
pub fn engine_module_base() -> Option<usize> {
    let name = c"hw.dll";
    let handle = unsafe { GetModuleHandleA(name.as_ptr() as *const u8) };
    (!handle.is_null()).then_some(handle as usize)
}

/// One of the engine's or vgui2's interface singletons, by module and
/// version string (`VGUI_Surface026` from `hw.dll`, `VGUI_Panel007` from
/// `vgui2.dll`), through the module's exported `CreateInterface`. `None`
/// while the module isn't loaded or doesn't serve that name.
#[cfg_attr(not(target_arch = "x86"), allow(dead_code))]
pub fn create_interface(module: &std::ffi::CStr, name: &std::ffi::CStr) -> Option<*mut c_void> {
    use windows_sys::Win32::System::LibraryLoader::GetProcAddress;
    type CreateInterfaceFn = unsafe extern "C" fn(*const c_char, *mut i32) -> *mut c_void;
    let handle = unsafe { GetModuleHandleA(module.as_ptr() as *const u8) };
    if handle.is_null() {
        return None;
    }
    let create = unsafe { GetProcAddress(handle, c"CreateInterface".as_ptr() as *const u8) }?;
    // Safety: the interface factory's signature, the same on every Valve DLL.
    let create: CreateInterfaceFn = unsafe { std::mem::transmute(create) };
    let object = unsafe { create(name.as_ptr(), std::ptr::null_mut()) };
    (!object.is_null()).then_some(object)
}

type LoadLibraryAFn = unsafe extern "system" fn(*const u8) -> HMODULE;
type GetProcAddressFn = unsafe extern "system" fn(HMODULE, *const u8) -> *mut c_void;
static REAL_LOAD_LIBRARY_A: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static REAL_GET_PROC_ADDRESS: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
/// The `client.dll` module handle, as observed coming back out of
/// `LoadLibraryA`. Used to tell *its* `GetProcAddress` lookups apart from the
/// many others `hw.dll` makes against other modules.
pub(super) static CLIENT_DLL: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// The engine loads the mod's client DLL by a path relative to the game exe
/// (e.g. `dod\cl_dlls\client.dll`), never by the bare filename -- so this
/// compares only the path's basename, not the whole string.
unsafe fn path_basename_eq_ignore_ascii_case(ptr: *const u8, other: &str) -> bool {
    unsafe {
        if ptr.is_null() {
            return false;
        }
        let path = std::ffi::CStr::from_ptr(ptr as *const i8).to_string_lossy();
        let basename = path.rsplit(['\\', '/']).next().unwrap_or(&path);
        basename.eq_ignore_ascii_case(other)
    }
}

unsafe extern "system" fn hook_load_library_a(lp_lib_file_name: *const u8) -> HMODULE {
    let real = REAL_LOAD_LIBRARY_A.load(Ordering::Acquire);
    let real: LoadLibraryAFn = unsafe { std::mem::transmute(real) };
    let result = unsafe { real(lp_lib_file_name) };

    if !result.is_null()
        && unsafe { path_basename_eq_ignore_ascii_case(lp_lib_file_name, "client.dll") }
    {
        CLIENT_DLL.store(result, Ordering::Release);
        unsafe {
            crate::debug::report(&format!(
                "LoadLibraryA: client.dll loaded at {result:p}, watching its GetProcAddress lookups"
            ))
        };
    }

    result
}

/// Intercepts every export `hw.dll` resolves out of `client.dll`, and swaps in
/// our own entry points for the ones we need. See `engine.rs`'s module docs for why this
/// -- rather than patching `client.dll`'s export table -- is the hook point
/// that actually works.
unsafe extern "system" fn hook_get_proc_address(module: HMODULE, name: *const u8) -> *mut c_void {
    let real = REAL_GET_PROC_ADDRESS.load(Ordering::Acquire);
    let real: GetProcAddressFn = unsafe { std::mem::transmute(real) };
    let result = unsafe { real(module, name) };

    let client = CLIENT_DLL.load(Ordering::Acquire);
    if client.is_null() || !std::ptr::eq(module, client) || result.is_null() {
        return result;
    }

    // An import can be requested by ordinal instead of by name, in which case
    // `name` is not a pointer at all -- it's the ordinal in its low word, and
    // dereferencing it would fault.
    if (name as usize) >> 16 == 0 {
        return result;
    }
    let Ok(requested) = (unsafe { std::ffi::CStr::from_ptr(name as *const c_char) }).to_str()
    else {
        return result;
    };

    unsafe {
        crate::debug::report(&format!(
            "GetProcAddress(client.dll, \"{requested}\") -> {result:p}"
        ))
    };

    // "F" is the secured single-callback export, and is what DoD 1.3 actually
    // uses; the ten named entries below are the classic convention, kept so
    // this works unchanged on a non-secured client.dll too.
    match requested {
        "F" => {
            REAL_F.store(result, Ordering::Release);
            unsafe {
                crate::debug::report(
                    "GetProcAddress: secured \"F\" export intercepted -- returning our wrapper",
                )
            };
            hook_f as *mut c_void
        }
        "Initialize" => {
            REAL_INITIALIZE.store(result, Ordering::Release);
            tramp_initialize as *mut c_void
        }
        "HUD_Frame" => {
            REAL_HUD_FRAME.store(result, Ordering::Release);
            tramp_hud_frame as *mut c_void
        }
        "HUD_Redraw" => {
            REAL_HUD_REDRAW.store(result, Ordering::Release);
            tramp_hud_redraw as *mut c_void
        }
        "HUD_Key_Event" => {
            REAL_HUD_KEY_EVENT.store(result, Ordering::Release);
            tramp_hud_key_event as *mut c_void
        }
        "V_CalcRefdef" => {
            REAL_CALC_REFDEF.store(result, Ordering::Release);
            tramp_calc_refdef as *mut c_void
        }
        "HUD_AddEntity" => {
            REAL_HUD_ADD_ENTITY.store(result, Ordering::Release);
            tramp_hud_add_entity as *mut c_void
        }
        "HUD_DrawTransparentTriangles" => {
            REAL_DRAW_TRANSPARENT_TRIANGLES.store(result, Ordering::Release);
            tramp_draw_transparent_triangles as *mut c_void
        }
        "HUD_Shutdown" => {
            REAL_HUD_SHUTDOWN.store(result, Ordering::Release);
            tramp_hud_shutdown as *mut c_void
        }
        "HUD_DirectorMessage" => {
            REAL_HUD_DIRECTOR_MESSAGE.store(result, Ordering::Release);
            tramp_hud_director_message as *mut c_void
        }
        "HUD_GetStudioModelInterface" => {
            REAL_GET_STUDIO_MODEL_INTERFACE.store(result, Ordering::Release);
            tramp_get_studio_model_interface as *mut c_void
        }
        _ => result,
    }
}

/// Waits (briefly) for `module_name` to be loaded -- we're injected before the
/// engine's own modules exist, so this can't assume it's already there.
fn wait_for_module(module_name: &str, timeout_ms: u32) -> Option<HMODULE> {
    let module_name_c = format!("{module_name}\0");
    let mut waited = 0u32;
    loop {
        let h = unsafe { GetModuleHandleA(module_name_c.as_ptr()) };
        if !h.is_null() {
            return Some(h);
        }
        if waited >= timeout_ms {
            return None;
        }
        unsafe { Sleep(25) };
        waited += 25;
    }
}

/// Overwrites one of `module`'s IAT slots so its own calls to
/// `KERNEL32!<import_name>` route through `replacement`, stashing the original
/// in `real_store`. Only that module's calls are affected -- unlike
/// inline-patching the function itself, which would redirect the whole
/// process (HLAE's `CAfxImportDllHook` makes the same trade-off).
fn hook_import(
    module: HMODULE,
    import_name: &str,
    replacement: *mut c_void,
    real_store: &AtomicPtr<c_void>,
) -> bool {
    let Some(slot) = (unsafe { pe::find_iat_slot(module as *mut u8, "KERNEL32.dll", import_name) })
    else {
        return false;
    };

    unsafe {
        real_store.store(*slot, Ordering::Release);
        // Left writable: HLAE hooks imports on this page too (#1059).
        crate::patch::write_pointer(slot, replacement, crate::patch::After::LeaveWritable)
    }
}

/// Installs both `hw.dll` IAT hooks: `LoadLibraryA` (to learn `client.dll`'s
/// module handle) and `GetProcAddress` (to intercept how the engine resolves
/// `client.dll`'s entry points). Call once, from a background thread -- never
/// do this work directly in `DllMain` (see `lib.rs`).
pub fn install() {
    // GoldSrc's OpenGL renderer module is virtually always what's actually
    // loaded in any modern setup (HLAE's own recording pipeline requires
    // it); `sw.dll` (the old software renderer) is not handled here.
    let Some(hw) = wait_for_module("hw.dll", 15_000) else {
        unsafe {
            crate::debug::report(
                "goldsrc-hooks: hw.dll never appeared -- sound/animation fixes are inactive this session.",
            )
        };
        return;
    };

    // Order matters: GetProcAddress first, so there is no window in which
    // client.dll could load and be resolved before that hook is armed.
    let hooked_gpa = hook_import(
        hw,
        "GetProcAddress",
        hook_get_proc_address as *mut c_void,
        &REAL_GET_PROC_ADDRESS,
    );
    let hooked_lla = hook_import(
        hw,
        "LoadLibraryA",
        hook_load_library_a as *mut c_void,
        &REAL_LOAD_LIBRARY_A,
    );

    unsafe {
        crate::debug::report(&format!(
            "goldsrc-hooks: hw.dll IAT hooks -- GetProcAddress: {}, LoadLibraryA: {}",
            if hooked_gpa { "ok" } else { "FAILED" },
            if hooked_lla { "ok" } else { "FAILED" },
        ))
    };

    if !hooked_gpa || !hooked_lla {
        unsafe {
            crate::debug::report(
                "goldsrc-hooks: an IAT slot wasn't found -- sound/animation fixes are inactive this session.",
            )
        };
    }
}
