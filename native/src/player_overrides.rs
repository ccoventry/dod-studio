//! Configuration > Players (#428): the spray and the crosshair each player
//! gets in a demo, and what everyone not listed gets.
//!
//! Saved in settings.json as [`PlayerTable`], and mirrored for the hook DLL to
//! `%APPDATA%\dod-studio\goldsrc_hooks_players.txt` ([`table_text`]) whenever
//! it changes. The hook reads that file (`goldsrc-hooks/src/player_table.rs`)
//! rather than settings.json so its parser stays a few lines of tab-separated
//! text, and so a settings file it can't read never touches the table. Both
//! sides test against `test-fixtures/players_table.txt`, so the format can't
//! drift between the writer and the reader. Studio never writes into the game
//! install for this: the PNGs and sprites are the user's own, in
//! `<game>\logos\` and `<game>\sprites\`.
//!
//! The hook applies the table only while `dodstudio_player_overrides` is 1;
//! Studio sets it from the tab's "Use in the game" box ([`cvar_line`]) when it
//! starts the game, a review or a batch, and when the box changes.

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The file the hook reads, in Studio's app-data folder.
pub const TABLE_FILE: &str = "goldsrc_hooks_players.txt";
/// The hook's switch for the whole table (`goldsrc-hooks/src/player_overrides.rs`).
pub const OVERRIDES_CVAR: &str = "dodstudio_player_overrides";
/// The crosshair sprite DoD ships and `cl_xhair_style` picks a tile of.
pub const DEFAULT_CROSSHAIR_SPRITE: &str = "customXHair.spr";
/// `customXHair.spr`'s grid: 4x4 tiles of 64x64 (`docs/goldsrc_crosshair.md`).
pub const TILES: u8 = 16;

/// The console line that turns the table on or off in the game.
pub fn cvar_line(on: bool) -> String {
    format!("{OVERRIDES_CVAR} {}", u8::from(on))
}

/// One row: a player, and what he gets. An empty `png` or a `crosshair_tile`
/// of 0 leaves that one to the fallback.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerRow {
    /// SteamID64 as digits, or empty when only the name is known (a bot, a
    /// player the demo carries no SteamID for, or one typed by name).
    #[serde(default)]
    pub steam_id: String,
    /// The name to match when there is no SteamID, and the label otherwise.
    #[serde(default)]
    pub name: String,
    /// A PNG in `<game>\logos\`.
    #[serde(default)]
    pub png: String,
    /// A sprite in `<game>\sprites\`.
    #[serde(default)]
    pub crosshair_sprite: String,
    /// 1..=16, the tile of the sprite's 4x4 grid; 0 for none.
    #[serde(default)]
    pub crosshair_tile: u8,
}

/// What a player not in the table gets for his spray.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SprayFallback {
    /// The game's own: the spray the demo saved (POV), the bullet hole the
    /// engine draws when it has no image (HLTV with no PNG picked).
    #[default]
    Own,
    /// The one PNG in `*_fallback_png`.
    Png,
    /// No spray at all.
    None,
}

/// What a player not in the table gets for his crosshair.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrosshairFallback {
    /// The viewer's own `cl_xhair_style` tile of `customXHair.spr`, as today.
    #[default]
    Game,
    /// The one sprite and tile in `crosshair_fallback_*`.
    Sprite,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerTable {
    /// The tab's "Use in the game" box: [`OVERRIDES_CVAR`] as Studio sets it.
    #[serde(default = "default_true")]
    pub use_in_game: bool,
    #[serde(default)]
    pub players: Vec<PlayerRow>,
    /// POV demos' spray fallback: `own` by default.
    #[serde(default)]
    pub pov_fallback: SprayFallback,
    #[serde(default)]
    pub pov_fallback_png: String,
    /// HLTV demos': `png` by default. `own` is never offered (an HLTV demo
    /// records that a player sprayed, never the image) and reads as the
    /// game's bullet hole, like `png` with no PNG picked.
    #[serde(default = "default_hltv_fallback")]
    pub hltv_fallback: SprayFallback,
    #[serde(default)]
    pub hltv_fallback_png: String,
    #[serde(default)]
    pub crosshair_fallback: CrosshairFallback,
    #[serde(default = "default_crosshair_sprite")]
    pub crosshair_fallback_sprite: String,
    #[serde(default = "default_tile")]
    pub crosshair_fallback_tile: u8,
}

