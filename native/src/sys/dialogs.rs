//! Error boxes shown by the processes DoD Studio started (`hl.exe`, the HLAE
//! launcher and its `injector.exe`), read so Studio can say what went wrong.
//!
//! Every one of them stops the game where it is until someone clicks it, and
//! none reaches Studio any other way. Seen on 2026-09-29: HLAE's "AfxHook
//! error, Code: 1", the game's "Failed to initalize authentication
//! interface" (Steam not running) and, on Anniversary, a Visual C++
//! "Assertion failed! ... !m_bMounted" box after it.
//!
//! Reading is all `error_dialogs` does: it looks at windows belonging to the
//! given process ids and reads their text; what to do about a box is the
//! caller's. The one exception is `dismiss`, for a box whose process can't
//! be ended any other way (see there).

/// One error box, and the text on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorDialog {
    /// Identifies the box, so a watcher reports each one once.
    pub window: isize,
    pub pid: u32,
    pub title: String,
    /// Every non-button line of text on it, in window order.
    pub lines: Vec<String>,
}

impl ErrorDialog {
    /// The whole box on one line, for the log.
    pub fn full_text(&self) -> String {
        format!("\"{}\": {}", self.title, self.lines.join(" | "))
    }

    /// What happened, in plain words, for the user. The box's own text is in
    /// the log (`full_text`).
    pub fn summary(&self) -> String {
        let text = self.lines.join(" ");
        let lower = text.to_lowercase();
        if lower.contains("authentication interface") {
            "The game closed because Steam isn't running or isn't signed in (\"Failed to initalize authentication interface\"). Start Steam, sign in, and try again.".to_string()
        } else if lower.contains("only one instance") {
            "Day of Defeat is already running, and it only allows one copy at a time. Close it and try again.".to_string()
        } else if lower.contains("afxhook error") || self.title.eq_ignore_ascii_case("HLAE Error") {
            let code = self
                .lines
                .iter()
                .position(|l| l.trim_end_matches(':').eq_ignore_ascii_case("code"))
                .and_then(|i| self.lines.get(i + 1))
                .map(|c| format!(", code {}", c.trim()))
                .unwrap_or_default();
            format!(
                "HLAE couldn't attach to the game (AfxHook error{code}). Usually the game closed while HLAE was starting it: Steam not signed in, the game crashing at startup, or antivirus blocking HLAE."
            )
        } else if lower.contains("assertion failed") {
            let expression = self
                .lines
                .iter()
                .find_map(|l| {
                    // Only the expression's own line: the box goes on with
                    // "For information on how your program can cause…".
                    l.split_once("Expression:")
                        .map(|(_, e)| e.lines().next().unwrap_or("").trim().to_string())
                })
                .unwrap_or_default();
            if expression.is_empty() {
                "The game stopped on one of its own internal checks (an \"Assertion failed\" box)."
                    .to_string()
            } else {
                format!(
                    "The game stopped on one of its own internal checks (\"Assertion failed\": {expression})."
                )
            }
        } else {
            let mut line = text;
            if line.chars().count() > 300 {
                line = line.chars().take(300).collect::<String>() + "…";
            }
            format!("The game showed an error box, \"{}\": {line}", self.title)
        }
    }
}

/// Visible error boxes belonging to `pids` right now. For the launcher and
/// the injector, any visible window counts (HLAE runs them with no window of
/// their own); for the game, only standard dialog boxes do, never the game
/// window itself.
#[cfg(windows)]
pub fn error_dialogs(pids: &[u32], launcher_pids: &[u32]) -> Vec<ErrorDialog> {
    win::error_dialogs(pids, launcher_pids)
}

#[cfg(not(windows))]
pub fn error_dialogs(_pids: &[u32], _launcher_pids: &[u32]) -> Vec<ErrorDialog> {
    Vec::new()
}

