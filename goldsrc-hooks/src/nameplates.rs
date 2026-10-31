//! `ds_hud_show_player_names` and `ds_hud_show_player_health` (#1130): a name
//! above each player the game draws this frame, and a health bar, while a
//! demo plays. Two settings, both off by default: someone may want the bars
//! without the names, or the names alone.
//!
//! ## What is drawn
//!
//! - **Who:** the players `client.dll`'s `HUD_AddEntity` let through this
//!   frame ([`note_added`], from `engine.rs`'s detour, after the real one
//!   answered): every player the engine is about to render, so nobody out of
//!   the recorder's view, nobody the hook's own `ds_map_hide_asset` hid, and
//!   not the player an HLTV camera watches in-eye (the client returns 0 for
//!   him). The recording player of a POV demo is left out too, and anyone
//!   `g_PlayerExtraInfo` holds as dead or not on Allies or Axis.
//! - **Where:** 12 units over the top of the player's box
//!   (`curstate.maxs[2]`, which follows crouch and prone), at the place the
//!   engine draws him this frame (`cl_entity_t::origin`), through TriAPI's
//!   `WorldToScreen`, the engine's own projection, so a FOV change
//!   (`default_fov`, HLAE's `mirv_fov`) moves the plates with the view.
//! - **The name:** the one the scoreboard shows, so `ds_player_change_name`
//!   applies, in the engine's console font, in a light tint of the kill
//!   feed's team colour over a translucent box.
//! - **The health:** `g_PlayerExtraInfo[i].health` (`player_info.rs` says
//!   who fills it: the entity state in every demo, the `HLTV` message too
//!   in HLTV demos), as a bar from green to red; nothing outside 1..=100.
//!
//! Names show through walls: "drawn this frame" means in the recorder's
//! potentially visible set, not in his line of sight. A trace per player
//! would hide them; that is a look for the user to choose (#1130).
//!
//! Only in demo playback: in a live game the plates would show enemies
//! through walls.
//!
//! ## Cost
//!
//! Off: one atomic load per entity in `HUD_AddEntity` and two cvar reads in
//! `HUD_Redraw`. On: per frame, one `WorldToScreen`, one text measure, one or
//! two boxes and one string per player drawn, and no allocation. Everything
//! runs on the engine's thread; the only shared state is atomics.

#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::names::console_name;
use crate::setting::Cvar;

pub const NAMES_NAME: &str = console_name!("hud_show_player_names");
pub const HEALTH_NAME: &str = console_name!("hud_show_player_health");

pub static NAMES_CVAR: Cvar = Cvar::new();
pub static HEALTH_CVAR: Cvar = Cvar::new();

/// Whether either setting was on at the last `HUD_Redraw`, so `note_added`
/// costs one load while both are off.
static ENABLED: AtomicBool = AtomicBool::new(false);
/// Bit `i - 1` set: player `i` was let through `HUD_AddEntity` since the
/// last `HUD_Redraw`.
static ADDED: AtomicU32 = AtomicU32::new(0);

/// Units above the top of a player's box the plate sits at.
const ABOVE_HEAD: f32 = 12.0;
/// A standing player's `maxs[2]`, for a state that doesn't carry one.
const STANDING_TOP: f32 = 36.0;

type Rgb = (i32, i32, i32);

/// Lighter tints of DoD's kill-feed colours (`deathmsg::backdrop`'s
/// `ALLIES_NAME` (76, 102, 76) and `AXIS_NAME` (255, 63, 63)): the game's
/// Allies green is too dark to read over a map.
const ALLIES_TEXT: (f32, f32, f32) = (0.6, 0.85, 0.6);
const AXIS_TEXT: (f32, f32, f32) = (1.0, 0.5, 0.5);
const BOX: Rgb = (0, 0, 0);
const BOX_ALPHA: i32 = 140;
const BAR_ALPHA: i32 = 220;
/// Around the name, and between the name and the bar.
const PADDING: i32 = 2;

