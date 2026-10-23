//! Built-in command profiles (#911): profiles DoD Studio ships, like Clean
//! clips (#898). They live here, never in the settings file, so every update
//! replaces them; the Commands tab lists them beside the user's own, locked,
//! and Duplicate makes a copy the user can change.
//!
//! A settings file can still hold an editable copy: settings migration 2 added
//! Clean clips as an ordinary profile. `separate_builtin_copies` sorts those
//! out at load.

use crate::settings_manager::CommandProfile;

/// The Clean clips profile's name.
pub const CLEAN_CLIPS_PROFILE: &str = "Clean clips";

/// What a renamed copy of a built-in profile is called: "Clean clips (mine)",
/// then "Clean clips (mine 2)" when that name is taken.
const MINE_SUFFIX: &str = "mine";

/// A starter profile for clips without the clutter a recording carries
/// (scoreboard, voice menus, HLTV messages and bars). Every clip-look hook
/// setting is set, even where the value is the hook's own default, so applying
/// it gives the same clip whatever was changed elsewhere. Settings Studio
/// sends itself, the in-game window's `ds_ui_*` and `ds_debug_*` are left out.
/// `r_decals 256` and `mirv_fov 90` lead it because applying a profile
/// replaces Initial Commands, and those two are the first-run lines the
/// pipeline reads back.
///
/// Changing it: add the version being replaced to `CLEAN_CLIPS_SHIPPED`, so a
/// copy of it in someone's settings is still recognised as unchanged.
const CLEAN_CLIPS: &[&str] = &[
    "r_decals 256",
    "mirv_fov 90",
    "ds_spec_match_pov 1",
    "ds_spec_hide_hltv_messages 1",
    "ds_spec_hide_bars 1",
    "ds_spec_deathcam 0",
    "ds_spec_hide_text 1",
    "ds_hud_hide_scoreboard 1",
    "ds_hud_hide_voice_menus 1",
    "ds_hud_hide_crosshair 0",
    "ds_hud_hide_map_text 1",
    "ds_hud_hide_clan_text 1",
    "ds_hud_hide_center_text 1",
    "ds_hud_show_scope 1",
    "ds_hud_overview_players 0",
    "ds_hud_overview_team_marker 0",
    "ds_hud_overview_hide_speaker 0",
    // 1 once #904 (DoD's own bob model) is in.
    "ds_view_use_cl_bob 0",
    "ds_view_hide_headshot_red_screen 1",
    "ds_view_hide_screen_shake 1",
    "ds_player_hide_hand_signals 0",
    "ds_sound_mute_voice_commands 0",
    "ds_map_allow_shaders 0",
];

/// Every earlier version of Clean clips that settings migration 2 could have
/// copied into a settings file, oldest first (from git history: #898 in
/// dbfcaa64, b2b638f0, then b1d05fcb's, which is the current one). A copy
/// equal to any of these was never changed by the user.
const CLEAN_CLIPS_SHIPPED: &[&[&str]] = &[
    &[
        "r_decals 256",
        "mirv_fov 90",
        "ds_spec_match_pov 1",
        "ds_spec_hide_hltv_messages 1",
        "ds_spec_hide_bars 1",
        "ds_spec_deathcam 0",
        "ds_hud_hide_scoreboard 1",
        "ds_hud_hide_voice_menus 1",
        "ds_hud_hide_crosshair 0",
        "ds_hud_hide_map_text 0",
        "ds_hud_hide_clan_text 1",
        "ds_hud_show_scope 1",
        "ds_hud_overview_players 0",
        "ds_hud_overview_team_marker 0",
        "ds_view_use_cl_bob 0",
        "ds_view_hide_headshot_red_screen 0",
        "ds_view_hide_screen_shake 0",
        "ds_player_hide_hand_signals 0",
        "ds_sound_mute_voice_commands 0",
        "ds_map_allow_shaders 0",
    ],
    &[
        "r_decals 256",
        "mirv_fov 90",
        "ds_spec_match_pov 1",
        "ds_spec_hide_hltv_messages 1",
        "ds_spec_hide_bars 1",
        "ds_spec_deathcam 0",
        "ds_hud_hide_scoreboard 1",
        "ds_hud_hide_voice_menus 1",
        "ds_hud_hide_crosshair 0",
        "ds_hud_hide_map_text 1",
        "ds_hud_hide_clan_text 1",
        "ds_hud_show_scope 1",
        "ds_hud_overview_players 0",
        "ds_hud_overview_team_marker 0",
        "ds_view_use_cl_bob 0",
        "ds_view_hide_headshot_red_screen 1",
        "ds_view_hide_screen_shake 0",
        "ds_player_hide_hand_signals 0",
        "ds_sound_mute_voice_commands 0",
        "ds_map_allow_shaders 0",
    ],
];

