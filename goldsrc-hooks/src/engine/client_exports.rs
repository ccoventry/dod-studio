//! client.dll's `cldll_func_t` table: the slots `F` fills that we swap for
//! our own trampolines, and the trampolines themselves, each of which
//! chains to the client's real function.

use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use super::client_cmd::watch_client_commands;
use super::frame::tramp_hud_frame;
use super::sdk::{ClEngineFuncsPartial, ClEntityS, EngineStudioApiPartial, RefParamsPartial};
use super::{ENGFUNCS, ENGINE_STUDIO};

/// Number of function-pointer slots the secured `F` export writes into the
/// buffer the engine hands it. Confirmed two independent ways: by
/// disassembling DoD 1.3's own `F` (`rep movsd` with `ecx = 0x2b` = 43), and
/// against Xash3D's `cldll_func_src_t`, whose 43 members line up name-for-name
/// with the 43 addresses `F` writes (each of which resolves back to the
/// identically-named export in `client.dll`).
const CLDLL_FUNC_SLOTS: usize = 43;

/// Slot indices within that table -- verified by resolving every address DoD's
/// `F` writes back to its own export name.
const SLOT_INITIALIZE: usize = 0;
/// Checked against both installs' `client.dll` (#660): `F` writes
/// `HUD_Redraw` here and `HUD_Key_Event` at slot 34. `hw.dll` calls the first
/// as `(float time, int intermission)` (pre-Anniversary `+0xbec9`, 25th
/// Anniversary `+0x196f5a`) and the second as `(down, key, binding)` from
/// `ClientDLL_Key_Event` (`+0xb98f`, `+0x1971d4`).
const SLOT_HUD_REDRAW: usize = 3;
/// `V_CalcRefdef`, the slot before `HUD_AddEntity` in `cldll_func_t`
/// (`pfnCalcRefdef` in Xash3D's `cldll_func_src_t`).
const SLOT_CALC_REFDEF: usize = 19;
const SLOT_HUD_ADD_ENTITY: usize = 20;
/// Checked 2026-10-11: `F` stores `HUD_DrawTransparentTriangles`
/// (`+0x4c830`) at its table's slot 23.
const SLOT_DRAW_TRANSPARENT_TRIANGLES: usize = 23;
/// `F` stores `HUD_Shutdown` (`+0x2a570`) at slot 26.
const SLOT_HUD_SHUTDOWN: usize = 26;
const SLOT_HUD_FRAME: usize = 33;
const SLOT_HUD_KEY_EVENT: usize = 34;
/// Re-checked 2026-09-29 against both installs' `client.dll` (byte-identical):
/// `F` writes `HUD_DirectorMessage` (`+0x2a6a0`) here.
const SLOT_HUD_DIRECTOR_MESSAGE: usize = 38;
const SLOT_GET_STUDIO_MODEL_INTERFACE: usize = 39;

const _: () = assert!(
    SLOT_INITIALIZE < CLDLL_FUNC_SLOTS
        && SLOT_CALC_REFDEF < CLDLL_FUNC_SLOTS
        && SLOT_HUD_ADD_ENTITY < CLDLL_FUNC_SLOTS
        && SLOT_DRAW_TRANSPARENT_TRIANGLES < CLDLL_FUNC_SLOTS
        && SLOT_HUD_SHUTDOWN < CLDLL_FUNC_SLOTS
        && SLOT_HUD_FRAME < CLDLL_FUNC_SLOTS
        && SLOT_HUD_REDRAW < CLDLL_FUNC_SLOTS
        && SLOT_HUD_KEY_EVENT < CLDLL_FUNC_SLOTS
        && SLOT_HUD_DIRECTOR_MESSAGE < CLDLL_FUNC_SLOTS
        && SLOT_GET_STUDIO_MODEL_INTERFACE < CLDLL_FUNC_SLOTS,
    "a cldll_func_t slot index is outside the table F actually writes"
);

