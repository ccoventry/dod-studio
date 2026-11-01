//! X-ray (#1297): while a demo plays, every player behind a wall is drawn as
//! a team-coloured glow through it, like CS:GO's GOTV `spec_show_xray`. Its
//! own switch, `ds_spec_xray`, off by default (a big change to the picture).
//! What CS:GO does and why this looks the way it does:
//! `docs/goldsrc_xray.md`.
//!
//! ## How
//!
//! Before the renderer draws a player (`StudioDrawPlayer`, wrapped in
//! `strafe_twitch`), [`before_player_drawn`] draws him once more as a glow
//! shell: the game's own `kRenderFxGlowShell` (the model pushed out along its
//! normals by `renderamt`, in `rendercolor`, added to the picture), with the
//! depth test turned round (`GL_GREATER`) so the shell only lands where
//! something already drawn is in front of it. A player in the open gets no
//! glow, a player behind a wall glows through it, and a player half behind
//! a crate glows only where the crate hides him, as in CS:GO. Drawing the
//! shell first, before the player himself, keeps his own body from counting
//! as a wall: his far arm behind his chest doesn't glow.
//!
//! The renderer draws a glow-shell entity twice, the model and then the
//! shell. During our pass the engine's `StudioDrawPoints` (wrapped through
//! the `engine_studio_api_t` the client copies, [`install`]) skips the model
//! and draws only the shell, so the player's skin never shows through walls.
//!
//! The stock shell shimmers: the engine lights it from a direction that
//! turns with the clock (`r_glowshellfreq`, PRE `hw+0x8230c`). For an even
//! glow that holds still frame to frame (#1297: movies at 300 fps) the pass
//! sets `r_glowshellfreq` to 0 for its length and draws with texturing off:
//! a flat team colour.
//!
//! Only players the engine draws at all can glow: it drops every entity
//! outside the camera's PVS before the renderer sees it. In an HLTV demo that
//! is the camera's PVS (the whole server is in the demo); a POV demo only
//! has the players the recorder's own PVS held.

#![cfg_attr(not(target_arch = "x86"), allow(dead_code, unused_imports))]

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize, Ordering};

use crate::engine::{self, ClEntityS, CvarSPartial, EngineStudioApiPartial};
use crate::names::console_name;
use crate::player_info;
use crate::setting::Setting;

/// The switch: 1 draws players behind walls as a team-coloured glow.
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

/// The shell's thickness (`renderamt`): how far past the model the glow
/// reaches.
const SHELL_AMOUNT: i32 = 12;

/// The glow's colours, added to the picture: the hues of DoD's kill-feed
/// team colours (`deathmsg::backdrop`), brighter, since a glow over a dark
/// wall is only as bright as its colour.
const ALLIES_GLOW: [u8; 3] = [90, 200, 90];
const AXIS_GLOW: [u8; 3] = [220, 70, 70];

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

/// TEMP(dev): the shell thickness, to compare looks.
pub const DEV_AMOUNT_NAME: &str = console_name!("debug_xray_amount");
pub static DEV_AMOUNT: crate::setting::Cvar = crate::setting::Cvar::new();

/// TEMP(dev): pause and play the demo for same-frame screenshots.
pub const DEV_PAUSE_NAME: &str = console_name!("debug_xray_pause");
pub const DEV_PLAY_NAME: &str = console_name!("debug_xray_play");
pub unsafe extern "C" fn dev_pause() {
    let r = crate::demo_seek::set_paused(true);
    unsafe { crate::debug::report(&format!("xray dev pause: {r:?}")) };
}
pub unsafe extern "C" fn dev_play() {
    let r = crate::demo_seek::set_paused(false);
    unsafe { crate::debug::report(&format!("xray dev play: {r:?}")) };
}

