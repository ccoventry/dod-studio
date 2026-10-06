//! `dodstudio_demo_list_folders 1`: the Load Demo window lists folders too,
//! and opens them, instead of only the demos sitting in `dod/` (issue #408).
//!
//! ## How the window fills its list
//!
//! The window is `CDemoPlayerFileDialog` in `valve\cl_dlls\GameUI.dll`. When
//! it is built it calls one method (the "fill", `thiscall`, no arguments,
//! both builds) that empties its `ListPanel` and asks the file system for
//! `FindFirst("*.dem")` / `FindNext` / `FindClose`, adding each name as a row
//! whose `demoname` is that name. Load (or a double-click, which sends the
//! same command) posts `DemoSelected {demoname}` to the demo bar, and the bar
//! runs `viewdemo <demoname>`, cut only at `;` or a newline. `viewdemo` takes
//! a path relative to `dod/`, `../` included (tested live, #408).
//!
//! ## What this changes
//!
//! Two sets of vftable slots, no code:
//!
//! - **The file system's `FindFirst`/`FindNext`/`FindClose`** (slots 27, 28
//!   and 30 of the one `VFileSystem009` object `FileSystem_Stdio.dll` hands
//!   everyone). A call whose wildcard is GameUI's own `"*.dem"` literal -- the
//!   fill is the only code that passes it -- gets this module's listing of the
//!   current folder instead: `../`, each subfolder as `name/`, then the demos.
//!   Every row is a full path from `dod/`, so a demo row is already what
//!   `viewdemo` needs; one with a space is wrapped in quotes, which the
//!   demo bar passes through unchanged. Every other call goes straight to the
//!   file system.
//! - **The window's `OnCommand`** (slot 87). `load` on a row ending in `/`
//!   makes that folder current and runs the fill again, which re-reads the
//!   list; any other row, and any other command, goes to the window's own
//!   handler.
//!
//! The current folder outlives the window, so it reopens where it was left.
//! On by default (a box on the DoD Studio window's Settings tab); off, the
//! list looks stock. The hooks are in
//! place either way and pass everything through while it is off.
//!
//! ## Per build
//!
//! [`GAMEUI_BUILDS`] and [`FILESYSTEM_BUILDS`] name each DLL build by PE
//! timestamp and image size; anything else is refused.
//! `tools/verify_demo_list_folders.py` checks every address and slot here
//! against both movie installs.

// The addresses are only used by the 32-bit build; a host build compiles
// them for the tests alone.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use crate::engine::CvarSPartial;
use crate::names::console_name;

pub const NAME: &str = console_name!("demo_list_folders");

/// The fallback path's flag, when the cvar could not be registered.
pub static ENABLED: AtomicBool = AtomicBool::new(true);
static CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());

/// Called by `commands.rs` once `dodstudio_demo_list_folders` is registered.
pub fn set_cvar(cvar: *mut CvarSPartial) {
    CVAR.store(cvar, Ordering::Release);
}

pub(crate) fn enabled() -> bool {
    let cvar = CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        ENABLED.load(Ordering::Relaxed)
    } else {
        // Safety: the engine owns the cvar for the session.
        unsafe { (*cvar).value != 0.0 }
    }
}

/// A second on/off setting of this module, a cvar or, when that couldn't be
/// registered, the fallback toggle's flag.
pub struct Setting {
    pub name: &'static str,
    pub fallback: AtomicBool,
    cvar: AtomicPtr<CvarSPartial>,
}

impl Setting {
    const fn new(name: &'static str, on: bool) -> Self {
        Self {
            name,
            fallback: AtomicBool::new(on),
            cvar: AtomicPtr::new(std::ptr::null_mut()),
        }
    }

    /// Called by `commands.rs` once the cvar is registered.
    pub fn set_cvar(&self, cvar: *mut CvarSPartial) {
        self.cvar.store(cvar, Ordering::Release);
    }

    pub fn on(&self) -> bool {
        let cvar = self.cvar.load(Ordering::Acquire);
        if cvar.is_null() {
            self.fallback.load(Ordering::Relaxed)
        } else {
            // Safety: the engine owns the cvar for the session.
            unsafe { (*cvar).value != 0.0 }
        }
    }
}

/// `dodstudio_demo_list_hide_empty 1` (the default): while folders are
/// listed, a folder with no demo anywhere inside it, at any depth, isn't.
pub static HIDE_EMPTY: Setting = Setting::new(console_name!("demo_list_hide_empty"), true);

