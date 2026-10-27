//! What the DoD Studio window's Settings tab lists (#899): every `ds_*`
//! setting a player would change, grouped under the area its name says
//! (`ds_hud_...` under HUD, CLAUDE.md "Console names"), and the two game
//! cvars the tab always had.
//!
//! [`AREAS`] is the one list. `ui/Settings.res` is generated from it (the
//! tests' `layout::settings_res`); a test fails while the file differs and rewrites it
//! when run with `DS_WRITE_SETTINGS_RES=1`, so a new setting is one line
//! here. Another test fails while `ui/Commands.txt` names a setting that is
//! neither here nor in [`LEFT_OUT`].
//!
//! Left out: what Studio sets itself for each launch or batch
//! (`ds_spec_lock`, `ds_capture_*`, `ds_player_overrides`) and the
//! diagnostics (`ds_debug_*`).
//!
//! A row is bound by its control's name, `cvar_<name>` (`studio_panel`'s
//! `sync_settings`): a check box for an on/off setting, a dropdown for one
//! with a few values ([`Kind::Choice`]) or the HD styles built on disk
//! ([`Kind::Styles`]).

// The window is 32-bit only; a host build compiles the list for the tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use crate::names::console_name;

/// What a row's control is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// A check box: 1 ticked, 0 not.
    Check,
    /// A dropdown of (value, what it shows). A value set elsewhere that is
    /// none of these shows as itself ([`choice_text`]).
    Choice(&'static [(&'static str, &'static str)]),
    /// A dropdown of the HD styles built on disk (`texture_hires`).
    Styles,
}

/// One setting on the tab.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Row {
    pub cvar: &'static str,
    /// The control's own text (a check box) or the label before it (a
    /// dropdown): what happens, in plain words.
    pub label: &'static str,
    /// The help line's text while the pointer is over it, after the name.
    pub help: &'static str,
    pub kind: Kind,
}

const fn check(cvar: &'static str, label: &'static str, help: &'static str) -> Row {
    Row {
        cvar,
        label,
        help,
        kind: Kind::Check,
    }
}

const fn choice(
    cvar: &'static str,
    label: &'static str,
    help: &'static str,
    values: &'static [(&'static str, &'static str)],
) -> Row {
    Row {
        cvar,
        label,
        help,
        kind: Kind::Choice(values),
    }
}

/// One heading and its rows. `pair` puts the next area beside this one, a
/// column each, for two short areas. The DLL reads only the rows; the rest
/// is the layout's.
#[cfg_attr(not(test), allow(dead_code))]
pub struct Area {
    /// The heading's control name, `Heading<key>`.
    pub key: &'static str,
    pub title: &'static str,
    pub rows: &'static [Row],
    pub pair: bool,
}

const OVERVIEW_PLAYERS: &[(&str, &str)] = &[
    ("0", "The game's own"),
    ("1", "Everyone"),
    ("2", "The other team"),
    ("3", "The watched player"),
    ("4", "Nobody"),
];
const OVERVIEW_MARKER: &[(&str, &str)] = &[
    ("0", "The game's own"),
    ("1", "HLTV: team icon"),
    ("2", "None"),
];
const OWN_BLOOD: &[(&str, &str)] = &[
    ("0", "The game's own"),
    ("1", "A little"),
    ("2", "Most"),
    ("3", "Nearly all"),
    ("4", "All"),
];
const FAST_LOAD: &[(&str, &str)] = &[
    ("0", "Off"),
    ("1", "8 ms a frame"),
    ("16", "16 ms a frame"),
    ("33", "33 ms a frame"),
];
const EX_INTERP_MAX: &[(&str, &str)] = &[
    ("100", "100 ms (stock)"),
    ("200", "200 ms"),
    ("300", "300 ms"),
    ("500", "500 ms"),
    ("1000", "1000 ms"),
];

