//! Studio's Sprays table (Configuration > Sprays, #428), as the hook reads it.
//!
//! Studio writes `%APPDATA%\dod-studio\goldsrc_hooks_sprays.txt` whenever the
//! table changes (`native::sprays::table_text`); this parses it, and decides
//! what one player's spray becomes in a demo. The format is in that writer's
//! doc, and both sides test against `test-fixtures/sprays_table.txt`. Pure,
//! so the rules are tested without a game.
//!
//! The rule, the same for both demo types: a player in the table gets his
//! PNG; anyone else gets the fallback for the demo's type -- POV: the game's
//! own (the spray the demo saved), one PNG, or none; HLTV: one PNG or none
//! (`game` there is the engine's bullet hole, which is what an HLTV demo
//! draws with no image). Who the camera follows plays no part.

use std::path::PathBuf;

/// The file's name in Studio's app-data folder.
pub const FILE_NAME: &str = "goldsrc_hooks_sprays.txt";

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
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Choice::Game => f.write_str("the game's own"),
            Choice::Blank => f.write_str("no spray"),
            Choice::Png(png) => f.write_str(png),
        }
    }
}

/// One row of the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// SteamID64, or 0 for a row matched by name only.
    pub steam_id: u64,
    pub name: String,
    pub png: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Table {
    pub players: Vec<Row>,
    pub pov: Choice,
    pub hltv: Choice,
    /// Lines that weren't a comment, blank, or one of the shapes, by number.
    pub bad: Vec<usize>,
}

/// A PNG name: a file name inside `logos\`, never a path.
fn png_name(raw: &str) -> Option<String> {
    let name = raw.trim();
    (!name.is_empty()
        && !name.contains(['/', '\\', ':'])
        && name.to_ascii_lowercase().ends_with(".png"))
    .then(|| name.to_string())
}

fn fallback(cells: &[&str]) -> Option<Choice> {
    match cells {
        ["game" | "own"] => Some(Choice::Game),
        ["none"] => Some(Choice::Blank),
        ["png", file] => png_name(file).map(Choice::Png),
        _ => None,
    }
}

/// A 17-digit SteamID64 (the form the file holds).
fn steam_id64(text: &str) -> Option<u64> {
    const BASE: u64 = 76561197960265728;
    let text = text.trim();
    if text.len() != 17 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok().filter(|&id| id > BASE)
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
            [which @ ("pov_fallback" | "hltv_fallback"), rest @ ..] => match fallback(rest) {
                Some(choice) if *which == "pov_fallback" => {
                    table.pov = choice;
                    true
                }
                Some(choice) => {
                    table.hltv = choice;
                    true
                }
                None => false,
            },
            ["player", "steamid", id, png, rest @ ..] if rest.len() <= 1 => {
                match (steam_id64(id), png_name(png)) {
                    (Some(steam_id), Some(png)) => {
                        let name = rest.first().map_or(String::new(), |n| n.to_string());
                        table.players.push(Row {
                            steam_id,
                            name,
                            png,
                        });
                        true
                    }
                    _ => false,
                }
            }
            ["player", "name", name, png] if !name.is_empty() => match png_name(png) {
                Some(png) => {
                    table.players.push(Row {
                        steam_id: 0,
                        name: name.to_string(),
                        png,
                    });
                    true
                }
                None => false,
            },
            _ => false,
        };
        if !ok {
            table.bad.push(i + 1);
        }
    }
    table
}

impl Table {
    /// Whether the table asks for anything the game wouldn't do by itself.
    /// Until it does, the hook installs nothing for it.
    pub fn active(&self) -> bool {
        !self.players.is_empty() || self.pov != Choice::Game || self.hltv != Choice::Game
    }

    /// The row for a player: by SteamID first; then by name, ignoring case,
    /// among the rows with no SteamID -- and the SteamID rows too when the
    /// game doesn't know this player's SteamID (`steam_id` 0), since a known
    /// but different SteamID is a different person who took the name.
    pub fn row_for(&self, name: &str, steam_id: u64) -> Option<&Row> {
        if steam_id != 0
            && let Some(row) = self.players.iter().find(|r| r.steam_id == steam_id)
        {
            return Some(row);
        }
        let name = name.trim().to_lowercase();
        if name.is_empty() {
            return None;
        }
        self.players
            .iter()
            .find(|r| (r.steam_id == 0 || steam_id == 0) && r.name.trim().to_lowercase() == name)
    }

    /// What this player's spray becomes in a demo of this type.
    pub fn choose(&self, hltv: bool, name: &str, steam_id: u64) -> Choice {
        match self.row_for(name, steam_id) {
            Some(row) => Choice::Png(row.png.clone()),
            None if hltv => self.hltv.clone(),
            None => self.pov.clone(),
        }
    }

