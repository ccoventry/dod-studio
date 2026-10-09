//! Who made the map, for the overview's title card (#580).
//!
//! Where a credit comes from, best first (a survey of 156 maps is on #580):
//! - the map's own `maps/<map>.txt`, whose first line on Valve's maps is
//!   `DOD_ANZIO by <author>`;
//! - worldspawn's `mapper` key (rare), then its `message` when it says "by".
//!
//! Only the "by ..." part is kept, and anything that looks like an email
//! address or a web address is dropped: those are personal, and a title card
//! is no place for them. The WAD paths in a BSP are never read, for the same
//! reason (they leak the mapper's own folders).

use std::path::Path;

use crate::patch::bsp_entities::MapEntity;

/// The longest credit kept, in characters.
const MAX_CHARS: usize = 60;
/// How many non-empty lines at the top of a `.txt` are searched.
const TOP_LINES: usize = 5;
/// The mod folders a map's `.txt` can sit in, in the engine's search order.
const MOD_DIRS: [&str; 3] = ["dod_addon", "dod", "dod_downloads"];

/// The credit for `map` in `install`, or `None` when nothing names anyone.
pub fn map_credit(install: &Path, map: &str, entities: &[MapEntity]) -> Option<String> {
    MOD_DIRS
        .iter()
        .filter_map(|dir| {
            std::fs::read(install.join(dir).join("maps").join(format!("{map}.txt"))).ok()
        })
        .find_map(|bytes| credit_from_text(&decode(&bytes)))
        .or_else(|| credit_from_entities(entities))
}

/// A map's `.txt` as text: UTF-8 when it is, else Latin-1, which is what
/// older ones were written in (a French credit's accents survive either way).
fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    }
}

/// "by ..." from the first few lines of a map's `.txt`.
pub fn credit_from_text(text: &str) -> Option<String> {
    text.lines()
        .map(|l| l.trim().trim_start_matches('/').trim())
        .filter(|l| !l.is_empty())
        .take(TOP_LINES)
        .find_map(by_part)
}

/// worldspawn's `mapper`, else its `message` when it says "by".
pub fn credit_from_entities(entities: &[MapEntity]) -> Option<String> {
    let world = entities.iter().find(|e| e.classname() == "worldspawn")?;
    if let Some(mapper) = world.get("mapper").map(clean).filter(|m| !m.is_empty()) {
        return Some(limit(format!("by {mapper}")));
    }
    world.get("message").and_then(by_part)
}

/// The text from a standalone "by" (any case) to the end of the line, made
/// tidy: "by" in lower case, email and web addresses dropped. `None` when
/// there is no "by" or nothing after it.
fn by_part(line: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    let at = lower.match_indices("by").map(|(i, _)| i).find(|&i| {
        let before = lower[..i].chars().next_back();
        let after = lower[i + 2..].chars().next();
        before.is_none_or(|c| !c.is_alphanumeric()) && after.is_some_and(char::is_whitespace)
    })?;
    let name = clean(&line[at + 2..]);
    (!name.is_empty()).then(|| limit(format!("by {name}")))
}

/// Drops email and web addresses (and brackets left empty by that), then
/// spaces and punctuation left dangling.
fn clean(text: &str) -> String {
    let personal =
        |w: &str| w.contains('@') || w.contains("://") || w.to_ascii_lowercase().contains("www.");
    let mut out = String::new();
    for word in text.split_whitespace() {
        if personal(word) {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    for empty in ["()", "[]", "<>", "{}"] {
        out = out.replace(empty, "");
    }
    out.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|c: char| c.is_whitespace() || ",;:-(".contains(c))
        .to_string()
}

fn limit(text: String) -> String {
    if text.chars().count() <= MAX_CHARS {
        return text;
    }
    let cut: String = text.chars().take(MAX_CHARS - 1).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world(pairs: &[(&str, &str)]) -> Vec<MapEntity> {
        let mut pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        pairs.push(("classname".into(), "worldspawn".into()));
        vec![MapEntity { pairs }]
    }

    #[test]
    fn reads_valves_first_line_without_the_email() {
        let anzio = "DOD_ANZIO by Davide \"Chow_Yun_Fat\" Pernigo, (bido@halflifeitalia.com)\n\nJanuary, 1944, Italy";
        assert_eq!(
            credit_from_text(anzio).as_deref(),
            Some("by Davide \"Chow_Yun_Fat\" Pernigo")
        );
        let avalanche = "DOD_AVALANCHE by Iikka \"Fingers\" Keranen\n\nSeptember, 1943";
        assert_eq!(
            credit_from_text(avalanche).as_deref(),
            Some("by Iikka \"Fingers\" Keranen")
        );
    }

    #[test]
    fn reads_a_comment_line_and_other_layouts() {
        let lennon = "// overview description file for dod_lennon_test.bmp\n// Map by Lerf; Tox\n\nglobal\n{";
        assert_eq!(credit_from_text(lennon).as_deref(), Some("by Lerf; Tox"));
        assert_eq!(
            credit_from_text("Modified for GG BY LEE.Xr").as_deref(),
            Some("by LEE.Xr")
        );
        // "by" inside a word, or at the end of a line, is no credit.
        assert_eq!(credit_from_text("Standby for orders\nnearby"), None);
        // Only the top of the file is read.
        assert_eq!(credit_from_text("a\nb\nc\nd\ne\nmade by someone"), None);
    }

    #[test]
    fn drops_web_addresses_and_keeps_it_short() {
        assert_eq!(
            credit_from_text("DOD_X by Bob (www.example.com) http://x.org").as_deref(),
            Some("by Bob")
        );
        let long = format!("by {}", "a".repeat(100));
        let got = credit_from_text(&long).unwrap();
        assert_eq!(got.chars().count(), MAX_CHARS);
        assert!(got.ends_with('…'));
    }

    #[test]
    fn reads_a_latin1_file() {
        assert_eq!(
            decode(b"by Les v\xe9t\xe9rans"),
            "by Les v\u{e9}t\u{e9}rans"
        );
        assert_eq!(decode("by Ren\u{e9}".as_bytes()), "by Ren\u{e9}");
    }

    #[test]
    fn falls_back_to_worldspawn() {
        assert_eq!(
            credit_from_entities(&world(&[("mapper", "Tiger Team")])).as_deref(),
            Some("by Tiger Team")
        );
        assert_eq!(
            credit_from_entities(&world(&[("message", "DUST by Dave Johnston")])).as_deref(),
            Some("by Dave Johnston")
        );
        assert_eq!(
            credit_from_entities(&world(&[("message", "dod_dust")])),
            None
        );
        assert_eq!(credit_from_entities(&[]), None);
    }

    #[test]
    fn the_txt_wins_over_worldspawn() {
        let dir = crate::test_support::Scratch::new("overview_credit");
        std::fs::create_dir_all(dir.join("dod").join("maps")).unwrap();
        std::fs::write(dir.join("dod/maps/dod_x.txt"), "DOD_X by Txt Author").unwrap();
        let entities = world(&[("mapper", "World Author")]);
        assert_eq!(
            map_credit(&dir, "dod_x", &entities).as_deref(),
            Some("by Txt Author")
        );
        assert_eq!(
            map_credit(&dir, "dod_y", &entities).as_deref(),
            Some("by World Author")
        );
    }
}
