//! `dodstudio_spec_deathcam 1`: when the player being watched in first person
//! in an HLTV demo dies, the view becomes the death cam his own recording
//! shows (issue #709). A switch of its own rather than part of
//! `dodstudio_spec_match_pov`: a movie wants it, a shoutcast following the
//! fight does not.
//!
//! ## What the player sees
//!
//! A POV demo does not stay in the dead player's eyes. From the first dead
//! frame the camera sits 64 units behind a point 17 above his body, pulls
//! back at 96 units a second to at most 168, keeps level (no roll), shows no
//! gun, and turns smoothly toward the killer until the server respawns him
//! (1.8-2.4 s in the demos measured for #709). It is DoD's own death cam,
//! `V_GetDeathCam` (`cl_dll/view.cpp` for the names; the numbers above are
//! read from `client.dll`, which is the same file in both engine builds):
//!
//! ```text
//!     v_lastDistance += v_frametime * 96;  if (v_resetCamera) v_lastDistance = 64;
//!     v_lastDistance = min(v_lastDistance, 168);
//!     head = ent1->origin + (0, 0, 17)              (when ent1 is a player)
//!     target angles = toward ent2, or straight down when ent2 is NULL
//!     V_SmoothInterpolateAngles(v_lastAngles, target, angles, 120);
//!     V_GetChaseOrigin(angles, head, v_lastDistance, origin);   // traces back
//!     v_lastAngles = angles
//! ```
//!
//! `V_CalcRefdef` only reaches it when the chase target is the local player,
//! which a spectator's never is. The in-eye camera's own dead case
//! (`V_GetInEyePos`: roll 80, eye down 8) is what an HLTV viewer gets, and the
//! dropped weapon fills the clip.
//!
//! ## What this does
//!
//! [`after_calc_refdef`] runs after `client.dll`'s own `V_CalcRefdef`. While
//! the camera is in a dead player's eyes and the switch is on, it calls the
//! game's `V_GetDeathCam` with the body and the killer (the last `DeathMsg`
//! naming him as the victim, noted by `deathmsg`'s handler) and writes the
//! result over the view, as `V_GetChasePos` does for the player's own death.
//! The gun is moved out of the picture, since a dead player's own view has
//! none. On the first dead frame `v_resetCamera` is set, so the pull-back
//! starts at 64, and `v_lastAngles` is put back to his last living view, since
//! the frame's own `V_CalcNormalRefdef` has just stored the rolled dead-eye
//! angles there. Once he is solid again nothing is touched: the game's in-eye
//! view is his own, as it is in the POV recording.
//!
//! No code is written: a signature finds `V_GetDeathCam`, and the two globals
//! it needs are read off its own instructions.
//!
//! The death cam lasts [`HOLD_SECONDS`], then the view is the game's own for
//! the dead player again (in his eyes, as without this switch) until he
//! respawns. His own recording leaves the death cam after that long too, for a
//! respawn or an observer view of a teammate (not reproduced); the pull-back
//! itself takes 1.1 s. So with `dodstudio_spec_lock 1` a long reinforcement
//! wait is spent in his eyes, not circling the body.
//!
//! Stands down in a POV demo like the `spec_match_pov` fixes: that recording
//! already has the death cam.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use crate::engine::{self, ClEntityS, CvarSPartial, RefParamsPartial, Vec3};
use crate::names::console_name;
use crate::scan;

/// The cvar: 1 shows the death cam when the watched player dies.
pub const NAME: &str = console_name!("spec_deathcam");

/// How long the death cam shows before the view returns to the dead player's
/// eyes. Measured from POV recordings, where it is constant per recording:
/// 2.43 s on every death in `wsod25_grp2_h1_m00cat` (16 of 19; the rest were
/// instant respawns), 1.81 s in `wsod25_grp2_h1_element` (25 of 28). A server
/// rule, not the client's 4 s spectator switch; the difference between the
/// two players is not explained.
const HOLD_SECONDS: f32 = 2.4;

