//! When the player's own view would have no crosshair, so the spectated view
//! of them should have none either (issue #310).
//!
//! ## The reference is a POV demo, not the game being played
//!
//! `CHudDoDCrossHair::Draw` (`client+0x2cd20`) draws the POV crosshair only
//! when `ShouldDrawCrossHair` (`client+0x2d0e0`) says so; the full table is
//! on #310. Played live, that function hides the crosshair for more reasons
//! than a recording of the same player does, and it is the recording this
//! matches: footage from an HLTV demo sits beside footage from POV demos.
//!
//! Measured by recording POV demos to frames and looking for the crosshair
//! in each one (`analysis/examples/crosshair_pov_probe.rs` prints the state
//! to lay beside them; about 13,000 frames over six demos):
//!
//! | state | live | in a POV demo |
//! | --- | --- | --- |
//! | sprint keys held and moving | hidden | hidden, to the frame |
//! | going prone / getting up | hidden | hidden 1.53s from the start of each |
//! | prone and moving | hidden | hidden, to the frame |
//! | on a ladder | hidden | hidden |
//! | knife, spade, sniper rifle | hidden | hidden |
//! | machine gun not deployed | hidden | hidden; back the instant it deploys |
//! | dead | hidden | hidden |
//! | 0.5s after drawing a weapon | hidden | **shown** (4 of 706 frames hidden) |
//! | reloading | hidden | **shown** (0 of 75) |
//! | 1.6s after a bolt rifle's shot | hidden | **shown** (0 of 132) |
//! | in the air after a jump | hidden | **shown** (2 of 35) |
//!
//! The four that differ are driven by the player's own client predicting his
//! weapon and movement (`flBoltHideXHair`, `g_iinjump`), which does not run
//! when a demo plays. So they are left out here.
//!
//! ## What a spectator has for each
//!
//! All of it is replicated player state:
//!
//! | POV's test | read here from |
//! | --- | --- |
//! | sprint key and a move key | gait `dod_sprint`, which follows them within a frame or two |
//! | prone transition | 1.5s from the body entering `get_down` / `get_up` |
//! | prone and a move key | gait `prone_forward` (`dod_crawl` is the crouched walk) |
//! | ladder | `movetype` 5 |
//! | weapon | the third-person model held |
//! | MG deployed | body `sandbag_*` / `bipod_*` |
//! | dead | body `die_*` / `dead*` |
//!
//! ## Where it is not exact
//!
//! - **Crawling** can read up to half a second long: the gait stays
//!   `prone_forward` while the player slides to a stop after letting go.
//! - **Underwater** is left out: POV hides only when fully submerged, and
//!   the body's `swim` sequence covers swimming on the surface too.
//! - **The scoped FG42 while zoomed** is left out: zoom is not replicated
//!   for other players.
//!
//! The British knife is the US knife's weapon with another model, and the
//! paratrooper knife is the German knife's, so both are in the list by model.

use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

/// `i_ProneCounter`: how long the gun stays lowered from the start of going
/// prone or getting up. The body's `get_down` runs 1.3s and `get_up` 2.0s, so
/// neither sequence's own length is the answer.
const PRONE_TRANSITION_SECONDS: f64 = 1.5;
/// `MOVETYPE_FLY`: on a ladder.
const MOVETYPE_FLY: i32 = 5;

// A demo time as f64 bits, zero for "none".
static PRONE_TRANSITION_AT: AtomicU64 = AtomicU64::new(0);
/// Which transition the body was in last frame: 0 none, 1 down, 2 up.
static IN_TRANSITION: AtomicU8 = AtomicU8::new(0);

fn since(cell: &AtomicU64, now: f64) -> Option<f64> {
    let at = f64::from_bits(cell.load(Ordering::Relaxed));
    // A clock that went backwards is a new demo; nothing from before counts.
    (at != 0.0 && now >= at).then_some(now - at)
}

/// The camera moved to another player: the timer was the last one's.
pub(super) fn forget() {
    PRONE_TRANSITION_AT.store(0, Ordering::Relaxed);
    IN_TRANSITION.store(0, Ordering::Relaxed);
}

/// Starts the prone timer on the frame the body enters `get_down` or
/// `get_up`. A player first seen mid-transition starts it then, which can
/// only run long, by less than the transition.
fn note_body(now: f64, body: &str) {
    let transition = match body {
        "get_down" => 1,
        "get_up" => 2,
        _ => 0,
    };
    if IN_TRANSITION.swap(transition, Ordering::Relaxed) != transition && transition != 0 {
        PRONE_TRANSITION_AT.store(now.to_bits(), Ordering::Relaxed);
    }
}

