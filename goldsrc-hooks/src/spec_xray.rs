//! X-ray (#1297): while a demo plays, every player behind a wall is drawn as
//! a team-coloured glow through it, like CS:GO's GOTV `spec_show_xray`. Its
//! own setting, `ds_spec_xray`, off by default (a big change to the
//! picture): `1` glows players where walls hide them, as CS:GO does; `2`
//! also outlines the players in plain view. What CS:GO does and what this
//! looks like: `docs/goldsrc_xray.md`.
//!
//! ## How
//!
//! Before the renderer draws a player (`StudioDrawPlayer`, wrapped in
//! `strafe_twitch`), [`before_player_drawn`] draws him once more as a glow
//! shell: the game's own `kRenderFxGlowShell` (the model pushed out along its
//! normals by `renderamt` x 0.05 units, in `rendercolor`, added to the
//! picture), with the depth test turned round (`GL_GREATER`) so the shell
//! only lands where something already drawn is in front of it. A player in
//! the open gets no glow, a player behind a wall glows through it, and one
//! half behind a crate glows only where the crate hides him. Drawn first,
//! before the player himself, his own body never counts as a wall. With `2`
//! the shell is drawn whatever the depth; the player then draws over its
//! middle, which leaves a ring round him: an outline.
//!
//! The renderer draws a glow-shell entity twice, the model and then the
//! shell. During our pass the engine's `StudioDrawPoints` (wrapped through
//! the `engine_studio_api_t` the client copies, [`install`]) skips the model
//! and draws only the shell, untextured, so the player's skin never shows
//! through a wall.
//!
//! The stock shell shimmers: the engine lights it from a direction that
//! turns with the clock (`r_glowshellfreq`, PRE `hw+0x8230c`). For a glow
//! that holds still frame to frame (movies at 300 fps), `r_glowshellfreq`'s
//! value is 0 for the length of the pass.
//!
//! An HLTV demo drops every entity outside the camera's PVS before the
//! renderer sees it; [`after_add_entity`] lets the players through, so the
//! whole map's players glow. A POV demo has only the players the recorder's
//! server sent (his own PVS), so those are all it can show.

#![cfg_attr(not(target_arch = "x86"), allow(dead_code, unused_imports))]

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize, Ordering};

use crate::engine::{self, ClEntityS, CvarSPartial, EngineStudioApiPartial, Vec3};
use crate::names::console_name;
use crate::player_info;
use crate::setting::Setting;

mod gl;

/// The setting: 0 off, 1 players behind walls glow, 2 and players in view
/// get an outline too.
pub const NAME: &str = console_name!("spec_xray");

/// The cvar, or its default (off) when it could not be registered.
pub static XRAY: Setting = Setting::new(NAME, false);

/// `STUDIO_RENDER`: draw, without `STUDIO_EVENTS` (sounds, muzzle flashes),
/// which the player's own draw runs.
const STUDIO_RENDER: i32 = 1;
/// `kRenderFxGlowShell`.
const RENDER_FX_GLOW_SHELL: i32 = 19;
/// `StudioDrawPoints`' slot in `engine_studio_api_t` (`r_studioint.h`).
const SLOT_STUDIO_DRAW_POINTS: usize = 25;

/// The shell's thickness per unit of distance from the camera
/// ([`shell_amount`]): about 2.5 pixels of a 1280-wide picture at a 90
/// degree field of view, whatever the distance.
const UNITS_PER_DISTANCE: f32 = 0.004;

/// The glow's colours, added to the picture: the hues of DoD's kill-feed
/// team colours (`deathmsg::backdrop`'s (76, 102, 76) and (255, 63, 63)),
/// at a strength that stays a colour rather than white once the engine has
/// lit it (its brightness tables double a model's light on the movie
/// installs' `brightness 2`).
const ALLIES_GLOW: [u8; 3] = [50, 145, 50];
const AXIS_GLOW: [u8; 3] = [160, 40, 40];

/// `int StudioDrawPlayer(int flags, entity_state_t *pplayer)`.
pub type DrawPlayerFn = unsafe extern "C" fn(i32, *mut c_void) -> i32;
/// `void StudioDrawPoints(void)`.
type DrawPointsFn = unsafe extern "C" fn();
/// `cl_entity_t *GetCurrentEntity(void)`.
type CurrentEntityFn = unsafe extern "C" fn() -> *mut ClEntityS;