/// The tab, top to bottom, in the order of `ui/Commands.txt`.
pub const AREAS: &[Area] = &[
    Area {
        key: "Spec",
        title: "SPECTATING (HLTV demos)",
        pair: false,
        rows: &[
            check(
                console_name!("spec_match_pov"),
                "Match the player's own view",
                "a spectated first-person view looks like the player's own recording.",
            ),
            check(
                console_name!("spec_deathcam"),
                "Show the player's death cam",
                "when the watched player dies, show the death cam their own recording shows.",
            ),
            check(
                console_name!("spec_hide_bars"),
                "Hide the spectator bars",
                "hide the spectator bars and everything on them.",
            ),
            check(
                console_name!("spec_hide_text"),
                "Hide the camera mode's name",
                "hide 'Free Look' and the rest, and 'Press DUCK for Spectator Menu'.",
            ),
            check(
                console_name!("spec_hide_hltv_messages"),
                "Hide HLTV messages",
                "hide the HLTV proxy's on-screen messages.",
            ),
        ],
    },
    Area {
        key: "Hud",
        title: "HUD",
        pair: false,
        rows: &[
            check("hud_draw", "Draw the HUD", "the game's HUD."),
            check(
                console_name!("hud_hide_scoreboard"),
                "Hide the scoreboard",
                "a POV demo's recorded TAB presses don't show the scoreboard.",
            ),
            check(
                console_name!("hud_hide_crosshair"),
                "Hide the crosshair",
                "hide the crosshair and keep it hidden.",
            ),
            check(
                console_name!("hud_hide_voice_menus"),
                "Hide the voice-command menus",
                "hide the numbered voice-command and hand-signal menus a POV demo's player opened.",
            ),
            check(
                console_name!("hud_hide_hints"),
                "Hide the hint box",
                "hide the auto-help hint box in the top-left corner.",
            ),
            check(
                console_name!("hud_hide_map_text"),
                "Hide the map's own text",
                "hide the text a map puts on screen (mortar warnings, round results).",
            ),
            check(
                console_name!("hud_hide_clan_text"),
                "Hide clan-match text",
                "hide warm-up and ready rules, Warmup Mode, the countdown, MATCH IS LIVE!",
            ),
            check(
                console_name!("hud_hide_center_text"),
                "Hide the server's centre text",
                "hide what the server prints in the middle of the screen.",
            ),
            check(
                console_name!("hud_hide_server_text"),
                "Hide server plugins' messages",
                "hide server plugins' coloured HUD messages; the map's own text stays.",
            ),
            check(
                console_name!("hud_show_scope"),
                "Show the sniper scope",
                "show the sniper scope while a demo plays.",
            ),
            choice(
                console_name!("hud_overview_players"),
                "Overview shows",
                "which players the overview map shows.",
                OVERVIEW_PLAYERS,
            ),
            choice(
                console_name!("hud_overview_team_marker"),
                "Overview icon",
                "the camera icon on the overview map.",
                OVERVIEW_MARKER,
            ),
            check(
                console_name!("hud_overview_hide_speaker"),
                "No speaker icons on the overview",
                "leave the speaker icon off the players on the overview map.",
            ),
        ],
    },
    Area {
        key: "View",
        title: "VIEW (POV demos: the recording player's own view)",
        pair: false,
        rows: &[
            check(
                "r_drawviewmodel",
                "Draw the gun",
                "the gun in first person.",
            ),
            check(
                console_name!("view_use_cl_bob"),
                "Bob by your own cl_bob",
                "the view and gun bob by your cl_bob settings, not the recorded bob.",
            ),
            check(
                console_name!("view_hide_headshot_red_screen"),
                "No red screen on head hits",
                "no red screen when the player is hit in the head.",
            ),
            check(
                console_name!("view_hide_screen_shake"),
                "No screen shake",
                "no screen shake from grenades going off nearby.",
            ),
            choice(
                console_name!("view_show_own_blood"),
                "Own blood",
                "show the player's own blood when they're hit (All is blocky up close).",
                OWN_BLOOD,
            ),
        ],
    },
    Area {
        key: "Player",
        title: "PLAYERS",
        pair: true,
        rows: &[
            check(
                console_name!("player_hide_hand_signals"),
                "Hide hand signals",
                "replace players' hand-signal animations.",
            ),
            check(
                console_name!("player_fix_model_flicker"),
                "Stop models flickering",
                "HLTV demos: strafing players' models no longer flicker for a frame.",
            ),
        ],
    },
    Area {
        key: "Map",
        title: "MAP",
        pair: false,
        rows: &[check(
            console_name!("map_allow_shaders"),
            "Allow map shaders",
            "25th Anniversary only: allow the engine's map-surface shaders during playback.",
        )],
    },
    Area {
        key: "Sound",
        title: "SOUND",
        pair: false,
        rows: &[
            check(
                console_name!("sound_mute_voice_commands"),
                "Mute voice commands",
                "silence voice commands ('fire in the hole!' and the rest).",
            ),
            check(
                console_name!("sound_mute_victory_music"),
                "Mute the round-win music",
                "silence the music the map plays when a team wins the round.",
            ),
            check(
                console_name!("sound_mute_flag_capture"),
                "Mute capture announcements",
                "silence the map's point-captured and objective-secured announcements.",
            ),
            check(
                console_name!("sound_restore_gunshots"),
                "Play gunshots a demo left out",
                "play the gunshots a demo left silent, every player's.",
            ),
        ],
    },
    Area {
        key: "Demo",
        title: "DEMO PLAYBACK",
        pair: false,
        rows: &[
            check(
                console_name!("demo_seek_skip_between"),
                "Seeking skips what it jumps over",
                "a jump skips the events and commands it passes over.",
            ),
            check(
                console_name!("demo_other_drives"),
                "Open demos on other drives",
                "viewdemo and playdemo open a demo on another drive by its full path.",
            ),
            check(
                console_name!("demo_show_load_progress"),
                "Show how far a demo has loaded",
                "the VCR bar shows how far a loading demo has got.",
            ),
            choice(
                console_name!("demo_fast_load"),
                "Fast loading",
                "viewdemo loads the demo for this long each frame; Off is the game's 33 packets a frame.",
                FAST_LOAD,
            ),
            choice(
                console_name!("demo_ex_interp_max"),
                "ex_interp up to",
                "raise the engine's 100 ms ceiling on ex_interp.",
                EX_INTERP_MAX,
            ),
        ],
    },
    Area {
        key: "Ui",
        title: "DOD STUDIO WINDOW",
        pair: false,
        rows: &[
            check(
                console_name!("ui_viewdemo_in_panel"),
                "viewdemo opens this window",
                "viewdemo opens this window on Playback instead of the VCR bar.",
            ),
            check(
                console_name!("ui_console_in_panel"),
                "The console key opens this window",
                "the console key opens this window on the Console tab.",
            ),
            check(
                console_name!("ui_resizable_windows"),
                "Resizable windows",
                "every game window can be resized by its edges.",
            ),
            check(
                console_name!("ui_remember_layout"),
                "Remember where windows were",
                "game windows come back where you left them.",
            ),
            check(
                console_name!("ui_demo_list_folders"),
                "Demo lists show folders",
                "the Demos tab and the Load Demo window list folders too.",
            ),
            check(
                console_name!("ui_demo_list_show_counts"),
                "Show folders' demo counts",
                "folder rows show their demo count. Off: no counting, so folders open faster.",
            ),
            check(
                console_name!("ui_demo_list_hide_empty"),
                "Hide empty folders",
                "a folder with no demos anywhere inside, subfolders included, isn't listed.",
            ),
            check(
                console_name!("ui_demo_list_count_subfolders"),
                "Counts include subfolders",
                "a folder's demo count includes the demos in its subfolders.",
            ),
        ],
    },
    Area {
        key: "Hd",
        title: "HD TEXTURES",
        pair: false,
        rows: &[
            check(
                console_name!("hd_enabled"),
                "HD textures",
                "HD textures (walls, detail and sky change from the next map).",
            ),
            Row {
                cvar: console_name!("hd_style"),
                label: "HD style",
                help: "which style DoD Studio's HD page built to use (walls, detail and sky change from the next map).",
                kind: Kind::Styles,
            },
        ],
    },
];

