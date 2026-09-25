//! The user's own HD styles, `my_styles.txt` (#372 part 3): read for the HD
//! page's lists, and written by its custom-style form.
//!
//! The file is the scripts' (`tools/hd/styles.py`'s `load_my_styles`, which
//! documents the format), in the install's `dod\dodstudio_hd` folder
//! (`hdcommon.user_file`, #385). The command line and the page share it, so
//! this module follows the scripts' rules exactly and never rewrites more of
//! the file than the one line a change is about: comments and the user's own
//! layout stay as they were.
//!
//! One line per style:
//!
//! ```text
//! name = <model file name>             an AI style
//! name = plain <sharpening 0-500>      no AI, more or less sharpened
//! name = blend <style> <style> <0-100> two styles mixed, the first at that percent
//! ```

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::BUILT_IN_STYLES;

/// The file's name, in `dodstudio_hd` (`hdcommon.MY_STYLES`).
pub const MY_STYLES: &str = "my_styles.txt";

/// What `plain` with no number means (`styles.py`).
const DEFAULT_SHARPENING: u32 = 60;

/// How a style is made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StyleDef {
    /// Real-ESRGAN with this model (`models\<model>.param/.bin`).
    Ai { model: String },
    /// A plain enlargement, sharpened by this percent.
    Plain { sharpening: u32 },
    /// Two built styles mixed file by file, `percent` of `a`.
    Blend { a: String, b: String, percent: u32 },
}

