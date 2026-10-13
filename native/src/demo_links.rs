//! Demo folder links (#376): a junction under the game's `dod/demos/` that
//! points at a demo folder anywhere on the PC, so `viewdemo` can play those
//! demos in place.
//!
//! `viewdemo` and `playdemo` take a path relative to `dod/`, `../` included,
//! and nothing else: an absolute path is prepended to each search path, and
//! the Anniversary build asserts on a `:`. A relative path cannot change
//! drive, so a folder on `D:` needs a path under `dod/`; a junction gives it
//! one, with no copy and no engine change. The game follows the junction
//! like any folder, for listing (the Load Demo window, #409) and opening.
//!
//! Only a click in Studio makes one, and these rules keep a link from making
//! any folder walk loop (the walks themselves skip links, `sys::folder_walk`):
//!
//! 1. The link points at the **chosen folder**, never a drive root:
//!    `<game>\dod\demos\<name>` → `D:\matches`.
//! 2. A folder that **contains the game install, or is inside it**, is
//!    refused, as is a folder that is itself a link, or on a network path.
//! 3. The link's name is the folder's name made safe for `viewdemo`: letters,
//!    digits, `_` and `-`, with a number added when the name is taken.
//! 4. **Remove** unlinks the junction only (`sys::junction::remove`); the
//!    folder and its demos stay.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::log_markdown;
use crate::sys::junction;

/// Under `dod/`: `dod/demos/<name>` is the link.
pub const LINKS_FOLDER: &str = "demos";

/// A link as Studio lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DemoLink {
    /// The link's folder name under `dod/demos/`.
    pub name: String,
    /// The link's full path.
    pub link: String,
    /// The folder it points at.
    pub target: String,
    /// What to put in front of a demo's name in the console:
    /// `viewdemo "demos/<name>/<demo>"`.
    pub console_prefix: String,
}

/// The game folder (where `hl.exe` is) from the Half-Life Executable setting.
pub fn game_dir_of(hl_exe: &Path) -> Result<PathBuf, String> {
    let dir = hl_exe
        .parent()
        .filter(|p| p.join("dod").is_dir())
        .ok_or_else(|| {
            format!(
                "The Half-Life executable setting does not point into a game folder: {}",
                hl_exe.display()
            )
        })?;
    Ok(dir.to_path_buf())
}

/// `<game>/dod/demos`.
pub fn links_dir(game_dir: &Path) -> PathBuf {
    game_dir.join("dod").join(LINKS_FOLDER)
}

fn console_prefix(name: &str) -> String {
    format!("{LINKS_FOLDER}/{name}/")
}

/// Every link under `dod/demos/`, by name. Real folders there are not links
/// and are left out.
pub fn list(game_dir: &Path) -> Vec<DemoLink> {
    let dir = links_dir(game_dir);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut links: Vec<DemoLink> = entries
        .flatten()
        .filter_map(|entry| {
            let link = entry.path();
            let target = junction::target(&link)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            Some(DemoLink {
                console_prefix: console_prefix(&name),
                name,
                link: link.to_string_lossy().into_owned(),
                target: target.to_string_lossy().into_owned(),
            })
        })
        .collect();
    links.sort_by_key(|l| l.name.to_lowercase());
    links
}

/// `path` lowercased with one trailing `\`, so "inside" is a prefix test.
fn folder_key(path: &Path) -> String {
    let mut key = path.to_string_lossy().to_lowercase();
    while key.ends_with('\\') || key.ends_with('/') {
        key.pop();
    }
    key.push('\\');
    key
}

/// Why `target` cannot be linked, or its plain full path when it can.
pub fn check_target(game_dir: &Path, target: &Path) -> Result<PathBuf, String> {
    let shown = target.display();
    let meta = std::fs::symlink_metadata(target)
        .map_err(|_| format!("Cannot link {shown}: the folder is not there"))?;
    if meta.file_type().is_symlink() {
        return Err(format!(
            "Cannot link {shown}: it is itself a link. Link the folder it points at instead"
        ));
    }
    if !meta.is_dir() {
        return Err(format!("Cannot link {shown}: it is not a folder"));
    }
    let full = junction::plain_canonical(target)
        .map_err(|e| format!("Cannot link {shown}: the full path could not be read ({e})"))?;
    let text = full.to_string_lossy().into_owned();
    if text.starts_with(r"\\") {
        return Err(format!(
            "Cannot link {text}: it is on the network. A link needs a folder on a drive of this PC"
        ));
    }
    let is_drive_root = full.components().all(|c| {
        matches!(
            c,
            std::path::Component::Prefix(_) | std::path::Component::RootDir
        )
    });
    if is_drive_root {
        return Err(format!(
            "Cannot link {text}: that is a whole drive. Pick the folder the demos are in"
        ));
    }
    let game = junction::plain_canonical(game_dir)
        .map_err(|e| format!("Cannot read the game folder {} ({e})", game_dir.display()))?;
    let (target_key, game_key) = (folder_key(&full), folder_key(&game));
    if game_key.starts_with(&target_key) {
        return Err(format!(
            "Cannot link {text}: the game is installed inside it, so the game would see itself inside its own demo folder, without end"
        ));
    }
    if target_key.starts_with(&game_key) {
        return Err(format!(
            "Cannot link {text}: it is inside the game folder already. The game can play those demos by their path"
        ));
    }
    Ok(full)
}

