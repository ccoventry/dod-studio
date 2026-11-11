//! Studio's Players table (Configuration > Players, #428), as the hook reads
//! it: each player's spray and crosshair in a demo.
//!
//! Studio writes `%APPDATA%\dod-studio\goldsrc_hooks_players.txt` whenever the
//! table changes (`native::player_overrides::table_text`); this parses it,
//! and decides what one player's spray or crosshair becomes. The format is in
//! that writer's doc, and both sides test against
//! `test-fixtures/players_table.txt`. Pure, so the rules are tested without a
//! game. Nothing here applies unless `ds_player_overrides` is 1
//! (`player_overrides.rs`).
//!
//! The rule, the same for both demo types: a player in the table gets their
//! PNG and their crosshair; anyone else gets the fallbacks. Sprays fall back
//! per demo type -- POV: the game's own (the spray the demo saved), one PNG,
//! or none; HLTV: one PNG or none (`game` there is the engine's bullet hole,
//! which is what an HLTV demo draws with no image). The crosshair falls back
//! to the game's own (the viewer's `cl_xhair_style` tile of
//! `customXHair.spr`) or one sprite and tile.

use std::path::PathBuf;

/// The file's name in Studio's app-data folder.
pub const FILE_NAME: &str = "goldsrc_hooks_players.txt";
/// Tiles in a crosshair sprite's 4x4 grid (`spectator_crosshair::tile_rect`).
pub const TILES: u8 = 16;
/// The longer side of a spray built from a PNG when the file names none:
/// the size of a typical saved spray, so a built one sits beside a real one
/// at the same scale.
pub const DEFAULT_SPRAY_SIZE: u32 = 64;
/// The sizes the file may name: the writer's sides go in steps of 16, and
/// `Draw_ValidateCustomLogo` takes up to 256.
const SPRAY_SIZES: std::ops::RangeInclusive<u32> = 16..=256;

/// What a spray becomes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum Choice {
    /// Whatever the engine would draw: the demo's own spray, or the bullet
    /// hole when it has none.
    #[default]
    Game,
    /// Nothing: a fully transparent spray.
    Blank,
    /// A PNG in `<game>\logos\`, by file name.
    Png(String),
    /// A listed player's "From the demo": the spray the demo saved, drawn by
    /// the game as it is; [`Table::resolve`] makes it the fallback when the
    /// demo saved none.
    Own,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Choice::Game => f.write_str("the game's own"),
            Choice::Blank => f.write_str("no spray"),
            Choice::Own => f.write_str("the spray from the demo"),
            Choice::Png(png) => f.write_str(png),
        }
    }
}

/// What a crosshair becomes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum Crosshair {
    /// The game's own: whatever `CHudDoDCrossHair` draws.
    #[default]
    Game,
    /// A sprite in `<game>\sprites\` and a tile (1..=16) of its 4x4 grid.
    Tile { sprite: String, tile: u8 },
}

impl std::fmt::Display for Crosshair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Crosshair::Game => f.write_str("the game's own"),
            Crosshair::Tile { sprite, tile } => write!(f, "tile {tile} of {sprite}"),
        }
    }
}

/// Who a row is for: a SteamID64 (0 for none) and a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Who {
    pub steam_id: u64,
    pub name: String,
}

/// One spray row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub who: Who,
    /// A PNG, or `None` for "From the demo".
    pub png: Option<String>,
}

/// One crosshair row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrosshairRow {
    pub who: Who,
    pub sprite: String,
    pub tile: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Table {
    pub players: Vec<Row>,
    pub crosshairs: Vec<CrosshairRow>,
    pub pov: Choice,
    pub hltv: Choice,
    pub crosshair: Crosshair,
    /// The longer side of a spray built from a PNG, from a `spray_size`
    /// line; 0 when there is none ([`Table::spray_side`]).
    pub spray_size: u32,
    /// Lines that weren't a comment, blank, or one of the shapes, by number.
    pub bad: Vec<usize>,
}