/// Called from `engine.rs`'s `HUD_AddEntity` detour for each entity the
/// client let through. Notes the players among them.
///
/// # Safety
///
/// `entity` is the engine's `cl_entity_t` for this call, or null.
pub(crate) unsafe fn note_added(entity: *const crate::engine::ClEntityS) {
    if !ENABLED.load(Ordering::Relaxed) || entity.is_null() {
        return;
    }
    // Safety: the caller's.
    let (player, index) = unsafe { ((*entity).player, (*entity).index) };
    if player != 0
        && let Some(bit) = player_bit(index)
    {
        ADDED.fetch_or(bit, Ordering::Relaxed);
    }
}

/// The [`ADDED`] bit of entity `index`, for the 32 player slots.
fn player_bit(index: i32) -> Option<u32> {
    (1..=32).contains(&index).then(|| 1u32 << (index - 1))
}

/// The pixel a `WorldToScreen` result (-1..1 each way, y up) lands on, if it
/// is on the screen.
fn to_pixels(ndc: [f32; 2], width: i32, height: i32) -> Option<(i32, i32)> {
    let [x, y] = ndc;
    if !(-1.0..=1.0).contains(&x) || !(-1.0..=1.0).contains(&y) {
        return None;
    }
    let px = (1.0 + x) * 0.5 * width as f32;
    let py = (1.0 - y) * 0.5 * height as f32;
    Some((px as i32, py as i32))
}

/// The height the plate sits at over the player's origin.
fn plate_height(maxs_z: f32) -> f32 {
    let top = if maxs_z.is_finite() && maxs_z > 0.0 {
        maxs_z
    } else {
        STANDING_TOP
    };
    top + ABOVE_HEAD
}

/// The bar's fill, 0..=1, for a health value; `None` for anything outside
/// 1..=100 (dead, or never heard).
fn health_fraction(health: i32) -> Option<f32> {
    (1..=100).contains(&health).then(|| health as f32 / 100.0)
}

/// Green at full health through yellow to red.
fn bar_colour(fraction: f32) -> Rgb {
    let f = fraction.clamp(0.0, 1.0);
    let red = if f > 0.5 {
        (2.0 - 2.0 * f) * 255.0
    } else {
        255.0
    };
    let green = if f < 0.5 { 2.0 * f * 255.0 } else { 255.0 };
    (red as i32, green as i32, 0)
}

/// The bar's size for a screen `height` px tall: 49x5 at 1080.
fn bar_size(height: i32) -> (i32, i32) {
    ((height / 22).max(24), (height / 216).max(3))
}

/// Where a plate's parts go, as `[x0, y0, x1, y1]` boxes.
#[derive(Debug, PartialEq, Eq)]
struct Layout {
    /// The name's top-left, and its box.
    name: Option<((i32, i32), [i32; 4])>,
    bar: Option<[i32; 4]>,
}

/// The plate over a head at `anchor`: the bar's bottom edge on it, the name
/// over the bar, both centred. `name` is the text's size, `bar` the bar's.
fn layout(anchor: (i32, i32), name: Option<(i32, i32)>, bar: Option<(i32, i32)>) -> Layout {
    let (cx, mut bottom) = anchor;
    let bar_box = bar.map(|(w, h)| {
        let r = [cx - w / 2, bottom - h, cx - w / 2 + w, bottom];
        bottom -= h + PADDING;
        r
    });
    let name_box = name.map(|(w, h)| {
        let left = cx - w / 2;
        let top = bottom - h;
        (
            (left, top),
            [
                left - PADDING,
                top - PADDING,
                left + w + PADDING,
                bottom + PADDING,
            ],
        )
    });
    Layout {
        name: name_box,
        bar: bar_box,
    }
}

/// Draws the plates. Runs after `client.dll`'s own `HUD_Redraw`, every frame.
///
/// # Safety
///
/// On the engine's thread, inside `HUD_Redraw`.
pub(crate) unsafe fn draw(intermission: i32) {
    let names = NAMES_CVAR.on();
    let health = HEALTH_CVAR.on();
    ENABLED.store(names || health, Ordering::Relaxed);
    let added = ADDED.swap(0, Ordering::Relaxed);
    let wanted = (names || health) && added != 0 && intermission == 0;
    #[cfg(target_arch = "x86")]
    if wanted {
        // Safety: the caller's.
        unsafe { live::draw(added, Show { names, health }) };
    }
    #[cfg(not(target_arch = "x86"))]
    let _ = wanted;
}

/// Which parts of the plate are on.
#[derive(Clone, Copy)]
struct Show {
    names: bool,
    health: bool,
}