/// `V_GetDeathCam`'s first 30 bytes: the frame's distance step and the 168
/// cap. Unique in `client.dll`, the same file in both builds.
///
/// ```text
///     83 EC 1C                    sub  esp, 0x1c
///     D9 05 <v_frametime>         fld  dword [v_frametime]
///     D8 0D <96.0>                fmul dword [96.0]
///     A1 <v_resetCamera>          mov  eax, [v_resetCamera]
///     C7 44 24 00 00 00 28 43     mov  dword [esp], 168.0
///     85 C0                       test eax, eax
/// ```
const PATTERN: &str = "83 EC 1C D9 05 ?? ?? ?? ?? D8 0D ?? ?? ?? ?? A1 ?? ?? ?? ?? \
                       C7 44 24 00 00 00 28 43 85 C0";
/// Where `v_resetCamera`'s address sits in the pattern (the `mov eax`
/// operand).
const RESET_CAMERA_OPERAND_AT: usize = 0x10;
/// Later in the same function, `push v_lastAngles` (`68 imm32`) is the first
/// argument of the `V_SmoothInterpolateAngles` call. Past the pattern, so the
/// opcode and the store below are checked before the address is believed.
const LAST_ANGLES_PUSH_AT: usize = 0x10b;
/// `mov [v_lastAngles], edx` (`89 15 imm32`) at the end of the function, which
/// stores the result back. Its operand must name the same address as the
/// push.
const LAST_ANGLES_STORE_AT: usize = 0x12f;
// The store comes after the push, as the code reads.
const _: () = assert!(LAST_ANGLES_STORE_AT > LAST_ANGLES_PUSH_AT + 5);
const PUSH_IMM32: u8 = 0x68;
const MOV_MEM_EDX: [u8; 2] = [0x89, 0x15];

/// `entity_state_s::solid` for a body the engine no longer collides with:
/// what `V_GetInEyePos` tests for a dead player.
const SOLID_NOT: i16 = 0;
/// Player entity indices run 1..=32.
const MAX_PLAYERS: usize = 32;
/// How far below the camera the gun is parked while the death cam shows:
/// well outside any view.
const GUN_OUT_OF_VIEW: f32 = 4096.0;

/// `void V_GetDeathCam(cl_entity_t *ent1, cl_entity_t *ent2, float *angles,
/// float *origin)`, cdecl (the callers pop four arguments).
type GetDeathCamFn = unsafe extern "C" fn(
    ent1: *mut ClEntityS,
    ent2: *mut ClEntityS,
    angles: *mut [f32; 3],
    origin: *mut [f32; 3],
);

static CVAR: std::sync::atomic::AtomicPtr<CvarSPartial> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
/// Where the function and its two globals were found, or 0 before the scan.
static FUNCTION: AtomicUsize = AtomicUsize::new(0);
static RESET_CAMERA: AtomicUsize = AtomicUsize::new(0);
static LAST_ANGLES: AtomicUsize = AtomicUsize::new(0);
/// The module base they were resolved against (see `scoreboard::SCANNED_BASE`).
static SCANNED_BASE: AtomicUsize = AtomicUsize::new(0);
/// Whether a failed scan has been reported, so it is said once.
static COMPLAINED: AtomicBool = AtomicBool::new(false);

/// The spectated player's in-eye view angles on his last living frame, roll
/// 0, as f32 bits. The death cam turns from here toward the killer.
static ALIVE_ANGLES: [AtomicU32; 3] = [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];
/// The dead player the camera is on, or 0.
static DEAD_TARGET: AtomicI32 = AtomicI32::new(0);
/// The `ref_params_s::time` of his first dead frame, as f32 bits.
static DIED_AT: AtomicU32 = AtomicU32::new(0);
/// Whether his death cam has run its time and the view is back in his eyes.
static DONE: AtomicBool = AtomicBool::new(false);
/// The `ref_params_s::time` the death cam was last computed for, as f32 bits,
/// so a second view in one frame (the picture-in-picture inset) reuses the
/// result instead of stepping the distance twice.
static COMPUTED_AT: AtomicU32 = AtomicU32::new(0);
/// The last computed view: angles then origin, as f32 bits.
static RESULT: [AtomicU32; 6] = [
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
];
/// Who last killed each player (by entity index), from `DeathMsg`. 0 is the
/// world, or no death seen yet.
static LAST_KILLER: [AtomicI32; MAX_PLAYERS + 1] = [const { AtomicI32::new(0) }; MAX_PLAYERS + 1];
/// Death cams shown this session, for `dodstudio_debug_status`.
static DEATHS: AtomicU64 = AtomicU64::new(0);

