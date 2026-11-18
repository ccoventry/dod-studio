//! Putting DoD Studio's VEGAS Pro scripts into VEGAS's script menu (#429,
//! #1514). VEGAS lists the scripts in `Documents\Vegas Script Menu`; the app
//! ships the scripts in the repo's `vegas` folder and copies them there on
//! request.

use std::path::{Path, PathBuf};

/// The scripts' file names, in the repo's `vegas` folder, the app's bundle
/// and VEGAS's menu folder: the nameplates (#429) and the relink (#1514).
/// Only files of these names are ever overwritten.
pub const SCRIPT_FILES: [&str; 2] = ["DoD Studio Nameplates.cs", "DoD Studio Relink Clips.cs"];

/// The folder VEGAS reads its script menu from, inside `documents`.
const MENU_FOLDER: &str = "Vegas Script Menu";

/// The scripts as the repo has them, for a dev build, which has no bundled copy.
pub fn dev_script_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("vegas")
}

/// The folder to install from: the app's bundled copy (`vegas-scripts` in its
/// resources), else the repo's; a debug build tries the repo's first, as
/// [`crate::blender::scripts_dir`] does. Only a folder holding every one of
/// [`SCRIPT_FILES`] counts, so an older bundle never installs half of them.
/// `None` when neither has them all.
pub fn script_source(bundled_dir: Option<&Path>) -> Option<PathBuf> {
    let bundled = bundled_dir.map(Path::to_path_buf);
    let order = if cfg!(debug_assertions) {
        [Some(dev_script_dir()), bundled]
    } else {
        [bundled, Some(dev_script_dir())]
    };
    order
        .into_iter()
        .flatten()
        .find(|dir| SCRIPT_FILES.iter().all(|name| dir.join(name).is_file()))
}

/// The user's real Documents folder (it may be redirected, for example to
/// OneDrive), from Windows' known-folder lookup.
pub fn documents_dir() -> Result<PathBuf, String> {
    dirs::document_dir().ok_or_else(|| crate::messages::VEGAS_NO_DOCUMENTS.to_string())
}

/// Copies each of [`SCRIPT_FILES`] from `source_dir` into
/// `documents\Vegas Script Menu`, creating that folder, and replaces earlier
/// copies of them. Nothing else in the folder is touched. Returns the menu
/// folder.
pub fn install_scripts(source_dir: &Path, documents: &Path) -> Result<PathBuf, String> {
    let folder = documents.join(MENU_FOLDER);
    std::fs::create_dir_all(&folder).map_err(|e| crate::messages::labeled(folder.display(), e))?;
    for name in SCRIPT_FILES {
        let source = source_dir.join(name);
        let target = folder.join(name);
        std::fs::copy(&source, &target).map_err(|e| {
            crate::messages::labeled(format!("{} to {}", source.display(), target.display()), e)
        })?;
    }
    Ok(folder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    fn sources_in(dir: &Path, text: &str) -> PathBuf {
        let folder = dir.join("src");
        std::fs::create_dir_all(&folder).unwrap();
        for name in SCRIPT_FILES {
            std::fs::write(folder.join(name), format!("{text} {name}")).unwrap();
        }
        folder
    }

    #[test]
    fn install_creates_the_menu_folder_and_copies_every_script() {
        let scratch = Scratch::new("vegas_install_new");
        let source = sources_in(scratch.path(), "v1");
        let docs = scratch.path().join("Documents");
        let installed = install_scripts(&source, &docs).unwrap();
        assert_eq!(installed, docs.join("Vegas Script Menu"));
        for name in SCRIPT_FILES {
            assert_eq!(
                std::fs::read_to_string(installed.join(name)).unwrap(),
                format!("v1 {name}")
            );
        }
    }

    #[test]
    fn install_replaces_older_copies_and_leaves_other_scripts_alone() {
        let scratch = Scratch::new("vegas_install_overwrite");
        let docs = scratch.path().join("Documents");
        let menu = docs.join("Vegas Script Menu");
        std::fs::create_dir_all(&menu).unwrap();
        std::fs::write(menu.join(SCRIPT_FILES[0]), "old").unwrap();
        std::fs::write(menu.join("Other.cs"), "theirs").unwrap();
        let source = sources_in(scratch.path(), "new");
        install_scripts(&source, &docs).unwrap();
        assert_eq!(
            std::fs::read_to_string(menu.join(SCRIPT_FILES[0])).unwrap(),
            format!("new {}", SCRIPT_FILES[0])
        );
        assert_eq!(
            std::fs::read_to_string(menu.join("Other.cs")).unwrap(),
            "theirs"
        );
    }

    #[test]
    fn a_missing_source_error_names_both_paths() {
        let scratch = Scratch::new("vegas_install_missing");
        let docs = scratch.path().join("Documents");
        let source = scratch.path().join("nope");
        let err = install_scripts(&source, &docs).unwrap_err();
        assert!(
            err.contains(&source.join(SCRIPT_FILES[0]).display().to_string()),
            "{err}"
        );
        assert!(err.contains("Vegas Script Menu"), "{err}");
    }

    #[test]
    fn an_uncreatable_menu_folder_error_names_the_folder() {
        let scratch = Scratch::new("vegas_install_blocked");
        let source = sources_in(scratch.path(), "x");
        // A file where the Documents folder should be.
        let docs = scratch.path().join("Documents");
        std::fs::write(&docs, "file").unwrap();
        let err = install_scripts(&source, &docs).unwrap_err();
        let folder = docs.join("Vegas Script Menu");
        assert!(err.contains(&folder.display().to_string()), "{err}");
    }

    #[test]
    fn the_repo_holds_every_script() {
        let found = script_source(None).expect("vegas/ holds the scripts");
        for name in SCRIPT_FILES {
            assert!(found.join(name).is_file(), "{name}");
        }
    }

    #[test]
    fn a_folder_missing_one_script_is_not_a_source() {
        let scratch = Scratch::new("vegas_bundled_partial");
        std::fs::write(scratch.path().join(SCRIPT_FILES[0]), "only one").unwrap();
        let found = script_source(Some(scratch.path()));
        // Only the repo's copy can answer.
        assert!(found.is_none_or(|f| f != scratch.path()));
    }
}