impl StyleDef {
    /// What goes after `name = `.
    fn value(&self) -> String {
        match self {
            StyleDef::Ai { model } => model.clone(),
            StyleDef::Plain { sharpening } => format!("plain {sharpening}"),
            StyleDef::Blend { a, b, percent } => format!("blend {a} {b} {percent}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CustomStyle {
    pub name: String,
    #[serde(flatten)]
    pub def: StyleDef,
}

/// The file as the page shows it.
#[derive(Debug, Clone, Serialize)]
pub struct MyStyles {
    /// The file builds read, and the one the form writes.
    pub path: String,
    pub exists: bool,
    /// A copy from before #385 in the scripts' folder, read while `path` has
    /// none; the form's first save copies it to `path`.
    pub old_place: Option<String>,
    pub styles: Vec<CustomStyle>,
    /// Why the file can't be read, in the scripts' words. A build refuses to
    /// start while there is one.
    pub error: Option<String>,
}

/// `name` is a folder name the hook accepts (`styles.py`'s `NAME`).
pub fn name_ok(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

fn is_built_in(name: &str) -> bool {
    BUILT_IN_STYLES.iter().any(|s| s.name == name)
}

/// A model file name the upscaler can take: no path, no spaces.
fn model_ok(model: &str) -> bool {
    !model.is_empty()
        && !model.contains(['/', '\\', ':', '#'])
        && !model.chars().any(char::is_whitespace)
}

/// One line's style, or `None` for a blank or comment-only line.
fn parse_line(raw: &str, where_: &str) -> Result<Option<CustomStyle>, String> {
    let line = raw.split('#').next().unwrap_or_default().trim();
    if line.is_empty() {
        return Ok(None);
    }
    let Some((name, value)) = line.split_once('=') else {
        return Err(crate::messages::hd_style_expected_eq(where_, line));
    };
    let (name, value) = (name.trim().to_ascii_lowercase(), value.trim());
    if value.is_empty() {
        return Err(crate::messages::hd_style_expected_eq(where_, line));
    }
    if !name_ok(&name) {
        return Err(crate::messages::hd_style_bad_name(where_, &name));
    }
    if is_built_in(&name) {
        return Err(crate::messages::hd_style_built_in(where_, &name));
    }
    let words: Vec<&str> = value.split_whitespace().collect();
    let bad = || crate::messages::hd_style_bad_value(where_, value);
    let number = |word: &str, max: u32| word.parse::<u32>().ok().filter(|n| *n <= max);
    let def = match words[0].to_ascii_lowercase().as_str() {
        "plain" => StyleDef::Plain {
            sharpening: match words.get(1) {
                Some(word) => number(word, 500).ok_or_else(bad)?,
                None => DEFAULT_SHARPENING,
            },
        },
        "blend" => StyleDef::Blend {
            a: words.get(1).ok_or_else(bad)?.to_ascii_lowercase(),
            b: words.get(2).ok_or_else(bad)?.to_ascii_lowercase(),
            percent: number(words.get(3).ok_or_else(bad)?, 100).ok_or_else(bad)?,
        },
        _ if words.len() == 1 => StyleDef::Ai {
            model: words[0].to_string(),
        },
        _ => return Err(bad()),
    };
    Ok(Some(CustomStyle { name, def }))
}

/// Every style in `text`, as `load_my_styles` reads it: later lines win a
/// repeated name, and a blend must name styles that exist.
pub fn parse(text: &str) -> Result<Vec<CustomStyle>, String> {
    let mut styles: Vec<CustomStyle> = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        if let Some(style) = parse_line(raw, &format!("{MY_STYLES} line {}", n + 1))? {
            match styles.iter_mut().find(|s| s.name == style.name) {
                Some(existing) => *existing = style,
                None => styles.push(style),
            }
        }
    }
    for style in &styles {
        if let StyleDef::Blend { a, b, .. } = &style.def {
            for source in [a, b] {
                if !is_built_in(source) && !styles.iter().any(|s| &s.name == source) {
                    return Err(crate::messages::hd_style_blends_unknown(
                        &style.name,
                        source,
                    ));
                }
            }
        }
    }
    Ok(styles)
}

/// The file in `hd_root`, and the scripts' folder's old copy when builds
/// read that one instead (`hdcommon.user_file`: only while `hd_root` has
/// none).
pub fn locate(hd_root: &Path, scripts: Option<&Path>) -> (PathBuf, Option<PathBuf>) {
    let path = hd_root.join(MY_STYLES);
    let old = scripts
        .map(|dir| dir.join(MY_STYLES))
        .filter(|old| !path.exists() && old.is_file());
    (path, old)
}

/// Reads the file builds would use.
pub fn read(hd_root: &Path, scripts: Option<&Path>) -> MyStyles {
    let (path, old) = locate(hd_root, scripts);
    let source = old.as_deref().unwrap_or(&path);
    let (styles, error) = match std::fs::read_to_string(source) {
        Ok(text) => match parse(&text) {
            Ok(styles) => (styles, None),
            Err(e) => (Vec::new(), Some(e)),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Vec::new(), None),
        Err(e) => (
            Vec::new(),
            Some(crate::messages::labeled(source.display(), e)),
        ),
    };
    MyStyles {
        path: path.to_string_lossy().to_string(),
        exists: path.is_file(),
        old_place: old.map(|p| p.to_string_lossy().to_string()),
        styles,
        error,
    }
}

/// What a new file starts with: where the format is explained.
const HEADER: &str = "\
# Your own HD styles, one per line (DoD Studio's HD page writes this file,
# and the build scripts read it). See my_styles.example.txt for the format.
";

/// `text` with `name`'s line set to `def` (in place when it has one, else
/// added at the end), or removed with `def` `None`. Other lines, comments
/// included, stay as they are.
fn edit(text: &str, name: &str, def: Option<&StyleDef>) -> String {
    let defines = |raw: &str| {
        let line = raw.split('#').next().unwrap_or_default();
        line.split_once('=')
            .is_some_and(|(n, _)| n.trim().eq_ignore_ascii_case(name))
    };
    let new_line = def.map(|def| format!("{name} = {}", def.value()));
    let mut out: Vec<String> = Vec::new();
    let mut placed = false;
    for raw in text.lines() {
        if defines(raw) {
            // The first line naming it takes the new definition; any later
            // ones (which the scripts would read instead) go.
            if let Some(line) = new_line.as_ref().filter(|_| !placed) {
                out.push(line.clone());
            }
            placed = true;
        } else {
            out.push(raw.to_string());
        }
    }
    if let Some(line) = new_line.filter(|_| !placed) {
        out.push(line);
    }
    let mut text = out.join("\n");
    text.push('\n');
    text
}

/// Writes `text` to `path` whole or not at all.
fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| crate::messages::labeled(dir.display(), e))?;
    }
    let part = path.with_extension("txt.part");
    std::fs::write(&part, text).map_err(|e| crate::messages::labeled(part.display(), e))?;
    std::fs::rename(&part, path).map_err(|e| crate::messages::labeled(path.display(), e))
}

/// The text a change starts from: the file, else the old copy builds were
/// reading, else a header.
fn current_text(path: &Path, old: Option<&Path>) -> Result<String, String> {
    let source = old.unwrap_or(path);
    match std::fs::read_to_string(source) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HEADER.to_string()),
        Err(e) => Err(crate::messages::labeled(source.display(), e)),
    }
}

