//! A Windows file's version resource, read without loading the file (#1492):
//! what the Properties dialog shows for `HLAE.exe` as "File version" and
//! "Product version". Reads the disk, so call it off the main thread.

use std::path::Path;

/// The four-part file version ("2.189.5.0") of `path`, or `None` when the
/// file is missing, unreadable or has no version resource.
#[cfg(windows)]
pub fn file_version(path: &Path) -> Option<String> {
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

    let block = win::Block::load(path)?;
    let bytes = block.query("\\")?;
    if bytes.len() < std::mem::size_of::<FixedFileInfo>() {
        return None;
    }
    // SAFETY: `bytes` holds at least a `FixedFileInfo`'s worth; read
    // unaligned because a `Vec<u8>` is not aligned for it.
    let fixed = unsafe { bytes.as_ptr().cast::<FixedFileInfo>().read_unaligned() };
    if fixed.signature != SIGNATURE {
        return None;
    }
    Some(format_version(fixed.file_version_ms, fixed.file_version_ls))
}

/// The "ProductVersion" string of `path`'s version resource, which for
/// HLAE carries the build's git hash ("2.192.4.0+55ebb71"); `None` when the
/// file is missing or has no such string.
#[cfg(windows)]
pub fn product_version(path: &Path) -> Option<String> {
    let block = win::Block::load(path)?;
    // The first language and code page the resource lists, as its string
    // table is named after them.
    let translation = block.query("\\VarFileInfo\\Translation")?;
    let [l0, l1, c0, c1, ..] = translation[..] else {
        return None;
    };
    let (lang, page) = (u16::from_le_bytes([l0, l1]), u16::from_le_bytes([c0, c1]));
    let bytes = block.query(&format!(
        "\\StringFileInfo\\{lang:04x}{page:04x}\\ProductVersion"
    ))?;
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .take_while(|&u| u != 0)
        .collect();
    let text = String::from_utf16_lossy(&units).trim().to_string();
    (!text.is_empty()).then_some(text)
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

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

    /// A file's whole version resource, as `GetFileVersionInfoW` fills it.
    pub struct Block {
        /// u32 elements: the block is read as structs of u32s, so keep it
        /// aligned.
        data: Vec<u32>,
    }

    impl Block {
        pub fn load(path: &Path) -> Option<Block> {
            let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
            let mut handle = 0u32;
            // SAFETY: `wide` is a NUL-terminated string that outlives the call.
            let size = unsafe { GetFileVersionInfoSizeW(wide.as_ptr(), &mut handle) };
            if size == 0 {
                return None;
            }
            let mut data = vec![0u32; (size as usize).div_ceil(4)];
            // SAFETY: `data` holds at least `size` bytes.
            let read =
                unsafe { GetFileVersionInfoW(wide.as_ptr(), 0, size, data.as_mut_ptr().cast()) };
            (read != 0).then_some(Block { data })
        }

        /// The bytes of the resource's `sub_block` (`\`, a string's path),
        /// copied out of the block. The length the call gives counts bytes
        /// for the fixed info and characters for a string, so up to twice it
        /// is copied, clamped to the block's end; each reader uses only the
        /// part it knows the layout of.
        pub fn query(&self, sub_block: &str) -> Option<Vec<u8>> {
            let name: Vec<u16> = sub_block.encode_utf16().chain([0]).collect();
            let mut info: *mut c_void = std::ptr::null_mut();
            let mut len = 0u32;
            // SAFETY: `data` is what `load` filled; on success `info` points
            // inside it, which stays alive until the copy below.
            let found = unsafe {
                VerQueryValueW(
                    self.data.as_ptr().cast(),
                    name.as_ptr(),
                    &mut info,
                    &mut len,
                )
            };
            if found == 0 || info.is_null() {
                return None;
            }
            let have = self.data.len() * 4;
            let start = (info as usize).checked_sub(self.data.as_ptr() as usize)?;
            if start >= have {
                return None;
            }
            // Strings are UTF-16: twice the character count, within the block.
            let len = (len as usize * 2).min(have - start);
            // SAFETY: `start + len` is within `data`, checked above.
            Some(unsafe { std::slice::from_raw_parts(info.cast::<u8>(), len) }.to_vec())
        }
    }
}

/// Never reads anything off Windows.
#[cfg(not(windows))]
pub fn file_version(_path: &Path) -> Option<String> {
    None
}

/// Never reads anything off Windows.
#[cfg(not(windows))]
pub fn product_version(_path: &Path) -> Option<String> {
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
    fn system32(name: &str) -> std::path::PathBuf {
        let root = std::env::var_os("SystemRoot").expect("SystemRoot is set on Windows");
        Path::new(&root).join("System32").join(name)
    }

    #[cfg(windows)]
    #[test]
    fn a_system_dll_has_a_version() {
        let version =
            file_version(&system32("kernel32.dll")).expect("kernel32.dll has a version resource");
        assert_eq!(version.split('.').count(), 4);
        assert!(version.split('.').all(|p| p.parse::<u32>().is_ok()));
    }

    #[cfg(windows)]
    #[test]
    fn a_system_binary_has_a_product_version_string() {
        let text = product_version(&system32("cmd.exe")).expect("cmd.exe has a ProductVersion");
        assert!(
            text.chars().next().is_some_and(|c| c.is_ascii_digit()),
            "{text}"
        );
        assert!(!text.contains('\0'), "{text:?}");
    }

    #[test]
    fn a_missing_file_or_one_without_a_version_reads_as_none() {
        assert_eq!(file_version(Path::new("no-such-folder/missing.dll")), None);
        assert_eq!(
            product_version(Path::new("no-such-folder/missing.dll")),
            None
        );
        // Cargo.toml exists and is no executable.
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert_eq!(file_version(&manifest), None);
        assert_eq!(product_version(&manifest), None);
    }
}
