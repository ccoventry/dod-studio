//! `ds_demo_other_drives 1`: `viewdemo` and `playdemo` open a demo on
//! another drive by its full path, `viewdemo "D:/folder/demo"` (issue #776).
//!
//! ## Why the game can't by itself
//!
//! Every demo open ends in the file system's `Open(name, "rb", pathID = 0)`:
//! `hw.dll`'s `FS_Open` for `playdemo`, and `Core.dll`'s
//! `DemoFile::LoadDemo` (reached through `DemoPlayer.dll`) for `viewdemo`.
//! `Open` only ever tries `<search path> + name` for each search path in
//! turn; an absolute path is never opened as such, so `D:/x.dem` ends in
//! "Coudn't open demo file". The 25th Anniversary `FileSystem_Stdio.dll`
//! asserts before even trying: `!strchr(pFileName, ':')`
//! (`BaseFileSystem.cpp` line 471, `_wassert`, a fatal dialog). Nothing on
//! the way there looks at the name, on either build.
//!
//! ## What this does
//!
//! One vftable slot of the file system object GameUI and the engine share
//! (`VFileSystem009`, whose `Find*` slots `demo_list_folders` hooks): `Open`,
//! slot 10. A name that starts with a drive, `D:/` or `D:\`, is opened as
//! the rest of the path (`folder/demo.dem`) after `D:\` has been added as a
//! search path, once per drive (`AddSearchPath`, slot 4, which takes a path
//! with a drive letter as it is and appends it last, so no game file is
//! shadowed). The search path carries its own path ID, so only opens that
//! name no path ID -- the demo opens -- look there; the engine's `GAME` and
//! `GAMECONFIG` lookups never do. Any other name passes straight through:
//! the cost is a byte compare per open.
//!
//! The rewrite happens in our slot, before the original `Open` and its `:`
//! assert, which is why this is a hook and not a console command.
//! `AddSearchPath` runs on the thread that opens the demo, the engine's.
//!
//! Without the hook DLL, a demo on another drive still needs Studio's
//! + Link Folder to Game (a junction, #376) or a copy into `dod\`.
//!
//! `tools/verify_demo_other_drives.py` checks the slots and the `:` assert
//! against both movie installs.

// The hook is only built for the 32-bit DLL; a host build compiles the
// path logic for the tests alone.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::names::console_name;
use crate::setting::Setting;

pub const NAME: &str = console_name!("demo_other_drives");

/// `ds_demo_other_drives`, or its default when the cvar could not be
/// registered.
pub static SETTING: Setting = Setting::new(NAME, true);
/// Bit `letter - b'A'` is set once that drive's root is a search path.
static DRIVES: AtomicU32 = AtomicU32::new(0);
/// Set once the `Open` slot is ours.
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Studio looks for these bytes in the DLL file
/// (`native::preview_in_place::HOOK_FULL_PATH_MARKER`, #779): a DLL that has
/// them sends a batch's, a review's and Go's demo on another drive to
/// `viewdemo` by its full path ([`ready`], `review::viewdemo_line`), so Studio
/// plays such a demo in place. For an older DLL it copies the demo into
/// `dod\` for the batch. The install line prints it, which keeps it in the
/// binary.
pub const FULL_PATH_MARKER: &str = "ds-viewdemo-full-path-779";

/// Whether `viewdemo "D:/..."` opens a demo now: the hook is installed and
/// the setting is on. `Err` says why not.
pub(crate) fn ready() -> Result<(), String> {
    if !INSTALLED.load(Ordering::Acquire) {
        return Err(
            "the hook that opens demos on other drives is not installed in this game".into(),
        );
    }
    if !enabled() {
        return Err(format!("{NAME} is 0 (set it to 1 to play it in place)"));
    }
    Ok(())
}

/// `IFileSystem` (`VFileSystem009`) slots.
const FS_SLOT_ADD_SEARCH_PATH: usize = 4;
const FS_SLOT_OPEN: usize = 10;
/// The path ID our search paths carry. An open that names a path ID
/// (`GAME`, `GAMECONFIG`, ...) skips them; the demo opens name none.
const PATH_ID: &std::ffi::CStr = c"DODSTUDIO";

