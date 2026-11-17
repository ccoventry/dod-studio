use super::*;
use std::sync::Mutex;

/// A corrected animation keeps the sleeves the game put on the view
/// model: US 0, British 1, Axis 2 (`EV_GetWeaponBody`), never a fixed 0
/// (#1467).
#[test]
fn a_corrected_animation_keeps_the_view_models_sleeves() {
    for body in [0, 1, 2] {
        // SAFETY: `cl_entity_s` is plain data; all zeroes is a valid value.
        let mut viewmodel: Box<ClEntityS> = Box::new(unsafe { std::mem::zeroed() });
        viewmodel.curstate.body = body;
        assert_eq!(sleeve_body(&*viewmodel), body);
    }
    assert_eq!(sleeve_body(std::ptr::null()), 0);
}

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
/// `pub(crate)`: `commands/status.rs`'s test also stores into `LEVEL`
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
/// Turns the fix on, because `LEVEL` starts at *off* (the game's default
/// is stored at startup by `lib.rs`) and most tests below would otherwise
/// be asserting against a disabled fix.
#[must_use = "the guard must outlive the test, or the statics are not actually reserved"]
fn reset_settle_state() -> std::sync::MutexGuard<'static, ()> {
    let guard = lock_statics();
    SETTLED_VIEWMODEL.store(std::ptr::null_mut(), Ordering::Relaxed);
    LAST_DRAW_TRIGGERED.store(0f64.to_bits(), Ordering::Relaxed);
    grenade::forget("a test starting");
    LEVEL.store(LEVEL_MAX, Ordering::Relaxed);
    guard
}

/// Every option in range needs its own description -- `ds_debug_status`
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

/// Only a POV demo stands the fixes down (#613): an HLTV demo is played
/// back spectate-only, and live play, spectating or not, plays nothing back.
#[test]
fn only_a_pov_demo_counts_as_one() {
    assert!(is_pov_demo(true, false));
    assert!(!is_pov_demo(true, true), "HLTV demo");
    assert!(!is_pov_demo(false, true), "live through an HLTV proxy");
    assert!(!is_pov_demo(false, false), "live play");
}

/// #613: an HLTV demo published entity 13 and its viewmodel, then a POV
/// demo ran with nobody spectated. Each round entity 13 fired in the POV
/// demo played the old viewmodel's "shoot" index on the new gun. A frame
/// with no spectated view now forgets them, so the fire trigger has no one
/// to match.
#[test]
fn a_frame_with_no_spectated_view_forgets_the_last_one() {
    let _statics = reset_settle_state();
    // A real `model_t`-shaped value, in case a regression gets as far as
    // reading the viewmodel's name.
    let mut model: ModelSPartial = unsafe { std::mem::zeroed() };
    CURRENT_SPECTATED.store(13, Ordering::Relaxed);
    CURRENT_VIEWMODEL.store(&mut model, Ordering::Relaxed);
    CURRENT_DEPLOY_STATE.store(0, Ordering::Relaxed);
    let fires_before = LAST_FIRE_PLAYED.load(Ordering::Relaxed);
    let played_before = ANIMATIONS_PLAYED.load(Ordering::Relaxed);

    // No engine under test: `apply()` stops at its first check, as it
    // does at `IsSpectateOnly()` in a POV demo.
    apply();

    assert_eq!(CURRENT_SPECTATED.load(Ordering::Relaxed), -1);
    assert!(CURRENT_VIEWMODEL.load(Ordering::Relaxed).is_null());
    assert_eq!(CURRENT_DEPLOY_STATE.load(Ordering::Relaxed), -1);

    on_weapon_fired(13);
    assert_eq!(LAST_FIRE_PLAYED.load(Ordering::Relaxed), fires_before);
    assert_eq!(ANIMATIONS_PLAYED.load(Ordering::Relaxed), played_before);
}
