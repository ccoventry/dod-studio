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
//! ## Drives, Back / Forward / Up, Go to (#779)
//!
//! In the game's Load Demo window `dod/`'s list starts with a row per drive
//! (`D:/`); the Demos tab reaches drives through This PC and Go to instead,
//! so its `dod/` lists only what is there. A drive-rooted
//! folder is listed straight from the disk (the game's file system only
//! knows paths from `dod/`); its demo rows are full paths, quoted, which
//! `viewdemo` takes with the hook DLL (`demo_other_drives`, #776). The
//! folders visited are kept for the Demos tab's Back and Forward, the
//! folder shown is remembered between sessions
//! (`%APPDATA%\dod-studio\goldsrc_hooks_demos.txt`), and the tab's Go to
//! dropdown offers the game folder, Studio's analysed demos (every demo in
//! its analyzer cache as one list, the `studio:/` place), each drive,
//! Studio's demo folders (read out of its settings.json) and the folders
//! visited lately.
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

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

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

/// The folder the lists show: a path from `dod/` (`""` for `dod/`), or a
/// drive-rooted one (`D:/demos/`) for a folder on any drive (#779).
static CURRENT_FOLDER: Mutex<String> = Mutex::new(String::new());

/// The folder the Load Demo window and the Demos tab are in.
pub fn current_folder() -> String {
    CURRENT_FOLDER.lock().map(|f| f.clone()).unwrap_or_default()
}

fn set_current(folder: &str) {
    if let Ok(mut current) = CURRENT_FOLDER.lock() {
        current.clear();
        current.push_str(folder);
    }
}

/// The folders visited, oldest first, and which one shows: the Demos tab's
/// Back and Forward (#779).
struct Nav {
    visited: Vec<String>,
    at: usize,
}

impl Nav {
    const fn new() -> Self {
        Self {
            visited: Vec::new(),
            at: 0,
        }
    }

    fn current(&self) -> &str {
        self.visited.get(self.at).map_or("", String::as_str)
    }

    /// Moves to `folder`, forgetting anything Forward of here. False when
    /// already there.
    fn go(&mut self, folder: &str) -> bool {
        if !self.visited.is_empty() && self.current() == folder {
            return false;
        }
        self.visited.truncate(self.at + 1);
        self.visited.push(folder.to_string());
        if self.visited.len() > NAV_KEPT {
            self.visited.remove(0);
        }
        self.at = self.visited.len() - 1;
        true
    }

    fn back(&mut self) -> bool {
        if self.at == 0 {
            return false;
        }
        self.at -= 1;
        true
    }

    fn forward(&mut self) -> bool {
        if !self.can_go_forward() {
            return false;
        }
        self.at += 1;
        true
    }

    fn can_go_back(&self) -> bool {
        self.at > 0
    }

    fn can_go_forward(&self) -> bool {
        self.at + 1 < self.visited.len()
    }

    /// The folders visited lately other than the current one, newest first,
    /// each once.
    fn recent(&self, most: usize) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for folder in self.visited.iter().rev() {
            if folder != self.current() && !out.iter().any(|f| f.eq_ignore_ascii_case(folder)) {
                out.push(folder.clone());
                if out.len() == most {
                    break;
                }
            }
        }
        out
    }
}

/// How many visited folders Back can walk.
const NAV_KEPT: usize = 50;
/// How many lately visited folders the state file keeps, and the Go to list
/// shows.
const RECENT_KEPT: usize = 8;
const RECENT_SHOWN: usize = 5;
static NAV: Mutex<Nav> = Mutex::new(Nav::new());

/// The Demos tab's own Load Demo window (`studio_panel`'s hidden one): its
/// list has Back, Forward and Up, so no up row. The game's Load Demo window
/// keeps the up row, its only way up. `FILLING_TAB` is set around each fill
/// of the tab's window, so the listing knows whose it is.
static TAB_DIALOG: AtomicUsize = AtomicUsize::new(0);
static FILLING_TAB: AtomicBool = AtomicBool::new(false);

/// Called by `studio_panel` with its hidden Load Demo window.
pub fn set_tab_dialog(dialog: usize) {
    TAB_DIALOG.store(dialog, Ordering::Release);
}

/// Runs `fill`, a fill of the Demos tab's list, which gets no up row.
pub fn tab_fill<R>(fill: impl FnOnce() -> R) -> R {
    FILLING_TAB.store(true, Ordering::Release);
    let out = fill();
    FILLING_TAB.store(false, Ordering::Release);
    out
}

