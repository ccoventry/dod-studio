//! Grenade practice on your own local server (issue #594): the real path of
//! every grenade you throw, drawn into the world, and a saved spot to throw
//! from again.
//!
//! - `dodstudio_nade_trails` (default 1): each throw's path as a coloured
//!   line, a white cross at each bounce, and a ring and post where it blew up.
//!   The flight time, bounces and distance go to the centre of the screen and
//!   the console when it lands. The last `dodstudio_nade_keep` (default 5)
//!   throws stay; `dodstudio_nade_clear` wipes them.
//! - `dodstudio_nade_xray` (default 1): paths behind walls still show, faded.
//! - `dodstudio_nade_save`, `dodstudio_nade_return`: remember where you stand
//!   and look, and go back there. `dodstudio_nade_goto` takes you to where the
//!   last grenade blew up.
//! - `dodstudio_nade_autoreturn` (default 0): that many seconds after a
//!   grenade blows up, back to the saved spot.
//!
//! ## Offline only
//!
//! Everything reads and moves the game's own server, `dod.dll`, which only
//! runs inside this `hl.exe` on a server you started yourself with `map`
//! (`game_dll.rs`). On top of that, nothing runs unless you are the only
//! human on that server (`game_dll::local_player_alone`), and nothing runs
//! during demo playback. On anyone else's server there is no server here to
//! read, and the connect guard (#451) refuses to join one anyway.
//!
//! The trails are the grenade's real server positions, one per server frame,
//! not a prediction. A bounce is a change in velocity that gravity doesn't
//! explain (`track.rs`).
//!
//! The trail cvar defaults to 1, unlike most `dodstudio_` switches, because
//! it changes nothing unless you are throwing grenades on your own server:
//! demo playback and captures never run one.

#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

mod draw;
mod game_dll;
mod track;

use std::ffi::CString;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Mutex, MutexGuard};

use crate::engine::{self, CvarSPartial};
use crate::names::console_name;

use game_dll as g;
use track::{Event, Sample, Tracker};

pub const TRAILS_NAME: &str = console_name!("nade_trails");
pub const XRAY_NAME: &str = console_name!("nade_xray");
pub const KEEP_NAME: &str = console_name!("nade_keep");
pub const AUTORETURN_NAME: &str = console_name!("nade_autoreturn");
const CLEAR_NAME: &str = console_name!("nade_clear");
const SAVE_NAME: &str = console_name!("nade_save");
const RETURN_NAME: &str = console_name!("nade_return");
const GOTO_NAME: &str = console_name!("nade_goto");
const DEBUG_NAME: &str = console_name!("debug_nade_entities");

/// The commands, for `commands::install` to register.
pub const COMMANDS: &[(&str, engine::ConsoleCommandFn)] = &[
    (CLEAR_NAME, cmd_clear),
    (SAVE_NAME, cmd_save),
    (RETURN_NAME, cmd_return),
    (GOTO_NAME, cmd_goto),
    (DEBUG_NAME, cmd_debug),
];

/// The models DoD's thrown grenades use: American, German, British.
const GRENADE_MODELS: [&str; 3] = [
    "models/w_grenade.mdl",
    "models/w_stick.mdl",
    "models/w_mills.mdl",
];

/// A standing player's origin is this far above their feet, a crouched one's
/// half that (`VEC_HULL_MIN`, `VEC_DUCK_HULL_MIN`).
const STAND_HEIGHT: f32 = 36.0;
const DUCK_HEIGHT: f32 = 18.0;
/// `TraceHull` hull numbers.
const HULL_STANDING: i32 = 1;

static CVAR_TRAILS: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
static CVAR_XRAY: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
static CVAR_KEEP: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
static CVAR_AUTORETURN: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());

