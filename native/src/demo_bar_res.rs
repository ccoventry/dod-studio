//! The demo player bar's layout with DoD Studio's loading controls (#465).
//!
//! While `viewdemo` reads a demo the bar's title says "Loading <demo> ..." and
//! nothing says how far along it is. The hook DLL can fill in a percent label
//! and a progress bar, but only controls the bar's `.res` lays out exist, and
//! the game's own `dod\resource\DemoPlayerDialog.res` is the user's, never
//! written. So every launch copies the user's layout (or the stock `valve`
//! one when `dod` has none) into `dod_addon\resource\`, which `-addons` puts
//! ahead of `dod`, with the two controls added (for another Game folder,
//! `<folder>\resource` into `<folder>_addon`, #402): [`PERCENT_LABEL`] (a `Label`)
//! and [`PROGRESS_BAR`] (a `ProgressBar`), both hidden until the hook shows
//! them (`goldsrc-hooks/src/demo_bar_progress.rs`). They sit where
//! `TimeLabel` is, since the time means nothing while loading.
//!
//! The user's own text is kept byte for byte; the blocks are inserted before
//! the top-level object's closing brace, and a layout that already has them
//! is left as it is.

use std::path::{Path, PathBuf};

/// The `Label` the hook writes `n%` into.
pub const PERCENT_LABEL: &str = "StudioLoadPercent";
/// The `ProgressBar` the hook sets.
pub const PROGRESS_BAR: &str = "StudioLoadProgress";
/// The bar's own time display, whose place the two controls take.
const TIME_LABEL: &str = "TimeLabel";
/// `TimeLabel`'s place in the stock layout, for a layout without one.
const STOCK_TIME_LABEL: Rect = Rect {
    x: 284,
    y: 64,
    wide: 142,
    tall: 24,
};
/// Room for the percent text, which the bar starts after.
const PERCENT_WIDE: i32 = 36;

const FILE_NAME: &str = "DemoPlayerDialog.res";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rect {
    x: i32,
    y: i32,
    wide: i32,
    tall: i32,
}

/// What a launch did with the layout.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Written from `source` to `target`.
    Written { source: PathBuf, target: PathBuf },
    /// The target already holds exactly what would be written.
    Unchanged,
}

/// `res` with the two controls added, or unchanged when it already has
/// them. `None` when it holds no object to add them to.
pub fn with_loading_controls(res: &str) -> Option<String> {
    if res.contains(&format!("\"{PERCENT_LABEL}\"")) {
        return Some(res.to_string());
    }
    let close = res.rfind('}')?;
    let eol = if res.contains("\r\n") { "\r\n" } else { "\n" };
    let place = block(res, TIME_LABEL)
        .and_then(rect_of)
        .unwrap_or(STOCK_TIME_LABEL);
    let mut out = String::with_capacity(res.len() + 1024);
    out.push_str(&res[..close]);
    // The user's text may already end its last block with a newline; a blank
    // line before ours either way keeps the file readable.
    if !out.ends_with('\n') {
        out.push_str(eol);
    }
    out.push_str(&controls(place).replace('\n', eol));
    out.push_str(&res[close..]);
    Some(out)
}

