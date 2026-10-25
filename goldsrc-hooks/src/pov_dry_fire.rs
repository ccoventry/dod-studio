//! `ds_sound_fix_pov_dry_fire`: plays the recorder's shots that a POV demo
//! fired but never recorded a sound for (#1029).
//!
//! ## What goes wrong
//!
//! A POV demo's own gunshots are the recorder's client predicting them: its
//! weapon code fires the event (`events/weapons/<gun>.sc`) the moment the
//! trigger goes down, and the demo saves that event (a `Dem_Event` frame).
//! Playback replays those events and nothing else: the client's weapon
//! prediction does not run while a demo plays (`HUD_PostRunCmd` is never
//! called, measured on POST), and the server sends the shooter no event of
//! its own (`FEV_NOTHOST`).
//!
//! So a round the server fired that the recorder's client did not predict
//! has no sound, no muzzle flash and no fire animation on playback. The
//! usual way it happens: fire pressed a little before the gun is ready again
//! (40-110 ms early for all four in `ktps8w3-dyelife_ih_anzio_h1`, a Garand,
//! after the previous shot). The server fires it as soon as the gun is
//! ready; the client never does. Heard on playback as a dud where a shot
//! should be, which is what the issue calls the dry-fire click: the
//! client's real empty click (`PlayEmptySound`, `weapons/357_cock1.wav`)
//! never plays during playback either, not even for a real empty press.
//!
//! Measured over 153 POV demos from 34 recorder names: 220 of 33,937
//! server-confirmed shots (0.65%) have no recorded event, most of them from
//! a few recorders.
//!
//! ## What this does
//!
//! The server tells the recorder its clip after every shot (`CurWeapon`,
//! `active, weapon id, clip`). A drop in the held weapon's clip is a round the
//! server fired. The recorder's own events come 22-102 ms before it (398 ms at
//! most) and never after, so each drop is paired with the recorded shot
//! sound heard just before it; a drop with none gets a stand-in: the
//! weapon's own fire handler in `client.dll`, called for the recorder with
//! the arguments its recorded events carry ([`event_args`]). That plays the
//! sound, the muzzle flash, the shell and the first-person fire animation,
//! from the same code a recorded shot uses. Played on the next frame, from
//! `commands::poll`.
//!
//! A real empty press drops no clip, so it is left alone.
//!
//! POV demos only (`IsPlayingback()` and not `IsSpectateOnly()`); an HLTV
//! demo's lost shots are `missing_shots`' (`ds_spec_match_pov`). `2` also logs
//! every shot it pairs or plays.
//!
//! ## What it cannot do
//!
//! - Know the spread or the aim of the round: a small random spread and the
//!   recorder's replicated angles are used, so a restored round's impact is
//!   near, not at, where the real one landed.
//! - Play a round at the moment the trigger went down: it plays when the
//!   server's clip count arrives, about when the server fired it.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU32, Ordering};

use crate::engine::{self, ClEntityS, CvarSPartial};
use crate::missing_shots::{self, EventArgs};
use crate::names::console_name;

pub const NAME: &str = console_name!("sound_fix_pov_dry_fire");

/// The user message the server sends the recorder after each round.
pub const MESSAGE: &str = "CurWeapon";

/// A recorded shot sound this long before a clip drop belongs to it. The
/// survey's longest gap was 0.40 s, plus up to 64 ms for `CurWeapon` to
/// follow the client data.
const PAIRING_SECONDS: f32 = 0.75;

/// A gap in the engine's clock this long between frames is a seek, a pause
/// or a new demo: a clip count from before it says nothing about now.
const GAP_SECONDS: f32 = 0.5;

/// Rounds one message can account for. More is a seek or a lost update,
/// not shots.
const MOST_ROUNDS_PER_MESSAGE: u32 = 3;