/// `dodstudio_demo_list_count_subfolders 1` (the default): a folder's demo
/// count on the Demos tab includes its subfolders, not just the demos
/// directly in it.
pub static COUNT_SUBFOLDERS: Setting =
    Setting::new(console_name!("demo_list_count_subfolders"), true);

/// For the fallback toggle commands' bare-name query.
pub fn hide_empty_status() -> String {
    if HIDE_EMPTY.on() {
        "folders with no demo anywhere inside them aren't listed".to_string()
    } else {
        "every folder is listed, with or without demos".to_string()
    }
}

pub fn count_subfolders_status() -> String {
    if COUNT_SUBFOLDERS.on() {
        "a folder's count includes its subfolders' demos".to_string()
    } else {
        "a folder's count is only the demos directly in it".to_string()
    }
}

/// The folder the lists show, as a path from `dod/` (`""` for `dod/`).
static CURRENT_FOLDER: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// The folder the Load Demo window and the Demos tab are in.
pub fn current_folder() -> String {
    CURRENT_FOLDER.lock().map(|f| f.clone()).unwrap_or_default()
}

/// For the fallback toggle command's bare-name query.
pub fn status() -> String {
    if enabled() {
        "the Load Demo window lists and opens folders".to_string()
    } else {
        "the Load Demo window lists only the demos in dod/".to_string()
    }
}

/// One `GameUI.dll` build: its identity and where the window's pieces are.
pub struct GameUiBuild {
    pub name: &'static str,
    pub time_date_stamp: u32,
    pub size_of_image: u32,
    /// `CDemoPlayerFileDialog`'s vftable.
    pub file_dialog_vftable: usize,
    /// The fill: empties the list and adds a row per `FindFirst`/`FindNext` name.
    pub fill: usize,
    /// The `"*.dem"` literal the fill passes to `FindFirst`.
    pub wildcard: usize,
    /// The window's `ListPanel *`.
    pub list_field: usize,
}

pub const GAMEUI_BUILDS: [GameUiBuild; 2] = [
    GameUiBuild {
        name: "pre-Anniversary",
        time_date_stamp: 0x5f28_cefc,
        size_of_image: 0xe_3000,
        file_dialog_vftable: 0x9_425c,
        fill: 0x2_0910,
        wildcard: 0xa_ef14,
        list_field: 0x110,
    },
    GameUiBuild {
        name: "25th Anniversary",
        time_date_stamp: 0x6704_99b2,
        size_of_image: 0xd_f000,
        file_dialog_vftable: 0x9_ab80,
        fill: 0x2_7210,
        wildcard: 0x9_af10,
        list_field: 0x118,
    },
];

/// One `FileSystem_Stdio.dll` build.
pub struct FileSystemBuild {
    pub name: &'static str,
    pub time_date_stamp: u32,
    pub size_of_image: u32,
}

pub const FILESYSTEM_BUILDS: [FileSystemBuild; 2] = [
    FileSystemBuild {
        name: "pre-Anniversary",
        time_date_stamp: 0x5f28_ceff,
        size_of_image: 0x1_f000,
    },
    FileSystemBuild {
        name: "25th Anniversary",
        time_date_stamp: 0x6704_9a0a,
        size_of_image: 0x1_0000,
    },
];

/// `CDemoPlayerFileDialog`'s `OnCommand(const char *)`.
const SLOT_ON_COMMAND: usize = 87;
/// `ListPanel::GetSelectedItem(int)`, `IsValidItemID(int)`, `GetItem(int)`.
const LIST_SLOT_GET_SELECTED_ITEM: usize = 176;
const LIST_SLOT_IS_VALID_ITEM_ID: usize = 170;
const LIST_SLOT_GET_ITEM: usize = 153;
/// `KeyValues::GetString(const char *key, const char *default)`.
const KEYVALUES_SLOT_GET_STRING: usize = 12;
/// The key each row's name is stored under.
const ROW_KEY: &std::ffi::CStr = c"demoname";
/// `IFileSystem` (`VFileSystem009`) slots.
const FS_SLOT_FIND_FIRST: usize = 27;
const FS_SLOT_FIND_NEXT: usize = 28;
const FS_SLOT_FIND_IS_DIRECTORY: usize = 29;
const FS_SLOT_FIND_CLOSE: usize = 30;

/// The handle this module's own listing hands out; the file system's real
/// handles are small indices.
const OWN_HANDLE: i32 = 0x5d5d_0001;
const _: () = assert!(OWN_HANDLE > 0x1000);

