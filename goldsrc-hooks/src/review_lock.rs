//! While Review highlights runs, the game's own demo controls are locked
//! (#826): the user can't seek, pause, change speed or load another demo from
//! the VCR bar, the Load Demo window or DoD Studio's in-game window, so the
//! only ways to move are the review's own (Yes / Maybe / No, Replay, Back,
//! Next, Stop and the Review tab's boxes).
//!
//! The lock lasts as long as the review holds `spectator_follow`'s lock (the
//! same lifetime as `spectator_buttons`): from `dodstudio_review start` to the
//! frame after it stops or ends. ESC, the console and the Settings tab are
//! left alone; Stop on the Review tab is the way out.
//!
//! This module holds what is decided by name, so the tests can check it; the
//! greying itself (vgui2's `Panel::SetEnabled`) is
//! `studio_panel/hook/lock_controls.rs`, and the code paths DoD Studio owns
//! (its tabs' Go / Load / Goto handlers) refuse on [`locked`] as well, since a
//! list's double-click doesn't check whether it is enabled.

// Its callers are the 32-bit window's; a host build compiles it for the tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

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
