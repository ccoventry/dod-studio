//! Drives the first-person viewmodel's animations while spectating a player
//! in-eye, which the engine otherwise leaves largely static.
//!
//! ## Why the animations are missing
//!
//! Structural, not a bug. GoldSrc's weapon event scripts animate the viewmodel
//! only for the **local** player -- the `EV_IsLocal` check in every
//! `events/weapons/*.sc`. Every other player gets the sound and the muzzle
//! flash but no first-person animation, because normally nobody is looking
//! down their sights. Spectating in-eye, and HLTV playback in particular, is
//! exactly the case that assumption does not hold for: the viewmodel on screen
//! belongs to someone who is not the local player, so firing, reloading and
//! drawing never reach it.
//!
//! ## What this drives, and from where
//!
//! - **shoot** -- from the spectated player's own body animation. DoD's player
//!   models name every sequence `<stance>_<weapon>_<action>`, so
//!   `stand_bolt_shoot` or `bipod_mg_shoot` says outright that they are firing,
//!   and that is replicated entity state present in an HLTV demo. The
//!   weapon-fire *sound* is kept as a second trigger (`on_weapon_fired`), which
//!   matters only for automatic fire: a held trigger leaves the body sequence
//!   sitting on the same `_shoot` label, so the individual rounds after the
//!   first have no sequence change to key off.
//! - **pin pull, then throw** -- a grenade's firing sequence
//!   (`stand_gren_shoot`, `crouch_stick_roll`, ...) marks the button's
//!   *release*; the server holds the grenade another 0.5s before it leaves
//!   the hand, and the pull itself is never networked. So the body change
//!   plays `pinpull` and books `throw` for `GRENADE_WINDUP_SECONDS` later
//!   (`dodstudio_hltv_grenade_pinpull`, default on; off plays `throw` at the
//!   body change as it used to).
//! - **reload** -- from the spectated player's body animation as well
//!   (`crouch_bar_reload`, `prone_webley_reload`, ...).
//! - **draw** -- when the viewmodel *settles* on a different weapon. Not
//!   simply when it changes: the viewmodel rotates through several of a
//!   player's weapons many times a second, so a bare change test starts a draw
//!   ten times a second and none of them survive long enough to be seen.
//! - **idle** -- on switching to a different spectated player, so the new
//!   viewmodel does not inherit whatever sequence the last one was left on.
//!
//! ## Bipod weapons, additionally
//!
//! MG42, MG34, BAR and Bren keep two parallel sequence families in one model
//! -- "up" (hip-fire) and "down" (deployed), e.g. `upidle`/`downidle` -- and
//! nothing tells a puppeted in-eye viewmodel which to use. Unlike Counter-
//! Strike's silenced/unsilenced M4A1 fix this was modeled on, DoD's deploy
//! state is not hidden in a fire-event side channel: it drives a real,
//! always-replicated `entity_state_t::weaponmodel` swap between two
//! third-person models (`p_mg42bu.mdl` <-> `p_mg42bd.mdl`), so it can be read
//! directly each frame. Every animation above is then looked up within
//! whichever family is current. Confirmed against the `.mdl` files shipped
//! with DoD 1.3.
//!
//! This part matters far less in practice than the plain animations: league
//! configs generally limit the MGs to zero, and deploying the BAR's bipod is
//! rare. It is a refinement on top, not the point.
//!
//! ## Where the rest of the reasoning lives
//!
//! `docs/goldsrc_hltv_animation_fix.md` is the design write-up: the measurement
//! that justifies inferring at all, the per-frame stage order, the two firing
//! triggers and the window between them, the four mistakes worth not repeating,
//! and how to check a session. This module's comments explain each decision at
//! its own call site; the document explains the shape.
//!
//! Originally ported from a prototype written against HLAE's own source
//! (`AfxHookGoldSrc/hooks/client/dod/ViewmodelAnimationFix.cpp`), adapted to
//! the engine interfaces this crate captures itself.

mod classify;
mod sequences;
mod trace;

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU64, Ordering};

use classify::{
    ATTACK_SEQUENCES, BodyAction, DeployState, DeployableWeapon, classify_body_sequence,
    deploy_state_from_body_sequence, find_deployable_weapon, is_grenade_viewmodel, is_throw_label,
    model_stem, third_person_stem,
};
pub(crate) use sequences::sequence_label;
use sequences::{animation_lookup_any, animation_lookup_sequence, model_sequence_duration};
pub use trace::{LOG_HELD_MODELS, status};
use trace::{
    STAGE_DISABLED, STAGE_NO_ENGFUNCS, STAGE_NO_SPECTATED_PLAYER, STAGE_NO_VIEWMODEL_ENTITY,
    STAGE_NO_VIEWMODEL_MODEL, STAGE_NOT_SPECTATING, STAGE_RUNNING, STAGE_VIEWMODEL_MISMATCH,
    describe_player, note_held_model, note_unmatched_pair, note_viewmodel, stage, stage_with,
};

use crate::engine::{self, ClEntityS, ModelSPartial};

/// Which answer to the emptied-hand problem is in force.
///
/// A grenade throw legitimately empties the hand -- `v_stick`'s `throw` is two
/// frames, 0.050s, and the pose it ends on holds nothing. What is *not*
/// settled is what should happen next, because the signal that would settle it
/// is missing: a spectator cannot see the pin pull, and the replicated
/// `weaponmodel` lags the thrower's own client badly. Measured on one capture,
/// the server still claimed he held the grenade four seconds after he threw
/// it; his own client had drawn the next weapon 1.25s in.
///
/// So this is a genuine choice between imperfect options rather than a
/// difficulty being deferred, and the numbers exist so they can be compared in
/// one session instead of across four rebuilds.
///
/// | value | behaviour | its flaw |
/// | --- | --- | --- |
/// | 0 | fix off entirely | no animations at all |
/// | 1 | leave the hand empty | empty for as long as the server lags -- 3.5s, measured |
/// | 2 | draw what is held as soon as the throw ends | shows a grenade he may already have put away |
/// | 3 | never play the throw, so the grenade stays in hand | no throw animation, and the grenade lingers |
/// | 4 | wait, then draw what is held | as 2, but gives the server time to catch up first |
///
/// Everything else the fix does -- drawing on the leading edge, not drawing
/// when the camera changes player, not swallowing a weapon flashed past -- is
/// settled and unconditional. Those were measured against captures and fixed;
/// they are not options.
pub static LEVEL: AtomicI32 = AtomicI32::new(0);

pub const LEVEL_OFF: i32 = 0;
/// Play the throw and leave the hand empty until something else draws.
pub const LEVEL_EMPTY_HAND: i32 = 1;
/// Draw whatever the replicated state says is held, the moment the throw ends.
pub const LEVEL_REDRAW_NOW: i32 = 2;
/// Never empty the hand: skip the throw animation for grenades entirely.
pub const LEVEL_NEVER_EMPTY: i32 = 3;
/// Draw what is held, but only after `LOOKAHEAD_SECONDS`.
pub const LEVEL_LOOKAHEAD: i32 = 4;
pub const LEVEL_MAX: i32 = LEVEL_LOOKAHEAD;