fn up_row_wanted() -> bool {
    !FILLING_TAB.load(Ordering::Acquire)
}

/// Shows `folder` (a listing path, see [`CURRENT_FOLDER`]): the next list
/// fill reads it. Recorded for Back, and kept for the next session. False
/// when the lists were there already.
pub fn go_to(folder: &str) -> bool {
    let folder = drive_root_of(&crate::texture_hires::game_dir(), folder)
        .unwrap_or_else(|| folder.to_string());
    let folder = folder.as_str();
    let moved = NAV.lock().map(|mut nav| nav.go(folder)).unwrap_or(false);
    set_current(folder);
    if moved {
        remember();
    }
    moved
}

/// The Demos tab's Back: true when the folder changed.
pub fn go_back() -> bool {
    step(Nav::back)
}

/// The Demos tab's Forward: true when the folder changed.
pub fn go_forward() -> bool {
    step(Nav::forward)
}

fn step(walk: fn(&mut Nav) -> bool) -> bool {
    let Ok(mut nav) = NAV.lock() else {
        return false;
    };
    if !walk(&mut nav) {
        return false;
    }
    let folder = nav.current().to_string();
    drop(nav);
    set_current(&folder);
    remember();
    true
}

/// The Demos tab's Up: the folder above. Above a drive's
/// root is This PC, every drive in one list, as in Explorer, and that is as
/// high as Up goes. (The game's own Load Demo window keeps its up row,
/// which goes from a drive's root to `dod/`, where its drive rows are.)
pub fn go_up() -> bool {
    let here = current_folder();
    let up = up_from(&here);
    up != here && go_to(&up)
}

/// Whether Back and Forward have somewhere to go: the tab greys each out
/// where it has not.
pub fn can_go_back() -> bool {
    NAV.lock().is_ok_and(|nav| nav.can_go_back())
}

pub fn can_go_forward() -> bool {
    NAV.lock().is_ok_and(|nav| nav.can_go_forward())
}

/// Whether Up has somewhere to go: the tab greys it out on This PC.
pub fn can_go_up() -> bool {
    let here = current_folder();
    up_from(&here) != here
}

/// The folder Up opens from `here`: This PC from a drive's root, and
/// `here` itself on This PC.
fn up_from(here: &str) -> String {
    if is_drives_place(here) {
        return here.to_string();
    }
    if is_drive_root(here) {
        return DRIVES_PLACE.to_string();
    }
    resolve(&format!("{here}../"))
}

/// `folder`, a path from `dod`, as its drive (`C:/`) when it climbs to the
/// drive's root or past it. Windows lists `C:/../` as `C:/`, so without this
/// Up and the up row at the root only grew the path by `../` each time, and
/// the line above the list read `.../../../`. As a drive path, Up stops
/// there as at any drive's root. `None` for any other folder.
fn drive_root_of(dod: &Path, folder: &str) -> Option<String> {
    if folder.is_empty() || is_drive_path(folder) || is_place(folder) {
        return None;
    }
    let full = crate::folder_counts::normalize(&full_path(dod, folder));
    let at_root = full.parent().is_none()
        || full
            .components()
            .any(|c| c == std::path::Component::ParentDir);
    if !full.has_root() || !at_root {
        return None;
    }
    let text = full.to_string_lossy();
    let drive = text.chars().next().filter(char::is_ascii_alphabetic)?;
    (text.chars().nth(1) == Some(':')).then(|| format!("{}:/", drive.to_ascii_uppercase()))
}

/// The folders visited lately, newest first, for the Go to list.
pub fn recent_folders(most: usize) -> Vec<String> {
    NAV.lock().map(|nav| nav.recent(most)).unwrap_or_default()
}

/// Where the folder shown and the ones visited lately are kept between
/// sessions.
fn state_path() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    let appdata = std::env::var_os("APPDATA")?;
    let dir = PathBuf::from(appdata).join("dod-studio");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("goldsrc_hooks_demos.txt"))
}

/// The state file's text: the folder shown, then the ones visited lately.
fn state_text(current: &str, recent: &[String]) -> String {
    let mut out = format!("folder={current}\n");
    for folder in recent {
        out.push_str(&format!("recent={folder}\n"));
    }
    out
}

fn parse_state(text: &str) -> (String, Vec<String>) {
    let mut current = String::new();
    let mut recent = Vec::new();
    for line in text.lines() {
        if let Some(folder) = line.strip_prefix("folder=") {
            current = folder.trim().to_string();
        } else if let Some(folder) = line.strip_prefix("recent=")
            && !folder.trim().is_empty()
        {
            recent.push(folder.trim().to_string());
        }
    }
    (current, recent)
}