/// The two blocks, as KeyValues text (LF line endings, tab indented like
/// the stock layout).
fn controls(at: Rect) -> String {
    let bar_x = at.x + PERCENT_WIDE;
    let bar_wide = (at.wide - PERCENT_WIDE).max(24);
    let bar_tall = (at.tall / 2).max(8);
    let bar_y = at.y + (at.tall - bar_tall) / 2;
    format!(
        "\t\"{PERCENT_LABEL}\"\n\
         \t{{\n\
         \t\t\"ControlName\"\t\t\"Label\"\n\
         \t\t\"fieldName\"\t\t\"{PERCENT_LABEL}\"\n\
         \t\t\"xpos\"\t\t\"{}\"\n\
         \t\t\"ypos\"\t\t\"{}\"\n\
         \t\t\"wide\"\t\t\"{}\"\n\
         \t\t\"tall\"\t\t\"{}\"\n\
         \t\t\"autoResize\"\t\t\"0\"\n\
         \t\t\"pinCorner\"\t\t\"0\"\n\
         \t\t\"visible\"\t\t\"0\"\n\
         \t\t\"enabled\"\t\t\"1\"\n\
         \t\t\"labelText\"\t\t\"\"\n\
         \t\t\"textAlignment\"\t\t\"west\"\n\
         \t}}\n\
         \t\"{PROGRESS_BAR}\"\n\
         \t{{\n\
         \t\t\"ControlName\"\t\t\"ProgressBar\"\n\
         \t\t\"fieldName\"\t\t\"{PROGRESS_BAR}\"\n\
         \t\t\"xpos\"\t\t\"{bar_x}\"\n\
         \t\t\"ypos\"\t\t\"{bar_y}\"\n\
         \t\t\"wide\"\t\t\"{bar_wide}\"\n\
         \t\t\"tall\"\t\t\"{bar_tall}\"\n\
         \t\t\"autoResize\"\t\t\"0\"\n\
         \t\t\"pinCorner\"\t\t\"0\"\n\
         \t\t\"visible\"\t\t\"0\"\n\
         \t\t\"enabled\"\t\t\"1\"\n\
         \t}}\n",
        at.x, at.y, at.wide, at.tall
    )
}

/// The body of the block named `name` (between its braces), if any.
fn block<'a>(res: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("\"{name}\"");
    let mut from = 0;
    while let Some(at) = res[from..].find(&key) {
        let after = from + at + key.len();
        let rest = res[after..].trim_start();
        if let Some(body) = rest.strip_prefix('{') {
            let end = body.find('}')?;
            return Some(&body[..end]);
        }
        from = after;
    }
    None
}

/// `xpos`, `ypos`, `wide` and `tall` out of a block's body.
fn rect_of(body: &str) -> Option<Rect> {
    let value = |key: &str| -> Option<i32> {
        body.lines().find_map(|line| {
            let mut words = tokens(line);
            (words.next()? == key)
                .then(|| words.next())
                .flatten()?
                .parse()
                .ok()
        })
    };
    Some(Rect {
        x: value("xpos")?,
        y: value("ypos")?,
        wide: value("wide")?,
        tall: value("tall")?,
    })
}

/// A KeyValues line's words: quoted strings whole, the rest split on
/// whitespace.
fn tokens(line: &str) -> impl Iterator<Item = &str> {
    let mut rest = line.trim();
    std::iter::from_fn(move || {
        rest = rest.trim_start();
        if rest.is_empty() || rest.starts_with("//") {
            return None;
        }
        let (word, after) = if let Some(quoted) = rest.strip_prefix('"') {
            let end = quoted.find('"').unwrap_or(quoted.len());
            (&quoted[..end], quoted.get(end + 1..).unwrap_or(""))
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            rest.split_at(end)
        };
        rest = after;
        Some(word)
    })
}

/// The layout a launch reads: the user's own in the game folder's
/// `resource` (`dod` unless the Game folder setting says otherwise, #402),
/// else the stock `valve\resource` one.
fn source_in(game_dir: &Path, game_folder: &str) -> Option<PathBuf> {
    [game_folder, "valve"]
        .into_iter()
        .map(|folder| game_dir.join(folder).join("resource").join(FILE_NAME))
        .find(|path| path.is_file())
}

/// Where the launch writes it: `<game folder>_addon`, the folder `-addons`
/// searches for `-game <game folder>` (`shared::paths::addon_dir`).
fn target_in(game_dir: &Path, game_folder: &str) -> PathBuf {
    game_dir
        .join(format!("{game_folder}_addon"))
        .join("resource")
        .join(FILE_NAME)
}