/// How long option 4 waits before drawing.
///
/// Long enough to be worth waiting for -- the one measurement available puts
/// the thrower's own client at 1.25s -- and short enough that the hand is not
/// empty for anything like the 3.5s option 1 produced.
const LOOKAHEAD_SECONDS: f64 = 1.0;

/// What each option is, for `dodstudio_debug_status` and the startup line.
pub fn level_description(level: i32) -> &'static str {
    match level {
        LEVEL_OFF => "off",
        LEVEL_EMPTY_HAND => "throw empties the hand, left empty",
        LEVEL_REDRAW_NOW => "draw what is held as soon as the throw ends",
        LEVEL_NEVER_EMPTY => "no throw animation, grenade stays in hand",
        _ => "draw what is held, after a 1s wait",
    }
}

pub fn level() -> i32 {
    LEVEL.load(Ordering::Relaxed).clamp(LEVEL_OFF, LEVEL_MAX)
}

pub fn enabled() -> bool {
    level() > LEVEL_OFF
}

/// `dodstudio_hltv_grenade_pinpull`: whether a grenade's body-sequence change
/// plays `pinpull` and schedules `throw` for `GRENADE_WINDUP_SECONDS` later
/// (on), or plays `throw` on the spot as it used to (off).
///
/// A sub-option of the fix, which stays off by default; this one defaults on
/// because the previous timing emptied the hand half a second before the
/// grenade was thrown. The body change is the *release* of the button -- the
/// server's `WeaponIdle` sets the body sequence and `m_flStartThrow = time +
/// 0.5` in the same call, and the grenade entity and `weapons/grenthrow.wav`
/// follow 0.461-0.566s later (median 0.494s over 846 throws). The real pin
/// pull is never networked, so `pinpull` here is the nearest stand-in for a
/// moment that cannot be seen, not a reading of it. See the note above
/// `is_grenade_viewmodel` in `classify.rs`.
pub static GRENADE_PINPULL: AtomicBool = AtomicBool::new(true);

/// How long the server holds a released grenade before it leaves the hand:
/// `m_flStartThrow = gpGlobals->time + 0.5` in `dod.dll`'s `WeaponIdle`
/// (hand `0x9c66`, stick `0x15296`, the same double constant at `0xcf5c8`).
/// The primed `_ex` classes use 0.3s, but nothing in an HLTV stream tells
/// them apart, so they get this value too.
pub const GRENADE_WINDUP_SECONDS: f64 = 0.5;

/// What the pin-pull option is doing, for `dodstudio_debug_status` and the
/// startup line.
pub fn grenade_pinpull_description() -> &'static str {
    if GRENADE_PINPULL.load(Ordering::Relaxed) {
        "1 (pin pull at the wind-up, throw 0.5s later)"
    } else {
        "0 (throw at the wind-up)"
    }
}

/// Whether a firing body sequence on this viewmodel is a grenade wind-up to
/// animate as pin pull now and throw later, rather than an attack to play on
/// the spot. Every other weapon keeps the two-trigger firing path untouched.
fn pin_pull_on_windup(viewmodel_name: &str) -> bool {
    GRENADE_PINPULL.load(Ordering::Relaxed) && is_grenade_viewmodel(viewmodel_name)
}

/// Whether the viewmodel on screen is the weapon the spectated player is
/// actually holding.
///
/// The viewmodel pointer alone cannot be trusted: live logging shows it
/// alternating between two of a player's weapons (v_98k and v_luger) every few
/// milliseconds, which fires a spurious "draw" on every flip. The spectated
/// player's `curstate.weaponmodel` is replicated entity state and does not
/// flap, so it is the authority on what is held; the viewmodel is only
/// consulted for its sequence list once the two agree.
///
/// Matching is viewmodel-stem inside third-person name rather than the
/// reverse, because the bipod weapons append a deploy suffix on the
/// third-person side only (`v_mg42.mdl` vs `p_mg42bu.mdl` / `p_mg42bd.mdl`).
fn viewmodel_matches_held_weapon(viewmodel_name: &str, spectated: &ClEntityS) -> Option<bool> {
    let verdict = viewmodel_match_inner(viewmodel_name, spectated);

    // The filter is still letting a spurious draw through, alternating between
    // v_bar and v_colt, so log how the decision is actually being reached for
    // the first few. `None` means the held weapon could not be resolved at all,
    // which is currently treated as "allow" and would explain it.
    if LOG_HELD_MODELS.load(Ordering::Relaxed) {
        let held = engine::engine_studio()
            .map(|studio| unsafe { (studio.get_model_by_index)(spectated.curstate.weaponmodel) })
            .filter(|m| !m.is_null())
            .map(|m| unsafe { (*m).name_str() }.into_owned())
            .unwrap_or_else(|| "<unresolved>".into());
        unsafe {
            crate::debug::report(&format!(
                "anim_fix: match check -- viewmodel \"{viewmodel_name}\" vs held \"{held}\" (weaponmodel index {}) -> {}",
                spectated.curstate.weaponmodel,
                match verdict {
                    Some(true) => "match",
                    Some(false) => "MISMATCH, frame skipped",
                    None => "UNRESOLVED, frame allowed through",
                }
            ))
        };
    }

    verdict
}

fn viewmodel_match_inner(viewmodel_name: &str, spectated: &ClEntityS) -> Option<bool> {
    let studio = engine::engine_studio()?;
    let held = unsafe { (studio.get_model_by_index)(spectated.curstate.weaponmodel) };
    if held.is_null() {
        return None;
    }
    let held_name = unsafe { (*held).name_str() }.into_owned();
    let stem = third_person_stem(model_stem(viewmodel_name));
    if stem.is_empty() {
        return None;
    }
    let matched = model_stem(&held_name).contains(stem);
    if !matched {
        note_unmatched_pair(viewmodel_name, &held_name);
    }
    Some(matched)
}

/// Reads bipod state off the third-person model the spectated player is
/// holding.
///
/// Returns `None` for anything that is neither "bu" nor "bd", which is a real
/// and common case rather than an error: there are far more `p_` models than
/// weapons, because they also vary by stance. The MGs alone ship
/// `p_mg42bu` / `p_mg42bd` / `p_mg42pr` / `p_mg42sr`, and the Bren adds
/// `p_brenbr` / `p_brenpr` / `p_brensr` / `p_bren_l`. A player prone with an
/// MG is on one of those stance variants, so the deploy state is simply not
/// readable from the model name at that moment and the animation falls back to
/// whichever family was last known. Worth knowing before reading "deploy state
/// unknown" in a log as a failure.
fn get_spectated_deploy_state(
    weapon: &DeployableWeapon,
    entity: &ClEntityS,
) -> Option<DeployState> {
    let studio = engine::engine_studio()?;
    let weapon_model = unsafe { (studio.get_model_by_index)(entity.curstate.weaponmodel) };
    if weapon_model.is_null() {
        return None;
    }
    let name = unsafe { (*weapon_model).name_str() };
    if name.contains(weapon.deployed_marker) {
        Some(DeployState::Down)
    } else if name.contains(weapon.undeployed_marker) {
        Some(DeployState::Up)
    } else {
        None
    }
}