static REAL_DRAW_POINTS: AtomicUsize = AtomicUsize::new(0);
/// Set for the length of our pass: [`draw_points`] draws the shell only.
static IN_PASS: AtomicBool = AtomicBool::new(false);
/// `r_glowshellfreq`, looked up on the first pass.
static GLOW_FREQ: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
/// `dev_overview`, looked up on the first player let through.
static DEV_OVERVIEW: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
/// Whether [`after_add_entity`] set `dev_overview` and must put it back.
static OVERVIEW_SET: AtomicBool = AtomicBool::new(false);
/// Glow passes drawn so far, for `ds_debug_status`.
static PASSES: AtomicU32 = AtomicU32::new(0);

/// Whether the setting is on (1 or 2).
pub fn enabled() -> bool {
    XRAY.on()
}

/// Whether players in plain view get an outline too (`2`).
fn outlines_in_view(value: Option<f32>) -> bool {
    value.is_some_and(|v| v >= 2.0)
}

/// The glow colour for a `teamnumber`, or `None` for no team (spectators,
/// a slot not yet filled).
fn glow_colour(team: i32) -> Option<[u8; 3]> {
    match team {
        player_info::ALLIES => Some(ALLIES_GLOW),
        player_info::AXIS => Some(AXIS_GLOW),
        _ => None,
    }
}

/// Whether player `index` gets a glow: a player slot, on a team, alive, and
/// not the one the in-eye camera looks out of.
fn glows(index: i32, team: Option<i32>, dead: Option<bool>, in_eye: Option<i32>) -> bool {
    (1..=32).contains(&index)
        && team.and_then(glow_colour).is_some()
        && dead == Some(false)
        && in_eye != Some(index)
}

/// The shell's `renderamt` for a player `distance` units from the camera: a
/// thickness that grows with distance keeps the glow's edge about the same
/// width on screen. Between 1.5 and 12.75 units (`renderamt` is a byte).
fn shell_amount(distance: f32) -> i32 {
    const MIN: i32 = 30;
    const MAX: i32 = 255;
    let amount = (distance.max(0.0) * UNITS_PER_DISTANCE / 0.05).round();
    if amount.is_nan() {
        return MIN;
    }
    (amount as i32).clamp(MIN, MAX)
}

/// Puts [`draw_points`] in `StudioDrawPoints`' slot of the engine's studio
/// API, before `HUD_GetStudioModelInterface` hands it to the client (which
/// copies it).
///
/// Safety: `pstudio` must be the engine's `engine_studio_api_t`.
pub unsafe fn install(pstudio: *mut EngineStudioApiPartial) {
    if pstudio.is_null() || REAL_DRAW_POINTS.load(Ordering::Acquire) != 0 {
        return;
    }
    // Safety: the struct is a table of function pointers; slot 25 is inside it.
    let slot = unsafe { (pstudio as *mut *mut c_void).add(SLOT_STUDIO_DRAW_POINTS) };
    if unsafe { *slot }.is_null() {
        return;
    }
    let result = unsafe {
        crate::patch::swap_vftable_slot(
            slot as usize,
            draw_points as *const () as usize,
            &REAL_DRAW_POINTS,
        )
    };
    if let Err(why) = result {
        REAL_DRAW_POINTS.store(0, Ordering::Release);
        unsafe { crate::debug::report(&format!("x-ray: not installed: {why}")) };
    }
}

/// Whether a demo is playing: x-ray never draws in a live game, where it
/// would be a wallhack.
fn playing_demo(e: &engine::ClEngineFuncsPartial) -> bool {
    let api = e.p_demo_api;
    // Safety: the engine's own demo API, called on its thread.
    !api.is_null() && unsafe { ((*api).is_playingback)() } != 0
}

/// The entity the renderer is drawing, or null.
fn current_entity(studio: &EngineStudioApiPartial) -> *mut ClEntityS {
    let slot = studio.slots_before_view_info()[0];
    if slot.is_null() {
        return std::ptr::null_mut();
    }
    // Safety: the engine's own accessor, called on its render thread.
    let current: CurrentEntityFn = unsafe { std::mem::transmute(slot) };
    unsafe { current() }
}

