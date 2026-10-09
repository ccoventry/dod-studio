//! `dodstudio_debug_position`: where the view and the watched player are,
//! for checking an overview's reach in the game (#581).
//!
//! The stock `spec_pos` prints the view's position, but only while the map
//! has no overview loaded: once one exists it prints the overview camera's
//! instead (seen live, 2026-10-02), which is exactly when the reach checks
//! need it. This prints:
//!
//! - **the eye**: the view origin and angles `V_CalcRefdef` produced on the
//!   last frame, the same numbers `spec_pos` gives without an overview;
//! - **the player**: in a spectator's in-eye view the player being watched,
//!   otherwise the local player, with the entity's origin and where its feet
//!   are (the origin is the hull's middle: 36 units above the feet standing,
//!   18 crouched or prone, which uses the crouch hull);
//! - **the map**.
//!
//! The line also goes to the hook log, so a scripted test can read it back.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::commands::console_print;
use crate::engine::{self, RefParamsPartial};
use crate::names::console_name;

pub const NAME: &str = console_name!("debug_position");

/// The last frame's view origin and angles, as f32 bits.
static VIEW: [AtomicU32; 6] = [const { AtomicU32::new(0) }; 6];
static HAVE_VIEW: AtomicBool = AtomicBool::new(false);

/// Half the standing hull's height: the origin's height above the feet.
const STANDING_HALF: f32 = 36.0;
/// The same for the crouch hull, which prone uses too.
const CROUCHED_HALF: f32 = 18.0;
/// `usehull` for the crouch hull.
const HULL_CROUCHED: i32 = 1;

/// Called after `V_CalcRefdef`, with the view it just set.
pub fn record(params: &RefParamsPartial) {
    let values = [
        params.vieworg[0],
        params.vieworg[1],
        params.vieworg[2],
        params.viewangles[0],
        params.viewangles[1],
        params.viewangles[2],
    ];
    for (slot, value) in VIEW.iter().zip(values) {
        slot.store(value.to_bits(), Ordering::Relaxed);
    }
    HAVE_VIEW.store(true, Ordering::Relaxed);
}

fn view() -> Option<([f32; 3], [f32; 3])> {
    if !HAVE_VIEW.load(Ordering::Relaxed) {
        return None;
    }
    let v: Vec<f32> = VIEW
        .iter()
        .map(|a| f32::from_bits(a.load(Ordering::Relaxed)))
        .collect();
    Some(([v[0], v[1], v[2]], [v[3], v[4], v[5]]))
}

/// Where the feet are for an entity at `origin_z` using hull `usehull`, and
/// what that hull is called.
pub fn feet(origin_z: f32, usehull: i32) -> (f32, &'static str) {
    if usehull == HULL_CROUCHED {
        (origin_z - CROUCHED_HALF, "crouched or prone")
    } else {
        (origin_z - STANDING_HALF, "standing")
    }
}

/// The one line the command prints.
pub fn describe(
    eye: Option<([f32; 3], [f32; 3])>,
    player: Option<(i32, [f32; 3], i32)>,
    map: Option<&str>,
) -> String {
    let mut parts = Vec::new();
    match eye {
        Some((o, a)) => parts.push(format!(
            "eye {:.1} {:.1} {:.1}, pitch {:.1} yaw {:.1}",
            o[0], o[1], o[2], a[0], a[1]
        )),
        None => parts.push("no view drawn yet".to_string()),
    }
    if let Some((index, o, usehull)) = player {
        let (feet_z, hull) = feet(o[2], usehull);
        parts.push(format!(
            "player {index} at {:.1} {:.1} {:.1} (feet at {:.1}, {hull})",
            o[0], o[1], o[2], feet_z
        ));
    }
    if let Some(map) = map {
        parts.push(format!("map {map}"));
    }
    format!("{NAME}: {}", parts.join("; "))
}

/// The player in view: the one watched in eye, else the local player.
/// `(index, origin, usehull)`.
fn player() -> Option<(i32, [f32; 3], i32)> {
    let engfuncs = engine::engfuncs()?;
    // Safety: engine-owned entities, valid for the frame; null-checked.
    unsafe {
        let ent = match crate::spectator_target::in_eye_target() {
            Some(index) => (engfuncs.get_entity_by_index)(index),
            None => (engfuncs.get_local_player)().cast::<engine::ClEntityS>(),
        };
        if ent.is_null() {
            return None;
        }
        let e = &*ent;
        Some((
            e.index,
            [e.origin.x, e.origin.y, e.origin.z],
            e.curstate.usehull,
        ))
    }
}

fn map() -> Option<String> {
    let engfuncs = engine::engfuncs()?;
    // Safety: a pointer into the engine's client state; null-checked.
    let raw = unsafe { (engfuncs.pfn_get_level_name)() };
    if raw.is_null() {
        return None;
    }
    let name = unsafe { std::ffi::CStr::from_ptr(raw) }.to_string_lossy();
    let name = name.trim_start_matches("maps/").trim_end_matches(".bsp");
    (!name.is_empty()).then(|| name.to_string())
}

/// `dodstudio_debug_position`.
pub unsafe extern "C" fn command() {
    let line = describe(view(), player(), map().as_deref());
    console_print(&format!("{line}\n"));
    unsafe { crate::debug::report(&line) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_feet_are_below_the_hull_middle() {
        assert_eq!(feet(100.0, 0), (64.0, "standing"));
        assert_eq!(feet(100.0, 1), (82.0, "crouched or prone"));
    }

    #[test]
    fn says_everything_it_knows() {
        let line = describe(
            Some(([1.0, 2.5, 60.0], [10.0, 270.0, 0.0])),
            Some((3, [1.0, 2.5, 38.0], 0)),
            Some("dod_anzio"),
        );
        assert_eq!(
            line,
            "dodstudio_debug_position: eye 1.0 2.5 60.0, pitch 10.0 yaw 270.0; player 3 at 1.0 2.5 38.0 (feet at 2.0, standing); map dod_anzio"
        );
        assert_eq!(
            describe(None, None, None),
            "dodstudio_debug_position: no view drawn yet"
        );
    }
}
