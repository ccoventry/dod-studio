//! Configuration › Paths, "Size the Game's Menus Like the Pre-Anniversary
//! Engine" (#985).
//!
//! The 25th Anniversary engine draws GameUI's windows (main menu, Options,
//! console, server browser, DoD Studio's in-game window) "HD proportional":
//! every layout size and font is multiplied by screen height /
//! `ProportionalBaseHeightHD` (720 in Valve's `TrackerScheme.res`), so 2x at
//! 1440. The engine clamps that base to the screen height, so a base at or
//! above any real screen gives 1x: the sizes the pre-Anniversary engine uses.
//!
//! Turned on, this writes a copy of the scheme the game would load (the game
//! folder's own `resource\TrackerScheme.res`, else `valve`'s) with only the
//! two `ProportionalBase*HD` values raised, into `<game>_addon\resource\`.
//! The engine reads it there only with `-addons`, which Studio's launches
//! pass. The game's own files are never written (CLAUDE.md: the game's `.res`
//! files are the user's), and a `TrackerScheme.res` in the addon folder that
//! isn't ours is left alone. Turned off, our copy is removed.
//!
//! Anniversary installs only: the pre-Anniversary engine has no HD scaling,
//! and a copy of `valve`'s scheme would replace PRE's own `dod` one (custom
//! fonts) for nothing.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// The scheme file GameUI loads from `resource\`.
pub const FILE: &str = "TrackerScheme.res";
/// First line of our copy: marks it as DoD Studio's, to update or remove.
pub const MARK: &str = "// DoD Studio: menus sized like the pre-Anniversary engine (#985).";
/// The HD base written: 8K, above any screen, so the scale is always 1x.
pub const BASE_WIDTH_HD: u32 = 7680;
pub const BASE_HEIGHT_HD: u32 = 4320;

const WIDTH_KEY: &str = "ProportionalBaseWidthHD";
const HEIGHT_KEY: &str = "ProportionalBaseHeightHD";

/// What Configuration shows for the selected install.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MenuScaleStatus {
    /// The install runs the 25th Anniversary engine, whose menus scale up.
    pub applies: bool,
    /// Our scheme copy is in the addon folder.
    pub on: bool,
    /// A `TrackerScheme.res` that isn't ours is in the addon folder; it is
    /// never touched, so the setting can't be turned on.
    pub foreign: bool,
    /// `<game>_addon\resource\TrackerScheme.res`.
    pub path: String,
}

/// Text decoded as UTF-8, or byte for byte when it isn't, so a scheme with a
/// stray Latin-1 byte round-trips unchanged.
fn decode(bytes: &[u8]) -> (String, bool) {
    match std::str::from_utf8(bytes) {
        Ok(text) => (text.to_string(), true),
        Err(_) => (bytes.iter().map(|&b| b as char).collect(), false),
    }
}

fn encode(text: &str, utf8: bool) -> Vec<u8> {
    if utf8 {
        text.as_bytes().to_vec()
    } else {
        text.chars().map(|c| c as u32 as u8).collect()
    }
}

/// The braces in `line` outside quotes and `//` comments: (opens, closes).
fn braces(line: &str) -> (usize, usize) {
    let (mut open, mut close, mut quoted) = (0, 0, false);
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            '/' if !quoted && chars.peek() == Some(&'/') => break,
            '{' if !quoted => open += 1,
            '}' if !quoted => close += 1,
            _ => {}
        }
    }
    (open, close)
}

/// The first token of `line`, quoted or bare, without its quotes.
fn first_token(line: &str) -> &str {
    let line = line.trim_start();
    if let Some(rest) = line.strip_prefix('"') {
        rest.split('"').next().unwrap_or("")
    } else {
        line.split(|c: char| c.is_whitespace() || c == '{' || c == '"')
            .next()
            .unwrap_or("")
    }
}

fn base_line(indent: &str, key: &str, value: u32) -> String {
    format!("{indent}\"{key}\" \"{value}\"\t// DoD Studio (#985): 1x at any screen height")
}