/// Draws the player the renderer is about to draw as a glow, when he gets
/// one. `real` is the renderer's own `StudioDrawPlayer`, which the caller
/// runs next for the player himself.
///
/// Safety: on the engine's thread, inside the `StudioDrawPlayer` wrapper,
/// with its arguments.
pub unsafe fn before_player_drawn(real: DrawPlayerFn, flags: i32, pplayer: *mut c_void) {
    if flags & STUDIO_RENDER == 0
        || !enabled()
        || REAL_DRAW_POINTS.load(Ordering::Acquire) == 0
        || IN_PASS.load(Ordering::Relaxed)
    {
        return;
    }
    let (Some(e), Some(studio), Some(base)) = (
        engine::engfuncs(),
        engine::engine_studio(),
        engine::client_module_base(),
    ) else {
        return;
    };
    if !playing_demo(e) {
        return;
    }
    let ent = current_entity(studio);
    if ent.is_null() {
        return;
    }
    // Safety: the entity being drawn, valid for this call.
    let (index, origin) = unsafe { ((*ent).index, (*ent).origin) };
    let team = player_info::team(base, index);
    if !glows(
        index,
        team,
        player_info::is_dead(base, index),
        crate::spectator_target::in_eye_target(),
    ) {
        return;
    }
    let Some(colour) = team.and_then(glow_colour) else {
        return;
    };
    let everywhere = outlines_in_view(XRAY.value());
    let freq = glow_freq(e);
    // Safety: as above; put back below, before anything else reads it.
    let state = unsafe { &mut (*ent).curstate };
    let saved = (state.renderfx, state.renderamt, state.rendercolor);
    state.renderfx = RENDER_FX_GLOW_SHELL;
    state.renderamt = shell_amount(view_distance(studio, origin));
    state.rendercolor = colour;
    // Safety: `r_glowshellfreq`'s cvar_t, which the engine keeps for good.
    let saved_freq =
        (!freq.is_null()).then(|| unsafe { std::mem::replace(&mut (*freq).value, 0.0) });
    IN_PASS.store(true, Ordering::Relaxed);
    // Safety: on the engine's thread, inside the renderer's draw; the
    // renderer's own function, drawing the same entity.
    let drawn = unsafe {
        gl::shell_pass(everywhere, || {
            real(STUDIO_RENDER, pplayer);
        })
    };
    IN_PASS.store(false, Ordering::Relaxed);
    if let Some(value) = saved_freq {
        unsafe { (*freq).value = value };
    }
    // Safety: the same entity; still this call's.
    let state = unsafe { &mut (*ent).curstate };
    (state.renderfx, state.renderamt, state.rendercolor) = saved;
    if drawn {
        PASSES.fetch_add(1, Ordering::Relaxed);
    }
}

