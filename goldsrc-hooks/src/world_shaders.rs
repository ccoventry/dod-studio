//! `dodstudio_allow_shaders`: let the 25th Anniversary engine draw the world
//! through its own GLSL shaders while a demo plays.
//!
//! ## What the engine does
//!
//! The 25th Anniversary `hw.dll` can draw world surfaces (the map's brushes;
//! not models, sprites or the HUD) through a GLSL program it compiles from
//! `platform/gl_shaders/vs_world.vert` and `fs_world.frag`. `gl_reloadshaders`
//! recompiles them in place. The pre-Anniversary engine has none of this.
//!
//! Every world-drawing site checks the same three things before using the
//! program (for example at `hw.dll+0x246f8b`): the shaders compiled, the
//! archived client cvar `gl_use_shaders` is non-zero, and `sv_allow_shaders`
//! is non-zero. Only the last one is the problem during a demo:
//!
//! - `sv_allow_shaders` defaults to `"0"` and carries `FCVAR_SPONLY`, so the
//!   console refuses it while connected to anything multiplayer -- which a
//!   playing demo counts as ("Can't set sv_allow_shaders in multiplayer").
//! - The only thing that sets it in a client is the `svc_stufftext` handler
//!   (`hw.dll+0x1aa3e0`): a server built for the 25th Anniversary sends
//!   `allow_shaders <0|1>` on connect, and the handler writes the cvar
//!   directly. Demos recorded on older servers never carry that line.
//! - The disconnect path (`hw.dll+0x1a2ce3`) writes `"0"` back whenever this
//!   process is not hosting a server, and every `playdemo`/`viewdemo` begins
//!   with a disconnect -- so a value set at the menu is gone before the
//!   demo's first frame.
//!
//! ## What this does
//!
//! With `dodstudio_allow_shaders 1`, `poll` writes `1.0` into
//! `sv_allow_shaders`' `value` whenever a demo is playing and it reads 0,
//! which catches the reset after every demo load. It finds the cvar through
//! `pfnGetCvarPointer` by name, so no engine address is involved and nothing
//! here is per build; on the pre-Anniversary engine the cvar does not exist
//! and this does nothing.
//!
//! Only `value` is written, because that is the only field the draw code
//! reads. The engine owns the `string` field and frees it on the next real
//! set, so it is left alone -- which means typing `sv_allow_shaders` in the
//! console still prints `"0"` while shaders are on. `dodstudio_debug_status`
//! reports the value that counts.
//!
//! It acts only while `pDemoAPI->IsPlayingback()` is true, so it never
//! overrides a live server's own `allow_shaders 0`. `gl_use_shaders` is not
//! touched: it is archived, so writing it would end up in the user's
//! `config.cfg`. If it is 0, turning this on says so in the console.
//!
//! Default 0, like every other `dodstudio_` switch: nothing changes until it
//! is asked for.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};

use crate::engine::{self, ClEngineFuncsPartial, CvarSPartial};
use crate::names::console_name;

pub const NAME: &str = console_name!("allow_shaders");

/// The engine's own gate. See the module docs.
const ENGINE_CVAR: &std::ffi::CStr = c"sv_allow_shaders";
/// The client-side switch, reported but never written.
const USE_SHADERS_CVAR: &std::ffi::CStr = c"gl_use_shaders";

static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
/// `sv_allow_shaders`, once looked up. Engine cvars live for the whole
/// process, so one lookup is enough.
static TARGET: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
static LOOKED_UP: AtomicBool = AtomicBool::new(false);
/// This switch's value as last seen, so a change is logged once.
static WANTED: AtomicBool = AtomicBool::new(false);
/// Set when the engine's 1 is one this module wrote, so turning the switch
/// off puts back only what it changed.
static OURS: AtomicBool = AtomicBool::new(false);
/// How many times the engine's 0 was raised -- roughly once per demo load.
static RAISES: AtomicU32 = AtomicU32::new(0);

/// Called by `commands.rs` once `dodstudio_allow_shaders` is registered.
pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