fn default_true() -> bool {
    true
}

fn default_hltv_fallback() -> SprayFallback {
    SprayFallback::Png
}

fn default_crosshair_sprite() -> String {
    DEFAULT_CROSSHAIR_SPRITE.to_string()
}

fn default_tile() -> u8 {
    1
}

impl Default for PlayerTable {
    fn default() -> Self {
        Self {
            use_in_game: true,
            players: Vec::new(),
            pov_fallback: SprayFallback::Own,
            pov_fallback_png: String::new(),
            hltv_fallback: default_hltv_fallback(),
            hltv_fallback_png: String::new(),
            crosshair_fallback: CrosshairFallback::Game,
            crosshair_fallback_sprite: default_crosshair_sprite(),
            crosshair_fallback_tile: default_tile(),
        }
    }
}

/// A field as one tab-separated cell: no tab, no line break.
fn cell(text: &str) -> String {
    text.trim()
        .chars()
        .map(|c| {
            if matches!(c, '\t' | '\r' | '\n') {
                ' '
            } else {
                c
            }
        })
        .collect()
}

/// A file name the hook accepts: inside its folder, never a path.
fn is_file_name(name: &str, extension: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && !name.contains(['/', '\\', ':', '\t'])
        && name.to_ascii_lowercase().ends_with(extension)
}

/// A PNG name the hook accepts: a file name inside `logos\`.
pub fn is_png_name(name: &str) -> bool {
    is_file_name(name, ".png")
}

/// A sprite name the hook accepts: a file name inside `sprites\`.
pub fn is_sprite_name(name: &str) -> bool {
    is_file_name(name, ".spr")
}

fn valid_tile(tile: u8) -> bool {
    (1..=TILES).contains(&tile)
}

/// `game`, `none` or `png <file>`; a PNG fallback with no usable file is
/// the game's own.
fn spray_fallback_cells(kind: SprayFallback, png: &str) -> String {
    match kind {
        SprayFallback::None => "none".to_string(),
        SprayFallback::Png if is_png_name(png) => format!("png\t{}", cell(png)),
        _ => "game".to_string(),
    }
}

/// `game` or `sprite <file> <tile>`; an unusable sprite or tile is the
/// game's own.
fn crosshair_fallback_cells(table: &PlayerTable) -> String {
    match table.crosshair_fallback {
        CrosshairFallback::Sprite
            if is_sprite_name(&table.crosshair_fallback_sprite)
                && valid_tile(table.crosshair_fallback_tile) =>
        {
            format!(
                "sprite\t{}\t{}",
                cell(&table.crosshair_fallback_sprite),
                table.crosshair_fallback_tile
            )
        }
        _ => "game".to_string(),
    }
}

fn steam_id64(id: &str) -> Option<String> {
    let id = cell(id);
    (id.len() == 17 && id.bytes().all(|b| b.is_ascii_digit())).then_some(id)
}