/// Adds `name` as `def`, or changes it if the file already has it. Refused,
/// with nothing written, when the result is a file the scripts would refuse.
pub fn save(
    hd_root: &Path,
    scripts: Option<&Path>,
    name: &str,
    def: &StyleDef,
) -> Result<(), String> {
    let name = name.trim().to_ascii_lowercase();
    let where_ = crate::messages::HD_STYLE_FORM;
    if !name_ok(&name) {
        return Err(crate::messages::hd_style_bad_name(where_, &name));
    }
    if is_built_in(&name) {
        return Err(crate::messages::hd_style_built_in(where_, &name));
    }
    match def {
        StyleDef::Ai { model } if !model_ok(model) => {
            return Err(crate::messages::hd_style_bad_value(where_, model));
        }
        StyleDef::Plain { sharpening } if *sharpening > 500 => {
            return Err(crate::messages::hd_style_bad_value(where_, &def.value()));
        }
        StyleDef::Blend { a, b, percent } => {
            if *percent > 100 || !name_ok(a) || !name_ok(b) {
                return Err(crate::messages::hd_style_bad_value(where_, &def.value()));
            }
            if a == &name || b == &name {
                return Err(crate::messages::hd_style_blends_itself(&name));
            }
        }
        _ => {}
    }
    let (path, old) = locate(hd_root, scripts);
    let text = edit(&current_text(&path, old.as_deref())?, &name, Some(def));
    parse(&text)?;
    write(&path, &text)
}