/// `name` with everything but letters, digits, `_` and `-` turned into `_`,
/// runs of them folded, so `viewdemo` takes it unquoted and `-` only ever
/// sits inside the name. Empty comes back as `demos`.
pub fn safe_name(name: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' {
            out.push(ch);
            gap = false;
        } else if !gap {
            out.push('_');
            gap = true;
        }
    }
    let trimmed = out.trim_matches('_').trim_matches('-').to_string();
    if trimmed.is_empty() {
        "demos".to_string()
    } else {
        trimmed
    }
}

/// `base`, or `base-2`, `base-3`, ... the first not in `taken` (compared
/// without case, as the file system does).
pub fn free_name(base: &str, taken: &[String]) -> String {
    let is_taken = |name: &str| taken.iter().any(|t| t.eq_ignore_ascii_case(name));
    if !is_taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|name| !is_taken(name))
        .unwrap_or_else(|| base.to_string())
}

/// Links `target` under `dod/demos/`. A link to that folder that is already
/// there is given back as is.
pub fn add(game_dir: &Path, target: &Path) -> Result<DemoLink, String> {
    let full = check_target(game_dir, target).inspect_err(|e| {
        log_markdown(&format!("[demo link] Refused: {e}"));
    })?;
    let dir = links_dir(game_dir);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Cannot make the links folder {} ({e})", dir.display()))?;

    let full_key = folder_key(&full);
    if let Some(existing) = list(game_dir)
        .into_iter()
        .find(|l| folder_key(Path::new(&l.target)) == full_key)
    {
        log_markdown(&format!(
            "[demo link] {} is already linked as {}",
            full.display(),
            existing.link
        ));
        return Ok(existing);
    }

    let taken: Vec<String> = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let base = safe_name(&full.file_name().unwrap_or_default().to_string_lossy());
    let name = free_name(&base, &taken);
    let link = dir.join(&name);
    junction::create(&link, &full).map_err(|e| {
        let text = format!(
            "Could not link {} as {} ({e})",
            full.display(),
            link.display()
        );
        log_markdown(&format!("[demo link] {text}"));
        text
    })?;
    log_markdown(&format!(
        "[demo link] Linked {} as {}: in the game, viewdemo \"{}<demo>\"",
        full.display(),
        link.display(),
        console_prefix(&name)
    ));
    Ok(DemoLink {
        console_prefix: console_prefix(&name),
        name,
        link: link.to_string_lossy().into_owned(),
        target: full.to_string_lossy().into_owned(),
    })
}

