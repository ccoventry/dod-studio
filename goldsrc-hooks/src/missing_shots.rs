//! Plays the gunshots an HLTV recording lost.
//!
//! ## What is missing
//!
//! A shot reaches a client as a fire event (`events/weapons/<gun>.sc`), and the
//! client's handler for it makes the sound, the muzzle flash and the bullet
//! impact. An HLTV demo carries the event for well under half of the shots
//! fired: in one match half, 1180 events against 1812 rounds with none (61%),
//! and 177 of 324 gunfire kills had no fire event from the killer in the
//! quarter second before (`analysis/examples/hltv_shot_evidence_probe.rs`).
//! Those rounds are silent, flashless and leave no mark.
//!
//! ## What still says the round was fired
//!
//! The shooter's own body. The server restarts the attack animation on every
//! shot (`SetAnimation(PLAYER_ATTACK1)`), and `sequence` and `frame` are
//! replicated in nearly every player update. So a round shows up as the body
//! entering a `*_shoot` sequence, or as `frame` stepping backwards inside
//! one. Measured on the same half: every one of the 1180 events has such a
//! restart beside it, and the two kinds of restart lose their event equally
//! often (39.2% of each keep it), which is what real rounds would do.
//!
//! ## What this does
//!
//! Each frame it reads every player's `sequence` and `frame`. A restart in a
//! bullet weapon's `*_shoot` sequence that no fire event came with gets the
//! event it lost: `client.dll`'s own handler for that weapon is called with
//! the arguments the engine would have built from the player's state, so the
//! round gets its sound, flash, tracer and impact from the same code a
//! recorded round does.
//!
//! - **Which handler.** `pfnHookEvent` is wrapped for the length of
//!   `client.dll`'s `Initialize`, which is when it registers its event
//!   handlers; the wrapper notes the handler for each weapon script and
//!   forwards the call. No address is hardcoded.
//! - **Which weapon.** The third-person model the player holds
//!   (`curstate.weaponmodel`), through [`WEAPONS`].
//! - **Not twice.** `sound_fix`'s `EV_PlaySound` hook reports every real
//!   `_shoot` sample with its entity. A restart waits one frame for its own
//!   event; one that arrives, either side of it, cancels the stand-in.
//!
//! Only while an HLTV demo plays (`IsSpectateOnly()`), and only with
//! `dodstudio_hltv_play_missing_gunshots 1`. Off by default. `2` also logs
//! every round it plays, not only the first twenty.
//!
//! ## What it cannot do
//!
//! - Know the spread: the lost event carried it. A small random one is used,
//!   so a restored round's impact is near, not at, where the real one landed.
//! - Tell a Garand's last round, so a restored one has no clip ping.
//! - Restore melee, grenades or rockets. Their events carry arguments (hit or
//!   miss, which swing) that the body does not show.
//! - See two rounds from one player inside a single rendered frame. At 60 fps
//!   that cannot happen; under about 20 fps an MG42 could lose one.

use std::ffi::{CStr, c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU32, AtomicU64, Ordering};

use crate::engine::{
    self, ClEngineFuncsPartial, ClEntityS, CvarSPartial, EventHandlerFn, ModelSPartial,
};
use crate::names::console_name;

pub const NAME: &str = console_name!("hltv_play_missing_gunshots");

/// `event_args_t` (`common/event_args.h`), what a fire handler is passed.
#[repr(C)]
#[derive(Default)]
struct EventArgs {
    flags: i32,
    entindex: i32,
    origin: [f32; 3],
    angles: [f32; 3],
    velocity: [f32; 3],
    ducking: i32,
    fparam1: f32,
    fparam2: f32,
    iparam1: i32,
    iparam2: i32,
    bparam1: i32,
    bparam2: i32,
}

const _: () = assert!(size_of::<EventArgs>() == 72);