/// Answers a box with Ignore (and closes it, for an OK-only one).
///
/// Only for a box whose process is already exiting: Windows refuses to
/// terminate a process that is terminating, so killing the game does nothing
/// while Anniversary's `!m_bMounted` assert (raised during its own exit) is
/// up, and the box would wait for a click. Ignore lets that exit finish.
/// Seen live 2026-09-29.
#[cfg(windows)]
pub fn dismiss(dialog: &ErrorDialog) {
    win::dismiss(dialog.window)
}

#[cfg(not(windows))]
pub fn dismiss(_dialog: &ErrorDialog) {}

#[cfg(windows)]
mod win {
    use super::ErrorDialog;

    type Hwnd = isize;
    type Lparam = isize;
    type Bool = i32;
    type EnumProc = unsafe extern "system" fn(Hwnd, Lparam) -> Bool;

    const WM_GETTEXT: u32 = 0x000D;
    const SMTO_ABORTIFHUNG: u32 = 0x0002;
    /// The class every standard dialog, `MessageBox` and CRT assert box has.
    const DIALOG_CLASS: &str = "#32770";

    #[link(name = "user32")]
    unsafe extern "system" {
        fn EnumWindows(callback: EnumProc, param: Lparam) -> Bool;
        fn EnumChildWindows(parent: Hwnd, callback: EnumProc, param: Lparam) -> Bool;
        fn GetWindowThreadProcessId(window: Hwnd, pid: *mut u32) -> u32;
        fn IsWindowVisible(window: Hwnd) -> Bool;
        fn GetClassNameW(window: Hwnd, name: *mut u16, max: i32) -> i32;
        fn PostMessageW(window: Hwnd, msg: u32, wparam: usize, lparam: isize) -> Bool;
        fn SendMessageTimeoutW(
            window: Hwnd,
            msg: u32,
            wparam: usize,
            lparam: isize,
            flags: u32,
            timeout_ms: u32,
            result: *mut usize,
        ) -> isize;
    }

    unsafe extern "system" fn collect(window: Hwnd, param: Lparam) -> Bool {
        // SAFETY: `param` is the `&mut Vec<Hwnd>` passed by `windows_of`,
        // alive for the whole enumeration.
        let found = unsafe { &mut *(param as *mut Vec<Hwnd>) };
        found.push(window);
        1
    }

    fn top_level_windows() -> Vec<Hwnd> {
        let mut found: Vec<Hwnd> = Vec::new();
        // SAFETY: `collect` only pushes into `found`, which outlives the call.
        unsafe { EnumWindows(collect, &mut found as *mut Vec<Hwnd> as Lparam) };
        found
    }

    fn children(window: Hwnd) -> Vec<Hwnd> {
        let mut found: Vec<Hwnd> = Vec::new();
        // SAFETY: as above.
        unsafe { EnumChildWindows(window, collect, &mut found as *mut Vec<Hwnd> as Lparam) };
        found
    }

    fn class_name(window: Hwnd) -> String {
        let mut buf = vec![0u16; 256];
        // SAFETY: `buf` holds 256 u16s, the length passed.
        let len = unsafe { GetClassNameW(window, buf.as_mut_ptr(), buf.len() as i32) };
        String::from_utf16_lossy(&buf[..len.max(0) as usize])
    }

