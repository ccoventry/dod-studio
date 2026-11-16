//! Minimizes another process's window as soon as it appears, without taking
//! focus.
//!
//! Blender's AGR import needs Blender's window (the importer wants a UI
//! context), and Blender opens it maximized and in front. A minimized start
//! (`STARTUPINFO.wShowWindow`, PowerShell's `-WindowStyle Minimized`) is
//! ignored: Blender 4.4 still came up maximized and focused. Minimizing the
//! window once it exists works, and the import runs the same minimized
//! (byte-identical `imported.blend`). Measured 2026-10-08.

use std::time::{Duration, Instant};

/// Waits up to `timeout` for a visible top-level window of `pid`, then
/// minimizes it without activating it. Returns whether one was found.
/// Gives up early when `alive` says the process has gone.
#[cfg(windows)]
pub fn minimize_when_shown(pid: u32, timeout: Duration, alive: impl Fn() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout && alive() {
        if let Some(window) = crate::sys::window_list::visible_window(pid) {
            win::minimize(window);
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[cfg(not(windows))]
pub fn minimize_when_shown(_pid: u32, _timeout: Duration, _alive: impl Fn() -> bool) -> bool {
    false
}

#[cfg(windows)]
mod win {
    use crate::sys::window_list::{Bool, Hwnd};

    /// Minimized, and the window that was active stays active.
    const SW_SHOWMINNOACTIVE: i32 = 7;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn ShowWindow(window: Hwnd, cmd: i32) -> Bool;
    }

    pub fn minimize(window: Hwnd) {
        // SAFETY: ShowWindow on a window that has since closed just fails.
        unsafe { ShowWindow(window, SW_SHOWMINNOACTIVE) };
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn a_process_with_no_window_is_not_waited_on_once_it_has_gone() {
        // Our own test process has no visible window, and saying it has gone
        // ends the wait at once instead of after the timeout.
        let started = Instant::now();
        assert!(!minimize_when_shown(
            std::process::id(),
            Duration::from_secs(5),
            || false
        ));
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