/// Settings `ui/Commands.txt` lists that the tab leaves out, and why.
#[cfg(test)]
const LEFT_OUT: &[&str] = &[
    // Studio sets these for each launch or batch.
    console_name!("spec_lock"),
    console_name!("player_overrides"),
    console_name!("capture_"),
    // Diagnostics.
    console_name!("debug_"),
];

/// Every row, in tab order.
pub fn rows() -> impl Iterator<Item = &'static Row> {
    AREAS.iter().flat_map(|a| a.rows.iter())
}

/// The row bound to `cvar`.
pub fn row(cvar: &str) -> Option<&'static Row> {
    rows().find(|r| r.cvar.eq_ignore_ascii_case(cvar))
}

/// Whether two cvar values are the same: as numbers when both are, else as
/// text, case ignored (`8` and `8.000000` are one value).
fn same_value(a: &str, b: &str) -> bool {
    match (a.trim().parse::<f64>(), b.trim().parse::<f64>()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a.trim().eq_ignore_ascii_case(b.trim()),
    }
}

/// What a dropdown shows for a cvar's value: its choice's text, or the value
/// itself when it is none of them (typed in the console or a `.cfg`).
pub fn choice_text(choices: &[(&str, &str)], value: &str) -> String {
    choices
        .iter()
        .find(|(v, _)| same_value(v, value))
        .map(|(_, text)| text.to_string())
        .unwrap_or_else(|| value.trim().to_string())
}