/// The fire script of each DoD weapon id `CurWeapon` names. Ids 3-12, 20 and
/// 24 were checked against the recorded events of 153 POV demos (each id's
/// events all used the one script); the rest follow the `dod` crate's id
/// table and the script of the same name, and no surveyed demo fired them.
/// Melee weapons, grenades, rockets and the mortar are left out.
const SCRIPTS: &[(u8, &str)] = &[
    (3, "colt"),
    (4, "luger"),
    (5, "garand"),
    (6, "scopedkar"),
    (7, "thompson"),
    (8, "mp44"),
    (9, "spring"),
    (10, "kar"),
    (11, "bar"),
    (12, "mp40"),
    (17, "mg42"),
    (18, "30cal"),
    (20, "m1carbine"),
    (21, "mg34"),
    (22, "greasegun"),
    (23, "fg42"),
    (24, "k43"),
    (25, "enfield"),
    (26, "sten"),
    (27, "bren"),
    (28, "webley"),
    (35, "fg42"),
    (36, "m1carbine"),
    (38, "scopedenfield"),
];

fn script_for(weapon_id: u8) -> Option<&'static str> {
    SCRIPTS
        .iter()
        .find(|(id, _)| *id == weapon_id)
        .map(|(_, script)| *script)
}

/// What a POV recorder's own event for a round carries as
/// `[iparam1, bparam1, bparam2]`, given the clip after it. From every
/// recorder event paired with its clip drop in the survey:
///
/// - Garand, M1 carbine, K43: `bparam1` is the rounds left (0 on the last,
///   which plays the Garand's clip ping).
/// - BAR: `iparam1` is the rounds left. The MG42 and .30 cal take it the same
///   way in `missing_shots`; none were in the surveyed demos.
/// - Scoped K98: `bparam2` is 1 on most rounds, `bparam1` 1 on the last.
/// - Everything else: `bparam1` 1 on the last round, else all zero.
fn event_args(script: &str, clip_after: i32) -> [i32; 3] {
    let last = i32::from(clip_after <= 0);
    match script {
        "garand" | "m1carbine" | "k43" => [0, clip_after.max(0), 0],
        "bar" | "mg42" | "30cal" => [clip_after.max(0), 0, 0],
        "scopedkar" => [0, last, 1],
        _ => [0, last, 0],
    }
}

/// The rounds one `CurWeapon` says were fired, from the clip the last one
/// gave: a drop in the same weapon's clip. A different weapon, a clip that
/// went up (a reload, a pickup) or none known before: none.
fn rounds_fired(before: Option<(u8, u8)>, now: (u8, u8)) -> u32 {
    match before {
        Some((weapon, clip)) if weapon == now.0 && now.1 < clip => {
            u32::from(clip - now.1).min(MOST_ROUNDS_PER_MESSAGE)
        }
        _ => 0,
    }
}

/// Of `fired` rounds, how many the recording already played (`heard`
/// recorded shot sounds not yet paired) and how many it did not.
fn pair(fired: u32, heard: u32) -> (u32, u32) {
    let paired = fired.min(heard);
    (paired, fired - paired)
}

static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
static WANTED: AtomicBool = AtomicBool::new(false);
static EVERY_SHOT: AtomicBool = AtomicBool::new(false);

/// The last `CurWeapon` for the held weapon: `weapon id << 8 | clip`, or -1.
static LAST_HELD: AtomicI32 = AtomicI32::new(-1);
/// Rounds the server fired that wait for the next frame, the weapon and the
/// clip after the last of them.
static PENDING: AtomicU32 = AtomicU32::new(0);
static PENDING_WEAPON: AtomicU32 = AtomicU32::new(0);
static PENDING_CLIP: AtomicI32 = AtomicI32::new(0);
/// Recorded shot sounds of the recorder not yet paired with a round, and the
/// engine clock of the latest.
static HEARD: AtomicU32 = AtomicU32::new(0);
static HEARD_AT: AtomicU32 = AtomicU32::new(0);
/// The engine clock at the last frame, as f32 bits; 0 for none.
static LAST_FRAME: AtomicU32 = AtomicU32::new(0);

static PLAYED: AtomicU32 = AtomicU32::new(0);
static PAIRED: AtomicU32 = AtomicU32::new(0);
static NO_HANDLER: AtomicU32 = AtomicU32::new(0);

pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

fn report(line: &str) {
    unsafe { crate::debug::report(&format!("pov_dry_fire: {line}")) };
}

