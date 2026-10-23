//! The test hook DLLs Configuration > Paths offers in its GoldSrc Hooks DLL
//! dropdown, in builds that use the override (`updater_manager`
//! `hook_dll_override_allowed`), so trying another build is one pick instead
//! of editing a path. They live in the main checkout's `local\` (a
//! worktree's build finds the same folder), in either of two forms:
//!
//! - `local\hook-dlls\dodstudio_goldsrc_hooks_<label>.dll`, all in one folder;
//! - `local\<folder>\dodstudio_goldsrc_hooks.dll`, a folder per build, as the
//!   `local\test-910\` folders have always been.
//!
//! The dropdown shows `<label>` or the folder's name, without a leading
//! `test`: `910` for both `dodstudio_goldsrc_hooks_910.dll` and `test-910\`.

use serde::Serialize;
use std::path::{Path, PathBuf};

const FILE_NAME: &str = "dodstudio_goldsrc_hooks.dll";
const PREFIX: &str = "dodstudio_goldsrc_hooks_";

/// One test DLL.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HookDll {
    pub label: String,
    pub path: String,
    /// The file's modified time, seconds since the epoch.
    pub modified_unix_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HookDllList {
    /// The folder for one-file test DLLs, `None` when there is no checkout.
    pub folder: Option<String>,
    /// This checkout's own release build of the hook, when it has one.
    pub checkout_build: Option<String>,
    pub dlls: Vec<HookDll>,
}

/// `name` without a leading `test`, `test-` or `test_`; `None` if nothing is
/// left.
fn without_test(name: &str) -> Option<String> {
    let rest = match name.get(..4) {
        Some(head) if head.eq_ignore_ascii_case("test") => name[4..].trim_start_matches(['-', '_']),
        _ => name,
    };
    (!rest.is_empty()).then(|| rest.to_string())
}

/// The dropdown's name for a file in `local\hook-dlls\`: what follows the
/// prefix (`dodstudio_goldsrc_hooks_test-910.dll` is `910`). `None` for any
/// other file, the plain `dodstudio_goldsrc_hooks.dll` included.
pub fn label_of(file_name: &str) -> Option<String> {
    let lower = file_name.to_ascii_lowercase();
    let stem = lower.strip_suffix(".dll")?.strip_prefix(PREFIX)?;
    // Back to the file's own case; the lowercase copy has the same length.
    without_test(&file_name[PREFIX.len()..PREFIX.len() + stem.len()])
}

/// Orders labels the way a person reads them: `99` before `910`, `910`
/// before `910b`, names without a number last.
fn natural_key(label: &str) -> (u64, String) {
    let digits: String = label.chars().take_while(|c| c.is_ascii_digit()).collect();
    let number = digits.parse().unwrap_or(u64::MAX);
    (number, label.to_ascii_lowercase())
}

/// The checkout holding `start`: for a worktree, the main checkout its
/// `.git` file points into (`<main>\.git\worktrees\<name>`).
fn main_checkout(start: &Path) -> Option<PathBuf> {
    let dot_git = start
        .ancestors()
        .map(|d| d.join(".git"))
        .find(|p| p.exists())?;
    let checkout = dot_git.parent()?.to_path_buf();
    if dot_git.is_dir() {
        return Some(checkout);
    }
    let text = std::fs::read_to_string(&dot_git).ok()?;
    let target = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
    let target = if target.is_absolute() {
        target
    } else {
        checkout.join(target)
    };
    let worktrees = target.parent()?;
    if !worktrees
        .file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("worktrees"))
    {
        return Some(checkout);
    }
    Some(worktrees.parent()?.parent()?.to_path_buf())
}

fn modified(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

/// Every test DLL under `local`, in label order.
fn dlls_in(local: &Path) -> Vec<HookDll> {
    let mut dlls = Vec::new();
    let one_folder = local.join("hook-dlls");
    if let Ok(entries) = std::fs::read_dir(&one_folder) {
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            if let Some(label) = label_of(&entry.file_name().to_string_lossy()) {
                let path = entry.path();
                dlls.push(HookDll {
                    label,
                    modified_unix_secs: modified(&path),
                    path: path.display().to_string(),
                });
            }
        }
    }
    if let Ok(entries) = std::fs::read_dir(local) {
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let path = entry.path().join(FILE_NAME);
            let name = entry.file_name().to_string_lossy().into_owned();
            // The one-file folder's own plain DLL has no label.
            if name.eq_ignore_ascii_case("hook-dlls") {
                continue;
            }
            let Some(label) = without_test(&name) else {
                continue;
            };
            if path.is_file() {
                dlls.push(HookDll {
                    label,
                    modified_unix_secs: modified(&path),
                    path: path.display().to_string(),
                });
            }
        }
    }
    dlls.sort_by(|a, b| {
        natural_key(&a.label)
            .cmp(&natural_key(&b.label))
            .then_with(|| a.path.cmp(&b.path))
    });
    dlls
}