/// Counts the animations this fix has actually forced, so a session can be
/// judged without trusting scrollback -- reaching "running" says the
/// preconditions held, not that anything was corrected.
static ANIMATIONS_PLAYED: AtomicI32 = AtomicI32::new(0);

/// Reassurance that a long session is still working.
const ANIMATION_SUMMARY_EVERY: i32 = 100;

fn play_viewmodel_animation(
    sequence: i32,
    reason: &str,
    state: Option<DeployState>,
    viewmodel: *mut ModelSPartial,
) {
    if sequence < 0 {
        // Worth seeing: it means the model had no sequence matching what the
        // deploy state asked for, which is a gap in the up/down mapping rather
        // than a no-op.
        unsafe {
            crate::debug::report(&format!(
                "anim_fix: {reason} -- no matching sequence found, nothing played"
            ))
        };
        return;
    }
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };

    let played = ANIMATIONS_PLAYED.fetch_add(1, Ordering::Relaxed) + 1;
    if played % ANIMATION_SUMMARY_EVERY == 0 {
        unsafe { crate::debug::report(&format!("anim_fix: {played} animations corrected so far")) };
    }
    let played_label = sequence_label(viewmodel, sequence as usize);
    let played_label = played_label.as_deref().unwrap_or("<unknown>");
    {
        let label = &played_label;
        let family = match state {
            Some(DeployState::Up) => "bipod up",
            Some(DeployState::Down) => "bipod down",
            None => "deploy state unknown",
        };
        unsafe {
            crate::debug::report(&format!(
                "anim_fix: {reason} -- {family}, playing sequence {sequence} (\"{label}\")"
            ))
        };
    }

    // A grenade throw empties the hand and leaves it that way. `throw` is two
    // frames -- 0.050s on `v_stick` -- so the viewmodel sits on that final,
    // empty pose for as long as nothing else plays. Measured against a real
    // HLTV capture that was 3.5 seconds, because the replicated `weaponmodel`
    // went on insisting the player still held the grenade for four seconds
    // after he threw it. His own client had drawn the next weapon 1.25s in.
    //
    // So schedule a re-draw for the moment the throw finishes. What gets drawn
    // is whatever the replicated state then says is in hand, which is the only
    // answer available -- it is behind the player's own client, but an empty
    // hand for four seconds is further from the truth than a late draw.
    //
    // Measured from when the throw is *played*, which with the pin-pull option
    // on is `GRENADE_WINDUP_SECONDS` after the body change, not the body
    // change itself -- the hand is only empty once the grenade has left it.
    if is_throw_label(played_label) {
        // Never empty the hand in the first place.
        if level() == LEVEL_NEVER_EMPTY {
            unsafe {
                crate::debug::report(
                    "anim_fix: skipping the throw animation, so the grenade stays in hand",
                )
            };
            return;
        }
        let ends = engine::client_time() + model_sequence_duration(viewmodel, sequence).max(0.05);
        if let Some(at) = redraw_time_after_throw(level(), ends) {
            REDRAW_AFTER.store(at.to_bits(), Ordering::Relaxed);
        }
    }

    unsafe { (engfuncs.pfn_weapon_anim)(sequence, 0) };
}

/// When an option draws again after a throw that ends at `throw_ends`, or
/// `None` for the options that leave the hand as the throw left it.
fn redraw_time_after_throw(level: i32, throw_ends: f64) -> Option<f64> {
    match level {
        LEVEL_REDRAW_NOW => Some(throw_ends),
        LEVEL_LOOKAHEAD => Some(throw_ends + LOOKAHEAD_SECONDS),
        // LEVEL_EMPTY_HAND: play it and leave the hand as it lands.
        _ => None,
    }
}

/// When to draw whatever is in hand after a throw empties it, as demo-time
/// bits, or zero for "nothing pending".
static REDRAW_AFTER: AtomicU64 = AtomicU64::new(0);

/// Plays the draw for the weapon currently in view if a throw has finished.
///
/// Called every frame. Deliberately does nothing unless a throw actually
/// scheduled one, so the ordinary path is a single relaxed load.
fn redraw_after_throw_if_due(now: f64, state: Option<DeployState>, viewmodel: *mut ModelSPartial) {
    let due = f64::from_bits(REDRAW_AFTER.load(Ordering::Relaxed));
    if due == 0.0 || now < due {
        return;
    }
    REDRAW_AFTER.store(0, Ordering::Relaxed);
    play_viewmodel_animation(
        animation_lookup_sequence("draw", state, viewmodel),
        "throw finished, drawing what is now in hand",
        state,
        viewmodel,
    );
}

/// When to play the throw a grenade wind-up scheduled, as demo-time bits, or
/// zero for "nothing pending". Same shape as `REDRAW_AFTER`, and cancelled
/// in the same places: a weapon change or a camera change before it fires.
static THROW_AFTER: AtomicU64 = AtomicU64::new(0);

/// Books the throw for `GRENADE_WINDUP_SECONDS` after a wind-up seen at `now`.
/// A second wind-up before the first fires simply moves the booking.
fn schedule_throw(now: f64) {
    THROW_AFTER.store((now + GRENADE_WINDUP_SECONDS).to_bits(), Ordering::Relaxed);
}

/// A booked throw first seen this long after it was due is dropped instead of
/// played. `apply()` only reaches the throw on frames that are in-eye on this
/// weapon; if none came for this long the camera was elsewhere, and the
/// grenade is long gone. A hitch hands `HUD_Frame` at most 1.0s at a time
/// (`engine::tramp_hud_frame`), so an honest frame can never be later than
/// this.
const STALE_THROW_SECONDS: f64 = 1.0;

/// Consumes the pending throw if its time has come. `false` when nothing is
/// pending, it is not due yet, or it went stale -- so the ordinary frame is
/// one relaxed load.
fn take_due_throw(now: f64) -> bool {
    let due = f64::from_bits(THROW_AFTER.load(Ordering::Relaxed));
    if due == 0.0 || now < due {
        return false;
    }
    THROW_AFTER.store(0, Ordering::Relaxed);
    let overdue = now - due;
    if overdue > STALE_THROW_SECONDS {
        unsafe {
            crate::debug::report(&format!(
                "anim_fix: booked throw dropped -- {overdue:.3}s overdue, the frames it waited for were not in-eye on this grenade"
            ))
        };
        return false;
    }
    true
}

/// Drops a pending throw, saying why, and reports whether there was one.
fn cancel_pending_throw(why: &str) -> bool {
    if THROW_AFTER.swap(0, Ordering::Relaxed) == 0 {
        return false;
    }
    unsafe {
        crate::debug::report(&format!(
            "anim_fix: pending throw cancelled -- {why} before it fired"
        ))
    };
    true
}

/// The grenade wind-up: `pinpull` now, `throw` booked for when the grenade
/// actually leaves the hand.
///
/// `pinpull` is 0.867s long and is left to run; the throw cuts it short at
/// `GRENADE_WINDUP_SECONDS`, which is what a POV recording of a quick throw
/// looks like too.
fn wind_up_throw(now: f64, state: Option<DeployState>, viewmodel: *mut ModelSPartial) {
    schedule_throw(now);
    play_viewmodel_animation(
        animation_lookup_sequence("pinpull", state, viewmodel),
        &format!("grenade wind-up, pin pull now and the throw in {GRENADE_WINDUP_SECONDS:.3}s"),
        state,
        viewmodel,
    );
}

