//! The flag icons the game draws on its map (#371), for the Overviews page
//! to preview over an overview: where a flag's icon will cover it.
//!
//! The game draws them from `CObjectiveIcons::CalcIconLocations`
//! (dod13-client `dod_objectives.cpp`): each control point's HUD icon,
//! at the sprite's own size in screen pixels, centred on the point. So
//! unlike the player icons (a fixed size in map units) they cover more of
//! the map the smaller the screen. Which icon is the point's
//! `point_hud_icon_neutral` / `_allies` / `_axis` key, by its
//! `point_default_owner`, an index into the table below (`szCPIcons` in
//! `client.dll`, 64-byte entries from `+0xac468`; `%s` is the map name).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::level::Level;

/// `szCPIcons`, in order.
const ICONS: [&str; 37] = [
    "sprites/obj_icons/icon_obj_neutral.spr",
    "sprites/obj_icons/icon_obj_allies.spr",
    "sprites/obj_icons/icon_obj_axis.spr",
    "sprites/obj_icons/icon_obj_88_neutral.spr",
    "sprites/obj_icons/icon_obj_88_allies.spr",
    "sprites/obj_icons/icon_obj_88_axis.spr",
    "sprites/obj_icons/icon_obj_bridge_neutral.spr",
    "sprites/obj_icons/icon_obj_bridge_allies.spr",
    "sprites/obj_icons/icon_obj_bridge_axis.spr",
    "sprites/obj_icons/icon_obj_radio_neutral.spr",
    "sprites/obj_icons/icon_obj_radio_allies.spr",
    "sprites/obj_icons/icon_obj_radio_axis.spr",
    "sprites/obj_icons/icon_obj_docs_neutral.spr",
    "sprites/obj_icons/icon_obj_docs_allies.spr",
    "sprites/obj_icons/icon_obj_docs_axis.spr",
    "sprites/obj_icons/icon_obj_truck_neutral.spr",
    "sprites/obj_icons/icon_obj_truck_allies.spr",
    "sprites/obj_icons/icon_obj_truck_axis.spr",
    "sprites/obj_icons/%s/icon_obj_custom1_neutral.spr",
    "sprites/obj_icons/%s/icon_obj_custom1_allies.spr",
    "sprites/obj_icons/%s/icon_obj_custom1_axis.spr",
    "sprites/obj_icons/%s/icon_obj_custom2_neutral.spr",
    "sprites/obj_icons/%s/icon_obj_custom2_allies.spr",
    "sprites/obj_icons/%s/icon_obj_custom2_axis.spr",
    "sprites/obj_icons/%s/icon_obj_custom3_neutral.spr",
    "sprites/obj_icons/%s/icon_obj_custom3_allies.spr",
    "sprites/obj_icons/%s/icon_obj_custom3_axis.spr",
    "sprites/obj_icons/icon_obj_brit.spr",
    "sprites/obj_icons/%s/icon_obj_custom4_neutral.spr",
    "sprites/obj_icons/%s/icon_obj_custom4_allies.spr",
    "sprites/obj_icons/%s/icon_obj_custom4_axis.spr",
    "sprites/obj_icons/%s/icon_obj_custom5_neutral.spr",
    "sprites/obj_icons/%s/icon_obj_custom5_allies.spr",
    "sprites/obj_icons/%s/icon_obj_custom5_axis.spr",
    "sprites/obj_icons/%s/icon_obj_custom6_neutral.spr",
    "sprites/obj_icons/%s/icon_obj_custom6_allies.spr",
    "sprites/obj_icons/%s/icon_obj_custom6_axis.spr",
];

/// One flag's icon, as the page draws it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlagIcon {
    /// The control point, in world units.
    pub world: [f32; 3],
    /// The icon's size in screen pixels.
    pub width: u32,
    pub height: u32,
    /// Base64 of `width * height * 4` bytes, rows top first.
    pub rgba: String,
}