type InitializeFn = unsafe extern "C" fn(*mut ClEngineFuncsPartial, i32) -> i32;
pub(super) type HudFrameFn = unsafe extern "C" fn(f64);
/// `int HUD_Redraw(float time, int intermission)`.
type HudRedrawFn = unsafe extern "C" fn(f32, i32) -> i32;
/// `int HUD_Key_Event(int down, int keynum, const char *binding)`: 0
/// swallows the key, so the engine runs no bind for it.
type HudKeyEventFn = unsafe extern "C" fn(i32, i32, *const c_char) -> i32;
/// `void V_CalcRefdef(struct ref_params_s *pparams)`.
type CalcRefdefFn = unsafe extern "C" fn(*mut RefParamsPartial);
type GetStudioModelInterfaceFn =
    unsafe extern "C" fn(i32, *mut *mut c_void, *mut EngineStudioApiPartial) -> i32;
/// `int (*pHudAddEntity)(int type, cl_entity_t *ent, const char *modelname)`.
/// `ent` is passed through opaquely -- nothing here reads its fields, only
/// `modelname`, so there's no need to model `cl_entity_t`'s own layout.
type HudAddEntityFn = unsafe extern "C" fn(i32, *mut c_void, *const c_char) -> i32;
/// `void HUD_DrawTransparentTriangles(void)`.
type DrawTransparentTrianglesFn = unsafe extern "C" fn();
/// `void HUD_Shutdown(void)`.
type HudShutdownFn = unsafe extern "C" fn();
/// `void (*pfnDirectorMessage)(int iSize, void *pbuf)`: one `svc_director`
/// message, with the opcode and length byte already stripped.
type HudDirectorMessageFn = unsafe extern "C" fn(i32, *mut c_void);
/// The secured single-callback export: fills the caller-provided buffer with
/// `CLDLL_FUNC_SLOTS` function pointers. `__cdecl`, one pointer argument --
/// confirmed from `hw.dll`'s call site (`push edx; call eax; add esp, 4`).
type ClientApiFn = unsafe extern "C" fn(*mut *mut c_void);

pub(super) static REAL_F: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
pub(super) static REAL_INITIALIZE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
pub(super) static REAL_HUD_ADD_ENTITY: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
pub(super) static REAL_DRAW_TRANSPARENT_TRIANGLES: AtomicPtr<c_void> =
    AtomicPtr::new(std::ptr::null_mut());
pub(super) static REAL_HUD_SHUTDOWN: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
pub(super) static REAL_CALC_REFDEF: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
pub(super) static REAL_HUD_FRAME: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
pub(super) static REAL_HUD_REDRAW: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
pub(super) static REAL_HUD_KEY_EVENT: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
pub(super) static REAL_HUD_DIRECTOR_MESSAGE: AtomicPtr<c_void> =
    AtomicPtr::new(std::ptr::null_mut());
pub(super) static REAL_GET_STUDIO_MODEL_INTERFACE: AtomicPtr<c_void> =
    AtomicPtr::new(std::ptr::null_mut());

/// Runs once, on the engine's own thread, immediately after `client.dll`'s
/// real `Initialize` returns -- see `set_on_engine_ready`.
static ON_ENGINE_READY: std::sync::OnceLock<fn()> = std::sync::OnceLock::new();

/// Registers the work that must happen once `pEngfuncs` is live.
///
/// This exists so that work runs **on the engine thread, at a deterministic
/// point**, rather than from our worker thread whenever it happens to notice.
/// It matters because that work mutates engine-owned global state --
/// `pfnAddCommand` prepends to the engine's command list, and `fire_sounds`
/// overwrites a function pointer inside the live `event_api_s` -- none of
/// which is thread-safe against an engine that may be running concurrently.
/// Registering right after `Initialize` also matches where DoD's own client
/// registers its 110 console commands (from `HUD_Init`, immediately after).
///
/// Must be called before `install()`, so there is no window in which
/// `Initialize` could fire before the callback is set.
pub fn set_on_engine_ready(callback: fn()) {
    let _ = ON_ENGINE_READY.set(callback);
}