/// `scheme` with `ProportionalBaseWidthHD`/`HeightHD` in its `BaseSettings`
/// block set to [`BASE_WIDTH_HD`] x [`BASE_HEIGHT_HD`] (added at the block's
/// end when missing), and [`MARK`] as its first line. Everything else is
/// kept as it was, line endings too. `None` when there's no `BaseSettings`
/// block to put them in.
pub fn pre_sized(scheme: &str) -> Option<String> {
    let newline = if scheme.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut out: Vec<String> = Vec::new();
    // Depth of the braces, and the depth of BaseSettings' own block once in it.
    let mut depth = 0usize;
    let mut block: Option<usize> = None;
    let mut awaiting_open = false;
    let (mut width_done, mut height_done, mut found) = (false, false, false);
    let mut indent = String::from("\t\t");
    for line in scheme.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let (open, close) = braces(line);
        let token = first_token(line);
        let mut replaced = None;
        if let Some(inner) = block {
            if depth == inner && (token == WIDTH_KEY || token == HEIGHT_KEY) {
                let lead: String = line.chars().take_while(|c| c.is_whitespace()).collect();
                indent = lead.clone();
                if token == WIDTH_KEY {
                    width_done = true;
                    replaced = Some(base_line(&lead, WIDTH_KEY, BASE_WIDTH_HD));
                } else {
                    height_done = true;
                    replaced = Some(base_line(&lead, HEIGHT_KEY, BASE_HEIGHT_HD));
                }
            } else if depth == inner && close > open && depth - (close - open) < inner {
                // The block's closing brace: add whatever was missing first.
                if !width_done {
                    out.push(base_line(&indent, WIDTH_KEY, BASE_WIDTH_HD));
                }
                if !height_done {
                    out.push(base_line(&indent, HEIGHT_KEY, BASE_HEIGHT_HD));
                }
                block = None;
            }
        } else if !found && token.eq_ignore_ascii_case("BaseSettings") {
            found = true;
            awaiting_open = true;
        }
        if awaiting_open && open > 0 {
            block = Some(depth + 1);
            awaiting_open = false;
        }
        depth = (depth + open).saturating_sub(close);
        out.push(replaced.unwrap_or_else(|| line.to_string()));
    }
    if !found || block.is_some() || awaiting_open {
        return None;
    }
    let mut text = String::from(MARK);
    text.push_str(newline);
    text.push_str(&out.join(newline));
    if scheme.ends_with('\n') {
        text.push_str(newline);
    }
    Some(text)
}

/// The scheme the game loads without our copy: `<game>\resource\`, else
/// `valve\resource\`. `game_dir` is `<install>\<game folder>`.
pub fn source_scheme(game_dir: &Path) -> Option<PathBuf> {
    let own = game_dir.join("resource").join(FILE);
    if own.is_file() {
        return Some(own);
    }
    let valve = game_dir.parent()?.join("valve").join("resource").join(FILE);
    valve.is_file().then_some(valve)
}

/// `<game>_addon\resource\TrackerScheme.res`.
pub fn addon_scheme(game_dir: &Path) -> PathBuf {
    crate::player_overrides::addon_of(game_dir)
        .join("resource")
        .join(FILE)
}

fn is_ours(path: &Path) -> Option<bool> {
    let bytes = std::fs::read(path).ok()?;
    Some(decode(&bytes).0.starts_with(MARK))
}

fn status_in(game_dir: &Path, applies: bool) -> MenuScaleStatus {
    let path = addon_scheme(game_dir);
    let ours = is_ours(&path);
    MenuScaleStatus {
        applies,
        on: ours == Some(true),
        foreign: ours == Some(false),
        path: path.display().to_string(),
    }
}