/// Weapons POV never draws a crosshair for: ids 1, 2, 19 and 32 in the
/// binary (US knife, German knife, spade, mortar), and the three scoped
/// rifles, which GL hides whether zoomed or not. Id 1 is held as `amerk` or,
/// by the British, `fairbairn`; id 2 as `paraknife`. `youthk` ships with the
/// game as a knife model that no weapon table read so far names; it is listed
/// in case something does hold it.
fn has_no_crosshair(held_stem: &str) -> bool {
    matches!(
        held_stem,
        "amerk"
            | "fairbairn"
            | "paraknife"
            | "youthk"
            | "spade"
            | "mortar"
            | "spring"
            | "spring_l"
            | "k98s"
            | "k98s_l"
            | "enfields"
            | "enfields_l"
    )
}

/// The three machine guns that only have a crosshair while deployed.
fn is_machine_gun(held_stem: &str) -> bool {
    held_stem.starts_with("mg42") || held_stem.starts_with("mg34") || held_stem.starts_with("30cal")
}

/// Deployed on a ledge or on the bipod, read from the body: `sandbag_*` and
/// `bipod_*`, in their aim, shoot and reload forms alike.
fn is_deployed(body: &str) -> bool {
    body.starts_with("sandbag_") || body.starts_with("bipod_")
}

/// What the spectated player is doing, as far as the crosshair cares.
pub(super) struct View<'a> {
    /// The body sequence's label, e.g. `stand_bolt_aim`.
    pub body: &'a str,
    /// The gait sequence's label, e.g. `dod_sprint`.
    pub gait: &'a str,
    pub movetype: i32,
    /// The third-person weapon model's stem, e.g. `k98s`.
    pub held_stem: &'a str,
}

/// Why POV would hide the crosshair. A small enum rather than text so the
/// current reason fits in one atomic for `dodstudio_debug_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Hidden {
    Dead = 1,
    NoCrosshairWeapon,
    UndeployedMachineGun,
    Sprinting,
    ProneTransition,
    Crawling,
    OnALadder,
}

impl Hidden {
    const ALL: [Hidden; 7] = [
        Hidden::Dead,
        Hidden::NoCrosshairWeapon,
        Hidden::UndeployedMachineGun,
        Hidden::Sprinting,
        Hidden::ProneTransition,
        Hidden::Crawling,
        Hidden::OnALadder,
    ];

    /// The reason stored as `code`, or `None` for 0 ("drawn") and anything
    /// that is not a reason.
    pub(crate) fn from_code(code: u8) -> Option<Hidden> {
        Hidden::ALL.into_iter().find(|reason| *reason as u8 == code)
    }

    pub(crate) fn text(self) -> &'static str {
        match self {
            Hidden::Dead => "dead",
            Hidden::NoCrosshairWeapon => "this weapon has no crosshair",
            Hidden::UndeployedMachineGun => "machine gun not deployed",
            Hidden::Sprinting => "sprinting",
            Hidden::ProneTransition => "going prone or getting up",
            Hidden::Crawling => "crawling",
            Hidden::OnALadder => "on a ladder",
        }
    }
}