pub fn set_cvars(
    trails: *mut CvarSPartial,
    xray: *mut CvarSPartial,
    keep: *mut CvarSPartial,
    autoreturn: *mut CvarSPartial,
) {
    CVAR_TRAILS.store(trails, Ordering::Release);
    CVAR_XRAY.store(xray, Ordering::Release);
    CVAR_KEEP.store(keep, Ordering::Release);
    CVAR_AUTORETURN.store(autoreturn, Ordering::Release);
}

fn cvar(c: &AtomicPtr<CvarSPartial>, default: f32) -> f32 {
    let p = c.load(Ordering::Acquire);
    if p.is_null() {
        return default;
    }
    let v = unsafe { (*p).value };
    if v.is_finite() { v } else { default }
}

#[derive(Clone, Copy)]
struct Spot {
    feet: [f32; 3],
    view: [f32; 3],
}

#[derive(Default)]
struct State {
    tracker: Tracker,
    /// The server time last sampled; a smaller one is a new map.
    last_time: f32,
    active: bool,
    saved: Option<Spot>,
    /// When `dodstudio_nade_autoreturn` sends you back, in server time.
    return_at: Option<f32>,
}

static STATE: Mutex<State> = Mutex::new(State {
    tracker: Tracker::new(),
    last_time: 0.0,
    active: false,
    saved: None,
    return_at: None,
});

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}

fn playing_demo() -> bool {
    let Some(engfuncs) = engine::engfuncs() else {
        return true;
    };
    let api = engfuncs.p_demo_api;
    !api.is_null() && unsafe { ((*api).is_playingback)() } != 0
}

/// The local player, when practice is allowed: your own server, nobody else
/// on it, and no demo playing.
fn me() -> Option<usize> {
    if playing_demo() {
        return None;
    }
    g::local_player_alone()
}

/// Once per frame, from `commands::poll`.
pub fn poll() {
    let Some(me) = me() else {
        let mut st = state();
        if st.active {
            st.active = false;
            st.tracker.reset();
            st.saved = None;
            st.return_at = None;
        }
        return;
    };
    let Some(now) = g::time() else { return };
    let mut st = state();
    if !st.active || now < st.last_time {
        // A new server, or a new map on it.
        st.tracker.reset();
        st.saved = None;
        st.return_at = None;
        st.active = true;
    }
    if now == st.last_time {
        // No server frame since the last look (paused, or a client-only frame).
        return;
    }
    st.last_time = now;

    if let Some(at) = st.return_at
        && now >= at
    {
        st.return_at = None;
        if let Some(spot) = st.saved
            && alive(me)
        {
            put(me, spot);
        }
    }

    if cvar(&CVAR_TRAILS, 1.0) == 0.0 {
        if !st.tracker.throws.is_empty() {
            st.tracker.clear();
        }
        return;
    }
    let live = grenades();
    let keep = cvar(&CVAR_KEEP, 5.0).clamp(1.0, 50.0) as usize;
    let events = st.tracker.update(now, &live, keep);
    for e in events {
        let Event::Landed(n) = e else { continue };
        if let Some(t) = st.tracker.throws.iter().find(|t| t.number == n) {
            let line = format!(
                "Grenade {}: {:.1} s, {} bounce{}, {:.0} units",
                n + 1,
                t.flight_time().unwrap_or(0.0),
                t.bounces.len(),
                if t.bounces.len() == 1 { "" } else { "s" },
                t.distance()
            );
            centre_print(&line);
            crate::commands::console_print(&format!("{line}\n"));
        }
        let delay = cvar(&CVAR_AUTORETURN, 0.0);
        if delay > 0.0 && st.saved.is_some() {
            st.return_at = Some(now + delay);
        }
    }
}

