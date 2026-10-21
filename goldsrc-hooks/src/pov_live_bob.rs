//! `ds_view_use_cl_bob`: a POV demo bobs the view and the gun by the `cl_bob`,
//! `cl_bobcycle` and `cl_bobup` set now (#359). Off (0), a POV demo shows the
//! bob it recorded, the recording player's own, whatever `cl_bob` says here:
//! about 13 of 20 POV demos from different recorders carry one, the rest
//! were recorded with none.
//!
//! ## What the engine does
//!
//! `client.dll`'s `V_CalcRefdef` works the bob out live in every demo: the
//! view moves sideways by `bob` along `right`, and the gun by `0.4 * bob`
//! along `forward` and `bob` up, where `bob` comes from `cl_bob`,
//! `cl_bobcycle`, `cl_bobup` and the player's speed (`V_CalcBob`). Logged in
//! a POV demo, its output followed `cl_bob` (0.01 against 0.1: a sideways
//! offset of 1-2 units against up to the -7/+4 clamp).
//!
//! But in POV playback `V_RenderView` throws that away: right after
//! `ClientDLL_CalcRefdef` it calls the demo view override (pre-Anniversary
//! `hw.dll+0xd75d`, 25th Anniversary `+0x198e90`; `playdemo` and `viewdemo`
//! alike), which copies the recorded `ref_params_s` over the live one and
//! puts the gun at the recorded position. Both carry the recorder's bob, not
//! ours: on wsod25_grp2_h1_m00cat, recorded with no bob, the recorded
//! `vieworg` sits exactly at `simorg + viewheight + 1/32` on every frame, and
//! the recorded gun height never moves. So changing `cl_bob` does nothing. HLTV demos skip the
//! override (the engine's spectator flag), which is why the cvar works there.
//!
//! ## The change
//!
//! With this on, the call to the override goes through [`after_override`]:
//! once the recorded view is in place, the recorded bob is measured from it
//! (the sideways part of `vieworg - simorg - viewheight`, which is all
//! `V_CalcRefdef` adds there besides a fixed 1/32 and screen shake; zero in
//! the demos measured) and replaced by the live one [`after_calc_refdef`]
//! measured the same way a moment earlier. The camera and the gun move by the
//! difference, so the recorded path, its smoothing and everything else in the
//! view stay as recorded. The gun's bob tilt stays out: the override sets the
//! gun's angles from the view angles alone, so a POV demo has never shown it.
//!
//! The call site is found by a byte pattern per build, each unique in its own
//! `hw.dll`, and redirected only once the setting is first turned on;
//! `tools/verify_pov_live_bob_offsets.py` re-derives both.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize, Ordering};

use crate::engine::{self, CvarSPartial, RefParamsPartial};
use crate::names::console_name;
use crate::scan;

pub const NAME: &str = console_name!("view_use_cl_bob");

/// One build's call to the demo view override in `V_RenderView`.
struct Site {
    build: &'static str,
    pattern: &'static str,
    /// Offset of the `E8 rel32` call.
    call_at: usize,
}

#[rustfmt::skip]
const SITES: &[Site] = &[
    Site {
        build: "pre-Anniversary",
        //         push viewmodel  lea ecx,[ebp-F4]   push viewent.origin push ecx call            fld [ebp-E8]
        pattern: "68 ?? ?? ?? ?? 8D 8D 0C FF FF FF 68 ?? ?? ?? ?? 51 E8 ?? ?? ?? ?? D9 85 18 FF FF FF",
        call_at: 17,
    },
    Site {
        build: "25th Anniversary",
        //         push viewmodel  lea eax,[ebp-EC]   push viewent.origin push eax call            movss xmm3,[ebp-E0]
        pattern: "68 ?? ?? ?? ?? 8D 85 14 FF FF FF 68 ?? ?? ?? ?? 50 E8 ?? ?? ?? ?? F3 0F 10 9D 20 FF FF FF",
        call_at: 17,
    },
];

/// `ref_params_s` (`common/ref_params.h`) up to `viewheight`.
#[repr(C)]
struct View {
    vieworg: [f32; 3],
    viewangles: [f32; 3],
    forward: [f32; 3],
    right: [f32; 3],
    up: [f32; 3],
    frametime: f32,
    time: f32,
    intermission: i32,
    paused: i32,
    spectator: i32,
    onground: i32,
    waterlevel: i32,
    simvel: [f32; 3],
    simorg: [f32; 3],
    viewheight: [f32; 3],
}

/// `V_CalcRefdef` adds this to every axis of `vieworg`.
const NUDGE: f32 = 1.0 / 32.0;

type OverrideFn = unsafe extern "C" fn(*mut View, *mut [f32; 3], *mut i32);

static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
/// The override's own address, once the call is redirected.
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static GAVE_UP: AtomicBool = AtomicBool::new(false);
/// The bob the live `V_CalcRefdef` worked out this frame, as f32 bits.
static LIVE_BOB: AtomicU32 = AtomicU32::new(0);
static HAVE_LIVE: AtomicBool = AtomicBool::new(false);

pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

