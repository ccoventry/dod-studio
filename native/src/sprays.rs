//! Configuration > Sprays (#428): which spray each player gets in a demo,
//! and what everyone not listed gets.
//!
//! Saved in settings.json as [`SprayTable`], and mirrored for the hook DLL to
//! `%APPDATA%\dod-studio\goldsrc_hooks_sprays.txt` ([`table_text`]) whenever
//! it changes. The hook reads that file (`goldsrc-hooks/src/spray_table.rs`)
//! rather than settings.json so its parser stays a few lines of tab-separated
//! text, and so a settings file it can't read never touches the sprays. Both
//! sides test against `test-fixtures/sprays_table.txt`, so the format can't
//! drift between the writer and the reader. Studio never writes into the game
//! install for this: the PNGs are the user's own, in `<game>\logos\`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The file the hook reads, in Studio's app-data folder.
pub const TABLE_FILE: &str = "goldsrc_hooks_sprays.txt";

/// One row: a player and the PNG (a file name in `<game>\logos\`) he gets.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SprayPlayer {
    /// SteamID64 as digits, or empty when only the name is known (a bot, a
    /// player the demo carries no SteamID for, or one typed by name).
    #[serde(default)]
    pub steam_id: String,
    /// The name to match when there is no SteamID, and the label otherwise.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub png: String,
}

/// What a player not in the table gets.
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SprayTable {
    #[serde(default)]
    pub players: Vec<SprayPlayer>,
    /// POV demos: `own` by default.
    #[serde(default)]
    pub pov_fallback: SprayFallback,
    #[serde(default)]
    pub pov_fallback_png: String,
    /// HLTV demos: `png` by default. `own` is never offered (an HLTV demo
    /// records that a player sprayed, never the image) and reads as the
    /// game's bullet hole, like `png` with no PNG picked.
    #[serde(default = "default_hltv_fallback")]
    pub hltv_fallback: SprayFallback,
    #[serde(default)]
    pub hltv_fallback_png: String,
}

fn default_hltv_fallback() -> SprayFallback {
    SprayFallback::Png
}

