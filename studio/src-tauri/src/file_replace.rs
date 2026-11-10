//! Replaces a file whole, so a full disk or a crash mid-write can't leave a
//! damaged one: the text goes to a temp file beside it, reaches the disk, and
//! is then renamed over the target. Project files go through this (#1085):
//! after the first save in a new format, nothing else covers them.

use crate::messages;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// Tells temp files apart: two saves of one path can run at once.
static NEXT_TEMP: AtomicU32 = AtomicU32::new(0);

/// A temp name beside `path`, on its drive, so the rename can't turn into a copy.
fn temp_path(path: &Path) -> PathBuf {
    let number = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}-{number}.tmp", std::process::id()));
    path.with_file_name(name)
}

/// A read-only target refuses here, as a plain write does: the attribute is
/// the user's (#478), and a rename could otherwise go straight over it.
fn refuse_read_only(path: &Path) -> std::io::Result<()> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.permissions().readonly() => {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        }
        _ => Ok(()),
    }
}

fn write_temp(temp: &Path, contents: &str) -> std::io::Result<()> {
    let mut file = std::fs::File::create(temp)?;
    file.write_all(contents.as_bytes())?;
    // On the disk before the rename, or a crash could swap in an empty file.
    file.sync_all()
}

/// Writes `contents` to `path`, creating it or replacing it whole. On any
/// failure the file at `path` is as it was and the temp file is gone. A
/// read-only target is refused. Blocks on the disk, so call it off the main
/// thread. Returns the error text to show.
pub fn replace_file(path: &Path, contents: &str) -> Result<(), String> {
    let shown = path.display().to_string();
    refuse_read_only(path).map_err(|e| messages::failed_to_write_file(&shown, e))?;
    let temp = temp_path(path);
    let result = write_temp(&temp, contents).and_then(|()| std::fs::rename(&temp, path));
    if result.is_err() {
        // Nothing more to say if this fails too: a stray .tmp harms nothing,
        // and the write's own error is the one to show.
        let _ = std::fs::remove_file(&temp);
    }
    result.map_err(|e| messages::failed_to_write_file(&shown, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("ds_file_replace_{}_{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn names(&self) -> Vec<String> {
            let mut names: Vec<String> = std::fs::read_dir(&self.0)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_new_file_is_created_with_no_temp_left() {
        let dir = Scratch::new("new");
        let path = dir.0.join("a.dodstudio");
        replace_file(&path, "text").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "text");
        assert_eq!(dir.names(), ["a.dodstudio"]);
    }

    #[test]
    fn an_existing_file_is_replaced_whole_even_by_shorter_text() {
        let dir = Scratch::new("replace");
        let path = dir.0.join("a.dodstudio");
        std::fs::write(&path, "a much longer old file").unwrap();
        replace_file(&path, "new").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(dir.names(), ["a.dodstudio"]);
    }

    #[test]
    fn a_read_only_target_is_refused_and_keeps_its_text_and_attribute() {
        let dir = Scratch::new("read_only");
        let path = dir.0.join("a.dodstudio");
        std::fs::write(&path, "old").unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&path, perms.clone()).unwrap();

        let err = replace_file(&path, "new").unwrap_err();
        assert!(err.contains("a.dodstudio"), "{err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
        assert!(std::fs::metadata(&path).unwrap().permissions().readonly());
        assert_eq!(dir.names(), ["a.dodstudio"]);

        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        std::fs::set_permissions(&path, perms).unwrap();
    }

    #[test]
    fn a_failed_rename_leaves_the_target_alone_and_cleans_up_the_temp() {
        // A folder at the target's name: the temp writes, the rename can't land.
        let dir = Scratch::new("rename_fails");
        let path = dir.0.join("a.dodstudio");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("inside.txt"), "kept").unwrap();

        assert!(replace_file(&path, "new").is_err());
        assert_eq!(
            std::fs::read_to_string(path.join("inside.txt")).unwrap(),
            "kept"
        );
        assert_eq!(dir.names(), ["a.dodstudio"]);
    }

    #[test]
    fn a_target_held_open_without_delete_sharing_keeps_its_text() {
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let dir = Scratch::new("held_open");
            let path = dir.0.join("a.dodstudio");
            std::fs::write(&path, "old").unwrap();
            // What a sync client or a virus scanner does: read, sharing reads only.
            const FILE_SHARE_READ: u32 = 1;
            let held = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ)
                .open(&path)
                .unwrap();

            assert!(replace_file(&path, "new").is_err());
            drop(held);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
            assert_eq!(dir.names(), ["a.dodstudio"]);
        }
    }

    #[test]
    fn a_missing_folder_is_an_error_naming_the_file() {
        let dir = Scratch::new("no_folder");
        let path = dir.0.join("gone").join("a.dodstudio");
        assert!(
            replace_file(&path, "new")
                .unwrap_err()
                .contains("a.dodstudio")
        );
    }
}
