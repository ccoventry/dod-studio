//! A `brightness`, `gamma` or `lightgamma` change rebuilds the map's
//! lightmaps on the pre-Anniversary engine, as the 25th Anniversary engine
//! does by itself (#662).
//!
//! ## The gap
//!
//! `V_CheckGamma` (pre-Anniversary `hw.dll+0xc19b0`) runs at the start of
//! every screen update. When one of the gamma cvars changed it rebuilds the
//! gamma tables, then calls a "flush" (+0x4703e) that is a lone `ret`. So the
//! table `R_BuildLightMap` reads per luxel follows the cvars, but nothing
//! rebuilds the lightmaps that were made from the old one: the world keeps
//! the lighting it loaded with until the next map, while player models,
//! which are lit every frame, follow the change at once. That mismatch is
//! #662's "models brighter than the world". `lightmap_gamma` (#365) only
//! makes the map load use the current table.
//!
//! The 25th Anniversary `hw.dll` points that flush at `GL_BuildLightmaps`
//! (`jmp` thunk +0x245a70 → +0x246f80), and its `GL_BuildLightmaps` builds a
//! surface's display list only while the surface has none (`fa->polys`,
//! +0x24, is null), so a rebuild redoes the lightmaps alone.
//!
//! ## The fix: the same two changes, on pre-Anniversary
//!
//! 1. `V_CheckGamma`'s call to the flush goes to [`rebuild_lightmaps`]
//!    instead, which calls `GL_BuildLightmaps` while a map's world model is
//!    loaded. At the main menu there is nothing to rebuild (the next map load
//!    builds its lightmaps from the new table anyway), and a model left over
//!    from the previous map may be half-unloaded, which `GL_BuildLightmaps`
//!    would reload from disk through `CL_GetModelByIndex`.
//! 2. `GL_BuildLightmaps`' call to `BuildSurfaceDisplayList` (+0x497b0) goes
//!    to [`build_display_list_once`], which skips a surface that already has
//!    one. The stock function `Hunk_Alloc`s a new polygon and links it in
//!    front of the old one (`poly->next = fa->polys`) every time it runs, so
//!    without this each rebuild would grow the map's hunk use and every
//!    surface's polygon chain. A fresh map's surfaces come out of zeroed hunk
//!    memory, so the map load still builds every display list. This also
//!    fixes the engine's own rebuild when `sv_cheats 0` clamps `lightgamma`
//!    (+0x45626), which chained a second polygon onto every surface.
//!
//! Both are a `call rel32` retargeted through [`hw_call_site`], whose
//! signatures pin the stock `rel32`, so a build whose call goes anywhere else
//! doesn't match. The display-list guard goes in first, and the flush only
//! once the guard is in.
//!
//! With `lightmap_gamma`'s detour also in, a map load that finds the tables
//! stale rebuilds them from `GL_BuildLightmaps`' entry, and their flush then
//! builds the lightmaps once from inside that call before the outer build
//! does it again: one extra build, only at that load, and the guard keeps
//! the display lists single.
//!
//! Textures are not rebuilt: a `texgamma` change still needs a map reload, on
//! both builds.
//!
//! On by default, since it makes the pre-Anniversary engine do what the 25th
//! Anniversary one does; `GOLDSRC_HOOKS_LIGHTMAP_REBUILD=0` turns it off. The
//! signatures match nothing in the Anniversary `hw.dll`, which logs "not
//! installed". `tools/verify_lightmap_rebuild_offsets.py` checks every fact
//! above against both builds (`--anniversary`).

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::hw_call_site::{Build, CallSite, call_bytes};
use crate::scan;

/// `V_CheckGamma`'s call to the empty flush, after it stores the cvar values
/// it compares against next time; then `mov eax, 1` and the store before its
/// `ret`. The `rel32` (`F9 55 F8 FF`) pins the target, +0x4703e.
const FLUSH_BUILDS: [Build; 1] = [Build {
    name: "pre-Anniversary",
    pattern: "89 15 ?? ?? ?? ?? A3 ?? ?? ?? ?? 89 0D ?? ?? ?? ?? E8 F9 55 F8 FF \
              B8 01 00 00 00 A3 ?? ?? ?? ?? C3",
    at: 17,
}];

