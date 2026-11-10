//! `ds_hud_crosshair_scale`: the crosshair drawn S times bigger (#1335), for
//! movies captured at high resolutions, and drawn pixel for pixel at any scale.
//!
//! The game draws a crosshair from a small rect of a sprite sheet: a 64x64
//! tile, 4% of the height of a 2560x1440 frame. Nothing in the engine scales a
//! HUD sprite, so each of the crosshair's draws goes through `gl_zoom` about
//! the screen's centre, the way `deathmsg::scale` does the kill feed. Both
//! crosshair paths centre on that point, so the crosshair stays on it at any
//! scale, and the red crosshair's five pieces keep their layout, the gap
//! included.
//!
//! ## Pixel for pixel
//!
//! The engine's sprite draw does not draw a rect at its own pixels: it maps
//! `w - 1` texels of a `w`-px rect over `w` px with linear filtering
//! (measured on both builds, `docs/goldsrc_crosshair.md` §4). Pixel `px` of a
//! draw at `x` samples texel coordinate `(px + 0.5 - x) * (w - 1) / w`, so the
//! art comes out a pixel bigger than it is, about every second pixel is a
//! half-and-half blend, and a 64x64 tile's middle sits half a pixel down and
//! right of where the game centred it. So every crosshair draw, at the game's
//! size too, is zoomed by [`fit`] about its own top-left first: texel `j` then
//! lands on the middle of pixel `x + j`, and the art draws at exactly its own
//! pixels. At any other scale the same fit is folded into the scale's zoom.
//!
//! One number for every crosshair on screen: the Players table's, the game's
//! `customXHair.spr` tile, the spectator's stock `crosshairs.spr` rect and the
//! red `dodcross.spr` one. It changes the size of a draw and nothing about
//! which draw happens, so `ds_hud_hide_crosshair` and the spectator gate work
//! unchanged.
//!
//! The draw path reads and writes atomics only; the scale and the screen's
//! centre are read once a frame by [`poll`].

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
/// The last crosshair draw's rect size, for the status line.
static LAST_WIDTH: AtomicI32 = AtomicI32::new(0);
static LAST_HEIGHT: AtomicI32 = AtomicI32::new(0);
/// Crosshair draws zoomed this session, for the status line.
static ZOOMED: AtomicU32 = AtomicU32::new(0);

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

/// One crosshair draw's zoom: the draw's top-left `from` moved to `to`, and
/// everything about it `scale` times bigger across and down, in the frame's
/// pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zoom {
    from: (f32, f32),
    to: (f32, f32),
    scale: (f32, f32),
}

impl Zoom {
    /// The same zoom with everything moved a further `(dx, dy)` of the
    /// frame's pixels: an outline copy (`crosshair_outline`), which stays on
    /// whole pixels because the offset is applied after the zoom.
    pub fn shifted(self, dx: i32, dy: i32) -> Zoom {
        Zoom {
            to: (self.to.0 + dx as f32, self.to.1 + dy as f32),
            ..self
        }
    }

    /// Runs `draw` under this zoom. Returns `false`, without calling `draw`,
    /// when the GL calls cannot be had; the caller then draws it as it is.
    ///
    /// # Safety
    ///
    /// Only from a HUD draw, on the engine's thread with its GL context
    /// current.
    pub unsafe fn run(self, draw: impl FnOnce()) -> bool {
        // Safety: the caller's.
        unsafe { crate::gl_zoom::mapped_xy(self.from, self.to, self.scale, draw) }
    }
}

/// The zoom that undoes the engine's stretch of a rect `size` px across: it
/// maps `size - 1` texels over `size` px, so `(size - 1) / size` puts them
/// back on `size - 1` px, texel `j` on pixel `j`. 1 for a rect too small to
/// have a stretch.
fn fit(size: i32) -> f32 {
    if size >= 2 {
        (size - 1) as f32 / size as f32
    } else {
        1.0
    }
}

/// The zoom for a crosshair draw of `rect` at `(x, y)`: [`fit`] about the
/// draw's own top-left, then, at a `scale` other than 1, that scale about the
/// screen's centre corner `centre`. Without a centre it draws at its own size.
///
/// The fit puts a tile's middle (texel 32, the corner the creator draws the
/// crosshair about) exactly on the centre corner the game centred the tile
/// on, so zooming about that same corner keeps it there at any scale, and the
/// crosshair mirrors about a pixel corner both ways (#1348).
fn zoom_for(x: i32, y: i32, rect: &Wrect, scale: f32, centre: Option<(i32, i32)>) -> Zoom {
    let from = (x as f32, y as f32);
    let fit = (fit(rect.right - rect.left), fit(rect.bottom - rect.top));
    match centre {
        Some((cx, cy)) if scale != 1.0 => {
            let about = |c: i32, p: f32| c as f32 + scale * (p - c as f32);
            Zoom {
                from,
                to: (about(cx, from.0), about(cy, from.1)),
                scale: (scale * fit.0, scale * fit.1),
            }
        }
        _ => Zoom {
            from,
            to: from,
            scale: fit,
        },
    }
}

