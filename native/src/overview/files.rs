//! Where overviews come from and go: the Half-Life installs on this PC,
//! their maps, the overview each already has, the page's saved edits, and
//! writing a finished overview without losing anyone's own.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::transform::Transform;

/// What the first line of every `.txt` this app writes contains, so its own
/// files are told apart from a user's.
pub const OURS: &str = "made by DoD Studio";
/// Where a user's own overview is moved before one of ours replaces it.
pub const BACKUP_DIR: &str = "dodstudio_backup";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Install {
    /// The install's folder name ("Half-Life", "Half-Life - PRE-Anniversary
    /// for Movies").
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapEntry {
    pub name: String,
    pub bsp: String,
    /// `dod/overviews/<map>.txt` exists.
    pub has_overview: bool,
    /// An overview this app wrote, in `dod` or `dod_addon`.
    pub has_ours: bool,
    /// The page has saved edits for it.
    pub has_edits: bool,
}

/// Where to write the overview the game reads: `dod/overviews`, which every
/// launch reads, or `dod_addon/overviews`, read first but only when the
/// game runs with `-addons` (DoD Studio's launches add it). The
/// high-quality copy always goes to `dod_addon` ([`save_hd`]).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Addon,
    Game,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Tga,
    Bmp,
}

/// `<steam>/steamapps/common` folders: the one holding `hint` (an `hl.exe`
/// or an install folder), Steam's default, and every library in
/// `libraryfolders.vdf`.
fn common_dirs(hint: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(hint) = hint {
        for ancestor in hint.ancestors() {
            if ancestor
                .file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("common"))
            {
                dirs.push(ancestor.to_path_buf());
                break;
            }
        }
    }
    let steam = PathBuf::from(r"C:\Program Files (x86)\Steam");
    dirs.push(steam.join("steamapps").join("common"));
    if let Ok(vdf) = std::fs::read_to_string(steam.join("steamapps").join("libraryfolders.vdf")) {
        for line in vdf.lines() {
            let parts: Vec<&str> = line.split('"').collect();
            if parts.len() >= 5 && parts[1].eq_ignore_ascii_case("path") {
                let library = PathBuf::from(parts[3].replace("\\\\", "\\"));
                dirs.push(library.join("steamapps").join("common"));
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| seen.insert(d.to_string_lossy().to_ascii_lowercase()));
    dirs
}

/// Every Half-Life install with Day of Defeat in it.
pub fn installs(hint: Option<&Path>) -> Vec<Install> {
    let mut out = Vec::new();
    for common in common_dirs(hint) {
        let Ok(entries) = std::fs::read_dir(&common) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.join("hl.exe").is_file() && path.join("dod").join("maps").is_dir() {
                out.push(Install {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    path: path.to_string_lossy().into_owned(),
                });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn is_ours(txt: &Path) -> bool {
    std::fs::read_to_string(txt).is_ok_and(|t| t.contains(OURS))
}

/// The page's saved edits for `map`: `%APPDATA%/dod-studio/overviews/<map>.json`.
pub fn edits_path(map: &str) -> PathBuf {
    crate::shared::paths::get_appdata_dir()
        .join("overviews")
        .join(format!("{}.json", safe(map)))
}

/// A map name as a file name: no path separators or `..`.
fn safe(map: &str) -> String {
    map.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "_-.".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_start_matches('.')
        .to_string()
}

/// The folders the game reads maps from, highest priority first:
/// `dod_addon` (with `-addons`), `dod`, then `dod_downloads`, where the
/// 25th Anniversary game saves maps downloaded from a server.
const MAP_DIRS: [&str; 3] = ["dod_addon", "dod", "dod_downloads"];

/// Where `map`'s `.bsp` is in `install`, looking where the game does
/// ([`MAP_DIRS`]); `dod/maps` when it is nowhere, so the error names the
/// usual place.
pub fn bsp_path(install: &Path, map: &str) -> PathBuf {
    let file = format!("{}.bsp", safe(map));
    MAP_DIRS
        .iter()
        .map(|dir| install.join(dir).join("maps").join(&file))
        .find(|p| p.is_file())
        .unwrap_or_else(|| install.join("dod").join("maps").join(&file))
}

/// Every map in an install's map folders ([`MAP_DIRS`]), and what overview
/// each has. A map in more than one folder is listed once, from the folder
/// the game would load it from.
pub fn maps(install: &Path) -> Vec<MapEntry> {
    let dod = install.join("dod");
    let addon = install.join("dod_addon");
    let mut out: Vec<MapEntry> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for dir in MAP_DIRS {
        let Ok(entries) = std::fs::read_dir(install.join(dir).join("maps")) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("bsp"))
            {
                continue;
            }
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !seen.insert(name.to_ascii_lowercase()) {
                continue;
            }
            let txt = format!("{name}.txt");
            let game_txt = dod.join("overviews").join(&txt);
            let addon_txt = addon.join("overviews").join(&txt);
            out.push(MapEntry {
                has_overview: game_txt.is_file(),
                has_ours: is_ours(&game_txt) || is_ours(&addon_txt),
                has_edits: edits_path(&name).is_file() || sidecar_path(install, &name).is_file(),
                bsp: path.to_string_lossy().into_owned(),
                name,
            });
        }
    }
    out.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
    });
    out
}

