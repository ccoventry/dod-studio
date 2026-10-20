//! While Review highlights runs, the game's own demo controls are locked
//! (#826): the user can't seek, pause, change speed or load another demo from
//! the VCR bar, the Load Demo window or DoD Studio's in-game window, so the
//! only ways to move are the review's own (Yes / Maybe / No, Replay, Back,
//! Next, Stop and the Review tab's boxes).
//!
//! The lock lasts as long as the review holds `spectator_follow`'s lock (the
//! same lifetime as `spectator_buttons`): from `dodstudio_review start` to the
//! frame after it stops or ends. ESC and the Settings tab are left alone;
//! Stop on the Review tab is the way out.
//!
//! The console is locked the same way (#855): the commands and cvars that
//! move the demo or the camera ([`CONSOLE_DEMO`], [`CONSOLE_CAMERA`],
//! [`GUARDED_CVARS`]) are refused with [`console_note`], whoever types,
//! binds or sends them. The review's own `viewdemo` and camera line go
//! through the same commands, so it names them first ([`let_through`]); its
//! seeks, speed and pause call the demo player directly and never meet the
//! lock. The wrapping and the cvar guard are [`console`].
//!
//! This module holds what is decided by name, so the tests can check it; the
//! greying itself (vgui2's `Panel::SetEnabled`) is
//! `studio_panel/hook/lock_controls.rs`, and the code paths DoD Studio owns
//! (its tabs' Go / Load / Goto handlers) refuse on [`locked`] as well, since a
//! list's double-click doesn't check whether it is enabled.

// Its callers are the 32-bit window's; a host build compiles it for the tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

pub mod console;

/// Whether a review is running, so the controls are locked.
pub fn locked() -> bool {
    crate::spectator_follow::review_holds()
}

/// What a refused click or key prints to the console.
pub const LOCKED_NOTE: &str =
    "the demo controls are locked while a review runs -- Stop on the Review tab unlocks them";

/// Where a lockable control sits: which window or tab it is a child of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holder {
    /// The game's VCR bar (GameUI's `CDemoPlayerDialog`).
    VcrBar,
    /// The game's own Load Demo window (`CDemoPlayerFileDialog`).
    LoadDemo,
    /// One of the hidden Load Demo windows that lend the Demos and
    /// Highlights tabs their list and Load / Go button, while it holds them.
    LendingList,
    /// DoD Studio's in-game window's Playback tab.
    PlaybackTab,
    /// Its Demos or Highlights tab (the borrowed Load / Go button).
    ListTab,
}

/// The VCR bar's panel name, and the Load Demo window's (both builds'
/// `GameUI.dll` name their panels so).
pub const VCR_BAR: &str = "DemoPlayerDialog";
pub const LOAD_DEMO: &str = "DemoPlayerFileDialog";

/// The time slider, lent to the Playback tab while that setting is on.
const TIME_SLIDER: &str = "TimeSlider";
/// The Load Demo window's button, lent (as Load or Go) to the Demos and
/// Highlights tabs.
const LOAD_BUTTON: &str = "LoadButton";

/// The Playback tab's controls that seek, pause, change speed or load
/// (`ui/Playback.res`). Clear decals and "use this tab" stay.
const PLAYBACK_LOCKED: &[&str] = &[
    "startButton",
    "slowerButton",
    "stepbButton",
    "pauseButton",
    "playButton",
    "stepfButton",
    "fasterButton",
    "endButton",
    "stopButton",
    "LoadButton",
    "EventsButton",
    "Back10Button",
    "Forward10Button",
    "HalfSpeedButton",
    "NormalSpeedButton",
    "GotoTime",
    "GotoButton",
    "ReloadButton",
    TIME_SLIDER,
];

/// Whether the control `name`, a child of `holder`, is greyed out during a
/// review. On the VCR bar that is the slider and every button: the stock
/// ones (`valve/resource/DemoPlayerDialog.res`: Load, Start, Slower, StepB,
/// Pause, Play, StepF, Faster, End, Stop, Master, Events, Save) and any a
/// layout adds, which by GameUI's own naming end in "Button". The window's
/// own frame buttons (`frame_close` and the like) and the labels don't.
pub fn locks(holder: Holder, name: &str) -> bool {
    match holder {
        Holder::VcrBar => {
            name.eq_ignore_ascii_case(TIME_SLIDER) || name.to_ascii_lowercase().ends_with("button")
        }
        Holder::LoadDemo | Holder::LendingList | Holder::ListTab => {
            name.eq_ignore_ascii_case(LOAD_BUTTON)
        }
        Holder::PlaybackTab => PLAYBACK_LOCKED
            .iter()
            .any(|locked| locked.eq_ignore_ascii_case(name)),
    }
}

