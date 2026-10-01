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
//! - **grenades** -- copied from what the thrower's own recording shows, as
//!   far as a spectator can see it: `pinpull` when the body enters its
//!   grenade attack, `throw` 0.5s later when the grenade leaves the hand, and
//!   then whichever of three things the player did -- caught it again to
//!   prime it, kept a second grenade, or went to the next weapon. See
//!   `grenade.rs`.
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
mod grenade;
mod sequences;
mod trace;

use std::sync::atomic::{AtomicI32, AtomicPtr, AtomicU64, Ordering};

use classify::{
    ATTACK_SEQUENCES, BodyAction, DeployState, DeployableWeapon, classify_body_sequence,
    deploy_state_from_body_sequence, find_deployable_weapon, is_grenade_viewmodel, model_stem,
    third_person_stem,
};
pub(crate) use sequences::sequence_label;
use sequences::{animation_lookup_any, animation_lookup_sequence};
pub use trace::{LOG_HELD_MODELS, status};
use trace::{
    STAGE_DISABLED, STAGE_NO_ENGFUNCS, STAGE_NO_SPECTATED_PLAYER, STAGE_NO_VIEWMODEL_ENTITY,
    STAGE_NO_VIEWMODEL_MODEL, STAGE_NOT_SPECTATING, STAGE_RUNNING, STAGE_VIEWMODEL_MISMATCH,
    describe_player, note_held_model, note_unmatched_pair, note_viewmodel, stage, stage_with,
};

use crate::engine::{self, ClEntityS, ModelSPartial};

/// The animation part of `dodstudio_fix_spectator_pov`: 0 is off, 1 is on.
///
/// This used to be a ladder of four ways to treat the hand after a grenade
/// throw (leave it empty, draw at once, never throw, draw after a second),
/// kept side by side because nothing said which was right. A POV demo does:
/// `grenade.rs` now copies what the thrower's own recording plays, so there
/// is one behaviour and the cvar is a switch. Values above 1 are taken as 1,
/// so a config that still says 2 or 4 keeps working.
pub static LEVEL: AtomicI32 = AtomicI32::new(0);

pub const LEVEL_OFF: i32 = 0;
pub const LEVEL_MAX: i32 = 1;

/// What each value is, for `dodstudio_debug_status` and the startup line.
pub fn level_description(level: i32) -> &'static str {
    match level {
        LEVEL_OFF => "off",
        _ => "on",
    }
}

pub fn level() -> i32 {
    LEVEL.load(Ordering::Relaxed).clamp(LEVEL_OFF, LEVEL_MAX)
}

pub fn enabled() -> bool {
    level() > LEVEL_OFF
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

    unsafe { (engfuncs.pfn_weapon_anim)(sequence, 0) };
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
/// Called from `fire_sounds`' `EV_PlaySound` hook, on the engine thread, same as
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
        grenade::forget("spectated player changed");
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
                        if is_grenade_viewmodel(&viewmodel_name) {
                            grenade::wind_up(now, state, viewmodel_model);
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
            // A real switch draws anyway, so whatever a throw still had
            // coming is dropped -- unless this is a thrown grenade coming
            // back into hand, which is a catch and plays its own animation.
            let caught = grenade::weapon_changed(
                now,
                state,
                viewmodel_model,
                is_grenade_viewmodel(&viewmodel_name),
            );
            if !caught {
                play_viewmodel_animation(
                    animation_lookup_sequence("draw", state, viewmodel_model),
                    "weapon changed",
                    state,
                    viewmodel_model,
                );
            }
        }
    }

    // Last, so anything this frame genuinely wanted to play has already had
    // its say: the throw a wind-up booked, and what follows it.
    if is_grenade_viewmodel(&viewmodel_name) {
        grenade::each_frame(now, state, viewmodel_model, &viewmodel_name, spectated);
    }

    PREVIOUS_DEPLOY_STATE.store(deploy_state_to_i32(state), Ordering::Relaxed);
    PREVIOUS_SEQUENCE.store(spectated.curstate.sequence, Ordering::Relaxed);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Only the three grenades wind up and throw; every other weapon fires
    /// on the spot.
    #[test]
    fn only_the_grenades_take_the_grenade_path() {
        for grenade in [
            "models/v_grenade.mdl",
            "models/v_stick.mdl",
            "models/v_mills.mdl",
        ] {
            assert!(is_grenade_viewmodel(grenade), "{grenade}");
        }
        for other in [
            "models/v_garand.mdl",
            "models/v_mg42.mdl",
            "models/v_knife.mdl",
        ] {
            assert!(!is_grenade_viewmodel(other), "{other}");
        }
    }

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
    /// Turns the fix on, because it defaults to *off* and most tests below
    /// would otherwise be asserting against a disabled fix.
    #[must_use = "the guard must outlive the test, or the statics are not actually reserved"]
    fn reset_settle_state() -> std::sync::MutexGuard<'static, ()> {
        let guard = lock_statics();
        SETTLED_VIEWMODEL.store(std::ptr::null_mut(), Ordering::Relaxed);
        LAST_DRAW_TRIGGERED.store(0f64.to_bits(), Ordering::Relaxed);
        grenade::forget("a test starting");
        LEVEL.store(LEVEL_MAX, Ordering::Relaxed);
        guard
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

    /// Out of range is clamped rather than refused, so a config left over from
    /// the four-option ladder (`2`, `4`) still turns the fix on.
    #[test]
    fn a_level_outside_the_ladder_is_clamped() {
        let _statics = lock_statics();
        for old_option in [2, 3, 4, 99] {
            LEVEL.store(old_option, Ordering::Relaxed);
            assert_eq!(level(), LEVEL_MAX);
            assert!(enabled());
        }

        LEVEL.store(-5, Ordering::Relaxed);
        assert_eq!(level(), LEVEL_OFF);
        assert!(!enabled());

        LEVEL.store(LEVEL_MAX, Ordering::Relaxed);
    }
}
