//! Pure reading of DoD's model and sequence naming schemes: which weapons have
//! a bipod, what a body sequence says the player is doing, how a viewmodel's
//! name maps to its third-person model, and the up/down sequence families.
//! No engine access, so everything here is unit-tested directly.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum DeployState {
    Up,
    Down,
}

pub(super) struct DeployableWeapon {
    pub(super) viewmodel_match: &'static str,
    pub(super) deployed_marker: &'static str,
    pub(super) undeployed_marker: &'static str,
}

// Confirmed against the actual .mdl files shipped with DoD 1.3 (see the R&D
// write-up for the sequence-label dump). All four use the same "up"/"down"
// (or "up_"/"down_") first-person sequence-family split as the third-person
// p_*bu.mdl / p_*bd.mdl model swap.
const DEPLOYABLE_WEAPONS: &[DeployableWeapon] = &[
    DeployableWeapon {
        viewmodel_match: "mg42",
        deployed_marker: "bd.mdl",
        undeployed_marker: "bu.mdl",
    },
    DeployableWeapon {
        viewmodel_match: "mg34",
        deployed_marker: "bd.mdl",
        undeployed_marker: "bu.mdl",
    },
    DeployableWeapon {
        viewmodel_match: "bar",
        deployed_marker: "bd.mdl",
        undeployed_marker: "bu.mdl",
    },
    DeployableWeapon {
        viewmodel_match: "bren",
        deployed_marker: "bd.mdl",
        undeployed_marker: "bu.mdl",
    },
    // Also matches v_scopedfg42.mdl, which is correct: it has the same
    // up_*/down_* sequence set. It ships only p_scopedfg42bu.mdl with no "bd"
    // counterpart, so its deploy state simply always reads as up, which is
    // what a scoped FG42 does.
    DeployableWeapon {
        viewmodel_match: "fg42",
        deployed_marker: "bd.mdl",
        undeployed_marker: "bu.mdl",
    },
    // v_30cal.mdl has the same upidle/downidle first-person split, but its
    // p_30cal*.mdl set (p_30cal / p_30calpr / p_30calr / p_30calsr) has no
    // matching bd/bu third-person pair -- DoD 1.3's 30cal is normally a
    // fixed, already-mounted tripod gun rather than carried and
    // bipod-deployed the way the other four are, so its "up"/"down"
    // viewmodel sequences likely key off something other than a weaponmodel
    // swap. Left out until confirmed live; see the R&D write-up.
];

pub(super) fn find_deployable_weapon(viewmodel_name: &str) -> Option<&'static DeployableWeapon> {
    DEPLOYABLE_WEAPONS
        .iter()
        .find(|w| viewmodel_name.contains(w.viewmodel_match))
}

/// What the player being spectated is doing, read off their own body animation.
///
/// DoD's player models name every sequence `<stance>_<weapon>_<action>` --
/// `stand_bolt_shoot`, `crouch_bar_reload`, `bipod_mg_aim`, `sprint_sten_aim`.
/// Read straight out of `models/player/us-inf/us-inf.mdl`, whose 345 sequences
/// cover every weapon and stance in the game.
///
/// This is the trigger the firing animation hangs off, and the reason it does
/// is that `curstate.sequence` is *replicated*: it survives into an HLTV demo,
/// which almost nothing about another player's weapon does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum BodyAction {
    Shoot,
    Reload,
    Other,
}

pub(super) fn classify_body_sequence(label: &str) -> BodyAction {
    let label = label.to_ascii_lowercase();
    if label.ends_with("_shoot") || label.ends_with("_roll") {
        // Covers every attack, not just gunfire: `stand_gren_shoot` is a
        // grenade throw and `crouch_knife_shoot` a stab, and
        // `ATTACK_SEQUENCES` below has the viewmodel labels for both.
        //
        // `_roll` is the underhand grenade throw (`stand_stick_roll`), which
        // was being ignored -- the grenade viewmodels have one animation,
        // "throw", for both, so an overhand throw animated and a rolled one
        // did not.
        BodyAction::Shoot
    } else if label.contains("reload") || label.contains("zoomload") {
        // "zoomload" is the rocket weapons reloading while scoped.
        BodyAction::Reload
    } else {
        BodyAction::Other
    }
}