/// Each bullet weapon's event script (`events/weapons/<name>.sc`) and the
/// third-person models that fire it, with the `p_` prefix, `.mdl` and any
/// `_l` suffix removed. From the 75 `p_*.mdl` files and the 29 scripts the
/// game ships; melee, grenades, rockets and the mortar are left out on
/// purpose (see the module docs).
const WEAPONS: &[(&str, &[&str])] = &[
    ("30cal", &["30cal", "30calpr", "30calr", "30calsr"]),
    ("bar", &["barbd", "barbu"]),
    (
        "bren",
        &["bren", "brenbd", "brenbr", "brenbu", "brenpr", "brensr"],
    ),
    ("colt", &["colt"]),
    ("enfield", &["enfield"]),
    ("scopedenfield", &["enfields"]),
    (
        "fg42",
        &[
            "fg42",
            "fg42bd",
            "fg42bu",
            "fg42pr",
            "fg42s",
            "fg42sr",
            "scopedfg42bu",
        ],
    ),
    ("garand", &["garand"]),
    ("greasegun", &["grease"]),
    ("k43", &["k43"]),
    ("kar", &["k98"]),
    ("scopedkar", &["k98s"]),
    ("luger", &["luger"]),
    ("m1carbine", &["fcarb", "m1carb"]),
    ("mg34", &["mg34bd", "mg34bu", "mg34pr", "mg34sr"]),
    ("mg42", &["mg42bd", "mg42bu", "mg42pr", "mg42sr"]),
    ("mp40", &["mp40"]),
    ("mp44", &["stg44"]),
    ("spring", &["spring"]),
    ("sten", &["sten"]),
    ("thompson", &["tommy"]),
    ("webley", &["webley"]),
];

/// The handler `client.dll` registered for each of [`WEAPONS`], or null.
static HANDLERS: [AtomicPtr<c_void>; WEAPONS.len()] =
    [const { AtomicPtr::new(std::ptr::null_mut()) }; WEAPONS.len()];

/// The body tokens whose `*_shoot` sequence is not a bullet leaving a barrel.
const NOT_BULLETS: &[&str] = &[
    "gren", "stick", "mills", "knife", "spade", "bazooka", "pschreck", "piat",
];

/// Players are entities 1 to 32; slot 0 is unused so the index needs no
/// arithmetic.
const SLOTS: usize = 33;

/// Half the width of the stand-in spread. Recorded MP40 rounds carry about
/// +-0.1 on the move and far less standing; this sits between.
const SPREAD: f32 = 0.04;

/// A gap between frames this long is a pause, a seek or a new demo, and what
/// was being tracked no longer follows from what is seen now.
const TRACKING_GAP_SECONDS: f64 = 0.5;

static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());
static WANTED: AtomicBool = AtomicBool::new(false);
/// `dodstudio_hltv_play_missing_gunshots 2`: a log line for every round, not
/// only the first few. For checking a session against the offline probe.
static EVERY_ROUND: AtomicBool = AtomicBool::new(false);

// What each player's body was last seen doing.
static SEEN: [AtomicBool; SLOTS] = [const { AtomicBool::new(false) }; SLOTS];
static SEQUENCE: [AtomicI32; SLOTS] = [const { AtomicI32::new(0) }; SLOTS];
static FRAME: [AtomicU32; SLOTS] = [const { AtomicU32::new(0) }; SLOTS];
static MSG_TIME: [AtomicU32; SLOTS] = [const { AtomicU32::new(0) }; SLOTS];
/// The frame number a restart was seen on, while it waits for its own event.
/// Zero for none.
static PENDING: [AtomicU32; SLOTS] = [const { AtomicU32::new(0) }; SLOTS];
/// The frame number a real shot was heard on and not yet paired with a
/// restart. Zero for none.
static HEARD: [AtomicU32; SLOTS] = [const { AtomicU32::new(0) }; SLOTS];