/// Lists the test DLLs, making `local\hook-dlls\` if it isn't there yet so
/// it can be found to put them in. Nothing in a build that ignores the
/// override (an installed release).
#[tauri::command(async)]
pub fn list_hook_dlls() -> HookDllList {
    if !crate::updater_manager::hook_dll_override_allowed() {
        return HookDllList {
            folder: None,
            checkout_build: None,
            dlls: Vec::new(),
        };
    }
    let source = Path::new(env!("CARGO_MANIFEST_DIR"));
    let checkout_build = source
        .ancestors()
        .nth(2)
        .map(|repo| {
            repo.join("target")
                .join("i686-pc-windows-msvc")
                .join("release")
                .join(FILE_NAME)
        })
        .filter(|p| p.is_file())
        .map(|p| p.display().to_string());
    let Some(local) = main_checkout(source).map(|c| c.join("local")) else {
        return HookDllList {
            folder: None,
            checkout_build,
            dlls: Vec::new(),
        };
    };
    let folder = local.join("hook-dlls");
    // Best-effort: a folder that can't be made just lists nothing.
    let _ = std::fs::create_dir_all(&folder);
    HookDllList {
        folder: Some(folder.display().to_string()),
        checkout_build,
        dlls: dlls_in(&local),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    #[test]
    fn a_label_is_what_follows_the_prefix_without_test() {
        for (name, label) in [
            ("dodstudio_goldsrc_hooks_910.dll", "910"),
            ("dodstudio_goldsrc_hooks_test910.dll", "910"),
            ("dodstudio_goldsrc_hooks_test-910.dll", "910"),
            ("DODSTUDIO_GOLDSRC_HOOKS_910b.DLL", "910b"),
            ("dodstudio_goldsrc_hooks_860-edges.dll", "860-edges"),
        ] {
            assert_eq!(label_of(name).as_deref(), Some(label), "{name}");
        }
    }

    #[test]
    fn other_files_have_no_label() {
        for name in [
            "dodstudio_goldsrc_hooks.dll",
            "dodstudio_goldsrc_hooks_.dll",
            "dodstudio_goldsrc_hooks_test.dll",
            "dodstudio_goldsrc_hooks_910.pdb",
            "inject.exe",
            "hooks_910.dll",
        ] {
            assert_eq!(label_of(name), None, "{name}");
        }
    }

    #[test]
    fn labels_sort_by_number_then_text() {
        let mut labels = vec!["910", "99", "capture-wsod25d", "910b", "1000", "860-edges"];
        labels.sort_by_key(|l| natural_key(l));
        assert_eq!(
            labels,
            ["99", "860-edges", "910", "910b", "1000", "capture-wsod25d"]
        );
    }

    #[test]
    fn both_forms_are_listed_and_nothing_else() {
        let local = Scratch::new("hook_dll_list");
        let local = local.path();
        let one = local.join("hook-dlls");
        std::fs::create_dir_all(&one).unwrap();
        for name in [
            "dodstudio_goldsrc_hooks_924.dll",
            "dodstudio_goldsrc_hooks.dll",
            "notes.txt",
        ] {
            std::fs::write(one.join(name), b"x").unwrap();
        }
        for folder in ["test-910", "capture-wsod25d", "review-images"] {
            std::fs::create_dir_all(local.join(folder)).unwrap();
        }
        std::fs::write(local.join("test-910").join(FILE_NAME), b"x").unwrap();
        std::fs::write(local.join("capture-wsod25d").join(FILE_NAME), b"x").unwrap();
        let labels: Vec<String> = dlls_in(local).into_iter().map(|d| d.label).collect();
        assert_eq!(labels, ["910", "924", "capture-wsod25d"]);
        assert!(dlls_in(&local.join("missing")).is_empty());
    }

    #[test]
    fn a_worktree_finds_the_main_checkout() {
        let dir = Scratch::new("hook_dll_checkout");
        let main = dir.path().join("main");
        let tree = dir.path().join("wt");
        let gitdir = main.join(".git").join("worktrees").join("wt");
        std::fs::create_dir_all(&gitdir).unwrap();
        std::fs::create_dir_all(tree.join("studio").join("src-tauri")).unwrap();
        std::fs::write(tree.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
        assert_eq!(
            main_checkout(&tree.join("studio").join("src-tauri")),
            Some(main.clone())
        );
        std::fs::create_dir_all(main.join("studio")).unwrap();
        assert_eq!(main_checkout(&main.join("studio")), Some(main));
    }
}