/// The hook's file. Lines are tab-separated:
///
/// - `spray_pov_fallback` / `spray_hltv_fallback`, then `game`, `none` or
///   `png <file>`;
/// - `crosshair_fallback`, then `game` or `sprite <file> <tile>`;
/// - `spray steamid <SteamID64> <file> <name>` / `spray name <name> <file>`;
/// - `crosshair steamid <SteamID64> <file> <tile> <name>` /
///   `crosshair name <name> <file> <tile>`.
///
/// A `steamid` row matches by SteamID (and by name only when the game doesn't
/// know the player's SteamID); a `name` row by name, ignoring case. Rows
/// without a usable file or anyone to match are left out.
pub fn table_text(table: &PlayerTable) -> String {
    let mut out = String::from(
        "// DoD Studio's Players table (Configuration > Players), for the hook DLL (#428).\n\
         // Studio rewrites this file whenever the table changes: edit it there.\n",
    );
    out.push_str(&format!(
        "spray_pov_fallback\t{}\n",
        spray_fallback_cells(table.pov_fallback, &table.pov_fallback_png)
    ));
    out.push_str(&format!(
        "spray_hltv_fallback\t{}\n",
        spray_fallback_cells(table.hltv_fallback, &table.hltv_fallback_png)
    ));
    out.push_str(&format!(
        "crosshair_fallback\t{}\n",
        crosshair_fallback_cells(table)
    ));
    for p in &table.players {
        let name = cell(&p.name);
        let id = steam_id64(&p.steam_id);
        if id.is_none() && name.is_empty() {
            continue;
        }
        if is_png_name(&p.png) {
            let png = cell(&p.png);
            match &id {
                Some(id) => out.push_str(&format!("spray\tsteamid\t{id}\t{png}\t{name}\n")),
                None => out.push_str(&format!("spray\tname\t{name}\t{png}\n")),
            }
        }
        if is_sprite_name(&p.crosshair_sprite) && valid_tile(p.crosshair_tile) {
            let (sprite, tile) = (cell(&p.crosshair_sprite), p.crosshair_tile);
            match &id {
                Some(id) => out.push_str(&format!(
                    "crosshair\tsteamid\t{id}\t{sprite}\t{tile}\t{name}\n"
                )),
                None => out.push_str(&format!("crosshair\tname\t{name}\t{sprite}\t{tile}\n")),
            }
        }
    }
    out
}

pub fn table_path() -> PathBuf {
    crate::shared::paths::get_appdata_dir().join(TABLE_FILE)
}

/// Writes the hook's file when its text changed. The hook rereads it when
/// its modified time moves, so an unchanged table isn't written again.
pub fn write_table_at(path: &Path, table: &PlayerTable) -> Result<bool, String> {
    let text = table_text(table);
    if std::fs::read_to_string(path).is_ok_and(|old| old == text) {
        return Ok(false);
    }
    std::fs::write(path, text).map_err(|e| {
        format!(
            "could not write the Players table to {}: {e}",
            path.display()
        )
    })?;
    Ok(true)
}

pub fn write_table(table: &PlayerTable) -> Result<bool, String> {
    write_table_at(&table_path(), table)
}

/// The files in `dir` whose name `keep` accepts, sorted case-insensitively.
/// Empty when the folder isn't there.
fn list_files(dir: &Path, keep: fn(&str) -> bool) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .filter_map(|e| e.file_name().to_str().map(str::to_string))
                .filter(|n| keep(n))
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|n| n.to_ascii_lowercase());
    out
}

/// The PNGs in `<game>\logos\`, for the spray dropdowns.
pub fn list_logo_pngs(game_dir: &Path) -> Vec<String> {
    list_files(&game_dir.join("logos"), is_png_name)
}

/// The sprites in `<game>\sprites\`, for the crosshair dropdowns.
pub fn list_sprites(game_dir: &Path) -> Vec<String> {
    list_files(&game_dir.join("sprites"), is_sprite_name)
}

// ── Sprite preview ──────────────────────────────────────────────────────────

/// A sprite's first frame as RGBA, for the tile picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpriteImage {
    pub width: u32,
    pub height: u32,
    /// RGBA8, top row first, base64.
    pub rgba: String,
}

/// Three of the four `texFormat`s of a GoldSrc sprite (0 is plain).
const SPR_ADDITIVE: i32 = 1;
const SPR_INDEXALPHA: i32 = 2;
const SPR_ALPHTEST: i32 = 3;
/// The largest frame side decoded for a preview.
const MAX_SIDE: u32 = 1024;

fn read_i32(bytes: &[u8], at: usize) -> Result<i32, String> {
    bytes
        .get(at..at.saturating_add(4))
        .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| "the file ends early".to_string())
}