pub(crate) fn enabled() -> bool {
    SETTING.on()
}

/// `D:/folder/demo` split into the drive letter (upper case) and the path
/// under the drive's root, its leading separators dropped. `None` for
/// anything else: a relative name, a bare `D:` or `D:/`, a UNC path.
fn drive_relative(name: &[u8]) -> Option<(u8, &[u8])> {
    let [letter, b':', sep, rest @ ..] = name else {
        return None;
    };
    if !letter.is_ascii_alphabetic() || !matches!(sep, b'/' | b'\\') {
        return None;
    }
    let start = rest.iter().position(|c| !matches!(c, b'/' | b'\\'))?;
    Some((letter.to_ascii_uppercase(), &rest[start..]))
}

/// The search path for a drive, `D:\`, NUL-terminated.
fn drive_root(letter: u8) -> [u8; 4] {
    [letter, b':', b'\\', 0]
}

#[cfg(target_arch = "x86")]
mod hook {
    use std::ffi::{CStr, CString, c_char, c_void};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    use super::*;
    use crate::demo_list_folders::FILESYSTEM_BUILDS;

    type OpenFn = unsafe extern "thiscall" fn(
        *mut c_void,
        *const c_char,
        *const c_char,
        *const c_char,
    ) -> *mut c_void;
    type AddSearchPathFn = unsafe extern "thiscall" fn(*mut c_void, *const c_char, *const c_char);
    type CreateInterfaceFn = unsafe extern "C" fn(*const c_char, *mut i32) -> *mut c_void;

    static OPEN: AtomicUsize = AtomicUsize::new(0);
    static ADD_SEARCH_PATH: AtomicUsize = AtomicUsize::new(0);
    static DONE: AtomicBool = AtomicBool::new(false);

    unsafe fn cast<F: Copy>(address: usize) -> F {
        unsafe { std::mem::transmute_copy(&address) }
    }

    /// Adds `D:\` as a search path the first time drive `letter` is named.
    /// True when this call added it.
    unsafe fn ensure_drive(fs: *mut c_void, letter: u8) -> bool {
        let bit = 1u32 << (letter - b'A');
        if DRIVES.fetch_or(bit, Ordering::AcqRel) & bit != 0 {
            return false;
        }
        let root = drive_root(letter);
        // Safety: slot 4 of a checked build, read at install.
        let add: AddSearchPathFn = unsafe { cast(ADD_SEARCH_PATH.load(Ordering::Acquire)) };
        unsafe { add(fs, root.as_ptr() as *const c_char, PATH_ID.as_ptr()) };
        true
    }

    /// Our `Open`: a `D:/...` name is opened as `...` under the `D:\` search
    /// path. Everything else goes to the file system's own `Open` as it came.
    unsafe extern "thiscall" fn open(
        fs: *mut c_void,
        name: *const c_char,
        options: *const c_char,
        path_id: *const c_char,
    ) -> *mut c_void {
        // Safety: saved before the slot was repointed.
        let original: OpenFn = unsafe { cast(OPEN.load(Ordering::Acquire)) };
        if !name.is_null() && enabled() {
            // Safety: the engine hands a NUL-terminated name.
            let bytes = unsafe { CStr::from_ptr(name) }.to_bytes();
            if let Some((letter, rest)) = drive_relative(bytes)
                && let Ok(rest) = CString::new(rest)
            {
                let added = unsafe { ensure_drive(fs, letter) };
                let handle = unsafe { original(fs, rest.as_ptr(), options, path_id) };
                if added {
                    // After the open: the console print may itself open a
                    // file (`-condebug`), which comes back through here.
                    unsafe {
                        crate::debug::report(&format!(
                            "demo_other_drives: {}:\\ added as a search path for {}",
                            letter as char,
                            String::from_utf8_lossy(bytes)
                        ))
                    };
                }
                return handle;
            }
        }
        unsafe { original(fs, name, options, path_id) }
    }

