//! Lays out the top of the screen while spectating, so nothing sits under the
//! spectator bar.
//!
//! ## What the game does
//!
//! While the spectator camera is up (`g_iUser1` non-zero), DoD pushes the
//! top-of-screen HUD down by a fixed share of the screen height, but each
//! element by its own share, and none of them by the bar's:
//!
//! | element | POV demo y | spectating y |
//! | --- | --- | --- |
//! | spectator top bar (`TopBar`) | -- | 0 to `64 * H / 480` (`client+0x82dae`) |
//! | objective icons | `round(2 * H / 480)` | `round(54 * H / 480)` |
//! | objective timer | 2 | `round(54 * H / 480)` |
//! | kill feed | 20 | `round(42 * H / 480) + 20` (`client+0x2aefc`) |
//! | minimap (`_cl_minimap 2`) | -- | cached y + `round(54 * H / 480)` (`client+0x22ac1`) |
//!
//! So at 1280x716 the bar ends at 95 and the objectives start at 81: they draw
//! over it. With `dodstudio_hide_spectator_bars 1` the bar is gone but the
//! gap stays.
//!
//! ## What this does instead
//!
//! Every frame while spectating, it puts each element where it sits in a POV
//! demo, moved down by the bar's real height, or by nothing while the bar is
//! hidden:
//!
//! - the objective icons and timer, through `dodstudio_objectives`' detours;
//! - the kill feed, through `dodstudio_deathmsg offset`'s detour, except while
//!   the minimap is up: the game then puts the feed under the minimap itself
//!   (`round(2 * H / 480)` below its bottom edge), and moving the minimap
//!   moves it;
//! - the minimap, by shifting its cached rect the way `dodstudio_overviewmap`
//!   writes it.
//!
//! A value typed with any of those three commands still wins. `_cl_minimap 1`
//! (the full map) is centred and clears the bar on its own, so it is left
//! alone; the objective icons drawn on either map keep their map positions.
//! Not spectating, every element goes back to the game.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::engine;

/// `ScreenHeight` in `client.dll`'s `.data`, the integer every y above is
/// scaled from (the `fild` operand at `+0x2aefc` and in each formula).
const SCREEN_HEIGHT_RVA: usize = 0x17_60c4;

/// The top bar's height in 480-line units: `GetProportionalScaledValue(64)`
/// in `CSpectatorGUI`'s constructor (`client+0x82dae`).
const BAR_UNITS: f32 = 64.0;
/// How far the game pushes the objectives and the minimap while spectating.
const GAME_PUSH_UNITS: f32 = 54.0;

/// The y each element sits at in a POV demo.
const ICON_UNITS: f32 = 2.0;
const TIMER_Y: i32 = 2;
const FEED_Y: i32 = 20;

/// Whether the layout was applied last frame, so leaving the spectator camera
/// hands everything back once rather than every frame.
static ACTIVE: AtomicBool = AtomicBool::new(false);
static SHIFT_FAILED: AtomicBool = AtomicBool::new(false);

/// Where each element goes for one screen height.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Layout {
    icon_y: i32,
    timer_y: i32,
    feed_y: i32,
    /// Added to the minimap's cached y.
    mini_shift: i32,
}

/// `round(units * height / 480)` the way the game computes it: `height *
/// (1/480) * units + 0.5`, truncated.
fn game_scaled(units: f32, height: i32) -> i32 {
    (height as f32 * (1.0 / 480.0) * units + 0.5) as i32
}

/// VGUI2's proportional scaling, which truncates.
fn bar_height(height: i32) -> i32 {
    (BAR_UNITS * height as f32 / 480.0) as i32
}

fn layout(height: i32, bar_shown: bool) -> Layout {
    let bar = if bar_shown { bar_height(height) } else { 0 };
    Layout {
        icon_y: bar + game_scaled(ICON_UNITS, height),
        timer_y: bar + TIMER_Y,
        feed_y: bar + FEED_Y,
        mini_shift: bar - game_scaled(GAME_PUSH_UNITS, height),
    }
}

/// Called every frame from `commands::poll`, after `spectator_bars::poll`.
pub fn poll() {
    let spectating = crate::spectator_target::mode().is_some_and(|mode| mode != 0);
    if !spectating {
        if ACTIVE.swap(false, Ordering::Relaxed) {
            crate::objicons::set_auto(None, None);
            crate::deathmsg::set_auto_offset(None);
            crate::overview_map::unshift_mini();
        }
        return;
    }
    let Some(base) = engine::client_module_base() else {
        return;
    };
    // Safety: a dword in client.dll's .data, read-only here.
    let height = unsafe { ((base + SCREEN_HEIGHT_RVA) as *const i32).read_unaligned() };
    if height <= 0 {
        return;
    }
    ACTIVE.store(true, Ordering::Relaxed);

    let at = layout(height, !crate::spectator_bars::hiding());
    crate::objicons::set_auto(Some(at.icon_y), Some(at.timer_y));
    let minimap_up = crate::overview_map::mode() == Some(2);
    crate::deathmsg::set_auto_offset((!minimap_up).then_some(at.feed_y));
    if let Err(why) = crate::overview_map::shift_mini(at.mini_shift)
        && !SHIFT_FAILED.swap(true, Ordering::Relaxed)
    {
        unsafe {
            crate::debug::report(&format!(
                "spectator_hud: could not move the minimap -- {why}"
            ))
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_the_bar_shown_everything_starts_below_it() {
        // 1280x716, the window the live tests ran in.
        let at = layout(716, true);
        let bar = bar_height(716);
        assert_eq!(bar, 95);
        assert!(at.icon_y > bar && at.timer_y > bar && at.feed_y > bar);
        // The minimap's cached y is H/240 (2 here) plus the game's push of 81:
        // after the shift it sits as far below the bar as it does from the top.
        assert_eq!(
            716 / 240 + game_scaled(GAME_PUSH_UNITS, 716) + at.mini_shift,
            bar + 2
        );
    }

    #[test]
    fn with_the_bar_hidden_everything_sits_where_a_pov_demo_has_it() {
        let at = layout(1080, false);
        assert_eq!(at.icon_y, 5);
        assert_eq!(at.timer_y, TIMER_Y);
        assert_eq!(at.feed_y, FEED_Y);
        // Cancels the game's push exactly.
        assert_eq!(at.mini_shift, -game_scaled(GAME_PUSH_UNITS, 1080));
    }

    #[test]
    fn the_scaling_matches_the_values_the_game_was_seen_to_use() {
        // 1080p: the objectives at 122 while spectating, 5 in a POV demo.
        assert_eq!(game_scaled(GAME_PUSH_UNITS, 1080), 122);
        assert_eq!(game_scaled(ICON_UNITS, 1080), 5);
        assert_eq!(bar_height(1080), 144);
    }
}
