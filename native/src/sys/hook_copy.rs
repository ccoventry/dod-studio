//! The game launches from a copy of the hook DLL (#1146), so neither the file
//! Studio ships nor a `local\test-*` build is ever locked by an open game:
//! a test DLL can be rebuilt in place, and Studio updated or uninstalled,
//! with DoD running.
//!
//! Windows refuses to overwrite or delete a DLL a process has loaded (#1143),
//! so each build gets its own folder under `%LOCALAPPDATA%\dod-studio\hooks`,
//! named for its build id and a hash of its bytes: a rebuild makes a new
//! folder rather than writing over a copy a game still runs. Beside each
//! copy, [`SOURCE_FILE`] names the file it was copied from, so the running-
//! game check can still name the folder the user picked.
//!
//! The hook resolves nothing relative to its own path (its data comes from
//! `%APPDATA%\dod-studio`, game files from `hl.exe`'s folder), so the DLL is
//! the only file a copy needs.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// The folder under Studio's local app-data folder that holds the copies.
pub const COPIES_DIR: &str = "hooks";

/// Beside each copy: the path of the file it was copied from. Rewritten on
/// every launch from that copy, so its time says when the copy was last used.
pub const SOURCE_FILE: &str = "dodstudio_hook_source.txt";

/// How long a copy is kept after its last launch even when no game has it
/// loaded: covers a game another launch is still starting, which has not
/// loaded its DLL yet.
const KEEP_AFTER_LAST_USE: Duration = Duration::from_secs(10 * 60);

/// The longest build id kept in a folder name; ids are far shorter.
const MAX_ID_IN_NAME: usize = 64;

/// The DLL a launch injects for `source`: a copy of it, or `source` itself
/// when no copy could be made, so a launch never fails over the copy. Says
/// which in the activity log, and removes copies no game uses any more.
pub fn for_launch(source: &Path) -> PathBuf {
    let root = crate::shared::paths::get_local_appdata_dir().join(COPIES_DIR);
    match launch_copy_in(&root, source) {
        Ok(copy) => {
            let removed = match folders_games_run() {
                Some(mut keep) => {
                    keep.extend(copy.parent().map(Path::to_path_buf));
                    remove_unused(&root, &keep, SystemTime::now())
                }
                None => 0,
            };
            if removed > 0 {
                crate::log_markdown(&format!(
                    "goldsrc-hooks: removed {removed} hook DLL cop{} no game uses from {}",
                    if removed == 1 { "y" } else { "ies" },
                    root.display()
                ));
            }
            crate::log_markdown(&format!(
                "goldsrc-hooks: launching from a copy, {} (of {})",
                copy.display(),
                source.display()
            ));
            copy
        }
        Err(e) => {
            crate::log_markdown(&format!(
                "⚠️ goldsrc-hooks: {e}; launching from {} itself, which stays locked while the game runs",
                source.display()
            ));
            source.to_path_buf()
        }
    }
}

/// Copies `source` into its own folder under `root` (or finds the copy an
/// earlier launch made) and returns the copy's path.
pub fn launch_copy_in(root: &Path, source: &Path) -> Result<PathBuf, String> {
    let bytes = std::fs::read(source)
        .map_err(|e| format!("could not read the hook DLL {}: {e}", source.display()))?;
    let name = source
        .file_name()
        .ok_or_else(|| format!("{} names no file", source.display()))?;
    let dir = root.join(folder_name(&bytes));
    let copy = dir.join(name);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not make the folder {}: {e}", dir.display()))?;
    let up_to_date = std::fs::read(&copy).is_ok_and(|existing| existing == bytes);
    if !up_to_date {
        // Written beside it and renamed into place, so a copy cut short by a
        // full disk or a crash is never launched.
        let partial = dir.join(format!("{}.partial", name.to_string_lossy()));
        std::fs::write(&partial, &bytes)
            .and_then(|()| std::fs::rename(&partial, &copy))
            .map_err(|e| format!("could not copy the hook DLL to {}: {e}", copy.display()))?;
    }
    std::fs::write(dir.join(SOURCE_FILE), source.to_string_lossy().as_bytes())
        .map_err(|e| format!("could not write {} in {}: {e}", SOURCE_FILE, dir.display()))?;
    Ok(copy)
}

