//! The chase cameras of an HLTV demo draw the players and models around them
//! (issue #918).
//!
//! ## What hid them
//!
//! In locked or free chase the watched player, everyone else and most models
//! went missing while the world still drew, and came back in first person.
//! Stock: HLAE alone, with no hook DLL, loses them too (PRE, `dod_clinic`,
//! chase after free roaming).
//!
//! In an HLTV demo both engine builds drop every entity outside the PVS of
//! one point, `cl.simorg`, not the camera's:
//!
//! - `CL_EmitEntities`, before it links the frame's entities, marks the leaves
//!   visible from `cl.simorg` while an HLTV demo plays (POST `hw+0x19d764`:
//!   `Mod_PointInLeaf`, `Mod_LeafPVS`, then the marking at `hw+0x1e73f0`), and
//!   re-marks the camera's leaves once they are linked (`hw+0x19d82f`).
//! - `CL_AddVisibleEntity` (PRE `hw+0xbae0`, POST `hw+0x1969b0`) asks the
//!   client's `HUD_AddEntity` first, then drops any entity whose box is in no
//!   marked leaf. So the watched player reaches `HUD_AddEntity` and is never
//!   drawn: measured, his `StudioDrawPlayer` count is 0 while `HUD_AddEntity`
//!   sees him every frame.
//!
//! The engine copies `ref_params_s::simorg` back into `cl.simorg` after
//! `V_CalcRefdef` returns (POST `hw+0x231df2`), and DoD's `V_CalcRefdef` is
//! what keeps it on the camera: in first person `V_GetInEyePos` writes the
//! watched player's eye there, and free roaming flies the camera from it. The
//! two chase modes leave it alone, so it stays wherever it was last put: the
//! map's origin after first person (logged as `0 0 0` on every chase frame),
//! or the free-roaming camera's last spot. What is drawn is then what can be
//! seen from there. So how much goes missing depends on the map and the
//! route into chase: on `dod_harrington` nearly every player, from first
//! person or not; on `dod_clinic` nobody after first person, probably because
//! its origin is inside a wall and the engine sees everything from a solid
//! leaf, but players after free roaming.
//!
//! ## What this does
//!
//! [`after_calc_refdef`] runs after the client's own `V_CalcRefdef`: in a
//! chase mode of an HLTV demo it writes the camera's position into `simorg`,
//! so the entities are culled from where the camera is, as the world already
//! is. No code is patched and `hw.dll` is not touched, so there is nothing per
//! build. Only the main view writes it: a picture-in-picture inset renders
//! second, and its own camera is not the one the frame is cut from.
//!
//! First person and free roaming already keep `simorg` on the camera, and a
//! POV demo or a game joined as a player is not spectate-only, so all of
//! those are left alone. On by default, since it only restores what the camera can see;
//! `GOLDSRC_HOOKS_CHASE_PVS=0` turns it off for a session.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::engine::{self, RefParamsPartial};

/// Whether to act at all -- `GOLDSRC_HOOKS_CHASE_PVS=0` turns it off.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

/// Whether the first chase frame has been reported, so it is said once.
static REPORTED: AtomicBool = AtomicBool::new(false);

/// `g_iUser1` for the two chase cameras.
const OBS_CHASE_LOCKED: i32 = 1;
const OBS_CHASE_FREE: i32 = 2;

/// Whether this view's `simorg` should be the camera: the main view of a
/// chase camera in an HLTV demo.
fn culls_from_camera(spectate_only: bool, mode: Option<i32>, main_view: bool) -> bool {
    spectate_only && main_view && matches!(mode, Some(OBS_CHASE_LOCKED | OBS_CHASE_FREE))
}

/// Whether this call of `V_CalcRefdef` is for the main view: the engine
/// passes `nextView` 0 for it and 1 for an inset the client asked for. Read
/// before the client's own `V_CalcRefdef`, which rewrites it.
///
/// Safety: `pparams` is the engine's `ref_params_s` for this view, or null.
pub unsafe fn is_main_view(pparams: *const RefParamsPartial) -> bool {
    !pparams.is_null() && unsafe { (*pparams).next_view } == 0
}

/// Runs after `client.dll`'s own `V_CalcRefdef`, with the same `pparams` and
/// what [`is_main_view`] said before it ran.
///
/// Safety: `pparams` is the engine's `ref_params_s` for this view, valid for
/// the call, or null.
pub unsafe fn after_calc_refdef(pparams: *mut RefParamsPartial, main_view: bool) {
    if pparams.is_null() || !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    // Safety: the engine's own accessor, called on its thread.
    let spectate_only = unsafe { (engfuncs.is_spectate_only)() } != 0;
    if !culls_from_camera(spectate_only, crate::spectator_target::mode(), main_view) {
        return;
    }
    let params = unsafe { &mut *pparams };
    params.simorg = params.vieworg;
    if !REPORTED.swap(true, Ordering::Relaxed) {
        unsafe {
            crate::debug::report(
                "spectator_chase_pvs: chase camera in an HLTV demo -- players and models are culled from the camera, not the map's origin (#918)",
            )
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_main_view_of_a_chase_camera_in_an_hltv_demo() {
        assert!(culls_from_camera(true, Some(OBS_CHASE_LOCKED), true));
        assert!(culls_from_camera(true, Some(OBS_CHASE_FREE), true));
        // First person and free roaming keep simorg on the camera themselves.
        for mode in [None, Some(0), Some(3), Some(4)] {
            assert!(!culls_from_camera(true, mode, true), "{mode:?}");
        }
        // A picture-in-picture inset is not the camera the frame is cut from.
        assert!(!culls_from_camera(true, Some(OBS_CHASE_LOCKED), false));
        // A POV demo or a live game: not spectate-only.
        assert!(!culls_from_camera(false, Some(OBS_CHASE_LOCKED), true));
    }
}