/// Bipod state read from the player's own body animation.
///
/// Better than the `p_*bu`/`p_*bd` model name it falls back to, which only
/// carries the marker in some stances and so goes unreadable exactly when a
/// machine gunner is prone. The body label carries it in every stance.
pub(super) fn deploy_state_from_body_sequence(label: &str) -> Option<DeployState> {
    // Called every frame a bipod weapon is in view, so it compares in place
    // rather than lowercasing a copy of the label first.
    let starts_with = |prefix: &str| {
        label
            .as_bytes()
            .get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
    };
    // `sandbag_` is deployed onto cover rather than on the bipod, but it drives
    // the same "down" first-person sequence family.
    if starts_with("bipod_") || starts_with("sandbag_") {
        Some(DeployState::Down)
    } else if ["stand_", "crouch_", "prone_", "sprint_"]
        .iter()
        .any(|p| starts_with(p))
    {
        Some(DeployState::Up)
    } else {
        None
    }
}

/// The names DoD's viewmodels give their attack animation, in the order worth
/// trying. Taken from a dump of all 41 `v_*.mdl` sequence lists.
///
/// Plain "shoot" (98k, enfield, luger, sten, webley, m1carbine), numbered
/// "shoot1" (colt, garand, k43, mp40, mp44, tommy, greasegun, spring) and
/// prefixed "up_shoot"/"upshoot" (bar, bren, fg42, mg42, mg34, 30cal) are all
/// reached by the "shoot" entry via the substring fallback. "launch" is the
/// rocket weapons (bazooka, panzerschreck, PIAT), "fire" the mortar, "throw"
/// every grenade, and "slash1" the knife and spade.
pub(super) const ATTACK_SEQUENCES: &[&str] = &["shoot", "launch", "fire", "throw", "slash1"];

// A grenade's firing body sequence (`stand_gren_shoot`, `crouch_stick_roll`,
// ...) is the **release** of the button, not the pin pull, and not the throw:
// the server sets it in the same call as `m_flStartThrow = time + 0.5`, and
// the grenade leaves the hand half a second later. The pull itself is never
// networked -- `StartThrow` sends `svc_weaponanim` to the owner alone, and a
// probe over 107 pulls found nothing an HLTV demo carries that moves at one
// (`analysis/examples/grenade_pinpull_tell_probe.rs`). What follows from
// that, and everything else a grenade does in the hand, is in `grenade.rs`.

/// Whether a viewmodel is one of the three grenades, which are the only
/// weapons whose attack is a wind-up followed by a throw rather than a shot.
///
/// Exact on the stem, not a substring: `v_grenade`, `v_stick` and `v_mills`
/// are the three files, and nothing else in the 41 `v_*.mdl` set shares a
/// stem with them.
pub(super) fn is_grenade_viewmodel(viewmodel_name: &str) -> bool {
    matches!(model_stem(viewmodel_name), "grenade" | "stick" | "mills")
}

/// `"models/v_98k.mdl"` -> `"98k"`, `"models/p_mg42bd.mdl"` -> `"mg42bd"`.
///
/// DoD uses three model prefixes and all three are stripped: `v_` is the
/// first-person viewmodel (41 files), `p_` the third-person attachment in a
/// player's hands (75), `w_` the world model of a dropped weapon (56).
pub(super) fn model_stem(name: &str) -> &str {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let file = file.strip_suffix(".mdl").unwrap_or(file);
    for prefix in ["v_", "p_", "w_"] {
        if let Some(rest) = file.strip_prefix(prefix) {
            return rest;
        }
    }
    file
}

/// Weapons whose first- and third-person models are not named the same thing.
///
/// The match filter (`viewmodel_match_inner`, in `mod.rs`) compares the
/// viewmodel's stem against the held model's, which works for most weapons
/// (`v_garand` / `p_garand`). Seven do
/// not match at all, and for those *every* frame was discarded as "the
/// viewmodel is not the weapon the spectated player is holding" -- 7139 frames
/// in a single session for the STG44 alone. That silently disabled draw,
/// reload and the body-sequence firing trigger for all seven; only the
/// sound-driven firing trigger still worked, which is what made it look like a
/// missing draw animation rather than a whole weapon being skipped.
///
/// Keys are exact viewmodel stems, values a substring of the third-person
/// stem. Read out of the shipped model files rather than guessed.
const VIEWMODEL_ALIASES: &[(&str, &str)] = &[
    ("98k", "k98"),
    ("scoped98k", "k98s"),
    ("mp44", "stg44"),
    ("greasegun", "grease"),
    ("m1carbine", "m1carb"),
    ("panzerschreck", "pschreck"),
    ("enfield_scoped", "enfields"),
];

/// The third-person name to look for, given a viewmodel's stem.
pub(super) fn third_person_stem(viewmodel_stem: &str) -> &str {
    VIEWMODEL_ALIASES
        .iter()
        .find(|(viewmodel, _)| *viewmodel == viewmodel_stem)
        .map(|(_, third_person)| *third_person)
        .unwrap_or(viewmodel_stem)
}