fn profile(name: &str, init_commands: &[&str]) -> CommandProfile {
    CommandProfile {
        name: name.to_string(),
        init_commands: init_commands.iter().map(|c| c.to_string()).collect(),
        custom_commands: Vec::new(),
    }
}

/// One built-in profile: its name, what it holds now, and what earlier
/// versions held. Built-ins have no Scheduled Commands.
struct Builtin {
    name: &'static str,
    current: &'static [&'static str],
    shipped: &'static [&'static [&'static str]],
}

const BUILTINS: &[Builtin] = &[Builtin {
    name: CLEAN_CLIPS_PROFILE,
    current: CLEAN_CLIPS,
    shipped: CLEAN_CLIPS_SHIPPED,
}];

/// The built-in profiles, as the Commands tab lists them.
pub fn builtin_command_profiles() -> Vec<CommandProfile> {
    BUILTINS
        .iter()
        .map(|b| profile(b.name, b.current))
        .collect()
}

/// Whether `copy` holds exactly what some version of `builtin` held: the same
/// Initial Commands in the same order (trimmed, blank rows dropped, as the
/// frontend saves them) and no Scheduled Commands.
fn is_unchanged_copy(builtin: &Builtin, copy: &CommandProfile) -> bool {
    if !copy
        .custom_commands
        .iter()
        .all(|c| c.command.trim().is_empty())
    {
        return false;
    }
    let lines: Vec<&str> = copy
        .init_commands
        .iter()
        .map(|c| c.trim())
        .filter(|c| !c.is_empty())
        .collect();
    std::iter::once(builtin.current)
        .chain(builtin.shipped.iter().copied())
        .any(|version| lines == version)
}

/// "Clean clips (mine)", or "(mine 2)", "(mine 3)"… past the names `taken`.
fn mine_name(builtin: &str, taken: &[String]) -> String {
    let free = |candidate: &str| {
        !taken
            .iter()
            .any(|t| t.trim().eq_ignore_ascii_case(candidate))
    };
    let first = format!("{builtin} ({MINE_SUFFIX})");
    if free(&first) {
        return first;
    }
    (2..)
        .map(|n| format!("{builtin} ({MINE_SUFFIX} {n})"))
        .find(|candidate| free(candidate))
        .expect("an unbounded range finds a free name")
}