/// A file name inside its folder, never a path.
fn file_name(raw: &str, extension: &str) -> Option<String> {
    let name = raw.trim();
    (!name.is_empty()
        && !name.contains(['/', '\\', ':'])
        && name.to_ascii_lowercase().ends_with(extension))
    .then(|| name.to_string())
}

fn png_name(raw: &str) -> Option<String> {
    file_name(raw, ".png")
}

/// A sprite name: a file name inside `sprites\`.
pub fn sprite_name(raw: &str) -> Option<String> {
    file_name(raw, ".spr")
}

fn tile(raw: &str) -> Option<u8> {
    raw.trim().parse().ok().filter(|t| (1..=TILES).contains(t))
}

fn spray_fallback(cells: &[&str]) -> Option<Choice> {
    match cells {
        ["game" | "own"] => Some(Choice::Game),
        ["none"] => Some(Choice::Blank),
        ["png", file] => png_name(file).map(Choice::Png),
        _ => None,
    }
}

fn crosshair_fallback(cells: &[&str]) -> Option<Crosshair> {
    match cells {
        ["game"] => Some(Crosshair::Game),
        ["sprite", file, t] => Some(Crosshair::Tile {
            sprite: sprite_name(file)?,
            tile: tile(t)?,
        }),
        _ => None,
    }
}

/// A 17-digit SteamID64 (the form the file holds).
fn steam_id64(text: &str) -> Option<u64> {
    use crate::steam_id::BASE;
    let text = text.trim();
    if text.len() != 17 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok().filter(|&id| id > BASE)
}

/// Who a row's cells after its kind name: `steamid <id> ... [<name>]` or
/// `name <name> ...`, with `n` cells of its own after the identity, and the
/// rest of the line.
fn who<'a>(cells: &'a [&'a str], n: usize) -> Option<(Who, &'a [&'a str])> {
    match cells {
        ["steamid", id, rest @ ..] if rest.len() == n || rest.len() == n + 1 => Some((
            Who {
                steam_id: steam_id64(id)?,
                name: rest.get(n).map_or(String::new(), |s| s.to_string()),
            },
            &rest[..n],
        )),
        ["name", name, rest @ ..] if rest.len() == n && !name.is_empty() => Some((
            Who {
                steam_id: 0,
                name: name.to_string(),
            },
            rest,
        )),
        _ => None,
    }
}

pub fn parse(text: &str) -> Table {
    let mut table = Table::default();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.trim_start().starts_with("//") {
            continue;
        }
        let cells: Vec<&str> = line.split('\t').map(str::trim).collect();
        let ok = match cells.as_slice() {
            ["spray_pov_fallback", rest @ ..] => spray_fallback(rest).map(|c| table.pov = c),
            ["spray_hltv_fallback", rest @ ..] => spray_fallback(rest).map(|c| table.hltv = c),
            ["spray_size", n] => n
                .parse::<u32>()
                .ok()
                .filter(|n| SPRAY_SIZES.contains(n) && n.is_multiple_of(16))
                .map(|n| table.spray_size = n),
            ["crosshair_fallback", rest @ ..] => {
                crosshair_fallback(rest).map(|c| table.crosshair = c)
            }
            ["spray", rest @ ..] => who(rest, 1).and_then(|(who, cells)| {
                let png = match cells[0] {
                    "own" => None,
                    file => Some(png_name(file)?),
                };
                table.players.push(Row { who, png });
                Some(())
            }),
            ["crosshair", rest @ ..] => who(rest, 2).and_then(|(who, cells)| {
                let (sprite, tile) = (sprite_name(cells[0])?, tile(cells[1])?);
                table.crosshairs.push(CrosshairRow { who, sprite, tile });
                Some(())
            }),
            _ => None,
        }
        .is_some();
        if !ok {
            table.bad.push(i + 1);
        }
    }
    table
}

