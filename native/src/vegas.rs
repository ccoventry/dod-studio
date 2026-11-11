//! Putting the VEGAS Pro nameplate script into VEGAS's script menu (#429).
//! VEGAS lists the scripts in `Documents\Vegas Script Menu`; the app ships
//! `vegas/DoD Studio Nameplates.cs` and copies it there on request.

use std::path::{Path, PathBuf};

/// The script's file name, in the repo's `vegas` folder, the app's bundle and
/// VEGAS's menu folder. Only a file of this name is ever overwritten.
pub const SCRIPT_FILE: &str = "DoD Studio Nameplates.cs";

/// The folder VEGAS reads its script menu from, inside `documents`.
const MENU_FOLDER: &str = "Vegas Script Menu";

/// The script as the repo has it, for a dev build, which has no bundled copy.
pub fn dev_script_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("vegas")
}

/// The script to install: the app's bundled copy (`vegas-scripts` in its
/// resources), else the repo's; a debug build tries the repo's first, as
/// [`crate::blender::scripts_dir`] does. `None` when neither has it.
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
        .map(|dir| dir.join(SCRIPT_FILE))
        .find(|file| file.is_file())
}

/// The user's real Documents folder (it may be redirected, for example to
/// OneDrive), from Windows' known-folder lookup.
pub fn documents_dir() -> Result<PathBuf, String> {
    dirs::document_dir().ok_or_else(|| crate::messages::VEGAS_NO_DOCUMENTS.to_string())
}

/// Copies the script at `source` into `documents\Vegas Script Menu`, creating
/// that folder, and replaces an earlier copy of the script. Nothing else in
/// the folder is touched. Returns the installed file.
pub fn install_script(source: &Path, documents: &Path) -> Result<PathBuf, String> {
    let folder = documents.join(MENU_FOLDER);
    std::fs::create_dir_all(&folder).map_err(|e| crate::messages::labeled(folder.display(), e))?;
    let target = folder.join(SCRIPT_FILE);
    std::fs::copy(source, &target).map_err(|e| {
        crate::messages::labeled(format!("{} to {}", source.display(), target.display()), e)
    })?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    fn source_in(dir: &Path, text: &str) -> PathBuf {
        let file = dir.join("src").join(SCRIPT_FILE);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, text).unwrap();
        file
    }

    #[test]
    fn install_creates_the_menu_folder_and_copies_the_script() {
        let scratch = Scratch::new("vegas_install_new");
        let source = source_in(scratch.path(), "script v1");
        let docs = scratch.path().join("Documents");
        let installed = install_script(&source, &docs).unwrap();
        assert_eq!(installed, docs.join("Vegas Script Menu").join(SCRIPT_FILE));
        assert_eq!(std::fs::read_to_string(installed).unwrap(), "script v1");
    }

    #[test]
    fn install_replaces_an_older_copy_and_leaves_other_scripts_alone() {
        let scratch = Scratch::new("vegas_install_overwrite");
        let docs = scratch.path().join("Documents");
        let menu = docs.join("Vegas Script Menu");
        std::fs::create_dir_all(&menu).unwrap();
        std::fs::write(menu.join(SCRIPT_FILE), "old").unwrap();
        std::fs::write(menu.join("Other.cs"), "theirs").unwrap();
        let source = source_in(scratch.path(), "new");
        let installed = install_script(&source, &docs).unwrap();
        assert_eq!(std::fs::read_to_string(installed).unwrap(), "new");
        assert_eq!(
            std::fs::read_to_string(menu.join("Other.cs")).unwrap(),
            "theirs"
        );
    }

    #[test]
    fn a_missing_source_error_names_both_paths() {
        let scratch = Scratch::new("vegas_install_missing");
        let docs = scratch.path().join("Documents");
        let source = scratch.path().join("nope.cs");
        let err = install_script(&source, &docs).unwrap_err();
        assert!(err.contains(&source.display().to_string()), "{err}");
        assert!(err.contains(SCRIPT_FILE), "{err}");
    }

    #[test]
    fn an_uncreatable_menu_folder_error_names_the_folder() {
        let scratch = Scratch::new("vegas_install_blocked");
        let source = source_in(scratch.path(), "x");
        // A file where the Documents folder should be.
        let docs = scratch.path().join("Documents");
        std::fs::write(&docs, "file").unwrap();
        let err = install_script(&source, &docs).unwrap_err();
        let folder = docs.join("Vegas Script Menu");
        assert!(err.contains(&folder.display().to_string()), "{err}");
    }

    #[test]
    fn the_repo_script_is_found_without_a_bundled_copy() {
        let found = script_source(None).expect("vegas/ holds the script");
        assert!(found.ends_with(SCRIPT_FILE));
    }

    #[test]
    fn a_folder_without_the_script_finds_nothing_of_its_own() {
        let scratch = Scratch::new("vegas_bundled_empty");
        let found = script_source(Some(scratch.path()));
        // Only the repo's copy can answer.
        assert!(found.is_none_or(|f| !f.starts_with(scratch.path())));
    }
}