    /// Every PNG the table names, each once, in file order.
    pub fn pngs(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let fallbacks = [&self.pov, &self.hltv];
        let named =
            self.players
                .iter()
                .map(|r| r.png.as_str())
                .chain(fallbacks.into_iter().filter_map(|c| match c {
                    Choice::Png(p) => Some(p.as_str()),
                    _ => None,
                }));
        for png in named {
            if !out.iter().any(|p| p.eq_ignore_ascii_case(png)) {
                out.push(png.to_string());
            }
        }
        out
    }
}

/// `%APPDATA%\dod-studio\goldsrc_hooks_sprays.txt`, where Studio keeps the
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

/// The player slot (0-based, `cl.players`' index) a custom spray's texture
/// is named after: `{` or `}`, then `%03i%02i` of the slot and the lump.
/// `None` for anything else, and for the `-2` slot the engine validates a
/// spray under when it connects.
pub fn slot_of_custom_name(identifier: &str) -> Option<usize> {
    let b = identifier.as_bytes();
    if b.len() != 6 || !(b[0] == b'{' || b[0] == b'}') || !b[1..].iter().all(u8::is_ascii_digit) {
        return None;
    }
    identifier[1..4]
        .parse()
        .ok()
        .filter(|&slot: &usize| slot < 32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Table {
        parse(include_str!("../../test-fixtures/sprays_table.txt"))
    }

    /// The file `native::sprays::table_text` writes (its test checks it
    /// writes exactly this) reads back as the table Studio saved.
    #[test]
    fn reads_the_shared_fixture() {
        let t = fixture();
        assert!(t.bad.is_empty(), "{:?}", t.bad);
        assert_eq!(t.pov, Choice::Blank);
        assert_eq!(t.hltv, Choice::Png("team_allies.png".into()));
        assert_eq!(
            t.players,
            vec![
                Row {
                    steam_id: 76561197972576011,
                    name: "dicE[: :]m00cat :D".into(),
                    png: "chris_teamwork.png".into(),
                },
                Row {
                    steam_id: 0,
                    name: "Cpl. Preddy".into(),
                    png: "bot spray.png".into(),
                },
            ]
        );
        assert!(t.active());
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
        assert_eq!(t.choose(false, "someone else", 76561197972576011), m00cat);
        // By name when the game doesn't know his SteamID.
        assert_eq!(t.choose(true, "DICE[: :]M00CAT :d", 0), m00cat);
        // A known, different SteamID with his name is someone else.
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
    fn everyone_else_gets_the_fallback_for_the_demo_type() {
        let t = fixture();
        assert_eq!(t.choose(false, "stranger", 0), Choice::Blank);
        assert_eq!(
            t.choose(true, "stranger", 0),
            Choice::Png("team_allies.png".into())
        );
        let defaults = parse("pov_fallback\tgame\nhltv_fallback\tgame\n");
        assert!(!defaults.active());
        assert_eq!(defaults.choose(false, "x", 0), Choice::Game);
        assert_eq!(defaults.choose(true, "x", 0), Choice::Game);
        let none = parse("hltv_fallback\tnone\n");
        assert!(none.active());
        assert_eq!(none.choose(true, "x", 0), Choice::Blank);
        assert_eq!(none.choose(false, "x", 0), Choice::Game);
        assert_eq!(parse(""), Table::default());
    }

    #[test]
    fn bad_lines_are_counted_not_used() {
        let t = parse(
            "// comment\n\
             pov_fallback\tsometimes\n\
             hltv_fallback\tpng\t../up.png\n\
             player\tsteamid\t12\ta.png\n\
             player\tname\t\ta.png\n\
             player\tname\tx\tnot-a-png\n\
             player\tname\tx\ta.png\textra\n\
             something else\n\
             player\tname\tok\tok.png\n",
        );
        assert_eq!(t.bad, vec![2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(t.players.len(), 1);
        assert_eq!(t.pov, Choice::Game);
    }

    #[test]
    fn custom_spray_names_give_the_slot() {
        assert_eq!(slot_of_custom_name("{00300"), Some(3));
        assert_eq!(slot_of_custom_name("}03101"), Some(31));
        assert_eq!(slot_of_custom_name("{-0200"), None);
        assert_eq!(slot_of_custom_name("{03200"), None);
        assert_eq!(slot_of_custom_name("{shot5"), None);
        assert_eq!(slot_of_custom_name("{hltvspray0001"), None);
    }
}