/// `GL_BuildLightmaps`' call to `BuildSurfaceDisplayList`, behind the
/// `SURF_DRAWTURB` (0x10) test, then the loop's step to the next 0x5c-byte
/// surface. The `rel32` (`1C FB FF FF`) pins the target, +0x497b0.
const DISPLAY_LIST_BUILDS: [Build; 1] = [Build {
    name: "pre-Anniversary",
    pattern: "8D 04 17 F6 C1 10 75 09 50 E8 1C FB FF FF 83 C4 04 8B 86 B0 00 00 00 \
              43 83 C7 5C",
    at: 9,
}];

/// `msurface_t.polys`, the display list `BuildSurfaceDisplayList` links its
/// polygon into, in both builds.
const SURFACE_POLYS_AT: usize = 0x24;

static FLUSH: CallSite = CallSite::new("V_CheckGamma's lightmap flush", &FLUSH_BUILDS);
static DISPLAY_LIST: CallSite = CallSite::new(
    "GL_BuildLightmaps' call to BuildSurfaceDisplayList",
    &DISPLAY_LIST_BUILDS,
);

/// Whether to install at all -- `GOLDSRC_HOOKS_LIGHTMAP_REBUILD=0` turns it off.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

/// `GL_BuildLightmaps`, which [`rebuild_lightmaps`] calls.
static BUILD_LIGHTMAPS: AtomicUsize = AtomicUsize::new(0);
/// The stock `BuildSurfaceDisplayList`, which [`build_display_list_once`] calls.
static BUILD_DISPLAY_LIST: AtomicUsize = AtomicUsize::new(0);

/// Gamma changes that rebuilt the lightmaps, and those that found no map.
static REBUILT: AtomicU32 = AtomicU32::new(0);
static NO_MAP: AtomicU32 = AtomicU32::new(0);
/// The last [`REBUILT`] count written to the log.
static LOGGED: AtomicU32 = AtomicU32::new(0);

static INSTALLED: AtomicBool = AtomicBool::new(false);

type BuildLightmapsFn = unsafe extern "C" fn();
type BuildDisplayListFn = unsafe extern "C" fn(surface: *mut c_void);

/// Patches both calls in the loaded `hw.dll`. Runs before
/// `lightmap_gamma::install`, whose detour overwrites the start of
/// `GL_BuildLightmaps` that the signature here begins with. A second call
/// does nothing.
pub fn install() {
    if !ENABLED.load(Ordering::Relaxed) {
        unsafe {
            crate::debug::report(
                "lightmap_rebuild: off (GOLDSRC_HOOKS_LIGHTMAP_REBUILD=0) -- a gamma change doesn't relight the map on pre-Anniversary (#662)",
            )
        };
        return;
    }
    if INSTALLED.load(Ordering::Acquire) {
        return;
    }
    match install_both() {
        Ok(build) => {
            INSTALLED.store(true, Ordering::Release);
            unsafe {
                crate::debug::report(&format!(
                    "lightmap_rebuild: a gamma change rebuilds the lightmaps (GL_BuildLightmaps +{build:#x}), display lists built once per surface (#662)"
                ))
            };
        }
        Err(why) => unsafe {
            crate::debug::report(&format!("lightmap_rebuild: not installed -- {why}"))
        },
    }
}

/// Finds `GL_BuildLightmaps`, then writes the guard and the flush, in that
/// order. Returns `GL_BuildLightmaps`' offset in `hw.dll`.
fn install_both() -> Result<usize, String> {
    let base = crate::engine::engine_module_base().ok_or("hw.dll is not loaded yet")?;
    // Safety: `base` is the loaded hw.dll.
    let build = unsafe { scan::find_unique(base, crate::lightmap_gamma::BUILD_PATTERN) }
        .map_err(|e| format!("GL_BuildLightmaps: {e}"))?;
    let display_list_at = DISPLAY_LIST.resolve()?;
    let flush_at = FLUSH.resolve()?;
    let stock_display_list = DISPLAY_LIST
        .original_target()
        .ok_or("BuildSurfaceDisplayList's call was not read")?;
    BUILD_LIGHTMAPS.store(build, Ordering::Release);
    BUILD_DISPLAY_LIST.store(stock_display_list, Ordering::Release);
    let guard = build_display_list_once as BuildDisplayListFn as usize;
    DISPLAY_LIST.put(true, call_bytes(display_list_at, guard))?;
    let flush = rebuild_lightmaps as BuildLightmapsFn as usize;
    FLUSH.put(true, call_bytes(flush_at, flush))?;
    Ok(build - base)
}