fn on() -> bool {
    let cvar = CVAR.load(Ordering::Acquire);
    // Safety: the engine owns the cvar for the session.
    !cvar.is_null() && unsafe { (*cvar).value } != 0.0
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The sideways offset `V_CalcRefdef` put into `vieworg`: its bob.
fn bob_of(view: &View) -> f32 {
    let side = [0, 1, 2].map(|i| view.vieworg[i] - view.simorg[i] - view.viewheight[i] - NUDGE);
    dot(side, view.right)
}

/// What [`after_override`] adds to the recorded view and gun for a live bob
/// of `live`: (camera, gun) offsets.
fn shift(recorded: &View, live: f32) -> ([f32; 3], [f32; 3]) {
    let delta = live - bob_of(recorded);
    let camera = recorded.right.map(|r| r * delta);
    let mut gun = recorded.forward.map(|f| f * delta * 0.4);
    gun[2] += delta;
    (camera, gun)
}

/// Called by `engine.rs` right after `client.dll`'s `V_CalcRefdef`, with the
/// view it worked out live.
pub fn after_calc_refdef(params: *const RefParamsPartial) {
    if params.is_null() || ORIGINAL.load(Ordering::Acquire) == 0 {
        return;
    }
    // Safety: the engine's ref_params_s for this frame, which is at least
    // `View` long.
    let view = unsafe { &*(params as *const View) };
    LIVE_BOB.store(bob_of(view).to_bits(), Ordering::Relaxed);
    HAVE_LIVE.store(true, Ordering::Relaxed);
}

/// Stands in for the engine's demo view override at its one call site.
unsafe extern "C" fn after_override(view: *mut View, gun: *mut [f32; 3], viewmodel: *mut i32) {
    let original = ORIGINAL.load(Ordering::Acquire);
    // Safety: the address the call went to before it was redirected.
    let original: OverrideFn = unsafe { std::mem::transmute(original) };
    unsafe { original(view, gun, viewmodel) };
    if view.is_null() || gun.is_null() || !on() || !HAVE_LIVE.swap(false, Ordering::Relaxed) {
        return;
    }
    let live = f32::from_bits(LIVE_BOB.load(Ordering::Relaxed));
    // Safety: the engine's ref_params_s and the gun entity's origin, both
    // live for this call.
    unsafe {
        let (camera, gun_shift) = shift(&*view, live);
        for i in 0..3 {
            (*view).vieworg[i] += camera[i];
            (*gun)[i] += gun_shift[i];
        }
    }
}

fn redirect() -> Result<&'static str, String> {
    let base = engine::engine_module_base().ok_or("hw.dll is not loaded yet")?;
    let mut errors = Vec::new();
    for site in SITES {
        // Safety: `base` is a module handle the loader gave us.
        let found = match unsafe { scan::find_unique(base, site.pattern) } {
            Ok(found) => found,
            Err(why) => {
                errors.push(format!("{}: {why}", site.build));
                continue;
            }
        };
        let call = found + site.call_at;
        // Safety: the scan matched an `E8 rel32` here.
        let rel = unsafe { std::ptr::read_unaligned((call + 1) as *const i32) };
        let original = (call as isize + 5 + rel as isize) as usize;
        let ours = after_override as *const () as usize;
        let new_rel = (ours as isize - (call as isize + 5)) as i32;
        ORIGINAL.store(original, Ordering::Release);
        if !unsafe { crate::patch::write_code_bytes(call + 1, &new_rel.to_le_bytes()) } {
            ORIGINAL.store(0, Ordering::Release);
            return Err("could not make the demo view override's call writable".into());
        }
        return Ok(site.build);
    }
    Err(format!(
        "the demo view override's call wasn't found ({})",
        errors.join("; ")
    ))
}

/// Redirects the call the first time the setting is on. Every frame.
pub fn poll() {
    if ORIGINAL.load(Ordering::Acquire) != 0 || GAVE_UP.load(Ordering::Relaxed) || !on() {
        return;
    }
    let line = match redirect() {
        Ok(build) => format!("{NAME}: on ({build} hw.dll)"),
        Err(why) => {
            GAVE_UP.store(true, Ordering::Relaxed);
            format!("{NAME}: {why}")
        }
    };
    unsafe { crate::debug::report(&line) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(vieworg: [f32; 3], simorg: [f32; 3]) -> View {
        View {
            vieworg,
            viewangles: [0.0; 3],
            forward: [1.0, 0.0, 0.0],
            right: [0.0, -1.0, 0.0],
            up: [0.0, 0.0, 1.0],
            frametime: 0.0,
            time: 0.0,
            intermission: 0,
            paused: 0,
            spectator: 0,
            onground: 1,
            waterlevel: 0,
            simvel: [0.0; 3],
            simorg,
            viewheight: [0.0, 0.0, 22.0],
        }
    }

    #[test]
    fn the_bob_is_the_sideways_part_of_the_eye_offset() {
        // Moved 2 units along `right` (-y), plus the fixed nudge.
        let v = view([NUDGE, -2.0 + NUDGE, 22.0 + NUDGE], [0.0; 3]);
        assert!((bob_of(&v) - 2.0).abs() < 1e-5);
    }

    #[test]
    fn the_view_and_gun_move_by_the_live_minus_the_recorded_bob() {
        let recorded = view([NUDGE, -1.0 + NUDGE, 22.0 + NUDGE], [0.0; 3]);
        let (camera, gun) = shift(&recorded, 3.0);
        assert!((camera[1] + 2.0).abs() < 1e-5, "{camera:?}");
        assert!(
            (gun[0] - 0.8).abs() < 1e-5 && (gun[2] - 2.0).abs() < 1e-5,
            "{gun:?}"
        );
        let (camera, gun) = shift(&recorded, 1.0);
        assert!(camera.iter().chain(&gun).all(|v| v.abs() < 1e-5));
    }

    #[test]
    fn each_call_offset_lands_on_the_call() {
        for site in SITES {
            let bytes: Vec<&str> = site.pattern.split(' ').collect();
            assert_eq!(bytes[site.call_at], "E8", "{}", site.build);
            assert!(
                bytes[site.call_at + 1..site.call_at + 5]
                    .iter()
                    .all(|b| *b == "??")
            );
        }
    }
}
