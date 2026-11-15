//! The DLLs a running process has loaded, for telling which hook DLL a
//! running game carries (#725). HLAE injects it and exits, so the game's own
//! module list is the only place left that says which file it was.

use std::path::PathBuf;

/// A module loaded in another process: its file and where its image sits in
/// that process's memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleImage {
    /// The path it was loaded from. Windows keeps this name even after the
    /// file is renamed, since a loaded DLL can be renamed but not overwritten
    /// (#1143).
    pub path: PathBuf,
    pub base: usize,
    pub size_bytes: usize,
}

/// Every module `pid` has loaded, 32-bit ones included, by full path. `None`
/// when the process can't be read (gone, or another user's).
pub fn loaded_modules(pid: u32) -> Option<Vec<PathBuf>> {
    module_images(pid).map(|images| images.into_iter().map(|m| m.path).collect())
}

/// [`loaded_modules`] with each one's place in memory.
#[cfg(windows)]
pub fn module_images(pid: u32) -> Option<Vec<ModuleImage>> {
    win::module_images(pid)
}

#[cfg(not(windows))]
pub fn module_images(_pid: u32) -> Option<Vec<ModuleImage>> {
    None
}

/// The bytes of `image` as loaded in `pid`, with any page that can't be read
/// left as zeros. `None` when the process can't be opened for reading or no
/// page of the image could be read.
#[cfg(windows)]
pub fn read_module_image(pid: u32, image: &ModuleImage) -> Option<Vec<u8>> {
    win::read_module_image(pid, image)
}

#[cfg(not(windows))]
pub fn read_module_image(_pid: u32, _image: &ModuleImage) -> Option<Vec<u8>> {
    None
}

#[cfg(windows)]
mod win {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;

    use super::ModuleImage;

    // The pointer type `junction.rs` declares `CloseHandle` with: two
    // declarations of one import must agree.
    type Handle = *mut std::ffi::c_void;
    const INVALID_HANDLE_VALUE: Handle = usize::MAX as Handle;
    const TH32CS_SNAPMODULE: u32 = 0x0000_0008;
    const TH32CS_SNAPMODULE32: u32 = 0x0000_0010;
    /// The snapshot asks again on this: the process was still loading.
    const ERROR_BAD_LENGTH: u32 = 24;
    const MAX_MODULE_NAME32: usize = 255;
    const MAX_PATH: usize = 260;
    const PROCESS_VM_READ: u32 = 0x0010;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    /// Read a page at a time, so one page that can't be read (a guard page,
    /// one decommitted) costs only itself.
    const PAGE_BYTES: usize = 4096;

    #[repr(C)]
    struct ModuleEntry32W {
        dw_size: u32,
        th32_module_id: u32,
        th32_process_id: u32,
        glblcnt_usage: u32,
        proccnt_usage: u32,
        mod_base_addr: *mut u8,
        mod_base_size: u32,
        h_module: isize,
        sz_module: [u16; MAX_MODULE_NAME32 + 1],
        sz_exe_path: [u16; MAX_PATH],
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> Handle;
        fn Module32FirstW(snapshot: Handle, entry: *mut ModuleEntry32W) -> i32;
        fn Module32NextW(snapshot: Handle, entry: *mut ModuleEntry32W) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
        fn GetLastError() -> u32;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        fn ReadProcessMemory(
            process: Handle,
            base: *const std::ffi::c_void,
            buffer: *mut std::ffi::c_void,
            size: usize,
            read: *mut usize,
        ) -> i32;
    }

    pub(super) fn module_images(pid: u32) -> Option<Vec<ModuleImage>> {
        let mut snapshot = INVALID_HANDLE_VALUE;
        for _ in 0..5 {
            // SAFETY: plain Win32 call; the handle is closed below.
            snapshot =
                unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid) };
            // SAFETY: as above.
            if snapshot != INVALID_HANDLE_VALUE || unsafe { GetLastError() } != ERROR_BAD_LENGTH {
                break;
            }
        }
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut out = Vec::new();
        // SAFETY: an all-zero MODULEENTRY32W is valid; dw_size is set before use.
        let mut entry: ModuleEntry32W = unsafe { std::mem::zeroed() };
        entry.dw_size = std::mem::size_of::<ModuleEntry32W>() as u32;
        // SAFETY: `entry` is a properly sized MODULEENTRY32W.
        let mut more = unsafe { Module32FirstW(snapshot, &mut entry) } != 0;
        while more {
            let len = entry
                .sz_exe_path
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(MAX_PATH);
            out.push(ModuleImage {
                path: PathBuf::from(OsString::from_wide(&entry.sz_exe_path[..len])),
                base: entry.mod_base_addr as usize,
                size_bytes: entry.mod_base_size as usize,
            });
            // SAFETY: as above.
            more = unsafe { Module32NextW(snapshot, &mut entry) } != 0;
        }
        // SAFETY: a handle this function opened.
        unsafe { CloseHandle(snapshot) };
        Some(out)
    }

    pub(super) fn read_module_image(pid: u32, image: &ModuleImage) -> Option<Vec<u8>> {
        // SAFETY: plain Win32 call; a null handle is checked and the handle
        // is closed below.
        let process =
            unsafe { OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return None;
        }
        let mut bytes = vec![0u8; image.size_bytes];
        let mut any_read = false;
        for (index, page) in bytes.chunks_mut(PAGE_BYTES).enumerate() {
            let mut read = 0usize;
            // SAFETY: `page` is a writable buffer of `page.len()` bytes;
            // ReadProcessMemory writes at most that many into it.
            let ok = unsafe {
                ReadProcessMemory(
                    process,
                    (image.base + index * PAGE_BYTES) as *const std::ffi::c_void,
                    page.as_mut_ptr().cast(),
                    page.len(),
                    &mut read,
                )
            } != 0;
            any_read |= ok && read > 0;
        }
        // SAFETY: a handle this function opened.
        unsafe { CloseHandle(process) };
        any_read.then_some(bytes)
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn this_process_lists_its_own_exe_and_kernel32() {
        let modules = loaded_modules(std::process::id()).expect("own modules");
        let exe = std::env::current_exe().unwrap();
        assert!(
            modules.iter().any(|m| m.file_name() == exe.file_name()),
            "{modules:?}"
        );
        assert!(
            modules.iter().any(|m| m
                .file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("kernel32.dll"))),
            "{modules:?}"
        );
    }

    #[test]
    fn this_process_reads_its_own_exe_image() {
        let exe = std::env::current_exe().unwrap();
        let image = module_images(std::process::id())
            .expect("own modules")
            .into_iter()
            .find(|m| m.path.file_name() == exe.file_name())
            .expect("own exe in the list");
        let bytes = read_module_image(std::process::id(), &image).expect("own image");
        assert_eq!(bytes.len(), image.size_bytes);
        assert_eq!(&bytes[..2], b"MZ");
    }
}
