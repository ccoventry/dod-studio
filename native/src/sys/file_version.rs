//! A Windows file's version resource, read without loading the file (#1492):
//! the "File version" the Properties dialog shows for `HLAE.exe` and
//! `AfxHookGoldSrc.dll`. Reads the disk, so call it off the main thread.

use std::path::Path;

/// The four-part file version ("2.189.5.0") of `path`, or `None` when the
/// file is missing, unreadable or has no version resource.
#[cfg(windows)]
pub fn file_version(path: &Path) -> Option<String> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "version")]
    unsafe extern "system" {
        fn GetFileVersionInfoSizeW(file: *const u16, handle: *mut u32) -> u32;
        fn GetFileVersionInfoW(file: *const u16, handle: u32, len: u32, data: *mut c_void) -> i32;
        fn VerQueryValueW(
            block: *const c_void,
            sub_block: *const u16,
            buffer: *mut *mut c_void,
            len: *mut u32,
        ) -> i32;
    }
    // The fixed part of the resource, from `VS_FIXEDFILEINFO`: after its
    // signature and structure version come the file version's two words.
    #[repr(C)]
    struct FixedFileInfo {
        signature: u32,
        struct_version: u32,
        file_version_ms: u32,
        file_version_ls: u32,
    }
    const SIGNATURE: u32 = 0xFEEF_04BD;

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let root: Vec<u16> = "\\".encode_utf16().chain([0]).collect();
    let mut handle = 0u32;
    // SAFETY: `wide` is a NUL-terminated string that outlives the call.
    let size = unsafe { GetFileVersionInfoSizeW(wide.as_ptr(), &mut handle) };
    if size == 0 {
        return None;
    }
    // u32 elements: the block is read as a struct of u32s, so keep it aligned.
    let mut block = vec![0u32; (size as usize).div_ceil(4)];
    // SAFETY: `block` holds at least `size` bytes.
    let read = unsafe {
        GetFileVersionInfoW(
            wide.as_ptr(),
            0,
            block.len() as u32 * 4,
            block.as_mut_ptr().cast(),
        )
    };
    if read == 0 {
        return None;
    }
    let mut info: *mut c_void = std::ptr::null_mut();
    let mut len = 0u32;
    // SAFETY: `block` is the data the call above filled; on success `info`
    // points inside it, which stays alive until the copy below.
    let found =
        unsafe { VerQueryValueW(block.as_ptr().cast(), root.as_ptr(), &mut info, &mut len) };
    if found == 0 || info.is_null() || (len as usize) < std::mem::size_of::<FixedFileInfo>() {
        return None;
    }
    // SAFETY: `info` points at `len` bytes inside `block`, at least a
    // `FixedFileInfo`'s worth; read unaligned in case the call's pointer is.
    let fixed = unsafe { info.cast::<FixedFileInfo>().read_unaligned() };
    if fixed.signature != SIGNATURE {
        return None;
    }
    Some(format_version(fixed.file_version_ms, fixed.file_version_ls))
}

/// Never reads anything off Windows.
#[cfg(not(windows))]
pub fn file_version(_path: &Path) -> Option<String> {
    None
}

/// `ms`/`ls` as the resource stores them (two 16-bit parts each) as
/// "major.minor.build.revision".
pub fn format_version(ms: u32, ls: u32) -> String {
    format!("{}.{}.{}.{}", ms >> 16, ms & 0xFFFF, ls >> 16, ls & 0xFFFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_words_make_a_four_part_version() {
        assert_eq!(format_version(0x0002_00BD, 0x0005_0000), "2.189.5.0");
    }

    #[cfg(windows)]
    #[test]
    fn a_system_dll_has_a_version() {
        let root = std::env::var_os("SystemRoot").expect("SystemRoot is set on Windows");
        let version = file_version(&Path::new(&root).join("System32").join("kernel32.dll"))
            .expect("kernel32.dll has a version resource");
        assert_eq!(version.split('.').count(), 4);
        assert!(version.split('.').all(|p| p.parse::<u32>().is_ok()));
    }

    #[test]
    fn a_missing_file_or_one_without_a_version_reads_as_none() {
        assert_eq!(file_version(Path::new("no-such-folder/missing.dll")), None);
        // Cargo.toml exists and is no executable.
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert_eq!(file_version(&manifest), None);
    }
}
