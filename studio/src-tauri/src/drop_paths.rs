//! Files and folders dropped on the Highlights page from Windows (#530): which
//! are demos, which are folders to scan, and which are neither.

use serde::Serialize;
use std::path::Path;

/// A drop, sorted: `.dem` files (any case), folders, and the rest by name.
#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct DroppedPaths {
    pub demos: Vec<String>,
    pub folders: Vec<String>,
    /// Neither a `.dem` file nor a folder, or no longer there: file names
    /// only, for the toast that names them.
    pub skipped: Vec<String>,
}

/// Sorts `paths` by what they are on disk. A folder is told by `is_dir`,
/// which follows a link, so a dropped junction to a demo folder is scanned
/// as that folder.
pub fn sort_dropped(paths: &[String]) -> DroppedPaths {
    let mut out = DroppedPaths::default();
    for path in paths {
        let p = Path::new(path);
        if p.is_dir() {
            out.folders.push(path.clone());
        } else if p.is_file()
            && p.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("dem"))
        {
            out.demos.push(path.clone());
        } else {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone());
            out.skipped.push(name);
        }
    }
    out
}

/// Async: a dropped folder on a slow or cloud drive can take a while to stat.
#[tauri::command(async)]
pub fn sort_dropped_paths(paths: Vec<String>) -> DroppedPaths {
    sort_dropped(&paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    #[test]
    fn demos_folders_and_the_rest_are_told_apart() {
        let scratch = Scratch::new("drop_paths");
        let root = scratch.path();
        std::fs::create_dir_all(root.join("folder")).unwrap();
        for f in ["a.dem", "B.DEM", "notes.txt", "project.json"] {
            std::fs::write(root.join(f), b"").unwrap();
        }
        let s = |name: &str| root.join(name).to_string_lossy().into_owned();
        let dropped = sort_dropped(&[
            s("a.dem"),
            s("folder"),
            s("notes.txt"),
            s("B.DEM"),
            s("project.json"),
            s("gone.dem"),
        ]);
        assert_eq!(dropped.demos, [s("a.dem"), s("B.DEM")]);
        assert_eq!(dropped.folders, [s("folder")]);
        assert_eq!(dropped.skipped, ["notes.txt", "project.json", "gone.dem"]);
    }
}