/// Writes or removes our copy for `game_dir`, whatever its engine (the build
/// check is [`set`]'s). Writing again when the copy is current changes nothing
/// on disk.
pub fn set_in(game_dir: &Path, on: bool, applies: bool) -> Result<MenuScaleStatus, String> {
    let path = addon_scheme(game_dir);
    match is_ours(&path) {
        Some(false) => {
            return Err(format!(
                "{} is not DoD Studio's, so it was left alone; move it away to use this setting",
                path.display()
            ));
        }
        Some(true) if !on => {
            std::fs::remove_file(&path)
                .map_err(|e| format!("could not remove {}: {e}", path.display()))?;
        }
        _ if on => {
            let source = source_scheme(game_dir).ok_or_else(|| {
                format!(
                    "found no {FILE} to copy in {} or valve\\resource",
                    game_dir.join("resource").display()
                )
            })?;
            let bytes = std::fs::read(&source)
                .map_err(|e| format!("could not read {}: {e}", source.display()))?;
            let (text, utf8) = decode(&bytes);
            let sized = pre_sized(&text).ok_or_else(|| {
                format!("{} has no BaseSettings block to change", source.display())
            })?;
            let sized = encode(&sized, utf8);
            if std::fs::read(&path).ok().as_deref() != Some(sized.as_slice()) {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)
                        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
                }
                std::fs::write(&path, sized)
                    .map_err(|e| format!("could not write {}: {e}", path.display()))?;
            }
        }
        _ => {}
    }
    Ok(status_in(game_dir, applies))
}

fn is_anniversary(hl_exe: &Path) -> bool {
    crate::sys::packet_limit::packet_entity_limit(hl_exe)
        == Some(crate::sys::packet_limit::ANNIVERSARY_PACKET_ENTITIES)
}

/// The setting's state for `hl_exe`'s `game_folder`. When our copy is on, it
/// is brought up to date with the game's own scheme (a game update), which
/// writes only when it changed.
pub fn status(hl_exe: &Path, game_dir: &Path) -> MenuScaleStatus {
    let applies = is_anniversary(hl_exe);
    let now = status_in(game_dir, applies);
    if now.on && applies {
        return set_in(game_dir, true, applies).unwrap_or(now);
    }
    now
}