/// Every thrown grenade on the server this frame.
fn grenades() -> Vec<Sample> {
    let gravity = g::cvar(c"sv_gravity").unwrap_or(800.0);
    let mut out = Vec::new();
    for index in g::max_clients() + 1..g::max_entities() {
        let Some(e) = g::edict(index) else { continue };
        let v = g::vars(e);
        if g::read_i32(v, g::EV_MOVETYPE) != g::MOVETYPE_BOUNCE {
            continue;
        }
        let Some(model) = g::string(v, g::EV_MODEL) else {
            continue;
        };
        if !GRENADE_MODELS.contains(&model.as_str()) {
            continue;
        }
        let own = g::read_f32(v, g::EV_GRAVITY);
        out.push(Sample {
            id: (index, g::private_data(e)),
            origin: g::read_vec(v, g::EV_ORIGIN),
            velocity: g::read_vec(v, g::EV_VELOCITY),
            gravity: gravity * if own == 0.0 { 1.0 } else { own },
            on_ground: g::read_i32(v, g::EV_FLAGS) & g::FL_ONGROUND != 0,
            exploded: g::read_i32(v, g::EV_EFFECTS) & g::EF_NODRAW != 0,
        });
    }
    out
}

/// From `HUD_DrawTransparentTriangles`.
pub fn draw() {
    if cvar(&CVAR_TRAILS, 1.0) == 0.0 {
        return;
    }
    let Ok(st) = STATE.try_lock() else { return };
    if !st.active {
        return;
    }
    draw::throws(&st.tracker.throws, cvar(&CVAR_XRAY, 1.0) != 0.0);
}

fn height(ducking: bool) -> f32 {
    if ducking { DUCK_HEIGHT } else { STAND_HEIGHT }
}

fn ducking(vars: usize) -> bool {
    g::read_i32(vars, g::EV_FLAGS) & g::FL_DUCKING != 0
}

/// Moves the player to `spot`, feet first, whatever their stance now, and
/// turns them to its view.
fn put(me: usize, spot: Spot) {
    let v = g::vars(me);
    let mut origin = spot.feet;
    origin[2] += height(ducking(v));
    g::set_origin(me, origin);
    g::write_vec(v, g::EV_VELOCITY, [0.0; 3]);
    // The engine sends `angles` to the client as a forced view while
    // `fixangle` is set.
    g::write_vec(v, g::EV_ANGLES, spot.view);
    g::write_vec(v, g::EV_V_ANGLE, spot.view);
    g::write_i32(v, g::EV_FIXANGLE, 1);
}

fn say(text: &str) {
    crate::commands::console_print(&format!("{text}\n"));
}

const DEAD: &str = "Wait until you respawn.";

fn alive(me: usize) -> bool {
    g::read_i32(g::vars(me), g::EV_DEADFLAG) == 0
}

const NOT_HERE: &str =
    "Grenade practice only works on your own local server (map <name>), with nobody else on it.";

unsafe extern "C" fn cmd_clear() {
    state().tracker.clear();
}

unsafe extern "C" fn cmd_save() {
    let Some(me) = me() else { return say(NOT_HERE) };
    if !alive(me) {
        return say(DEAD);
    }
    let v = g::vars(me);
    let mut feet = g::read_vec(v, g::EV_ORIGIN);
    feet[2] -= height(ducking(v));
    state().saved = Some(Spot {
        feet,
        view: g::read_vec(v, g::EV_V_ANGLE),
    });
    centre_print("Spot saved");
}

unsafe extern "C" fn cmd_return() {
    let Some(me) = me() else { return say(NOT_HERE) };
    if !alive(me) {
        return say(DEAD);
    }
    let Some(spot) = state().saved else {
        return say(&format!(
            "No spot saved yet: stand somewhere and use {SAVE_NAME} first."
        ));
    };
    put(me, spot);
}