/// The first of `rows` for a player: by SteamID first; then by name,
/// ignoring case, among the rows with no SteamID -- and the SteamID rows too
/// when the game doesn't know this player's SteamID (`steam_id` 0), since a
/// known but different SteamID is a different person who took the name.
fn row_for<'a, T>(
    rows: &'a [T],
    who: impl Fn(&T) -> &Who,
    name: &str,
    steam_id: u64,
) -> Option<&'a T> {
    if steam_id != 0
        && let Some(row) = rows.iter().find(|r| who(r).steam_id == steam_id)
    {
        return Some(row);
    }
    let name = name.trim().to_lowercase();
    if name.is_empty() {
        return None;
    }
    rows.iter().find(|r| {
        let w = who(r);
        (w.steam_id == 0 || steam_id == 0) && w.name.trim().to_lowercase() == name
    })
}

impl Table {
    /// Whether the sprays ask for anything the game wouldn't do by itself.
    /// Until they do, the hook installs nothing for them.
    pub fn active(&self) -> bool {
        !self.players.is_empty() || self.pov != Choice::Game || self.hltv != Choice::Game
    }

    /// Whether the crosshairs ask for anything.
    /// How big a spray built from a PNG is drawn: its longer side.
    pub fn spray_side(&self) -> u32 {
        if self.spray_size == 0 {
            DEFAULT_SPRAY_SIZE
        } else {
            self.spray_size
        }
    }

    pub fn crosshair_active(&self) -> bool {
        !self.crosshairs.is_empty() || self.crosshair != Crosshair::Game
    }

    /// The spray row for a player.
    pub fn row_for(&self, name: &str, steam_id: u64) -> Option<&Row> {
        row_for(&self.players, |r| &r.who, name, steam_id)
    }

    /// What this player's row or the fallback says for a demo of this type,
    /// before [`Table::resolve`] knows whether the demo saved the spray.
    pub fn choose(&self, hltv: bool, name: &str, steam_id: u64) -> Choice {
        match self.row_for(name, steam_id) {
            Some(Row { png: Some(png), .. }) => Choice::Png(png.clone()),
            Some(Row { png: None, .. }) => Choice::Own,
            None => self.fallback(hltv),
        }
    }

    /// What players not listed get in a demo of this type.
    pub fn fallback(&self, hltv: bool) -> Choice {
        if hltv {
            self.hltv.clone()
        } else {
            self.pov.clone()
        }
    }

    /// What the spray is, once it is known whether the demo saved one for
    /// the player (`saved`). "From the demo" with none saved is the fallback
    /// for the demo type. The game's own with none saved would be the
    /// engine's bullet-hole stand-in, which is never what the table means:
    /// it is no spray. So [`Choice::Game`] comes back only with a saved
    /// spray, and never [`Choice::Own`].
    pub fn resolve(&self, choice: Choice, saved: bool, hltv: bool) -> Choice {
        match choice {
            Choice::Own if saved => Choice::Game,
            Choice::Own => self.resolve(self.fallback(hltv), saved, hltv),
            Choice::Game if saved => Choice::Game,
            Choice::Game => Choice::Blank,
            other => other,
        }
    }

    /// The crosshair row for a player.
    pub fn crosshair_row_for(&self, name: &str, steam_id: u64) -> Option<&CrosshairRow> {
        row_for(&self.crosshairs, |r| &r.who, name, steam_id)
    }

    /// What the crosshair becomes while this player's view is on screen.
    pub fn choose_crosshair(&self, name: &str, steam_id: u64) -> Crosshair {
        match self.crosshair_row_for(name, steam_id) {
            Some(row) => Crosshair::Tile {
                sprite: row.sprite.clone(),
                tile: row.tile,
            },
            None => self.crosshair.clone(),
        }
    }

    /// Every PNG the table names, each once, in file order.
    pub fn pngs(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let fallbacks = [&self.pov, &self.hltv];
        let named = self.players.iter().filter_map(|r| r.png.as_deref()).chain(
            fallbacks.into_iter().filter_map(|c| match c {
                Choice::Png(p) => Some(p.as_str()),
                _ => None,
            }),
        );
        for png in named {
            if !out.iter().any(|p| p.eq_ignore_ascii_case(png)) {
                out.push(png.to_string());
            }
        }
        out
    }
}

