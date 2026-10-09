//! Which maps get HD map textures and skies: the install's `hd_maps.txt`,
//! read and written by the HD page's map list.
//!
//! The file is the scripts' (`tools/hd/hdcommon.py`'s `map_patterns` and
//! `select_maps`), in the install's `dod\dodstudio_hd` folder
//! (`hdcommon.user_file`, #385). One pattern per line, `#` starts a comment,
//! `.bsp` is optional, case doesn't matter; `*` matches any run of
//! characters and `?` exactly one (`fnmatch.fnmatchcase` on the lowercased
//! name). With no file, every map in `dod\maps` is built.
//!
//! "Every map" is therefore the file's absence. So that choosing it never
//! throws a list away, the page moves the list aside to `hd_maps.off.txt`,
//! which the scripts don't read, and the next list saved replaces it.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// `hdcommon.MAP_LIST`.
pub const MAP_LIST: &str = "hd_maps.txt";

/// Where "every map" keeps the list it turned off. Only the page reads it.
pub const MAP_LIST_OFF: &str = "hd_maps.off.txt";

/// What a new list starts with: the format, in the example file's words.
const HEADER: &str = "\
# Which maps get HD map textures and skies (DoD Studio's HD page writes this
# file, and the build scripts read it). One map per line; case doesn't matter.
#   *   matches any run of characters: dod_railroad* is every railroad map
#   ?   matches exactly one character
";

/// A map a build can make textures for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MapFile {
    /// The `.bsp`'s name without the extension, as on disk.
    pub name: String,
    pub bytes: u64,
}

/// The list as the page shows it.
#[derive(Debug, Clone, Serialize)]
pub struct MapList {
    /// The file builds read, and the one the page writes.
    pub path: String,
    /// A list is in effect: builds skip every map it doesn't pick. False
    /// means every map is built.
    pub active: bool,
    /// A copy from before #385 in the scripts' folder, read while `path` has
    /// none; the page's first save moves it to `path`.
    pub old_place: Option<String>,
    /// The list's text: the one in effect, else the one "every map" set
    /// aside, else a new list's header.
    pub text: String,
    /// Every map in `<game>\dod\maps`, sorted: the maps a build can choose
    /// from (`hdcommon.all_maps`).
    pub maps: Vec<MapFile>,
}

/// `hdcommon.map_patterns`: the patterns in the file's text, lowercased and
/// without `.bsp`.
pub fn patterns(text: &str) -> Vec<String> {
    text.lines()
        .map(|raw| {
            raw.split('#')
                .next()
                .unwrap_or_default()
                .trim()
                .to_lowercase()
        })
        .map(|line| {
            line.strip_suffix(".bsp")
                .map(str::to_string)
                .unwrap_or(line)
        })
        .filter(|line| !line.is_empty())
        .collect()
}

/// `fnmatch.fnmatchcase` for the two wildcards the file documents, on a
/// lowercased pattern and name.
pub fn matches(pattern: &str, name: &str) -> bool {
    fn go(pattern: &[u8], name: &[u8]) -> bool {
        match (pattern.first(), name.first()) {
            (None, None) => true,
            (Some(b'*'), _) => {
                go(&pattern[1..], name) || (!name.is_empty() && go(pattern, &name[1..]))
            }
            (Some(b'?'), Some(_)) => go(&pattern[1..], &name[1..]),
            (Some(p), Some(n)) if p == n => go(&pattern[1..], &name[1..]),
            _ => false,
        }
    }
    go(pattern.as_bytes(), name.to_lowercase().as_bytes())
}

/// `hdcommon.select_maps`: the names `text`'s patterns pick, in order.
pub fn select<'a>(text: &str, names: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let patterns = patterns(text);
    names
        .into_iter()
        .filter(|name| patterns.iter().any(|p| matches(p, name)))
        .collect()
}

/// Every `.bsp` in `<game>\dod\maps`, sorted case-insensitively.
pub fn available_maps(game_exe: &Path) -> Vec<MapFile> {
    let Some(maps_dir) = game_exe.parent().map(|g| g.join("dod").join("maps")) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(maps_dir) else {
        return Vec::new();
    };
    let mut maps: Vec<MapFile> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let stem = name
                .strip_suffix(".bsp")
                .or_else(|| name.strip_suffix(".BSP"))?;
            Some(MapFile {
                name: stem.to_string(),
                bytes: e.metadata().map(|m| m.len()).unwrap_or(0),
            })
        })
        .collect();
    maps.sort_by_key(|m| m.name.to_lowercase());
    maps
}