impl Default for SprayTable {
    fn default() -> Self {
        Self {
            players: Vec::new(),
            pov_fallback: SprayFallback::Own,
            pov_fallback_png: String::new(),
            hltv_fallback: default_hltv_fallback(),
            hltv_fallback_png: String::new(),
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

/// A PNG name the hook accepts: a file name inside `logos\`, never a path.
pub fn is_png_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && !name.contains(['/', '\\', ':', '\t'])
        && name.to_ascii_lowercase().ends_with(".png")
}

/// `game`, `none` or `png <file>`; a PNG fallback with no usable file is
/// the game's own.
fn fallback_cells(kind: SprayFallback, png: &str) -> String {
    match kind {
        SprayFallback::None => "none".to_string(),
        SprayFallback::Png if is_png_name(png) => format!("png\t{}", cell(png)),
        _ => "game".to_string(),
    }
}

/// The hook's file. Lines are tab-separated:
///
/// - `pov_fallback` / `hltv_fallback`, then `game`, `none` or `png <file>`;
/// - `player steamid <SteamID64> <file> <name>`, matched by SteamID (and
///   by name only when the game doesn't know the player's SteamID);
/// - `player name <name> <file>`, matched by name, case-insensitively.
///
/// Rows without a usable PNG or anyone to match are left out.
pub fn table_text(table: &SprayTable) -> String {
    let mut out = String::from(
        "// DoD Studio's Sprays table (Configuration > Sprays), for the hook DLL (#428).\n\
         // Studio rewrites this file whenever the table changes: edit it there.\n",
    );
    out.push_str(&format!(
        "pov_fallback\t{}\n",
        fallback_cells(table.pov_fallback, &table.pov_fallback_png)
    ));
    out.push_str(&format!(
        "hltv_fallback\t{}\n",
        fallback_cells(table.hltv_fallback, &table.hltv_fallback_png)
    ));
    for p in &table.players {
        if !is_png_name(&p.png) {
            continue;
        }
        let (id, name, png) = (cell(&p.steam_id), cell(&p.name), cell(&p.png));
        if !id.is_empty() && id.len() == 17 && id.bytes().all(|b| b.is_ascii_digit()) {
            out.push_str(&format!("player\tsteamid\t{id}\t{png}\t{name}\n"));
        } else if !name.is_empty() {
            out.push_str(&format!("player\tname\t{name}\t{png}\n"));
        }
    }
    out
}

pub fn table_path() -> PathBuf {
    crate::shared::paths::get_appdata_dir().join(TABLE_FILE)
}

/// Writes the hook's file when its text changed. The hook rereads it when
/// its modified time moves, so an unchanged table isn't written again.
pub fn write_table_at(path: &Path, table: &SprayTable) -> Result<bool, String> {
    let text = table_text(table);
    if std::fs::read_to_string(path).is_ok_and(|old| old == text) {
        return Ok(false);
    }
    std::fs::write(path, text).map_err(|e| {
        format!(
            "could not write the Sprays table to {}: {e}",
            path.display()
        )
    })?;
    Ok(true)
}

pub fn write_table(table: &SprayTable) -> Result<bool, String> {
    write_table_at(&table_path(), table)
}

/// The PNGs in `<game>\logos\`, sorted case-insensitively, for the table's
/// dropdowns. Empty when the folder isn't there.
pub fn list_logo_pngs(game_dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(game_dir.join("logos"))
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .filter_map(|e| e.file_name().to_str().map(str::to_string))
                .filter(|n| is_png_name(n))
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|n| n.to_ascii_lowercase());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table `test-fixtures/sprays_table.txt` holds; the hook's parser
    /// tests read the same file (`spray_table.rs`).
    fn fixture_table() -> SprayTable {
        SprayTable {
            players: vec![
                SprayPlayer {
                    steam_id: "76561197972576011".into(),
                    name: "dicE[: :]m00cat :D".into(),
                    png: "chris_teamwork.png".into(),
                },
                SprayPlayer {
                    steam_id: String::new(),
                    name: "Cpl. Preddy".into(),
                    png: "bot spray.png".into(),
                },
                // No PNG: left out.
                SprayPlayer {
                    steam_id: "76561197960265729".into(),
                    name: "nobody".into(),
                    png: String::new(),
                },
                // A path, not a file name: left out.
                SprayPlayer {
                    steam_id: String::new(),
                    name: "escape".into(),
                    png: "../x.png".into(),
                },
            ],
            pov_fallback: SprayFallback::None,
            pov_fallback_png: "ignored.png".into(),
            hltv_fallback: SprayFallback::Png,
            hltv_fallback_png: "team_allies.png".into(),
        }
    }

    #[test]
    fn the_file_is_the_shared_fixture() {
        let fixture = include_str!("../../test-fixtures/sprays_table.txt").replace("\r\n", "\n");
        assert_eq!(table_text(&fixture_table()), fixture);
    }

    #[test]
    fn fallbacks_without_a_png_are_the_games_own() {
        let table = SprayTable::default();
        let text = table_text(&table);
        assert!(text.contains("pov_fallback\tgame\n"), "{text}");
        // The HLTV default is a fallback PNG, but none is picked yet.
        assert_eq!(table.hltv_fallback, SprayFallback::Png);
        assert!(text.contains("hltv_fallback\tgame\n"), "{text}");
        let own = SprayTable {
            hltv_fallback: SprayFallback::Own,
            ..SprayTable::default()
        };
        assert!(table_text(&own).contains("hltv_fallback\tgame\n"));
    }

    #[test]
    fn cells_never_split_a_line() {
        let table = SprayTable {
            players: vec![SprayPlayer {
                steam_id: String::new(),
                name: "tab\there\nnewline".into(),
                png: "a.png".into(),
            }],
            ..SprayTable::default()
        };
        assert!(table_text(&table).contains("player\tname\ttab here newline\ta.png\n"));
    }

    #[test]
    fn a_settings_file_without_the_table_loads_the_defaults() {
        let table: SprayTable = serde_json::from_str("{}").unwrap();
        assert_eq!(table, SprayTable::default());
        let partial: SprayTable =
            serde_json::from_str(r#"{"pov_fallback":"png","pov_fallback_png":"x.png"}"#).unwrap();
        assert_eq!(partial.pov_fallback, SprayFallback::Png);
        assert_eq!(partial.hltv_fallback, SprayFallback::Png);
    }

    #[test]
    fn written_only_when_the_text_changes() {
        let dir = crate::test_support::Scratch::new("sprays_table");
        let path = dir.join(TABLE_FILE);
        assert!(write_table_at(&path, &fixture_table()).unwrap());
        assert!(!write_table_at(&path, &fixture_table()).unwrap());
        assert!(write_table_at(&path, &SprayTable::default()).unwrap());
    }

    #[test]
    fn lists_only_png_files_in_logos() {
        let dir = crate::test_support::Scratch::new("sprays_logos");
        let logos = dir.join("logos");
        std::fs::create_dir_all(logos.join("sub.png")).unwrap();
        for name in ["b.PNG", "a.png", "tempdecal.wad", "notes.txt"] {
            std::fs::write(logos.join(name), b"x").unwrap();
        }
        assert_eq!(list_logo_pngs(&dir), vec!["a.png", "b.PNG"]);
        assert!(list_logo_pngs(&dir.join("missing")).is_empty());
    }
}