/// Captures `pEnginefuncs` -- the whole reason this crate exists -- then hands
/// straight off to `client.dll`'s real `Initialize` so the game is unaffected.
pub(super) unsafe extern "C" fn tramp_initialize(
    engfuncs: *mut ClEngineFuncsPartial,
    version: i32,
) -> i32 {
    if !engfuncs.is_null() {
        ENGFUNCS.store(engfuncs, Ordering::Release);
        unsafe {
            crate::debug::report(&format!(
                "Initialize: captured pEngfuncs at {engfuncs:p} (interface version {version})"
            ))
        };
    } else {
        unsafe {
            crate::debug::report(
                "Initialize: engine passed a null pEnginefuncs -- nothing captured",
            )
        };
    }

    let real = REAL_INITIALIZE.load(Ordering::Acquire);
    if real.is_null() {
        // Can't happen (we only ever install the trampoline after storing the
        // real pointer), but returning "wrong interface version" is the
        // engine's own documented failure signal, so fail visibly, not weirdly.
        return 0;
    }
    let real: InitializeFn = unsafe { std::mem::transmute(real) };
    // Before the client copies the table: high-quality overview tiles.
    unsafe { crate::overview_hd::wrap(engfuncs as *mut std::ffi::c_void) };
    let result = if engfuncs.is_null() {
        unsafe { real(engfuncs, version) }
    } else {
        // The client registers its event handlers from inside Initialize;
        // missing_shots notes the weapon ones as they go past.
        unsafe { crate::missing_shots::noting_event_handlers(engfuncs, || real(engfuncs, version)) }
    };

    // Only once the client is genuinely initialised, and only if we actually
    // captured the table -- installing against a null pEngfuncs would crash
    // the game rather than merely fail.
    // gEngfuncs is only populated once the real Initialize has run, so this
    // has to come after it.
    unsafe { watch_client_commands() };
    // The HUD loads its sheets (`SPR_Load`) at the first VidInit, right after
    // this, so the hooks that map handles to paths go in now, not on the
    // first `ds_map_hide_asset add` (#808).
    crate::hud_sprites::install();
    // Before the first map loads, whose .bsp copy the client would leak.
    crate::map_file_leak::install();

    if !engfuncs.is_null()
        && let Some(callback) = ON_ENGINE_READY.get()
    {
        callback();
    }

    result
}

/// Captures `pstudio`, which `anim_fix` needs to read a model's sequence
/// labels.
pub(super) unsafe extern "C" fn tramp_get_studio_model_interface(
    version: i32,
    ppinterface: *mut *mut c_void,
    pstudio: *mut EngineStudioApiPartial,
) -> i32 {
    if !pstudio.is_null() {
        ENGINE_STUDIO.store(pstudio, Ordering::Release);
        unsafe {
            crate::debug::report(&format!(
                "HUD_GetStudioModelInterface: captured pstudio at {pstudio:p} (studio interface version {version})"
            ))
        };
    }

    let real = REAL_GET_STUDIO_MODEL_INTERFACE.load(Ordering::Acquire);
    if real.is_null() {
        return 0;
    }
    let real: GetStudioModelInterfaceFn = unsafe { std::mem::transmute(real) };
    // Before the client copies the engine's studio API: x-ray's
    // `StudioDrawPoints` has to be in the copy.
    unsafe { crate::spec_xray::install(pstudio) };
    let result = unsafe { real(version, ppinterface, pstudio) };
    if result != 0 && !ppinterface.is_null() {
        // Safety: the client just filled it with its renderer interface,
        // a static that lives as long as client.dll.
        unsafe { crate::strafe_twitch::install(*ppinterface as *mut *mut c_void) };
    }
    result
}