/// Whether the switch is on: the cvar, or its default without one.
pub fn enabled() -> bool {
    XRAY.on()
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

/// Draws the player the renderer is about to draw as a glow through walls,
/// when he gets one. `real` is the renderer's own `StudioDrawPlayer`, called
/// next by the caller for the player himself.
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
    // Demo playback only: in a live game this would be a wallhack.
    let api = e.p_demo_api;
    // Safety: the engine's own demo API, called on its thread.
    if api.is_null() || unsafe { ((*api).is_playingback)() } == 0 {
        return;
    }
    let current_slot = studio.slots_before_view_info()[0];
    if current_slot.is_null() {
        return;
    }
    // Safety: the engine's own accessor.
    let current: CurrentEntityFn = unsafe { std::mem::transmute(current_slot) };
    let ent = unsafe { current() };
    if ent.is_null() {
        return;
    }
    // Safety: the entity being drawn, valid for this call.
    let index = unsafe { (*ent).index };
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
    let Some(gl) = gl::get() else {
        return;
    };
    // TEMP(dev): 1 flat, 2 the chrome texture held still, 3 the stock shimmer.
    let style = XRAY.value().unwrap_or(1.0) as i32;
    let amount = DEV_AMOUNT.value().map_or(SHELL_AMOUNT, |v| v as i32);
    let freq = if style >= 3 { std::ptr::null_mut() } else { glow_freq(e) };
    // Safety: as above; restored below, before anything else reads it.
    let state = unsafe { &mut (*ent).curstate };
    let saved = (state.renderfx, state.renderamt, state.rendercolor);
    state.renderfx = RENDER_FX_GLOW_SHELL;
    state.renderamt = if amount > 0 { amount } else { SHELL_AMOUNT };
    state.rendercolor = colour;
    // Safety: `r_glowshellfreq`'s cvar_t, which the engine keeps for good.
    let saved_freq = (!freq.is_null()).then(|| unsafe { std::mem::replace(&mut (*freq).value, 0.0) });
    IN_PASS.store(true, Ordering::Relaxed);
    // Safety: the renderer's own function, drawing the same entity.
    unsafe { gl.behind_walls(style == 1, || real(STUDIO_RENDER, pplayer)) };
    IN_PASS.store(false, Ordering::Relaxed);
    if let Some(value) = saved_freq {
        unsafe { (*freq).value = value };
    }
    // Safety: the same entity; still this call's.
    let state = unsafe { &mut (*ent).curstate };
    (state.renderfx, state.renderamt, state.rendercolor) = saved;
    PASSES.fetch_add(1, Ordering::Relaxed);
}

/// Called from `HUD_AddEntity`'s detour once the client has answered for
/// `ent`, right before the engine's own PVS check in `CL_AddVisibleEntity`
/// (PRE `hw+0xbb1b`, POST `hw+0x1969f6`): in an HLTV demo the engine drops
/// every entity outside the camera's PVS there, unless `dev_overview` is set
/// (`hw+0x20920` / `+0x1ad020`: `dev_overview` and, in an HLTV demo, no
/// `sv_cheats` needed). So for a player who gets a glow, `dev_overview`'s
/// value is 1 for that one check, and 0 again from the next entity on
/// ([`before_calc_refdef`] at the latest, before anything is drawn). Only
/// its value is touched, never its string, and only when it is 0: someone
/// using the overview themselves keeps it.
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
    let index = unsafe { (*ent).index };
    if unsafe { (*ent).player } == 0
        || !glows(
            index,
            player_info::team(base, index),
            player_info::is_dead(base, index),
            None,
        )
    {
        return;
    }
    // Safety: the engine's own accessors, called on its thread.
    let api = e.p_demo_api;
    if api.is_null()
        || unsafe { ((*api).is_playingback)() } == 0
        || unsafe { (e.is_spectate_only)() } == 0
    {
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

/// The engine's `StudioDrawPoints`, as the client calls it. During our pass,
/// the model's own pass is skipped and only the shell is drawn.
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
    let current_slot = studio.slots_before_view_info()[0];
    if current_slot.is_null() {
        return;
    }
    // Safety: the engine's own accessor.
    let current: CurrentEntityFn = unsafe { std::mem::transmute(current_slot) };
    let ent = unsafe { current() };
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
        "{} glow pass(es) drawn so far",
        PASSES.load(Ordering::Relaxed)
    )
}