    fn install(filesystem: usize) -> Result<&'static str, String> {
        // Safety: a module handle the loader gave us.
        let build = unsafe { crate::pe::image_identity(filesystem as *mut u8) }.and_then(
            |(stamp, size)| {
                FILESYSTEM_BUILDS
                    .iter()
                    .find(|b| b.time_date_stamp == stamp && b.size_of_image == size)
            },
        );
        let Some(build) = build else {
            return Err("FileSystem_Stdio.dll is a build this was not checked against".into());
        };
        let create =
            unsafe { GetProcAddress(filesystem as _, c"CreateInterface".as_ptr() as *const u8) }
                .ok_or("FileSystem_Stdio.dll exports no CreateInterface")?;
        // Safety: the interface factory's signature, on a checked build.
        let create: CreateInterfaceFn = unsafe { std::mem::transmute(create) };
        let fs = unsafe { create(c"VFileSystem009".as_ptr(), std::ptr::null_mut()) };
        if fs.is_null() {
            return Err("FileSystem_Stdio.dll did not hand out VFileSystem009".into());
        }
        // Safety: slots of a vftable checked by tools/verify_demo_other_drives.py
        // on exactly these builds. `AddSearchPath` is read, never repointed.
        unsafe {
            let vftable = *(fs as *const usize);
            ADD_SEARCH_PATH.store(
                *((vftable + FS_SLOT_ADD_SEARCH_PATH * 4) as *const usize),
                Ordering::Release,
            );
            crate::patch::swap_vftable_slot(
                vftable + FS_SLOT_OPEN * 4,
                open as OpenFn as usize,
                &OPEN,
            )?;
        }
        Ok(build.name)
    }

    pub(super) fn poll() {
        if DONE.load(Ordering::Relaxed) {
            return;
        }
        let handle = unsafe { GetModuleHandleA(c"FileSystem_Stdio.dll".as_ptr() as *const u8) };
        if handle.is_null() {
            return; // not loaded yet: try again next frame
        }
        DONE.store(true, Ordering::Relaxed);
        let message = match install(handle as usize) {
            Ok(build) => {
                INSTALLED.store(true, Ordering::Release);
                format!(
                    "demo_other_drives: installed ({build} file system); {NAME} 1 opens a demo on another drive by its full path (#776; {FULL_PATH_MARKER})"
                )
            }
            Err(why) => format!("demo_other_drives: not installed -- {why}"),
        };
        unsafe { crate::debug::report(&message) };
    }
}

/// Installs the hook once `FileSystem_Stdio.dll` is loaded. Runs every frame
/// from `commands::poll`; one atomic load after.
pub fn poll() {
    #[cfg(target_arch = "x86")]
    hook::poll();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drive_path_splits_into_the_drive_and_the_rest() {
        assert_eq!(
            drive_relative(b"D:/dodstudio_preview_test/x.dem"),
            Some((b'D', &b"dodstudio_preview_test/x.dem"[..]))
        );
        // Lower case (DemoFile::LoadDemo lower-cases the name) and backslashes.
        assert_eq!(
            drive_relative(b"d:\\demos\\x.dem"),
            Some((b'D', &b"demos\\x.dem"[..]))
        );
        // Doubled separators after the drive are dropped.
        assert_eq!(drive_relative(b"E://x.dem"), Some((b'E', &b"x.dem"[..])));
    }

    #[test]
    fn anything_else_passes_through() {
        for name in [
            &b"demos/x.dem"[..],
            b"../other/x.dem",
            b"D:",
            b"D:/",
            b"D:\\\\",
            b"D:x.dem",
            b"1:/x.dem",
            b"\\\\server\\share\\x.dem",
            b"demos/d:/x.dem",
            b"",
        ] {
            assert_eq!(
                drive_relative(name),
                None,
                "{}",
                String::from_utf8_lossy(name)
            );
        }
    }

    #[test]
    fn the_search_path_is_the_drive_root() {
        assert_eq!(&drive_root(b'D'), b"D:\\\0");
    }
}
