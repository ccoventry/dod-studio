//! The DLLs a running process has loaded, for telling which hook DLL a
//! running game carries (#725). HLAE injects it and exits, so the game's own
//! module list is the only place left that says which file it was.

use std::path::PathBuf;

/// Every module `pid` has loaded, 32-bit ones included, by full path. `None`
/// when the process can't be read (gone, or another user's).
#[cfg(windows)]
pub fn loaded_modules(pid: u32) -> Option<Vec<PathBuf>> {
    win::loaded_modules(pid)
}

#[cfg(not(windows))]
pub fn loaded_modules(_pid: u32) -> Option<Vec<PathBuf>> {
    None
}

#[cfg(windows)]
mod win {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;

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
    }

    pub(super) fn loaded_modules(pid: u32) -> Option<Vec<PathBuf>> {
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
            out.push(PathBuf::from(OsString::from_wide(
                &entry.sz_exe_path[..len],
            )));
            // SAFETY: as above.
            more = unsafe { Module32NextW(snapshot, &mut entry) } != 0;
        }
        // SAFETY: a handle this function opened.
        unsafe { CloseHandle(snapshot) };
        Some(out)
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
}