/// From a draw hook: the zoom for the draw about to happen, or `None` when it
/// is not the crosshair's. `prc` is the game's rect, which says whose draw it
/// is; `drawn` at `(x, y)` is what is drawn, the Players table's tile when
/// `player_crosshair` swaps it. Atomics only.
///
/// # Safety
///
/// `prc` is the rect pointer the game passed to `SPR_Draw*`, null or valid.
pub unsafe fn zoom(sheet: Sheet, prc: *const Wrect, x: i32, y: i32, drawn: &Wrect) -> Option<Zoom> {
    if !unsafe { is_crosshair(sheet, prc) } {
        return None;
    }
    let scale = f32::from_bits(SCALE.load(Ordering::Relaxed));
    let centre = (
        CENTRE_X.load(Ordering::Relaxed),
        CENTRE_Y.load(Ordering::Relaxed),
    );
    LAST_WIDTH.store(drawn.right - drawn.left, Ordering::Relaxed);
    LAST_HEIGHT.store(drawn.bottom - drawn.top, Ordering::Relaxed);
    ZOOMED.fetch_add(1, Ordering::Relaxed);
    let centre = (centre.0 > 0 && centre.1 > 0).then_some(centre);
    Some(zoom_for(x, y, drawn, scale, centre))
}

/// The `ds_debug_status` line, once a crosshair has drawn: its scale and the
/// fit that draws it pixel for pixel.
pub fn status_line() -> Option<String> {
    let scale = f32::from_bits(SCALE.load(Ordering::Relaxed));
    let draws = ZOOMED.load(Ordering::Relaxed);
    let (w, h) = (
        LAST_WIDTH.load(Ordering::Relaxed),
        LAST_HEIGHT.load(Ordering::Relaxed),
    );
    let fit = format!(
        "its {w}x{h} rect fitted by {}/{w} x {}/{h} so each texel is one pixel ({draws} draw(s) this session)",
        (w - 1).max(1),
        (h - 1).max(1)
    );
    if scale != 1.0 {
        Some(format!(
            "{NAME} {scale}: the crosshair draws {scale} times its size about the screen's centre, {fit}"
        ))
    } else {
        (draws > 0).then(|| format!("{NAME} 1: the crosshair draws pixel for pixel, {fit}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TILE: Wrect = Wrect {
        left: 0,
        right: 64,
        top: 0,
        bottom: 64,
    };
    const SPECTATOR: Wrect = Wrect {
        left: 24,
        right: 48,
        top: 0,
        bottom: 24,
    };

    /// Where texel coordinate `u` of a rect `size` px across, drawn by the
    /// engine at `x`, lands on screen under `zoom`'s x axis. The engine puts
    /// it at `x + u * size / (size - 1)` (the measured stretch).
    fn on_screen_x(zoom: &Zoom, x: i32, size: i32, u: f32) -> f32 {
        let engine = x as f32 + u * size as f32 / (size - 1) as f32;
        zoom.to.0 + zoom.scale.0 * (engine - zoom.from.0)
    }

    fn on_screen_y(zoom: &Zoom, y: i32, size: i32, u: f32) -> f32 {
        let engine = y as f32 + u * size as f32 / (size - 1) as f32;
        zoom.to.1 + zoom.scale.1 * (engine - zoom.from.1)
    }

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
        assert!(is_scaled_draw(Sheet::Custom, &TILE, false));
        assert!(is_scaled_draw(Sheet::Red, &TILE, false));
        assert!(!is_scaled_draw(Sheet::Other, &TILE, true));
        // crosshairs.spr holds other things: only the spectator's rect, in-eye.
        assert!(!is_scaled_draw(Sheet::Stock, &TILE, true));
    }

    #[test]
    fn at_the_games_size_every_texel_lands_on_the_middle_of_its_own_pixel() {
        for (size, x) in [(64, 1248), (24, 1268), (9, 1270), (2, 5)] {
            let rect = Wrect {
                left: 0,
                right: size,
                top: 0,
                bottom: size,
            };
            let zoom = zoom_for(x, x, &rect, 1.0, Some((1280, 720)));
            for j in 0..size - 1 {
                let middle = j as f32 + 0.5;
                let want = (x + j) as f32 + 0.5;
                assert!(
                    (on_screen_x(&zoom, x, size, middle) - want).abs() < 1e-3,
                    "{size}: x {j}"
                );
                assert!(
                    (on_screen_y(&zoom, x, size, middle) - want).abs() < 1e-3,
                    "{size}: y {j}"
                );
            }
        }
    }

    #[test]
    fn a_red_crosshair_piece_is_fitted_across_and_down_apart() {
        let horiz = Wrect {
            left: 0,
            right: 12,
            top: 0,
            bottom: 3,
        };
        let zoom = zoom_for(1286, 719, &horiz, 1.0, None);
        assert_eq!(zoom.scale, (11.0 / 12.0, 2.0 / 3.0));
        assert!((on_screen_x(&zoom, 1286, 12, 10.5) - 1296.5).abs() < 1e-3);
        assert!((on_screen_y(&zoom, 719, 3, 1.5) - 720.5).abs() < 1e-3);
    }

    #[test]
    fn a_rect_too_small_to_stretch_is_left_its_size() {
        assert_eq!(fit(1), 1.0);
        assert_eq!(fit(0), 1.0);
        assert_eq!(fit(64), 63.0 / 64.0);
    }

    #[test]
    fn the_tiles_middle_lands_on_the_centre_corner_at_any_scale() {
        for s in [0.5f32, 0.7, 1.0, 1.25, 1.5, 2.0, 3.0, 8.0] {
            // The game's 64x64 tile, and the spectator's 24x24 rect, both
            // centred by the game on the corner (1280, 720).
            for (rect, half) in [(TILE, 32), (SPECTATOR, 12)] {
                let (x, y) = (1280 - half, 720 - half);
                let zoom = zoom_for(x, y, &rect, s, Some((1280, 720)));
                let size = 2 * half;
                let mid = half as f32;
                assert!(
                    (on_screen_x(&zoom, x, size, mid) - 1280.0).abs() < 1e-3,
                    "x {size} at {s}"
                );
                assert!(
                    (on_screen_y(&zoom, y, size, mid) - 720.0).abs() < 1e-3,
                    "y {size} at {s}"
                );
            }
        }
    }

    #[test]
    fn at_any_scale_each_texel_is_that_many_pixels_and_mirrors_about_the_centre() {
        for s in [0.7f32, 1.25, 2.0] {
            let zoom = zoom_for(1248, 688, &TILE, s, Some((1280, 720)));
            for u in [0.0f32, 10.0, 21.5, 31.0] {
                let step = on_screen_x(&zoom, 1248, 64, u + 1.0) - on_screen_x(&zoom, 1248, 64, u);
                assert!((step - s).abs() < 1e-3, "{u} at {s}");
                // The art's left and right edges mirror about the corner, so
                // a symmetric crosshair is sampled alike on both sides.
                let left = on_screen_x(&zoom, 1248, 64, u);
                let right = on_screen_x(&zoom, 1248, 64, 64.0 - u);
                assert!((left + right - 2560.0).abs() < 1e-3, "{u} at {s}");
            }
        }
    }

    #[test]
    fn an_outline_copy_moves_by_whole_pixels_after_the_zoom() {
        let zoom = zoom_for(1248, 688, &TILE, 0.7, Some((1280, 720)));
        let copy = zoom.shifted(-2, 1);
        for u in [0.0f32, 13.5, 40.0] {
            let d = on_screen_x(&copy, 1248, 64, u) - on_screen_x(&zoom, 1248, 64, u);
            assert!((d + 2.0).abs() < 1e-4);
            let d = on_screen_y(&copy, 688, 64, u) - on_screen_y(&zoom, 688, 64, u);
            assert!((d - 1.0).abs() < 1e-4);
        }
    }

    #[test]
    fn without_a_screen_centre_a_scaled_crosshair_draws_at_its_own_size() {
        let zoom = zoom_for(1248, 688, &TILE, 2.0, None);
        assert_eq!(zoom, zoom_for(1248, 688, &TILE, 1.0, Some((1280, 720))));
    }

    #[test]
    fn a_draw_that_is_not_the_crosshairs_is_never_zoomed() {
        assert_eq!(unsafe { zoom(Sheet::Other, &TILE, 0, 0, &TILE) }, None);
        assert_eq!(
            unsafe { zoom(Sheet::Custom, std::ptr::null(), 0, 0, &TILE) },
            None
        );
    }
}