/// `folder` (a row ending in `/`) resolved into a clean folder path from
/// `dod/`: `""` for `dod/` itself, otherwise ending in `/`. A `..` cancels the
/// folder before it and is kept when there is none, so `../` climbs out.
fn resolve(folder: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in folder.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|p| *p != "..") => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.iter().map(|p| format!("{p}/")).collect()
}

/// The rows for `folder`: `../` first, then subfolders, then demos, each a
/// path from `dod/`. The window sorts them itself; the order here only
/// decides which duplicate (the same name in two search paths) is kept.
///
/// The file system's listing merges every search path (`dod/`, `valve/`, the
/// install folder), so `dod/` would list `valve/` and `WindowsCrashDumps/`.
/// `on_disk(path, is_dir)` keeps only what really is at `dod/<path>`, as the
/// Demo Analyzer's folder view shows the disk (user's call, 2026-10-05).
fn rows(
    folder: &str,
    entries: impl IntoIterator<Item = (String, bool)>,
    on_disk: impl Fn(&str, bool) -> bool,
) -> Vec<String> {
    let mut out = vec![format!("{folder}../")];
    let mut demos = Vec::new();
    for (name, is_dir) in entries {
        // `.` and `..` are this folder and the one above (the `../` row
        // already climbs out). Never asked about: counting them counted this
        // folder's whole tree again, and the parent's (found live 2026-10-05).
        if name == "." || name == ".." || !on_disk(&format!("{folder}{name}"), is_dir) {
            continue;
        }
        if is_dir {
            out.push(format!("{folder}{name}/"));
        } else if name.to_ascii_lowercase().ends_with(".dem") {
            let path = format!("{folder}{name}");
            // The demo bar runs `viewdemo <row>` unquoted.
            demos.push(if path.contains(' ') {
                format!("\"{path}\"")
            } else {
                path
            });
        }
    }
    out.extend(demos);
    let mut seen = std::collections::HashSet::new();
    out.retain(|row| seen.insert(row.to_ascii_lowercase()));
    out
}

#[cfg(target_arch = "x86")]
mod hook {
    use std::ffi::{CStr, CString, c_char, c_void};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    use super::*;

    type OnCommandFn = unsafe extern "thiscall" fn(*mut c_void, *const c_char);
    type FillFn = unsafe extern "thiscall" fn(*mut c_void);
    type FindFirstFn = unsafe extern "thiscall" fn(
        *mut c_void,
        *const c_char,
        *mut i32,
        *const c_char,
    ) -> *const c_char;
    type FindNextFn = unsafe extern "thiscall" fn(*mut c_void, i32) -> *const c_char;
    // `bool` in `al`; read all of `eax` and keep the low byte.
    type FindIsDirectoryFn = unsafe extern "thiscall" fn(*mut c_void, i32) -> u32;
    type FindCloseFn = unsafe extern "thiscall" fn(*mut c_void, i32);
    type CreateInterfaceFn = unsafe extern "C" fn(*const c_char, *mut i32) -> *mut c_void;
    type IntFn = unsafe extern "thiscall" fn(*mut c_void, i32) -> i32;
    type PtrFn = unsafe extern "thiscall" fn(*mut c_void, i32) -> *mut c_void;
    type GetStringFn =
        unsafe extern "thiscall" fn(*mut c_void, *const c_char, *const c_char) -> *const c_char;

    static ON_COMMAND: AtomicUsize = AtomicUsize::new(0);
    static FILL: AtomicUsize = AtomicUsize::new(0);
    static WILDCARD: AtomicUsize = AtomicUsize::new(0);
    static LIST_FIELD: AtomicUsize = AtomicUsize::new(0);
    static FIND_FIRST: AtomicUsize = AtomicUsize::new(0);
    static FIND_NEXT: AtomicUsize = AtomicUsize::new(0);
    static FIND_IS_DIRECTORY: AtomicUsize = AtomicUsize::new(0);
    static FIND_CLOSE: AtomicUsize = AtomicUsize::new(0);
    static DONE: AtomicBool = AtomicBool::new(false);

    /// The current folder, and the listing being handed out.
    struct Browser {
        folder: String,
        rows: Vec<CString>,
        next: usize,
    }

    static BROWSER: Mutex<Browser> = Mutex::new(Browser {
        folder: String::new(),
        rows: Vec::new(),
        next: 0,
    });

    unsafe fn slot(object: *mut c_void, index: usize) -> usize {
        unsafe { *(*(object as *const *const usize)).add(index) }
    }

