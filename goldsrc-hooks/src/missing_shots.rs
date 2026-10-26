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
//! - **Not twice.** `fire_sounds`' `EV_PlaySound` hook reports every real
//!   `_shoot` sample with its entity. A restart waits one frame for its own
//!   event; one that arrives, either side of it, cancels the stand-in.
//!
//! ## One switch, a path per kind of demo
//!
//! [`NAME`] (`ds_sound_restore_gunshots`, default 1; `2` also logs every
//! round played, not only the first twenty) picks what runs from what is
//! playing ([`Playing::paths`]), the way `ds_map_hide_asset` picks its hook
//! from what it hides (#1036):
//!
//! - **HLTV demo** (`IsSpectateOnly()`): this module, every player.
//! - **POV demo, other players:** this module, every player but the
//!   recorder (below).
//! - **POV demo, the recorder:** `pov_recorder_shots`, from the clip count
//!   the server sends him (#1029). His rounds are his client's own predicted
//!   events, which never pass `EV_PlaySound` before the restart is seen.
//! - **Live play:** nothing.
//!
//! It was part of `ds_spec_match_pov` until #1036, which keeps the rest of
//! matching POV (viewmodel animations, crosshair, eye height, gun lowering)
//! and has no say over gunshots any more.
//!
//! ## POV demos: the other players
//!
//! A POV demo loses other players' rounds the same way: about one in seven
//! that the recorder saw fired has no fire event (14.4% of 518,947 rounds in
//! view, 361 demos from 27 recorders, #1036), and in game they play nothing.
//! So while a POV demo plays, the same detection runs for every player but
//! the recorder, with two differences:
//!
//! - **Only players in the latest update.** A POV recording carries only the
//!   players the recorder could see; one out of view keeps the state it was
//!   last sent, and coming back into view would look like a restart. A
//!   player whose `messagenum` is not the newest is forgotten until seen
//!   again.
//! - **A slightly wider window for an event that came first.** Over the same
//!   361 demos, 97.2% of the rounds that kept their event had it in the very
//!   message the restart came in, and 0.2% one message (1-20 ms) before it;
//!   6 of 444,099 came after. So a real shot heard up to
//!   [`POV_HEARD_BEFORE_SECONDS`] before the restart, or on the frame before
//!   it, is that round's (see [`had_its_event`]). Anything further away is a
//!   neighbouring round of the burst (80-200 ms), and does not count.
//!
//! The recorder's own rounds are left alone here: they reach the demo as the
//! client's own predicted events, not from the server, and need another
//! signal, the clip count (`pov_recorder_shots`, #1029).
//!
//! ## What it cannot do
//!
//! - Know the spread: the lost event carried it. A small random one is used,
//!   so a restored round's impact is near, not at, where the real one landed.
//! - Be sure of a Garand's last round. The ping is an argument of the event
//!   (`bparam1` 0), so a restored round has to work it out: the eighth round
//!   since the body's reload or the player's death pings. Across 33 halves
//!   that is the recorded pinging round 377 times of 409, and an earlier
//!   round pings 14 times of 5736. Before the first reload or death is seen,
//!   no restored round pings.
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

/// The one switch for every path (see the module docs). Registered in
/// `commands.rs`; `pov_recorder_shots` reads it through [`level`].
pub const NAME: &str = console_name!("sound_restore_gunshots");

/// How long before its restart a real shot can be heard and still be that
/// round's, in a POV demo. The measured spread is 1-20 ms (see the module
/// docs); a neighbouring round of a burst is 80 ms or more away.
const POV_HEARD_BEFORE_SECONDS: f64 = 0.025;