/// Takes every profile out of `profiles` that has a built-in profile's name
/// (ignoring case and surrounding spaces), since the built-in one now stands
/// in that name. A copy the user never changed is dropped: the built-in one
/// replaces it. A changed one is kept under "<name> (mine)", and its new name
/// is returned so the Commands tab can say so once. `active` (the profile the
/// lists were last applied from) follows a renamed copy, and takes the
/// built-in's spelling for a dropped one.
pub fn separate_builtin_copies(
    profiles: &mut Vec<CommandProfile>,
    active: &mut String,
) -> Vec<String> {
    let mut kept = Vec::new();
    let mut i = 0;
    while i < profiles.len() {
        let name = profiles[i].name.trim().to_string();
        let Some(builtin) = BUILTINS.iter().find(|b| b.name.eq_ignore_ascii_case(&name)) else {
            i += 1;
            continue;
        };
        let was_active = active.trim().eq_ignore_ascii_case(&name);
        if is_unchanged_copy(builtin, &profiles[i]) {
            profiles.remove(i);
            if was_active {
                *active = builtin.name.to_string();
            }
            continue;
        }
        let mut taken: Vec<String> = profiles.iter().map(|p| p.name.clone()).collect();
        taken.extend(BUILTINS.iter().map(|b| b.name.to_string()));
        let new_name = mine_name(builtin.name, &taken);
        if was_active {
            *active = new_name.clone();
        }
        profiles[i].name = new_name.clone();
        kept.push(new_name);
        i += 1;
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture_manager::CustomCommandPayload;

    fn named(name: &str, lines: &[&str]) -> CommandProfile {
        profile(name, lines)
    }

    #[test]
    fn the_current_version_is_not_also_listed_as_an_old_one() {
        assert!(!CLEAN_CLIPS_SHIPPED.contains(&CLEAN_CLIPS));
    }

    #[test]
    fn an_unchanged_copy_of_any_shipped_version_is_dropped() {
        for version in CLEAN_CLIPS_SHIPPED
            .iter()
            .chain(std::iter::once(&CLEAN_CLIPS))
        {
            let mut profiles = vec![
                named("FOTW", &["mirv_fov 100"]),
                named("Clean clips", version),
            ];
            let mut active = "Clean clips".to_string();
            let kept = separate_builtin_copies(&mut profiles, &mut active);
            assert!(kept.is_empty());
            assert_eq!(profiles.len(), 1);
            assert_eq!(profiles[0].name, "FOTW");
            assert_eq!(active, CLEAN_CLIPS_PROFILE);
        }
    }

    #[test]
    fn spaces_and_blank_rows_do_not_count_as_a_change() {
        let mut lines: Vec<String> = CLEAN_CLIPS.iter().map(|c| format!(" {c} ")).collect();
        lines.push(String::new());
        let mut profiles = vec![CommandProfile {
            name: " clean CLIPS ".to_string(),
            init_commands: lines,
            custom_commands: vec![CustomCommandPayload {
                command: "  ".to_string(),
                relation: "Before".to_string(),
                offset_seconds: 2.0,
            }],
        }];
        let mut active = String::new();
        assert!(separate_builtin_copies(&mut profiles, &mut active).is_empty());
        assert!(profiles.is_empty());
        assert_eq!(active, "");
    }

    #[test]
    fn a_changed_copy_is_kept_as_mine_and_the_active_name_follows() {
        let mut lines = CLEAN_CLIPS.to_vec();
        lines[3] = "ds_spec_deathcam 1";
        let mut profiles = vec![named("Clean clips", &lines)];
        let mut active = "Clean clips".to_string();
        let kept = separate_builtin_copies(&mut profiles, &mut active);
        assert_eq!(kept, ["Clean clips (mine)"]);
        assert_eq!(profiles[0].name, "Clean clips (mine)");
        assert_eq!(profiles[0].init_commands[3], "ds_spec_deathcam 1");
        assert_eq!(active, "Clean clips (mine)");
    }

    #[test]
    fn a_scheduled_command_is_a_change() {
        let mut copy = profile(CLEAN_CLIPS_PROFILE, CLEAN_CLIPS);
        copy.custom_commands.push(CustomCommandPayload {
            command: "host_timescale 0.5".to_string(),
            relation: "Before".to_string(),
            offset_seconds: 1.0,
        });
        let mut profiles = vec![copy];
        let mut active = "FOTW".to_string();
        let kept = separate_builtin_copies(&mut profiles, &mut active);
        assert_eq!(kept, ["Clean clips (mine)"]);
        assert_eq!(active, "FOTW", "another active profile is left alone");
    }

    #[test]
    fn the_mine_name_skips_names_already_taken() {
        let mut profiles = vec![
            named("Clean clips (mine)", &["mirv_fov 100"]),
            named("clean clips", &["mirv_fov 110"]),
        ];
        let mut active = String::new();
        let kept = separate_builtin_copies(&mut profiles, &mut active);
        assert_eq!(kept, ["Clean clips (mine 2)"]);
        assert_eq!(profiles[0].name, "Clean clips (mine)");
        assert_eq!(profiles[0].init_commands, ["mirv_fov 100"]);
        assert_eq!(profiles[1].name, "Clean clips (mine 2)");
        // Nothing left to do on the next load.
        assert!(separate_builtin_copies(&mut profiles, &mut active).is_empty());
        assert_eq!(profiles.len(), 2);
    }

    #[test]
    fn clean_clips_holds_nothing_refused_or_renamed() {
        let profile = profile(CLEAN_CLIPS_PROFILE, CLEAN_CLIPS);
        assert!(profile.custom_commands.is_empty());
        // Starts with the first-run lines, since applying it replaces the list.
        let fresh = crate::settings_manager::AppSettings::default();
        assert_eq!(profile.init_commands[..2], fresh.init_commands[..]);
        assert!(
            profile
                .init_commands
                .contains(&"ds_spec_match_pov 1".to_string())
        );
        // Nothing in it is refused, a no-op at demo load, or renamed. (The
        // mid-demo list doesn't apply: these run once, at demo load.)
        let commands = &profile.init_commands;
        assert!(native::patch::cfg_scan::banned_commands(commands).is_empty());
        assert!(native::patch::cfg_scan::noop_commands_in_init(commands).is_empty());
        assert!(native::patch::cfg_scan::renamed_commands(commands).is_empty());
    }
}