/// The value a dropdown's text stands for.
pub fn choice_value<'a>(choices: &[(&'a str, &'a str)], text: &str) -> Option<&'a str> {
    choices.iter().find(|(_, t)| *t == text).map(|(v, _)| *v)
}

/// What a bound dropdown needs this frame, from what it shows (`shown`), what
/// was last put in it or seen (`last`), and what the cvar's value shows as
/// (`want`).
#[derive(Debug, PartialEq)]
pub enum ChoiceStep {
    /// The user picked another entry: set the cvar to it.
    Picked,
    /// The cvar changed elsewhere, or this is the first look: show `want`.
    Show,
    /// Still waiting for what was shown to appear: keep `last`.
    Wait,
    /// Nothing to do: `shown` is what it shows.
    Nothing,
}

/// Frames to wait for a dropdown to show what it was given before taking
/// its text as the user's: a combo box may take its new text a frame or
/// more later, which would otherwise read as a pick of the old entry and
/// set the cvar straight back.
pub const SHOW_WAIT: u8 = 30;

/// [`ChoiceStep`] for this frame, and the frames left to wait (`wait`, as
/// returned the frame before).
pub fn settle_choice(shown: &str, last: Option<&str>, want: &str, wait: u8) -> (ChoiceStep, u8) {
    if let Some(was) = last
        && wait > 0
        && was != shown
    {
        // Gave up waiting: what it shows stands, and no cvar changes.
        return if wait > 1 {
            (ChoiceStep::Wait, wait - 1)
        } else {
            (ChoiceStep::Nothing, 0)
        };
    }
    match last {
        Some(was) if was != shown && !shown.is_empty() => (ChoiceStep::Picked, 0),
        _ if shown != want => (ChoiceStep::Show, SHOW_WAIT),
        _ => (ChoiceStep::Nothing, 0),
    }
}

/// The HD styles for the dropdown: those built (`found`, any order, the
/// `overrides` folder not a style), the one in use, and the default, sorted
/// and once each.
pub fn style_choices(found: &[String], current: &str, default: &str) -> Vec<String> {
    let mut styles: Vec<String> = found
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .chain([current.trim().to_ascii_lowercase(), default.to_string()])
        .filter(|s| !s.is_empty() && s != "overrides")
        .collect();
    styles.sort();
    styles.dedup();
    styles
}

/// The scroll buttons' names and commands.
pub const SCROLL_UP: &str = "SettingsUp";
pub const SCROLL_DOWN: &str = "SettingsDown";
pub const SCROLL_UP_COMMAND: &str = "settings_up";
pub const SCROLL_DOWN_COMMAND: &str = "settings_down";

/// The layout `ui/Settings.res` holds, which only the tests write: the
/// DLL loads the file (built in, or the copy in `dodstudio_ui`).
#[cfg(test)]
mod layout {
    use super::*;

    // Design pixels, as the pre-Anniversary window draws them; the 25th
    // Anniversary build scales them with its menus.
    const LEFT: i32 = 8;
    const COLUMN: i32 = 254;
    const CELL_WIDE: i32 = 246;
    const LINE: i32 = 22;
    const HEADING_TALL: i32 = 20;
    const GAP: i32 = 6;
    /// A dropdown's label takes this much of its cell; the box takes the rest.
    const CHOICE_LABEL_WIDE: i32 = 104;
    /// Where the scroll buttons sit: right of both columns.
    const SCROLL_X: i32 = LEFT + 2 * COLUMN - 2;
    const SCROLL_WIDE: i32 = 20;