    /// WM_GETTEXT with a timeout, which works for controls in another process
    /// (GetWindowText does not) and gives up on a hung one.
    fn text(window: Hwnd) -> String {
        let mut buf = vec![0u16; 2048];
        let mut copied: usize = 0;
        // SAFETY: `buf` holds 2048 u16s, the length passed; the timeout keeps
        // a hung window from blocking the caller.
        let ok = unsafe {
            SendMessageTimeoutW(
                window,
                WM_GETTEXT,
                buf.len(),
                buf.as_mut_ptr() as isize,
                SMTO_ABORTIFHUNG,
                200,
                &mut copied,
            )
        };
        if ok == 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..copied.min(buf.len())])
            .trim()
            .to_string()
    }

    fn owner(window: Hwnd) -> u32 {
        let mut pid = 0u32;
        // SAFETY: `pid` is a valid out pointer.
        unsafe { GetWindowThreadProcessId(window, &mut pid) };
        pid
    }

    pub(super) fn dismiss(window: Hwnd) {
        const WM_COMMAND: u32 = 0x0111;
        const WM_CLOSE: u32 = 0x0010;
        const IDIGNORE: usize = 5;
        // SAFETY: posting to a window handle; a stale one just fails.
        unsafe {
            PostMessageW(window, WM_COMMAND, IDIGNORE, 0);
            PostMessageW(window, WM_CLOSE, 0, 0);
        }
    }

    pub(super) fn error_dialogs(pids: &[u32], launcher_pids: &[u32]) -> Vec<ErrorDialog> {
        top_level_windows()
            .into_iter()
            .filter_map(|window| {
                let pid = owner(window);
                let is_launcher = launcher_pids.contains(&pid);
                if !is_launcher && !pids.contains(&pid) {
                    return None;
                }
                // SAFETY: plain query on a window handle; a stale one returns 0.
                if unsafe { IsWindowVisible(window) } == 0 {
                    return None;
                }
                if !is_launcher && class_name(window) != DIALOG_CLASS {
                    return None;
                }
                let title = text(window);
                let lines: Vec<String> = children(window)
                    .into_iter()
                    .filter(|&child| !class_name(child).to_ascii_lowercase().contains("button"))
                    .map(text)
                    .filter(|t| !t.is_empty())
                    .collect();
                if title.is_empty() && lines.is_empty() {
                    return None;
                }
                Some(ErrorDialog {
                    window,
                    pid,
                    title,
                    lines,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog(title: &str, lines: &[&str]) -> ErrorDialog {
        ErrorDialog {
            window: 1,
            pid: 2,
            title: title.to_string(),
            lines: lines.iter().map(|l| l.to_string()).collect(),
        }
    }

    #[test]
    fn the_three_boxes_seen_live_get_plain_summaries() {
        let steam = dialog(
            "Fatal Error",
            &["Failed to initalize authentication interface. Exiting..."],
        );
        assert!(steam.summary().contains("Steam isn't running"));

        let hlae = dialog(
            "HLAE Error",
            &[
                "AfxHook error",
                "Code:",
                "1",
                "Description:",
                "No description available.",
            ],
        );
        assert!(
            hlae.summary().contains("AfxHook error, code 1"),
            "{}",
            hlae.summary()
        );

        let assert_box = dialog(
            "Microsoft Visual C++ Runtime Library",
            &[
                // The real box's text, as read live on 2026-09-29.
                "Assertion failed!\n\nProgram: ...filesystem_stdio.dll\nLine: 84\n\nExpression: !m_bMounted\n\nFor information on how your program can cause an assertion\nfailure, see the Visual C++ documentation on asserts",
            ],
        );
        assert_eq!(
            assert_box.summary(),
            "The game stopped on one of its own internal checks (\"Assertion failed\": !m_bMounted)."
        );
    }

    #[test]
    fn an_unknown_box_is_quoted_and_the_log_line_has_everything() {
        let other = dialog("Something", &["line one", "line two"]);
        assert_eq!(
            other.summary(),
            "The game showed an error box, \"Something\": line one line two"
        );
        assert_eq!(other.full_text(), "\"Something\": line one | line two");
    }

    #[test]
    fn a_second_game_is_named() {
        let twice = dialog(
            "Error",
            &["Only one instance of this game can be run at a time."],
        );
        assert!(twice.summary().contains("already running"));
    }

    /// Nothing belongs to a process id that can't exist, and nothing of this
    /// test's own is a dialog.
    #[test]
    fn finds_nothing_for_processes_with_no_boxes() {
        assert!(error_dialogs(&[u32::MAX], &[]).is_empty());
        assert!(error_dialogs(&[std::process::id()], &[]).is_empty());
    }
}