/// A build's folder: its id (`sys::hook_build`) and the start of a SHA-256
/// of its bytes. The id makes the folder readable; the hash keeps two
/// different files apart when the id doesn't (a build from before ids, or one
/// commit built with other settings).
fn folder_name(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(bytes);
    let hash: String = hash[..8].iter().map(|b| format!("{b:02x}")).collect();
    let id = crate::sys::hook_build::build_id_in(bytes).unwrap_or_else(|| "no-id".into());
    let id: String = id
        .chars()
        .take(MAX_ID_IN_NAME)
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{id}_{hash}")
}

/// The file the copy at `dll` was made from, read from [`SOURCE_FILE`]
/// beside it, or `None` when `dll` is not a launch copy.
pub fn source_of(dll: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(dll.parent()?.join(SOURCE_FILE)).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| PathBuf::from(text))
}

/// The folders of the hook DLLs every running game has loaded, or `None`
/// when a game's modules could not be read: then no copy is known to be
/// free, and none is removed.
fn folders_games_run() -> Option<Vec<PathBuf>> {
    let mut folders = Vec::new();
    for pid in crate::sys::process::game_pids() {
        let modules = crate::sys::modules::loaded_modules(pid)?;
        folders.extend(
            modules
                .iter()
                .filter(|m| {
                    m.file_name().is_some_and(|n| {
                        n.to_string_lossy()
                            .eq_ignore_ascii_case(crate::patch::batch_reuse::HOOK_DLL_NAME)
                    })
                })
                .filter_map(|m| m.parent().map(Path::to_path_buf)),
        );
    }
    Some(folders)
}