/// The Review tab's controls that only act on a running review
/// (`ui/Review.res`): the answers, the moves, Stop and the boxes the answer
/// is read from. The other way round from the lock: they are greyed out
/// while no review runs, so the tab looks idle when it is.
pub const REVIEW_ONLY: &[&str] = &[
    "YesButton",
    "MaybeButton",
    "NoButton",
    "ReplayButton",
    "BackButton",
    "NextButton",
    "StopButton",
    "ReviewFrom",
    "ReviewTo",
    "ReviewNote",
];

/// Whether a console line a Playback tab button runs (`engine <line>`) moves
/// the demo: a seek, a speed change, a reload or another demo.
pub fn moves_the_demo(line: &str) -> bool {
    let Some(first) = line.split_whitespace().next() else {
        return false;
    };
    [
        crate::demo_seek::SEEK_TO_NAME,
        crate::demo_seek::SEEK_BY_NAME,
        crate::demo_seek::SKIP_BETWEEN_NAME,
        crate::demo_reload::NAME,
        "dem_speed",
        "demo_pause",
        "demo_resume",
        "demo_togglepause",
        "viewdemo",
        "playdemo",
        "timedemo",
        "stop",
        "disconnect",
    ]
    .iter()
    .any(|name| name.eq_ignore_ascii_case(first))
}

/// Console commands refused while a review runs (#855), because each moves
/// the demo: seeks, speed, pause, or another demo (or no demo) in its place.
/// Typed, bound, run from a `.cfg` or sent over the game's pipe, they all go
/// through the same handler, so all are refused; the review's own lines are
/// let through ([`let_through`]). `dem_*` are DemoPlayer.dll's (both builds);
/// the rest the engine's, read from `cmdlist` on both builds. `quit` stays.
pub const CONSOLE_DEMO: &[&str] = &[
    crate::demo_seek::SEEK_TO_NAME,
    crate::demo_seek::SEEK_BY_NAME,
    crate::demo_reload::NAME,
    "dem_jump",
    "dem_speed",
    "dem_pause",
    "dem_start",
    "viewdemo",
    "playdemo",
    "timedemo",
    "stopdemo",
    "startdemos",
    "demos",
    "stop",
    "disconnect",
    "connect",
    "retry",
    "reconnect",
    "map",
    "changelevel",
    "changelevel2",
    "restart",
    "reload",
    "load",
];

/// Console commands refused while a review runs because each moves the
/// camera: the spectator mode and target, the director, a third-person or
/// free camera, or playing a POV demo as HLTV.
pub const CONSOLE_CAMERA: &[&str] = &[
    crate::spectator_follow::TARGET_NAME,
    "spec_mode",
    "spec_autodirector",
    "dem_forcehltv",
    "thirdperson",
    "mirv_input",
    "mirv_campath",
];

/// Cvars that move the demo or the camera. A cvar has no handler to wrap, so
/// a typed value is put back the frame it changes: `host_framerate` sets
/// how much demo time each frame plays, `spec_autodirector_internal` lets
/// the demo's director move the camera. (`chase_active` would be one, but the
/// engine itself refuses it in a demo: "Can't set chase_active in
/// multiplayer".) The note is printed once per cvar per review, since HLAE
/// sets `host_framerate` every frame while it records.
pub const GUARDED_CVARS: &[&str] = &["host_framerate", "spec_autodirector_internal"];

/// Whether the console command `name` is refused while a review runs.
pub fn console_locks(name: &str) -> bool {
    CONSOLE_DEMO
        .iter()
        .chain(CONSOLE_CAMERA)
        .any(|locked| locked.eq_ignore_ascii_case(name))
}

/// The line a refused command or cvar prints to the console.
pub fn console_note(name: &str) -> String {
    format!(
        "{name} is locked while a review runs -- Stop on the Review tab, or {} stop, ends the review\n",
        crate::review::NAME
    )
}