/// The file's bytes as one char each (Latin-1), so text that is not UTF-8
/// comes back out unchanged: everything searched for and added is ASCII.
fn bytes_as_text(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

/// The inverse of [`bytes_as_text`]; every char it and the added blocks hold
/// is below 256.
fn text_as_bytes(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| u8::try_from(c).unwrap_or(b'?'))
        .collect()
}

/// Writes `<game folder>_addon\resource\DemoPlayerDialog.res` for the game at
/// `game_dir` (the folder `hl.exe` is in). Nothing in the game folder or
/// `valve` is ever written.
pub fn install(game_dir: &Path, game_folder: &str) -> Result<Outcome, String> {
    let source = source_in(game_dir, game_folder).ok_or_else(|| {
        format!(
            "no {FILE_NAME} in {} or {}",
            game_dir.join(game_folder).join("resource").display(),
            game_dir.join("valve").join("resource").display()
        )
    })?;
    let bytes =
        std::fs::read(&source).map_err(|e| format!("could not read {}: {e}", source.display()))?;
    // ASCII blocks spliced into UTF-16 text would break the whole layout.
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        return Err(format!(
            "{} is UTF-16 text, which this can't add to",
            source.display()
        ));
    }
    let with_controls = with_loading_controls(&bytes_as_text(&bytes))
        .map(|text| text_as_bytes(&text))
        .ok_or_else(|| format!("{} holds no layout to add to", source.display()))?;
    let target = target_in(game_dir, game_folder);
    if std::fs::read(&target).is_ok_and(|now| now == with_controls) {
        return Ok(Outcome::Unchanged);
    }
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }
    std::fs::write(&target, &with_controls)
        .map_err(|e| format!("could not write {}: {e}", target.display()))?;
    Ok(Outcome::Written { source, target })
}

