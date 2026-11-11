//! Built HD output that an app update would build differently (#1155).
//!
//! Some build steps stamp what they wrote with the version of the script that
//! wrote it; a later build repaints only what is behind. Nothing told a user
//! with HD already built that a new app version had something to repaint, so
//! the HD page asks here. Today one step stamps its output: the kill-feed
//! icons (`killfeed_icons.py`'s `VERSION` and `EVEN_VERSION`, recorded per
//! style in `<hd root>\killfeed_icons.json`). Output without a stamp is not
//! checked: the other steps skip files that exist and have no way to say
//! which are old. A step that gains a stamp adds one `check_*` here and one
//! [`Step`] variant, not a special case in the page.
//!
//! Read-only: this reads the bundled scripts' constants and the record, and
//! writes nothing.

use std::path::Path;

use serde::Serialize;

/// The record `killfeed_icons.py` writes (`RECORD`) in the HD root.
const KILLFEED_RECORD: &str = "killfeed_icons.json";
/// The record key of the even layout's atlas (`ATLAS_NAME`); every other key
/// is a painted sheet.
const KILLFEED_ATLAS: &str = "dodstudio_killfeed";
/// The folder name of the app's bundled copy of the scripts. The repo's copy
/// never holds the kill-feed step back (`sprites_hd.killfeed_icons_held`).
const BUNDLED_DIR: &str = "hd-scripts";

/// A build step with a version stamp on its output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// The kill-feed icons painted into the HUD sheets and the even layout's
    /// atlas. The page words what changed, from this name.
    KillfeedIcons,
}

/// A style whose built output is older than the scripts would build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Outdated {
    pub style: String,
    pub step: Step,
}

/// The versions the scripts in a folder would stamp, `None` for a step they
/// do not run (so a build would not touch it either).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Current {
    killfeed: Option<(u64, u64)>,
}

/// The value of the first `NAME = <integer>` line in `source`, ignoring a
/// trailing comment.
fn int_constant(source: &str, name: &str) -> Option<u64> {
    source.lines().find_map(|line| {
        let rest = line.strip_prefix(name)?.trim_start().strip_prefix('=')?;
        let digits: String = rest
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        digits.parse().ok()
    })
}

/// Whether `sprites_hd.py` would skip the kill-feed step in this folder: its
/// `HOLD_KILLFEED_ICONS` is on and this is the bundled copy
/// (`killfeed_icons_held`). A script without the flag holds nothing.
fn killfeed_held(scripts: &Path, sprites_hd: &str) -> bool {
    let on = sprites_hd.lines().any(|line| {
        line.strip_prefix("HOLD_KILLFEED_ICONS")
            .and_then(|rest| rest.trim_start().strip_prefix('='))
            .is_some_and(|value| value.trim_start().starts_with("True"))
    });
    on && scripts.file_name().is_some_and(|n| n == BUNDLED_DIR)
}

fn current(scripts: &Path) -> Current {
    let read = |name: &str| std::fs::read_to_string(scripts.join(name)).unwrap_or_default();
    let sprites = read("sprites_hd.py");
    let icons = read("killfeed_icons.py");
    let killfeed = if killfeed_held(scripts, &sprites) {
        None
    } else {
        int_constant(&icons, "VERSION").zip(int_constant(&icons, "EVEN_VERSION"))
    };
    Current { killfeed }
}

/// Whether anything recorded for one style is older than `current`.
/// A newer record (built by a later app, then this one reopened) is not
/// "behind", and a record in a shape this does not know is left alone.
fn killfeed_behind(style_record: &serde_json::Value, (version, even): (u64, u64)) -> bool {
    let Some(entries) = style_record.as_object() else {
        return false;
    };
    entries.iter().any(|(key, entry)| {
        let stamp = &entry["version"];
        if key == KILLFEED_ATLAS {
            match stamp.as_array().map(Vec::as_slice) {
                Some([v, e]) => v
                    .as_u64()
                    .zip(e.as_u64())
                    .is_some_and(|recorded| recorded < (version, even)),
                _ => false,
            }
        } else {
            stamp.as_u64().is_some_and(|recorded| recorded < version)
        }
    })
}