    /// The note at the top of the tab, on how its values are kept.
    const NOTE: &str = "Kept for the next launch. Studio's Initial Commands still win.";
    const NOTE_HELP: &str = "A change is saved to dodstudio_ui/settings.cfg, never config.cfg, and applied when the game starts. Studio's Initial Commands, and any .cfg they exec, run at every demo load and win for the session.";

    fn control(out: &mut String, name: &str, fields: &[(&str, String)]) {
        out.push_str(&format!("\t\"{name}\"\n\t{{\n"));
        for (key, value) in fields {
            out.push_str(&format!("\t\t\"{key}\"\t\t\"{value}\"\n"));
        }
        out.push_str("\t}\n");
    }

    fn place(kind: &str, name: &str, at: (i32, i32, i32, i32)) -> Vec<(&'static str, String)> {
        vec![
            ("ControlName", kind.to_string()),
            ("fieldName", name.to_string()),
            ("xpos", at.0.to_string()),
            ("ypos", at.1.to_string()),
            ("wide", at.2.to_string()),
            ("tall", at.3.to_string()),
            ("visible", "1".to_string()),
            ("enabled", "1".to_string()),
        ]
    }

    fn cell(out: &mut String, row: &Row, x: i32, y: i32) {
        let name = format!("cvar_{}", row.cvar);
        let help = format!("{}: {}", row.cvar, row.help);
        match row.kind {
            Kind::Check => {
                let mut f = place("CheckButton", &name, (x, y, CELL_WIDE, LINE));
                f.push(("labelText", row.label.to_string()));
                f.push(("textAlignment", "west".to_string()));
                f.push(("helptext", help));
                control(out, &name, &f);
            }
            Kind::Choice(_) | Kind::Styles => {
                let label = format!("label_{}", row.cvar);
                let mut f = place("Label", &label, (x + 4, y, CHOICE_LABEL_WIDE - 4, LINE));
                f.push(("labelText", row.label.to_string()));
                f.push(("textAlignment", "west".to_string()));
                f.push(("helptext", help.clone()));
                control(out, &label, &f);
                let mut f = place(
                    "ComboBox",
                    &name,
                    (
                        x + CHOICE_LABEL_WIDE,
                        y + 1,
                        CELL_WIDE - CHOICE_LABEL_WIDE,
                        LINE - 2,
                    ),
                );
                f.push(("editable", "0".to_string()));
                f.push(("helptext", help));
                control(out, &name, &f);
            }
        }
    }

    fn heading(out: &mut String, area: &Area, x: i32, y: i32, wide: i32) {
        let name = format!("Heading{}", area.key);
        let mut f = place("Label", &name, (x, y, wide, HEADING_TALL));
        f.push(("labelText", area.title.to_string()));
        f.push(("textAlignment", "west".to_string()));
        control(out, &name, &f);
    }