    unsafe fn cast<F: Copy>(address: usize) -> F {
        unsafe { std::mem::transmute_copy(&address) }
    }

    fn module(name: &CStr) -> Option<usize> {
        let handle = unsafe { GetModuleHandleA(name.as_ptr() as *const u8) };
        (!handle.is_null()).then_some(handle as usize)
    }

    fn identity(base: usize) -> Option<(u32, u32)> {
        // Safety: a module handle the loader gave us.
        unsafe { crate::pe::image_identity(base as *mut u8) }
    }

    /// Lists `folder` through the real file system, as rows.
    unsafe fn list(fs: *mut c_void, folder: &str, path_id: *const c_char) -> Vec<CString> {
        let mut entries = Vec::new();
        let Ok(wildcard) = CString::new(format!("{folder}*")) else {
            return Vec::new();
        };
        unsafe {
            let find_first: FindFirstFn = cast(FIND_FIRST.load(Ordering::Relaxed));
            let find_next: FindNextFn = cast(FIND_NEXT.load(Ordering::Relaxed));
            let is_directory: FindIsDirectoryFn = cast(FIND_IS_DIRECTORY.load(Ordering::Relaxed));
            let find_close: FindCloseFn = cast(FIND_CLOSE.load(Ordering::Relaxed));
            let mut handle = 0i32;
            let mut name = find_first(fs, wildcard.as_ptr(), &mut handle, path_id);
            while !name.is_null() {
                let text = CStr::from_ptr(name).to_string_lossy().into_owned();
                entries.push((text, is_directory(fs, handle) & 0xff != 0));
                name = find_next(fs, handle);
            }
            find_close(fs, handle);
        }
        let dod = crate::texture_hires::game_dir();
        let hide_empty = super::HIDE_EMPTY.on();
        // No counting here, on the game thread: a folder known to be empty
        // is left out, and the rest are counted in the background
        // (`folder_counts`), the Demos tab listing again when that is done.
        let to_count = std::cell::RefCell::new(Vec::new());
        let listed = rows(folder, entries, |path, is_dir| {
            let full = dod.join(path);
            if !is_dir {
                return full.is_file();
            }
            if !full.is_dir() || (hide_empty && crate::folder_counts::known_empty(&full)) {
                return false;
            }
            to_count.borrow_mut().push(full);
            true
        });
        // The up row's count too: this folder's total from its parts, plus
        // the rest of the parent, so nothing is counted twice.
        let here = dod.join(folder);
        crate::folder_counts::count_later(
            folder,
            to_count.into_inner(),
            Some((here.join(".."), here)),
        );
        listed
            .into_iter()
            .filter_map(|row| CString::new(row).ok())
            .collect()
    }

    unsafe extern "thiscall" fn find_first(
        fs: *mut c_void,
        wildcard: *const c_char,
        handle: *mut i32,
        path_id: *const c_char,
    ) -> *const c_char {
        unsafe {
            if wildcard as usize == WILDCARD.load(Ordering::Relaxed) && enabled() {
                let Ok(mut browser) = BROWSER.lock() else {
                    return std::ptr::null();
                };
                let folder = browser.folder.clone();
                browser.rows = list(fs, &folder, path_id);
                browser.next = 1;
                *handle = OWN_HANDLE;
                return browser
                    .rows
                    .first()
                    .map_or(std::ptr::null(), |row| row.as_ptr());
            }
            let original: FindFirstFn = cast(FIND_FIRST.load(Ordering::Relaxed));
            original(fs, wildcard, handle, path_id)
        }
    }

    unsafe extern "thiscall" fn find_next(fs: *mut c_void, handle: i32) -> *const c_char {
        if handle == OWN_HANDLE {
            let Ok(mut browser) = BROWSER.lock() else {
                return std::ptr::null();
            };
            let at = browser.next;
            browser.next += 1;
            return browser
                .rows
                .get(at)
                .map_or(std::ptr::null(), |row| row.as_ptr());
        }
        unsafe {
            let original: FindNextFn = cast(FIND_NEXT.load(Ordering::Relaxed));
            original(fs, handle)
        }
    }

    unsafe extern "thiscall" fn find_close(fs: *mut c_void, handle: i32) {
        if handle == OWN_HANDLE {
            // The window has copied every name by now.
            if let Ok(mut browser) = BROWSER.lock() {
                browser.rows.clear();
                browser.next = 0;
            }
            return;
        }
        unsafe {
            let original: FindCloseFn = cast(FIND_CLOSE.load(Ordering::Relaxed));
            original(fs, handle)
        }
    }