/// Removes every copy under `root` but those in `keep` (the one being
/// launched, the ones running games have loaded) that was not launched from
/// in the last [`KEEP_AFTER_LAST_USE`]; returns how many went. Windows also
/// refuses to delete a loaded DLL, so a copy whose DLL won't delete stays
/// for a later launch to remove.
pub fn remove_unused(root: &Path, keep: &[PathBuf], now: SystemTime) -> usize {
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };
    let same = |a: &Path, b: &Path| {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() || keep.iter().any(|k| same(k, &dir)) {
            continue;
        }
        let last_use = std::fs::metadata(dir.join(SOURCE_FILE))
            .or_else(|_| std::fs::metadata(&dir))
            .and_then(|m| m.modified());
        let recent = last_use
            .is_ok_and(|t| now.duration_since(t).unwrap_or_default() < KEEP_AFTER_LAST_USE);
        if recent || !dlls_deleted(&dir) {
            continue;
        }
        if std::fs::remove_dir_all(&dir).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Deletes the DLLs in `dir`, and says whether none is left: `false` when one
/// could not be deleted because a game has it loaded.
fn dlls_deleted(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().all(|e| {
        let path = e.path();
        let is_dll = path
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("dll"));
        !is_dll || std::fs::remove_file(&path).is_ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DLL_NAME: &str = "dodstudio_goldsrc_hooks.dll";

    /// A fresh folder for one test, with a source DLL in a `build` folder.
    fn setup(test: &str, dll: &[u8]) -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("hook_copy_{test}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let build = base.join("build");
        std::fs::create_dir_all(&build).unwrap();
        let source = build.join(DLL_NAME);
        std::fs::write(&source, dll).unwrap();
        (base.join("copies"), source)
    }

    fn copies(root: &Path) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(root)
            .map(|e| e.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        dirs.sort();
        dirs
    }

    #[test]
    fn a_launch_copy_has_the_source_s_bytes_name_and_path() {
        let (root, source) = setup("copy", b"MZ DSHOOKBUILD:0.10.0+abc123\0 code");
        let copy = launch_copy_in(&root, &source).unwrap();
        assert_eq!(copy.file_name().unwrap(), DLL_NAME);
        assert_eq!(std::fs::read(&copy).unwrap(), std::fs::read(&source).unwrap());
        assert_eq!(source_of(&copy), Some(source.clone()));
        let folder = copy.parent().unwrap().file_name().unwrap().to_string_lossy();
        assert!(folder.starts_with("0.10.0+abc123_"), "{folder}");
        // The same file again: the same copy, nothing new made.
        assert_eq!(launch_copy_in(&root, &source).unwrap(), copy);
        assert_eq!(copies(&root).len(), 1);
    }

    /// The point of #1146: the source can be rebuilt while a game holds the
    /// copy, and the new build gets its own copy rather than overwriting it.
    #[cfg(windows)]
    #[test]
    fn a_rebuild_while_a_game_holds_the_copy_gets_a_new_copy() {
        use std::os::windows::fs::OpenOptionsExt;
        let (root, source) = setup("rebuild", b"MZ DSHOOKBUILD:0.10.0+aaa\0 one");
        let first = launch_copy_in(&root, &source).unwrap();
        // Open without delete or write sharing, as a loaded DLL is held.
        const FILE_SHARE_READ: u32 = 1;
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&first)
            .unwrap();
        std::fs::write(&source, b"MZ DSHOOKBUILD:0.10.0+bbb\0 two").unwrap();
        let second = launch_copy_in(&root, &source).unwrap();
        assert_ne!(first, second);
        assert_eq!(std::fs::read(&second).unwrap(), b"MZ DSHOOKBUILD:0.10.0+bbb\0 two");
        // Long after both launches, only the held copy survives a clean-up.
        let later = SystemTime::now() + KEEP_AFTER_LAST_USE * 2;
        assert_eq!(remove_unused(&root, &[], later), 1);
        assert_eq!(copies(&root), vec![first.parent().unwrap().to_path_buf()]);
        drop(held);
        assert_eq!(remove_unused(&root, &[], later), 1);
        assert!(copies(&root).is_empty());
    }

    #[test]
    fn two_files_with_one_id_or_none_get_their_own_copies() {
        let (root, source) = setup("same_id", b"MZ DSHOOKBUILD:0.10.0+aaa\0 one");
        let a = launch_copy_in(&root, &source).unwrap();
        std::fs::write(&source, b"MZ DSHOOKBUILD:0.10.0+aaa\0 two").unwrap();
        let b = launch_copy_in(&root, &source).unwrap();
        assert_ne!(a, b);
        std::fs::write(&source, b"MZ an old build").unwrap();
        let c = launch_copy_in(&root, &source).unwrap();
        let folder = c.parent().unwrap().file_name().unwrap().to_string_lossy();
        assert!(folder.starts_with("no-id_"), "{folder}");
    }

    #[test]
    fn the_copy_being_launched_and_recent_ones_are_kept() {
        let (root, source) = setup("keep", b"MZ DSHOOKBUILD:0.10.0+aaa\0 one");
        let a = launch_copy_in(&root, &source).unwrap();
        std::fs::write(&source, b"MZ DSHOOKBUILD:0.10.0+bbb\0 two").unwrap();
        let b = launch_copy_in(&root, &source).unwrap();
        let keep = vec![b.parent().unwrap().to_path_buf()];
        // Just launched: both kept.
        assert_eq!(remove_unused(&root, &keep, SystemTime::now()), 0);
        assert_eq!(copies(&root).len(), 2);
        // Later: the one being launched is kept, the other goes.
        let later = SystemTime::now() + KEEP_AFTER_LAST_USE * 2;
        assert_eq!(remove_unused(&root, &keep, later), 1);
        assert_eq!(copies(&root), vec![b.parent().unwrap().to_path_buf()]);
        assert!(!a.exists());
    }

    #[test]
    fn odd_characters_in_an_id_do_not_reach_the_folder_name() {
        let name = folder_name(b"DSHOOKBUILD:1.0+a/b:c\0");
        assert!(name.starts_with("1.0+a_b_c_"), "{name}");
        assert_eq!(name.len(), "1.0+a_b_c_".len() + 16);
    }

    #[test]
    fn a_dll_that_is_not_a_copy_has_no_source() {
        let (_, source) = setup("not_a_copy", b"MZ");
        assert_eq!(source_of(&source), None);
    }

    #[test]
    fn a_missing_source_is_an_error_naming_it() {
        let (root, source) = setup("missing", b"MZ");
        let missing = source.with_file_name("gone.dll");
        let err = launch_copy_in(&root, &missing).unwrap_err();
        assert!(err.contains("gone.dll"), "{err}");
    }
}