/// `sv_allow_shaders`, or `None` on an engine without it. Looks it up once,
/// and checks the name it got back before trusting the pointer.
fn target(engfuncs: &ClEngineFuncsPartial) -> Option<*mut CvarSPartial> {
    if !LOOKED_UP.swap(true, Ordering::AcqRel) {
        let found = unsafe { (engfuncs.pfn_get_cvar_pointer)(ENGINE_CVAR.as_ptr()) };
        let name_matches = !found.is_null()
            && unsafe { (*found).name_str() }.is_some_and(|n| n == "sv_allow_shaders");
        let report = if name_matches {
            TARGET.store(found, Ordering::Release);
            "world_shaders: found sv_allow_shaders (25th Anniversary engine)".to_string()
        } else if found.is_null() {
            "world_shaders: this engine has no sv_allow_shaders -- the pre-Anniversary build has no world shaders, so dodstudio_allow_shaders does nothing".to_string()
        } else {
            "world_shaders: pfnGetCvarPointer returned a cvar with the wrong name -- not touching it".to_string()
        };
        unsafe { crate::debug::report(&report) };
    }
    let ptr = TARGET.load(Ordering::Acquire);
    (!ptr.is_null()).then_some(ptr)
}

fn playing_back(engfuncs: &ClEngineFuncsPartial) -> bool {
    let api = engfuncs.p_demo_api;
    !api.is_null() && unsafe { ((*api).is_playingback)() } != 0
}

fn use_shaders(engfuncs: &ClEngineFuncsPartial) -> f32 {
    unsafe { (engfuncs.pfn_get_cvar_float)(USE_SHADERS_CVAR.as_ptr()) }
}

/// Called every frame from `commands::poll`.
pub fn poll() {
    let cvar = CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        return;
    }
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    let wanted = unsafe { (*cvar).value } != 0.0;

    if wanted != WANTED.swap(wanted, Ordering::Relaxed) {
        unsafe {
            crate::debug::report(&format!(
                "world_shaders: {NAME} = {}",
                if wanted { "1" } else { "0" }
            ))
        };
        if wanted && use_shaders(engfuncs) == 0.0 {
            crate::commands::console_print(&format!(
                "{NAME}: gl_use_shaders is 0, so the world still won't use shaders -- set gl_use_shaders 1 as well\n"
            ));
        }
    }

    if !wanted {
        if OURS.swap(false, Ordering::Relaxed)
            && let Some(t) = target(engfuncs)
            && unsafe { (*t).value } == 1.0
        {
            unsafe { (*t).value = 0.0 };
        }
        return;
    }

    // Never override a live server's own answer.
    if !playing_back(engfuncs) {
        return;
    }
    let Some(t) = target(engfuncs) else {
        return;
    };
    if unsafe { (*t).value } == 0.0 {
        unsafe { (*t).value = 1.0 };
        OURS.store(true, Ordering::Relaxed);
        let n = RAISES.fetch_add(1, Ordering::Relaxed) + 1;
        unsafe {
            crate::debug::report(&format!(
                "world_shaders: sv_allow_shaders was 0 during playback, set to 1 ({n} time(s) so far)"
            ))
        };
    }
}

/// A `dodstudio_debug_status` line, or `None` while off and never used.
pub fn status_line() -> Option<String> {
    let cvar = CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        return None;
    }
    let wanted = unsafe { (*cvar).value } != 0.0;
    let raises = RAISES.load(Ordering::Relaxed);
    if !wanted && raises == 0 {
        return None;
    }
    let engfuncs = engine::engfuncs()?;
    let engine_side = match target(engfuncs) {
        Some(t) => format!(
            "sv_allow_shaders value {} (the console shows the string, which stays \"0\"), raised {raises} time(s)",
            unsafe { (*t).value }
        ),
        None => "no sv_allow_shaders on this engine (pre-Anniversary)".to_string(),
    };
    Some(format!(
        "{NAME} = {} -- {engine_side}; gl_use_shaders {}; demo playing: {}",
        if wanted { "1" } else { "0" },
        use_shaders(engfuncs),
        if playing_back(engfuncs) { "yes" } else { "no" }
    ))
}