#[cfg(target_arch = "x86")]
mod gl {
    use std::sync::OnceLock;

    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    const GL_DEPTH_FUNC: u32 = 0x0b74;
    const GL_DEPTH_WRITEMASK: u32 = 0x0b72;
    const GL_TEXTURE_2D: u32 = 0x0de1;
    const GL_LESS: u32 = 0x0201;
    const GL_GREATER: u32 = 0x0204;
    const GL_GEQUAL: u32 = 0x0206;

    type GetIntegervFn = unsafe extern "system" fn(u32, *mut i32);
    type GetBooleanvFn = unsafe extern "system" fn(u32, *mut u8);
    type EnumFn = unsafe extern "system" fn(u32);
    type DepthMaskFn = unsafe extern "system" fn(u8);
    type IsEnabledFn = unsafe extern "system" fn(u32) -> u8;

    pub(super) struct Gl {
        get_integerv: GetIntegervFn,
        get_booleanv: GetBooleanvFn,
        depth_func: EnumFn,
        depth_mask: DepthMaskFn,
        enable: EnumFn,
        disable: EnumFn,
        is_enabled: IsEnabledFn,
    }

    impl Gl {
        /// Runs `draw` with the depth test passing only behind what is
        /// already drawn, no depth writes and no texture, then puts all of
        /// it back.
        ///
        /// Safety: on the engine's thread, its GL context current.
        pub(super) unsafe fn behind_walls(&self, flat: bool, draw: impl FnOnce() -> i32) {
            unsafe {
                let mut func = 0i32;
                (self.get_integerv)(GL_DEPTH_FUNC, &mut func);
                let mut mask = 1u8;
                (self.get_booleanv)(GL_DEPTH_WRITEMASK, &mut mask);
                let textured = flat && (self.is_enabled)(GL_TEXTURE_2D) != 0;
                // A reversed depth range (`GL_GEQUAL`) turns "behind" round too.
                let behind = if func as u32 == GL_GEQUAL || func as u32 == GL_GREATER {
                    GL_LESS
                } else {
                    GL_GREATER
                };
                (self.depth_func)(behind);
                (self.depth_mask)(0);
                if textured {
                    (self.disable)(GL_TEXTURE_2D);
                }
                draw();
                if textured {
                    (self.enable)(GL_TEXTURE_2D);
                }
                (self.depth_mask)(mask);
                (self.depth_func)(func as u32);
            }
        }
    }

    static GL: OnceLock<Option<Gl>> = OnceLock::new();

    fn load() -> Option<Gl> {
        let module = unsafe { GetModuleHandleA(c"opengl32.dll".as_ptr() as *const u8) };
        if module.is_null() {
            return None;
        }
        let proc = |name: &std::ffi::CStr| -> Option<usize> {
            unsafe { GetProcAddress(module, name.as_ptr() as *const u8) }.map(|p| p as usize)
        };
        // Safety: the exported functions' own signatures (stdcall).
        unsafe {
            Some(Gl {
                get_integerv: std::mem::transmute::<usize, GetIntegervFn>(proc(c"glGetIntegerv")?),
                get_booleanv: std::mem::transmute::<usize, GetBooleanvFn>(proc(c"glGetBooleanv")?),
                depth_func: std::mem::transmute::<usize, EnumFn>(proc(c"glDepthFunc")?),
                depth_mask: std::mem::transmute::<usize, DepthMaskFn>(proc(c"glDepthMask")?),
                enable: std::mem::transmute::<usize, EnumFn>(proc(c"glEnable")?),
                disable: std::mem::transmute::<usize, EnumFn>(proc(c"glDisable")?),
                is_enabled: std::mem::transmute::<usize, IsEnabledFn>(proc(c"glIsEnabled")?),
            })
        }
    }

    pub(super) fn get() -> Option<&'static Gl> {
        GL.get_or_init(load).as_ref()
    }
}

#[cfg(not(target_arch = "x86"))]
mod gl {
    pub(super) struct Gl;

    impl Gl {
        pub(super) unsafe fn behind_walls(&self, _flat: bool, _draw: impl FnOnce() -> i32) {}
    }

    pub(super) fn get() -> Option<&'static Gl> {
        None
    }
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
}