/// The icon of each control point of `level`, in the team that holds it
/// when the round starts; points whose icon file isn't in the install are
/// left out.
pub fn flag_icons(install: &Path, level: &Level) -> Vec<FlagIcon> {
    use base64::Engine as _;
    let mut out = Vec::new();
    for (entity, origin) in level.points("dod_control_point") {
        let number = |key: &str| entity.get(key).and_then(|v| v.trim().parse::<usize>().ok());
        let owner = number("point_default_owner").unwrap_or(0).min(2);
        let key = [
            "point_hud_icon_neutral",
            "point_hud_icon_allies",
            "point_hud_icon_axis",
        ][owner];
        let index = number(key).filter(|&i| i < ICONS.len()).unwrap_or(owner);
        let Some(path) = icon_file(install, &level.name, ICONS[index]) else {
            continue;
        };
        let Some((width, height, rgba)) = std::fs::read(&path).ok().and_then(|b| decode_spr(&b))
        else {
            continue;
        };
        out.push(FlagIcon {
            world: origin,
            width,
            height,
            rgba: base64::engine::general_purpose::STANDARD.encode(rgba),
        });
    }
    out
}

/// `name` (with `%s` the map) in the install's `dod`, else `dod_addon`.
fn icon_file(install: &Path, map: &str, name: &str) -> Option<PathBuf> {
    let relative = name.replace("%s", map);
    ["dod", "dod_addon"]
        .into_iter()
        .map(|game| install.join(game).join(&relative))
        .find(|p| p.is_file())
}

/// The first frame of a GoldSrc sprite (`IDSP`, version 2) as RGBA, drawn
/// as the HUD draws it (`SPR_DrawHoles`): palette index 255 transparent, or
/// for an index-alpha sprite the index as alpha over the last colour.
pub fn decode_spr(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let i32_at = |at: usize| -> Option<i32> {
        Some(i32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
    };
    if bytes.get(0..4)? != b"IDSP" || i32_at(4)? != 2 {
        return None;
    }
    let format = i32_at(12)?;
    let colours = u16::from_le_bytes(bytes.get(40..42)?.try_into().ok()?) as usize;
    let palette = bytes.get(42..42 + colours * 3)?;
    let mut at = 42 + colours * 3;
    // A frame group: its count and intervals come first.
    if i32_at(at)? != 0 {
        let frames = i32_at(at + 4)?.max(0) as usize;
        at += 8 + frames * 4;
    } else {
        at += 4;
    }
    let width = i32_at(at + 8)?;
    let height = i32_at(at + 12)?;
    if !(1..=1024).contains(&width) || !(1..=1024).contains(&height) {
        return None;
    }
    let (width, height) = (width as usize, height as usize);
    let pixels = bytes.get(at + 16..at + 16 + width * height)?;
    let colour = |i: usize| -> [u8; 3] {
        palette
            .get(i * 3..i * 3 + 3)
            .map(|c| [c[0], c[1], c[2]])
            .unwrap_or([0, 0, 0])
    };
    let mut rgba = Vec::with_capacity(width * height * 4);
    for &p in pixels {
        let (rgb, alpha) = if format == 2 {
            (colour(colours.saturating_sub(1)), p)
        } else if p == 255 {
            ([0, 0, 0], 0)
        } else {
            (colour(p as usize), 255)
        };
        rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], alpha]);
    }
    Some((width as u32, height as u32, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2x1 single-frame alpha-test sprite: one red pixel, one hole.
    fn sprite() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"IDSP");
        for v in [2i32, 0, 3] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&1.0f32.to_le_bytes());
        for v in [2i32, 1, 1] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&0.0f32.to_le_bytes());
        b.extend_from_slice(&0i32.to_le_bytes());
        b.extend_from_slice(&256u16.to_le_bytes());
        let mut palette = vec![0u8; 768];
        palette[3..6].copy_from_slice(&[255, 0, 0]);
        b.extend_from_slice(&palette);
        for v in [0i32, -1, 0, 2, 1] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&[1, 255]);
        b
    }

    #[test]
    fn a_hud_sprite_decodes_with_its_holes() {
        let (w, h, rgba) = decode_spr(&sprite()).unwrap();
        assert_eq!((w, h), (2, 1));
        assert_eq!(rgba, vec![255, 0, 0, 255, 0, 0, 0, 0]);
    }

    #[test]
    fn not_a_sprite_is_none() {
        assert!(decode_spr(b"WAD3....").is_none());
    }

    #[test]
    fn the_icon_table_matches_the_maps_keys() {
        // Keys seen across a library: 27 is the British flag, 19-20 the
        // map's first custom pair.
        assert!(ICONS[27].ends_with("icon_obj_brit.spr"));
        assert!(ICONS[19].contains("%s/icon_obj_custom1_allies"));
        assert!(ICONS[2].ends_with("icon_obj_axis.spr"));
    }
}