fn playing_pov_demo() -> bool {
    let Some(engfuncs) = engine::engfuncs() else {
        return false;
    };
    let api = engfuncs.p_demo_api;
    !api.is_null()
        && unsafe { ((*api).is_playingback)() } != 0
        && unsafe { (engfuncs.is_spectate_only)() } == 0
}

fn local_player() -> Option<&'static ClEntityS> {
    let engfuncs = engine::engfuncs()?;
    // Safety: the engine's own accessor; a `cl_entity_t` that lives as long
    // as the level, read on the engine thread.
    let local = unsafe { (engfuncs.get_local_player)() } as *const ClEntityS;
    (!local.is_null()).then(|| unsafe { &*local })
}

/// The demo's own clock under `viewdemo`, else the engine's. Not the
/// engine's under `viewdemo`: the demo player's stepped seek replays seconds
/// of messages while the engine clock runs on smoothly, and only the demo
/// clock shows the jump. Only called while a demo plays: the demo player's
/// clock read during start-up crashes it.
fn clock() -> Option<f32> {
    crate::demo_seek::clock()
        .filter(|c| c.active && !c.loading)
        .map(|c| c.now as f32)
        .or_else(engine::engine_client_time)
        .filter(|t| t.is_finite())
}

fn forget() {
    LAST_HELD.store(-1, Ordering::Relaxed);
    PENDING.store(0, Ordering::Relaxed);
    HEARD.store(0, Ordering::Relaxed);
}

/// Whether the engine clock has run on smoothly from the last frame to
/// `now`: no seek, pause or new demo in between.
fn continuous(now: f32) -> bool {
    let last = f32::from_bits(LAST_FRAME.load(Ordering::Relaxed));
    last != 0.0 && now >= last && now - last <= GAP_SECONDS
}

/// A recorded `_shoot` sample was just played for `entity`. Called from
/// `fire_sounds`' `EV_PlaySound` hook, on the engine thread.
pub fn on_shot_sound(entity: i32) {
    if !WANTED.load(Ordering::Relaxed) || missing_shots::standing_in() || !playing_pov_demo() {
        return;
    }
    if local_player().is_none_or(|local| local.index != entity) {
        return;
    }
    let Some(now) = clock() else {
        return;
    };
    let heard = HEARD.load(Ordering::Relaxed);
    HEARD.store((heard + 1).min(8), Ordering::Relaxed);
    HEARD_AT.store(now.to_bits(), Ordering::Relaxed);
}

/// One `CurWeapon` the server sent the recorder: `active, weapon id, clip`.
/// Called from the hooked handler (here or `msglog`'s), before the game's.
pub(crate) fn on_cur_weapon(payload: &[u8]) {
    if !WANTED.load(Ordering::Relaxed) || !playing_pov_demo() {
        return;
    }
    let [active, weapon, clip, ..] = *payload else {
        return;
    };
    // The weapon put away, in a switch: the next message is the one drawn.
    if active == 0 {
        return;
    }
    let held = i32::from(weapon) << 8 | i32::from(clip);
    let before = LAST_HELD.swap(held, Ordering::Relaxed);
    let Some(now) = clock() else {
        return;
    };
    // A seek replays a stretch of messages within a frame or two: their
    // drops are not rounds being fired now.
    if !continuous(now) {
        return;
    }
    let before = (before >= 0).then_some(((before >> 8) as u8, before as u8));
    let fired = rounds_fired(before, (weapon, clip));
    if fired == 0 {
        return;
    }
    // Paired now rather than at the next frame: a later round's recorded
    // sound must not be taken for this one's.
    if now - f32::from_bits(HEARD_AT.load(Ordering::Relaxed)) > PAIRING_SECONDS {
        HEARD.store(0, Ordering::Relaxed);
    }
    let (paired, missing) = pair(fired, HEARD.load(Ordering::Relaxed));
    HEARD.fetch_sub(paired, Ordering::Relaxed);
    PAIRED.fetch_add(paired, Ordering::Relaxed);
    if EVERY_SHOT.load(Ordering::Relaxed) && paired > 0 {
        report(&format!(
            "{paired} round(s) of weapon {weapon} at {now:.3} had their recorded sound ({clip} left)"
        ));
    }
    if missing > 0 {
        PENDING.fetch_add(missing, Ordering::Relaxed);
        PENDING_WEAPON.store(u32::from(weapon), Ordering::Relaxed);
        PENDING_CLIP.store(i32::from(clip), Ordering::Relaxed);
    }
}