/// Called once the cvar is registered.
pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

/// Whether the switch is on.
pub fn enabled() -> bool {
    let cvar = CVAR.load(Ordering::Acquire);
    // Safety: the engine's own cvar record, which lives for the session.
    !cvar.is_null() && unsafe { (*cvar).value } != 0.0
}

/// Whether the death cam has run its time since `died_at` (or the clock has
/// gone backwards, a seek), so the view returns to the player's eyes.
fn over(now: f32, died_at: f32) -> bool {
    let elapsed = now - died_at;
    !(0.0..=HOLD_SECONDS).contains(&elapsed)
}

/// Notes a kill from the `DeathMsg` handler. Both are player entity indices
/// (killer 0 for the world); anything out of range is ignored.
pub fn note_death(killer: i32, victim: i32) {
    if let Some(slot) = LAST_KILLER.get(victim.max(0) as usize)
        && victim > 0
    {
        slot.store(killer, Ordering::Relaxed);
    }
}

/// The entity to turn toward for `victim`'s death: the killer, unless it was
/// the world or the player himself, which the game shows by looking down.
fn killer_of(victim: i32, killer: i32) -> Option<i32> {
    ((1..=MAX_PLAYERS as i32).contains(&killer) && killer != victim).then_some(killer)
}

/// `V_GetDeathCam` and the two globals it keeps between frames.
struct Game {
    get_death_cam: GetDeathCamFn,
    reset_camera: *mut i32,
    last_angles: *mut [f32; 3],
}

/// Whether four bytes at `address` are inside the loaded `client.dll`.
fn inside_client(base: usize, address: usize) -> bool {
    // Safety: `client_module_base` only returns a base for a mapped module.
    let Some(size) = (unsafe { crate::pe::image_size(base as *mut u8) }) else {
        return false;
    };
    address >= base && address.saturating_add(4) <= base + size
}

fn resolve() -> Result<Game, String> {
    let Some(base) = engine::client_module_base() else {
        return Err("client.dll is not loaded yet".to_string());
    };
    if SCANNED_BASE.load(Ordering::Acquire) != base || FUNCTION.load(Ordering::Acquire) == 0 {
        // Safety: `client_module_base` only returns a base for a mapped
        // module, and it stays mapped for the session.
        let function = unsafe { scan::find_unique(base, PATTERN) }
            .map_err(|why| format!("could not find the game's death cam -- {why}"))?;
        // Safety: the scan proved the pattern's bytes are mapped code, and
        // the two offsets past it are inside the same function, whose shape
        // is checked byte by byte before anything is believed.
        let read_u32 = |at: usize| unsafe { std::ptr::read_unaligned(at as *const u32) } as usize;
        let read_u8 = |at: usize| unsafe { *(at as *const u8) };
        let reset_camera = read_u32(function + RESET_CAMERA_OPERAND_AT);
        if read_u8(function + LAST_ANGLES_PUSH_AT) != PUSH_IMM32
            || [
                read_u8(function + LAST_ANGLES_STORE_AT),
                read_u8(function + LAST_ANGLES_STORE_AT + 1),
            ] != MOV_MEM_EDX
        {
            return Err(
                "the game's death cam does not have the shape expected past its signature"
                    .to_string(),
            );
        }
        let last_angles = read_u32(function + LAST_ANGLES_PUSH_AT + 1);
        if last_angles != read_u32(function + LAST_ANGLES_STORE_AT + 2) {
            return Err(
                "the game's death cam reads and writes different last-angle globals".to_string(),
            );
        }
        if !inside_client(base, reset_camera) || !inside_client(base, last_angles + 8) {
            return Err("the game's death cam's globals are not inside client.dll".to_string());
        }
        FUNCTION.store(function, Ordering::Release);
        RESET_CAMERA.store(reset_camera, Ordering::Release);
        LAST_ANGLES.store(last_angles, Ordering::Release);
        SCANNED_BASE.store(base, Ordering::Release);
    }
    let function = FUNCTION.load(Ordering::Acquire);
    Ok(Game {
        // Safety: `function` is the start of V_GetDeathCam, found by its
        // signature; the type is its cdecl prototype.
        get_death_cam: unsafe { std::mem::transmute::<usize, GetDeathCamFn>(function) },
        reset_camera: RESET_CAMERA.load(Ordering::Acquire) as *mut i32,
        last_angles: LAST_ANGLES.load(Ordering::Acquire) as *mut [f32; 3],
    })
}