/// The first reason POV would hide the crosshair now, or `None` if it draws.
/// Call once a frame: it also watches the body for the start of a prone
/// transition.
pub(super) fn hidden_because(view: &View, now: f64) -> Option<Hidden> {
    let View {
        body,
        gait,
        movetype,
        held_stem,
    } = *view;
    note_body(now, body);
    if body.starts_with("die_") || body.starts_with("dead") {
        return Some(Hidden::Dead);
    }
    if has_no_crosshair(held_stem) {
        return Some(Hidden::NoCrosshairWeapon);
    }
    let deployed = is_deployed(body);
    if is_machine_gun(held_stem) && !deployed {
        return Some(Hidden::UndeployedMachineGun);
    }
    // The gun-lowered states, none of which apply on a deployed gun.
    if !deployed {
        if gait == "dod_sprint" {
            return Some(Hidden::Sprinting);
        }
        if since(&PRONE_TRANSITION_AT, now).is_some_and(|s| s < PRONE_TRANSITION_SECONDS) {
            return Some(Hidden::ProneTransition);
        }
        if gait == "prone_forward" {
            return Some(Hidden::Crawling);
        }
        if movetype == MOVETYPE_FLY {
            return Some(Hidden::OnALadder);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim_fix::tests::lock_statics;

    fn view<'a>(body: &'a str, gait: &'a str, held_stem: &'a str) -> View<'a> {
        View {
            body,
            gait,
            movetype: 3,
            held_stem,
        }
    }

    #[test]
    fn standing_with_a_rifle_shows_the_crosshair() {
        let _statics = lock_statics();
        forget();
        for (body, gait, held) in [
            ("stand_rifle_aim", "dod_idle1", "garand"),
            ("crouch_stg44_aim", "dod_crouch_idle", "stg44"),
            ("stand_rifle_shoot", "dod_walk", "garand"),
            // Lying prone and still keeps it; so does a deployed machine gun.
            ("prone_rifle_aim", "prone_idle", "garand"),
            ("bipod_mg_aim", "prone_idle", "mg42bd"),
            ("sandbag_30cal_shoot", "dod_idle1", "30cal"),
            // Crouched and walking: `dod_crawl` is not the prone crawl.
            ("crouch_rifle_aim", "dod_crawl", "garand"),
            // Hidden while playing, shown in a POV demo: a reload, a jump.
            ("stand_garand_reload", "dod_idle1", "garand"),
            ("jump", "dod_jog", "garand"),
            ("stand_gren_aim", "dod_walk", "grenade"),
            ("stand_bazooka_aim", "dod_walk", "bazooka"),
        ] {
            assert_eq!(
                hidden_because(&view(body, gait, held), 10.0),
                None,
                "{body} / {gait} / {held}"
            );
        }
    }

    #[test]
    fn the_lowered_gun_states_hide_it() {
        let _statics = lock_statics();
        forget();
        for (body, gait, why) in [
            ("sprint_bolt_aim", "dod_sprint", Hidden::Sprinting),
            ("prone_rifle_aim", "prone_forward", Hidden::Crawling),
        ] {
            assert_eq!(
                hidden_because(&view(body, gait, "garand"), 10.0),
                Some(why),
                "{body} / {gait}"
            );
        }
        let ladder = View {
            movetype: MOVETYPE_FLY,
            ..view("stand_rifle_aim", "dod_idle1", "garand")
        };
        assert_eq!(hidden_because(&ladder, 10.0), Some(Hidden::OnALadder));
    }

    #[test]
    fn weapons_without_a_crosshair_never_show_one() {
        let _statics = lock_statics();
        forget();
        for held in [
            "amerk",
            "fairbairn",
            "paraknife",
            "youthk",
            "spade",
            "mortar",
            "spring",
            "k98s",
            "k98s_l",
            "enfields",
        ] {
            assert_eq!(
                hidden_because(&view("stand_bolt_aim", "dod_idle1", held), 10.0),
                Some(Hidden::NoCrosshairWeapon),
                "{held}"
            );
        }
    }

    #[test]
    fn a_machine_gun_has_one_only_while_deployed() {
        let _statics = lock_statics();
        forget();
        for held in ["mg42bu", "mg34pr", "30cal"] {
            assert_eq!(
                hidden_because(&view("stand_mg_aim", "dod_idle1", held), 10.0),
                Some(Hidden::UndeployedMachineGun),
                "{held}"
            );
            assert_eq!(
                hidden_because(&view("sandbag_mg_aim", "dod_idle1", held), 10.0),
                None,
                "{held} deployed"
            );
        }
        // The BAR, FG42 and Bren keep theirs either way.
        assert_eq!(
            hidden_because(&view("stand_bar_aim", "dod_idle1", "barbu"), 10.0),
            None
        );
    }

    #[test]
    fn going_prone_or_getting_up_hides_it_for_a_second_and_a_half() {
        let _statics = lock_statics();
        forget();
        // Measured in a POV demo: hidden 1.53s from the start of each, though
        // `get_down` plays for 1.3s and `get_up` for 2.0s.
        let down = view("get_down", "look_idle", "barbu");
        assert_eq!(hidden_because(&down, 100.0), Some(Hidden::ProneTransition));
        assert_eq!(hidden_because(&down, 101.2), Some(Hidden::ProneTransition));
        // Lying still, the body sequence over, the timer still running.
        let prone = view("prone_rifle_aim", "prone_idle", "barbu");
        assert_eq!(hidden_because(&prone, 101.4), Some(Hidden::ProneTransition));
        assert_eq!(hidden_because(&prone, 101.5), None);

        let up = view("get_up", "look_idle", "barbu");
        assert_eq!(hidden_because(&up, 110.0), Some(Hidden::ProneTransition));
        assert_eq!(hidden_because(&up, 111.4), Some(Hidden::ProneTransition));
        // Still in `get_up`, but the gun is already back.
        assert_eq!(hidden_because(&up, 111.6), None);
        forget();
    }

    #[test]
    fn every_reason_survives_the_trip_through_its_code() {
        for reason in Hidden::ALL {
            assert_eq!(Hidden::from_code(reason as u8), Some(reason));
            assert!(!reason.text().is_empty());
        }
        assert_eq!(Hidden::from_code(0), None);
        assert_eq!(Hidden::from_code(200), None);
    }

    #[test]
    fn dead_hides_it() {
        let _statics = lock_statics();
        forget();
        assert_eq!(
            hidden_because(&view("die_headshot", "dod_idle1", "garand"), 10.0),
            Some(Hidden::Dead)
        );
    }
}