/// `V_CheckGamma`'s flush: rebuilds every lightmap from the new table while
/// a map is loaded. Runs on the engine's thread, from the screen update.
unsafe extern "C" fn rebuild_lightmaps() {
    if !world_loaded() {
        NO_MAP.fetch_add(1, Ordering::Relaxed);
        return;
    }
    let build = BUILD_LIGHTMAPS.load(Ordering::Acquire);
    if build == 0 {
        return;
    }
    REBUILT.fetch_add(1, Ordering::Relaxed);
    // Safety: `GL_BuildLightmaps`, found by its unique signature; cdecl with
    // no arguments (its callers push none, checked by the verify script).
    unsafe { std::mem::transmute::<usize, BuildLightmapsFn>(build)() };
}

/// `BuildSurfaceDisplayList`, for a surface that has no display list yet.
unsafe extern "C" fn build_display_list_once(surface: *mut c_void) {
    // Safety: the engine passes a surface of the model it is walking, which
    // holds `polys` at this offset (checked by the verify script).
    let polys =
        unsafe { std::ptr::read_unaligned((surface as usize + SURFACE_POLYS_AT) as *const usize) };
    if polys != 0 {
        return;
    }
    let stock = BUILD_DISPLAY_LIST.load(Ordering::Acquire);
    // Safety: the stock target of the call this replaced, with its argument.
    unsafe { std::mem::transmute::<usize, BuildDisplayListFn>(stock)(surface) };
}

/// Whether the current map's world model is in place and loaded.
fn world_loaded() -> bool {
    let Some(engfuncs) = crate::engine::engfuncs() else {
        return false;
    };
    // Safety: the engine's own accessor, bounds-checked by the engine.
    let entity = unsafe { (engfuncs.get_entity_by_index)(0) };
    if entity.is_null() {
        return false;
    }
    // Safety: a cl_entity_t the engine owns; `model` is the SDK layout.
    let model = unsafe { (*entity).model } as usize;
    model != 0 && crate::map_light::world::model_loaded(model)
}

/// Logs the lightmap rebuilds, the first and then every tenfold. Runs every
/// frame from `commands::poll`.
pub fn poll() {
    let now = REBUILT.load(Ordering::Relaxed);
    let logged = LOGGED.load(Ordering::Relaxed);
    if now == logged || !crate::tempent_fix::worth_logging(logged, now) {
        return;
    }
    LOGGED.store(now, Ordering::Relaxed);
    unsafe {
        crate::debug::report(&format!(
            "lightmap_rebuild: a gamma change rebuilt the lightmaps ({now} time(s) so far) (#662)"
        ))
    };
}

/// The `ds_debug_status` line, or `None` when it was never installed.
pub fn status_line() -> Option<String> {
    INSTALLED.load(Ordering::Relaxed).then(|| {
        format!(
            "gamma relights the map: on, {} rebuild(s), {} change(s) with no map loaded",
            REBUILT.load(Ordering::Relaxed),
            NO_MAP.load(Ordering::Relaxed)
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_signatures_pin_a_call() {
        for builds in [&FLUSH_BUILDS, &DISPLAY_LIST_BUILDS] {
            crate::hw_call_site::check_builds(builds);
        }
    }

    #[test]
    fn the_pinned_calls_go_where_the_comments_say() {
        // At their pre-Anniversary addresses, +0xc1a40 and +0x49c8f.
        for (builds, from, to) in [
            (&FLUSH_BUILDS, 0xc1a40, 0x4703e),
            (&DISPLAY_LIST_BUILDS, 0x49c8f, 0x497b0),
        ] {
            let build = &builds[0];
            let bytes: Vec<u8> = build
                .pattern
                .split_whitespace()
                .skip(build.at)
                .take(crate::hw_call_site::CALL_LEN)
                .map(|t| u8::from_str_radix(t, 16).expect("a pinned byte"))
                .collect();
            let bytes: [u8; 5] = bytes.try_into().expect("five bytes");
            assert_eq!(crate::hw_call_site::call_target(from, &bytes), to);
        }
    }
}
