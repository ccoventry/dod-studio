//! Captures the engine's `cl_enginefuncs_s` table (`pEngfuncs`), the
//! `engine_studio_api_s` table (`pstudio`), a genuine per-frame callback, and
//! a per-entity suppress-or-forward hook for `hide_asset.rs`, by
//! intercepting how `hw.dll` resolves `client.dll`'s entry points in the
//! first place.
//!
//! ## Why this isn't the "obvious" export hook
//!
//! Two earlier designs failed, and understanding why is what makes this one
//! correct (see `docs/goldsrc_client_dll_internals.md` for the full
//! reverse-engineering write-up, and issue #204 for the investigation trail):
//!
//! 1. **Patching `client.dll`'s export table** for `Initialize` / `HUD_Frame`
//!    / `HUD_GetStudioModelInterface`. The patch was provably applied (the
//!    real Win32 `GetProcAddress` read our value back), yet nothing ever
//!    called through it. Root cause, confirmed by disassembling `hw.dll`:
//!    **the engine never asks for those names at all** on a "secured"
//!    `client.dll`. It calls `GetProcAddress(hClient, "F")` and, if that
//!    single export exists, calls it and stops -- if `F` is absent it
//!    `FreeLibrary`s and fails outright, with no per-name fallback on that
//!    path. DoD 1.3's `client.dll` does export `F`, so the classic names are
//!    dead weight in it. Xash3D's own GoldSrc-compatible loader calls this
//!    exact case "single callback export (secured client dlls)".
//!
//! 2. **Byte-signature scanning for `pEngfuncs`**, ported faithfully from
//!    HLAE's `hl_addresses.cpp`. Its anchor string `"ScreenFade"` simply does
//!    not exist anywhere in DoD's `client.dll` (verified both in-process and
//!    by offline string extraction from the file), so the scan can't start.
//!
//! ## What this module does instead
//!
//! It hooks **`hw.dll`'s Import Address Table entry for
//! `KERNEL32!GetProcAddress`** -- the exact call site the disassembly shows
//! the engine using -- alongside the `LoadLibraryA` entry it already hooked
//! to spot `client.dll` loading. This is the same IAT technique HLAE's own
//! `CAfxImportDllHook` uses, and the `LoadLibraryA` half was already proven
//! reliable in testing; the `GetProcAddress` half is a direct extension of it.
//!
//! Every lookup against the `client.dll` module is then handled in one place,
//! covering **both** engine conventions with one mechanism and no byte
//! patterns or hardcoded offsets:
//!
//! - `"F"` (secured builds, what DoD 1.3 actually uses) -- we return our own
//!   wrapper, which calls the real `F` to let it fill the caller's
//!   `cldll_func_t` table, then swaps ten of its 43 slots for our
//!   trampolines before handing it back to the engine.
//! - `"Initialize"` / `"HUD_Frame"` / `"HUD_AddEntity"` / `"HUD_Redraw"` /
//!   `"HUD_Key_Event"` / `"HUD_DirectorMessage"` /
//!   `"HUD_GetStudioModelInterface"` (classic non-secured builds) -- we
//!   return the trampoline directly.
//!
//! Either way the trampolines receive what we need as ordinary arguments:
//! `Initialize` hands us `pEnginefuncs`, `HUD_GetStudioModelInterface` hands
//! us `pstudio`, `HUD_Frame` gives a real per-frame tick, and `HUD_AddEntity`
//! hands us the model path of every entity about to be added to the render
//! list -- the first of two that aren't a one-time capture, since it runs the
//! suppress/forward decision itself, once per entity. `HUD_DirectorMessage`
//! is the second: it hands us every HLTV director message, so
//! `hltv_messages.rs` can drop the proxy's on-screen text. `HUD_Redraw` and
//! `HUD_Key_Event` carry the review's on-screen panel and answer keys
//! (`review_keys.rs`, #660).

mod client_cmd;
mod client_exports;
mod frame;
mod loader;
mod sdk;

use std::sync::atomic::{AtomicPtr, Ordering};

pub(crate) use client_cmd::{client_cmd, client_gengfuncs, patch_gengfuncs_slot};
pub use client_exports::{client_shut_down, set_on_engine_ready};
pub(crate) use frame::hud_frame_driving;
pub use frame::{client_time, engine_client_time, set_per_frame_callback, set_per_frame_prologue};
// Only 32-bit code calls it, as its own `allow(dead_code)` says.
#[cfg_attr(not(target_arch = "x86"), allow(unused_imports))]
pub use loader::create_interface;
pub use loader::{client_module_base, engine_module_base, install};
pub use sdk::*;

static ENGFUNCS: AtomicPtr<ClEngineFuncsPartial> = AtomicPtr::new(std::ptr::null_mut());
static ENGINE_STUDIO: AtomicPtr<EngineStudioApiPartial> = AtomicPtr::new(std::ptr::null_mut());

/// Returns the captured engine function table, once the engine has called
/// `client.dll`'s `Initialize` through our trampoline. `None` before that.
pub fn engfuncs() -> Option<&'static ClEngineFuncsPartial> {
    let ptr = ENGFUNCS.load(Ordering::Acquire);
    if ptr.is_null() {
        None
    } else {
        // Safety: set only by `tramp_initialize` (client_exports.rs), to the
        // pointer the engine passes `Initialize`, which the engine owns for the
        // life of the client.dll instance.
        Some(unsafe { &*ptr })
    }
}

/// Returns the captured studio-model engine interface, once the engine has
/// called `client.dll`'s `HUD_GetStudioModelInterface` through our trampoline.
/// `None` before that.
pub fn engine_studio() -> Option<&'static EngineStudioApiPartial> {
    let ptr = ENGINE_STUDIO.load(Ordering::Acquire);
    if ptr.is_null() {
        None
    } else {
        // Safety: same guarantee as `engfuncs()` above.
        Some(unsafe { &*ptr })
    }
}