/// `event_args_t` (`common/event_args.h`), what a fire handler is passed.
/// `pov_recorder_shots` builds them too.
#[repr(C)]
#[derive(Default)]
pub(crate) struct EventArgs {
    pub(crate) flags: i32,
    pub(crate) entindex: i32,
    pub(crate) origin: [f32; 3],
    pub(crate) angles: [f32; 3],
    pub(crate) velocity: [f32; 3],
    pub(crate) ducking: i32,
    pub(crate) fparam1: f32,
    pub(crate) fparam2: f32,
    pub(crate) iparam1: i32,
    pub(crate) iparam2: i32,
    pub(crate) bparam1: i32,
    pub(crate) bparam2: i32,
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

/// The body-animation token each of [`WEAPONS`] fires with
/// (`<stance>_<token>_shoot`), from the `szAnimExt` column of the game's
/// weapon table (dod13-client `dlls/dod_shared.cpp`, `WpnInfo`). Not the
/// script name: the Garand, K43, BAR and M1 carbine all fire as `rifle`, the
/// folding carbine as `fcarb`, the FG42 as `tommy`, the bolt rifles as `bolt`.
fn fires_as(event: &str) -> &'static [&'static str] {
    match event {
        "30cal" => &["30cal"],
        "bar" | "garand" | "k43" => &["rifle"],
        "m1carbine" => &["rifle", "fcarb"],
        "bren" => &["bren"],
        "colt" | "luger" | "webley" => &["pistol"],
        "enfield" | "scopedenfield" | "kar" | "scopedkar" | "spring" => &["bolt"],
        "fg42" | "thompson" => &["tommy"],
        "greasegun" => &["grease"],
        "mg34" | "mg42" => &["mg"],
        "mp40" => &["mp40"],
        "mp44" => &["stg44"],
        "sten" => &["sten"],
        _ => &[],
    }
}

/// The token of a `<stance>_<token>_shoot` body sequence.
fn shoot_token(label: &str) -> Option<&str> {
    label
        .strip_suffix("_shoot")?
        .split_once('_')
        .map(|(_, token)| token)
}

/// Which weapon fired a round, from the body sequence that fired it and the
/// last two weapons the player held (as [`WEAPONS`] indices, newest first).
///
/// The newest isn't always it: a player who fires and switches at once (the
/// K98's quick switch to the pistol, to skip the bolt) can have the next
/// weapon in hand by the update the shot arrives in, or the one after.
/// Falls back to the newest when neither fires that way.
fn fired_with(label: &str, held: [Option<usize>; 2]) -> Option<usize> {
    let token = shoot_token(label);
    held.into_iter()
        .flatten()
        .find(|&weapon| token.is_some_and(|t| fires_as(WEAPONS[weapon].0).contains(&t)))
        .or(held[0])
}

/// The handler `client.dll` registered for each of [`WEAPONS`], or null.
static HANDLERS: [AtomicPtr<c_void>; WEAPONS.len()] =
    [const { AtomicPtr::new(std::ptr::null_mut()) }; WEAPONS.len()];

/// What a recorded event for an ordinary round of a weapon carries as
/// `[iparam1, bparam1, bparam2]`, from every fire event in 33 HLTV halves
/// (`hltv_shot_evidence_probe` prints the table). Zero is not "ordinary" for
/// all of them, and the handlers act on the difference:
///
/// - Garand, K43, M1 carbine: `bparam1` is 1 on every round but the last
///   (5971 of 6384 Garand shots). On the Garand, 0 plays the clip ping --
///   `client.dll+0x7c2e`, `test ebp, ebp; jne` past the `clipding` sample.
/// - Scoped K98: `bparam2` is 1 on three shots in four.
/// - BAR: `iparam1` is the rounds left in the magazine, 19 down to 0. The
///   MG42 and .30 cal read it the same way (belt rounds left, for the
///   first-person belt); no recorded event for either was available, so
///   theirs follows the BAR's.
/// - Everything else: all zero (pistols and the bolt rifles set `bparam1`
///   on the last round instead).
fn ordinary_args(event: &str) -> [i32; 3] {
    match event {
        "garand" | "k43" | "m1carbine" => [0, 1, 0],
        "scopedkar" => [0, 0, 1],
        "bar" | "mg42" | "30cal" => [10, 0, 0],
        _ => [0, 0, 0],
    }
}

/// Rounds in a Garand clip. The last one pings.
const GARAND_CLIP: i32 = 8;

