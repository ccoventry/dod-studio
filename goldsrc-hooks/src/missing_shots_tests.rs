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
    assert!(SETTING.fallback.load(Ordering::Relaxed));
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