/// Decodes a GoldSrc sprite's first frame (version 2, `IDSP`): the 40-byte
/// header, a palette of `count` colours, then frames; a group's first frame
/// is taken. Transparency follows the sprite's own `texFormat` the way the
/// engine draws it: index 255 cut out (alpha-test), the index as alpha over
/// the last colour (index-alpha), brightness as alpha (additive, which adds
/// onto the screen, so black is nothing).
pub fn decode_sprite(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    if bytes.get(..4) != Some(b"IDSP") {
        return Err("not a sprite (no IDSP header)".into());
    }
    if read_i32(bytes, 4)? != 2 {
        return Err("not a version 2 (Half-Life) sprite".into());
    }
    let tex_format = read_i32(bytes, 12)?;
    if read_i32(bytes, 28)? < 1 {
        return Err("the sprite has no frames".into());
    }
    let count = bytes
        .get(40..42)
        .map(|b| u16::from_le_bytes([b[0], b[1]]) as usize)
        .ok_or("the file ends early")?;
    if count == 0 || count > 256 {
        return Err(format!("{count} palette colours"));
    }
    let palette = bytes
        .get(42..42 + 3 * count)
        .ok_or("the palette runs past the end")?;
    // Each frame starts with its type; a single frame's `dspriteframe_t`
    // (origin x, y, width, height, pixels) follows it, while a group has its
    // frame count and one interval per frame first, and its frames carry no
    // type of their own.
    let first = 42 + 3 * count;
    let frame = if read_i32(bytes, first)? == 0 {
        first + 4
    } else {
        let n = read_i32(bytes, first + 4)?;
        if !(1..=1024).contains(&n) {
            return Err(format!("a frame group of {n}"));
        }
        first + 8 + 4 * n as usize
    };
    let (width, height) = (read_i32(bytes, frame + 8)?, read_i32(bytes, frame + 12)?);
    if width < 1 || height < 1 || width as u32 > MAX_SIDE || height as u32 > MAX_SIDE {
        return Err(format!("a {width}x{height} frame"));
    }
    let (width, height) = (width as u32, height as u32);
    let pixels = bytes
        .get(frame + 16..frame + 16 + (width * height) as usize)
        .ok_or("the frame runs past the end")?;
    let colour = |i: u8| {
        let i = (i as usize).min(count - 1);
        [palette[3 * i], palette[3 * i + 1], palette[3 * i + 2]]
    };
    let mut rgba = Vec::with_capacity(pixels.len() * 4);
    for &i in pixels {
        let px = match tex_format {
            SPR_INDEXALPHA => {
                let [r, g, b] = colour(255);
                [r, g, b, i]
            }
            SPR_ALPHTEST if i == 255 => [0, 0, 0, 0],
            SPR_ADDITIVE => {
                let [r, g, b] = colour(i);
                [r, g, b, r.max(g).max(b)]
            }
            _ => {
                let [r, g, b] = colour(i);
                [r, g, b, 255]
            }
        };
        rgba.extend_from_slice(&px);
    }
    Ok((width, height, rgba))
}