/// Counts `poll` calls. Starts at 1 so zero can mean "none" above.
static FRAME_NUMBER: AtomicU32 = AtomicU32::new(1);
static LAST_POLL: AtomicU64 = AtomicU64::new(0);
/// Set while a handler runs on this module's behalf, so the sound it makes is
/// not taken for a recorded round.
static STANDING_IN: AtomicBool = AtomicBool::new(false);
static SPREAD_SEED: AtomicU32 = AtomicU32::new(0x2545_f491);

static PLAYED: AtomicU32 = AtomicU32::new(0);
static HAD_EVENT: AtomicU32 = AtomicU32::new(0);
static UNKNOWN_WEAPON: AtomicU32 = AtomicU32::new(0);

/// Called by `commands.rs` once the cvar is registered.
pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

/// `"events/weapons/mp40.sc"` -> the index of `mp40` in [`WEAPONS`].
fn weapon_for_script(script: &[u8]) -> Option<usize> {
    let name = script
        .strip_prefix(b"events/weapons/")?
        .strip_suffix(b".sc")?;
    WEAPONS
        .iter()
        .position(|(event, _)| event.as_bytes().eq_ignore_ascii_case(name))
}

/// `"models/p_k98s_l.mdl"` -> the index of `scopedkar` in [`WEAPONS`].
fn weapon_for_held_model(name: &str) -> Option<usize> {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let stem = file.strip_suffix(".mdl").unwrap_or(file);
    let stem = stem.strip_prefix("p_")?;
    let stem = stem.strip_suffix("_l").unwrap_or(stem);
    WEAPONS
        .iter()
        .position(|(_, held)| held.iter().any(|h| h.eq_ignore_ascii_case(stem)))
}

/// Whether a body sequence is a bullet weapon firing: `<stance>_<token>_shoot`
/// with a token that is not a grenade, a blade or a rocket.
fn is_bullet_shoot(label: &str) -> bool {
    let Some(rest) = label.strip_suffix("_shoot") else {
        return false;
    };
    let Some((_stance, token)) = rest.split_once('_') else {
        return false;
    };
    !NOT_BULLETS.contains(&token)
}

/// Whether the body went from `before` to `now` by starting an animation
/// over: a different sequence, or the same one further back than it was.
fn restarted(before: (i32, f32), now: (i32, f32)) -> bool {
    now.0 != before.0 || now.1 < before.1
}

/// The angles a fire handler is given for a player, from the replicated
/// ones: each wrapped into +-180, and the pitch turned back from the third
/// of it, negated, that a player's entity state carries. The engine's own
/// `CL_ParseEvent` does the same.
fn event_angles(state: [f32; 3]) -> [f32; 3] {
    let wrap = |a: f32| {
        let a = a % 360.0;
        if a > 180.0 {
            a - 360.0
        } else if a < -180.0 {
            a + 360.0
        } else {
            a
        }
    };
    [wrap(state[0]) * -3.0, wrap(state[1]), wrap(state[2])]
}

/// Whether a real shot heard on frame `heard` belongs to a restart seen on
/// frame `now`: the same frame's packet, read just before or just after.
fn heard_with(heard: u32, now: u32) -> bool {
    heard != 0 && heard + 1 >= now
}