/// Whether a body sequence starting means the Garand in hand next has a full
/// clip: its reload, or the player dying (the respawn brings a fresh one).
fn refills_the_clip(label: &str) -> bool {
    label.ends_with("_garand_reload") || label.starts_with("die_") || label.starts_with("dead")
}

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
/// [`NAME`] `2`: a log line for every round, not only the first few. For
/// checking a session against the offline probe.
static EVERY_ROUND: AtomicBool = AtomicBool::new(false);
/// [`NAME`] when it could not be a cvar: the fallback command's value. On,
/// as the cvar's default is.
pub static WITHOUT_A_CVAR: AtomicBool = AtomicBool::new(true);

/// [`NAME`]'s value: its cvar, or the fallback toggle without one. `0` off,
/// `1` on, `2` on and logging every round, on every path.
pub(crate) fn level() -> f32 {
    let cvar = CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        f32::from(u8::from(WITHOUT_A_CVAR.load(Ordering::Relaxed)))
    } else {
        // Safety: the engine owns the cvar for the session.
        unsafe { (*cvar).value }
    }
}

/// Whether [`NAME`] is on, for `ds_debug_status`.
pub fn enabled() -> bool {
    level() != 0.0
}

/// Whether a [`level`] asks for a log line for every round.
pub(crate) fn logs_every_round(level: f32) -> bool {
    level >= 2.0
}

/// What kind of demo is playing, which decides the paths and the players.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Playing {
    /// Live play, no demo, or a POV demo whose recorder can't be named.
    Nothing,
    /// An HLTV demo.
    Hltv,
    /// A POV demo recorded by player `recorder`.
    Pov { recorder: usize },
}

/// Which restorations run for a kind of demo, with [`NAME`] on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Paths {
    /// This module's body-restart path, for the players [`Playing::tracks`]
    /// names.
    pub(crate) bodies: bool,
    /// `pov_recorder_shots`' clip-drop path, for the recorder.
    pub(crate) recorders_clip: bool,
}

impl Playing {
    /// For noticing a change between frames.
    fn code(self) -> i32 {
        match self {
            Playing::Nothing => 0,
            Playing::Hltv => 1,
            Playing::Pov { recorder } => 100 + recorder as i32,
        }
    }

    /// The paths this kind of demo runs (the module docs say why).
    pub(crate) fn paths(self) -> Paths {
        match self {
            Playing::Nothing => Paths {
                bodies: false,
                recorders_clip: false,
            },
            Playing::Hltv => Paths {
                bodies: true,
                recorders_clip: false,
            },
            Playing::Pov { .. } => Paths {
                bodies: true,
                recorders_clip: true,
            },
        }
    }

    /// Whether player `slot`'s rounds are the body path's to restore.
    fn tracks(self, slot: usize) -> bool {
        match self {
            Playing::Nothing => false,
            Playing::Hltv => true,
            // The recorder's own rounds are the client's predicted events,
            // which this signal can't pair: `pov_recorder_shots`' (#1029).
            Playing::Pov { recorder } => slot != recorder,
        }
    }

    /// The POV recorder whose rounds the clip-drop path restores, if any.
    pub(crate) fn clip_recorder(self) -> Option<usize> {
        match self {
            Playing::Pov { recorder } if self.paths().recorders_clip => Some(recorder),
            _ => None,
        }
    }
}

static PLAYING: AtomicI32 = AtomicI32::new(0);

/// Whether a restart seen on frame `now` (at playback time `now_at`) came
/// with its own fire event: a real shot of that player's heard on frame
/// `heard` (zero for none) at `heard_at`. A restart this says no to waits one
/// more frame for an event behind it (`on_shot_sound`), then gets its
/// stand-in.
///
/// Either side of one frame, as in an HLTV demo; in a POV demo also anything
/// up to [`POV_HEARD_BEFORE_SECONDS`] before, the measured spread of an event
/// that came a message ahead of its restart.
fn had_its_event(playing: Playing, heard: u32, heard_at: f64, now: u32, now_at: f64) -> bool {
    if heard_with(heard, now) {
        return true;
    }
    matches!(playing, Playing::Pov { .. })
        && heard != 0
        && (0.0..=POV_HEARD_BEFORE_SECONDS).contains(&(now_at - heard_at))
}