unsafe extern "C" fn cmd_goto() {
    let Some(me) = me() else { return say(NOT_HERE) };
    if !alive(me) {
        return say(DEAD);
    }
    let Some(landing) = state().tracker.last_landing() else {
        return say("No grenade has landed yet.");
    };
    let v = g::vars(me);
    let view = g::read_vec(v, g::EV_V_ANGLE);
    // The landing spot itself, then a ring around it, for somewhere a
    // standing player fits.
    let mut tries = vec![[0.0, 0.0]];
    for r in [24.0f32, 48.0, 72.0] {
        for k in 0..8 {
            let a = k as f32 * std::f32::consts::FRAC_PI_4;
            tries.push([r * a.cos(), r * a.sin()]);
        }
    }
    for [dx, dy] in tries {
        for lift in [2.0, 18.0] {
            let feet = [landing[0] + dx, landing[1] + dy, landing[2] + lift];
            let origin = [feet[0], feet[1], feet[2] + STAND_HEIGHT];
            if g::hull_fits(origin, HULL_STANDING, me) == Some(true) {
                let mut spot = Spot { feet, view };
                if ducking(v) {
                    // Put the hull's centre where a standing one would fit.
                    spot.feet[2] += STAND_HEIGHT - DUCK_HEIGHT;
                }
                return put(me, spot);
            }
        }
    }
    say("Nowhere to stand near where it landed.");
}

/// Lists every moving entity that isn't a player, to the console and the hook
/// log: what the grenade finder looks at, for when it finds nothing.
unsafe extern "C" fn cmd_debug() {
    if me().is_none() {
        return say(NOT_HERE);
    }
    let mut lines = Vec::new();
    if let Some(me) = me() {
        let v = g::vars(me);
        let o = g::read_vec(v, g::EV_ORIGIN);
        lines.push(format!(
            "you: health {} team {} deadflag {} observer {} weapons {:#x} viewmodel {} at {:.0} {:.0} {:.0}",
            g::read_f32(v, 0x160),
            g::read_i32(v, 0x1ac),
            g::read_i32(v, 0x170),
            g::read_i32(v, 0x244),
            g::read_i32(v, 0x168),
            g::string(v, 0xbc).unwrap_or_default(),
            o[0],
            o[1],
            o[2],
        ));
    }
    for index in g::max_clients() + 1..g::max_entities() {
        let Some(e) = g::edict(index) else { continue };
        let v = g::vars(e);
        let vel = g::read_vec(v, g::EV_VELOCITY);
        let movetype = g::read_i32(v, g::EV_MOVETYPE);
        if vel == [0.0; 3] && movetype != g::MOVETYPE_BOUNCE {
            continue;
        }
        let o = g::read_vec(v, g::EV_ORIGIN);
        lines.push(format!(
            "#{index} {} model {} movetype {movetype} effects {} at {:.0} {:.0} {:.0} speed {:.0}",
            g::string(v, g::EV_CLASSNAME).unwrap_or_default(),
            g::string(v, g::EV_MODEL).unwrap_or_default(),
            g::read_i32(v, g::EV_EFFECTS),
            o[0],
            o[1],
            o[2],
            track::dist(vel, [0.0; 3]),
        ));
    }
    if lines.is_empty() {
        lines.push("no moving entities".to_string());
    }
    for l in &lines {
        say(l);
        unsafe { crate::debug::report(&format!("nade: {l}")) };
    }
}

/// `gEngfuncs.pfnCenterPrint`, slot 31 of `cl_enginefunc_t` (the one after
/// `pfnConsolePrint`).
fn centre_print(text: &str) {
    const SLOT_CENTER_PRINT: usize = 31;
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    let Ok(text) = CString::new(text) else { return };
    unsafe {
        let f = *(engfuncs as *const engine::ClEngineFuncsPartial as *const usize)
            .add(SLOT_CENTER_PRINT);
        if f == 0 {
            return;
        }
        let f: unsafe extern "C" fn(*const std::ffi::c_char) = std::mem::transmute(f);
        f(text.as_ptr());
    }
}

/// For `dodstudio_debug_status`, once practice has run this session.
pub fn status_line() -> Option<String> {
    let st = STATE.try_lock().ok()?;
    st.active.then(|| {
        format!(
            "grenade practice: on your local server, {} throw(s) drawn, spot {}",
            st.tracker.throws.len(),
            if st.saved.is_some() {
                "saved"
            } else {
                "not saved"
            }
        )
    })
}