fn remember() {
    let Some(path) = state_path() else {
        return;
    };
    let recent = recent_folders(RECENT_KEPT);
    let _ = std::fs::write(path, state_text(&current_folder(), &recent));
}

/// Puts the last session's folder back, when it still exists, with the
/// folders visited then as Back history. Once, when the hooks install.
fn restore(dod: &Path) {
    let Some(text) = state_path().and_then(|path| std::fs::read_to_string(path).ok()) else {
        return;
    };
    let (current, recent) = parse_state(&text);
    if let Ok(mut nav) = NAV.lock() {
        for folder in recent.iter().rev() {
            if full_path(dod, folder).is_dir() {
                nav.go(folder);
            }
        }
    }
    if is_place(&current) || full_path(dod, &current).is_dir() {
        go_to(&current);
    } else {
        go_to("");
    }
}

/// Whether a listing path is rooted at a drive (`D:/...`), rather than a
/// path from `dod/`.
pub fn is_drive_path(path: &str) -> bool {
    let bytes = path.trim().trim_matches('"').as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
}

/// Whether a row is a drive's root (`D:/`), one of `dod/`'s drive rows.
pub fn is_drive_root(row: &str) -> bool {
    let name = row.trim().trim_matches('"');
    is_drive_path(name) && name.len() == 3
}

/// The listing path of Studio's analysed demos: every demo in Studio's
/// analyzer cache as one list, wherever each is on disk (#779). Not a
/// folder, so it is listed by [`studio_rows`]; its up row goes to `dod/`
/// (`resolve` drops the one part).
pub const STUDIO_PLACE: &str = "studio:/";

pub fn is_studio_place(folder: &str) -> bool {
    folder.trim().trim_matches('"') == STUDIO_PLACE
}

/// The listing path of This PC: every drive as one list, what Up opens
/// from a drive's root. Its up row, in the game's own Load Demo window,
/// goes to `dod/` (`resolve` drops the one part).
pub const DRIVES_PLACE: &str = "drives:/";

pub fn is_drives_place(folder: &str) -> bool {
    folder.trim().trim_matches('"') == DRIVES_PLACE
}

/// Studio's analysed demos or This PC: listed by the hook, not a folder.
pub fn is_place(folder: &str) -> bool {
    is_studio_place(folder) || is_drives_place(folder)
}

/// This PC as rows: the up row when `up_row`, then a row per drive.
pub fn drives_rows(drives: &[char], up_row: bool) -> Vec<String> {
    let up = up_row.then(|| format!("{DRIVES_PLACE}../"));
    with_drives(up.into_iter().collect(), drives, up_row)
}

/// Studio's analysed demos as rows: the up row when `up_row`, then each
/// demo that is still on disk, as a quoted full path the way the drive
/// listings spell theirs, sorted by name.
pub fn studio_rows(
    paths: impl IntoIterator<Item = String>,
    on_disk: impl Fn(&str) -> bool,
    up_row: bool,
) -> Vec<String> {
    let mut demos: Vec<String> = paths
        .into_iter()
        .map(|path| path.trim().replace('\\', "/"))
        .filter(|path| is_drive_path(path) && on_disk(path))
        .map(|path| format!("\"{path}\""))
        .collect();
    demos.sort_by_key(|row| row.to_ascii_lowercase());
    demos.dedup();
    let mut out = if up_row {
        vec![format!("{STUDIO_PLACE}../")]
    } else {
        Vec::new()
    };
    out.extend(demos);
    out
}

/// The file or folder a row or listing path names on disk: a drive-rooted
/// path as it is, a path from `dod/` under `dod`. Quotes around a row are
/// dropped.
pub fn full_path(dod: &Path, row: &str) -> PathBuf {
    let name = row.trim().trim_matches('"');
    if is_drive_path(name) {
        PathBuf::from(name)
    } else {
        dod.join(name)
    }
}

/// `D:/`, `E:/`...: a row per drive in `dod/`'s list, first, or after the
/// up row when the list has one (`after_up`).
fn with_drives(mut rows: Vec<String>, drives: &[char], after_up: bool) -> Vec<String> {
    let at = usize::from(after_up && !rows.is_empty());
    for (i, drive) in drives.iter().enumerate() {
        rows.insert(at + i, format!("{}:/", drive.to_ascii_uppercase()));
    }
    rows
}