    /// The selected row's `demoname`, as the window's own `load` reads it.
    unsafe fn selected_row(dialog: *mut c_void) -> Option<String> {
        unsafe {
            let list = *((dialog as *const u8).add(LIST_FIELD.load(Ordering::Relaxed))
                as *const *mut c_void);
            if list.is_null() {
                return None;
            }
            let get_selected: IntFn = cast(slot(list, LIST_SLOT_GET_SELECTED_ITEM));
            let is_valid: IntFn = cast(slot(list, LIST_SLOT_IS_VALID_ITEM_ID));
            let get_item: PtrFn = cast(slot(list, LIST_SLOT_GET_ITEM));
            let id = get_selected(list, 0);
            if is_valid(list, id) & 0xff == 0 {
                return None;
            }
            let row = get_item(list, id);
            if row.is_null() {
                return None;
            }
            let get_string: GetStringFn = cast(slot(row, KEYVALUES_SLOT_GET_STRING));
            let text = get_string(row, ROW_KEY.as_ptr(), c"".as_ptr());
            (!text.is_null()).then(|| CStr::from_ptr(text).to_string_lossy().into_owned())
        }
    }

    unsafe extern "thiscall" fn on_command(dialog: *mut c_void, command: *const c_char) {
        unsafe {
            if enabled()
                && !command.is_null()
                && CStr::from_ptr(command).to_bytes() == b"load"
                && let Some(row) = selected_row(dialog)
                && row.ends_with('/')
            {
                let folder = resolve(&row);
                if let Ok(mut browser) = BROWSER.lock() {
                    browser.folder.clone_from(&folder);
                }
                if let Ok(mut current) = super::CURRENT_FOLDER.lock() {
                    current.clone_from(&folder);
                }
                crate::debug::report(&format!("demo_list_folders: opened dod/{folder}"));
                let fill: FillFn = cast(FILL.load(Ordering::Relaxed));
                fill(dialog);
                return;
            }
            let original: OnCommandFn = cast(ON_COMMAND.load(Ordering::Relaxed));
            original(dialog, command)
        }
    }

    /// Repoints one vftable slot, saving what it held into `original`
    /// **before** the slot changes. The other order leaves a window where the
    /// slot already points at our thunk but `original` still reads 0, and a
    /// call landing in that window (any thread) would jump to address 0.
    unsafe fn swap(vftable_slot: usize, ours: usize, original: &AtomicUsize) -> Result<(), String> {
        let old = unsafe { *(vftable_slot as *const usize) };
        original.store(old, Ordering::Release);
        if unsafe { crate::patch::write_code_bytes(vftable_slot, &(ours as u32).to_le_bytes()) } {
            Ok(())
        } else {
            Err(format!(
                "could not make the vftable at {vftable_slot:#x} writable"
            ))
        }
    }