/// `%APPDATA%\dod-studio\goldsrc_hooks_players.txt`, where Studio keeps the
/// other files it shares with the hook (`demo_list_folders`,
/// `window_layout`). `None` with no `%APPDATA%`.
pub fn path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(appdata).join("dod-studio").join(FILE_NAME))
}

/// Reads the table afresh; no file is an empty table.
pub fn load() -> Table {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map_or_else(Table::default, |text| parse(&text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Table {
        parse(include_str!("../../test-fixtures/players_table.txt"))
    }

    const M00CAT: u64 = 76561197972576011;

    /// The file `native::player_overrides::table_text` writes (its test
    /// checks it writes exactly this) reads back as the table Studio saved.
    #[test]
    fn reads_the_shared_fixture() {
        let t = fixture();
        assert!(t.bad.is_empty(), "{:?}", t.bad);
        assert_eq!(t.pov, Choice::Blank);
        assert_eq!(t.hltv, Choice::Png("team_allies.png".into()));
        assert_eq!(
            t.crosshair,
            Crosshair::Tile {
                sprite: "customXHair.spr".into(),
                tile: 7
            }
        );
        let who = |steam_id, name: &str| Who {
            steam_id,
            name: name.into(),
        };
        assert_eq!(
            t.players,
            vec![
                Row {
                    who: who(M00CAT, "dicE[: :]m00cat :D"),
                    png: Some("chris_teamwork.png".into()),
                },
                Row {
                    who: who(0, "Cpl. Preddy"),
                    png: Some("bot spray.png".into()),
                },
                Row {
                    who: who(0, "Own Spray"),
                    png: None,
                },
            ]
        );
        assert_eq!(
            t.crosshairs,
            vec![
                CrosshairRow {
                    who: who(M00CAT, "dicE[: :]m00cat :D"),
                    sprite: "customXHair.spr".into(),
                    tile: 3,
                },
                CrosshairRow {
                    who: who(0, "Xhair Only"),
                    sprite: "my_xhair.spr".into(),
                    tile: 16,
                },
            ]
        );
        assert!(t.active() && t.crosshair_active());
        assert_eq!(t.spray_side(), 96);
        assert_eq!(
            t.pngs(),
            vec!["chris_teamwork.png", "bot spray.png", "team_allies.png"]
        );
    }

    #[test]
    fn steam_id_first_then_name_ignoring_case() {
        let t = fixture();
        let m00cat = Choice::Png("chris_teamwork.png".into());
        // By SteamID, whatever the name is today.
        assert_eq!(t.choose(false, "someone else", M00CAT), m00cat);
        // By name when the game doesn't know their SteamID.
        assert_eq!(t.choose(true, "DICE[: :]M00CAT :d", 0), m00cat);
        // A known, different SteamID with their name is someone else.
        assert_ne!(
            t.choose(true, "dicE[: :]m00cat :D", 76561197960265729),
            m00cat
        );
        // A name row matches by name, SteamID or not.
        let bot = Choice::Png("bot spray.png".into());
        assert_eq!(t.choose(false, "cpl. preddy", 0), bot);
        assert_eq!(t.choose(false, " Cpl. Preddy ", 76561197960265730), bot);
        assert_eq!(t.row_for("", 0), None);
    }

    #[test]
    fn crosshairs_follow_the_same_matching_and_fall_back() {
        let t = fixture();
        let tile = |sprite: &str, tile| Crosshair::Tile {
            sprite: sprite.into(),
            tile,
        };
        assert_eq!(
            t.choose_crosshair("anyone", M00CAT),
            tile("customXHair.spr", 3)
        );
        assert_eq!(
            t.choose_crosshair("xhair only", 0),
            tile("my_xhair.spr", 16)
        );
        // Listed for a spray only: the crosshair falls back.
        assert_eq!(
            t.choose_crosshair("Cpl. Preddy", 0),
            tile("customXHair.spr", 7)
        );
        assert_eq!(
            t.choose_crosshair("stranger", 0),
            tile("customXHair.spr", 7)
        );
        let game = parse("crosshair_fallback\tgame\n");
        assert!(!game.crosshair_active());
        assert_eq!(game.choose_crosshair("x", 0), Crosshair::Game);
        assert_eq!(
            Crosshair::Tile {
                sprite: "a.spr".into(),
                tile: 2
            }
            .to_string(),
            "tile 2 of a.spr"
        );
    }

    #[test]
    fn from_the_demo_keeps_a_saved_spray_and_else_falls_back() {
        let t = fixture();
        // "Own Spray" is listed with "From the demo".
        assert_eq!(t.choose(false, "own spray", 0), Choice::Own);
        assert_eq!(t.resolve(Choice::Own, true, false), Choice::Game);
        // None saved: the fallback for the demo type.
        assert_eq!(t.resolve(Choice::Own, false, false), Choice::Blank);
        assert_eq!(
            t.resolve(Choice::Own, false, true),
            Choice::Png("team_allies.png".into())
        );
        // A POV fallback of "Their own spray" with none saved is no spray,
        // never the bullet hole -- listed or not.
        let own = parse("spray_pov_fallback\tgame\nspray\tname\tx\town\n");
        assert_eq!(
            own.resolve(own.choose(false, "x", 0), false, false),
            Choice::Blank
        );
        assert_eq!(
            own.resolve(own.choose(false, "y", 0), false, false),
            Choice::Blank
        );
        assert_eq!(
            own.resolve(own.choose(false, "y", 0), true, false),
            Choice::Game
        );
        // A PNG or no spray is that, saved or not.
        let png = Choice::Png("a.png".into());
        assert_eq!(t.resolve(png.clone(), true, false), png);
        assert_eq!(t.resolve(Choice::Blank, true, true), Choice::Blank);
    }

    #[test]
    fn everyone_else_gets_the_spray_fallback_for_the_demo_type() {
        let t = fixture();
        assert_eq!(t.choose(false, "stranger", 0), Choice::Blank);
        assert_eq!(
            t.choose(true, "stranger", 0),
            Choice::Png("team_allies.png".into())
        );
        let defaults = parse("spray_pov_fallback\tgame\nspray_hltv_fallback\tgame\n");
        assert!(!defaults.active());
        assert_eq!(defaults.choose(false, "x", 0), Choice::Game);
        assert_eq!(defaults.choose(true, "x", 0), Choice::Game);
        let none = parse("spray_hltv_fallback\tnone\n");
        assert!(none.active());
        assert_eq!(none.choose(true, "x", 0), Choice::Blank);
        assert_eq!(none.choose(false, "x", 0), Choice::Game);
        assert_eq!(parse(""), Table::default());
    }

    #[test]
    fn bad_lines_are_counted_not_used() {
        let t = parse(
            "// comment\n\
             spray_pov_fallback\tsometimes\n\
             spray_hltv_fallback\tpng\t../up.png\n\
             spray\tsteamid\t12\ta.png\n\
             spray\tname\t\ta.png\n\
             spray\tname\tx\tnot-a-png\n\
             spray\tname\tx\ta.png\textra\n\
             something else\n\
             crosshair\tname\tx\ta.spr\t17\n\
             crosshair\tname\tx\t../a.spr\t1\n\
             crosshair_fallback\tsprite\ta.spr\n\
             spray\tname\tok\tok.png\n\
             crosshair\tname\tok\tok.spr\t1\n",
        );
        assert_eq!(t.bad, vec![2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
        assert_eq!(t.players.len(), 1);
        assert_eq!(t.crosshairs.len(), 1);
        assert_eq!(t.pov, Choice::Game);
        assert_eq!(t.crosshair, Crosshair::Game);
        // No size line: a built spray is 64, like a typical saved one.
        assert_eq!(t.spray_side(), DEFAULT_SPRAY_SIZE);
        for bad in [
            "spray_size\t100\n",
            "spray_size\t512\n",
            "spray_size\tbig\n",
        ] {
            let t = parse(bad);
            assert_eq!(t.bad, vec![1], "{bad:?}");
            assert_eq!(t.spray_side(), 64);
        }
        assert_eq!(parse("spray_size\t128\n").spray_side(), 128);
    }
}