/// Puts back the HUD's FOV a POV demo played as HLTV left behind
/// (`hltv_fov`), sets the in-eye camera's crouch height for the frame
/// (`spectator_eye`), runs `client.dll`'s own `V_CalcRefdef`, then puts a chase camera's
/// position where the engine culls an HLTV demo's entities from
/// (`spectator_chase_pvs`), lets the spectated view
/// replace the camera for a dead player (`spectator_deathcam`, before the
/// position is recorded, so the recorded view is the drawn one), adjust the
/// viewmodel it just placed (`spectator_gun`) and keep its live bob for a POV
/// demo (`pov_live_bob`). A POV demo's crosshair takes the main view's
/// velocity (`pov_crosshair_gap`).
pub(super) unsafe extern "C" fn tramp_calc_refdef(pparams: *mut RefParamsPartial) {
    let real = REAL_CALC_REFDEF.load(Ordering::Acquire);
    if real.is_null() {
        return;
    }
    let real: CalcRefdefFn = unsafe { std::mem::transmute(real) };
    let main_view = unsafe { crate::spectator_chase_pvs::is_main_view(pparams) };
    crate::spec_xray::before_calc_refdef();
    if main_view {
        // Before the viewmodel is drawn, which reads the HUD's FOV.
        crate::hltv_fov::settle_hud_fov();
    }
    // Before: `V_GetInEyePos` reads the height inside `V_CalcRefdef`.
    unsafe { crate::spectator_eye::before_calc_refdef(pparams) };
    unsafe { real(pparams) };
    if main_view {
        // Safety: non-null (`is_main_view`), the engine's for this call.
        crate::pov_crosshair_gap::note_view(unsafe { &*pparams });
    }
    unsafe { crate::spectator_chase_pvs::after_calc_refdef(pparams, main_view) };
    unsafe { crate::spectator_deathcam::after_calc_refdef(pparams) };
    if !pparams.is_null() {
        // Safety: the engine's ref_params_s for this view, valid for the call.
        crate::position::record(unsafe { &*pparams });
    }
    unsafe { crate::spectator_gun::after_calc_refdef(pparams) };
    crate::screen_effects::after_calc_refdef();
    crate::pov_live_bob::after_calc_refdef(pparams);
}

/// Runs `client.dll`'s own `HUD_DrawTransparentTriangles` (where the sniper
/// scope is drawn), after `demo_scope` has read the frame's render view, and
/// with its black fill around the scope (#799).
pub(super) unsafe extern "C" fn tramp_draw_transparent_triangles() {
    let real = REAL_DRAW_TRANSPARENT_TRIANGLES.load(Ordering::Acquire);
    if real.is_null() {
        return;
    }
    crate::demo_scope::before_transparent_triangles();
    crate::pov_live_bob::before_view_model();
    let real: DrawTransparentTrianglesFn = unsafe { std::mem::transmute(real) };
    // Safety: the client's own export, called on the engine's thread.
    crate::demo_scope::with_black_fill(|| unsafe { real() });
}

/// Runs `client.dll`'s own `HUD_Redraw`, then draws the players' name
/// plates (#1130) and the review's panel over the HUD (#660).
pub(super) unsafe extern "C" fn tramp_hud_redraw(time: f32, intermission: i32) -> i32 {
    let real = REAL_HUD_REDRAW.load(Ordering::Acquire);
    let result = if real.is_null() {
        1
    } else {
        let real: HudRedrawFn = unsafe { std::mem::transmute(real) };
        // Safety: the client's own export, called on the engine's thread.
        unsafe { real(time, intermission) }
    };
    // Safety: on the engine's thread, inside HUD_Redraw. The plates go
    // first, so the review's panel covers them.
    unsafe {
        crate::nameplates::draw(intermission);
        crate::review_keys::draw();
    }
    result
}

/// A key the engine is about to run the bind of, only while keys go to the
/// game (never the console, chat or a window's box). The review's answer
/// keys take theirs here (#660); everything else goes to the client as is.
pub(super) unsafe extern "C" fn tramp_hud_key_event(
    down: i32,
    key: i32,
    binding: *const c_char,
) -> i32 {
    if crate::review_keys::key_event(down, key) {
        return 0;
    }
    let real = REAL_HUD_KEY_EVENT.load(Ordering::Acquire);
    if real.is_null() {
        return 1;
    }
    let real: HudKeyEventFn = unsafe { std::mem::transmute(real) };
    // Safety: the client's own export, with the engine's own arguments.
    unsafe { real(down, key, binding) }
}