    /// `ui/Settings.res`, from [`AREAS`]: the note, then each area's heading
    /// and its rows two to a line (a paired area takes a column each), then
    /// Reset, and the scroll buttons beside it all.
    pub(super) fn settings_res() -> String {
        let mut out = String::from("\"dodstudio_ui/Settings.res\"\n{\n");
        let mut f = place("Label", "Note", (LEFT, 4, 2 * COLUMN - GAP, HEADING_TALL));
        f.push(("labelText", NOTE.to_string()));
        f.push(("textAlignment", "west".to_string()));
        f.push(("helptext", NOTE_HELP.to_string()));
        control(&mut out, "Note", &f);
        let mut y = 4 + HEADING_TALL + GAP;
        let mut areas = AREAS.iter();
        while let Some(area) = areas.next() {
            if area.pair
                && let Some(other) = areas.next()
            {
                let mut bottom = y;
                for (column, a) in [area, other].into_iter().enumerate() {
                    let x = LEFT + column as i32 * COLUMN;
                    heading(&mut out, a, x, y, CELL_WIDE);
                    let mut row_y = y + HEADING_TALL;
                    for row in a.rows {
                        cell(&mut out, row, x, row_y);
                        row_y += LINE;
                    }
                    bottom = bottom.max(row_y);
                }
                y = bottom + GAP;
                continue;
            }
            heading(&mut out, area, LEFT, y, 2 * COLUMN - GAP);
            y += HEADING_TALL;
            for pair in area.rows.chunks(2) {
                for (column, row) in pair.iter().enumerate() {
                    cell(&mut out, row, LEFT + column as i32 * COLUMN, y);
                }
                y += LINE;
            }
            y += GAP;
        }
        let mut f = place("Button", "ResetSettingsButton", (LEFT, y, 128, 24));
        f.push(("labelText", "Reset to defaults".to_string()));
        f.push(("textAlignment", "center".to_string()));
        f.push(("Command", "reset_settings".to_string()));
        f.push((
            "helptext",
            "Put every setting changed here back to its default, and forget the saved values."
                .to_string(),
        ));
        control(&mut out, "ResetSettingsButton", &f);
        for (name, command, text, at_y) in [
            (SCROLL_UP, SCROLL_UP_COMMAND, "▲", 4),
            (SCROLL_DOWN, SCROLL_DOWN_COMMAND, "▼", 218),
        ] {
            let mut f = place("Button", name, (SCROLL_X, at_y, SCROLL_WIDE, LINE));
            f.push(("labelText", text.to_string()));
            f.push(("textAlignment", "center".to_string()));
            f.push(("Command", command.to_string()));
            f.push((
                "helptext",
                "Scroll the settings (the mouse wheel does too).".to_string(),
            ));
            control(&mut out, name, &f);
        }
        out.push_str("}\n");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::layout::settings_res;
    use super::*;

    const SETTINGS_RES: &str = include_str!("../ui/Settings.res");
    const COMMANDS_TXT: &str = include_str!("../ui/Commands.txt");

    /// `ui/Settings.res` is [`settings_res`]'s output. Run with
    /// `DS_WRITE_SETTINGS_RES=1` to write it after changing [`AREAS`].
    #[test]
    fn the_settings_layout_is_generated_from_the_list() {
        let want = settings_res();
        if std::env::var_os("DS_WRITE_SETTINGS_RES").is_some() {
            let path = concat!(env!("CARGO_MANIFEST_DIR"), "/ui/Settings.res");
            std::fs::write(path, &want).expect("write ui/Settings.res");
            return;
        }
        assert!(
            SETTINGS_RES.replace('\r', "") == want,
            "ui/Settings.res is out of date: run `DS_WRITE_SETTINGS_RES=1 cargo test -p goldsrc-hooks settings_tab`"
        );
    }

    /// The settings `ui/Commands.txt` lists: a name and one value.
    fn listed_settings() -> Vec<&'static str> {
        COMMANDS_TXT
            .lines()
            .filter_map(|l| l.strip_prefix("  "))
            .filter(|l| !l.starts_with(' '))
            .filter_map(|l| {
                let (name, args) = l.split_once(' ')?;
                let value = matches!(args, "0/1" | "<ms>" | "<name>" | "0/1/<ms>")
                    || args
                        .strip_prefix("0-")
                        .is_some_and(|n| n.parse::<u8>().is_ok());
                (name.starts_with(console_name!("")) && value).then_some(name)
            })
            .collect()
    }

    /// A setting a PR adds to `Commands.txt` needs a row here (or a reason
    /// in [`LEFT_OUT`]), so the tab never falls behind again.
    #[test]
    fn every_listed_setting_is_on_the_tab_or_left_out_on_purpose() {
        let listed = listed_settings();
        assert!(listed.len() > 30, "{listed:?}");
        for name in &listed {
            let left_out = LEFT_OUT.iter().any(|l| name.starts_with(l));
            assert!(
                left_out || row(name).is_some(),
                "{name} is in Commands.txt but not on the Settings tab: add a row to settings_tab::AREAS"
            );
        }
        for row in rows().filter(|r| r.cvar.starts_with(console_name!(""))) {
            assert!(
                listed.contains(&row.cvar),
                "{} is on the Settings tab but not a setting in Commands.txt",
                row.cvar
            );
        }
        assert!(row(console_name!("spec_lock")).is_none());
        assert!(row(console_name!("hud_hide_hints")).is_some());
    }