fn sequence_family(label: &str) -> Option<DeployState> {
    if label.len() >= 4 && label[..4].eq_ignore_ascii_case("down") {
        Some(DeployState::Down)
    } else if label.len() >= 2 && label[..2].eq_ignore_ascii_case("up") {
        Some(DeployState::Up)
    } else {
        None
    }
}

/// "upidle" <-> "downidle", "up_idle" <-> "down_idle" -- keeps whatever
/// followed the prefix (including a leading underscore, if any) intact.
pub(super) fn swap_family_prefix(label: &str, target: DeployState) -> String {
    match sequence_family(label) {
        Some(current) if current != target => {
            let prefix_len = if current == DeployState::Up { 2 } else { 4 };
            let rest = &label[prefix_len..];
            let new_prefix = if target == DeployState::Up {
                "up"
            } else {
                "down"
            };
            format!("{new_prefix}{rest}")
        }
        _ => label.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Every label below is copied from a dump of the real
    /// `models/player/us-inf/us-inf.mdl` and `models/v_*.mdl` shipped with
    /// DoD 1.3, not invented -- these functions exist only to read that
    /// naming scheme, so made-up labels would test nothing.
    #[test]
    fn body_sequences_classify_by_action() {
        for label in [
            "stand_bolt_shoot",
            "crouch_rifle_shoot",
            "prone_mg_shoot",
            "bipod_bren_shoot",
            "sandbag_30cal_shoot",
            // Not gunfire, but still an attack, and the viewmodels have
            // "throw" and "slash1" for them.
            "stand_gren_shoot",
            "crouch_knife_shoot",
            // The underhand grenade throw. Same "throw" viewmodel animation as
            // the overhand one, and it was being ignored.
            "stand_stick_roll",
            "crouch_mills_roll",
        ] {
            assert_eq!(classify_body_sequence(label), BodyAction::Shoot, "{label}");
        }

        for label in [
            "stand_garand_reload",
            "crouch_reload_webley",
            "prone_bar_reload",
            "bipod_mg42_reload",
            "stand_pschreck_zoomload",
        ] {
            assert_eq!(classify_body_sequence(label), BodyAction::Reload, "{label}");
        }

        for label in [
            // Aiming is the resting state between shots, and is what makes a
            // repeated shot show up as a sequence *change* at all.
            "stand_bolt_aim",
            "sprint_sten_aim",
            "bipod_mg_aim",
            "dod_idle1",
            "prone_forward",
            "die_headshot",
            // A rifle-butt swing, deliberately left alone: the viewmodels have
            // no matching sequence.
            "stand_rifle_swing",
        ] {
            assert_eq!(classify_body_sequence(label), BodyAction::Other, "{label}");
        }
    }

    #[test]
    fn body_sequences_carry_the_deploy_state_in_every_stance() {
        // The whole point of preferring this over the p_*bu/bd model name: a
        // prone or sprinting machine gunner still reports a state here.
        assert_eq!(
            deploy_state_from_body_sequence("prone_mg_shoot"),
            Some(DeployState::Up)
        );
        assert_eq!(
            deploy_state_from_body_sequence("sprint_bren_aim"),
            Some(DeployState::Up)
        );
        assert_eq!(
            deploy_state_from_body_sequence("stand_mg_aim"),
            Some(DeployState::Up)
        );
        assert_eq!(
            deploy_state_from_body_sequence("crouch_bar_reload"),
            Some(DeployState::Up)
        );

        assert_eq!(
            deploy_state_from_body_sequence("bipod_mg_shoot"),
            Some(DeployState::Down)
        );
        assert_eq!(
            deploy_state_from_body_sequence("sandbag_bren_reload"),
            Some(DeployState::Down)
        );

        // Sequences with no stance prefix say nothing either way, and must not
        // be read as "not deployed".
        assert_eq!(deploy_state_from_body_sequence("dod_idle1"), None);
        assert_eq!(deploy_state_from_body_sequence("hs_gogogo"), None);
    }

    /// The prefix test compares in place; it must still ignore case, and a
    /// label shorter than the prefix is simply not a match.
    #[test]
    fn body_deploy_state_ignores_case_and_short_labels() {
        assert_eq!(
            deploy_state_from_body_sequence("BIPOD_mg_shoot"),
            Some(DeployState::Down)
        );
        assert_eq!(
            deploy_state_from_body_sequence("Prone_Bar_Reload"),
            Some(DeployState::Up)
        );
        assert_eq!(
            deploy_state_from_body_sequence("stand_"),
            Some(DeployState::Up)
        );
        assert_eq!(deploy_state_from_body_sequence("stand"), None);
        assert_eq!(deploy_state_from_body_sequence(""), None);
        assert_eq!(deploy_state_from_body_sequence("bïpod_mg_aim"), None);
    }

    /// Every pair here is a real (v_*.mdl, p_*.mdl) pair shipped with DoD 1.3.
    /// The STG44 row is the one that cost 7139 discarded frames in a session.
    #[test]
    fn viewmodels_match_their_third_person_models() {
        let pairs = [
            // The seven that need an alias.
            ("models/v_98k.mdl", "models/p_k98.mdl"),
            ("models/v_scoped98k.mdl", "models/p_k98s.mdl"),
            ("models/v_mp44.mdl", "models/p_stg44.mdl"),
            ("models/v_greasegun.mdl", "models/p_grease.mdl"),
            ("models/v_m1carbine.mdl", "models/p_m1carb.mdl"),
            ("models/v_panzerschreck.mdl", "models/p_pschreck.mdl"),
            ("models/v_enfield_scoped.mdl", "models/p_enfields.mdl"),
            // Ordinary ones, which must keep working.
            ("models/v_garand.mdl", "models/p_garand.mdl"),
            ("models/v_colt.mdl", "models/p_colt.mdl"),
            // Stance and bipod suffixes live on the third-person side only.
            ("models/v_bar.mdl", "models/p_barbu.mdl"),
            ("models/v_mg42.mdl", "models/p_mg42bd.mdl"),
            ("models/v_bren.mdl", "models/p_brenpr.mdl"),
            ("models/v_greasegun.mdl", "models/p_grease_l.mdl"),
        ];
        for (viewmodel, held) in pairs {
            let stem = third_person_stem(model_stem(viewmodel));
            assert!(
                model_stem(held).contains(stem),
                "{viewmodel} should match {held} (looked for {stem:?})"
            );
        }
    }

    #[test]
    fn aliases_do_not_confuse_the_scoped_and_unscoped_variants() {
        // A scoped k98's viewmodel must not accept the plain k98 in hand.
        let scoped = third_person_stem(model_stem("models/v_scoped98k.mdl"));
        assert!(!model_stem("models/p_k98.mdl").contains(scoped));
        assert!(model_stem("models/p_k98s.mdl").contains(scoped));

        // Same for the Enfield, whose scoped third-person model is p_enfields.
        let scoped = third_person_stem(model_stem("models/v_enfield_scoped.mdl"));
        assert!(!model_stem("models/p_enfield.mdl").contains(scoped));
        assert!(model_stem("models/p_enfields.mdl").contains(scoped));

        // And the M1 carbine must not match the folding-stock carbine.
        let carbine = third_person_stem(model_stem("models/v_m1carbine.mdl"));
        assert!(!model_stem("models/p_fcarb.mdl").contains(carbine));
    }

    #[test]
    fn model_stem_strips_all_three_prefixes() {
        assert_eq!(model_stem("models/v_98k.mdl"), "98k");
        assert_eq!(model_stem("models/p_mg42bd.mdl"), "mg42bd");
        assert_eq!(model_stem("models\\w_luger.mdl"), "luger");
        assert_eq!(model_stem("models/player/us-inf/us-inf.mdl"), "us-inf");
    }

    /// The three grenade viewmodels, and nothing else: the pin pull must not
    /// play for a weapon that has no pin, and the stem test must be exact so
    /// that nothing merely containing one of the words qualifies.
    #[test]
    fn only_the_three_grenades_are_grenade_viewmodels() {
        for name in [
            "models/v_grenade.mdl",
            "models/v_stick.mdl",
            "models/v_mills.mdl",
            "models\\v_stick.mdl",
        ] {
            assert!(is_grenade_viewmodel(name), "{name}");
        }
        for name in [
            "models/v_garand.mdl",
            "models/v_98k.mdl",
            "models/v_mg42.mdl",
            "models/v_knife.mdl",
            "models/v_spade.mdl",
            // A stem that only contains the word is not the weapon.
            "models/v_stickgren.mdl",
            "models/v_grenade_launcher.mdl",
            "",
        ] {
            assert!(!is_grenade_viewmodel(name), "{name}");
        }
        // `model_stem` strips `p_` and `w_` the same as `v_`, so this is only
        // ever asked about the viewmodel's own name -- which is all `apply()`
        // ever hands it.
        assert!(is_grenade_viewmodel("models/p_stick.mdl"));
    }

    #[test]
    fn family_prefix_swaps_both_spellings() {
        assert_eq!(swap_family_prefix("upidle", DeployState::Down), "downidle");
        assert_eq!(
            swap_family_prefix("down_reload", DeployState::Up),
            "up_reload"
        );
        // Already in the target family, and unfamilied labels, are untouched.
        assert_eq!(swap_family_prefix("upshoot", DeployState::Up), "upshoot");
        assert_eq!(swap_family_prefix("reload", DeployState::Down), "reload");
    }
}