/// Called once per entity the engine is about to add to the render list.
/// Returning 0 suppresses that one entity; see `hide_asset.rs`'s module doc
/// for the evidence behind that contract and why it isn't patched.
pub(super) unsafe extern "C" fn tramp_hud_add_entity(
    entity_type: i32,
    ent: *mut c_void,
    modelname: *const c_char,
) -> i32 {
    if !modelname.is_null() {
        let name = unsafe { std::ffi::CStr::from_ptr(modelname) }.to_string_lossy();
        // Type 2 is ET_TEMPENTITY: the engine's tempent callback adds every
        // temporary effect through here too (#614).
        let drawn_as = if entity_type == 2 {
            crate::hide_asset::DrawnAs::Effect
        } else {
            crate::hide_asset::DrawnAs::Entity
        };
        if crate::hide_asset::should_hide(&name, drawn_as) {
            return 0;
        }
    }

    let real = REAL_HUD_ADD_ENTITY.load(Ordering::Acquire);
    if real.is_null() {
        // Can't tell the entity apart from one that should draw, so the safe
        // side to fail on is the one that changes nothing: let it through.
        return 1;
    }
    let real: HudAddEntityFn = unsafe { std::mem::transmute(real) };
    let drawn = unsafe { real(entity_type, ent, modelname) };
    if drawn != 0 {
        // Safety: the engine's own `cl_entity_t` for this call. The client's
        // answer counts: it drops the player an HLTV camera watches in-eye.
        unsafe { crate::nameplates::note_added(ent as *const ClEntityS) };
    }
    // Safety: as above; lets a player x-ray glows for past the engine's PVS
    // check, which runs next.
    unsafe { crate::spec_xray::after_add_entity(drawn != 0, ent as *const ClEntityS) };
    drawn
}

/// Set once `client.dll`'s `HUD_Shutdown` has run: the game is quitting.
static CLIENT_SHUT_DOWN: AtomicBool = AtomicBool::new(false);

/// Whether the client has shut down. Once it has, the engine frees every
/// cvar the client side registered, ours included, before SDL takes the
/// window down, so a cvar pointer must not be read any more: `ShowWindow`
/// reaching `behind_imports` read a freed `ds_capture_stay_behind` and
/// crashed the game as it quit (#1080).
pub fn client_shut_down() -> bool {
    CLIENT_SHUT_DOWN.load(Ordering::Acquire)
}

/// Marks the client shut down before `client.dll`'s own `HUD_Shutdown` runs,
/// so nothing reads our cvars past this point.
pub(super) unsafe extern "C" fn tramp_hud_shutdown() {
    if !CLIENT_SHUT_DOWN.swap(true, Ordering::AcqRel) {
        unsafe {
            crate::debug::report("HUD_Shutdown: the game is quitting; our cvars are no longer read")
        };
    }
    let real = REAL_HUD_SHUTDOWN.load(Ordering::Acquire);
    if !real.is_null() {
        let real: HudShutdownFn = unsafe { std::mem::transmute(real) };
        unsafe { real() };
    }
}

/// Called once per `svc_director` message. Drops the ones
/// `hltv_messages::should_drop` asks for and forwards the rest unchanged.
pub(super) unsafe extern "C" fn tramp_hud_director_message(size: i32, buf: *mut c_void) {
    if size > 0 && !buf.is_null() {
        // Safety: the engine's own buffer of `size` bytes, alive for the call.
        let bytes = unsafe { std::slice::from_raw_parts(buf as *const u8, size as usize) };
        if crate::hltv_messages::should_drop(bytes) {
            return;
        }
    }

    let real = REAL_HUD_DIRECTOR_MESSAGE.load(Ordering::Acquire);
    if real.is_null() {
        return;
    }
    let real: HudDirectorMessageFn = unsafe { std::mem::transmute(real) };
    unsafe { real(size, buf) }
}