    /// Each row's area is the word after the prefix, as the names are
    /// grouped (#555).
    #[test]
    fn every_row_sits_under_its_names_area() {
        for area in AREAS {
            let word = match area.key {
                "Spec" => "spec_",
                "Hud" => "hud_",
                "View" => "view_",
                "Player" => "player_",
                "Map" => "map_",
                "Sound" => "sound_",
                "Demo" => "demo_",
                "Ui" => "ui_",
                "Hd" => "hd_",
                other => panic!("no area word for {other}"),
            };
            for row in area.rows {
                let own = row
                    .cvar
                    .strip_prefix(console_name!(""))
                    .is_some_and(|rest| rest.starts_with(word));
                let game = !row.cvar.starts_with(console_name!(""));
                assert!(own || game, "{} is under {}", row.cvar, area.title);
            }
        }
    }

    #[test]
    fn every_row_is_listed_once() {
        let names: Vec<&str> = rows().map(|r| r.cvar).collect();
        for name in &names {
            assert_eq!(names.iter().filter(|n| *n == name).count(), 1, "{name}");
        }
    }

    #[test]
    fn a_dropdown_shows_its_value_or_the_value_itself() {
        assert_eq!(choice_text(FAST_LOAD, "1"), "8 ms a frame");
        assert_eq!(choice_text(FAST_LOAD, "16.000000"), "16 ms a frame");
        assert_eq!(choice_text(FAST_LOAD, "12"), "12");
        assert_eq!(choice_value(FAST_LOAD, "33 ms a frame"), Some("33"));
        assert_eq!(choice_value(FAST_LOAD, "12"), None);
        assert_eq!(
            choice_text(&[("ultrasharp", "ultrasharp")], "UltraSharp"),
            "ultrasharp"
        );
    }

    #[test]
    fn a_pick_sets_the_cvar_and_the_cvar_moves_the_dropdown() {
        // First look: show the cvar's value.
        assert_eq!(
            settle_choice("", None, "Off", 0),
            (ChoiceStep::Show, SHOW_WAIT)
        );
        // The user picked another entry.
        assert_eq!(
            settle_choice("Most", Some("All"), "All", 0),
            (ChoiceStep::Picked, 0)
        );
        // The cvar changed elsewhere.
        assert_eq!(
            settle_choice("All", Some("All"), "Most", 0),
            (ChoiceStep::Show, SHOW_WAIT)
        );
        assert_eq!(
            settle_choice("All", Some("All"), "All", 0),
            (ChoiceStep::Nothing, 0)
        );
    }

    /// A box that takes its new text late isn't read as the user picking
    /// the old entry, which would set the cvar back.
    #[test]
    fn a_dropdown_that_shows_late_is_not_a_pick() {
        // "Most" was asked for; it still says "All".
        assert_eq!(
            settle_choice("All", Some("Most"), "Most", 30),
            (ChoiceStep::Wait, 29)
        );
        // It caught up.
        assert_eq!(
            settle_choice("Most", Some("Most"), "Most", 29),
            (ChoiceStep::Nothing, 0)
        );
        // It never did: what it shows stands, no cvar set.
        assert_eq!(
            settle_choice("All", Some("Most"), "Most", 1),
            (ChoiceStep::Nothing, 0)
        );
    }

    #[test]
    fn the_style_list_holds_what_is_built_the_one_in_use_and_the_default() {
        let found = [
            "remacri".to_string(),
            "overrides".to_string(),
            "Plain".to_string(),
        ];
        assert_eq!(
            style_choices(&found, "mine", "ultrasharp"),
            ["mine", "plain", "remacri", "ultrasharp"]
        );
        assert_eq!(
            style_choices(&[], "ultrasharp", "ultrasharp"),
            ["ultrasharp"]
        );
    }

    #[test]
    fn the_layout_fits_two_columns_and_the_scroll_buttons() {
        let res = settings_res();
        let widest = res
            .split("\"xpos\"")
            .skip(1)
            .zip(res.split("\"wide\"").skip(1))
            .map(|(x, w)| {
                let n = |s: &str| s.split('"').nth(1).unwrap().parse::<i32>().unwrap();
                n(x) + n(w)
            })
            .max()
            .unwrap();
        // The tab is 542 wide in the default window.
        assert!(widest <= 534, "{widest}");
    }
}
