//! The Half-Life installs on this PC (#931): every folder under a Steam
//! library's `steamapps\common` holding an `hl.exe` and a `dod` folder, with
//! the engine build of the `hw.dll` beside it. Configuration › Paths lists
//! them to switch between; the Overviews page (#371) reads the same library
//! folders ([`common_dirs`]).

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Steam's default folder, for a PC whose registry doesn't say.
const DEFAULT_STEAM: &str = r"C:\Program Files (x86)\Steam";

/// One install, as the picker shows it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FoundInstall {
    /// The install's folder name ("Half-Life - PRE-Anniversary for Movies").
    pub name: String,
    /// The install's folder.
    pub path: String,
    /// Its `hl.exe`.
    pub hl_exe: String,
    /// `"anniversary"` or `"pre_anniversary"`, from its `hw.dll`
    /// ([`super::packet_limit`]); `None` when that can't be read.
    pub build: Option<&'static str>,
}

/// The library folders `libraryfolders.vdf` names: every `"path"` value,
/// with the file's doubled backslashes made single.
pub fn library_paths(vdf: &str) -> Vec<PathBuf> {
    vdf.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('"').collect();
            (parts.len() >= 5 && parts[1].eq_ignore_ascii_case("path"))
                .then(|| PathBuf::from(parts[3].replace(r"\\", r"\")))
        })
        .collect()
}

/// `steamapps\common` folders, each once: the one holding `hint` (an
/// `hl.exe` or an install folder, so a library Steam's own list misses is
/// still found), Steam's own (`steam`), and every library in its
/// `libraryfolders.vdf`.
pub fn common_dirs_from(steam: &Path, hint: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(common) = hint.and_then(|h| {
        h.ancestors().find(|a| {
            a.file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("common"))
        })
    }) {
        dirs.push(common.to_path_buf());
    }
    let steamapps = steam.join("steamapps");
    dirs.push(steamapps.join("common"));
    if let Ok(vdf) = std::fs::read_to_string(steamapps.join("libraryfolders.vdf")) {
        dirs.extend(
            library_paths(&vdf)
                .into_iter()
                .map(|library| library.join("steamapps").join("common")),
        );
    }
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| seen.insert(key(d)));
    dirs
}

/// [`common_dirs_from`] with Steam's folder from the registry
/// (`SteamPath`, else the folder of `SteamExe`), else the default one.
pub fn common_dirs(hint: Option<&Path>) -> Vec<PathBuf> {
    let steam = super::steam::steam_dir()
        .map(|p| tidy(&p))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_STEAM));
    common_dirs_from(&steam, hint)
}

/// The registry's `c:/program files (x86)/steam` as the folder's own
/// spelling (`C:\Program Files (x86)\Steam`), so the hl.exe paths saved from
/// it read like a browsed one. Unchanged when it can't be resolved.
fn tidy(path: &Path) -> PathBuf {
    let Ok(real) = std::fs::canonicalize(path) else {
        return path.to_path_buf();
    };
    let text = real.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

/// A path compared the way Windows does: any case, either slash.
fn key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', r"\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

/// Every install in `commons`, sorted by name. The same folder reached
/// through two spellings is listed once.
pub fn find_in(commons: &[PathBuf]) -> Vec<FoundInstall> {
    let mut out: Vec<FoundInstall> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for common in commons {
        let Ok(entries) = std::fs::read_dir(common) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let hl_exe = path.join("hl.exe");
            if !hl_exe.is_file() || !path.join("dod").is_dir() || !seen.insert(key(&path)) {
                continue;
            }
            out.push(FoundInstall {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: path.to_string_lossy().into_owned(),
                build: build_of(&hl_exe),
                hl_exe: hl_exe.to_string_lossy().into_owned(),
            });
        }
    }
    out.sort_by_key(|i| i.name.to_lowercase());
    out
}

/// Every install on this PC ([`common_dirs`], [`find_in`]).
pub fn find(hint: Option<&Path>) -> Vec<FoundInstall> {
    find_in(&common_dirs(hint))
}