/// [`decode_sprite`] of a file, for the frontend.
pub fn sprite_image(path: &Path) -> Result<SpriteImage, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let (width, height, rgba) =
        decode_sprite(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(SpriteImage {
        width,
        height,
        rgba: base64::engine::general_purpose::STANDARD.encode(rgba),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table `test-fixtures/players_table.txt` holds; the hook's parser
    /// tests read the same file (`player_table.rs`).
    fn fixture_table() -> PlayerTable {
        let row = |steam_id: &str, name: &str, png: &str, sprite: &str, tile: u8| PlayerRow {
            steam_id: steam_id.into(),
            name: name.into(),
            png: png.into(),
            crosshair_sprite: sprite.into(),
            crosshair_tile: tile,
        };
        PlayerTable {
            use_in_game: true,
            players: vec![
                row(
                    "76561197972576011",
                    "dicE[: :]m00cat :D",
                    "chris_teamwork.png",
                    "customXHair.spr",
                    3,
                ),
                row("", "Cpl. Preddy", "bot spray.png", "", 0),
                row("", "Xhair Only", "", "my_xhair.spr", 16),
                // Nothing usable: left out.
                row("76561197960265729", "nobody", "", "customXHair.spr", 0),
                // Paths, not file names: left out.
                row("", "escape", "../x.png", "../bad.spr", 2),
            ],
            pov_fallback: SprayFallback::None,
            pov_fallback_png: "ignored.png".into(),
            hltv_fallback: SprayFallback::Png,
            hltv_fallback_png: "team_allies.png".into(),
            crosshair_fallback: CrosshairFallback::Sprite,
            crosshair_fallback_sprite: "customXHair.spr".into(),
            crosshair_fallback_tile: 7,
        }
    }

    #[test]
    fn the_file_is_the_shared_fixture() {
        let fixture = include_str!("../../test-fixtures/players_table.txt").replace("\r\n", "\n");
        assert_eq!(table_text(&fixture_table()), fixture);
    }

    #[test]
    fn fallbacks_without_a_file_are_the_games_own() {
        let table = PlayerTable::default();
        let text = table_text(&table);
        assert!(text.contains("spray_pov_fallback\tgame\n"), "{text}");
        // The HLTV default is a fallback PNG, but none is picked yet.
        assert_eq!(table.hltv_fallback, SprayFallback::Png);
        assert!(text.contains("spray_hltv_fallback\tgame\n"), "{text}");
        assert!(text.contains("crosshair_fallback\tgame\n"), "{text}");
        let bad_tile = PlayerTable {
            crosshair_fallback: CrosshairFallback::Sprite,
            crosshair_fallback_tile: 17,
            ..PlayerTable::default()
        };
        assert!(table_text(&bad_tile).contains("crosshair_fallback\tgame\n"));
    }

    #[test]
    fn cells_never_split_a_line() {
        let table = PlayerTable {
            players: vec![PlayerRow {
                name: "tab\there\nnewline".into(),
                png: "a.png".into(),
                ..PlayerRow::default()
            }],
            ..PlayerTable::default()
        };
        assert!(table_text(&table).contains("spray\tname\ttab here newline\ta.png\n"));
    }

    #[test]
    fn a_settings_file_without_the_table_loads_the_defaults() {
        let table: PlayerTable = serde_json::from_str("{}").unwrap();
        assert_eq!(table, PlayerTable::default());
        assert!(table.use_in_game);
        assert_eq!(table.crosshair_fallback_sprite, "customXHair.spr");
        let partial: PlayerTable = serde_json::from_str(
            r#"{"use_in_game":false,"pov_fallback":"png","crosshair_fallback":"sprite"}"#,
        )
        .unwrap();
        assert!(!partial.use_in_game);
        assert_eq!(partial.pov_fallback, SprayFallback::Png);
        assert_eq!(partial.crosshair_fallback, CrosshairFallback::Sprite);
        assert_eq!(partial.hltv_fallback, SprayFallback::Png);
    }

    #[test]
    fn the_box_becomes_the_cvar_line() {
        assert_eq!(cvar_line(true), "dodstudio_player_overrides 1");
        assert_eq!(cvar_line(false), "dodstudio_player_overrides 0");
    }

    #[test]
    fn written_only_when_the_text_changes() {
        let dir = crate::test_support::Scratch::new("players_table");
        let path = dir.join(TABLE_FILE);
        assert!(write_table_at(&path, &fixture_table()).unwrap());
        assert!(!write_table_at(&path, &fixture_table()).unwrap());
        assert!(write_table_at(&path, &PlayerTable::default()).unwrap());
    }

    #[test]
    fn lists_only_pngs_in_logos_and_sprites_in_sprites() {
        let dir = crate::test_support::Scratch::new("players_lists");
        let logos = dir.join("logos");
        let sprites = dir.join("sprites");
        std::fs::create_dir_all(logos.join("sub.png")).unwrap();
        std::fs::create_dir_all(&sprites).unwrap();
        for name in ["b.PNG", "a.png", "tempdecal.wad", "notes.txt"] {
            std::fs::write(logos.join(name), b"x").unwrap();
        }
        for name in ["customXHair.spr", "Bar.SPR", "hud.txt"] {
            std::fs::write(sprites.join(name), b"x").unwrap();
        }
        assert_eq!(list_logo_pngs(&dir), vec!["a.png", "b.PNG"]);
        assert_eq!(list_sprites(&dir), vec!["Bar.SPR", "customXHair.spr"]);
        assert!(list_logo_pngs(&dir.join("missing")).is_empty());
    }

    /// A sprite as the engine's loader lays it out: one `w`x`h` frame, alone
    /// or as a group of one.
    fn sprite(tex_format: i32, w: i32, h: i32, pixels: &[u8], group: bool) -> Vec<u8> {
        let mut out = b"IDSP".to_vec();
        for v in [2, 2, tex_format] {
            out.extend_from_slice(&i32::to_le_bytes(v));
        }
        out.extend_from_slice(&0f32.to_le_bytes());
        for v in [w, h, 1] {
            out.extend_from_slice(&i32::to_le_bytes(v));
        }
        out.extend_from_slice(&0f32.to_le_bytes());
        out.extend_from_slice(&0i32.to_le_bytes());
        out.extend_from_slice(&256u16.to_le_bytes());
        for i in 0..=255u8 {
            out.extend_from_slice(&[i, 0, 255 - i]);
        }
        if group {
            out.extend_from_slice(&1i32.to_le_bytes()); // frame type: group
            out.extend_from_slice(&1i32.to_le_bytes()); // of one frame
            out.extend_from_slice(&0.1f32.to_le_bytes());
        } else {
            out.extend_from_slice(&0i32.to_le_bytes()); // frame type: single
        }
        for v in [-w / 2, h / 2, w, h] {
            out.extend_from_slice(&i32::to_le_bytes(v));
        }
        out.extend_from_slice(pixels);
        out
    }

    #[test]
    fn decodes_a_sprite_the_way_the_engine_draws_it() {
        let pixels = [0u8, 7, 255, 128];
        let (w, h, rgba) = decode_sprite(&sprite(3, 2, 2, &pixels, false)).unwrap();
        assert_eq!((w, h), (2, 2));
        assert_eq!(&rgba[..4], &[0, 0, 255, 255]);
        assert_eq!(&rgba[4..8], &[7, 0, 248, 255]);
        assert_eq!(&rgba[8..12], &[0, 0, 0, 0], "alpha-test cuts index 255 out");
        // Index-alpha: the last colour, the index as alpha.
        let (_, _, rgba) = decode_sprite(&sprite(2, 2, 2, &pixels, false)).unwrap();
        assert_eq!(&rgba[4..8], &[255, 0, 0, 7]);
        // Additive: brightness as alpha.
        let (_, _, rgba) = decode_sprite(&sprite(1, 2, 2, &pixels, false)).unwrap();
        assert_eq!(rgba[3], 255);
        // A group's first frame.
        let (w, h, _) = decode_sprite(&sprite(0, 2, 2, &pixels, true)).unwrap();
        assert_eq!((w, h), (2, 2));
    }

    #[test]
    fn a_bad_sprite_is_an_error_not_a_panic() {
        let good = sprite(0, 2, 2, &[1, 2, 3, 4], false);
        for len in 0..good.len() {
            assert!(decode_sprite(&good[..len]).is_err(), "{len}");
        }
        let mut huge = good.clone();
        let at = 42 + 768 + 4 + 8;
        huge[at..at + 4].copy_from_slice(&5000i32.to_le_bytes());
        assert!(decode_sprite(&huge).is_err());
        assert!(decode_sprite(b"WAD3....").is_err());
    }
}