/// The next stand-in spread value, in `-SPREAD..=SPREAD`. A xorshift step:
/// nothing here needs to be unpredictable, only not the same every round.
fn next_spread() -> f32 {
    let mut x = SPREAD_SEED.load(Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    SPREAD_SEED.store(x, Ordering::Relaxed);
    ((x >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0) * SPREAD
}

/// The engine's own `pfnHookEvent`, while it is wrapped.
static REAL_HOOK_EVENT: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

unsafe extern "C" fn wrapped_hook_event(name: *const c_char, handler: EventHandlerFn) {
    if !name.is_null() {
        // Safety: the client passes a NUL-terminated literal.
        let script = unsafe { CStr::from_ptr(name) }.to_bytes();
        if let Some(weapon) = weapon_for_script(script) {
            HANDLERS[weapon].store(handler as *mut c_void, Ordering::Release);
        }
    }
    let real = REAL_HOOK_EVENT.load(Ordering::Acquire);
    if !real.is_null() {
        // Safety: the pointer taken out of the engine's own table below.
        let real: engine::HookEventFn = unsafe { std::mem::transmute(real) };
        unsafe { real(name, handler) };
    }
}

/// Runs `initialize` -- `client.dll`'s real `Initialize` -- with
/// `pfnHookEvent` wrapped, so the handlers it registers are noted, then puts
/// the engine's own function back.
///
/// The client copies the table at the top of `Initialize` and registers its
/// events from inside it, so the wrapper has to be in the engine's table
/// before the call and is not needed after it. The copy keeps the wrapper,
/// which goes on forwarding for the life of the process.
///
/// Safety: `engfuncs` must be the engine's live table, on the engine thread.
pub unsafe fn noting_event_handlers<R>(
    engfuncs: *mut ClEngineFuncsPartial,
    initialize: impl FnOnce() -> R,
) -> R {
    let slot = unsafe { &raw mut (*engfuncs).pfn_hook_event } as usize;
    let real = unsafe { (*engfuncs).pfn_hook_event } as usize;
    REAL_HOOK_EVENT.store(real as *mut c_void, Ordering::Release);
    let wrapper = wrapped_hook_event as *const () as usize;
    // Through the patch helper, not a plain store: nothing says the engine
    // keeps this table in writable memory.
    let wrapped = unsafe { crate::patch::write_code_bytes(slot, &wrapper.to_ne_bytes()) };
    let result = initialize();
    if wrapped {
        unsafe { crate::patch::write_code_bytes(slot, &real.to_ne_bytes()) };
    }
    let noted = HANDLERS
        .iter()
        .filter(|h| !h.load(Ordering::Acquire).is_null())
        .count();
    unsafe {
        crate::debug::report(&format!(
            "missing_shots: noted {noted} of {} weapon fire handlers as client.dll registered them{}",
            WEAPONS.len(),
            if wrapped {
                ""
            } else {
                " (pfnHookEvent could not be wrapped)"
            }
        ))
    };
    result
}

/// A real `_shoot` sample was just played for `entity`. Called from
/// `sound_fix`'s `EV_PlaySound` hook, on the engine thread.
pub fn on_shot_sound(entity: i32) {
    if STANDING_IN.load(Ordering::Relaxed) || !WANTED.load(Ordering::Relaxed) {
        return;
    }
    let Some(slot) = usize::try_from(entity)
        .ok()
        .filter(|i| (1..SLOTS).contains(i))
    else {
        return;
    };
    // The restart was seen first and its event came a frame behind it.
    if PENDING[slot].swap(0, Ordering::Relaxed) != 0 {
        HAD_EVENT.fetch_add(1, Ordering::Relaxed);
        return;
    }
    HEARD[slot].store(FRAME_NUMBER.load(Ordering::Relaxed), Ordering::Relaxed);
}

fn forget(slot: usize) {
    SEEN[slot].store(false, Ordering::Relaxed);
    PENDING[slot].store(0, Ordering::Relaxed);
    HEARD[slot].store(0, Ordering::Relaxed);
}

fn forget_everyone() {
    for slot in 1..SLOTS {
        forget(slot);
    }
}

fn playing_hltv_demo(engfuncs: &ClEngineFuncsPartial) -> bool {
    let api = engfuncs.p_demo_api;
    !api.is_null()
        && unsafe { ((*api).is_playingback)() } != 0
        && unsafe { (engfuncs.is_spectate_only)() } != 0
}

/// Called every frame from `commands::poll`.
pub fn poll() {
    let cvar = CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        return;
    }
    let value = unsafe { (*cvar).value };
    let wanted = value != 0.0;
    EVERY_ROUND.store(value >= 2.0, Ordering::Relaxed);
    if WANTED.swap(wanted, Ordering::Relaxed) != wanted {
        unsafe {
            crate::debug::report(&format!(
                "missing_shots: {NAME} = {}",
                if wanted { "1 (on)" } else { "0 (off)" }
            ))
        };
        forget_everyone();
    }
    if !wanted {
        return;
    }
    let (Some(engfuncs), Some(studio)) = (engine::engfuncs(), engine::engine_studio()) else {
        return;
    };

    let now = engine::client_time();
    let last = f64::from_bits(LAST_POLL.swap(now.to_bits(), Ordering::Relaxed));
    if !playing_hltv_demo(engfuncs) || now - last > TRACKING_GAP_SECONDS {
        forget_everyone();
        return;
    }
    let frame_number = FRAME_NUMBER.fetch_add(1, Ordering::Relaxed) + 1;

    for slot in 1..SLOTS {
        // Safety: the engine's own accessor, null for an empty slot.
        let entity: *mut ClEntityS = unsafe { (engfuncs.get_entity_by_index)(slot as i32) };
        if entity.is_null() {
            forget(slot);
            continue;
        }
        // Safety: a non-null `cl_entity_t` lives as long as the frame, and
        // this is the engine's own thread.
        let entity = unsafe { &*entity };
        if entity.player == 0 {
            forget(slot);
            continue;
        }
        let state = &entity.curstate;
        let now_body = (state.sequence, state.frame);
        let before = SEEN[slot].swap(true, Ordering::Relaxed).then(|| {
            (
                SEQUENCE[slot].load(Ordering::Relaxed),
                f32::from_bits(FRAME[slot].load(Ordering::Relaxed)),
            )
        });
        let earlier =
            f32::from_bits(MSG_TIME[slot].swap(state.msg_time.to_bits(), Ordering::Relaxed));
        SEQUENCE[slot].store(now_body.0, Ordering::Relaxed);
        FRAME[slot].store(now_body.1.to_bits(), Ordering::Relaxed);

        if before.is_some() && state.msg_time < earlier {
            // The demo went backwards: a seek. Nothing follows from before it.
            PENDING[slot].store(0, Ordering::Relaxed);
            HEARD[slot].store(0, Ordering::Relaxed);
            continue;
        }

        // A restart that has waited a frame and heard nothing lost its event.
        // Before this frame's own restart is looked at, so a round seen now
        // cannot take the place of one still waiting.
        // Anything pending here was set by an earlier frame's pass.
        if PENDING[slot].swap(0, Ordering::Relaxed) != 0 {
            // Safety: null for an index the engine has not precached.
            let held = unsafe { (studio.get_model_by_index)(state.weaponmodel) };
            stand_in(slot, entity, held);
        }

        if let Some(before) = before
            && restarted(before, now_body)
        {
            // Safety: null for an index the engine has not precached, which
            // `sequence_label` handles.
            let body = unsafe { (studio.get_model_by_index)(state.modelindex) };
            let fired = usize::try_from(now_body.0)
                .ok()
                .and_then(|sequence| crate::anim_fix::sequence_label(body, sequence))
                .is_some_and(|label| is_bullet_shoot(&label));
            if fired {
                if heard_with(HEARD[slot].swap(0, Ordering::Relaxed), frame_number) {
                    HAD_EVENT.fetch_add(1, Ordering::Relaxed);
                } else {
                    PENDING[slot].store(frame_number, Ordering::Relaxed);
                }
            }
        }
    }
}

/// Calls the weapon's own fire handler for a round whose event was lost.
fn stand_in(slot: usize, entity: &ClEntityS, held: *mut ModelSPartial) {
    let held_name = (!held.is_null())
        // Safety: a non-null model from the engine's own lookup.
        .then(|| unsafe { (*held).name_str() });
    let handler = held_name
        .as_deref()
        .and_then(weapon_for_held_model)
        .map(|weapon| (weapon, HANDLERS[weapon].load(Ordering::Acquire)))
        .filter(|(_, handler)| !handler.is_null());
    let Some((weapon, handler)) = handler else {
        // Usually a round fired just before a weapon switch: by the frame the
        // wait is over, the model in hand is the next one.
        if UNKNOWN_WEAPON.fetch_add(1, Ordering::Relaxed) < 10 {
            unsafe {
                crate::debug::report(&format!(
                    "missing_shots: player {slot} fired a round with no fire event, but holds {} -- no fire handler for that, nothing played",
                    held_name
                        .as_deref()
                        .unwrap_or("nothing the engine can name")
                ))
            };
        }
        return;
    };
    let state = &entity.curstate;
    let mut args = EventArgs {
        entindex: slot as i32,
        origin: [state.origin.x, state.origin.y, state.origin.z],
        angles: event_angles([state.angles.x, state.angles.y, state.angles.z]),
        ducking: i32::from(state.usehull == 1),
        fparam1: next_spread(),
        fparam2: next_spread(),
        ..EventArgs::default()
    };
    let played = PLAYED.fetch_add(1, Ordering::Relaxed) + 1;
    // The first few say what is happening; after that the count is enough.
    if played <= 20 || played.is_multiple_of(200) || EVERY_ROUND.load(Ordering::Relaxed) {
        unsafe {
            crate::debug::report(&format!(
                "missing_shots: player {slot} fired a round with no fire event -- playing {} ({played} so far)",
                WEAPONS[weapon].0
            ))
        };
    }
    // Safety: a handler `client.dll` registered for exactly this, called on
    // the engine thread with the argument block it expects.
    let handler: EventHandlerFn = unsafe { std::mem::transmute(handler) };
    STANDING_IN.store(true, Ordering::Relaxed);
    unsafe { handler((&raw mut args).cast()) };
    STANDING_IN.store(false, Ordering::Relaxed);
}

/// One `dodstudio_debug_status` line, once the switch has been on.
pub fn status_line() -> Option<String> {
    let played = PLAYED.load(Ordering::Relaxed);
    (WANTED.load(Ordering::Relaxed) || played != 0).then(|| {
        format!(
            "missing gunshots: {played} played, {} rounds had their own event, {} skipped (weapon not known); {NAME} = {}",
            HAD_EVENT.load(Ordering::Relaxed),
            UNKNOWN_WEAPON.load(Ordering::Relaxed),
            u8::from(WANTED.load(Ordering::Relaxed))
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_script_name_finds_its_weapon() {
        let mp40 = weapon_for_script(b"events/weapons/mp40.sc").unwrap();
        assert_eq!(WEAPONS[mp40].0, "mp40");
        // The scripts with no bullet in them are not in the table.
        assert_eq!(weapon_for_script(b"events/weapons/melee.sc"), None);
        assert_eq!(weapon_for_script(b"events/weapons/bazooka.sc"), None);
        assert_eq!(weapon_for_script(b"events/misc/pain.sc"), None);
    }

    /// Every script named in the table is one the game ships.
    #[test]
    fn every_weapon_is_a_real_script() {
        const SHIPPED: &[&str] = &[
            "30cal",
            "bar",
            "bazooka",
            "bren",
            "colt",
            "enfield",
            "fg42",
            "garand",
            "gewehr",
            "greasegun",
            "k43",
            "kar",
            "knife",
            "luger",
            "m1carbine",
            "melee",
            "mg34",
            "mg42",
            "mortar",
            "mp40",
            "mp44",
            "piat",
            "pschreck",
            "scopedenfield",
            "scopedkar",
            "spring",
            "sten",
            "thompson",
            "webley",
        ];
        for (event, _) in WEAPONS {
            assert!(SHIPPED.contains(event), "{event} is not a shipped script");
        }
    }

    #[test]
    fn a_held_model_finds_its_weapon() {
        let name = |model: &str| weapon_for_held_model(model).map(|i| WEAPONS[i].0);
        assert_eq!(name("models/p_k98.mdl"), Some("kar"));
        assert_eq!(name("models/p_k98s.mdl"), Some("scopedkar"));
        assert_eq!(name("models/p_k98s_l.mdl"), Some("scopedkar"));
        assert_eq!(name("models/p_barbu.mdl"), Some("bar"));
        assert_eq!(name("models/p_stg44.mdl"), Some("mp44"));
        assert_eq!(name("models/p_mg42pr.mdl"), Some("mg42"));
        assert_eq!(name("models/p_enfields.mdl"), Some("scopedenfield"));
        // `p_barricade` starts with "bar" and is not a BAR.
        assert_eq!(name("models/p_barricade.mdl"), None);
        assert_eq!(name("models/p_grenade.mdl"), None);
        assert_eq!(name("models/p_spade.mdl"), None);
        assert_eq!(name("models/v_k98.mdl"), None);
    }

    /// No model stem is claimed by two weapons.
    #[test]
    fn no_held_model_is_listed_twice() {
        let mut seen = std::collections::HashSet::new();
        for (_, held) in WEAPONS {
            for stem in *held {
                assert!(seen.insert(*stem), "{stem} is listed twice");
            }
        }
    }

    #[test]
    fn only_bullet_weapons_count_as_firing() {
        assert!(is_bullet_shoot("stand_mp40_shoot"));
        assert!(is_bullet_shoot("crouch_rifle_shoot"));
        assert!(is_bullet_shoot("prone_30cal_shoot"));
        assert!(is_bullet_shoot("sandbag_mg_shoot"));
        assert!(!is_bullet_shoot("stand_gren_shoot"));
        assert!(!is_bullet_shoot("crouch_stick_shoot"));
        assert!(!is_bullet_shoot("stand_knife_shoot"));
        assert!(!is_bullet_shoot("prone_pschreck_shoot"));
        assert!(!is_bullet_shoot("stand_mp40_aim"));
        assert!(!is_bullet_shoot("stand_bolt_stab"));
        assert!(!is_bullet_shoot("crouch_gren_roll"));
        assert!(!is_bullet_shoot("hs_covering_fire"));
    }

    /// The MP40 sawtooth from a real HLTV half: 3, 34, 58, then back to 3.
    #[test]
    fn a_frame_stepping_back_or_a_new_sequence_is_a_restart() {
        assert!(!restarted((52, 3.0), (52, 34.0)));
        assert!(!restarted((52, 34.0), (52, 58.0)));
        assert!(restarted((52, 58.0), (52, 3.0)));
        // A finished animation sits on 255 until the next round.
        assert!(restarted((52, 255.0), (52, 0.0)));
        assert!(!restarted((52, 255.0), (52, 255.0)));
        assert!(restarted((118, 40.0), (52, 11.0)));
    }

    #[test]
    fn pitch_is_turned_back_from_the_replicated_third() {
        // Looking 30 degrees down is replicated as -10.
        assert_eq!(event_angles([-10.0, 90.0, 0.0]), [30.0, 90.0, 0.0]);
        // Angles arrive unwrapped.
        assert_eq!(event_angles([350.0, 270.0, 0.0]), [30.0, -90.0, 0.0]);
    }

    #[test]
    fn a_shot_is_paired_with_a_restart_one_frame_either_side() {
        assert!(heard_with(10, 10));
        assert!(heard_with(10, 11));
        assert!(!heard_with(10, 12), "an older shot is another round");
        assert!(!heard_with(0, 1), "zero is no shot at all");
    }

    #[test]
    fn the_spread_stays_inside_its_bound() {
        for _ in 0..1000 {
            let s = next_spread();
            assert!((-SPREAD..=SPREAD).contains(&s), "{s}");
        }
    }
}