/// The file in `hd_root`, and the scripts' folder's old copy when builds
/// read that one instead (`hdcommon.user_file`: only while `hd_root` has
/// none).
pub fn locate(hd_root: &Path, scripts: Option<&Path>) -> (PathBuf, Option<PathBuf>) {
    let path = hd_root.join(MAP_LIST);
    let old = scripts
        .map(|dir| dir.join(MAP_LIST))
        .filter(|old| !path.exists() && old.is_file());
    (path, old)
}

/// The list builds would use, and every map there is to pick from.
pub fn read(game_exe: &Path, hd_root: &Path, scripts: Option<&Path>) -> MapList {
    let (path, old) = locate(hd_root, scripts);
    let in_effect = old.as_deref().unwrap_or(&path);
    let (active, text) = match std::fs::read_to_string(in_effect) {
        Ok(text) => (true, text),
        Err(_) => (
            false,
            std::fs::read_to_string(hd_root.join(MAP_LIST_OFF))
                .unwrap_or_else(|_| HEADER.to_string()),
        ),
    };
    MapList {
        path: path.to_string_lossy().to_string(),
        active,
        old_place: old.map(|p| p.to_string_lossy().to_string()),
        text,
        maps: available_maps(game_exe),
    }
}

/// The maps a build makes map textures for: all of them without a list.
pub fn chosen(game_exe: &Path, hd_root: &Path, scripts: Option<&Path>) -> Vec<String> {
    let list = read(game_exe, hd_root, scripts);
    let names = list.maps.iter().map(|m| m.name.as_str());
    if list.active {
        select(&list.text, names)
            .into_iter()
            .map(str::to_string)
            .collect()
    } else {
        names.map(str::to_string).collect()
    }
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

/// Puts `text` in effect as the list. Refused when it picks nothing, since
/// the build would then make no map textures at all.
pub fn save(hd_root: &Path, text: &str) -> Result<(), String> {
    if patterns(text).is_empty() {
        return Err(crate::messages::HD_MAP_LIST_EMPTY.to_string());
    }
    let mut text = text.replace("\r\n", "\n");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    write(&hd_root.join(MAP_LIST), &text)?;
    // The saved list replaces the one set aside. A pre-#385 copy beside the
    // scripts stays, as `my_styles` leaves one: builds stop reading it now
    // that `hd_root` has a list.
    let _ = std::fs::remove_file(hd_root.join(MAP_LIST_OFF));
    Ok(())
}

/// Builds every map from now on: moves the list in effect aside, where the
/// page can bring it back. That includes a pre-#385 copy beside the
/// scripts, which builds would otherwise go on reading.
pub fn use_every_map(hd_root: &Path, scripts: Option<&Path>) -> Result<(), String> {
    let (path, old) = locate(hd_root, scripts);
    let in_effect = old.unwrap_or(path);
    let text = match std::fs::read_to_string(&in_effect) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(crate::messages::labeled(in_effect.display(), e)),
    };
    write(&hd_root.join(MAP_LIST_OFF), &text)?;
    std::fs::remove_file(&in_effect).map_err(|e| crate::messages::labeled(in_effect.display(), e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    fn install(tag: &str, maps: &[&str]) -> (Scratch, PathBuf, PathBuf) {
        let dir = Scratch::new(format!("hd_map_list_{tag}"));
        let maps_dir = dir.join("dod").join("maps");
        std::fs::create_dir_all(&maps_dir).unwrap();
        std::fs::write(dir.join("hl.exe"), b"").unwrap();
        for name in maps {
            std::fs::write(maps_dir.join(name), b"0123456789").unwrap();
        }
        let exe = dir.join("hl.exe");
        let root = dir.join("dod").join("dodstudio_hd");
        (dir, exe, root)
    }

    #[test]
    fn patterns_read_as_the_scripts_read_them() {
        assert_eq!(
            patterns("# mine\nDOD_ANZIO*  # both\n\ndod_harr?ngton.bsp\n  \n"),
            ["dod_anzio*", "dod_harr?ngton"]
        );
        for (pattern, name, hit) in [
            ("dod_railroad*", "dod_railroad2_s9a", true),
            ("dod_railroad*", "dod_rr2", false),
            ("dod_*sherman*", "dod_Sherman_b2", true),
            ("dod_caen", "dod_caen2", false),
            ("dod_saints2_b?", "dod_saints2_b3", true),
            ("dod_saints2_b?", "dod_saints2_b3e", false),
            ("*", "", true),
            ("a?c", "ac", false),
        ] {
            assert_eq!(matches(pattern, name), hit, "{pattern} {name}");
        }
        let common = include_str!("../../../goldsrc-hooks/tools/hd/hdcommon.py");
        assert!(common.contains(&format!("MAP_LIST = \"{MAP_LIST}\"")));
        assert!(common.contains("fnmatch.fnmatchcase(n.lower(), p)"));
        assert!(common.contains(r##"line = raw.split("#", 1)[0].strip().lower()"##));
        assert!(
            !common.contains(MAP_LIST_OFF),
            "the scripts must never read the set-aside list"
        );
    }

    #[test]
    fn without_a_list_every_map_is_chosen_and_the_page_gets_a_header() {
        let (_dir, exe, root) = install("none", &["dod_caen.bsp", "dod_Anzio.bsp", "readme.txt"]);
        let list = read(&exe, &root, None);
        assert!(!list.active);
        assert_eq!(list.text, HEADER);
        let names: Vec<&str> = list.maps.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["dod_Anzio", "dod_caen"]);
        assert_eq!(list.maps[0].bytes, 10);
        assert_eq!(chosen(&exe, &root, None), ["dod_Anzio", "dod_caen"]);
    }

    #[test]
    fn saving_puts_the_list_in_effect_and_every_map_sets_it_aside() {
        let (_dir, exe, root) = install(
            "save",
            &["dod_railroad.bsp", "dod_railroad2_s9a.bsp", "dod_caen.bsp"],
        );
        save(&root, "# mine\r\ndod_railroad*").unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join(MAP_LIST)).unwrap(),
            "# mine\ndod_railroad*\n"
        );
        assert_eq!(
            chosen(&exe, &root, None),
            ["dod_railroad", "dod_railroad2_s9a"]
        );

        use_every_map(&root, None).unwrap();
        assert!(!root.join(MAP_LIST).exists());
        let list = read(&exe, &root, None);
        assert!(!list.active);
        assert_eq!(
            list.text, "# mine\ndod_railroad*\n",
            "the set-aside list comes back to the page"
        );
        assert_eq!(chosen(&exe, &root, None).len(), 3);

        // The next save replaces the set-aside copy.
        save(&root, "dod_caen\n").unwrap();
        assert!(!root.join(MAP_LIST_OFF).exists());
        assert_eq!(chosen(&exe, &root, None), ["dod_caen"]);
    }

    #[test]
    fn a_list_that_picks_nothing_is_refused() {
        let (_dir, _exe, root) = install("empty", &["dod_caen.bsp"]);
        assert!(save(&root, "# only comments\n\n").is_err());
        assert!(!root.join(MAP_LIST).exists());
    }

    #[test]
    fn a_copy_left_beside_the_scripts_is_read_until_the_first_save_moves_it() {
        let (dir, exe, root) = install("old", &["dod_caen.bsp", "dod_anzio.bsp"]);
        let scripts = dir.join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(scripts.join(MAP_LIST), "dod_anzio\n").unwrap();
        let list = read(&exe, &root, Some(&scripts));
        assert!(list.active);
        assert!(list.old_place.is_some());
        assert_eq!(chosen(&exe, &root, Some(&scripts)), ["dod_anzio"]);

        save(&root, "dod_caen\n").unwrap();
        assert_eq!(chosen(&exe, &root, Some(&scripts)), ["dod_caen"]);
        assert!(read(&exe, &root, Some(&scripts)).old_place.is_none());

        // Every map: nothing left in effect, the old copy included.
        std::fs::remove_file(root.join(MAP_LIST)).unwrap();
        use_every_map(&root, Some(&scripts)).unwrap();
        assert!(!scripts.join(MAP_LIST).exists());
        assert_eq!(chosen(&exe, &root, Some(&scripts)).len(), 2);
        assert_eq!(read(&exe, &root, Some(&scripts)).text, "dod_anzio\n");
    }
}
