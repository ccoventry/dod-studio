//! Finding a project's rendered videos again (#1264).
//!
//! The project keeps the path each video was rendered to. A video the user
//! moved or renamed into another export folder is found again by its file
//! name in the render export folders (Settings' `render_export_dirs`); one
//! still not found is reported missing.
//!
//! Cheap by construction: a path still on disk costs one `stat`, and only
//! when something is missing are the export folders walked, once for every
//! missing name together, a few levels deep and with an entry budget, so a
//! folder pointed at a drive root cannot hang the lookup.

use crate::sys::folder_walk::{Kind, Step, Walker};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// How deep under an export folder a moved video is looked for: the folder
/// itself and two levels of subfolders (a per-project or per-day folder the
/// user sorted clips into).
pub const SEARCH_DEPTH: usize = 3;

/// Entries read across all export folders before the search gives up.
pub const SEARCH_ENTRY_BUDGET: usize = 50_000;

/// Where each of `paths` is now, in order: the path itself while it is a
/// file, else the file of the same name (in any case) found under
/// `export_dirs`, else `None`.
///
/// A name is only trusted when it is unambiguous: two missing videos with the
/// same file name (rendered into different folders) would both land on one
/// file, and a file that is another video's own path in this list is that
/// video's, so both stay missing rather than point at the wrong clip. When
/// one name exists in several export folders, the nearest wins (the walk is
/// breadth first, export folders in their listed order).
pub fn locate_videos(paths: &[String], export_dirs: &[String]) -> Vec<Option<PathBuf>> {
    let mut located: Vec<Option<PathBuf>> = paths
        .iter()
        .map(|p| Some(PathBuf::from(p)).filter(|p| !p.as_os_str().is_empty() && p.is_file()))
        .collect();
    let mut missing_by_name: HashMap<String, std::collections::HashSet<String>> = HashMap::new();
    for (p, _) in paths.iter().zip(&located).filter(|(_, f)| f.is_none()) {
        if let Some(name) = name_key(Path::new(p)) {
            missing_by_name
                .entry(name)
                .or_default()
                .insert(p.to_lowercase());
        }
    }
    // A name two different missing paths share cannot say which is which.
    let names: Vec<String> = missing_by_name
        .into_iter()
        .filter(|(_, from)| from.len() == 1)
        .map(|(name, _)| name)
        .collect();
    if names.is_empty() {
        return located;
    }
    let found = find_by_name(&names, export_dirs);
    let in_place: std::collections::HashSet<String> = located
        .iter()
        .flatten()
        .map(|p| p.to_string_lossy().to_lowercase())
        .collect();
    for (slot, path) in located.iter_mut().zip(paths) {
        if slot.is_none() {
            *slot = name_key(Path::new(path))
                .and_then(|name| found.get(&name).cloned())
                .filter(|f| !in_place.contains(&f.to_string_lossy().to_lowercase()));
        }
    }
    located
}

/// A file name as the search compares it: Windows names are case-blind.
fn name_key(path: &Path) -> Option<String> {
    path.file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .filter(|n| !n.is_empty())
}

/// The first file under `export_dirs` for each of `names`; the walk stops as
/// soon as every name is found.
fn find_by_name(names: &[String], export_dirs: &[String]) -> HashMap<String, PathBuf> {
    let roots: Vec<&Path> = export_dirs
        .iter()
        .map(|d| d.trim())
        .filter(|d| !d.is_empty())
        .map(Path::new)
        .collect();
    let mut wanted: std::collections::HashSet<&str> = names.iter().map(String::as_str).collect();
    let mut found = HashMap::new();
    if roots.is_empty() {
        return found;
    }
    Walker::new()
        .breadth_first()
        .skip_system_folders()
        .max_depth(SEARCH_DEPTH)
        .max_entries(SEARCH_ENTRY_BUDGET)
        .walk_roots(&roots, &mut |entry| {
            if entry.kind == Kind::File
                && let Some(name) = name_key(&entry.path)
                && wanted.remove(name.as_str())
            {
                found.insert(name, entry.path.clone());
                if wanted.is_empty() {
                    return Step::Stop;
                }
            }
            Step::Enter
        });
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dod_render_locate_{name}"));
        let _ = std::fs::remove_dir_all(&dir); // a leftover from an aborted run
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn s(p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn a_video_still_where_it_was_rendered_is_its_own_path() {
        let dir = temp_dir("in_place");
        let video = dir.join("clip.mov");
        std::fs::write(&video, b"x").unwrap();
        let located = locate_videos(&[s(&video)], &[]);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(located, vec![Some(video)]);
    }

    #[test]
    fn a_moved_video_is_found_by_name_in_an_export_folder_subfolder() {
        let dir = temp_dir("moved");
        let sorted = dir.join("exports").join("monday");
        std::fs::create_dir_all(&sorted).unwrap();
        std::fs::write(sorted.join("Clip.MOV"), b"x").unwrap();
        let old = s(&dir.join("gone").join("clip.mov"));
        let located = locate_videos(&[old], &[s(&dir.join("exports"))]);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(located, vec![Some(sorted.join("Clip.MOV"))]);
    }

    #[test]
    fn a_video_not_in_any_export_folder_is_missing() {
        let dir = temp_dir("missing");
        std::fs::write(dir.join("other.mov"), b"x").unwrap();
        let located = locate_videos(
            &[s(&dir.join("old").join("clip.mov")), String::new()],
            &[s(&dir), "  ".into()],
        );
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(located, vec![None, None]);
    }

    #[test]
    fn two_missing_videos_with_one_name_are_not_given_the_same_file() {
        let dir = temp_dir("same_name");
        let exports = dir.join("exports");
        std::fs::create_dir_all(&exports).unwrap();
        std::fs::write(exports.join("clip.mov"), b"x").unwrap();
        let located = locate_videos(
            &[
                s(&dir.join("a").join("clip.mov")),
                s(&dir.join("b").join("clip.mov")),
                s(&dir.join("c").join("other.mov")),
            ],
            &[s(&exports)],
        );
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(located, vec![None, None, None]);
    }

    #[test]
    fn a_file_that_is_another_videos_own_path_is_not_its_replacement() {
        let dir = temp_dir("claimed");
        let exports = dir.join("exports");
        std::fs::create_dir_all(&exports).unwrap();
        let here = exports.join("clip.mov");
        std::fs::write(&here, b"x").unwrap();
        let located = locate_videos(
            &[s(&here), s(&dir.join("gone").join("CLIP.mov"))],
            &[s(&exports)],
        );
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(located, vec![Some(here), None]);
    }

    #[test]
    fn a_video_deeper_than_the_search_depth_is_not_found() {
        let dir = temp_dir("deep");
        let deep = dir.join("a").join("b").join("c");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("clip.mov"), b"x").unwrap();
        let located = locate_videos(&["C:/made-up/clip.mov".into()], &[s(&dir)]);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(located, vec![None]);
    }
}