/// Swaps one slot of the `cldll_func_t` table `F` just filled for our own
/// trampoline, stashing the real pointer so the trampoline can chain to it.
///
/// Safety: `table` must point to at least `CLDLL_FUNC_SLOTS` writable
/// pointer-sized slots (guaranteed by `F`'s own contract -- it just wrote
/// exactly that many).
unsafe fn swap_slot(
    table: *mut *mut c_void,
    slot: usize,
    real_store: &AtomicPtr<c_void>,
    trampoline: *mut c_void,
    name: &str,
) {
    let entry = unsafe { table.add(slot) };
    let real = unsafe { *entry };
    if real.is_null() {
        unsafe {
            crate::debug::report(&format!(
                "F: slot {slot} ({name}) is null -- leaving it alone"
            ))
        };
        return;
    }
    real_store.store(real, Ordering::Release);
    unsafe { *entry = trampoline };
    unsafe {
        crate::debug::report(&format!(
            "F: hooked slot {slot} ({name}), real implementation at {real:p}"
        ))
    };
}

/// Our stand-in for `client.dll`'s secured `F` export. Lets the real `F` fill
/// the engine's `cldll_func_t` buffer exactly as it normally would, then
/// replaces the ten slots we care about before the engine ever reads them.
pub(super) unsafe extern "C" fn hook_f(table: *mut *mut c_void) {
    let real = REAL_F.load(Ordering::Acquire);
    if real.is_null() {
        unsafe {
            crate::debug::report(
                "F: real export pointer missing -- cannot forward, client will fail to initialise",
            )
        };
        return;
    }
    let real: ClientApiFn = unsafe { std::mem::transmute(real) };
    unsafe { real(table) };

    if table.is_null() {
        unsafe { crate::debug::report("F: engine passed a null table -- nothing hooked") };
        return;
    }

    unsafe {
        swap_slot(
            table,
            SLOT_INITIALIZE,
            &REAL_INITIALIZE,
            tramp_initialize as *mut c_void,
            "Initialize",
        );
        swap_slot(
            table,
            SLOT_HUD_FRAME,
            &REAL_HUD_FRAME,
            tramp_hud_frame as *mut c_void,
            "HUD_Frame",
        );
        swap_slot(
            table,
            SLOT_HUD_REDRAW,
            &REAL_HUD_REDRAW,
            tramp_hud_redraw as *mut c_void,
            "HUD_Redraw",
        );
        swap_slot(
            table,
            SLOT_HUD_KEY_EVENT,
            &REAL_HUD_KEY_EVENT,
            tramp_hud_key_event as *mut c_void,
            "HUD_Key_Event",
        );
        swap_slot(
            table,
            SLOT_CALC_REFDEF,
            &REAL_CALC_REFDEF,
            tramp_calc_refdef as *mut c_void,
            "V_CalcRefdef",
        );
        swap_slot(
            table,
            SLOT_HUD_ADD_ENTITY,
            &REAL_HUD_ADD_ENTITY,
            tramp_hud_add_entity as *mut c_void,
            "HUD_AddEntity",
        );
        swap_slot(
            table,
            SLOT_DRAW_TRANSPARENT_TRIANGLES,
            &REAL_DRAW_TRANSPARENT_TRIANGLES,
            tramp_draw_transparent_triangles as *mut c_void,
            "HUD_DrawTransparentTriangles",
        );
        swap_slot(
            table,
            SLOT_HUD_SHUTDOWN,
            &REAL_HUD_SHUTDOWN,
            tramp_hud_shutdown as *mut c_void,
            "HUD_Shutdown",
        );
        swap_slot(
            table,
            SLOT_HUD_DIRECTOR_MESSAGE,
            &REAL_HUD_DIRECTOR_MESSAGE,
            tramp_hud_director_message as *mut c_void,
            "HUD_DirectorMessage",
        );
        swap_slot(
            table,
            SLOT_GET_STUDIO_MODEL_INTERFACE,
            &REAL_GET_STUDIO_MODEL_INTERFACE,
            tramp_get_studio_model_interface as *mut c_void,
            "HUD_GetStudioModelInterface",
        );
    }
}