/// How far `origin` is from the frame's camera.
fn view_distance(studio: &EngineStudioApiPartial, origin: Vec3) -> f32 {
    let mut eye = [0f32; 3];
    let (mut up, mut right, mut forward) = ([0f32; 3], [0f32; 3], [0f32; 3]);
    // Safety: the engine's own accessor, filling four vec3s.
    unsafe {
        (studio.get_view_info)(
            eye.as_mut_ptr(),
            up.as_mut_ptr(),
            right.as_mut_ptr(),
            forward.as_mut_ptr(),
        )
    };
    let d = [origin.x - eye[0], origin.y - eye[1], origin.z - eye[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// Called from `HUD_AddEntity`'s detour once the client has answered for
/// `ent`, right before the engine's own PVS check in `CL_AddVisibleEntity`
/// (PRE `hw+0xbb1b`, POST `hw+0x1969f6`): in an HLTV demo the engine drops
/// every entity outside the camera's PVS there, unless `dev_overview` is set
/// (the check's skip, PRE `hw+0x20920`, POST `+0x1ad020`: `dev_overview`
/// and, in an HLTV demo, no `sv_cheats` needed). So for a player who gets a
/// glow, `dev_overview`'s value is 1 for that one check, and 0 again from
/// the next entity on ([`before_calc_refdef`] at the latest, before anything
/// is drawn). Only its value is touched, never its string, and only when it
/// is 0: someone using the overview themselves keeps it.
///
/// Safety: on the engine's thread; `ent` is the engine's `cl_entity_t` for
/// this call, or null.
pub unsafe fn after_add_entity(drawn: bool, ent: *const ClEntityS) {
    let cvar = DEV_OVERVIEW.load(Ordering::Relaxed);
    if OVERVIEW_SET.swap(false, Ordering::Relaxed) && !cvar.is_null() {
        // Safety: `dev_overview`'s cvar_t, which the engine keeps for good.
        unsafe { (*cvar).value = 0.0 };
    }
    if !drawn || ent.is_null() || !enabled() {
        return;
    }
    let (Some(e), Some(base)) = (engine::engfuncs(), engine::client_module_base()) else {
        return;
    };
    // Safety: the entity being added, valid for this call.
    let (index, player) = unsafe { ((*ent).index, (*ent).player) };
    if player == 0
        || !glows(
            index,
            player_info::team(base, index),
            player_info::is_dead(base, index),
            None,
        )
    {
        return;
    }
    // Safety: the engine's own accessor, called on its thread.
    if !playing_demo(e) || unsafe { (e.is_spectate_only)() } == 0 {
        return;
    }
    let cvar = if cvar.is_null() {
        // Safety: the engine's own lookup; null when there is no such cvar.
        let found = unsafe { (e.pfn_get_cvar_pointer)(c"dev_overview".as_ptr()) };
        DEV_OVERVIEW.store(found, Ordering::Relaxed);
        found
    } else {
        cvar
    };
    // Safety: as above.
    if !cvar.is_null() && unsafe { (*cvar).value } == 0.0 {
        unsafe { (*cvar).value = 1.0 };
        OVERVIEW_SET.store(true, Ordering::Relaxed);
    }
}

/// Puts `dev_overview` back before the view is set up and drawn, in case the
/// last entity added was a player [`after_add_entity`] let through.
pub fn before_calc_refdef() {
    let cvar = DEV_OVERVIEW.load(Ordering::Relaxed);
    if OVERVIEW_SET.swap(false, Ordering::Relaxed) && !cvar.is_null() {
        // Safety: `dev_overview`'s cvar_t, which the engine keeps for good.
        unsafe { (*cvar).value = 0.0 };
    }
}

/// `r_glowshellfreq`'s cvar, found once.
fn glow_freq(e: &engine::ClEngineFuncsPartial) -> *mut CvarSPartial {
    let known = GLOW_FREQ.load(Ordering::Relaxed);
    if !known.is_null() {
        return known;
    }
    // Safety: the engine's own lookup; null when there is no such cvar.
    let found = unsafe { (e.pfn_get_cvar_pointer)(c"r_glowshellfreq".as_ptr()) };
    GLOW_FREQ.store(found, Ordering::Relaxed);
    found
}

/// The engine's `StudioDrawPoints`, as the client calls it. During our pass
/// the model's own draw is skipped and only the shell is drawn.
unsafe extern "C" fn draw_points() {
    let real = REAL_DRAW_POINTS.load(Ordering::Acquire);
    if real == 0 {
        return;
    }
    // Safety: the engine's own function, which takes no arguments.
    let real: DrawPointsFn = unsafe { std::mem::transmute(real) };
    if !IN_PASS.load(Ordering::Relaxed) {
        return unsafe { real() };
    }
    let Some(studio) = engine::engine_studio() else {
        return;
    };
    let ent = current_entity(studio);
    // Safety: the entity being drawn, valid for this call.
    if !ent.is_null() && unsafe { (*ent).curstate.renderfx } == RENDER_FX_GLOW_SHELL {
        unsafe { real() };
    }
}

/// For `ds_debug_status`.
pub fn status() -> String {
    if REAL_DRAW_POINTS.load(Ordering::Acquire) == 0 {
        return "not installed (the studio API was never handed over)".to_string();
    }
    format!(
        "{} player glow(s) drawn so far",
        PASSES.load(Ordering::Relaxed)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn teams_have_their_kill_feed_hue_and_nobody_else_glows() {
        let allies = glow_colour(player_info::ALLIES).unwrap();
        let axis = glow_colour(player_info::AXIS).unwrap();
        assert!(allies[1] > allies[0] && allies[1] > allies[2], "green");
        assert!(axis[0] > axis[1] && axis[0] > axis[2], "red");
        assert_eq!(glow_colour(0), None);
        assert_eq!(glow_colour(3), None, "spectators");
    }

    #[test]
    fn only_living_players_on_a_team_glow_and_not_the_in_eye_one() {
        let allies = Some(player_info::ALLIES);
        assert!(glows(5, allies, Some(false), None));
        assert!(glows(5, allies, Some(false), Some(6)));
        assert!(!glows(5, allies, Some(false), Some(5)), "the in-eye player");
        assert!(!glows(5, allies, Some(true), None), "dead");
        assert!(!glows(5, allies, None, None), "unknown");
        assert!(!glows(5, Some(3), Some(false), None), "spectator");
        assert!(!glows(5, None, Some(false), None));
        assert!(!glows(0, allies, Some(false), None), "not a player slot");
        assert!(!glows(33, allies, Some(false), None));
    }

    #[test]
    fn two_outlines_the_players_in_view_too() {
        assert!(!outlines_in_view(None));
        assert!(!outlines_in_view(Some(0.0)));
        assert!(!outlines_in_view(Some(1.0)));
        assert!(outlines_in_view(Some(2.0)));
    }

    #[test]
    fn the_shell_grows_with_distance_within_its_bounds() {
        assert_eq!(shell_amount(0.0), 30, "close up: the minimum");
        assert_eq!(shell_amount(1000.0), 80);
        assert!(shell_amount(2000.0) > shell_amount(1000.0));
        assert_eq!(shell_amount(1.0e9), 255, "far: renderamt's byte");
        assert_eq!(shell_amount(f32::NAN), 30);
        assert_eq!(shell_amount(-5.0), 30);
    }
}