    fn install(gameui: usize, filesystem: usize) -> Result<String, String> {
        let ui = identity(gameui).and_then(|(stamp, size)| {
            GAMEUI_BUILDS
                .iter()
                .find(|b| b.time_date_stamp == stamp && b.size_of_image == size)
        });
        let Some(ui) = ui else {
            return Err("GameUI.dll is a build this was not checked against".into());
        };
        let fs_build = identity(filesystem).and_then(|(stamp, size)| {
            FILESYSTEM_BUILDS
                .iter()
                .find(|b| b.time_date_stamp == stamp && b.size_of_image == size)
        });
        let Some(fs_build) = fs_build else {
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

        FILL.store(gameui + ui.fill, Ordering::Relaxed);
        WILDCARD.store(gameui + ui.wildcard, Ordering::Relaxed);
        LIST_FIELD.store(ui.list_field, Ordering::Relaxed);
        // Safety: slots of vftables checked by tools/verify_demo_list_folders.py
        // on exactly these builds.
        unsafe {
            let fs_vftable = *(fs as *const usize);
            FIND_IS_DIRECTORY.store(slot(fs, FS_SLOT_FIND_IS_DIRECTORY), Ordering::Relaxed);
            swap(
                fs_vftable + FS_SLOT_FIND_FIRST * 4,
                find_first as FindFirstFn as usize,
                &FIND_FIRST,
            )?;
            swap(
                fs_vftable + FS_SLOT_FIND_NEXT * 4,
                find_next as FindNextFn as usize,
                &FIND_NEXT,
            )?;
            swap(
                fs_vftable + FS_SLOT_FIND_CLOSE * 4,
                find_close as FindCloseFn as usize,
                &FIND_CLOSE,
            )?;
            swap(
                gameui + ui.file_dialog_vftable + SLOT_ON_COMMAND * 4,
                on_command as OnCommandFn as usize,
                &ON_COMMAND,
            )?;
        }
        Ok(format!("{} GameUI, {} file system", ui.name, fs_build.name))
    }

    pub(super) fn poll() {
        if DONE.load(Ordering::Relaxed) {
            return;
        }
        let (Some(gameui), Some(filesystem)) =
            (module(c"GameUI.dll"), module(c"FileSystem_Stdio.dll"))
        else {
            return; // not loaded yet: try again next frame
        };
        DONE.store(true, Ordering::Relaxed);
        let message = match install(gameui, filesystem) {
            Ok(build) => format!(
                "demo_list_folders: installed ({build}); {NAME} 1 lists folders in the Load Demo window (#408)"
            ),
            Err(why) => format!("demo_list_folders: not installed -- {why}"),
        };
        unsafe { crate::debug::report(&message) };
    }
}

/// Installs the hooks once `GameUI.dll` and `FileSystem_Stdio.dll` are
/// loaded. Runs every frame from `commands::poll`; one atomic load after.
pub fn poll() {
    #[cfg(target_arch = "x86")]
    hook::poll();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_resolve_to_clean_paths_from_dod() {
        assert_eq!(resolve("test/"), "test/");
        assert_eq!(resolve("test/sub/"), "test/sub/");
        assert_eq!(resolve("test/../"), "");
        assert_eq!(resolve("../"), "../");
        assert_eq!(resolve("../../"), "../../");
        assert_eq!(resolve("../../dod/test/"), "../../dod/test/");
        assert_eq!(resolve("test/sub/../../../"), "../");
        assert_eq!(resolve("temp demos/"), "temp demos/");
    }

    #[test]
    fn rows_are_parent_folders_then_demos_with_spaces_quoted() {
        let entries = [
            (".".to_string(), true),
            ("..".to_string(), true),
            ("temp demos".to_string(), true),
            ("a.dem".to_string(), false),
            ("B.DEM".to_string(), false),
            ("notes.txt".to_string(), false),
            ("my clip.dem".to_string(), false),
        ];
        assert_eq!(
            rows("test/", entries, |_, _| true),
            vec![
                "test/../",
                "test/temp demos/",
                "test/a.dem",
                "test/B.DEM",
                "\"test/my clip.dem\"",
            ]
        );
    }

    #[test]
    fn a_name_in_two_search_paths_is_listed_once() {
        let entries = [
            ("a.dem".to_string(), false),
            ("sub".to_string(), true),
            ("A.dem".to_string(), false),
            ("sub".to_string(), true),
        ];
        assert_eq!(rows("", entries, |_, _| true), vec!["../", "sub/", "a.dem"]);
    }

    #[test]
    fn only_what_is_on_disk_under_dod_is_listed() {
        let entries = [
            ("valve".to_string(), true),
            ("WindowsCrashDumps".to_string(), true),
            ("temp demos".to_string(), true),
            ("root.dem".to_string(), false),
            ("mine.dem".to_string(), false),
        ];
        let on_disk = |path: &str, _: bool| matches!(path, "temp demos" | "mine.dem");
        assert_eq!(
            rows("", entries, on_disk),
            vec!["../", "temp demos/", "mine.dem"]
        );
    }

    #[test]
    fn this_folder_and_the_one_above_are_never_asked_about() {
        let entries = [
            (".".to_string(), true),
            ("..".to_string(), true),
            ("sub".to_string(), true),
        ];
        let asked = std::cell::RefCell::new(Vec::new());
        let listed = rows("test/", entries, |path, _| {
            asked.borrow_mut().push(path.to_string());
            true
        });
        assert_eq!(listed, vec!["test/../", "test/sub/"]);
        assert_eq!(
            *asked.borrow(),
            vec!["test/sub"],
            "only the real subfolder is checked or counted"
        );
    }

    #[test]
    fn the_builds_are_told_apart() {
        assert_ne!(
            GAMEUI_BUILDS[0].time_date_stamp,
            GAMEUI_BUILDS[1].time_date_stamp
        );
        assert_ne!(
            FILESYSTEM_BUILDS[0].time_date_stamp,
            FILESYSTEM_BUILDS[1].time_date_stamp
        );
    }
}