/// What every launch does before the game starts: [`install`] for the game
/// `hl_exe` belongs to and the `-game` folder it starts, with one
/// activity-log line when the layout was (re)written or could not be. An
/// empty `hl_exe` (a config with no game yet) does nothing.
pub fn prepare(hl_exe: &str, game_folder: &str) {
    let Some(game_dir) = Path::new(hl_exe)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    else {
        return;
    };
    match install(game_dir, game_folder) {
        Ok(Outcome::Written { source, target }) => crate::log_markdown(&format!(
            "- Demo bar layout: wrote {} from {} with the loading progress controls",
            target.display(),
            source.display()
        )),
        Ok(Outcome::Unchanged) => {}
        Err(why) => crate::log_markdown(&format!(
            "- Demo bar layout: skipped, the game will show no loading progress on the demo bar ({why})"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STOCK: &str = "\"DemoPlayer/DemoMainPanel.res\"\r\n{\r\n\t\"TimeLabel\"\r\n\t{\r\n\t\t\"ControlName\"\t\"Label\"\r\n\t\t\"fieldName\"\t\t\"TimeLabel\"\r\n\t\t\"xpos\"\t\t\t\"284\"\r\n\t\t\"ypos\"\t\t\t\"64\"\r\n\t\t\"wide\"\t\t\t\"142\"\r\n\t\t\"tall\"\t\t\t\"24\"\r\n\t\t\"visible\"\t\t\"1\"\r\n\t}\r\n\t\r\n\t\"StopButton\"\r\n\t{\r\n\t\t\"ControlName\"\t\"Button\"\r\n\t\t\"command\"\t\t\"stop\"\r\n\t}\r\n\t\r\n\t\t\r\n}\r\n";

    #[test]
    fn adds_both_controls_inside_the_object_and_keeps_the_rest() {
        let out = with_loading_controls(STOCK).unwrap();
        let close = out.rfind('}').unwrap();
        assert!(out.starts_with(&STOCK[..STOCK.rfind('}').unwrap()]));
        assert_eq!(&out[close..], "}\r\n");
        let body = &out[..close];
        assert!(body.contains("\"StudioLoadPercent\""));
        assert!(body.contains("\"ControlName\"\t\t\"ProgressBar\""));
        assert!(body.contains("\"fieldName\"\t\t\"StudioLoadProgress\""));
        // Both hidden until the hook shows them.
        assert_eq!(body.matches("\"visible\"\t\t\"0\"").count(), 2);
        // The file's own line ending throughout.
        assert!(!out.replace("\r\n", "").contains('\n'));
    }

    #[test]
    fn the_controls_take_time_labels_place() {
        let moved = STOCK
            .replace("\"284\"", "\"300\"")
            .replace("\"64\"", "\"70\"")
            .replace("\"142\"", "\"100\"")
            .replace("\"24\"", "\"20\"");
        let out = with_loading_controls(&moved).unwrap();
        let label = block(&out, PERCENT_LABEL).unwrap();
        assert_eq!(
            rect_of(label),
            Some(Rect {
                x: 300,
                y: 70,
                wide: 100,
                tall: 20
            })
        );
        let bar = block(&out, PROGRESS_BAR).unwrap();
        assert_eq!(
            rect_of(bar),
            Some(Rect {
                x: 336,
                y: 75,
                wide: 64,
                tall: 10
            })
        );
    }

    #[test]
    fn a_layout_without_time_label_uses_the_stock_place() {
        let out = with_loading_controls("\"x\"\n{\n\t\"A\"\n\t{\n\t}\n}\n").unwrap();
        let label = block(&out, PERCENT_LABEL).unwrap();
        assert_eq!(rect_of(label), Some(STOCK_TIME_LABEL));
    }

    #[test]
    fn is_idempotent() {
        let once = with_loading_controls(STOCK).unwrap();
        assert_eq!(with_loading_controls(&once).unwrap(), once);
    }

    #[test]
    fn keeps_what_follows_the_closing_brace() {
        // A build-mode save ends its file with a NUL after the brace.
        let saved = "\"Resource\\DemoPlayerDialog.res\"\n{\n\t\"TimeLabel\"\n\t{\n\t\t\"xpos\"\t\t\"284\"\n\t\t\"ypos\"\t\t\"64\"\n\t\t\"wide\"\t\t\"142\"\n\t\t\"tall\"\t\t\"24\"\n\t}\n}\n\0";
        let out = with_loading_controls(saved).unwrap();
        assert!(out.ends_with("}\n\0"));
        assert!(out.starts_with("\"Resource\\DemoPlayerDialog.res\"\n{\n\t\"TimeLabel\""));
    }

    #[test]
    fn no_object_means_nothing_to_add_to() {
        assert_eq!(with_loading_controls(""), None);
        assert_eq!(with_loading_controls("just words"), None);
    }

    #[test]
    fn tokens_take_quoted_and_bare_words() {
        let words: Vec<_> = tokens("\t\"xpos\"\t\t\"284\" // where").collect();
        assert_eq!(words, ["xpos", "284"]);
        let words: Vec<_> = tokens("wide 142").collect();
        assert_eq!(words, ["wide", "142"]);
    }

    #[test]
    fn block_finds_the_named_object_not_a_field_value() {
        // "fieldName" "TimeLabel" names the control inside its own block;
        // the block itself is the "TimeLabel" followed by a brace.
        let res = "{\n\t\"X\"\n\t{\n\t\t\"fieldName\"\t\"TimeLabel\"\n\t}\n\t\"TimeLabel\"\n\t{\n\t\t\"xpos\"\t\"1\"\n\t}\n}";
        assert_eq!(block(res, "TimeLabel").unwrap().trim(), "\"xpos\"\t\"1\"");
    }

    fn game_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("demo_bar_res_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("valve").join("resource")).unwrap();
        dir
    }

    #[test]
    fn install_prefers_dod_over_valve_and_writes_once() {
        let dir = game_dir("prefers");
        std::fs::write(dir.join("valve").join("resource").join(FILE_NAME), STOCK).unwrap();
        let target = target_in(&dir, "dod");
        assert!(matches!(install(&dir, "dod"), Ok(Outcome::Written { .. })));
        assert!(
            std::fs::read_to_string(&target)
                .unwrap()
                .contains(PROGRESS_BAR)
        );
        assert_eq!(install(&dir, "dod"), Ok(Outcome::Unchanged));

        std::fs::create_dir_all(dir.join("dod").join("resource")).unwrap();
        let users = STOCK.replace("\"284\"", "\"290\"");
        std::fs::write(dir.join("dod").join("resource").join(FILE_NAME), &users).unwrap();
        match install(&dir, "dod") {
            Ok(Outcome::Written { source, .. }) => assert!(source.starts_with(dir.join("dod"))),
            other => panic!("{other:?}"),
        }
        assert!(
            std::fs::read_to_string(&target)
                .unwrap()
                .contains("\"290\"")
        );
        // The user's files are never written.
        assert_eq!(
            std::fs::read_to_string(dir.join("dod").join("resource").join(FILE_NAME)).unwrap(),
            users
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("valve").join("resource").join(FILE_NAME)).unwrap(),
            STOCK
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn install_without_a_source_says_so() {
        let dir = game_dir("empty");
        let why = install(&dir, "dod").unwrap_err();
        assert!(why.contains(FILE_NAME), "{why}");
        assert!(!target_in(&dir, "dod").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn another_game_folder_reads_its_own_layout_and_writes_its_own_addon() {
        // `-game dod_movies -addons` searches dod_movies_addon, then
        // dod_movies: a copy in dod_addon would never load (#402).
        let dir = game_dir("folder");
        std::fs::write(dir.join("valve").join("resource").join(FILE_NAME), STOCK).unwrap();
        std::fs::create_dir_all(dir.join("dod").join("resource")).unwrap();
        std::fs::write(
            dir.join("dod").join("resource").join(FILE_NAME),
            STOCK.replace("\"284\"", "\"111\""),
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("dod_movies").join("resource")).unwrap();
        std::fs::write(
            dir.join("dod_movies").join("resource").join(FILE_NAME),
            STOCK.replace("\"284\"", "\"222\""),
        )
        .unwrap();
        match install(&dir, "dod_movies") {
            Ok(Outcome::Written { source, target }) => {
                assert!(source.starts_with(dir.join("dod_movies")));
                assert_eq!(
                    target,
                    dir.join("dod_movies_addon")
                        .join("resource")
                        .join(FILE_NAME)
                );
            }
            other => panic!("{other:?}"),
        }
        let written = std::fs::read_to_string(target_in(&dir, "dod_movies")).unwrap();
        assert!(written.contains("\"222\""));
        assert!(!dir.join("dod_addon").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bytes_that_are_not_utf8_are_kept() {
        // A Windows-1252 label text ("Zurück", 0xfc) must not become U+FFFD.
        let dir = game_dir("latin1");
        let mut users = STOCK.as_bytes().to_vec();
        let at = users.windows(4).position(|w| w == b"stop").unwrap();
        users.splice(at..at + 4, b"Zur\xfcck".iter().copied());
        std::fs::write(dir.join("valve").join("resource").join(FILE_NAME), &users).unwrap();
        install(&dir, "dod").unwrap();
        let written = std::fs::read(target_in(&dir, "dod")).unwrap();
        let close = users.iter().rposition(|&b| b == b'}').unwrap();
        assert!(written.starts_with(&users[..close]));
        assert!(written.ends_with(&users[close..]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_utf16_layout_is_refused_not_broken() {
        let dir = game_dir("utf16");
        let mut users = vec![0xff, 0xfe];
        users.extend(STOCK.encode_utf16().flat_map(u16::to_le_bytes));
        std::fs::write(dir.join("valve").join("resource").join(FILE_NAME), &users).unwrap();
        let why = install(&dir, "dod").unwrap_err();
        assert!(why.contains("UTF-16"), "{why}");
        assert!(!target_in(&dir, "dod").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prepare_does_nothing_for_no_game() {
        prepare("", "dod");
        prepare("hl.exe", "dod");
    }
}
