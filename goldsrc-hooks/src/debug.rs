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

/// Where the log goes: `%APPDATA%\dod-studio\logs\dodstudio_goldsrc_hooks_YYYYMMDD.log`.
///
/// The same folder the app's own activity log uses, so there is one place to
/// look rather than two. `native`'s `activity_log_dir()` resolves it through
/// `dirs::config_dir()`, which on Windows is `FOLDERID_RoamingAppData` -- the
/// same directory `%APPDATA%` names, so this matches it without taking a
/// dependency on `dirs` in a DLL that is deliberately kept to `windows-sys`.
///
/// One file per calendar day, same as `native`'s own `activity_*.md` --
/// before this, a single ever-appending file grew unbounded (269 KB and
/// climbing on a machine that had been recording for a month), with nothing
/// to rotate it. `new_session_separator`'s existing per-session marker is
/// kept regardless: a day can hold several sessions, and the date-stamped
/// filename alone does not say where one session ended and the next began.
/// Unlike `native`'s activity log, a session that crosses midnight is not
/// specially cross-referenced between the two files -- a lower-stakes
/// diagnostic trace, not a shared record, so the reader checking the next
/// day's file is an acceptable simplification here.
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
        return Some(dir.join(log_file_name()));
    }
    // Without this, `cargo test` appends to the developer's own live hook log,
    // where a test's lines are indistinguishable from a real session's -- and
    // that log is what gets read to diagnose a live test.
    if cfg!(test) {
        let dir = std::env::temp_dir().join("dod_studio_test_logs");
        let _ = std::fs::create_dir_all(&dir);
        return Some(dir.join(log_file_name()));
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let dir = std::path::PathBuf::from(appdata).join("dod-studio").join("logs");
        if std::fs::create_dir_all(&dir).is_ok() {
            return Some(dir.join(log_file_name()));
        }
    }
    std::env::var_os("TEMP").map(|t| std::path::PathBuf::from(t).join(log_file_name()))
}

/// The prefix every dated log file shares, so [`prune_old_hook_logs`] can
/// recognise its own files without also sweeping up an unrelated one that
/// happens to share the log directory.
const LOG_FILE_PREFIX: &str = "dodstudio_goldsrc_hooks_";
const LOG_FILE_SUFFIX: &str = ".log";

/// Today's log filename. Recomputed on every call rather than cached, so a
/// session that runs past midnight just starts writing into tomorrow's file
/// on its own -- the same reasoning as `native`'s `activity_log_path`.
fn log_file_name() -> String {
    // Safety: fills a plain struct we own; cannot fail.
    let mut now = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut now) };
    format!("{LOG_FILE_PREFIX}{:04}{:02}{:02}{LOG_FILE_SUFFIX}", now.wYear, now.wMonth, now.wDay)
}

/// How many days' worth of hook logs to keep on disk -- matches `native`'s
/// own `MAX_RETAINED_ACTIVITY_LOG_DAYS`, so both logs age out on the same
/// schedule rather than one quietly outliving the other.
const MAX_RETAINED_HOOK_LOG_DAYS: usize = 30;

/// Deletes dated log files beyond [`MAX_RETAINED_HOOK_LOG_DAYS`] -- filenames
/// are zero-padded `YYYYMMDD` dates, so lexical sort order is chronological
/// order, the same trick `native`'s `prune_old_activity_logs` uses.
fn prune_old_hook_logs(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with(LOG_FILE_PREFIX) && n.ends_with(LOG_FILE_SUFFIX))
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    if files.len() > MAX_RETAINED_HOOK_LOG_DAYS {
        for old in &files[..files.len() - MAX_RETAINED_HOOK_LOG_DAYS] {
            let _ = std::fs::remove_file(old);
        }
    }
}

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
///
/// Also where old dated logs get pruned: once per session, the same trigger
/// point `native`'s activity log uses ("the first time a given app launch
/// logs anything"), rather than on every single [`report`] call.
pub unsafe fn new_session_separator() {
    if let Some(path) = log_path()
        && let Some(dir) = path.parent()
    {
        prune_old_hook_logs(dir);
    }

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
        if std::env::var_os("DOD_STUDIO_LOG_DIR").is_some() {
            return;
        }

        let path = log_path().expect("a test build always resolves a path");

        if let Some(appdata) = std::env::var_os("APPDATA") {
            let live = std::path::PathBuf::from(appdata).join("dod-studio").join("logs");
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

    #[test]
    fn todays_log_file_name_is_dated_and_matches_the_prune_pattern() {
        let name = log_file_name();
        assert!(name.starts_with(LOG_FILE_PREFIX), "{name}");
        assert!(name.ends_with(LOG_FILE_SUFFIX), "{name}");
        let date_part = &name[LOG_FILE_PREFIX.len()..name.len() - LOG_FILE_SUFFIX.len()];
        assert_eq!(date_part.len(), 8, "expected YYYYMMDD, got {date_part:?} from {name}");
        assert!(date_part.chars().all(|c| c.is_ascii_digit()), "expected all digits, got {date_part:?}");
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("dod_studio_debug_rs_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The exact class of bug this replaces: a single ever-appending file
    /// with no way to age out. One file beyond the retention window: the
    /// single oldest must go, everything else must survive.
    #[test]
    fn prunes_the_oldest_dated_log_once_past_the_retention_window() {
        let dir = scratch("prune");
        for day in 1..=MAX_RETAINED_HOOK_LOG_DAYS + 1 {
            std::fs::write(dir.join(format!("{LOG_FILE_PREFIX}202601{day:02}{LOG_FILE_SUFFIX}")), "x").unwrap();
        }
        // A file that merely shares the directory must survive untouched --
        // pruning is scoped to this module's own filename shape.
        std::fs::write(dir.join("unrelated.log"), "keep me").unwrap();

        prune_old_hook_logs(&dir);

        assert!(
            !dir.join(format!("{LOG_FILE_PREFIX}20260101{LOG_FILE_SUFFIX}")).exists(),
            "the single oldest file must be pruned"
        );
        for day in 2..=MAX_RETAINED_HOOK_LOG_DAYS + 1 {
            assert!(
                dir.join(format!("{LOG_FILE_PREFIX}202601{day:02}{LOG_FILE_SUFFIX}")).exists(),
                "day {day} is within the retention window and must survive"
            );
        }
        assert!(dir.join("unrelated.log").exists(), "must not touch a file outside its own naming pattern");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn does_not_prune_when_within_the_retention_window() {
        let dir = scratch("no_prune");
        std::fs::write(dir.join(format!("{LOG_FILE_PREFIX}20260101{LOG_FILE_SUFFIX}")), "x").unwrap();

        prune_old_hook_logs(&dir);

        assert!(
            dir.join(format!("{LOG_FILE_PREFIX}20260101{LOG_FILE_SUFFIX}")).exists(),
            "a single file is nowhere near the retention window and must survive"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
