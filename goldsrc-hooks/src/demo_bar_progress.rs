//! `ds_demo_show_load_progress`: the stock demo player bar says how far a
//! `viewdemo` load has got (#465).
//!
//! While the demo player reads the file its title says "Loading <demo> ...",
//! and nothing says how far along it is. DoD Studio's launch adds two hidden
//! controls to the bar's layout (`native/src/demo_bar_res.rs`, into
//! `dod_addon\resource\DemoPlayerDialog.res`): a `Label` named
//! [`PERCENT_LABEL`] and a `ProgressBar` named [`PROGRESS_BAR`], where the
//! time label sits. Every frame while the player is loading, the vgui side
//! (`studio_panel/hook/demo_bar_progress.rs`) writes `n%` into the label,
//! sets the bar and shows both; once loaded they hide again and the time
//! label shows through. The percent is the one the DoD Studio window's
//! Playback tab shows: the buffered span over the demo's own playback length.
//!
//! On by default like `ds_ui_viewdemo_in_panel`: without the layout
//! Studio ships there is nothing to show, so it costs nothing elsewhere.
//! With `ds_ui_viewdemo_in_panel 1` the bar is parked off screen and the
//! Playback tab shows the progress instead, so this does nothing then.

// Only the 32-bit build has the bar; a host check still compiles this.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use crate::demo_list_folders::Setting;
use crate::names::console_name;

pub const NAME: &str = console_name!("demo_show_load_progress");

/// The two controls' panel names, as `demo_bar_res` writes them.
pub const PERCENT_LABEL: &str = "StudioLoadPercent";
pub const PROGRESS_BAR: &str = "StudioLoadProgress";

/// `ds_demo_show_load_progress 1` (the default).
pub static SETTING: Setting = Setting::new(NAME, true);

/// For the fallback toggle's bare-name query.
pub fn status() -> String {
    if SETTING.on() {
        "the demo bar shows how far a loading demo has got".to_string()
    } else {
        "the demo bar shows no loading progress".to_string()
    }
}

/// The label's text for a percent.
pub fn percent_text(percent: u32) -> String {
    format!("{percent}%")
}

/// Whether a change from `was` to `now` is worth a log line: each tenth,
/// and the end.
pub fn worth_logging(was: Option<u32>, now: Option<u32>) -> bool {
    match (was, now) {
        (None, None) => false,
        (Some(_), None) => true,
        (was, Some(p)) => was.is_none_or(|w| p / 10 != w / 10),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_each_tenth_and_the_end() {
        assert!(worth_logging(None, Some(0)));
        assert!(!worth_logging(Some(3), Some(7)));
        assert!(worth_logging(Some(9), Some(10)));
        assert!(worth_logging(Some(90), None));
        assert!(!worth_logging(None, None));
    }

    #[test]
    fn the_label_is_the_bare_percent() {
        assert_eq!(percent_text(47), "47%");
    }
}