/// The lockable commands in a console line, lowercased, in order: the first
/// word of each `;`- or newline-separated command, quotes respected as the
/// engine's own split does.
pub fn lockable_in(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut quoted = false;
    let mut start = 0;
    let mut push = |part: &str| {
        if let Some(first) = part.split_whitespace().next()
            && console_locks(first)
        {
            out.push(first.to_ascii_lowercase());
        }
    };
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ';' | '\n' if !quoted => {
                push(&line[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    push(&line[start..]);
    out
}

/// The review's own locked commands, queued and not yet run. Commands run
/// on the game's main thread; the lock is never held across a call out.
static OWN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn own() -> std::sync::MutexGuard<'static, Vec<String>> {
    OWN.lock().unwrap_or_else(|e| e.into_inner())
}

/// Called with a console line the hook itself is about to queue (the
/// review's `viewdemo`, its camera line after a seek): each lockable command
/// in it runs once without being refused. Nothing while no review runs.
pub fn let_through(line: &str) {
    if locked() {
        own().extend(lockable_in(line));
    }
}

/// Forgets the review's queued lines, once it ends.
pub fn forget_own() {
    own().clear();
}

/// Whether the command `name`, about to run, is refused: a locked one while
/// a review runs, unless it is one the review queued itself.
pub fn refuses(name: &str) -> bool {
    decide(locked(), &mut own(), name)
}

fn decide(locked: bool, own: &mut Vec<String>, name: &str) -> bool {
    if !locked || !console_locks(name) {
        return false;
    }
    match own.iter().position(|n| n.eq_ignore_ascii_case(name)) {
        Some(at) => {
            own.remove(at);
            false
        }
        None => true,
    }
}

/// One control the lock greyed out, and whether it was enabled before, so
/// the release puts back exactly that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    pub panel: u32,
    pub name: String,
    pub was_enabled: bool,
}

/// What to do with a lockable control found this frame while locked, given
/// what the lock already holds: `Some(was)` to remember a new one, and
/// whether to grey it out now (GameUI may have enabled it again).
pub fn on_sight(held: &[Held], panel: u32, name: &str, enabled: bool) -> (Option<bool>, bool) {
    let known = held.iter().any(|h| h.panel == panel && h.name == name);
    ((!known).then_some(enabled), enabled)
}

/// Whether a control found at release gets enabled again: only one the lock
/// greyed out (same panel and name) that was enabled before it.
pub fn restores(held: &[Held], panel: u32, name: &str) -> bool {
    held.iter()
        .any(|h| h.panel == panel && h.name == name && h.was_enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stock_vcr_bar_control_that_moves_the_demo_is_locked() {
        for name in [
            "LoadButton",
            "StartButton",
            "SlowerButton",
            "StepBButton",
            "PauseButton",
            "PlayButton",
            "StepFButton",
            "FasterButton",
            "EndButton",
            "StopButton",
            "TimeSlider",
            "MasterButton",
            "EventsButton",
            "SaveButton",
            // dod_addon's own -10 s / +10 s row.
            "Back10Button",
        ] {
            assert!(locks(Holder::VcrBar, name), "{name}");
        }
    }

    #[test]
    fn the_bar_keeps_its_labels_and_frame_chrome() {
        for name in [
            "TimeLabel",
            "frame_close",
            "frame_minimize",
            "frame_menu",
            "frame_caption",
            "frame_topGrip",
            "StudioLoadPercent",
            "StudioLoadProgress",
        ] {
            assert!(!locks(Holder::VcrBar, name), "{name}");
        }
    }

    #[test]
    fn the_playback_tab_locks_what_moves_the_demo_and_keeps_the_rest() {
        let res = include_str!("../ui/Playback.res");
        for name in PLAYBACK_LOCKED {
            if *name != TIME_SLIDER {
                assert!(
                    res.contains(&format!("\"{name}\"")),
                    "{name} is not in Playback.res"
                );
            }
            assert!(locks(Holder::PlaybackTab, name));
        }
        for name in ["DecalsButton", "EnablePlaybackButton", "TimeLabel", "Hint"] {
            assert!(!locks(Holder::PlaybackTab, name), "{name}");
        }
    }

    #[test]
    fn the_list_tabs_lock_only_their_borrowed_load_button() {
        for holder in [Holder::ListTab, Holder::LendingList, Holder::LoadDemo] {
            assert!(locks(holder, "LoadButton"));
            for name in [
                "DemoList",
                "DemoBack",
                "DemoUp",
                "DemoPlace",
                "StreakPlayer",
            ] {
                assert!(!locks(holder, name), "{name}");
            }
        }
    }

    #[test]
    fn the_review_only_controls_are_all_on_the_review_tab() {
        let res = include_str!("../ui/Review.res");
        for name in REVIEW_ONLY {
            assert!(
                res.contains(&format!("\"{name}\"")),
                "{name} is not in Review.res"
            );
        }
        // The labels stay: they say there is no review.
        for name in ["ReviewHeading", "ReviewDetail", "ReviewHint"] {
            assert!(!REVIEW_ONLY.contains(&name), "{name}");
        }
    }

    #[test]
    fn seeks_speed_and_loads_move_the_demo() {
        for line in [
            "dodstudio_seek_by -10",
            "dodstudio_seek_by 10",
            "dodstudio_seek_to 1850",
            "dem_speed 0.5",
            "dodstudio_reload_demo",
            "viewdemo x",
        ] {
            assert!(moves_the_demo(line), "{line}");
        }
        for line in [
            "dodstudio_clear_decals",
            "dodstudio_viewdemo_in_panel 1",
            "",
            "  ",
        ] {
            assert!(!moves_the_demo(line), "{line:?}");
        }
    }

    #[test]
    fn every_seek_load_and_camera_command_is_locked_at_the_console() {
        for name in [
            "dodstudio_seek_to",
            "dodstudio_seek_by",
            "dodstudio_reload_demo",
            "DEM_JUMP",
            "dem_speed",
            "dem_pause",
            "viewdemo",
            "playdemo",
            "stop",
            "disconnect",
            "dodstudio_spec_target",
            "spec_mode",
            "spec_autodirector",
            "mirv_input",
        ] {
            assert!(console_locks(name), "{name}");
        }
        for name in [
            crate::review::NAME,
            "quit",
            "dodstudio_panel",
            "dodstudio_clear_decals",
            "toggleconsole",
            "+attack",
            "spec_pip",
            "echo",
            "dodstudio_seek_skip_between",
        ] {
            assert!(!console_locks(name), "{name}");
        }
    }

    #[test]
    fn the_console_lock_knows_every_command_the_controls_lock() {
        // What the Playback tab's buttons refuse, where the name exists in a
        // build (hw.dll and DemoPlayer.dll have no `demo_pause` family).
        for line in [
            "dodstudio_seek_to 1",
            "dodstudio_seek_by 1",
            "dodstudio_reload_demo",
            "dem_speed 1",
            "viewdemo x",
            "playdemo x",
            "timedemo x",
            "stop",
            "disconnect",
        ] {
            assert!(moves_the_demo(line));
            assert_eq!(lockable_in(line).len(), 1, "{line}");
        }
    }

    #[test]
    fn a_line_names_each_locked_command_it_runs() {
        assert_eq!(
            lockable_in("spec_mode 4; dodstudio_spec_target 7\n"),
            ["spec_mode", "dodstudio_spec_target"]
        );
        assert_eq!(lockable_in("viewdemo \"../a;b/demo\"\n"), ["viewdemo"]);
        assert_eq!(
            lockable_in("echo hi; Dem_Speed 2;dodstudio_panel review"),
            ["dem_speed"]
        );
        assert!(lockable_in("").is_empty());
    }

    #[test]
    fn the_reviews_own_command_runs_once_and_a_typed_one_is_refused() {
        let mut own = vec!["spec_mode".to_string(), "viewdemo".to_string()];
        assert!(!decide(true, &mut own, "SPEC_MODE"));
        assert_eq!(own, ["viewdemo"]);
        // The same command again is the user's.
        assert!(decide(true, &mut own, "spec_mode"));
        assert!(!decide(true, &mut own, "viewdemo"));
        assert!(decide(true, &mut own, "viewdemo"));
        // Not locked: nothing is refused or used up.
        let mut own = vec!["spec_mode".to_string()];
        assert!(!decide(false, &mut own, "spec_mode"));
        assert_eq!(own.len(), 1);
        // A command the lock doesn't cover always runs.
        assert!(!decide(true, &mut own, "dodstudio_panel"));
    }

    #[test]
    fn the_note_says_how_to_end_the_review() {
        let note = console_note("spec_mode");
        assert!(note.starts_with("spec_mode is locked while a review runs"));
        assert!(note.contains("dodstudio_review stop"));
        assert!(note.ends_with('\n'));
    }

    #[test]
    fn a_control_is_remembered_once_and_greyed_whenever_enabled() {
        let mut held = Vec::new();
        assert_eq!(on_sight(&held, 7, "PlayButton", true), (Some(true), true));
        held.push(Held {
            panel: 7,
            name: "PlayButton".into(),
            was_enabled: true,
        });
        // Already greyed: nothing to do.
        assert_eq!(on_sight(&held, 7, "PlayButton", false), (None, false));
        // GameUI enabled it again: grey it, keep the first state.
        assert_eq!(on_sight(&held, 7, "PlayButton", true), (None, true));
        // A control that was disabled before the review stays so.
        assert_eq!(
            on_sight(&held, 8, "SaveButton", false),
            (Some(false), false)
        );
    }

    #[test]
    fn the_release_enables_only_what_was_enabled_before() {
        let held = vec![
            Held {
                panel: 7,
                name: "PlayButton".into(),
                was_enabled: true,
            },
            Held {
                panel: 8,
                name: "SaveButton".into(),
                was_enabled: false,
            },
        ];
        assert!(restores(&held, 7, "PlayButton"));
        assert!(!restores(&held, 8, "SaveButton"));
        // A new panel at a reused handle is not the one greyed.
        assert!(!restores(&held, 7, "TimeSlider"));
        assert!(!restores(&held, 9, "PlayButton"));
    }
}