/// What [`save`] did.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Saved {
    pub written: Vec<String>,
    /// The user's own files moved aside first.
    pub backed_up: Vec<String>,
    /// An older overview of ours for the map in the other folder, removed:
    /// left in `dod_addon` it would win over a new one in `dod` whenever
    /// the game runs with `-addons`.
    #[serde(default)]
    pub removed: Vec<String>,
}

/// Writes `image` (already encoded) and its `.txt` for `map`. A user's own
/// overview there is moved into `overviews/dodstudio_backup` first, once:
/// a backup already there is the original and is kept.
pub fn save(
    install: &Path,
    target: Target,
    map: &str,
    format: Format,
    image: &[u8],
    transform: &Transform,
) -> Result<Saved, String> {
    let map = safe(map);
    let folder = match target {
        Target::Addon => install.join("dod_addon").join("overviews"),
        Target::Game => install.join("dod").join("overviews"),
    };
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let txt = folder.join(format!("{map}.txt"));
    let mut saved = Saved::default();

    if txt.exists() && !is_ours(&txt) {
        let backups = folder.join(BACKUP_DIR);
        std::fs::create_dir_all(&backups).map_err(|e| format!("{}: {e}", backups.display()))?;
        for ext in ["txt", "bmp", "tga"] {
            let from = folder.join(format!("{map}.{ext}"));
            let to = backups.join(format!("{map}.{ext}"));
            if from.exists() && !to.exists() {
                std::fs::copy(&from, &to).map_err(|e| format!("{}: {e}", to.display()))?;
                saved.backed_up.push(to.to_string_lossy().into_owned());
            }
        }
    }

    let ext = match format {
        Format::Tga => "tga",
        Format::Bmp => "bmp",
    };
    let image_path = folder.join(format!("{map}.{ext}"));
    std::fs::write(&image_path, image).map_err(|e| format!("{}: {e}", image_path.display()))?;
    saved
        .written
        .push(image_path.to_string_lossy().into_owned());
    let text = transform.text(&map, &format!("overviews/{map}.{ext}"));
    std::fs::write(&txt, text).map_err(|e| format!("{}: {e}", txt.display()))?;
    saved.written.push(txt.to_string_lossy().into_owned());

    let other = match target {
        Target::Addon => install.join("dod").join("overviews"),
        Target::Game => install.join("dod_addon").join("overviews"),
    };
    if is_ours(&other.join(format!("{map}.txt"))) {
        for ext in ["txt", "bmp", "tga"] {
            let stale = other.join(format!("{map}.{ext}"));
            if stale.is_file() && std::fs::remove_file(&stale).is_ok() {
                saved.removed.push(stale.to_string_lossy().into_owned());
            }
        }
    }
    Ok(saved)
}

/// What the page hands over to write: its drawing as base64 RGBA, and where.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Export {
    pub install: String,
    pub map: String,
    pub target: Target,
    pub format: Format,
    pub width: u32,
    pub height: u32,
    /// Base64 of `width * height * 4` bytes, rows top first.
    pub rgba: String,
    pub transform: Transform,
    /// The page's edits, written beside the high-quality copy so the
    /// overview can be picked up again on another PC ([`sidecar_path`]).
    #[serde(default)]
    pub edits: Option<serde_json::Value>,
}

/// Encodes the page's drawing and writes it with [`save`].
pub fn export(request: &Export) -> Result<Saved, String> {
    use base64::Engine as _;
    let rgba = base64::engine::general_purpose::STANDARD
        .decode(request.rgba.as_bytes())
        .map_err(|e| format!("the drawing did not arrive whole: {e}"))?;
    let image = match request.format {
        Format::Tga => super::image::tga(request.width, request.height, &rgba)?,
        Format::Bmp => super::image::bmp(request.width, request.height, &rgba)?,
    };
    let install = Path::new(&request.install);
    let mut saved = save(
        install,
        request.target,
        &request.map,
        request.format,
        &image,
        &request.transform,
    )?;
    if let Some(edits) = &request.edits {
        saved
            .written
            .push(save_sidecar(install, &request.map, edits)?);
    }
    Ok(saved)
}