/// Turns the setting on or off. On is refused for a pre-Anniversary install;
/// off always removes our copy.
pub fn set(hl_exe: &Path, game_dir: &Path, on: bool) -> Result<MenuScaleStatus, String> {
    let applies = is_anniversary(hl_exe);
    if on && !applies {
        return Err(format!(
            "{} is not a 25th Anniversary install: its engine already draws the menus at this size",
            hl_exe.display()
        ));
    }
    set_in(game_dir, on, applies)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    const VALVE: &str = "Scheme\r\n{\r\n\tColors\r\n\t{\r\n\t\t\"BaseText\" \"216 222 211 255\"\r\n\t}\r\n\tBaseSettings\r\n\t{\r\n\t\t\"Border.Bright\" \"LightBorder\"\t// {not a brace}\r\n\t\tMenu\r\n\t\t{\r\n\t\t\t\"MenuItemHeight\" \"48\"\r\n\t\t}\r\n\t\t\"ProportionalBaseWidth\" \"640\"\r\n\t\t\"ProportionalBaseWidthHD\" \"1280\"\t\t\r\n\t\t\"ProportionalBaseHeightHD\" \"720\"\r\n\t}\r\n\tFonts\r\n\t{\r\n\t}\r\n}\r\n";

    #[test]
    fn raises_only_the_hd_base() {
        let out = pre_sized(VALVE).unwrap();
        assert!(out.starts_with(MARK));
        assert!(out.contains("\"ProportionalBaseWidthHD\" \"7680\""));
        assert!(out.contains("\"ProportionalBaseHeightHD\" \"4320\""));
        assert!(!out.contains("\"1280\"") && !out.contains("\"720\""));
        // The 640x480 base and everything else stay.
        assert!(out.contains("\"ProportionalBaseWidth\" \"640\""));
        assert!(out.contains("\"MenuItemHeight\" \"48\""));
        // CRLF kept, nothing but the mark and the two lines changed.
        assert!(!out.replace("\r\n", "").contains('\n'));
        assert_eq!(out.lines().count(), VALVE.lines().count() + 1);
    }

    #[test]
    fn adds_the_base_when_missing() {
        // PRE's dod scheme has no HD keys; a game folder may carry one like it.
        let scheme = "Scheme\n{\n\tBaseSettings {\n\t\t\"Menu.TextColor\" \"1 2 3 4\"\n\t}\n\tFonts\n\t{\n\t\tProportionalBaseHeightHD\n\t}\n}\n";
        let out = pre_sized(scheme).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        let close = lines.iter().position(|l| l.trim() == "}").unwrap();
        assert!(lines[close - 2].contains("\"ProportionalBaseWidthHD\" \"7680\""));
        assert!(lines[close - 1].contains("\"ProportionalBaseHeightHD\" \"4320\""));
        // A same-named token outside BaseSettings isn't touched.
        assert!(out.contains("\t\tProportionalBaseHeightHD\n"));
        assert!(out.ends_with("}\n"));
    }

    #[test]
    fn no_base_settings_is_none() {
        assert_eq!(pre_sized("Scheme\n{\n\tColors\n\t{\n\t}\n}\n"), None);
        assert_eq!(pre_sized("Scheme { BaseSettings\n"), None);
    }

    fn install(tag: &str) -> (Scratch, PathBuf) {
        let root = Scratch::new(tag);
        let game = root.path().join("dod");
        std::fs::create_dir_all(&game).unwrap();
        let valve = root.path().join("valve").join("resource");
        std::fs::create_dir_all(&valve).unwrap();
        std::fs::write(valve.join(FILE), VALVE).unwrap();
        (root, game)
    }

    #[test]
    fn on_writes_into_the_addon_folder_and_off_removes_it() {
        let (_root, game) = install("menu_scale_on_off");
        let st = set_in(&game, true, true).unwrap();
        assert!(st.on && !st.foreign);
        let path = addon_scheme(&game);
        assert!(path.ends_with(Path::new("dod_addon").join("resource").join(FILE)));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"4320\""));
        // The game's own files are untouched.
        assert!(!game.join("resource").exists());
        assert_eq!(
            std::fs::read_to_string(game.with_file_name("valve").join("resource").join(FILE))
                .unwrap(),
            VALVE
        );
        // Twice is fine; off removes it.
        assert!(set_in(&game, true, true).unwrap().on);
        let st = set_in(&game, false, true).unwrap();
        assert!(!st.on && !path.exists());
        // Off with nothing there is fine too.
        assert!(!set_in(&game, false, true).unwrap().on);
    }

    #[test]
    fn copies_the_game_folders_own_scheme_first() {
        let (_root, game) = install("menu_scale_own");
        std::fs::create_dir_all(game.join("resource")).unwrap();
        let own = VALVE.replace("216 222 211 255", "1 1 1 255");
        std::fs::write(game.join("resource").join(FILE), &own).unwrap();
        set_in(&game, true, true).unwrap();
        let text = std::fs::read_to_string(addon_scheme(&game)).unwrap();
        assert!(text.contains("1 1 1 255"));
    }

    #[test]
    fn someone_elses_file_is_left_alone() {
        let (_root, game) = install("menu_scale_foreign");
        let path = addon_scheme(&game);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "Scheme { }").unwrap();
        assert!(status_in(&game, true).foreign);
        assert!(set_in(&game, true, true).is_err());
        assert!(set_in(&game, false, true).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "Scheme { }");
    }

    #[test]
    fn a_latin1_byte_round_trips() {
        let (_root, game) = install("menu_scale_latin1");
        let mut bytes = VALVE.as_bytes().to_vec();
        let at = VALVE.find("BaseText").unwrap();
        bytes[at] = 0xe9;
        std::fs::write(
            game.with_file_name("valve").join("resource").join(FILE),
            &bytes,
        )
        .unwrap();
        set_in(&game, true, true).unwrap();
        let out = std::fs::read(addon_scheme(&game)).unwrap();
        assert!(out.contains(&0xe9));
    }
}