/// Plays the throw a wind-up booked, once its time has come.
///
/// Called every frame, ahead of `redraw_after_throw_if_due`, so the
/// emptied-hand options run from the moment the throw plays. Not deduplicated
/// through `claim_fire`: the sound trigger never fires for a grenade
/// (`weapons/grenthrow.wav` has no `_shoot` in it), so there is nothing to
/// dedup against, and a stale claim must not be able to swallow the throw.
fn throw_if_due(now: f64, state: Option<DeployState>, viewmodel: *mut ModelSPartial) {
    if !take_due_throw(now) {
        return;
    }
    play_viewmodel_animation(
        animation_lookup_any(ATTACK_SEQUENCES, state, viewmodel),
        &format!("scheduled throw fired, {GRENADE_WINDUP_SECONDS:.3}s after the wind-up"),
        state,
        viewmodel,
    );
}

// What `apply()` last saw, published for `on_weapon_fired`, which runs from
// the sound hook on the same thread but has none of this context.

/// Demo time of the last firing animation played, so the two independent
/// triggers cannot both play one shot.
static LAST_FIRE_PLAYED: AtomicU64 = AtomicU64::new(0);
/// The MG42's ~1200rpm puts 0.05s between rounds, so the window that keeps the
/// body-sequence and sound triggers from doubling up on a single shot has to
/// sit clear of that -- otherwise it would swallow real rounds of automatic
/// fire, which is the one case the sound trigger exists to catch.
const FIRE_DEDUP_SECONDS: f64 = 0.03;

/// Returns whether a firing animation should play now, or whether the other
/// trigger already played one for this same shot.
fn claim_fire(now: f64) -> bool {
    let last = f64::from_bits(LAST_FIRE_PLAYED.load(Ordering::Relaxed));
    // `now < last` means the clock went backwards -- a demo restarting -- so
    // let it through rather than blocking until it catches up again.
    if now >= last && now - last < FIRE_DEDUP_SECONDS {
        return false;
    }
    LAST_FIRE_PLAYED.store(now.to_bits(), Ordering::Relaxed);
    true
}

static CURRENT_SPECTATED: AtomicI32 = AtomicI32::new(-1);
static CURRENT_VIEWMODEL: AtomicPtr<ModelSPartial> = AtomicPtr::new(std::ptr::null_mut());

/// The entity index the engine's own `GetViewModel()` currently renders a
/// first-person viewmodel for -- i.e. who the game actually thinks "you" are
/// this frame, independent of `CHudSpectator`'s own bookkeeping. `-1` before
/// the first frame. Read by `spectator_target.rs`'s diagnostic; see its
/// module doc for why the two are tracked side by side.
pub(crate) fn current_viewmodel_entity() -> i32 {
    CURRENT_SPECTATED.load(Ordering::Relaxed)
}
static CURRENT_DEPLOY_STATE: AtomicI32 = AtomicI32::new(-1);
/// Last bipod state actually read off a "bu"/"bd" model, carried across the
/// stance variants that do not encode one. Cleared on a player switch, since
/// it says nothing about the next person.
static LAST_KNOWN_DEPLOY_STATE: AtomicI32 = AtomicI32::new(-1);

static PREVIOUS_SPECTATED_ENTITY: AtomicI32 = AtomicI32::new(-1);
static PREVIOUS_SEQUENCE: AtomicI32 = AtomicI32::new(-1);
static PREVIOUS_DEPLOY_STATE: AtomicI32 = AtomicI32::new(-1); // -1 none, 0 up, 1 down
/// Which weapon the viewmodel is *currently* showing, and since when.
///
/// Players switch weapons far faster than a draw animation takes to play --
/// one was measured alternating `p_colt` and `p_garand` six times in 1.5
/// seconds. That is real input, not engine noise: the spectated player's body
/// sequence changes on the *same frame*, `stand_pistol_aim` <->
/// `stand_rifle_aim`, which nothing happening only in the viewmodel could
/// cause. A POV recording of the same behaviour would start a draw each time
/// and cut each one short, so that is what to reproduce.
static SETTLED_VIEWMODEL: AtomicPtr<ModelSPartial> = AtomicPtr::new(std::ptr::null_mut());
/// When the last draw was triggered, so a weapon that changes and changes back
/// immediately does not produce a second one. Levels below `LEVEL_NO_COOLDOWN`
/// consult it; above, nothing does.
static LAST_DRAW_TRIGGERED: AtomicU64 = AtomicU64::new(0);

fn viewmodel_changed_to_a_new_weapon(current: *mut ModelSPartial, now: f64) -> bool {
    let previous = SETTLED_VIEWMODEL.swap(current, Ordering::Relaxed);
    if previous == current {
        return false;
    }
    // Nothing to draw *from* on the first weapon ever seen -- that is the
    // spectator arriving, not a switch.
    if previous.is_null() {
        return false;
    }
    LAST_DRAW_TRIGGERED.store(now.to_bits(), Ordering::Relaxed);
    true
}

/// Take the current viewmodel as the one in hand without treating it as a
/// switch the player made.
///
/// Used when the *spectator* moves to another player. The weapon in view
/// changes because the camera did, not because anybody drew anything, so a
/// draw animation there is simply wrong -- and it is not a rare edge: of the
/// draws in one measured session, 61 followed a spectated-player change rather
/// than a weapon change.
///
/// `apply()` already plays an idle on that frame, but that alone did not stop
/// it: the old settle timer kept running underneath and reported the new
/// weapon as a switch a frame or two later, after the branch that would have
/// suppressed it had been and gone.
fn adopt_viewmodel_without_drawing(current: *mut ModelSPartial) {
    SETTLED_VIEWMODEL.store(current, Ordering::Relaxed);
}

fn deploy_state_to_i32(state: Option<DeployState>) -> i32 {
    match state {
        None => -1,
        Some(DeployState::Up) => 0,
        Some(DeployState::Down) => 1,
    }
}

fn i32_to_deploy_state(v: i32) -> Option<DeployState> {
    match v {
        0 => Some(DeployState::Up),
        1 => Some(DeployState::Down),
        _ => None,
    }
}

/// Registers `apply()` to run every client frame. Safe to call regardless of
/// whether `ENABLED` is set -- `apply()` checks that itself, so this can be
/// installed unconditionally and toggled purely via the env var at any time
/// during the session (there's no per-install teardown needed).
pub fn install() {
    engine::set_per_frame_callback(apply);
}

