//! Switching a `.json` project to `.dodstudio` (#528): the rename behind the
//! offer `project_convert.js` makes after a `.json` project loads.
//!
//! A rename, not a copy, so no second file is left for Open Recent or the
//! user to mix up. The contents stay as they are (they were JSON all along).
//! It never replaces a file: a `<name>.dodstudio` already beside the project
//! means no offer, and one that turns up between the offer and the answer
//! makes the rename fail with that said.

use crate::messages;
use std::path::{Path, PathBuf};

/// The `.dodstudio` name `path` would switch to: `.json` projects only, never
/// a `.autosave.json`.
pub fn converted_path(path: &Path) -> Option<PathBuf> {
    let is_json = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("json"));
    let is_autosave = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.to_ascii_lowercase().ends_with(".autosave.json"));
    (is_json && !is_autosave).then(|| path.with_extension("dodstudio"))
}

/// Whether to offer the switch for `path`, and the name it would get: a
/// `.json` project that's there, that Studio may rename, whose `.dodstudio`
/// name is free.
pub fn offer_for(path: &Path) -> Option<PathBuf> {
    let target = converted_path(path)?;
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.permissions().readonly() || target.exists() {
        return None;
    }
    may_rename(path).then_some(target)
}

/// Whether this user may rename `path` where it is: delete it from its folder
/// and add a file to that folder. Asked of the file system's own access
/// checks by opening both with just those rights; nothing is written.
#[cfg(windows)]
fn may_rename(path: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    const DELETE: u32 = 0x0001_0000;
    const FILE_ADD_FILE: u32 = 0x0002;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    let Some(folder) = path.parent() else {
        return false;
    };
    let file_ok = std::fs::OpenOptions::new()
        .access_mode(DELETE)
        .open(path)
        .is_ok();
    let folder_ok = std::fs::OpenOptions::new()
        .access_mode(FILE_ADD_FILE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(folder)
        .is_ok();
    file_ok && folder_ok
}

#[cfg(not(windows))]
fn may_rename(path: &Path) -> bool {
    path.parent().is_some()
}

/// Renames `path` to its `.dodstudio` name; never replaces a file there.
pub fn convert(path: &Path) -> Result<PathBuf, String> {
    let target = converted_path(path).ok_or_else(|| {
        format!(
            "{} is not a .json project, so it was not switched to .dodstudio",
            path.display()
        )
    })?;
    if target.exists() {
        return Err(already_there(&target));
    }
    rename_no_replace(path, &target).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            already_there(&target)
        } else {
            format!(
                "Could not rename {} to {}: {e}",
                path.display(),
                target.display()
            )
        }
    })?;
    native::log_markdown(&format!(
        "📁 **Project** — switched {} to {}",
        path.display(),
        target.display()
    ));
    Ok(target)
}

fn already_there(target: &Path) -> String {
    format!(
        "{} already exists, so the project stays as .json",
        target.display()
    )
}

/// `std::fs::rename` replaces an existing file on Windows; this doesn't.
#[cfg(windows)]
fn rename_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    let wide = |p: &Path| -> Vec<u16> { p.as_os_str().encode_wide().chain([0]).collect() };
    let (from, to) = (wide(from), wide(to));
    // Safety: two NUL-terminated paths; flags 0 = fail if `to` exists.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) } != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(windows))]
fn rename_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    if to.exists() {
        return Err(std::io::ErrorKind::AlreadyExists.into());
    }
    std::fs::rename(from, to)
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
}

/// The `.dodstudio` name to offer for a just-loaded `.json` project, or
/// `None` when there's nothing to offer.
#[tauri::command]
pub async fn project_convert_offer(path: String) -> Result<Option<String>, String> {
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        Ok(offer_for(Path::new(&path)).map(|p| p.to_string_lossy().into_owned()))
    }))
    .await
}

/// Renames the project to `.dodstudio`; resolves the new path.
#[tauri::command]
pub async fn convert_project_file(path: String) -> Result<String, String> {
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        convert(Path::new(&path)).map(|p| p.to_string_lossy().into_owned())
    }))
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    #[test]
    fn only_json_projects_get_a_dodstudio_name() {
        assert_eq!(
            converted_path(Path::new(r"D:\p\monday.json")),
            Some(PathBuf::from(r"D:\p\monday.dodstudio"))
        );
        assert_eq!(
            converted_path(Path::new(r"D:\p\Old.JSON")),
            Some(PathBuf::from(r"D:\p\Old.dodstudio"))
        );
        assert_eq!(converted_path(Path::new(r"D:\p\monday.dodstudio")), None);
        assert_eq!(
            converted_path(Path::new(r"D:\p\monday.autosave.json")),
            None
        );
        assert_eq!(converted_path(Path::new(r"D:\p\demo.dem")), None);
    }

    #[test]
    fn a_json_project_is_renamed_with_its_contents() {
        let dir = Scratch::new("convert_rename");
        let json = dir.path().join("monday.json");
        std::fs::write(&json, br#"{"version":"0.12.0"}"#).unwrap();
        assert_eq!(offer_for(&json), Some(dir.path().join("monday.dodstudio")));

        let target = convert(&json).unwrap();
        assert_eq!(target, dir.path().join("monday.dodstudio"));
        assert!(!json.exists(), "a rename, not a copy");
        assert_eq!(std::fs::read(&target).unwrap(), br#"{"version":"0.12.0"}"#);
    }

    #[test]
    fn an_existing_dodstudio_is_never_replaced() {
        let dir = Scratch::new("convert_taken");
        let json = dir.path().join("monday.json");
        let taken = dir.path().join("monday.dodstudio");
        std::fs::write(&json, b"json").unwrap();
        std::fs::write(&taken, b"already here").unwrap();
        assert_eq!(offer_for(&json), None);

        let err = convert(&json).unwrap_err();
        assert!(err.contains("already exists"), "{err}");
        assert!(err.contains(&taken.display().to_string()), "{err}");
        assert_eq!(std::fs::read(&taken).unwrap(), b"already here");
        assert!(json.exists());
    }

    #[test]
    fn the_rename_itself_refuses_a_file_that_turned_up() {
        let dir = Scratch::new("convert_race");
        let from = dir.path().join("a.json");
        let to = dir.path().join("a.dodstudio");
        std::fs::write(&from, b"json").unwrap();
        std::fs::write(&to, b"other").unwrap();
        let err = rename_no_replace(&from, &to).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&to).unwrap(), b"other");
    }

    #[test]
    fn a_read_only_project_is_not_offered() {
        let dir = Scratch::new("convert_readonly");
        let json = dir.path().join("monday.json");
        std::fs::write(&json, b"json").unwrap();
        let mut perms = std::fs::metadata(&json).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&json, perms.clone()).unwrap();
        assert_eq!(offer_for(&json), None);
        // Writable again, or the scratch folder can't be removed.
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        std::fs::set_permissions(&json, perms).unwrap();
    }

    #[test]
    fn nothing_is_offered_for_a_missing_file_or_an_autosave() {
        let dir = Scratch::new("convert_missing");
        assert_eq!(offer_for(&dir.path().join("gone.json")), None);
        let autosave = dir.path().join("monday.autosave.json");
        std::fs::write(&autosave, b"json").unwrap();
        assert_eq!(offer_for(&autosave), None);
    }
}