/// Every style under `hd_root` whose stamped output is behind what the
/// scripts in `scripts` would build, sorted by style. A style with no folder
/// left under `sprites` (deleted since) is not reported.
pub fn check(hd_root: &Path, scripts: Option<&Path>) -> Vec<Outdated> {
    let Some(scripts) = scripts else {
        return Vec::new();
    };
    let current = current(scripts);
    let mut found = Vec::new();
    if let Some(killfeed) = current.killfeed
        && let Ok(text) = std::fs::read_to_string(hd_root.join(KILLFEED_RECORD))
        && let Ok(record) = serde_json::from_str::<serde_json::Value>(&text)
        && let Some(styles) = record.as_object()
    {
        for (style, entries) in styles {
            if hd_root.join("sprites").join(style).is_dir() && killfeed_behind(entries, killfeed) {
                found.push(Outdated {
                    style: style.clone(),
                    step: Step::KillfeedIcons,
                });
            }
        }
    }
    found.sort_by(|a, b| a.style.cmp(&b.style));
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    fn write(dir: &Path, name: &str, text: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(name), text).unwrap();
    }

    /// A scripts folder as `name` (`hd-scripts` is the bundled copy).
    fn scripts(root: &Path, name: &str, hold: Option<&str>) -> std::path::PathBuf {
        let dir = root.join(name);
        write(
            &dir,
            "killfeed_icons.py",
            "VERSION = 14  # 14: the skull\nEVEN_VERSION = 3  # 3: composed\nATLAS_TABLE_VERSION = 1\n",
        );
        let hold = hold.map_or(String::new(), |v| format!("HOLD_KILLFEED_ICONS = {v}\n"));
        write(&dir, "sprites_hd.py", &hold);
        dir
    }

    fn record(root: &Path, json: &str) {
        write(root, KILLFEED_RECORD, json);
        std::fs::create_dir_all(root.join("sprites").join("ultrasharp")).unwrap();
        std::fs::create_dir_all(root.join("sprites").join("remacri")).unwrap();
    }

    #[test]
    fn reads_the_constants_and_not_their_lookalikes() {
        let src = "ATLAS_TABLE_VERSION = 1\nVERSION = 14  # 14: x\nEVEN_VERSION = 3\n";
        assert_eq!(int_constant(src, "VERSION"), Some(14));
        assert_eq!(int_constant(src, "EVEN_VERSION"), Some(3));
        assert_eq!(int_constant(src, "MISSING"), None);
        assert_eq!(int_constant("VERSION = x", "VERSION"), None);
    }

    /// The repo's own scripts, in the shape this module reads: a reformat of
    /// `VERSION = 14` (an annotation, say) would otherwise switch the check
    /// off with every other test still green.
    #[test]
    fn the_repo_scripts_hold_the_lines_this_reads() {
        let dir = crate::hd::build::dev_scripts_dir();
        let icons = std::fs::read_to_string(dir.join("killfeed_icons.py")).unwrap();
        assert!(int_constant(&icons, "VERSION").is_some());
        assert!(int_constant(&icons, "EVEN_VERSION").is_some());
        let sprites = std::fs::read_to_string(dir.join("sprites_hd.py")).unwrap();
        for line in sprites
            .lines()
            .filter(|l| l.starts_with("HOLD_KILLFEED_ICONS"))
        {
            assert!(
                line == "HOLD_KILLFEED_ICONS = True" || line == "HOLD_KILLFEED_ICONS = False",
                "killfeed_held reads `HOLD_KILLFEED_ICONS = True|False`, found {line:?}"
            );
        }
    }

    #[test]
    fn a_sheet_or_atlas_stamped_older_is_reported_per_style() {
        let root = Scratch::new("hd_outdated_old");
        let s = scripts(root.path(), "scripts", None);
        let hd = root.path().join("hd");
        record(
            &hd,
            r#"{"ultrasharp": {"weapons1_0_ab": {"version": 13}, "weapons2_0_cd": {"version": 14}},
                "remacri": {"weapons1_0_ab": {"version": 14},
                            "dodstudio_killfeed": {"version": [14, 2]}}}"#,
        );
        let found = check(&hd, Some(&s));
        assert_eq!(
            found,
            vec![
                Outdated {
                    style: "remacri".into(),
                    step: Step::KillfeedIcons
                },
                Outdated {
                    style: "ultrasharp".into(),
                    step: Step::KillfeedIcons
                },
            ]
        );
    }

    #[test]
    fn output_at_or_past_the_current_version_is_not_reported() {
        let root = Scratch::new("hd_outdated_current");
        let s = scripts(root.path(), "scripts", None);
        let hd = root.path().join("hd");
        record(
            &hd,
            r#"{"ultrasharp": {"weapons1_0_ab": {"version": 14}, "dodstudio_killfeed": {"version": [14, 3]}},
                "remacri": {"weapons1_0_ab": {"version": 15}, "dodstudio_killfeed": {"version": [15, 1]}}}"#,
        );
        assert!(check(&hd, Some(&s)).is_empty());
    }

    #[test]
    fn the_hold_silences_the_bundled_copy_only() {
        let root = Scratch::new("hd_outdated_hold");
        let hd = root.path().join("hd");
        record(&hd, r#"{"ultrasharp": {"weapons1_0_ab": {"version": 13}}}"#);
        let bundled = scripts(root.path(), "hd-scripts", Some("True"));
        assert!(check(&hd, Some(&bundled)).is_empty());
        // The repo's copy paints the icons whatever the flag says.
        let repo = scripts(root.path(), "hd", Some("True"));
        assert_eq!(check(&hd, Some(&repo)).len(), 1);
        // Lifted: the bundled copy builds them again.
        let lifted = scripts(root.path(), "hd-scripts", Some("False"));
        assert_eq!(check(&hd, Some(&lifted)).len(), 1);
        let gone = scripts(root.path(), "hd-scripts", None);
        assert_eq!(check(&hd, Some(&gone)).len(), 1);
    }

    #[test]
    fn nothing_to_compare_means_nothing_reported() {
        let root = Scratch::new("hd_outdated_none");
        let s = scripts(root.path(), "scripts", None);
        let hd = root.path().join("hd");
        // No record, no scripts, a garbled record, a style folder deleted.
        assert!(check(&hd, Some(&s)).is_empty());
        record(&hd, r#"{"ultrasharp": {"weapons1_0_ab": {"version": 13}}}"#);
        assert!(check(&hd, None).is_empty());
        std::fs::remove_dir_all(hd.join("sprites").join("ultrasharp")).unwrap();
        assert!(check(&hd, Some(&s)).is_empty());
        write(&hd, KILLFEED_RECORD, "not json");
        assert!(check(&hd, Some(&s)).is_empty());
        write(
            &hd,
            KILLFEED_RECORD,
            r#"{"ultrasharp": {"weapons1_0_ab": {"version": "x"}}, "remacri": 3}"#,
        );
        assert!(check(&hd, Some(&s)).is_empty());
    }
}