/// Whether a player is in the update the client last read: an entity left
/// out of it keeps the state it was last sent (`messagenum` is the update it
/// came in).
fn in_latest_update(messagenum: i32, latest: i32) -> bool {
    messagenum == latest
}

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
/// The playback time (`engine::client_time`, as bits) [`HEARD`] was set at.
static HEARD_AT: [AtomicU64; SLOTS] = [const { AtomicU64::new(0) }; SLOTS];
/// The weapon each player holds, and the one before it, as [`WEAPONS`]
/// indices, or -1.
static HELD_NOW: [AtomicI32; SLOTS] = [const { AtomicI32::new(-1) }; SLOTS];
static HELD_BEFORE: [AtomicI32; SLOTS] = [const { AtomicI32::new(-1) }; SLOTS];
/// The weapon a pending restart fired with, chosen when it was seen.
static PENDING_WEAPON: [AtomicI32; SLOTS] = [const { AtomicI32::new(-1) }; SLOTS];
/// How many Garand rounds each player has fired since the clip was last
/// known full, or -1 while that is not known.
static CLIP_ROUND: [AtomicI32; SLOTS] = [const { AtomicI32::new(-1) }; SLOTS];

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
pub(crate) fn event_angles(state: [f32; 3]) -> [f32; 3] {
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
pub(crate) fn next_spread() -> f32 {
    let mut x = SPREAD_SEED.load(Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    SPREAD_SEED.store(x, Ordering::Relaxed);
    ((x >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0) * SPREAD
}

/// Calls `client.dll`'s own fire handler for the `event` script (a name in
/// [`WEAPONS`]) with `args`, as a stand-in: the sound it makes is not taken
/// for a recorded round, here or in `pov_recorder_shots`. False when the client
/// registered no handler for that script.
pub(crate) fn play_fire_event(event: &str, args: &mut EventArgs) -> bool {
    let Some(weapon) = WEAPONS.iter().position(|(e, _)| *e == event) else {
        return false;
    };
    let handler = HANDLERS[weapon].load(Ordering::Acquire);
    if handler.is_null() {
        return false;
    }
    // Safety: a handler `client.dll` registered for exactly this script,
    // called on the engine thread with the argument block it expects.
    let handler: EventHandlerFn = unsafe { std::mem::transmute(handler) };
    STANDING_IN.store(true, Ordering::Relaxed);
    unsafe { handler((args as *mut EventArgs).cast()) };
    STANDING_IN.store(false, Ordering::Relaxed);
    true
}

/// Whether `event` names one of [`WEAPONS`].
#[cfg(test)]
pub(crate) fn is_known_script(event: &str) -> bool {
    WEAPONS.iter().any(|(e, _)| *e == event)
}

/// Whether a stand-in's handler is running right now.
pub(crate) fn standing_in() -> bool {
    STANDING_IN.load(Ordering::Relaxed)
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
/// `fire_sounds`' `EV_PlaySound` hook, on the engine thread.
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
    HEARD_AT[slot].store(engine::client_time().to_bits(), Ordering::Relaxed);
}

fn forget(slot: usize) {
    SEEN[slot].store(false, Ordering::Relaxed);
    PENDING[slot].store(0, Ordering::Relaxed);
    HEARD[slot].store(0, Ordering::Relaxed);
    CLIP_ROUND[slot].store(-1, Ordering::Relaxed);
    HELD_NOW[slot].store(-1, Ordering::Relaxed);
    HELD_BEFORE[slot].store(-1, Ordering::Relaxed);
    PENDING_WEAPON[slot].store(-1, Ordering::Relaxed);
}

fn forget_everyone() {
    for slot in 1..SLOTS {
        forget(slot);
    }
}

/// What is playing now. For `pov_recorder_shots` too, whose callbacks run
/// between frames.
pub(crate) fn playing_now() -> Playing {
    engine::engfuncs().map_or(Playing::Nothing, playing)
}

fn playing(engfuncs: &ClEngineFuncsPartial) -> Playing {
    let api = engfuncs.p_demo_api;
    if api.is_null() || unsafe { ((*api).is_playingback)() } == 0 {
        return Playing::Nothing;
    }
    if unsafe { (engfuncs.is_spectate_only)() } != 0 {
        return Playing::Hltv;
    }
    // Safety: the engine's own accessor; a demo is playing, so a level is
    // loaded, and the pointer is checked before it is read.
    let local = unsafe { (engfuncs.get_local_player)() } as usize;
    if local == 0 || !crate::crash::readable(local, 4) {
        return Playing::Nothing;
    }
    // Safety: `cl_entity_t` starts with its index, readable as checked.
    let recorder = unsafe { *(local as *const i32) };
    usize::try_from(recorder)
        .ok()
        .filter(|i| (1..SLOTS).contains(i))
        .map_or(Playing::Nothing, |recorder| Playing::Pov { recorder })
}

/// The newest update any player came in, in a POV demo: the recorder's own
/// entity is in every one.
fn latest_update(engfuncs: &ClEngineFuncsPartial) -> i32 {
    (1..SLOTS)
        .filter_map(|slot| {
            // Safety: the engine's own accessor, null for an empty slot.
            let entity = unsafe { (engfuncs.get_entity_by_index)(slot as i32).as_ref() }?;
            (entity.player != 0).then_some(entity.curstate.messagenum)
        })
        .max()
        .unwrap_or(0)
}

/// Called every frame from `commands::poll`.
pub fn poll() {
    let playing = playing_now();
    let value = level();
    let wanted = value != 0.0;
    EVERY_ROUND.store(logs_every_round(value), Ordering::Relaxed);
    let switched = WANTED.swap(wanted, Ordering::Relaxed) != wanted;
    if switched {
        unsafe {
            crate::debug::report(&format!(
                "missing_shots: {} ({NAME})",
                if wanted {
                    "restoring lost gunshots"
                } else {
                    "off"
                }
            ))
        };
    }
    // A different kind of demo, or another recorder: nothing tracked carries.
    let changed = PLAYING.swap(playing.code(), Ordering::Relaxed) != playing.code();
    if switched || changed {
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
    if !playing.paths().bodies || now - last > TRACKING_GAP_SECONDS {
        forget_everyone();
        return;
    }
    let frame_number = FRAME_NUMBER.fetch_add(1, Ordering::Relaxed) + 1;
    let pov = matches!(playing, Playing::Pov { .. });
    let latest = if pov { latest_update(engfuncs) } else { 0 };

    for slot in 1..SLOTS {
        if !playing.tracks(slot) {
            forget(slot);
            continue;
        }
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
        // Out of the recorder's view: what it shows is old, and coming back
        // must not look like a restart.
        if pov && !in_latest_update(entity.curstate.messagenum, latest) {
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

        // Safety: null for an index the engine has not precached.
        let held_model = unsafe { (studio.get_model_by_index)(state.weaponmodel) };
        let held_weapon = (!held_model.is_null())
            // Safety: a non-null model from the engine's own lookup.
            .then(|| weapon_for_held_model(&unsafe { (*held_model).name_str() }))
            .flatten()
            .map_or(-1, |weapon| weapon as i32);
        let previous = HELD_NOW[slot].swap(held_weapon, Ordering::Relaxed);
        if previous != held_weapon && previous >= 0 {
            HELD_BEFORE[slot].store(previous, Ordering::Relaxed);
        }

        if before.is_some() && state.msg_time < earlier {
            // The demo went backwards: a seek. Nothing follows from before it.
            PENDING[slot].store(0, Ordering::Relaxed);
            HEARD[slot].store(0, Ordering::Relaxed);
            CLIP_ROUND[slot].store(-1, Ordering::Relaxed);
            continue;
        }

        // A restart that has waited a frame and heard nothing lost its event.
        // Before this frame's own restart is looked at, so a round seen now
        // cannot take the place of one still waiting.
        // Anything pending here was set by an earlier frame's pass.
        if PENDING[slot].swap(0, Ordering::Relaxed) != 0 {
            let weapon = usize::try_from(PENDING_WEAPON[slot].swap(-1, Ordering::Relaxed)).ok();
            stand_in(slot, entity, weapon, held_model);
        }

        if let Some(before) = before
            && restarted(before, now_body)
        {
            // Safety: null for an index the engine has not precached, which
            // `sequence_label` handles.
            let body = unsafe { (studio.get_model_by_index)(state.modelindex) };
            let label = usize::try_from(now_body.0)
                .ok()
                .and_then(|sequence| crate::anim_fix::sequence_label(body, sequence));
            let label = label.as_deref().unwrap_or("");
            if before.0 != now_body.0 && refills_the_clip(label) {
                CLIP_ROUND[slot].store(0, Ordering::Relaxed);
            }
            if is_bullet_shoot(label) {
                // Every Garand round counts towards the clip, recorded or not.
                // Safety: null for an index the engine has not precached.
                let held = unsafe { (studio.get_model_by_index)(state.weaponmodel) };
                if holds(held, "garand") {
                    let round = CLIP_ROUND[slot].load(Ordering::Relaxed);
                    if round >= 0 {
                        CLIP_ROUND[slot].store(round + 1, Ordering::Relaxed);
                    }
                }
                let heard_at = f64::from_bits(HEARD_AT[slot].load(Ordering::Relaxed));
                if had_its_event(
                    playing,
                    HEARD[slot].swap(0, Ordering::Relaxed),
                    heard_at,
                    frame_number,
                    now,
                ) {
                    HAD_EVENT.fetch_add(1, Ordering::Relaxed);
                } else {
                    let held = [
                        HELD_NOW[slot].load(Ordering::Relaxed),
                        HELD_BEFORE[slot].load(Ordering::Relaxed),
                    ]
                    .map(|weapon| usize::try_from(weapon).ok());
                    let weapon = fired_with(label, held).map_or(-1, |weapon| weapon as i32);
                    PENDING_WEAPON[slot].store(weapon, Ordering::Relaxed);
                    PENDING[slot].store(frame_number, Ordering::Relaxed);
                }
            }
        }
    }
}

/// Whether `held`, a third-person weapon model, fires the `event` script.
fn holds(held: *mut ModelSPartial, event: &str) -> bool {
    !held.is_null()
        // Safety: a non-null model from the engine's own lookup.
        && weapon_for_held_model(&unsafe { (*held).name_str() })
            .is_some_and(|weapon| WEAPONS[weapon].0 == event)
}

/// Calls the weapon's own fire handler for a round whose event was lost:
/// `weapon` if the shot named one, else whatever `held` is.
fn stand_in(slot: usize, entity: &ClEntityS, weapon: Option<usize>, held: *mut ModelSPartial) {
    let held_name = (!held.is_null())
        // Safety: a non-null model from the engine's own lookup.
        .then(|| unsafe { (*held).name_str() });
    let handler = weapon
        .or_else(|| held_name.as_deref().and_then(weapon_for_held_model))
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
    let event = WEAPONS[weapon].0;
    let [iparam1, mut bparam1, bparam2] = ordinary_args(event);
    let last_of_the_clip =
        event == "garand" && CLIP_ROUND[slot].load(Ordering::Relaxed) == GARAND_CLIP;
    if last_of_the_clip {
        bparam1 = 0;
    }
    let mut args = EventArgs {
        entindex: slot as i32,
        origin: [state.origin.x, state.origin.y, state.origin.z],
        angles: event_angles([state.angles.x, state.angles.y, state.angles.z]),
        ducking: i32::from(state.usehull == 1),
        fparam1: next_spread(),
        fparam2: next_spread(),
        iparam1,
        bparam1,
        bparam2,
        ..EventArgs::default()
    };
    let played = PLAYED.fetch_add(1, Ordering::Relaxed) + 1;
    // The first few say what is happening; after that the count is enough.
    if played <= 20 || played.is_multiple_of(200) || EVERY_ROUND.load(Ordering::Relaxed) {
        unsafe {
            crate::debug::report(&format!(
                "missing_shots: player {slot} fired a round with no fire event -- playing {event}{} ({played} so far)",
                if last_of_the_clip {
                    ", the clip's last"
                } else {
                    ""
                }
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

/// One `ds_debug_status` line, once the switch has been on.
pub fn status_line() -> Option<String> {
    let played = PLAYED.load(Ordering::Relaxed);
    (WANTED.load(Ordering::Relaxed) || played != 0).then(|| {
        format!(
            "missing gunshots: {played} played, {} rounds had their own event, {} skipped (weapon not known)",
            HAD_EVENT.load(Ordering::Relaxed),
            UNKNOWN_WEAPON.load(Ordering::Relaxed),
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

    /// element's quick switch in `monday-wsod25_r07_m1_h1_hltv`: a K98 round
    /// whose update already has the Luger in hand plays the K98, not the
    /// Luger.
    #[test]
    fn a_round_fired_just_before_a_switch_is_the_old_weapons() {
        let index = |event: &str| WEAPONS.iter().position(|(e, _)| *e == event);
        let (kar, luger) = (index("kar"), index("luger"));
        assert_eq!(fired_with("stand_bolt_shoot", [luger, kar]), kar);
        assert_eq!(fired_with("stand_bolt_shoot", [kar, luger]), kar);
        assert_eq!(fired_with("crouch_pistol_shoot", [luger, kar]), luger);
        // Nothing fires as `rifle` here: the weapon in hand it is.
        assert_eq!(fired_with("stand_rifle_shoot", [luger, kar]), luger);
        // The M1 carbine fires as `rifle` held one way and `fcarb` the other.
        let carbine = index("m1carbine");
        assert_eq!(fired_with("prone_fcarb_shoot", [luger, carbine]), carbine);
        assert_eq!(
            fired_with("bipod_tommy_shoot", [luger, index("fg42")]),
            index("fg42")
        );
    }

    /// Every weapon fires as some body token, and every token the player
    /// model has for a bullet weapon belongs to one.
    #[test]
    fn every_weapon_has_its_body_token() {
        for (event, _) in WEAPONS {
            assert!(!fires_as(event).is_empty(), "{event}");
        }
        for token in [
            "rifle", "bolt", "tommy", "fcarb", "mp40", "stg44", "sten", "grease", "pistol", "mg",
            "bren", "30cal",
        ] {
            assert!(
                WEAPONS
                    .iter()
                    .any(|(event, _)| fires_as(event).contains(&token)),
                "{token}"
            );
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

    /// An all-zero Garand event is the round that pings, which is how every
    /// restored Garand round came to ping.
    #[test]
    fn an_ordinary_round_carries_what_a_recorded_one_does() {
        assert_eq!(ordinary_args("garand"), [0, 1, 0]);
        assert_eq!(ordinary_args("k43"), [0, 1, 0]);
        assert_eq!(ordinary_args("m1carbine"), [0, 1, 0]);
        assert_eq!(ordinary_args("scopedkar"), [0, 0, 1]);
        assert_eq!(ordinary_args("bar")[0], 10);
        assert_eq!(ordinary_args("mp40"), [0, 0, 0]);
        assert_eq!(ordinary_args("colt"), [0, 0, 0]);
        assert_eq!(ordinary_args("kar"), [0, 0, 0]);
        // Every name it knows is a weapon in the table.
        for event in [
            "garand",
            "k43",
            "m1carbine",
            "scopedkar",
            "bar",
            "mg42",
            "30cal",
        ] {
            assert!(WEAPONS.iter().any(|(e, _)| *e == event), "{event}");
        }
    }

    #[test]
    fn a_reload_or_a_death_means_a_full_clip() {
        assert!(refills_the_clip("stand_garand_reload"));
        assert!(refills_the_clip("prone_garand_reload"));
        assert!(refills_the_clip("die_headshot"));
        assert!(refills_the_clip("deadback"));
        assert!(!refills_the_clip("stand_k43_reload"));
        assert!(!refills_the_clip("stand_pistol_reload"));
        assert!(!refills_the_clip("stand_rifle_shoot"));
        assert!(!refills_the_clip(""));
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

    /// An HLTV demo pairs by frames alone, as it always has: a shot heard
    /// three frames back is another round, however close in time.
    #[test]
    fn hltv_pairs_a_round_with_its_event_by_frames_alone() {
        let hltv = Playing::Hltv;
        assert!(had_its_event(hltv, 10, 5.000, 10, 5.000));
        assert!(had_its_event(hltv, 10, 5.000, 11, 5.004));
        assert!(!had_its_event(hltv, 10, 5.000, 13, 5.012));
        assert!(!had_its_event(hltv, 0, 0.0, 13, 0.010), "no shot heard");
    }

    /// The measured POV spread: 97.2% of events in the restart's own message,
    /// 0.2% one message (1-20 ms) ahead of it, the next round of a burst
    /// 80 ms or more away.
    #[test]
    fn pov_pairs_an_event_up_to_one_message_ahead_and_no_further() {
        let pov = Playing::Pov { recorder: 11 };
        // Same message, read just before or just after.
        assert!(had_its_event(pov, 40, 12.000, 40, 12.000));
        assert!(had_its_event(pov, 40, 12.000, 41, 12.007));
        // A message ahead, at 300 fps: six frames, 20 ms.
        assert!(had_its_event(pov, 40, 12.000, 46, 12.020));
        assert!(had_its_event(pov, 40, 12.000, 47, 12.024));
        // The burst's previous round, 100 ms back: this round lost its event.
        assert!(!had_its_event(pov, 40, 12.000, 70, 12.100));
        assert!(!had_its_event(pov, 40, 12.000, 50, 12.030));
        // No shot heard at all, whatever the clock says.
        assert!(!had_its_event(pov, 0, 0.0, 3, 0.010));
        // A time from before a seek back is not "just before".
        assert!(!had_its_event(pov, 40, 12.000, 45, 11.990));
    }

    /// The recorder's rounds are left to #1029's path; everyone else's are
    /// restored in a POV demo, and everyone's in an HLTV one.
    #[test]
    fn pov_leaves_the_recorder_alone() {
        let pov = Playing::Pov { recorder: 11 };
        assert!(!pov.tracks(11));
        assert!(pov.tracks(10));
        assert!(pov.tracks(13));
        assert!(Playing::Hltv.tracks(11));
        assert!(!Playing::Nothing.tracks(11));
    }

    /// #1036's routing: an HLTV demo restores every player from the body,
    /// a POV demo the other players from the body and the recorder from his
    /// clip, live play nothing.
    #[test]
    fn each_kind_of_demo_runs_its_own_paths() {
        let paths = |bodies, recorders_clip| Paths {
            bodies,
            recorders_clip,
        };
        assert_eq!(Playing::Hltv.paths(), paths(true, false));
        assert_eq!(Playing::Pov { recorder: 11 }.paths(), paths(true, true));
        assert_eq!(Playing::Nothing.paths(), paths(false, false));
        // The clip path is the recorder's, and only in a POV demo.
        assert_eq!(Playing::Pov { recorder: 11 }.clip_recorder(), Some(11));
        assert_eq!(Playing::Hltv.clip_recorder(), None);
        assert_eq!(Playing::Nothing.clip_recorder(), None);
    }

    /// One cvar for every path; `ds_spec_match_pov` decides none of them.
    #[test]
    fn one_switch_for_every_path() {
        assert_eq!(NAME, "ds_sound_restore_gunshots");
        assert_ne!(NAME, crate::names::SPEC_MATCH_POV);
        assert!(!logs_every_round(0.0));
        assert!(!logs_every_round(1.0));
        assert!(logs_every_round(2.0));
    }

    /// Without a cvar the setting is on, as the cvar's default is.
    #[test]
    fn the_fallback_starts_on() {
        assert!(WITHOUT_A_CVAR.load(Ordering::Relaxed));
    }

    #[test]
    fn a_change_of_recorder_is_a_change_of_demo() {
        assert_ne!(
            Playing::Pov { recorder: 3 }.code(),
            Playing::Pov { recorder: 4 }.code()
        );
        assert_ne!(Playing::Hltv.code(), Playing::Nothing.code());
    }

    #[test]
    fn a_player_left_out_of_the_update_is_not_watched() {
        assert!(in_latest_update(812, 812));
        assert!(!in_latest_update(790, 812));
    }

    #[test]
    fn the_spread_stays_inside_its_bound() {
        for _ in 0..1000 {
            let s = next_spread();
            assert!((-SPREAD..=SPREAD).contains(&s), "{s}");
        }
    }
}
