// pick_paths.rs — what `studio/src/pick_path.js` needs to choose where a
// Browse dialog starts (#936): which of a few candidate paths still exist,
// and the user's own folders for the per-field defaults. The choice itself
// is made in JS, where its tests live.

use serde::Serialize;
use std::path::Path;

/// One candidate start path, as it stands on disk now.
#[derive(Debug, Serialize, PartialEq)]
pub struct PickPathState {
    /// The path itself when it exists, else its nearest folder that does.
    pub path: String,
    /// `path` is a file (only ever when `exact`).
    pub is_file: bool,
    /// `path` is the candidate itself, not a folder above it.
    pub exact: bool,
    /// `path` is a drive root: a poor start for a path that moved.
    pub is_root: bool,
}

fn state_of(candidate: &str) -> Option<PickPathState> {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = Path::new(trimmed);
    // Relative paths would resolve against Studio's own working folder.
    if !path.is_absolute() {
        return None;
    }
    for (i, p) in path.ancestors().enumerate() {
        let is_file = i == 0 && p.is_file();
        if is_file || p.is_dir() {
            return Some(PickPathState {
                path: p.to_string_lossy().into_owned(),
                is_file,
                exact: i == 0,
                is_root: p.parent().is_none(),
            });
        }
    }
    None
}

/// Each candidate's state, in order: `None` for a blank or relative path, or
/// one on a drive that isn't there.
#[tauri::command(async)]
pub fn pick_path_states(paths: Vec<String>) -> Vec<Option<PickPathState>> {
    paths.iter().map(|p| state_of(p)).collect()
}

#[derive(Debug, Serialize)]
pub struct UserFolders {
    pub videos: Option<String>,
    pub documents: Option<String>,
    pub program_files: Option<String>,
}

/// The user's Videos and Documents folders and Program Files, for the
/// pickers that have nothing better to start in.
#[tauri::command(async)]
pub fn pick_user_folders() -> UserFolders {
    let s = |p: std::path::PathBuf| p.to_string_lossy().into_owned();
    UserFolders {
        videos: dirs::video_dir().map(s),
        documents: dirs::document_dir().or_else(dirs::home_dir).map(s),
        program_files: std::env::var("ProgramFiles").ok().filter(|v| !v.is_empty()),
    }
}

/// Whether one of Studio's own dialog windows is on screen. The first file
/// dialog of a session can take seconds to appear after Studio's window is
/// already disabled, where the page's busy cursor doesn't show, so
/// `dialog_wait.js` asks this to show a notice only while it hasn't appeared.
#[tauri::command(async)]
pub fn file_dialog_shown() -> bool {
    #[cfg(windows)]
    {
        own_dialog_visible()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
fn own_dialog_visible() -> bool {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowThreadProcessId, IsWindowVisible,
    };

    struct Search {
        pid: u32,
        found: bool,
    }

    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> i32 {
        // SAFETY: `lparam` is the `Search` passed below, alive for the whole
        // EnumWindows call; the Win32 calls only read this window's details.
        unsafe {
            let search = &mut *(lparam as *mut Search);
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid != search.pid || IsWindowVisible(hwnd) == 0 {
                return 1;
            }
            let mut class = [0u16; 16];
            let len = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
            // "#32770": the class of Windows' dialog boxes, the file dialogs too.
            if len > 0 && String::from_utf16_lossy(&class[..len as usize]) == "#32770" {
                search.found = true;
                return 0;
            }
            1
        }
    }

    let mut search = Search {
        // SAFETY: no preconditions.
        pid: unsafe { GetCurrentProcessId() },
        found: false,
    };
    // SAFETY: `visit` matches WNDENUMPROC and `search` outlives the call.
    unsafe { EnumWindows(Some(visit), &mut search as *mut Search as LPARAM) };
    search.found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("pick_paths_{}_{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_file_and_a_folder_are_themselves() {
        let dir = scratch("exact");
        let file = dir.join("hl.exe");
        std::fs::write(&file, b"x").unwrap();
        let f = state_of(&file.to_string_lossy()).unwrap();
        assert!(f.exact && f.is_file && !f.is_root);
        let d = state_of(&dir.to_string_lossy()).unwrap();
        assert!(d.exact && !d.is_file);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_path_gives_its_nearest_folder() {
        let dir = scratch("missing");
        let gone = dir.join("moved").join("demo.dem");
        let s = state_of(&gone.to_string_lossy()).unwrap();
        assert!(!s.exact && !s.is_file);
        assert_eq!(Path::new(&s.path), dir.as_path());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn no_dialog_is_shown_in_a_process_without_one() {
        assert!(!file_dialog_shown());
    }

    #[test]
    fn blank_and_relative_paths_have_no_state() {
        assert_eq!(state_of(""), None);
        assert_eq!(state_of("   "), None);
        assert_eq!(state_of("dod/demo.dem"), None);
    }
}