/// One entry of the Demos tab's Go to list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub label: String,
    pub folder: String,
}

/// The Go to list: the game folder, each drive, Studio's demo folders, then
/// the folders visited lately in here. Labels are unique, since a pick comes
/// back as the row's text. Folders are listing paths from `dod`; labels name
/// them as they are on disk, so `../../` reads as the folder it is.
pub fn build_places(
    dod: &Path,
    game: &str,
    drives: &[char],
    studio: &[String],
    recent: &[String],
) -> Vec<Place> {
    let mut places = vec![
        Place {
            label: format!("Game: {game}/"),
            folder: String::new(),
        },
        Place {
            label: "Studio's analysed demos".to_string(),
            folder: STUDIO_PLACE.to_string(),
        },
        Place {
            label: "This PC (every drive)".to_string(),
            folder: DRIVES_PLACE.to_string(),
        },
    ];
    for drive in drives {
        let folder = format!("{}:/", drive.to_ascii_uppercase());
        places.push(Place {
            label: format!("Drive: {folder}"),
            folder,
        });
    }
    for folder in studio {
        push_place(&mut places, dod, "Studio", folder);
    }
    for folder in recent {
        push_place(&mut places, dod, "Recent", folder);
    }
    places
}

/// `folder` (a listing path from `dod`) as a drive-rooted path with forward
/// slashes, ending in `/`: `../../` from `C:/Steam/common/HL/dod` is
/// `C:/Steam/common/`.
fn on_disk_folder(dod: &Path, folder: &str) -> String {
    let full = crate::folder_counts::normalize(&full_path(dod, folder));
    listing_path(&full.to_string_lossy())
}

fn push_place(places: &mut Vec<Place>, dod: &Path, kind: &str, folder: &str) {
    let here = on_disk_folder(dod, folder);
    if places
        .iter()
        .any(|p| on_disk_folder(dod, &p.folder).eq_ignore_ascii_case(&here))
    {
        return;
    }
    let base = format!("{kind}: {}", short_folder(&here));
    let mut label = base.clone();
    let mut n = 2;
    while places.iter().any(|p| p.label == label) {
        label = format!("{base} ({n})");
        n += 1;
    }
    places.push(Place {
        label,
        folder: folder.to_string(),
    });
}

/// A path's drive and its last two folders, `C:/.../Movies/dod/`, for a
/// narrow list; a path with nothing between them (`D:/demos/`,
/// `C:/Steam/common/`) in full. A path with no drive keeps only the last two
/// folders.
fn short_folder(folder: &str) -> String {
    let parts: Vec<&str> = folder.split('/').filter(|p| !p.is_empty()).collect();
    let drive = parts.first().filter(|p| is_drive_path(&format!("{p}/")));
    match (drive, parts.len()) {
        (_, 0) => folder.to_string(),
        (Some(_), n) if n <= 3 => format!("{}/", parts.join("/")),
        (Some(drive), n) => format!("{drive}/.../{}/{}/", parts[n - 2], parts[n - 1]),
        (None, n) if n <= 2 => format!("{}/", parts.join("/")),
        (None, n) => format!(".../{}/{}/", parts[n - 2], parts[n - 1]),
    }
}

