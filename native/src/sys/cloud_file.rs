//! Whether reading a file would download it from a cloud drive first (#992).
//!
//! Reading a folder's players analyses every demo the analyzer cache lacks.
//! On OneDrive, Dropbox or Google Drive that downloads each one (60-100 MB),
//! so those demos wait for the user to ask (Cache all, or opening one).
//!
//! Two signs, neither of which opens the file:
//! - Windows' cloud-files placeholders (OneDrive, Dropbox, Box, Google Drive's
//!   mirror mode) carry `RECALL_ON_DATA_ACCESS`, `RECALL_ON_OPEN` or `OFFLINE`
//!   in their attributes while their data is not on this PC.
//! - Google Drive for desktop's streaming drive (`G:\My Drive`, or another
//!   letter) is its own file system that sets none of those, so a file on a
//!   volume labelled "Google Drive" counts as in the cloud, cached or not.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x4_0000;
const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;

/// Whether `attributes` (a file's Win32 attributes) mark its data as not on
/// this PC.
pub fn attributes_say_online_only(attributes: u32) -> bool {
    attributes
        & (FILE_ATTRIBUTE_OFFLINE
            | FILE_ATTRIBUTE_RECALL_ON_OPEN
            | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
        != 0
}

/// Whether a volume with this label is Google Drive for desktop's streaming
/// drive.
pub fn label_is_streaming_drive(label: &str) -> bool {
    label.trim().eq_ignore_ascii_case("Google Drive")
}

/// Answers "is this file in the cloud" for many files, asking each volume
/// for its label once.
#[derive(Default)]
pub struct CloudCheck {
    streaming_volumes: HashMap<PathBuf, bool>,
}

impl CloudCheck {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether reading `path` would download it first. False when that can't
    /// be told (the file is then read as before).
    pub fn is_online_only(&mut self, path: &Path) -> bool {
        if win::attributes(path).is_some_and(attributes_say_online_only) {
            return true;
        }
        let Some(root) = win::volume_root(path) else {
            return false;
        };
        *self
            .streaming_volumes
            .entry(root)
            .or_insert_with_key(|root| {
                win::volume_label(root).is_some_and(|l| label_is_streaming_drive(&l))
            })
    }
}

#[cfg(windows)]
mod win {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::{Path, PathBuf};

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileAttributesW(file_name: *const u16) -> u32;
        fn GetVolumePathNameW(file_name: *const u16, volume_path: *mut u16, length: u32) -> i32;
        fn GetVolumeInformationW(
            root_path: *const u16,
            volume_name: *mut u16,
            volume_name_size: u32,
            serial_number: *mut u32,
            max_component_length: *mut u32,
            file_system_flags: *mut u32,
            file_system_name: *mut u16,
            file_system_name_size: u32,
        ) -> i32;
    }

    const INVALID_FILE_ATTRIBUTES: u32 = u32::MAX;
    const MAX_PATH_CHARS: usize = 1024;

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    fn from_wide(buffer: &[u16]) -> OsString {
        let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        OsString::from_wide(&buffer[..end])
    }

    pub fn attributes(path: &Path) -> Option<u32> {
        let name = wide(path);
        // SAFETY: `name` is NUL-terminated and outlives the call.
        let attributes = unsafe { GetFileAttributesW(name.as_ptr()) };
        (attributes != INVALID_FILE_ATTRIBUTES).then_some(attributes)
    }

    pub fn volume_root(path: &Path) -> Option<PathBuf> {
        let name = wide(path);
        let mut root = vec![0u16; MAX_PATH_CHARS];
        // SAFETY: both buffers are valid for the lengths passed.
        let ok = unsafe { GetVolumePathNameW(name.as_ptr(), root.as_mut_ptr(), root.len() as u32) };
        (ok != 0).then(|| PathBuf::from(from_wide(&root)))
    }

    pub fn volume_label(root: &Path) -> Option<String> {
        let name = wide(root);
        let mut label = vec![0u16; 261];
        // SAFETY: `label` is valid for its length; the other outputs are optional.
        let ok = unsafe {
            GetVolumeInformationW(
                name.as_ptr(),
                label.as_mut_ptr(),
                label.len() as u32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
            )
        };
        (ok != 0).then(|| from_wide(&label).to_string_lossy().into_owned())
    }
}

#[cfg(not(windows))]
mod win {
    use std::path::{Path, PathBuf};

    pub fn attributes(_: &Path) -> Option<u32> {
        None
    }

    pub fn volume_root(_: &Path) -> Option<PathBuf> {
        None
    }

    pub fn volume_label(_: &Path) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_attributes_are_online_only() {
        // A OneDrive "online-only" file: archive, reparse point, unpinned,
        // recall on data access.
        assert!(attributes_say_online_only(0x0040_0420 | 0x0010_0000));
        assert!(attributes_say_online_only(FILE_ATTRIBUTE_OFFLINE));
        assert!(attributes_say_online_only(FILE_ATTRIBUTE_RECALL_ON_OPEN));
        // "Always keep on this device" (pinned) and a plain local file.
        assert!(!attributes_say_online_only(0x0008_0420));
        assert!(!attributes_say_online_only(0x20));
    }

    #[test]
    fn google_drive_volume_label() {
        assert!(label_is_streaming_drive("Google Drive"));
        assert!(label_is_streaming_drive("google drive "));
        assert!(!label_is_streaming_drive("SSD 3"));
        assert!(!label_is_streaming_drive(""));
    }

    #[test]
    fn a_local_file_is_not_online_only() {
        let dir = std::env::temp_dir().join(format!("cloud_file_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("x.dem");
        std::fs::write(&file, b"demo").unwrap();
        let mut check = CloudCheck::new();
        assert!(!check.is_online_only(&file));
        // A path that doesn't exist reads as local too: the walk reports its error.
        assert!(!check.is_online_only(&dir.join("missing.dem")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
