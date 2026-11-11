//! Directory junctions (#376): create one, read where it points, and unlink
//! it without touching what it points at.
//!
//! Studio no longer makes one for a user (the Link Folder to Game button went
//! in #779); what is left is the folder-walk tests, which need a real link to
//! prove a walk skips it. It is made through the Win32 reparse-point call rather
//! than `cmd /c mklink /J`: `cmd` misreads `&`, `^` and `%` in a folder name,
//! and the call needs no admin rights on NTFS.
//!
//! Removing one is `RemoveDirectory` on the link (`std::fs::remove_dir`),
//! which unlinks it and leaves the target alone. [`remove`] refuses anything
//! that is not a junction, so a real folder can never be removed through it.

use std::path::{Path, PathBuf};

/// `\??\` in front of the target's path is what the reparse data holds: an NT
/// object path, which is what every `mklink /J` writes too.
const NT_PREFIX: &str = r"\??\";
/// What `canonicalize` puts in front of a local path.
const VERBATIM_PREFIX: &str = r"\\?\";

/// `path`'s full form without the `\\?\` prefix, so it reads and compares as
/// a plain path. A UNC path (`\\?\UNC\server\share`) is given back as
/// `\\server\share`.
pub fn plain_canonical(path: &Path) -> std::io::Result<PathBuf> {
    let full = std::fs::canonicalize(path)?;
    let text = full.to_string_lossy();
    let plain = if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else if let Some(local) = text.strip_prefix(VERBATIM_PREFIX) {
        local.to_string()
    } else {
        text.into_owned()
    };
    Ok(PathBuf::from(plain))
}

/// Makes `link` a junction to `target`, an existing folder on a local drive.
/// `link` must not exist yet.
pub fn create(link: &Path, target: &Path) -> std::io::Result<()> {
    let target = plain_canonical(target)?;
    let text = target.to_string_lossy();
    if text.starts_with(r"\\") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!("{text} is a network path; a junction needs a local drive"),
        ));
    }
    if !target.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotADirectory,
            format!("{text} is not a folder"),
        ));
    }
    std::fs::create_dir(link)?;
    if let Err(e) = win::set_mount_point(link, &text) {
        let _ = std::fs::remove_dir(link);
        return Err(e);
    }
    Ok(())
}

/// Where the junction at `path` points, as a plain path, or `None` when
/// `path` is not a junction (a real folder, a file, a symlink, or nothing).
pub fn target(path: &Path) -> Option<PathBuf> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_symlink() {
        return None;
    }
    let nt = win::mount_point_target(path)?;
    Some(PathBuf::from(nt.strip_prefix(NT_PREFIX).unwrap_or(&nt)))
}

/// Whether `path` is a junction.
pub fn is_junction(path: &Path) -> bool {
    target(path).is_some()
}

/// Unlinks the junction at `link`. The folder it points at, and everything in
/// it, stays. Refuses a path that is not a junction.
pub fn remove(link: &Path) -> std::io::Result<()> {
    if !is_junction(link) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{} is not a folder link", link.display()),
        ));
    }
    std::fs::remove_dir(link)
}