fn build_of(hl_exe: &Path) -> Option<&'static str> {
    match super::packet_limit::packet_entity_limit(hl_exe)? {
        super::packet_limit::ANNIVERSARY_PACKET_ENTITIES => Some("anniversary"),
        super::packet_limit::PRE_ANNIVERSARY_PACKET_ENTITIES => Some("pre_anniversary"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    /// A Steam folder's `libraryfolders.vdf`, as Steam writes it.
    const VDF: &str = r#""libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"label"		""
		"apps"
		{
			"70"		"0"
		}
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
	}
}
"#;

    #[test]
    fn reads_every_library_path_with_single_backslashes() {
        assert_eq!(
            library_paths(VDF),
            vec![
                PathBuf::from(r"C:\Program Files (x86)\Steam"),
                PathBuf::from(r"D:\SteamLibrary"),
            ]
        );
        assert!(library_paths("not a vdf").is_empty());
    }

    /// An install folder with `hl.exe`, `dod\` and a `hw.dll` whose bytes
    /// say which build it is (`None`: no `hw.dll`).
    fn install(common: &Path, name: &str, hw_dll: Option<&[u8]>) {
        let dir = common.join(name);
        std::fs::create_dir_all(dir.join("dod")).unwrap();
        std::fs::write(dir.join("hl.exe"), b"MZ").unwrap();
        if let Some(bytes) = hw_dll {
            std::fs::write(dir.join("hw.dll"), bytes).unwrap();
        }
    }

    #[test]
    fn finds_the_installs_in_every_library_with_their_build() {
        let root = Scratch::new("installs_find");
        let steam = root.join("Steam");
        let library = root.join("Library");
        let common = steam.join("steamapps").join("common");
        let other = library.join("steamapps").join("common");
        std::fs::create_dir_all(&common).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        // Steam writes its own paths with doubled backslashes.
        let vdf = format!(
            "\"libraryfolders\"\n{{\n\t\"1\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n}}\n",
            library.display().to_string().replace('\\', r"\\")
        );
        std::fs::write(steam.join("steamapps").join("libraryfolders.vdf"), vdf).unwrap();

        install(
            &common,
            "Half-Life",
            Some(b"MAX_PACKET_ENTITIES sv_allow_shaders"),
        );
        install(
            &other,
            "Half-Life - PRE",
            Some(b"xx MAX_PACKET_ENTITIES xx"),
        );
        install(&other, "No hw.dll", None);
        // Not installs: no dod folder, no hl.exe.
        std::fs::create_dir_all(common.join("Counter-Strike")).unwrap();
        std::fs::write(common.join("Counter-Strike").join("hl.exe"), b"MZ").unwrap();
        std::fs::create_dir_all(common.join("Empty").join("dod")).unwrap();

        let dirs = common_dirs_from(&steam, None);
        assert_eq!(dirs, vec![common.clone(), other.clone()]);
        let found = find_in(&dirs);
        let names: Vec<(&str, Option<&str>)> =
            found.iter().map(|i| (i.name.as_str(), i.build)).collect();
        assert_eq!(
            names,
            vec![
                ("Half-Life", Some("anniversary")),
                ("Half-Life - PRE", Some("pre_anniversary")),
                ("No hw.dll", None),
            ]
        );
        assert_eq!(
            found[1].hl_exe,
            other
                .join("Half-Life - PRE")
                .join("hl.exe")
                .to_string_lossy()
        );
    }

    #[test]
    fn the_current_hl_exe_adds_its_library_once() {
        let root = Scratch::new("installs_hint");
        let steam = root.join("Steam");
        let elsewhere = root.join("Elsewhere").join("steamapps").join("common");
        install(&elsewhere, "Half-Life", None);
        let hint = elsewhere.join("Half-Life").join("hl.exe");
        let dirs = common_dirs_from(&steam, Some(&hint));
        assert_eq!(dirs[0], elsewhere);
        // Steam's own common, even though it doesn't exist here.
        assert_eq!(dirs.len(), 2);
        // The same folder twice (another spelling) is listed once.
        let twice = [
            elsewhere.clone(),
            PathBuf::from(format!("{}/", elsewhere.display())),
        ];
        assert_eq!(find_in(&twice).len(), 1);
    }

    /// The real installs, where this PC has them.
    #[test]
    #[ignore]
    fn the_installs_on_this_pc() {
        for install in find(None) {
            println!(
                "{} | {:?} | {}",
                install.name, install.build, install.hl_exe
            );
        }
    }
}