/// Studio's demo folders, from its settings file: the Capture page's demo
/// folder list (`pinned_folders`, what its Add Folder button grows) and the
/// folders it scanned lately (`demo_folder_history`), as listing paths,
/// each once, those still on disk.
pub fn studio_folders() -> Vec<String> {
    let Some(appdata) = std::env::var_os("APPDATA") else {
        return Vec::new();
    };
    let path = PathBuf::from(appdata)
        .join("dod-studio")
        .join("settings.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    studio_folders_from(&text)
        .into_iter()
        .filter(|folder| Path::new(folder).is_dir())
        .collect()
}

fn studio_folders_from(json: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for key in ["pinned_folders", "demo_folder_history"] {
        let items = value
            .get(key)
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten();
        for folder in items.filter_map(|item| item.as_str()).map(listing_path) {
            if is_drive_path(&folder) && !out.iter().any(|f| f.eq_ignore_ascii_case(&folder)) {
                out.push(folder);
            }
        }
    }
    out
}

/// A Windows path as the lists spell a drive-rooted folder: forward
/// slashes, ending in `/`.
fn listing_path(path: &str) -> String {
    let mut folder = path.trim().replace('\\', "/");
    if !folder.is_empty() && !folder.ends_with('/') {
        folder.push('/');
    }
    folder
}

/// The Go to list as it stands now.
pub fn places() -> Vec<Place> {
    build_places(
        &crate::texture_hires::game_dir(),
        crate::game_folder::name(),
        &drives(),
        &studio_folders(),
        &recent_folders(RECENT_SHOWN),
    )
}

/// The drives a list can go to (fixed and removable), by letter.
pub fn drives() -> Vec<char> {
    #[cfg(target_arch = "x86")]
    {
        hook::drives()
    }
    #[cfg(not(target_arch = "x86"))]
    {
        Vec::new()
    }
}

/// For the fallback toggle command's bare-name query.
pub fn status() -> String {
    if enabled() {
        "the Load Demo window lists and opens folders".to_string()
    } else {
        format!(
            "the Load Demo window lists only the demos in {}/",
            crate::game_folder::name()
        )
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
/// Demo Analyzer's folder view shows the disk.
fn rows(
    folder: &str,
    entries: impl IntoIterator<Item = (String, bool)>,
    on_disk: impl Fn(&str, bool) -> bool,
    up_row: bool,
) -> Vec<String> {
    // The up row only where there is no Up button: the game's own window.
    let mut out = if up_row {
        vec![format!("{folder}../")]
    } else {
        Vec::new()
    };
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
            // The demo bar runs `viewdemo <row>` unquoted, and the console
            // splits an unquoted `D:/...` at the `:`.
            demos.push(if path.contains(' ') || path.contains(':') {
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

    /// The listing being handed out.
    struct Browser {
        rows: Vec<CString>,
        next: usize,
    }

    static BROWSER: Mutex<Browser> = Mutex::new(Browser {
        rows: Vec::new(),
        next: 0,
    });

    /// The drives a list can go to: fixed and removable ones, by letter.
    pub(super) fn drives() -> Vec<char> {
        use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeA, GetLogicalDrives};
        // `GetDriveType`'s answers (WinBase.h); windows-sys 0.59 has no
        // names for them.
        const DRIVE_REMOVABLE: u32 = 2;
        const DRIVE_FIXED: u32 = 3;
        let mask = unsafe { GetLogicalDrives() };
        (0..26u32)
            .filter(|i| mask & (1 << i) != 0)
            .map(|i| (b'A' + i as u8) as char)
            .filter(|letter| {
                let root = [*letter as u8, b':', b'\\', 0];
                let kind = unsafe { GetDriveTypeA(root.as_ptr()) };
                kind == DRIVE_FIXED || kind == DRIVE_REMOVABLE
            })
            .collect()
    }

    /// `folder` (a drive-rooted path) read straight from the disk, without
    /// hidden and system entries (`$Recycle.Bin` and the like at a root).
    fn disk_entries(folder: &str) -> Vec<(String, bool)> {
        use std::os::windows::fs::MetadataExt;
        const HIDDEN_OR_SYSTEM: u32 = 0x2 | 0x4;
        let Ok(dir) = std::fs::read_dir(folder) else {
            return Vec::new();
        };
        dir.filter_map(Result::ok)
            .filter_map(|entry| {
                let meta = entry.metadata().ok()?;
                if meta.file_attributes() & HIDDEN_OR_SYSTEM != 0 {
                    return None;
                }
                Some((
                    entry.file_name().to_string_lossy().into_owned(),
                    meta.is_dir(),
                ))
            })
            .collect()
    }

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

    /// `folder` (a path from `dod/`) through the real file system, which
    /// merges every search path.
    unsafe fn fs_entries(
        fs: *mut c_void,
        folder: &str,
        path_id: *const c_char,
    ) -> Vec<(String, bool)> {
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
        entries
    }

    /// Lists `folder` as rows: through the real file system for a path from
    /// `dod/`, straight from the disk for a drive-rooted one.
    unsafe fn list(
        fs: *mut c_void,
        folder: &str,
        path_id: *const c_char,
        up_row: bool,
    ) -> Vec<CString> {
        let dod = crate::texture_hires::game_dir();
        if super::is_studio_place(folder) {
            // Studio's analysed demos: the analyzer cache's entries, those
            // still on disk. The up row, where there is one, counts `dod/`,
            // where it goes.
            let paths = crate::streaks::cache_root()
                .map(|root| analysis::cache::cached_demo_paths(&root))
                .unwrap_or_default();
            let listed =
                super::studio_rows(paths, |path| std::path::Path::new(path).is_file(), up_row);
            crate::folder_counts::count_later(
                folder,
                Vec::new(),
                up_row.then(|| (dod.clone(), dod.join(super::STUDIO_PLACE))),
            );
            return listed
                .into_iter()
                .filter_map(|row| CString::new(row).ok())
                .collect();
        }
        if super::is_drives_place(folder) {
            // This PC: a row per drive, not counted (a whole drive is too
            // much to walk); the up row counts `dod/`, where it goes.
            crate::folder_counts::count_later(
                folder,
                Vec::new(),
                up_row.then(|| (dod.clone(), dod.join(super::DRIVES_PLACE))),
            );
            return super::drives_rows(&drives(), up_row)
                .into_iter()
                .filter_map(|row| CString::new(row).ok())
                .collect();
        }
        let entries = if super::is_drive_path(folder) {
            disk_entries(folder)
        } else {
            unsafe { fs_entries(fs, folder, path_id) }
        };
        let hide_empty = super::HIDE_EMPTY.on();
        // No counting here, on the game thread: a folder known to be empty
        // is left out, and the rest are counted in the background
        // (`folder_counts`), the Demos tab listing again when that is done.
        let to_count = std::cell::RefCell::new(Vec::new());
        let listed = rows(
            folder,
            entries,
            |path, is_dir| {
                let full = super::full_path(&dod, path);
                if !is_dir {
                    return full.is_file();
                }
                if !full.is_dir() || (hide_empty && crate::folder_counts::known_empty(&full)) {
                    return false;
                }
                to_count.borrow_mut().push(full);
                true
            },
            up_row,
        );
        // Drive rows are not counted: a whole drive is too much to walk for
        // a number in brackets. Only the game's Load Demo window (the one
        // with an up row) gets them; the Demos tab has This PC and Go to.
        let listed = if folder.is_empty() && up_row {
            super::with_drives(listed, &drives(), up_row)
        } else {
            listed
        };
        // The up row's count too, where there is one: this folder's total
        // from its parts, plus the rest of the parent, so nothing is counted
        // twice. A drive root's up row goes back to `dod/`.
        let here = super::full_path(&dod, folder);
        let up = if !up_row {
            None
        } else if super::is_drive_root(folder) {
            Some((dod.clone(), here))
        } else {
            Some((here.join(".."), here))
        };
        crate::folder_counts::count_later(folder, to_count.into_inner(), up);
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
                let folder = super::current_folder();
                browser.rows = list(fs, &folder, path_id, super::up_row_wanted());
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
                super::go_to(&folder);
                let opened = if super::is_drive_path(&folder) {
                    folder.clone()
                } else {
                    format!("{}/{folder}", crate::game_folder::name())
                };
                crate::debug::report(&format!("demo_list_folders: opened {opened}"));
                let fill: FillFn = cast(FILL.load(Ordering::Relaxed));
                if dialog as usize == super::TAB_DIALOG.load(Ordering::Acquire) {
                    super::tab_fill(|| fill(dialog));
                } else {
                    fill(dialog);
                }
                return;
            }
            let original: OnCommandFn = cast(ON_COMMAND.load(Ordering::Relaxed));
            original(dialog, command)
        }
    }

    /// Repoints one vftable slot, saving what it held into `original` first
    /// (`patch::swap_vftable_slot` says why that order).
    unsafe fn swap(vftable_slot: usize, ours: usize, original: &AtomicUsize) -> Result<(), String> {
        unsafe { crate::patch::swap_vftable_slot(vftable_slot, ours, original) }
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
            Ok(build) => {
                super::restore(&crate::texture_hires::game_dir());
                format!(
                    "demo_list_folders: installed ({build}); {NAME} 1 lists folders in the Load Demo window (#408)"
                )
            }
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
    fn a_drive_rooted_listing_path_is_told_from_one_under_dod() {
        assert!(is_drive_path("D:/"));
        assert!(is_drive_path("d:\\demos\\"));
        assert!(is_drive_path("\"C:/Program Files (x86)/x/\""));
        assert!(!is_drive_path(""));
        assert!(!is_drive_path("demos/"));
        assert!(!is_drive_path("../"));
        assert!(!is_drive_path("D:"));
        assert!(is_drive_root("D:/"));
        assert!(is_drive_root("\"d:/\""));
        assert!(!is_drive_root("D:/demos/"));
        let dod = Path::new("C:/games/dod");
        assert_eq!(full_path(dod, "demos/x.dem"), dod.join("demos/x.dem"));
        assert_eq!(
            full_path(dod, "\"temp demos/x.dem\""),
            dod.join("temp demos/x.dem")
        );
        assert_eq!(full_path(dod, "D:/x/y.dem"), PathBuf::from("D:/x/y.dem"));
        assert_eq!(full_path(dod, ""), dod);
    }

    #[test]
    fn drive_paths_resolve_and_climb_back_to_dod() {
        assert_eq!(resolve("D:/"), "D:/");
        assert_eq!(resolve("D:/a/b/../"), "D:/a/");
        assert_eq!(resolve("D:/../"), "");
    }

    #[test]
    fn studios_analysed_demos_are_one_list_whose_up_row_goes_to_dod() {
        assert!(is_studio_place("studio:/"));
        assert!(is_studio_place("\"studio:/\""));
        assert!(!is_studio_place("studio:/../"));
        assert!(!is_drive_path(STUDIO_PLACE));
        assert_eq!(resolve("studio:/../"), "");
        let paths = [
            "D:\\demos\\b.dem".to_string(),
            "D:/demos/A.dem".to_string(),
            "D:/gone/x.dem".to_string(),
            "D:/demos/A.dem".to_string(),
            "relative/x.dem".to_string(),
        ];
        assert_eq!(
            studio_rows(paths.clone(), |path| !path.contains("gone"), true),
            vec!["studio:/../", "\"D:/demos/A.dem\"", "\"D:/demos/b.dem\""]
        );
        assert_eq!(
            studio_rows(paths, |_| true, false),
            vec![
                "\"D:/demos/A.dem\"",
                "\"D:/demos/b.dem\"",
                "\"D:/gone/x.dem\""
            ]
        );
    }

    #[test]
    fn drive_rows_follow_the_up_row_and_drive_demos_are_quoted() {
        let listed = with_drives(
            vec!["../".to_string(), "demos/".to_string()],
            &['c', 'D'],
            true,
        );
        assert_eq!(listed, vec!["../", "C:/", "D:/", "demos/"]);
        assert_eq!(with_drives(Vec::new(), &['D'], true), vec!["D:/"]);
        // The Demos tab's list has no up row: the drives come first.
        assert_eq!(
            with_drives(vec!["demos/".to_string()], &['D'], false),
            vec!["D:/", "demos/"]
        );
        let entries = [("x.dem".to_string(), false), ("sub".to_string(), true)];
        assert_eq!(
            rows("D:/demos/", entries.clone(), |_, _| true, true),
            vec!["D:/demos/../", "D:/demos/sub/", "\"D:/demos/x.dem\""]
        );
        assert_eq!(
            rows("D:/demos/", entries, |_, _| true, false),
            vec!["D:/demos/sub/", "\"D:/demos/x.dem\""]
        );
    }

    #[test]
    fn back_and_forward_walk_the_folders_visited() {
        let mut nav = Nav::new();
        assert!(nav.go(""));
        assert!(!nav.go(""), "the same folder again is not a step");
        assert!(nav.go("demos/"));
        assert!(!nav.can_go_forward());
        assert!(nav.go("D:/"));
        assert!(nav.can_go_back());
        assert!(nav.back());
        assert!(nav.can_go_forward());
        assert_eq!(nav.current(), "demos/");
        assert!(nav.back());
        assert_eq!(nav.current(), "");
        assert!(!nav.back());
        assert!(nav.forward());
        assert_eq!(nav.current(), "demos/");
        // A new folder from here forgets what was Forward.
        assert!(nav.go("E:/"));
        assert!(!nav.forward());
        assert_eq!(nav.recent(5), vec!["demos/", ""]);
        assert_eq!(nav.recent(1), vec!["demos/"]);
    }

    #[test]
    fn this_pc_lists_every_drive() {
        assert_eq!(drives_rows(&['C', 'd'], false), vec!["C:/", "D:/"]);
        assert_eq!(
            drives_rows(&['C'], true),
            vec!["drives:/../".to_string(), "C:/".to_string()]
        );
        // Its up row goes to dod/.
        assert_eq!(resolve("drives:/../"), "");
        assert!(is_place(DRIVES_PLACE) && is_place(STUDIO_PLACE));
        assert!(!is_drive_path(DRIVES_PLACE) && !is_place("D:/"));
    }

    #[test]
    fn climbing_to_a_drive_root_lands_on_the_drive() {
        let dod = Path::new("C:/Steam/common/HL/dod");
        assert_eq!(drive_root_of(dod, "../../../../"), Some("C:/".to_string()));
        // Past the root, as a saved state from before this could hold.
        assert_eq!(
            drive_root_of(dod, "../../../../../../../"),
            Some("C:/".to_string())
        );
        assert_eq!(drive_root_of(dod, "../../../"), None);
        assert_eq!(drive_root_of(dod, "temp demos/"), None);
        assert_eq!(drive_root_of(dod, ""), None);
        assert_eq!(drive_root_of(dod, "D:/"), None);
        assert_eq!(drive_root_of(dod, STUDIO_PLACE), None);
        assert_eq!(drive_root_of(dod, DRIVES_PLACE), None);
        // Up from a drive's root is This PC, and stops there.
        assert_eq!(up_from("C:/"), DRIVES_PLACE);
        assert_eq!(up_from(DRIVES_PLACE), DRIVES_PLACE);
        assert_eq!(up_from("D:/demos/"), "D:/");
        assert_eq!(up_from("temp demos/"), "");
        assert_eq!(up_from(STUDIO_PLACE), "");
    }

    #[test]
    fn the_state_file_round_trips() {
        let text = state_text("D:/demos/", &["".to_string(), "temp demos/".to_string()]);
        assert_eq!(text, "folder=D:/demos/\nrecent=\nrecent=temp demos/\n");
        assert_eq!(
            parse_state(&text),
            ("D:/demos/".to_string(), vec!["temp demos/".to_string()])
        );
        assert_eq!(parse_state(""), (String::new(), Vec::new()));
    }

    #[test]
    fn the_go_to_list_names_each_place_once() {
        let studio = [
            "C:/Games/Half-Life/dod/".to_string(),
            "D:/demos/".to_string(),
        ];
        let recent = [
            "D:/demos/".to_string(),
            "temp demos/".to_string(),
            "../../".to_string(),
            "../".to_string(),
            "../../Half-Life/dod/".to_string(),
        ];
        let dod = Path::new("C:/Steam/common/HL/dod");
        let places = build_places(dod, "dod", &['C', 'D'], &studio, &recent);
        let labels: Vec<&str> = places.iter().map(|p| p.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "Game: dod/",
                "Studio's analysed demos",
                "This PC (every drive)",
                "Drive: C:/",
                "Drive: D:/",
                "Studio: C:/.../Half-Life/dod/",
                "Studio: D:/demos/",
                "Recent: C:/.../dod/temp demos/",
                "Recent: C:/Steam/common/",
                "Recent: C:/.../common/HL/",
                "Recent: C:/.../Half-Life/dod/",
            ]
        );
        assert_eq!(places[1].folder, STUDIO_PLACE);
        assert_eq!(places[2].folder, DRIVES_PLACE);
        assert_eq!(places[5].folder, "C:/Games/Half-Life/dod/");
        // Two different folders with the same drive and last two names stay
        // apart; on different drives the drive tells them apart.
        let twins = ["C:/a/x/dod/".to_string(), "C:/b/x/dod/".to_string()];
        let places = build_places(dod, "dod", &[], &twins, &[]);
        assert_eq!(places[3].label, "Studio: C:/.../x/dod/");
        assert_eq!(places[4].label, "Studio: C:/.../x/dod/ (2)");
        let apart = ["C:/a/x/dod/".to_string(), "D:/b/x/dod/".to_string()];
        let places = build_places(dod, "dod", &[], &apart, &[]);
        assert_eq!(places[4].label, "Studio: D:/.../x/dod/");
    }

    #[test]
    fn studios_folders_come_out_of_its_settings_file() {
        let json = r#"{"pinned_folders": ["C:\\Games\\Half-Life\\dod", "D:\\demos\\"],
            "demo_folder_history": ["D:\\demos", "\\\\server\\share", "C:\\x"], "other": 1}"#;
        assert_eq!(
            studio_folders_from(json),
            vec!["C:/Games/Half-Life/dod/", "D:/demos/", "C:/x/"]
        );
        assert!(studio_folders_from("not json").is_empty());
        assert!(studio_folders_from("{}").is_empty());
    }

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
            rows("test/", entries, |_, _| true, true),
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
        assert_eq!(
            rows("", entries, |_, _| true, true),
            vec!["../", "sub/", "a.dem"]
        );
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
            rows("", entries, on_disk, true),
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
        let listed = rows(
            "test/",
            entries,
            |path, _| {
                asked.borrow_mut().push(path.to_string());
                true
            },
            true,
        );
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