mod win {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateFileW(
            file_name: *const u16,
            desired_access: u32,
            share_mode: u32,
            security_attributes: *const c_void,
            creation_disposition: u32,
            flags_and_attributes: u32,
            template_file: *mut c_void,
        ) -> *mut c_void;
        fn DeviceIoControl(
            device: *mut c_void,
            io_control_code: u32,
            in_buffer: *const c_void,
            in_buffer_size: u32,
            out_buffer: *mut c_void,
            out_buffer_size: u32,
            bytes_returned: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_SHARE_READ: u32 = 1;
    const FILE_SHARE_WRITE: u32 = 2;
    const FILE_SHARE_DELETE: u32 = 4;
    const OPEN_EXISTING: u32 = 3;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FSCTL_SET_REPARSE_POINT: u32 = 0x0009_00A4;
    const FSCTL_GET_REPARSE_POINT: u32 = 0x0009_00A8;
    const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
    const MAXIMUM_REPARSE_DATA_BUFFER_SIZE: usize = 16 * 1024;

    /// A handle open on `path` itself (not what it points at), closed on drop.
    struct Handle(*mut c_void);

    impl Handle {
        fn open(path: &Path, access: u32) -> std::io::Result<Self> {
            let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
            // Safety: a nul-terminated wide string, null for the optional
            // pointers, and the flags that open a reparse point itself.
            let handle = unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    access,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                    std::ptr::null_mut(),
                )
            };
            if handle as isize == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(Self(handle))
        }
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            // Safety: a handle CreateFileW gave us, closed once.
            unsafe { CloseHandle(self.0) };
        }
    }

    fn push_u16(out: &mut Vec<u8>, value: u16) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn push_u32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    /// Writes mount-point reparse data on the empty folder `link`:
    /// `REPARSE_DATA_BUFFER` with `MountPointReparseBuffer`, the substitute
    /// name `\??\<target>` and the print name `<target>`, both nul-terminated.
    pub fn set_mount_point(link: &Path, target: &str) -> std::io::Result<()> {
        let substitute: Vec<u16> = format!("{}{target}", super::NT_PREFIX)
            .encode_utf16()
            .collect();
        let print: Vec<u16> = target.encode_utf16().collect();
        let substitute_bytes = substitute.len() * 2;
        let print_bytes = print.len() * 2;
        // The four offset/length fields, both names, and a nul after each.
        let data_length = 8 + substitute_bytes + 2 + print_bytes + 2;
        if data_length + 8 > MAXIMUM_REPARSE_DATA_BUFFER_SIZE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "the target path is too long for a junction",
            ));
        }
        let mut buffer = Vec::with_capacity(data_length + 8);
        push_u32(&mut buffer, IO_REPARSE_TAG_MOUNT_POINT);
        push_u16(&mut buffer, data_length as u16);
        push_u16(&mut buffer, 0);
        push_u16(&mut buffer, 0);
        push_u16(&mut buffer, substitute_bytes as u16);
        push_u16(&mut buffer, (substitute_bytes + 2) as u16);
        push_u16(&mut buffer, print_bytes as u16);
        for unit in substitute.iter().chain([0u16].iter()) {
            push_u16(&mut buffer, *unit);
        }
        for unit in print.iter().chain([0u16].iter()) {
            push_u16(&mut buffer, *unit);
        }

        let handle = Handle::open(link, GENERIC_WRITE)?;
        let mut returned = 0u32;
        // Safety: the buffer is laid out as REPARSE_DATA_BUFFER and outlives
        // the call; no output buffer is needed for a set.
        let ok = unsafe {
            DeviceIoControl(
                handle.0,
                FSCTL_SET_REPARSE_POINT,
                buffer.as_ptr().cast(),
                buffer.len() as u32,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    /// The substitute name of the mount point at `path` (`\??\D:\matches`),
    /// or `None` when `path` carries no mount-point reparse data.
    pub fn mount_point_target(path: &Path) -> Option<String> {
        let handle = Handle::open(path, 0).ok()?;
        let mut buffer = vec![0u8; MAXIMUM_REPARSE_DATA_BUFFER_SIZE];
        let mut returned = 0u32;
        // Safety: the output buffer is the documented maximum size and
        // outlives the call.
        let ok = unsafe {
            DeviceIoControl(
                handle.0,
                FSCTL_GET_REPARSE_POINT,
                std::ptr::null(),
                0,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return None;
        }
        let returned = returned as usize;
        let field = |at: usize| -> Option<usize> {
            let bytes = buffer.get(at..at + 2)?;
            Some(u16::from_le_bytes([bytes[0], bytes[1]]) as usize)
        };
        let tag = u32::from_le_bytes(buffer.get(0..4)?.try_into().ok()?);
        if tag != IO_REPARSE_TAG_MOUNT_POINT {
            return None;
        }
        // Header (8), then SubstituteNameOffset/Length, PrintNameOffset/
        // Length (8), then PathBuffer.
        let offset = field(8)?;
        let length = field(10)?;
        let start = 16 + offset;
        let end = start + length;
        if end > returned || length % 2 != 0 {
            return None;
        }
        let units: Vec<u16> = buffer[start..end]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        Some(String::from_utf16_lossy(&units))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh scratch folder, removed first in case a previous run left it.
    pub(crate) fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("dodstudio_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn a_junction_points_at_its_target_and_reads_through() {
        let root = scratch("junction_create");
        let target = root.join("matches & more");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("m1.dem"), b"demo").unwrap();
        let link = root.join("link");

        create(&link, &target).unwrap();

        assert!(is_junction(&link));
        assert!(!is_junction(&target), "a real folder is not a junction");
        assert!(!is_junction(&root.join("missing")));
        assert_eq!(
            super::target(&link).unwrap(),
            plain_canonical(&target).unwrap()
        );
        assert_eq!(std::fs::read(link.join("m1.dem")).unwrap(), b"demo");
        // std sees it as a directory symlink, never as a plain directory.
        let kind = std::fs::symlink_metadata(&link).unwrap().file_type();
        assert!(kind.is_symlink() && !kind.is_dir());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn removing_a_junction_keeps_the_target_and_its_files() {
        let root = scratch("junction_remove");
        let target = root.join("target");
        std::fs::create_dir_all(target.join("sub")).unwrap();
        std::fs::write(target.join("sub/m1.dem"), b"demo").unwrap();
        let link = root.join("link");
        create(&link, &target).unwrap();

        remove(&link).unwrap();

        assert!(std::fs::symlink_metadata(&link).is_err(), "link gone");
        assert_eq!(std::fs::read(target.join("sub/m1.dem")).unwrap(), b"demo");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn remove_refuses_a_real_folder_and_a_missing_one() {
        let root = scratch("junction_refuse");
        let real = root.join("real");
        std::fs::create_dir(&real).unwrap();
        assert!(remove(&real).is_err());
        assert!(real.is_dir(), "still there");
        assert!(remove(&root.join("missing")).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn create_refuses_a_missing_target_and_an_existing_link_path() {
        let root = scratch("junction_create_refuse");
        assert!(create(&root.join("link"), &root.join("missing")).is_err());
        assert!(
            std::fs::symlink_metadata(root.join("link")).is_err(),
            "nothing left behind"
        );
        let target = root.join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::create_dir(root.join("taken")).unwrap();
        assert!(create(&root.join("taken"), &target).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