/// Plays the firing animation from the weapon-fire *sound*, as a second
/// trigger behind the body-sequence one in `apply()`.
///
/// It exists for one case the body sequence cannot cover: a held trigger. The
/// server sets the player's body to `stand_mg_shoot` once and leaves it there
/// for the whole burst, so every round after the first has no sequence change
/// to detect, while the sound fires per round. Anything the body sequence
/// already caught is filtered out by `claim_fire`.
///
/// Called from `sound_fix`'s `EV_PlaySound` hook, on the engine thread, same as
/// `apply()`.
pub fn on_weapon_fired(entity_index: i32) {
    if !enabled() {
        return;
    }

    // Confirmed live: `ent` is a real, varying player index (3, 5, 8, 10,
    // 12, 13 across one session), so the sound does name the shooter. Still
    // logged, because what it cannot yet show is a session where the spectated
    // player is among them.
    let spectated = CURRENT_SPECTATED.load(Ordering::Relaxed);
    let matched = entity_index == spectated;
    // Gunfire from the rest of the match vastly outnumbers the spectated
    // player's own, so the misses are verbose-only; a hit is always worth a line.
    if matched || LOG_HELD_MODELS.load(Ordering::Relaxed) {
        unsafe {
            crate::debug::report(&format!(
                "anim_fix: weapon-fire sound from entity {entity_index}, currently spectating {spectated} -- {}",
                if matched { "MATCH" } else { "ignored" }
            ))
        };
    }

    // Only the player actually being watched. Playing a viewmodel animation
    // because somebody else fired would be worse than missing the round.
    if entity_index < 0 || entity_index != spectated {
        return;
    }
    let viewmodel = CURRENT_VIEWMODEL.load(Ordering::Relaxed);
    if viewmodel.is_null() {
        return;
    }
    if !claim_fire(engine::client_time()) {
        return;
    }

    let state = i32_to_deploy_state(CURRENT_DEPLOY_STATE.load(Ordering::Relaxed));
    let sequence = animation_lookup_any(ATTACK_SEQUENCES, state, viewmodel);
    play_viewmodel_animation(sequence, "spectated player fired (sound)", state, viewmodel);
}

