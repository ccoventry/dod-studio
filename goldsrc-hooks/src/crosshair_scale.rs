//! `ds_hud_crosshair_scale`: the crosshair drawn S times bigger (#1335), for
//! movies captured at high resolutions.
//!
//! The game draws a crosshair at its exact pixels from a 64x64 tile, 4% of the
//! height of a 2560x1440 frame. Nothing in the engine scales a HUD sprite, so
//! each of the crosshair's draws goes through `gl_zoom` about the screen's
//! centre, the way `deathmsg::scale` does the kill feed. Both crosshair paths
//! centre on that point, so the crosshair stays on it at any scale, and the
//! red crosshair's five pieces keep their layout, the gap included.
//!
//! One number for every crosshair on screen: the Players table's, the game's
//! `customXHair.spr` tile, the spectator's stock `crosshairs.spr` rect and the
//! red `dodcross.spr` one. It changes the size of a draw and nothing about
//! which draw happens, so `ds_hud_hide_crosshair` and the spectator gate work
//! unchanged.
//!
//! The draw path reads atomics only; the scale and the screen's centre are
//! read once a frame by [`poll`].

use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use crate::hud_sprites::Wrect;
use crate::names::console_name;
use crate::player_crosshair::{Sheet, is_crosshair_draw};

/// The cvar's name. Registered in `commands/install.rs`.
pub const NAME: &str = console_name!("hud_crosshair_scale");

/// The largest scale honoured: 8 turns a whole tile into 512 px, a third of a
/// 1440p frame, which is past any crosshair worth drawing.
const MAX_SCALE: f32 = 8.0;

/// The scale's bits, `1.0` for the game's own size.
static SCALE: AtomicU32 = AtomicU32::new(1.0f32.to_bits());
/// The screen's centre, in the HUD's pixels.
static CENTRE_X: AtomicI32 = AtomicI32::new(0);
static CENTRE_Y: AtomicI32 = AtomicI32::new(0);

/// The scale a cvar's value asks for: anything that isn't a positive number
/// is the game's own size, and anything above [`MAX_SCALE`] is that.
fn wanted(raw: f32) -> f32 {
    if raw.is_finite() && raw > 0.0 {
        raw.min(MAX_SCALE)
    } else {
        1.0
    }
}

/// Once a frame from `commands::poll`, with the cvar's value.
pub fn poll(raw: Option<f32>) {
    let Some(raw) = raw else {
        return;
    };
    let scale = wanted(raw);
    if SCALE.swap(scale.to_bits(), Ordering::Relaxed) != scale.to_bits() {
        unsafe { crate::debug::report(&format!("commands: {NAME} = {scale}")) };
    }
    if scale != 1.0 {
        refresh_centre();
    }
}

/// Reads the screen's size from the engine.
fn refresh_centre() {
    let Some(e) = crate::engine::engfuncs() else {
        return;
    };
    let mut screen = crate::engine::ScreenInfo {
        size: size_of::<crate::engine::ScreenInfo>() as i32,
        width: 0,
        height: 0,
        flags: 0,
        char_height: 0,
        char_widths: [0; 256],
    };
    // Safety: the engine's own call, into a struct whose size it checks.
    unsafe { (e.pfn_get_screen_info)(&mut screen) };
    if screen.width > 0 && screen.height > 0 {
        CENTRE_X.store(screen.width / 2, Ordering::Relaxed);
        CENTRE_Y.store(screen.height / 2, Ordering::Relaxed);
    }
}

/// Whether a draw of `sheet` with `rect` is part of the crosshair: what
/// `player_crosshair` swaps, or a piece of the red one.
fn is_scaled_draw(sheet: Sheet, rect: &Wrect, in_eye: bool) -> bool {
    sheet == Sheet::Red || is_crosshair_draw(sheet, rect, in_eye)
}

/// Whether the draw about to happen is part of the crosshair. Atomics only.
///
/// # Safety
///
/// `prc` is the rect pointer the game passed to `SPR_Draw*`, null or valid.
pub unsafe fn is_crosshair(sheet: Sheet, prc: *const Wrect) -> bool {
    sheet != Sheet::Other
        && !prc.is_null()
        && is_scaled_draw(
            sheet,
            unsafe { &*prc },
            crate::spectator_target::mode() == Some(4),
        )
}

/// From a draw hook: the point and scale to zoom this draw by, or `None` to
/// draw it as it is. Atomics only.
///
/// # Safety
///
/// `prc` is the rect pointer the game passed to `SPR_Draw*`, null or valid.
pub unsafe fn zoom(sheet: Sheet, prc: *const Wrect) -> Option<(f32, f32, f32)> {
    let scale = f32::from_bits(SCALE.load(Ordering::Relaxed));
    if scale == 1.0 || !unsafe { is_crosshair(sheet, prc) } {
        return None;
    }
    let (x, y) = (
        CENTRE_X.load(Ordering::Relaxed),
        CENTRE_Y.load(Ordering::Relaxed),
    );
    (x > 0 && y > 0).then_some((x as f32, y as f32, scale))
}

/// The `ds_debug_status` line, while the crosshair is scaled.
pub fn status_line() -> Option<String> {
    let scale = f32::from_bits(SCALE.load(Ordering::Relaxed));
    (scale != 1.0).then(|| format!("{NAME} {scale}: the crosshair draws {scale} times its size"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANY_RECT: Wrect = Wrect {
        left: 0,
        right: 64,
        top: 0,
        bottom: 64,
    };

    #[test]
    fn a_bad_value_is_the_games_own_size_and_a_big_one_is_capped() {
        assert_eq!(wanted(2.0), 2.0);
        assert_eq!(wanted(0.5), 0.5);
        assert_eq!(wanted(0.0), 1.0);
        assert_eq!(wanted(-2.0), 1.0);
        assert_eq!(wanted(f32::NAN), 1.0);
        assert_eq!(wanted(f32::INFINITY), 1.0);
        assert_eq!(wanted(100.0), MAX_SCALE);
    }

    #[test]
    fn only_the_crosshairs_own_draws_are_scaled() {
        assert!(is_scaled_draw(Sheet::Custom, &ANY_RECT, false));
        assert!(is_scaled_draw(Sheet::Red, &ANY_RECT, false));
        assert!(!is_scaled_draw(Sheet::Other, &ANY_RECT, true));
        // crosshairs.spr holds other things: only the spectator's rect, in-eye.
        assert!(!is_scaled_draw(Sheet::Stock, &ANY_RECT, true));
    }

    #[test]
    fn nothing_is_zoomed_at_the_games_own_size() {
        SCALE.store(1.0f32.to_bits(), Ordering::Relaxed);
        assert_eq!(unsafe { zoom(Sheet::Custom, &ANY_RECT) }, None);
    }
}