/// Our `CurWeapon` handler: notes the clip, then hands the game the same
/// buffer, untouched.
unsafe extern "C" fn hooked_cur_weapon(
    name: *const std::ffi::c_char,
    size: i32,
    buf: *mut std::ffi::c_void,
) -> i32 {
    let payload: &[u8] = if size > 0 && !buf.is_null() {
        unsafe { std::slice::from_raw_parts(buf as *const u8, size as usize) }
    } else {
        &[]
    };
    on_cur_weapon(payload);
    match crate::msglog::original_thunk(MESSAGE) {
        Some(original) => unsafe { original(name, size, buf) },
        // 1 is what the engine's own dispatcher treats as handled.
        None => 1,
    }
}

/// Safe every frame: `pfnHookUserMsg` returns early when the head record
/// already carries this handler (the shape of `map_text::install_hook`).
fn install_hook() {
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    let Ok(name) = std::ffi::CString::new(MESSAGE) else {
        return;
    };
    unsafe { (engfuncs.pfn_hook_user_msg)(name.as_ptr(), hooked_cur_weapon) };
}

/// Called every frame from `commands::poll`.
pub fn poll() {
    let cvar = CVAR.load(Ordering::Acquire);
    let value = if cvar.is_null() {
        0.0
    } else {
        unsafe { (*cvar).value }
    };
    let wanted = value != 0.0;
    EVERY_SHOT.store(value >= 2.0, Ordering::Relaxed);
    if WANTED.swap(wanted, Ordering::Relaxed) != wanted {
        report(if wanted {
            "on: a POV recorder's unrecorded shots get their fire sound"
        } else {
            "off"
        });
        forget();
    }
    if !wanted || !playing_pov_demo() {
        LAST_FRAME.store(0, Ordering::Relaxed);
        forget();
        return;
    }
    // Once a level is loaded, so a demo is playing: a user-message hook
    // put in from the menu never sees a message.
    if !crate::msglog::watching(MESSAGE) {
        install_hook();
    }
    let Some(now) = clock() else {
        return;
    };
    let smooth = continuous(now);
    LAST_FRAME.store(now.to_bits(), Ordering::Relaxed);
    if !smooth {
        forget();
        return;
    }
    let missing = PENDING.swap(0, Ordering::Relaxed);
    let weapon = PENDING_WEAPON.load(Ordering::Relaxed) as u8;
    let clip = PENDING_CLIP.load(Ordering::Relaxed);
    // The earliest first: each missing round left one more in the clip.
    for round in (0..missing).rev() {
        stand_in(weapon, clip + round as i32, now);
    }
}

/// Plays the recorder's fire handler for one round of `weapon` the demo has
/// no event for.
fn stand_in(weapon: u8, clip_after: i32, now: f32) {
    let Some(local) = local_player() else {
        return;
    };
    let Some(script) = script_for(weapon) else {
        return;
    };
    let state = &local.curstate;
    let [iparam1, bparam1, bparam2] = event_args(script, clip_after);
    let mut args = EventArgs {
        entindex: local.index,
        origin: [state.origin.x, state.origin.y, state.origin.z],
        angles: missing_shots::event_angles([state.angles.x, state.angles.y, state.angles.z]),
        ducking: i32::from(state.usehull == 1),
        fparam1: missing_shots::next_spread(),
        fparam2: missing_shots::next_spread(),
        iparam1,
        bparam1,
        bparam2,
        ..EventArgs::default()
    };
    if !missing_shots::play_fire_event(script, &mut args) {
        if NO_HANDLER.fetch_add(1, Ordering::Relaxed) < 5 {
            report(&format!(
                "no fire handler noted for {script}; round at {now:.3} not played"
            ));
        }
        return;
    }
    let played = PLAYED.fetch_add(1, Ordering::Relaxed) + 1;
    if played <= 20 || EVERY_SHOT.load(Ordering::Relaxed) {
        report(&format!(
            "the recorder fired a {script} round at {now:.3} the demo has no sound for -- played it ({clip_after} left, {played} so far)"
        ));
    }
}