/// GoldSrc's `AngleVectors` (`mathlib.c`): pitch, yaw, roll in degrees to the
/// view's forward, right and up.
fn angle_vectors(angles: [f32; 3]) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let [pitch, yaw, roll] = angles.map(f32::to_radians);
    let (sp, cp) = pitch.sin_cos();
    let (sy, cy) = yaw.sin_cos();
    let (sr, cr) = roll.sin_cos();
    let forward = [cp * cy, cp * sy, -sp];
    let right = [-sr * sp * cy + cr * sy, -sr * sp * sy - cr * cy, -sr * cp];
    let up = [cr * sp * cy + sr * sy, cr * sp * sy - sr * cy, cr * cp];
    (forward, right, up)
}

/// Whether the spectated player is dead: his body is no longer solid (the
/// game's own test) or is playing a death animation (an HLTV demo can be late
/// with `solid`).
fn is_dead(player: &ClEntityS) -> bool {
    if player.curstate.solid == SOLID_NOT {
        return true;
    }
    !player.model.is_null()
        && crate::anim_fix::sequence_label(player.model, player.curstate.sequence.max(0) as usize)
            .is_some_and(|label| label.starts_with("die") || label.starts_with("dead"))
}

/// The camera left the dead player (another target, another mode, the switch
/// off): the next death starts afresh.
fn leave() {
    DEAD_TARGET.store(0, Ordering::Relaxed);
}