/// The page's edits that travel with an overview:
/// `dod_addon/overviews/<map>.dodstudio.json`, with DoD Studio's other
/// files, not in the game's `.txt` (which the game reads on joining a map).
/// Copied with the overview to another PC, it lets the page pick the edits
/// up there.
pub fn sidecar_path(install: &Path, map: &str) -> PathBuf {
    install
        .join("dod_addon")
        .join("overviews")
        .join(format!("{}.dodstudio.json", safe(map)))
}

fn save_sidecar(install: &Path, map: &str, edits: &serde_json::Value) -> Result<String, String> {
    let path = sidecar_path(install, map);
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    }
    let text = serde_json::to_string_pretty(edits).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path.to_string_lossy().into_owned())
}

/// The page's edits for `map`: this PC's own, else the ones saved beside
/// the install's overview ([`sidecar_path`]).
pub fn load_edits_for(install: Option<&Path>, map: &str) -> Option<serde_json::Value> {
    load_edits(map).or_else(|| {
        let text = std::fs::read_to_string(sidecar_path(install?, map)).ok()?;
        serde_json::from_str(&text).ok()
    })
}

/// Writes the high-quality copy, `dod_addon/overviews/<map>_hd.tga`: only
/// DoD Studio's hook reads it (cutting the game's tiles from it), so it
/// stays out of the game's own folder. Always ours, so it is simply
/// replaced, and one an older version left in `dod/overviews` is removed.
pub fn save_hd(
    install: &Path,
    map: &str,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<String, String> {
    let map = safe(map);
    let folder = install.join("dod_addon").join("overviews");
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let bytes = super::image::tga_hd(width, height, rgba)?;
    let path = folder.join(format!("{map}_hd.tga"));
    std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let old = install
        .join("dod")
        .join("overviews")
        .join(format!("{map}_hd.tga"));
    if old.is_file() {
        let _ = std::fs::remove_file(&old);
    }
    Ok(path.to_string_lossy().into_owned())
}

/// The page's saved edits for `map`, as it wrote them.
pub fn load_edits(map: &str) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(edits_path(map)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Forgets the page's edits for `map`. Nothing saved is not an error.
pub fn remove_edits(map: &str) -> Result<(), String> {
    match std::fs::remove_file(edits_path(map)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

pub fn save_edits(map: &str, edits: &serde_json::Value) -> Result<(), String> {
    let path = edits_path(map);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(edits).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    fn transform() -> Transform {
        Transform {
            zoom: 1.5,
            origin: [1.0, 2.0, 0.0],
            rotated: false,
            height: 0.0,
        }
    }

    #[test]
    fn a_users_overview_is_backed_up_once_and_ours_replace_it() {
        let dir = Scratch::new("overview_save");
        let install = dir.path();
        let overviews = install.join("dod").join("overviews");
        std::fs::create_dir_all(&overviews).unwrap();
        std::fs::write(overviews.join("dod_x.txt"), "the user's own").unwrap();
        std::fs::write(overviews.join("dod_x.bmp"), "their image").unwrap();

        let saved = save(
            install,
            Target::Game,
            "dod_x",
            Format::Tga,
            b"tga",
            &transform(),
        )
        .unwrap();
        assert_eq!(saved.backed_up.len(), 2);
        let backups = overviews.join(BACKUP_DIR);
        assert_eq!(
            std::fs::read(backups.join("dod_x.bmp")).unwrap(),
            b"their image"
        );
        assert!(
            std::fs::read_to_string(overviews.join("dod_x.txt"))
                .unwrap()
                .contains(OURS)
        );
        assert!(
            std::fs::read_to_string(overviews.join("dod_x.txt"))
                .unwrap()
                .contains("overviews/dod_x.tga")
        );

        // A second save: the txt is ours now, nothing more is backed up, and
        // the original backup is untouched.
        let again = save(
            install,
            Target::Game,
            "dod_x",
            Format::Bmp,
            b"bmp",
            &transform(),
        )
        .unwrap();
        assert!(again.backed_up.is_empty());
        assert_eq!(
            std::fs::read(backups.join("dod_x.txt")).unwrap(),
            b"the user's own"
        );
    }

    #[test]
    fn the_addon_target_writes_under_dod_addon() {
        let dir = Scratch::new("overview_addon");
        let saved = save(
            dir.path(),
            Target::Addon,
            "dod_y",
            Format::Bmp,
            b"bmp",
            &transform(),
        )
        .unwrap();
        assert!(
            dir.path()
                .join("dod_addon")
                .join("overviews")
                .join("dod_y.bmp")
                .is_file()
        );
        assert_eq!(saved.written.len(), 2);
    }

    #[test]
    fn saving_to_dod_removes_our_older_copy_in_dod_addon() {
        let dir = Scratch::new("overview_move");
        save(
            dir.path(),
            Target::Addon,
            "dod_z",
            Format::Tga,
            b"tga",
            &transform(),
        )
        .unwrap();
        let saved = save(
            dir.path(),
            Target::Game,
            "dod_z",
            Format::Tga,
            b"tga",
            &transform(),
        )
        .unwrap();
        let addon = dir.path().join("dod_addon").join("overviews");
        assert!(!addon.join("dod_z.txt").exists());
        assert!(!addon.join("dod_z.tga").exists());
        assert_eq!(saved.removed.len(), 2);
        assert!(
            dir.path()
                .join("dod")
                .join("overviews")
                .join("dod_z.tga")
                .is_file()
        );
    }

    #[test]
    fn someone_elses_overview_in_the_other_folder_is_left_alone() {
        let dir = Scratch::new("overview_keep");
        let addon = dir.path().join("dod_addon").join("overviews");
        std::fs::create_dir_all(&addon).unwrap();
        std::fs::write(addon.join("dod_w.txt"), "a server's own").unwrap();
        let saved = save(
            dir.path(),
            Target::Game,
            "dod_w",
            Format::Tga,
            b"tga",
            &transform(),
        )
        .unwrap();
        assert!(saved.removed.is_empty());
        assert!(addon.join("dod_w.txt").is_file());
    }

    #[test]
    fn the_high_quality_copy_goes_to_dod_addon() {
        let dir = Scratch::new("overview_hd");
        let game = dir.path().join("dod").join("overviews");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("dod_v_hd.tga"), "older").unwrap();
        let rgba = vec![0u8; 4 * 1024 * 768];
        let path = save_hd(dir.path(), "dod_v", 1024, 768, &rgba).unwrap();
        assert!(
            path.replace('\\', "/")
                .ends_with("dod_addon/overviews/dod_v_hd.tga")
        );
        assert!(!game.join("dod_v_hd.tga").exists());
    }

    #[test]
    fn the_edits_travel_with_the_overview() {
        use base64::Engine as _;
        let dir = Scratch::new("overview_sidecar");
        let edits = serde_json::json!({ "version": 1, "labels": [{ "text": "Church" }] });
        let request = Export {
            install: dir.path().to_string_lossy().into_owned(),
            map: "dod_u".to_string(),
            target: Target::Game,
            format: Format::Tga,
            width: 128,
            height: 128,
            rgba: base64::engine::general_purpose::STANDARD.encode(vec![0u8; 128 * 128 * 4]),
            transform: transform(),
            edits: Some(edits.clone()),
        };
        let saved = export(&request).unwrap();
        let sidecar = sidecar_path(dir.path(), "dod_u");
        assert!(sidecar.ends_with("dod_addon/overviews/dod_u.dodstudio.json"));
        assert!(
            saved
                .written
                .contains(&sidecar.to_string_lossy().into_owned())
        );
        let text = std::fs::read_to_string(&sidecar).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&text).unwrap(),
            edits
        );
        // Nothing of ours in the game's .txt beyond its one comment line.
        let txt = std::fs::read_to_string(dir.path().join("dod/overviews/dod_u.txt")).unwrap();
        assert!(!txt.contains("Church"));
    }

    #[test]
    fn maps_are_listed_from_addon_dod_and_downloads_once_each() {
        let dir = Scratch::new("overview_map_dirs");
        let install = dir.path();
        for (folder, map) in [
            ("dod", "dod_anzio"),
            ("dod", "dod_both"),
            ("dod_addon", "dod_both"),
            ("dod_downloads", "dod_saints2_b5e"),
        ] {
            let maps = install.join(folder).join("maps");
            std::fs::create_dir_all(&maps).unwrap();
            std::fs::write(maps.join(format!("{map}.bsp")), folder).unwrap();
        }

        let listed = maps(install);
        let names: Vec<&str> = listed.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["dod_anzio", "dod_both", "dod_saints2_b5e"]);
        // A map in two folders comes from the one the game loads first.
        let both = listed.iter().find(|m| m.name == "dod_both").unwrap();
        assert!(both.bsp.contains("dod_addon"));

        assert_eq!(
            std::fs::read_to_string(bsp_path(install, "dod_saints2_b5e")).unwrap(),
            "dod_downloads"
        );
        assert_eq!(
            std::fs::read_to_string(bsp_path(install, "dod_both")).unwrap(),
            "dod_addon"
        );
        assert!(bsp_path(install, "dod_missing").ends_with("dod/maps/dod_missing.bsp"));
    }

    #[test]
    fn map_names_cannot_leave_the_folder() {
        assert_eq!(safe("../../evil"), "_.._evil");
        assert_eq!(safe("dod_anzio"), "dod_anzio");
    }
}