/// One `ds_debug_status` line, once the switch has been on.
pub fn status_line() -> Option<String> {
    let played = PLAYED.load(Ordering::Relaxed);
    (WANTED.load(Ordering::Relaxed) || played != 0).then(|| {
        format!(
            "{NAME}: {played} unrecorded POV shot(s) played, {} had their own sound, {} skipped (no fire handler)",
            PAIRED.load(Ordering::Relaxed),
            NO_HANDLER.load(Ordering::Relaxed),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The known case (#1029): `ktps8w3-dyelife_ih_anzio_h1`, the Garand
    /// round that kills seanality at 855.7 s, clip 3 -> 2 with no event.
    #[test]
    fn a_drop_in_the_held_clip_is_a_round() {
        assert_eq!(rounds_fired(Some((5, 3)), (5, 2)), 1);
        assert_eq!(pair(1, 0), (0, 1));
    }

    /// The round before it had its event, heard ~53 ms before the drop.
    #[test]
    fn a_round_with_its_recorded_sound_is_left_alone() {
        assert_eq!(rounds_fired(Some((5, 4)), (5, 3)), 1);
        assert_eq!(pair(1, 1), (1, 0));
    }

    #[test]
    fn a_reload_a_switch_or_nothing_known_is_no_round() {
        // The reload at 848.2 s: 0 -> 8.
        assert_eq!(rounds_fired(Some((5, 0)), (5, 8)), 0);
        // Garand to Colt.
        assert_eq!(rounds_fired(Some((5, 3)), (3, 7)), 0);
        assert_eq!(rounds_fired(None, (5, 7)), 0);
        assert_eq!(rounds_fired(Some((5, 7)), (5, 7)), 0);
    }

    /// A real empty press drops no clip, so it keeps whatever it does.
    #[test]
    fn an_empty_press_is_no_round() {
        assert_eq!(rounds_fired(Some((5, 0)), (5, 0)), 0);
    }

    #[test]
    fn a_big_drop_is_capped() {
        assert_eq!(rounds_fired(Some((11, 20)), (11, 18)), 2);
        assert_eq!(rounds_fired(Some((8, 30)), (8, 2)), MOST_ROUNDS_PER_MESSAGE);
    }

    #[test]
    fn rounds_pair_with_the_sounds_heard_first() {
        assert_eq!(pair(2, 1), (1, 1));
        assert_eq!(pair(1, 3), (1, 0));
        assert_eq!(pair(0, 2), (0, 0));
    }

    /// The arguments the survey's recorder events carry.
    #[test]
    fn stand_in_arguments_follow_the_recorded_events() {
        // Garand: rounds left in bparam1, so only the last pings.
        assert_eq!(event_args("garand", 2), [0, 2, 0]);
        assert_eq!(event_args("garand", 0), [0, 0, 0]);
        assert_eq!(event_args("k43", 9), [0, 9, 0]);
        // BAR: rounds left in iparam1.
        assert_eq!(event_args("bar", 17), [17, 0, 0]);
        // K98 and the rest: bparam1 marks the last round.
        assert_eq!(event_args("kar", 3), [0, 0, 0]);
        assert_eq!(event_args("kar", 0), [0, 1, 0]);
        assert_eq!(event_args("scopedkar", 4), [0, 0, 1]);
    }

    #[test]
    fn the_surveyed_ids_fire_their_scripts() {
        assert_eq!(script_for(5), Some("garand"));
        assert_eq!(script_for(10), Some("kar"));
        assert_eq!(script_for(11), Some("bar"));
        assert_eq!(script_for(24), Some("k43"));
        // Knives and grenades have no round to play.
        assert_eq!(script_for(1), None);
        assert_eq!(script_for(13), None);
    }

    /// Every script named is one `missing_shots` holds a handler slot for.
    #[test]
    fn every_script_is_a_known_weapon() {
        for (_, script) in SCRIPTS {
            assert!(
                missing_shots::is_known_script(script),
                "{script} is not in missing_shots::WEAPONS"
            );
        }
    }
}