/// Runs after `client.dll`'s own `V_CalcRefdef`, with the same `pparams`,
/// before the frame's position is recorded.
///
/// Safety: `pparams` is the engine's `ref_params_s` for this view, valid for
/// the call.
pub unsafe fn after_calc_refdef(pparams: *mut RefParamsPartial) {
    if pparams.is_null() {
        return;
    }
    if !enabled() || crate::anim_fix::pov_demo_playing() {
        return leave();
    }
    let Some(target) = crate::spectator_target::in_eye_target() else {
        return leave();
    };
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    let player = unsafe { (engfuncs.get_entity_by_index)(target) };
    if player.is_null() || unsafe { (*player).player } == 0 {
        return leave();
    }
    let params = unsafe { &mut *pparams };
    if !unsafe { is_dead(&*player) } {
        for (slot, value) in
            ALIVE_ANGLES
                .iter()
                .zip([params.viewangles[0], params.viewangles[1], 0.0])
        {
            slot.store(value.to_bits(), Ordering::Relaxed);
        }
        if DEAD_TARGET.swap(0, Ordering::Relaxed) != 0 {
            unsafe {
                crate::debug::report(&format!(
                    "spectator_deathcam: player {target} is alive again -- his own view"
                ))
            };
        }
        return;
    }

    let game = match resolve() {
        Ok(game) => game,
        Err(why) => {
            if !COMPLAINED.swap(true, Ordering::Relaxed) {
                unsafe {
                    crate::debug::report(&format!("spectator_deathcam: not applied -- {why}"))
                };
            }
            return;
        }
    };
    COMPLAINED.store(false, Ordering::Relaxed);

    let killer = LAST_KILLER[target as usize].load(Ordering::Relaxed);
    let first = DEAD_TARGET.swap(target, Ordering::Relaxed) != target;
    if first {
        DIED_AT.store(params.time.to_bits(), Ordering::Relaxed);
        DONE.store(false, Ordering::Relaxed);
        // Safety: both globals are inside client.dll's data, read and written
        // by V_GetDeathCam on this same thread.
        unsafe {
            *game.reset_camera = 1;
            *game.last_angles = ALIVE_ANGLES
                .each_ref()
                .map(|slot| f32::from_bits(slot.load(Ordering::Relaxed)));
        }
        DEATHS.fetch_add(1, Ordering::Relaxed);
        unsafe {
            crate::debug::report(&format!(
                "spectator_deathcam: player {target} died ({}) -- death cam, as his own recording shows",
                match killer_of(target, killer) {
                    Some(killer) => format!("killer {killer}"),
                    None => "no killer to turn to: looking down".to_string(),
                }
            ))
        };
    }

    // Its time is up: the game's own view of the dead player, as without
    // this switch, until he respawns.
    if DONE.load(Ordering::Relaxed) {
        return;
    }
    if !first && over(params.time, f32::from_bits(DIED_AT.load(Ordering::Relaxed))) {
        DONE.store(true, Ordering::Relaxed);
        unsafe {
            crate::debug::report(&format!(
                "spectator_deathcam: player {target}'s death cam is over after {HOLD_SECONDS} s -- back in his eyes until he respawns"
            ))
        };
        return;
    }

    let time = params.time.to_bits();
    if first || COMPUTED_AT.swap(time, Ordering::Relaxed) != time {
        let killer_entity = killer_of(target, killer)
            .map(|killer| unsafe { (engfuncs.get_entity_by_index)(killer) })
            .filter(|entity| !entity.is_null() && unsafe { (**entity).player } != 0)
            .unwrap_or(std::ptr::null_mut());
        let mut angles = [0f32; 3];
        let mut origin = [0f32; 3];
        // Safety: the game's own function with the arguments its own callers
        // give it; `player` is a live entity this frame.
        unsafe {
            (game.get_death_cam)(player, killer_entity, &mut angles, &mut origin);
            // V_GetChasePos clears this after its call; the function itself
            // does not.
            *game.reset_camera = 0;
        }
        for (slot, value) in RESULT.iter().zip(angles.into_iter().chain(origin)) {
            slot.store(value.to_bits(), Ordering::Relaxed);
        }
    }
    let view = RESULT
        .each_ref()
        .map(|slot| f32::from_bits(slot.load(Ordering::Relaxed)));
    let angles = [view[0], view[1], view[2]];
    let origin = [view[3], view[4], view[5]];
    params.viewangles = angles;
    params.vieworg = origin;
    let (forward, right, up) = angle_vectors(angles);
    params.forward = forward;
    params.right = right;
    params.up = up;

    // A dead player's own view has no gun; park the viewmodel below the
    // camera for this frame (the engine places it again next frame).
    let viewmodel = unsafe { (engfuncs.get_view_model)() };
    if !viewmodel.is_null() {
        // Safety: the engine's own viewmodel entity, which V_CalcRefdef has
        // just placed for this frame and the renderer reads next.
        unsafe {
            (*viewmodel).origin = Vec3 {
                x: origin[0] - up[0] * GUN_OUT_OF_VIEW,
                y: origin[1] - up[1] * GUN_OUT_OF_VIEW,
                z: origin[2] - up[2] * GUN_OUT_OF_VIEW,
            };
        }
    }
}