/// Runs once per client frame (see `engine::set_per_frame_callback`).
pub fn apply() {
    if !enabled() {
        stage(STAGE_DISABLED);
        return;
    }
    let Some(engfuncs) = engine::engfuncs() else {
        stage(STAGE_NO_ENGFUNCS);
        return;
    };
    if unsafe { (engfuncs.is_spectate_only)() } == 0 {
        stage(STAGE_NOT_SPECTATING);
        return;
    }

    let viewmodel_entity = unsafe { (engfuncs.get_view_model)() };
    if viewmodel_entity.is_null() {
        stage_with(
            STAGE_NO_VIEWMODEL_ENTITY,
            viewmodel_entity,
            std::ptr::null_mut::<u8>(),
            -1,
        );
        return;
    }
    let viewmodel_model = unsafe { (*viewmodel_entity).model };
    if viewmodel_model.is_null() {
        stage_with(
            STAGE_NO_VIEWMODEL_MODEL,
            viewmodel_entity,
            viewmodel_model,
            unsafe { (*viewmodel_entity).index },
        );
        return;
    }
    let viewmodel_name = unsafe { (*viewmodel_model).name_str() }.into_owned();
    // Only the four bipod weapons have an up/down sequence split; every other
    // weapon still needs draw/reload/shoot driven, so this is no longer a
    // reason to bail out -- it just means there is no deploy state to track.
    let deployable = find_deployable_weapon(&viewmodel_name);
    note_viewmodel(&viewmodel_name, deployable.is_some(), viewmodel_model);

    let viewmodel_index = unsafe { (*viewmodel_entity).index };
    let spectated = unsafe { (engfuncs.get_entity_by_index)(viewmodel_index) };
    if spectated.is_null() || unsafe { (*spectated).player } == 0 {
        stage_with(
            STAGE_NO_SPECTATED_PLAYER,
            viewmodel_entity,
            viewmodel_model,
            viewmodel_index,
        );
        return;
    }
    let spectated = unsafe { &*spectated };

    // Before the mismatch filter, so stance changes are still recorded on
    // frames the filter drops.
    note_held_model(spectated);

    // The viewmodel flaps between a player's weapons faster than they could
    // possibly be switching. Acting on a frame where it disagrees with what
    // the player actually holds is what made "draw" fire ~30 times a second,
    // and it also meant the entity index published for the fire trigger was
    // whichever weapon happened to be showing.
    //
    // This has to come before any of the previous-state trackers are touched,
    // or the flap still registers as a change on the next agreeing frame.
    if viewmodel_matches_held_weapon(&viewmodel_name, spectated) == Some(false) {
        stage_with(
            STAGE_VIEWMODEL_MISMATCH,
            viewmodel_entity,
            viewmodel_model,
            viewmodel_index,
        );
        return;
    }

    let previous_entity = PREVIOUS_SPECTATED_ENTITY.swap(viewmodel_index, Ordering::Relaxed);
    let switched_players = previous_entity != viewmodel_index;
    if switched_players {
        // Says nothing about the new player.
        LAST_KNOWN_DEPLOY_STATE.store(-1, Ordering::Relaxed);
        // The single most useful line for reading a session back: which player
        // the camera moved to, and what they are holding according to their own
        // replicated state rather than the viewmodel.
        let held = engine::engine_studio()
            .map(|studio| unsafe { (studio.get_model_by_index)(spectated.curstate.weaponmodel) })
            .filter(|m| !m.is_null())
            .map(|m| unsafe { (*m).name_str() }.into_owned())
            .unwrap_or_else(|| "<unknown>".into());
        unsafe {
            crate::debug::report(&format!(
                "anim_fix: now spectating {} (was {}), holding {held}, viewmodel \"{viewmodel_name}\"",
                describe_player(viewmodel_index),
                describe_player(previous_entity),
            ))
        };
    }

    stage_with(
        STAGE_RUNNING,
        viewmodel_entity,
        viewmodel_model,
        viewmodel_index,
    );

    // The spectated player's own body animation. Replicated, so unlike almost
    // anything else about another player's weapon it survives into an HLTV
    // demo, and its label names both the action and the stance. Everything
    // below is read off it.
    let body_label = if spectated.model.is_null() {
        None
    } else {
        sequence_label(spectated.model, spectated.curstate.sequence.max(0) as usize)
    };

    // Bipod state, preferring the body sequence because it carries the state in
    // every stance. The "bu"/"bd" model name is the fallback: it goes
    // unreadable whenever the player is on a stance variant that carries
    // neither marker (p_mg42pr.mdl, p_mg42sr.mdl), which is most of the time a
    // machine gunner actually matters. Falling back further to the last state
    // actually observed keeps the viewmodel in the right sequence family across
    // any remaining gap, rather than dropping to whichever family a bare lookup
    // happens to find first.
    let observed = deployable.and_then(|weapon| {
        body_label
            .as_deref()
            .and_then(deploy_state_from_body_sequence)
            .or_else(|| get_spectated_deploy_state(weapon, spectated))
    });
    let state = match observed {
        Some(seen) => {
            LAST_KNOWN_DEPLOY_STATE.store(deploy_state_to_i32(Some(seen)), Ordering::Relaxed);
            Some(seen)
        }
        // Only worth remembering for a weapon that has the two families at all.
        None if deployable.is_some() => {
            i32_to_deploy_state(LAST_KNOWN_DEPLOY_STATE.load(Ordering::Relaxed))
        }
        None => None,
    };

    // Published for the fire-event path, which runs from the sound hook rather
    // than from here and so has no view of any of this.
    CURRENT_SPECTATED.store(viewmodel_index, Ordering::Relaxed);
    CURRENT_VIEWMODEL.store(viewmodel_model, Ordering::Relaxed);
    CURRENT_DEPLOY_STATE.store(deploy_state_to_i32(state), Ordering::Relaxed);
    let previous_state = i32_to_deploy_state(PREVIOUS_DEPLOY_STATE.load(Ordering::Relaxed));
    let deploy_state_changed =
        previous_state.is_some() && state.is_some() && previous_state != state;

    let now = engine::client_time();
    let viewmodel_changed = viewmodel_changed_to_a_new_weapon(viewmodel_model, now);

    if switched_players {
        // A throw the previous player wound up must not empty this one's hand.
        cancel_pending_throw("spectated player changed");
        // Snap the new viewmodel straight to the right family's idle so it
        // doesn't sit on whatever sequence the previously-spectated player
        // left it on -- and adopt it as the weapon in hand, so the change of
        // camera is not mistaken for the new player drawing it.
        adopt_viewmodel_without_drawing(viewmodel_model);
        play_viewmodel_animation(
            animation_lookup_sequence("idle", state, viewmodel_model),
            "spectated player changed",
            state,
            viewmodel_model,
        );
    } else if deploy_state_changed {
        // TODO(R&D, unverified live): play the "uptodown"/"downtoup"-style
        // transition sequence here instead of snapping straight to idle.
        play_viewmodel_animation(
            animation_lookup_sequence("idle", state, viewmodel_model),
            "bipod deploy state changed",
            state,
            viewmodel_model,
        );
    } else {
        // Classify the action from the spectated player's body animation, not
        // the viewmodel's. Only on a *change* of sequence: the label persists
        // for as long as the animation runs, so acting on its mere presence
        // would restart the viewmodel animation every frame.
        let previous_sequence = PREVIOUS_SEQUENCE.load(Ordering::Relaxed);
        if previous_sequence != spectated.curstate.sequence {
            // Under the same switch as the held-model trail, because it answers
            // the same question and reads better next to it: the body label
            // names the stance outright ("prone_bar_reload"), where the `p_`
            // model name only abbreviates it.
            if LOG_HELD_MODELS.load(Ordering::Relaxed) {
                unsafe {
                    crate::debug::report(&format!(
                        "anim_fix: body sequence -> {} (index {})",
                        body_label.as_deref().unwrap_or("<unreadable>"),
                        spectated.curstate.sequence,
                    ))
                };
            }
            match body_label.as_deref().map(classify_body_sequence) {
                Some(BodyAction::Shoot) => {
                    if claim_fire(now) {
                        // A grenade's firing sequence is the *release*, and
                        // the grenade leaves the hand half a second later --
                        // so pin pull now, throw then. Every other weapon
                        // fires on the spot.
                        if pin_pull_on_windup(&viewmodel_name) {
                            wind_up_throw(now, state, viewmodel_model);
                        } else {
                            play_viewmodel_animation(
                                animation_lookup_any(ATTACK_SEQUENCES, state, viewmodel_model),
                                "spectated player fired",
                                state,
                                viewmodel_model,
                            );
                        }
                    } else {
                        // A detected shot that plays nothing looks identical in
                        // the log to a shot that was never detected, and the
                        // two have completely different causes. Say which.
                        unsafe {
                            crate::debug::report(
                                "anim_fix: spectated player fired, but the dedup window swallowed it -- the sound trigger should already have played this shot",
                            )
                        };
                    }
                }
                Some(BodyAction::Reload) => {
                    play_viewmodel_animation(
                        animation_lookup_sequence("reload", state, viewmodel_model),
                        "spectated player reloaded",
                        state,
                        viewmodel_model,
                    );
                }
                _ => {}
            }
        }

        if viewmodel_changed {
            // A real switch draws anyway, so drop any re-draw a throw had
            // queued -- otherwise it would fire again a moment later and
            // restart the animation this line just began. Same for a throw
            // still waiting on its wind-up: the grenade is no longer in view.
            cancel_pending_throw("weapon changed");
            REDRAW_AFTER.store(0, Ordering::Relaxed);
            play_viewmodel_animation(
                animation_lookup_sequence("draw", state, viewmodel_model),
                "weapon changed",
                state,
                viewmodel_model,
            );
        }
    }

    // Last, so anything this frame genuinely wanted to play has already had
    // its say: the throw a wind-up booked, then -- measured from that throw --
    // the re-draw, since a throw's hand stays empty until something draws
    // into it.
    throw_if_due(now, state, viewmodel_model);
    redraw_after_throw_if_due(now, state, viewmodel_model);

    PREVIOUS_DEPLOY_STATE.store(deploy_state_to_i32(state), Ordering::Relaxed);
    PREVIOUS_SEQUENCE.store(spectated.curstate.sequence, Ordering::Relaxed);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Fast-forwarding through the slow parts of a demo is routine when making
    /// movies, so returning to normal speed must not leave the fix mistiming
    /// animations. Nothing here integrates demo position -- every decision is
    /// made from the current frame against one stored timestamp -- so a jump
    /// can cost at most the animation at the boundary.
    #[test]
    fn a_time_jump_does_not_leave_the_state_stuck() {
        let (a, b) = (
            std::ptr::without_provenance_mut::<ModelSPartial>(1),
            std::ptr::without_provenance_mut::<ModelSPartial>(2),
        );
        let _statics = reset_settle_state();
        LAST_FIRE_PLAYED.store(0f64.to_bits(), Ordering::Relaxed);

        assert!(!viewmodel_changed_to_a_new_weapon(a, 0.0));
        assert!(!viewmodel_changed_to_a_new_weapon(a, 1.0));
        assert!(claim_fire(1.0));

        // Fast-forward: the clock leaps, and the weapon is different when it
        // lands. The switch is reported once, on the frame it lands -- a jump
        // must not swallow it, and must not make it repeat either.
        let after = 500.0;
        assert!(viewmodel_changed_to_a_new_weapon(b, after));
        assert!(!viewmodel_changed_to_a_new_weapon(b, after + 1.0));
        assert!(!viewmodel_changed_to_a_new_weapon(b, after + 2.0));

        // And normal-speed firing resumes immediately, at the real cyclic rate
        // rather than being held off by the stale timestamp.
        assert!(claim_fire(after + 2.0));
        assert!(
            !claim_fire(after + 2.0 + FIRE_DEDUP_SECONDS / 2.0),
            "same shot"
        );
        assert!(claim_fire(after + 2.1), "next round");
    }

    /// Rapid switching is real input -- the player's body sequence moves with
    /// it -- so each switch must produce its own draw, exactly as a POV
    /// recording would. This used to assert the opposite, on a misdiagnosis.
    #[test]
    fn rapid_switching_still_draws_each_time() {
        let (a, b) = (
            std::ptr::without_provenance_mut::<ModelSPartial>(1),
            std::ptr::without_provenance_mut::<ModelSPartial>(2),
        );
        let _statics = reset_settle_state();

        assert!(!viewmodel_changed_to_a_new_weapon(a, 0.0));
        assert!(
            !viewmodel_changed_to_a_new_weapon(a, 1.0),
            "nothing to differ from yet"
        );

        // A kar -> pistol -> kar flick, the case reported from live testing.
        // Each leg is held ~0.2s, far under the old 0.4s window that swallowed
        // both of them.
        let mut draws = 0;
        for (i, t) in [1.20, 1.25, 1.40, 1.45, 1.60, 1.65].iter().enumerate() {
            let model = if (i / 2) % 2 == 0 { b } else { a };
            if viewmodel_changed_to_a_new_weapon(model, *t) {
                draws += 1;
            }
        }
        assert!(draws >= 2, "expected a draw per switch, got {draws}");
    }

    /// The window still exists to absorb a viewmodel that changes and changes
    /// back within a frame or two, which should read as no switch at all.
    #[test]
    fn a_weapon_that_only_flashes_past_does_not_swallow_the_next_draw() {
        let (a, b) = (
            std::ptr::without_provenance_mut::<ModelSPartial>(1),
            std::ptr::without_provenance_mut::<ModelSPartial>(2),
        );
        let c = std::ptr::without_provenance_mut::<ModelSPartial>(3);
        let _statics = reset_settle_state();
        assert!(
            !viewmodel_changed_to_a_new_weapon(a, 0.0),
            "the first weapon seen is not a switch"
        );

        // The real case this comes from: DoD passes through a weapon slot on
        // the way to another one. A player alternating MP40 and stick grenade
        // put a spade in between for 18-61ms. Every one of these is a change
        // and every one draws -- a transient is cut short by the next, which
        // is what a POV recording of the same input shows.
        assert!(
            viewmodel_changed_to_a_new_weapon(b, 1.000),
            "flashed-past weapon"
        );
        assert!(
            viewmodel_changed_to_a_new_weapon(c, 1.018),
            "18ms later -- must not be swallowed"
        );
        assert!(
            viewmodel_changed_to_a_new_weapon(b, 1.043),
            "25ms later -- nor this"
        );

        // What must still never happen: the same weapon reporting twice.
        assert!(!viewmodel_changed_to_a_new_weapon(b, 1.044));
        assert!(!viewmodel_changed_to_a_new_weapon(b, 9.999));
    }

    #[test]
    fn a_weapon_switch_draws_on_the_very_first_frame() {
        let (a, b) = (
            std::ptr::without_provenance_mut::<ModelSPartial>(1),
            std::ptr::without_provenance_mut::<ModelSPartial>(2),
        );
        let _statics = reset_settle_state();

        assert!(!viewmodel_changed_to_a_new_weapon(a, 0.0));
        assert!(!viewmodel_changed_to_a_new_weapon(a, 0.5));

        // `b` appears and stays. The draw plays on that frame -- not after the
        // window, which is what made every draw land ~55ms late in a live
        // session.
        assert!(
            viewmodel_changed_to_a_new_weapon(b, 1.0),
            "draw is immediate"
        );

        // And only once -- it must not restart every frame afterwards, inside
        // the window or long past it.
        assert!(!viewmodel_changed_to_a_new_weapon(b, 1.025));
        for i in 1..10 {
            let t = 1.0 + i as f64 * 0.1;
            assert!(
                !viewmodel_changed_to_a_new_weapon(b, t),
                "re-reported at t={t}"
            );
        }
    }

    /// The camera moving to another player is not that player drawing a
    /// weapon. 61 of one session's draws came from this.
    #[test]
    fn a_spectator_change_adopts_the_weapon_without_drawing() {
        let (a, b) = (
            std::ptr::without_provenance_mut::<ModelSPartial>(1),
            std::ptr::without_provenance_mut::<ModelSPartial>(2),
        );
        let _statics = reset_settle_state();

        assert!(!viewmodel_changed_to_a_new_weapon(a, 0.0));

        // Camera moves to a player holding a different weapon.
        adopt_viewmodel_without_drawing(b);

        // No draw for it, now or once the old window would have expired --
        // which is exactly where the stale timer used to produce one.
        assert!(!viewmodel_changed_to_a_new_weapon(b, 0.1));
        assert!(!viewmodel_changed_to_a_new_weapon(b, 0.2));

        // A genuine switch afterwards still draws.
        assert!(viewmodel_changed_to_a_new_weapon(a, 1.0));
    }

    /// The re-draw is scheduled by a throw and consumed once, when its time
    /// comes -- not every frame afterwards, which would restart the draw
    /// animation continuously.
    #[test]
    fn a_queued_redraw_fires_once_and_only_when_due() {
        let _statics = lock_statics();
        let vm = std::ptr::without_provenance_mut::<ModelSPartial>(1);
        REDRAW_AFTER.store(10.0f64.to_bits(), Ordering::Relaxed);

        redraw_after_throw_if_due(9.9, None, vm);
        assert_ne!(REDRAW_AFTER.load(Ordering::Relaxed), 0, "not due yet");

        redraw_after_throw_if_due(10.0, None, vm);
        assert_eq!(REDRAW_AFTER.load(Ordering::Relaxed), 0, "consumed when due");

        // And nothing pending means nothing happens, however late it gets.
        redraw_after_throw_if_due(9999.0, None, vm);
        assert_eq!(REDRAW_AFTER.load(Ordering::Relaxed), 0);
    }

    /// Serialises every test that touches the module's statics.
    ///
    /// They all share `LEVEL`, `SETTLED_VIEWMODEL` and the rest, and cargo
    /// runs tests in parallel by default -- so without this, one test setting
    /// the level to 1 can decide what another test observes. That was
    /// survivable while the statics only held pointers; it stopped being so
    /// once the level became a thing tests deliberately vary.
    ///
    /// `pub(crate)`: `commands.rs`'s own test also stores into `LEVEL`
    /// directly (issue #321) and needs this same lock, not a second one --
    /// two independent mutexes would serialise each module's tests against
    /// themselves but not against each other, which is exactly the race
    /// #321 hit.
    static TEST_STATICS: Mutex<()> = Mutex::new(());

    pub(crate) fn lock_statics() -> std::sync::MutexGuard<'static, ()> {
        // A poisoned lock means some other test panicked, which is already
        // being reported -- take it anyway rather than cascading a second
        // failure into every test that follows.
        TEST_STATICS.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Resets state and holds the lock for the caller's lifetime.
    ///
    /// Sets the emptied-hand option to `LEVEL_REDRAW_NOW`, because the level
    /// defaults to *off* and most tests below would otherwise be asserting
    /// against a disabled fix.
    #[must_use = "the guard must outlive the test, or the statics are not actually reserved"]
    fn reset_settle_state() -> std::sync::MutexGuard<'static, ()> {
        let guard = lock_statics();
        SETTLED_VIEWMODEL.store(std::ptr::null_mut(), Ordering::Relaxed);
        LAST_DRAW_TRIGGERED.store(0f64.to_bits(), Ordering::Relaxed);
        REDRAW_AFTER.store(0, Ordering::Relaxed);
        THROW_AFTER.store(0, Ordering::Relaxed);
        GRENADE_PINPULL.store(true, Ordering::Relaxed);
        LEVEL.store(LEVEL_REDRAW_NOW, Ordering::Relaxed);
        guard
    }

    /// The options have to actually differ, or the number is decoration. This
    /// pins what each one does with a throw, which is the only thing they
    /// disagree about -- and that all of them measure from the moment the
    /// throw plays, which is `GRENADE_WINDUP_SECONDS` after the body change
    /// rather than the body change itself.
    #[test]
    fn each_option_treats_an_emptied_hand_differently() {
        let _statics = reset_settle_state();

        // The body change at 10.0 is the wind-up; the throw plays at 10.5.
        let body_change = 10.0f64;
        schedule_throw(body_change);
        assert!(!take_due_throw(body_change), "the throw is not the wind-up");
        let throw_at = body_change + GRENADE_WINDUP_SECONDS;
        assert!(take_due_throw(throw_at));
        // The shortest throw play_viewmodel_animation allows for.
        let ends = throw_at + 0.05;

        // 1: the throw plays and nothing is queued behind it.
        assert_eq!(redraw_time_after_throw(LEVEL_EMPTY_HAND, ends), None);

        // 2 and 4 both queue a draw; 4 waits LOOKAHEAD_SECONDS longer.
        let now_at = redraw_time_after_throw(LEVEL_REDRAW_NOW, ends).unwrap();
        let later_at = redraw_time_after_throw(LEVEL_LOOKAHEAD, ends).unwrap();
        assert_eq!(now_at, ends);
        assert!(later_at > now_at, "option 4 must wait longer than option 2");
        assert_eq!(later_at - now_at, LOOKAHEAD_SECONDS);

        // Both are measured from the throw, so each lands exactly the wind-up
        // later than it would have from the body change -- the hand is not
        // empty until the grenade has left it.
        let from_body_change = body_change + 0.05;
        assert!((now_at - from_body_change - GRENADE_WINDUP_SECONDS).abs() < 1e-9);
        assert!(
            (later_at - from_body_change - LOOKAHEAD_SECONDS - GRENADE_WINDUP_SECONDS).abs() < 1e-9
        );

        // 3 is the only one that suppresses the throw outright; it never
        // queues anything either.
        assert_eq!(redraw_time_after_throw(LEVEL_NEVER_EMPTY, ends), None);
        assert_eq!(
            level_description(LEVEL_NEVER_EMPTY),
            "no throw animation, grenade stays in hand"
        );
        for other in [LEVEL_EMPTY_HAND, LEVEL_REDRAW_NOW, LEVEL_LOOKAHEAD] {
            assert_ne!(other, LEVEL_NEVER_EMPTY);
        }
    }

    /// The throw is booked for the server's wind-up after the body change --
    /// the grenade is still in the hand until then -- and consumed exactly
    /// once when its time comes, not on every frame afterwards.
    #[test]
    fn a_scheduled_throw_fires_at_the_windup_and_not_before() {
        let _statics = reset_settle_state();
        assert!(!take_due_throw(0.0), "nothing pending at the start");

        schedule_throw(10.0);
        assert!(!take_due_throw(10.0), "not on the frame of the body change");
        assert!(!take_due_throw(10.25));
        assert!(!take_due_throw(10.0 + GRENADE_WINDUP_SECONDS - 0.001));
        assert_ne!(THROW_AFTER.load(Ordering::Relaxed), 0, "still pending");

        assert!(take_due_throw(10.0 + GRENADE_WINDUP_SECONDS), "due now");
        assert_eq!(THROW_AFTER.load(Ordering::Relaxed), 0, "consumed");
        assert!(!take_due_throw(10.51), "not twice");
        assert!(!take_due_throw(9999.0));

        // A frame that lands a little late still fires it -- a hitch can hand
        // the clock up to 1.0s in one step -- but one that lands long after
        // means the camera was elsewhere, and the throw is dropped, not
        // played on whatever grenade is in hand by then.
        schedule_throw(20.0);
        assert!(take_due_throw(
            20.0 + GRENADE_WINDUP_SECONDS + STALE_THROW_SECONDS
        ));
        schedule_throw(30.0);
        assert!(!take_due_throw(
            30.0 + GRENADE_WINDUP_SECONDS + STALE_THROW_SECONDS + 0.001
        ));
        assert_eq!(THROW_AFTER.load(Ordering::Relaxed), 0, "dropped, not kept");
    }

    /// A pending throw belongs to the grenade that was wound up. If the
    /// viewmodel is a different weapon by the time it would fire -- a death,
    /// a switch, or the camera moving to another player -- it must not play.
    #[test]
    fn a_weapon_change_cancels_the_pending_throw() {
        let _statics = reset_settle_state();
        assert!(!cancel_pending_throw("nothing"), "nothing to cancel yet");

        schedule_throw(10.0);
        assert!(cancel_pending_throw("weapon changed"));
        assert!(!take_due_throw(10.0 + GRENADE_WINDUP_SECONDS), "cancelled");
        assert!(!take_due_throw(9999.0));
        assert!(!cancel_pending_throw("again"), "already cancelled");

        // And a wind-up after the cancel books a fresh throw of its own.
        schedule_throw(30.0);
        assert!(!take_due_throw(30.1));
        assert!(take_due_throw(30.0 + GRENADE_WINDUP_SECONDS));
    }

    /// `dodstudio_hltv_grenade_pinpull 0` is the previous behaviour -- throw
    /// at the body change -- and the pin pull only ever applies to the three
    /// grenades; a rifle's firing path is untouched either way.
    #[test]
    fn pin_pull_off_restores_the_old_timing() {
        let _statics = reset_settle_state();

        GRENADE_PINPULL.store(true, Ordering::Relaxed);
        for grenade in [
            "models/v_grenade.mdl",
            "models/v_stick.mdl",
            "models/v_mills.mdl",
        ] {
            assert!(pin_pull_on_windup(grenade), "{grenade}");
        }
        for other in [
            "models/v_garand.mdl",
            "models/v_mg42.mdl",
            "models/v_knife.mdl",
        ] {
            assert!(!pin_pull_on_windup(other), "{other}");
        }
        assert!(grenade_pinpull_description().starts_with("1 "));

        let on = grenade_pinpull_description();

        GRENADE_PINPULL.store(false, Ordering::Relaxed);
        assert!(!pin_pull_on_windup("models/v_stick.mdl"));
        assert!(!pin_pull_on_windup("models/v_garand.mdl"));
        let off = grenade_pinpull_description();
        assert!(off.starts_with("0 "));
        assert_ne!(
            on, off,
            "the two states must read differently in dodstudio_debug_status"
        );
    }

    /// Every option in range needs its own description -- `dodstudio_debug_status`
    /// and the usage text are how a session tells them apart.
    #[test]
    fn every_option_is_described_distinctly() {
        let mut seen = std::collections::HashSet::new();
        for n in LEVEL_OFF..=LEVEL_MAX {
            let d = level_description(n);
            assert!(!d.is_empty(), "option {n} has no description");
            assert!(
                seen.insert(d),
                "option {n} reuses another option's description"
            );
        }
    }

    /// Out of range is clamped rather than refused, so `99` means "newest".
    #[test]
    fn a_level_outside_the_ladder_is_clamped() {
        let _statics = lock_statics();
        LEVEL.store(99, Ordering::Relaxed);
        assert_eq!(level(), LEVEL_MAX);
        assert!(enabled());

        LEVEL.store(-5, Ordering::Relaxed);
        assert_eq!(level(), LEVEL_OFF);
        assert!(!enabled());

        LEVEL.store(LEVEL_MAX, Ordering::Relaxed);
    }
}
