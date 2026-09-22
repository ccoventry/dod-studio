//! Best-effort, non-blocking diagnostics. Deliberately never pops a message
//! box or otherwise blocks -- this DLL runs inside an automated, headless
//! capture pipeline, and a blocking dialog would hang the whole batch.

use std::io::Write;

use windows_sys::Win32::System::SystemInformation::GetLocalTime;

/// Local wall-clock time as `HH:MM:SS.mmm`.
///
/// Wall clock rather than time-since-load: the point of the timestamps is to
/// line log lines up against something that happened in the game while
/// watching it, and milliseconds are kept because most of what this logs
/// changes at frame rate, where second resolution would collapse a burst of
/// distinct events into one indistinguishable clump.
fn timestamp() -> String {
    // Safety: fills a plain struct we own; cannot fail.
    let mut now = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut now) };
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        now.wHour, now.wMinute, now.wSecond, now.wMilliseconds
    )
}

/// Where the log goes: `%APPDATA%\dod-studio\logs\dodstudio_goldsrc_hooks.log`.
///
/// The same folder the app's own activity log uses, so there is one place to
/// look rather than two. `native`'s `activity_log_dir()` resolves it through
/// `dirs::config_dir()`, which on Windows is `FOLDERID_RoamingAppData` -- the
/// same directory `%APPDATA%` names, so this matches it without taking a
/// dependency on `dirs` in a DLL that is deliberately kept to `windows-sys`.
///
/// `DOD_STUDIO_LOG_DIR` redirects it, exactly as it redirects the activity log,
/// so a test run or a packaging check can keep its output out of the user's
/// own logs -- and a `cfg(test)` build redirects itself, so that safety is not
/// something every future test has to remember. The env var is checked first
/// so a test binary compiled *without* `cfg(test)` can still be pointed
/// somewhere safe. This mirrors `native`'s `redirected_log_dir` (issue #64).
///
/// Falls back to `%TEMP%` if neither resolves. Logging is best-effort and must
/// never be the reason a capture fails, so there is always somewhere to go.
fn log_path() -> Option<std::path::PathBuf> {
    if let Some(redirected) = std::env::var_os("DOD_STUDIO_LOG_DIR") {
        let dir = std::path::PathBuf::from(redirected);
        let _ = std::fs::create_dir_all(&dir);
        return Some(dir.join(LOG_FILE));
    }
    // Without this, `cargo test` appends to the developer's own live hook log,
    // where a test's lines are indistinguishable from a real session's -- and
    // that log is what gets read to diagnose a live test.
    if cfg!(test) {
        let dir = std::env::temp_dir().join("dod_studio_test_logs");
        let _ = std::fs::create_dir_all(&dir);
        return Some(dir.join(LOG_FILE));
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let dir = std::path::PathBuf::from(appdata).join("dod-studio").join("logs");
        if std::fs::create_dir_all(&dir).is_ok() {
            return Some(dir.join(LOG_FILE));
        }
    }
    std::env::var_os("TEMP").map(|t| std::path::PathBuf::from(t).join(LOG_FILE))
}

/// Matches the DLL's own filename, so the log is obviously its log.
const LOG_FILE: &str = "dodstudio_goldsrc_hooks.log";

/// Appends a line to the log (see [`log_path`]). Failures are swallowed --
/// logging must never be the thing that destabilizes the host process.
pub unsafe fn report(message: &str) {
    let Some(path) = log_path() else {
        return;
    };

    // Demo time alongside wall clock. Wall clock cannot be matched against
    // something seen on screen once playback is paused, seeked or
    // fast-forwarded; the demo clock can. Omitted before the first frame,
    // when there is no playback to be at a position in.
    let demo = match crate::engine::client_time() {
        t if t > 0.0 => format!(" [demo {t:9.3}]"),
        _ => String::new(),
    };

    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "[{}]{demo} [dodstudio_goldsrc_hooks] {message}", timestamp());
    }
}

/// Writes a visual break before a new session's first line, so the log file
/// can be left in place across runs instead of deleted each time -- just
/// copy from the last separator down.
pub unsafe fn new_session_separator() {
    // The date goes here rather than on every line: the log spans days, but a
    // session does not, so it only needs stating once per session.
    let mut now = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut now) };
    unsafe {
        report(&format!(
            "========== new session, {:04}-{:02}-{:02} ==========",
            now.wYear, now.wMonth, now.wDay
        ))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reason this module has a `cfg(test)` branch at all: a `cargo test`
    /// run must not append to the developer's real hook log, where its lines
    /// are indistinguishable from a real session's. Asserts the behaviour --
    /// where the path is *not* -- rather than the exact fallback directory.
    #[test]
    fn a_test_build_does_not_log_into_the_live_log_directory() {
        // The env var wins over the `cfg(test)` fallback by design, so this
        // only has something to prove when it is unset -- which is the
        // configuration an ordinary `cargo test` run has.
        if std::env::var_os("DOD_TOOLS_LOG_DIR").is_some() {
            return;
        }

        let path = log_path().expect("a test build always resolves a path");

        if let Some(appdata) = std::env::var_os("APPDATA") {
            let live = std::path::PathBuf::from(appdata).join("dod-tools").join("logs");
            assert!(
                !path.starts_with(&live),
                "test logging resolved to the live log directory: {}",
                path.display()
            );
        }
        assert!(
            path.starts_with(std::env::temp_dir()),
            "expected a temp fallback, got {}",
            path.display()
        );
    }
}