#[cfg(target_arch = "x86")]
mod live {
    use std::ffi::CStr;

    use super::*;
    use crate::engine::{self, ClEntityS, HudPlayerInfo, ScreenInfo};
    use crate::player_info;

    fn is_playing_demo(e: &engine::ClEngineFuncsPartial) -> bool {
        let api = e.p_demo_api;
        // Safety: the engine's demo API, valid for the session.
        !api.is_null() && unsafe { ((*api).is_playingback)() } != 0
    }

    /// The name the scoreboard shows for `index`: a `ds_player_change_name`
    /// rename, else the engine's.
    fn shown_name(e: &engine::ClEngineFuncsPartial, index: i32) -> Option<&'static CStr> {
        let mut info = HudPlayerInfo::empty();
        // Safety: the engine's own function, on its thread, into a struct of
        // the layout it writes.
        unsafe { (e.pfn_get_player_info)(index, &mut info) };
        if info.name.is_null() {
            return None;
        }
        // Safety: a pointer into the engine's static player array.
        let real = unsafe { CStr::from_ptr(info.name) };
        if real.is_empty() {
            return None;
        }
        Some(crate::player_names::new_name_for(index, real.to_bytes()).unwrap_or(real))
    }

    /// # Safety
    ///
    /// On the engine's thread, inside `HUD_Redraw`.
    pub(super) unsafe fn draw(added: u32, show: Show) {
        let (Some(e), Some(base)) = (engine::engfuncs(), engine::client_module_base()) else {
            return;
        };
        if !is_playing_demo(e) || e.p_tri_api.is_null() {
            return;
        }
        // Safety: the engine's TriAPI table, valid for the session.
        let tri = unsafe { &*e.p_tri_api };
        let mut screen = ScreenInfo {
            size: size_of::<ScreenInfo>() as i32,
            width: 0,
            height: 0,
            flags: 0,
            char_height: 0,
            char_widths: [0; 256],
        };
        // Safety: the engine's own call, into a struct whose size it checks.
        unsafe { (e.pfn_get_screen_info)(&mut screen) };
        if screen.width <= 0 || screen.height <= 0 {
            return;
        }
        // Safety: the engine's own call; null before a map is up.
        let local = unsafe { (e.get_local_player)() } as *const ClEntityS;
        // Safety: a non-null `cl_entity_t` the engine owns.
        let own = (!local.is_null()).then(|| unsafe { (*local).index });

        for index in 1..=32 {
            if added & (1 << (index - 1)) == 0 || Some(index) == own {
                continue;
            }
            let team = player_info::team(base, index).unwrap_or(0);
            if !matches!(team, player_info::ALLIES | player_info::AXIS)
                || player_info::is_dead(base, index).unwrap_or(true)
            {
                continue;
            }
            // Safety: the engine's accessor, null for an empty slot.
            let entity = unsafe { (e.get_entity_by_index)(index) } as *const ClEntityS;
            if entity.is_null() {
                continue;
            }
            // Safety: a non-null `cl_entity_t`, for this frame, on this thread.
            let (origin, top) = unsafe { ((*entity).origin, (*entity).curstate.maxs.z) };
            let world = [origin.x, origin.y, origin.z + plate_height(top)];
            let mut ndc = [0f32; 3];
            // Safety: three floats in, three out; 1 means behind the eye.
            if unsafe { (tri.world_to_screen)(world.as_ptr(), ndc.as_mut_ptr()) } != 0 {
                continue;
            }
            let Some(anchor) = to_pixels([ndc[0], ndc[1]], screen.width, screen.height) else {
                continue;
            };
            let name = if show.names {
                shown_name(e, index)
            } else {
                None
            };
            let fraction = if show.health {
                health_fraction(player_info::health(base, index).unwrap_or(0))
            } else {
                None
            };
            // Safety: the caller's.
            unsafe { plate(e, &screen, anchor, team, name, fraction) };
        }
    }

    /// One player's plate.
    ///
    /// # Safety
    ///
    /// On the engine's thread, inside `HUD_Redraw`.
    unsafe fn plate(
        e: &engine::ClEngineFuncsPartial,
        screen: &ScreenInfo,
        anchor: (i32, i32),
        team: i32,
        name: Option<&CStr>,
        fraction: Option<f32>,
    ) {
        let text_size = name.map(|n| {
            let (mut w, mut h) = (0, 0);
            // Safety: the engine's own measure, of a NUL-terminated string.
            unsafe { (e.pfn_draw_console_string_len)(n.as_ptr(), &mut w, &mut h) };
            (w, h)
        });
        let bar = fraction.map(|_| bar_size(screen.height));
        let at = layout(anchor, text_size, bar);
        // Safety: inside HUD_Redraw, with the engine's GL context current.
        unsafe {
            if let (Some(((x, y), rect)), Some(n)) = (at.name, name) {
                crate::deathmsg::fill_box(rect, BOX, BOX_ALPHA);
                let (r, g, b) = if team == player_info::AXIS {
                    AXIS_TEXT
                } else {
                    ALLIES_TEXT
                };
                (e.pfn_draw_set_text_color)(r, g, b);
                (e.pfn_draw_console_string)(x, y, n.as_ptr());
            }
            if let (Some(rect), Some(f)) = (at.bar, fraction) {
                let [x0, y0, x1, y1] = rect;
                crate::deathmsg::fill_box([x0 - 1, y0 - 1, x1 + 1, y1 + 1], BOX, BAR_ALPHA);
                let filled = x0 + ((x1 - x0) as f32 * f).round() as i32;
                crate::deathmsg::fill_box([x0, y0, filled, y1], bar_colour(f), BAR_ALPHA);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_32_player_slots_have_a_bit() {
        assert_eq!(player_bit(0), None);
        assert_eq!(player_bit(1), Some(1));
        assert_eq!(player_bit(32), Some(1 << 31));
        assert_eq!(player_bit(33), None);
        assert_eq!(player_bit(-5), None);
    }

    #[test]
    fn a_point_off_the_screen_has_no_pixel() {
        assert_eq!(to_pixels([1.2, 0.0], 1920, 1080), None);
        assert_eq!(to_pixels([0.0, -1.01], 1920, 1080), None);
        assert_eq!(to_pixels([f32::NAN, 0.0], 1920, 1080), None);
    }

    #[test]
    fn the_screen_centre_and_top_left_land_where_expected() {
        assert_eq!(to_pixels([0.0, 0.0], 1920, 1080), Some((960, 540)));
        // y is up in WorldToScreen's result, down on the screen.
        assert_eq!(to_pixels([-1.0, 1.0], 1920, 1080), Some((0, 0)));
    }

    #[test]
    fn the_plate_follows_the_box_top_and_falls_back_to_standing() {
        assert_eq!(plate_height(18.0), 30.0);
        assert_eq!(plate_height(0.0), 48.0);
        assert_eq!(plate_height(f32::NAN), 48.0);
    }

    #[test]
    fn health_outside_1_to_100_draws_no_bar() {
        assert_eq!(health_fraction(0), None);
        assert_eq!(health_fraction(-3), None);
        assert_eq!(health_fraction(101), None);
        assert_eq!(health_fraction(100), Some(1.0));
        assert_eq!(health_fraction(25), Some(0.25));
    }

    #[test]
    fn the_bar_goes_green_to_yellow_to_red() {
        assert_eq!(bar_colour(1.0), (0, 255, 0));
        assert_eq!(bar_colour(0.5), (255, 255, 0));
        assert_eq!(bar_colour(0.0), (255, 0, 0));
    }

    #[test]
    fn the_bar_is_49_by_5_at_1080_and_never_tiny() {
        assert_eq!(bar_size(1080), (49, 5));
        assert_eq!(bar_size(240), (24, 3));
    }

    #[test]
    fn the_name_sits_over_the_bar_both_centred_on_the_head() {
        let at = layout((100, 200), Some((40, 12)), Some((48, 5)));
        assert_eq!(at.bar, Some([76, 195, 124, 200]));
        // The bar is 5 high plus the padding: the name's bottom at 193.
        assert_eq!(at.name, Some(((80, 181), [78, 179, 122, 195])));
    }

    #[test]
    fn a_name_alone_sits_on_the_head() {
        let at = layout((100, 200), Some((40, 12)), None);
        assert_eq!(at.bar, None);
        assert_eq!(at.name, Some(((80, 188), [78, 186, 122, 202])));
    }
}