/// Takes `name` out of the file. Refused when another style blends it.
pub fn remove(hd_root: &Path, scripts: Option<&Path>, name: &str) -> Result<(), String> {
    let (path, old) = locate(hd_root, scripts);
    let current = current_text(&path, old.as_deref())?;
    if !parse(&current).is_ok_and(|styles| styles.iter().any(|s| s.name == name)) {
        return Ok(()); // nothing to take out, so nothing to write
    }
    let text = edit(&current, name, None);
    parse(&text)?;
    write(&path, &text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    fn ai(model: &str) -> StyleDef {
        StyleDef::Ai {
            model: model.into(),
        }
    }

    #[test]
    fn the_example_files_lines_read_as_the_scripts_read_them() {
        // Every example line, uncommented.
        let example = include_str!("../../../goldsrc-hooks/tools/hd/my_styles.example.txt");
        let lines: String = example
            .lines()
            .filter_map(|l| l.strip_prefix("# "))
            .filter(|l| l.contains(" = ") && !l.trim_start().starts_with("name"))
            .map(|l| format!("{l}\n"))
            .collect();
        let styles = parse(&lines).unwrap();
        let names: Vec<_> = styles.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["anime", "soft", "crisp", "sharp70", "gentle"]);
        assert_eq!(styles[0].def, ai("realesrgan-x4plus-anime"));
        assert_eq!(styles[1].def, StyleDef::Plain { sharpening: 0 });
        assert_eq!(styles[2].def, StyleDef::Plain { sharpening: 150 });
        assert_eq!(
            styles[3].def,
            StyleDef::Blend {
                a: "ultrasharp".into(),
                b: "plain".into(),
                percent: 70
            }
        );
    }

    #[test]
    fn bad_lines_are_refused_as_the_scripts_refuse_them() {
        for bad in [
            "crisp plain 150",   // no =
            "crisp =",           // no value
            "Crisp! = plain 1",  // not a folder name
            "plain = plain 10",  // a built-in name
            "crisp = plain 501", // too sharp
            "crisp = plain x",   // not a number
            "mix = blend plain", // too few words
            "mix = blend plain x4plus 101",
            "mix = blend plain nothing 50", // blends a style that doesn't exist
            "two = two words",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        // Case is folded, `plain` alone is 60, comments and blanks are
        // skipped, a later line wins, and a blend may name a custom style.
        let styles = parse(
            "# mine\n\nCRISP = Plain\nmix = blend crisp x4plus 25 # half\ncrisp = plain 90\n",
        )
        .unwrap();
        assert_eq!(styles.len(), 2);
        assert_eq!(styles[0].name, "crisp");
        assert_eq!(styles[0].def, StyleDef::Plain { sharpening: 90 });
    }

    #[test]
    fn saving_changes_one_line_and_keeps_the_rest() {
        let dir = Scratch::new("hd_my_styles_save");
        let root = dir.join("dodstudio_hd");
        // No folder, no file: both are made, with a header.
        save(&root, None, "crisp", &StyleDef::Plain { sharpening: 150 }).unwrap();
        let path = root.join(MY_STYLES);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# Your own HD styles"));
        assert!(text.ends_with("crisp = plain 150\n"));

        std::fs::write(
            &path,
            "# my notes\ncrisp = plain 150  # sharp\n\nanime = realesrgan-x4plus-anime\n",
        )
        .unwrap();
        save(&root, None, "Crisp", &StyleDef::Plain { sharpening: 90 }).unwrap();
        save(
            &root,
            None,
            "mix",
            &StyleDef::Blend {
                a: "crisp".into(),
                b: "anime".into(),
                percent: 30,
            },
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# my notes\ncrisp = plain 90\n\nanime = realesrgan-x4plus-anime\nmix = blend crisp anime 30\n"
        );

        // Refused, and nothing written: a blend of itself, of a missing
        // style, a built-in name, a bad model name, removing a blended style.
        let before = std::fs::read_to_string(&path).unwrap();
        let blend = |a: &str| StyleDef::Blend {
            a: a.into(),
            b: "plain".into(),
            percent: 50,
        };
        assert!(save(&root, None, "loop", &blend("loop")).is_err());
        assert!(save(&root, None, "other", &blend("nothing")).is_err());
        assert!(save(&root, None, "ultrasharp", &ai("x")).is_err());
        assert!(save(&root, None, "odd", &ai("../models/x")).is_err());
        assert!(save(&root, None, "odd", &ai("two words")).is_err());
        assert!(remove(&root, None, "anime").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);

        // Removing a style the file doesn't have writes nothing.
        remove(&root, None, "nothing").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        remove(&root, None, "mix").unwrap();
        remove(&root, None, "crisp").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# my notes\n\nanime = realesrgan-x4plus-anime\n"
        );
        let status = read(&root, None);
        assert!(status.exists && status.error.is_none());
        assert_eq!(status.styles.len(), 1);
    }

    #[test]
    fn a_copy_left_beside_the_scripts_is_read_until_the_first_save_moves_it() {
        let dir = Scratch::new("hd_my_styles_old");
        let root = dir.join("dodstudio_hd");
        let scripts = dir.join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(scripts.join(MY_STYLES), "soft = plain 0\n").unwrap();

        let status = read(&root, Some(&scripts));
        assert!(!status.exists);
        assert!(status.old_place.is_some());
        assert_eq!(status.styles[0].name, "soft");

        save(
            &root,
            Some(&scripts),
            "crisp",
            &StyleDef::Plain { sharpening: 150 },
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join(MY_STYLES)).unwrap(),
            "soft = plain 0\ncrisp = plain 150\n"
        );
        // Builds read the install's file now, so the old one is no longer
        // the one in use.
        let status = read(&root, Some(&scripts));
        assert!(status.exists && status.old_place.is_none());
        assert_eq!(status.styles.len(), 2);
    }

    #[test]
    fn an_unreadable_file_is_reported_not_emptied() {
        let dir = Scratch::new("hd_my_styles_bad");
        std::fs::write(dir.join(MY_STYLES), "crisp plain\n").unwrap();
        let status = read(&dir, None);
        assert!(status.error.as_deref().unwrap().contains("line 1"));
        assert!(save(&dir, None, "soft", &StyleDef::Plain { sharpening: 0 }).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join(MY_STYLES)).unwrap(),
            "crisp plain\n"
        );
    }

    /// The rules here are copies of `styles.py`'s; the source is read so a
    /// change there fails here.
    #[test]
    fn the_rules_match_styles_py() {
        let styles = include_str!("../../../goldsrc-hooks/tools/hd/styles.py");
        assert!(styles.contains(r#"NAME = re.compile(r"^[a-z0-9_-]{1,32}$")"#));
        assert!(styles.contains("if not 0 <= pct <= 500:"));
        assert!(styles.contains("if not 0 <= pct <= 100:"));
        assert!(styles.contains(&format!(
            "pct = int(words[1]) if len(words) > 1 else {DEFAULT_SHARPENING}"
        )));
        let common = include_str!("../../../goldsrc-hooks/tools/hd/hdcommon.py");
        assert!(common.contains(&format!("MY_STYLES = \"{MY_STYLES}\"")));
    }
}