/// Unlinks `dod/demos/<name>`. The folder it pointed at is not touched.
pub fn remove(game_dir: &Path, name: &str) -> Result<(), String> {
    if name.is_empty() || name.contains(['\\', '/']) || name == "." || name == ".." {
        return Err(format!("Not a link name: {name}"));
    }
    let link = links_dir(game_dir).join(name);
    let target = junction::target(&link)
        .ok_or_else(|| format!("{} is not a demo folder link", link.display()))?;
    junction::remove(&link).map_err(|e| {
        let text = format!("Could not remove the link {} ({e})", link.display());
        log_markdown(&format!("[demo link] {text}"));
        text
    })?;
    log_markdown(&format!(
        "[demo link] Removed the link {} (the folder {} is untouched)",
        link.display(),
        target.display()
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch root with a fake game install and a demo folder beside it.
    struct Scratch {
        root: PathBuf,
        game: PathBuf,
        matches: PathBuf,
    }

    impl Scratch {
        fn new(name: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("dodstudio_links_{name}_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let game = root.join("Half-Life");
            std::fs::create_dir_all(game.join("dod")).unwrap();
            std::fs::write(game.join("hl.exe"), b"").unwrap();
            let matches = root.join("drive").join("Philly LAN & more");
            std::fs::create_dir_all(matches.join("day2")).unwrap();
            std::fs::write(matches.join("m1.dem"), b"demo").unwrap();
            std::fs::write(matches.join("day2/m2.dem"), b"demo").unwrap();
            Self {
                root,
                game,
                matches,
            }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            for link in list(&self.game) {
                let _ = junction::remove(Path::new(&link.link));
            }
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn names_are_made_safe_for_viewdemo() {
        assert_eq!(safe_name("Philly LAN & more"), "Philly_LAN_more");
        assert_eq!(safe_name("wsod-25"), "wsod-25");
        assert_eq!(safe_name("  ..  "), "demos");
        assert_eq!(safe_name("Mäd demos!"), "M_d_demos");
        assert_eq!(free_name("matches", &[]), "matches");
        assert_eq!(
            free_name("matches", &["MATCHES".into(), "matches-2".into()]),
            "matches-3"
        );
    }

    #[test]
    fn links_the_folder_lists_it_and_plays_through_it() {
        let s = Scratch::new("add");
        let link = add(&s.game, &s.matches).unwrap();
        assert_eq!(link.name, "Philly_LAN_more");
        assert_eq!(link.console_prefix, "demos/Philly_LAN_more/");
        assert_eq!(
            Path::new(&link.link),
            s.game.join("dod/demos/Philly_LAN_more")
        );
        // What viewdemo "demos/Philly_LAN_more/day2/m2" opens.
        assert_eq!(
            std::fs::read(s.game.join("dod/demos/Philly_LAN_more/day2/m2.dem")).unwrap(),
            b"demo"
        );
        assert_eq!(list(&s.game), vec![link.clone()]);
        // Again: the same link, not a second one.
        assert_eq!(add(&s.game, &s.matches).unwrap(), link);
        assert_eq!(list(&s.game).len(), 1);
    }

    #[test]
    fn a_taken_name_gets_a_number_and_a_real_folder_is_not_listed() {
        let s = Scratch::new("taken");
        std::fs::create_dir_all(s.game.join("dod/demos/matches")).unwrap();
        let other = s.root.join("elsewhere/matches");
        std::fs::create_dir_all(&other).unwrap();
        let link = add(&s.game, &other).unwrap();
        assert_eq!(link.name, "matches-2");
        let listed = list(&s.game);
        assert_eq!(listed.len(), 1, "the real `matches` folder is no link");
        assert_eq!(listed[0].name, "matches-2");
    }

    #[test]
    fn remove_unlinks_and_leaves_the_demos_alone() {
        let s = Scratch::new("remove");
        let link = add(&s.game, &s.matches).unwrap();
        remove(&s.game, &link.name).unwrap();
        assert!(list(&s.game).is_empty());
        assert!(std::fs::symlink_metadata(&link.link).is_err());
        assert_eq!(std::fs::read(s.matches.join("m1.dem")).unwrap(), b"demo");
        assert_eq!(
            std::fs::read(s.matches.join("day2/m2.dem")).unwrap(),
            b"demo"
        );
        // A real folder under dod/demos is never removed through here.
        std::fs::create_dir_all(s.game.join("dod/demos/real")).unwrap();
        assert!(remove(&s.game, "real").is_err());
        assert!(s.game.join("dod/demos/real").is_dir());
        assert!(remove(&s.game, "../dod").is_err());
    }

    #[test]
    fn refuses_the_install_its_parents_its_insides_a_link_and_a_drive() {
        let s = Scratch::new("refuse");
        let inside = |e: &str| e.contains("inside the game folder");
        let contains = |e: &str| e.contains("installed inside it");
        assert!(
            contains(&add(&s.game, &s.game).unwrap_err()),
            "the install itself"
        );
        assert!(contains(&add(&s.game, &s.root).unwrap_err()), "a parent");
        assert!(inside(&add(&s.game, &s.game.join("dod")).unwrap_err()));
        // The same install reached through a junction is still the install.
        let via = s.root.join("via");
        junction::create(&via, &s.game.join("dod")).unwrap();
        let err = add(&s.game, &via).unwrap_err();
        assert!(err.contains("itself a link"), "{err}");
        let _ = junction::remove(&via);
        let drive = s.root.components().take(2).collect::<PathBuf>();
        let err = add(&s.game, &drive).unwrap_err();
        assert!(err.contains("whole drive"), "{err}");
        let err = add(&s.game, &s.matches.join("m1.dem")).unwrap_err();
        assert!(err.contains("not a folder"), "{err}");
        assert!(add(&s.game, &s.root.join("missing")).is_err());
        assert!(list(&s.game).is_empty());
    }

    #[test]
    fn the_game_dir_comes_from_hl_exe() {
        let s = Scratch::new("game_dir");
        assert_eq!(game_dir_of(&s.game.join("hl.exe")).unwrap(), s.game);
        assert!(
            game_dir_of(&s.root.join("hl.exe")).is_err(),
            "no dod/ beside it"
        );
    }
}