/// One line for `dodstudio_debug_status`, while the switch is on.
pub fn status_line() -> Option<String> {
    if !enabled() {
        return None;
    }
    let shown = DEATHS.load(Ordering::Relaxed);
    let now = match DEAD_TARGET.load(Ordering::Relaxed) {
        0 => "the watched player is alive".to_string(),
        target if DONE.load(Ordering::Relaxed) => {
            format!("player {target} is dead, his death cam over: in his eyes until he respawns")
        }
        target => format!(
            "death cam on player {target}, {}",
            match killer_of(target, LAST_KILLER[target as usize].load(Ordering::Relaxed)) {
                Some(killer) => format!("turning to player {killer}"),
                None => "looking down".to_string(),
            }
        ),
    };
    Some(format!(
        "spectator death cam: {NAME} = 1 -- {now} ({shown} shown so far, {HOLD_SECONDS} s each)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The operand offset is read off the disassembly in the module docs.
    /// Anchoring it to the pattern means a mistyped pattern cannot quietly
    /// move it onto an opcode.
    #[test]
    fn the_reset_camera_operand_follows_the_mov_eax() {
        let tokens: Vec<&str> = PATTERN.split_whitespace().collect();
        assert_eq!(tokens[RESET_CAMERA_OPERAND_AT - 1], "A1");
        assert!(
            tokens[RESET_CAMERA_OPERAND_AT..RESET_CAMERA_OPERAND_AT + 4]
                .iter()
                .all(|t| *t == "??")
        );
        // The two later offsets lie past the pattern, so the runtime checks
        // their opcodes (and `verify_deathcam_offsets.py` the binary).
        assert!(LAST_ANGLES_PUSH_AT > tokens.len());
    }

    #[test]
    fn angle_vectors_matches_the_engine_for_the_axes() {
        let close = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5);
        let (forward, right, up) = angle_vectors([0.0, 0.0, 0.0]);
        assert!(close(forward, [1.0, 0.0, 0.0]), "{forward:?}");
        assert!(close(right, [0.0, -1.0, 0.0]), "{right:?}");
        assert!(close(up, [0.0, 0.0, 1.0]), "{up:?}");
        // Looking straight down: forward is -z, up is +x.
        let (forward, _, up) = angle_vectors([90.0, 0.0, 0.0]);
        assert!(close(forward, [0.0, 0.0, -1.0]), "{forward:?}");
        assert!(close(up, [1.0, 0.0, 0.0]), "{up:?}");
        // Yaw 90: forward is +y, right is +x.
        let (forward, right, _) = angle_vectors([0.0, 90.0, 0.0]);
        assert!(close(forward, [0.0, 1.0, 0.0]), "{forward:?}");
        assert!(close(right, [1.0, 0.0, 0.0]), "{right:?}");
    }

    #[test]
    fn the_death_cam_runs_its_time_and_a_seek_back_ends_it() {
        assert!(!over(100.0, 100.0));
        assert!(!over(102.3, 100.0));
        assert!(over(102.5, 100.0));
        assert!(over(99.0, 100.0));
    }

    #[test]
    fn the_name_is_not_the_start_of_another_spec_name() {
        for other in [
            crate::spectator_follow::LOCK_NAME,
            crate::spectator_follow::TARGET_NAME,
            crate::names::SPEC_MATCH_POV,
        ] {
            assert!(!NAME.starts_with(other), "{other}");
            assert!(!other.starts_with(NAME), "{other}");
        }
    }

    #[test]
    fn the_world_and_the_player_himself_are_not_turned_to() {
        assert_eq!(killer_of(5, 7), Some(7));
        assert_eq!(killer_of(5, 0), None);
        assert_eq!(killer_of(5, 5), None);
        assert_eq!(killer_of(5, 33), None);
        assert_eq!(killer_of(5, -1), None);
    }

    #[test]
    fn deaths_are_noted_by_victim_and_out_of_range_ones_dropped() {
        note_death(3, 9);
        assert_eq!(LAST_KILLER[9].load(Ordering::Relaxed), 3);
        note_death(4, 0);
        note_death(4, 33);
        note_death(4, -2);
        assert_eq!(LAST_KILLER[0].load(Ordering::Relaxed), 0);
        assert_eq!(LAST_KILLER[9].load(Ordering::Relaxed), 3);
    }
}
