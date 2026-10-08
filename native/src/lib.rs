pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(not(target_arch = "wasm32"))]
use analysis::Analysis;
#[cfg(not(target_arch = "wasm32"))]
use std::fs;
#[cfg(not(target_arch = "wasm32"))]
use std::io::Read;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

pub mod patch;

#[cfg(not(target_arch = "wasm32"))]
pub mod hlcr;

pub mod shared;
pub mod sys;
pub mod utils;

mod messages;

#[cfg(not(target_arch = "wasm32"))]
pub mod capture_engine;

/// Which demos in a folder recorded more than one map (#624).
#[cfg(not(target_arch = "wasm32"))]
pub mod demo_maps_scan;
/// Splitting a demo that recorded more than one map (#624).
#[cfg(not(target_arch = "wasm32"))]
pub mod demo_split;
/// The HD texture files: what is built, and fetching the upscaler (#372).
#[cfg(not(target_arch = "wasm32"))]
pub mod hd;

/// Driving OBS Studio as an alternate capture path (#65).
#[cfg(not(target_arch = "wasm32"))]
pub mod obs;

/// The review mode's queue and answers (#623).
pub mod review_queue;

/// Helpers this crate's own tests share. See `Scratch` on why a temporary
/// directory needs a guard rather than a trailing `remove_dir_all` (#253).
#[cfg(test)]
mod test_support;

/// The demo file an analysis came from; defined beside the cache it is
/// saved in, which the hook DLL reads too.
#[cfg(not(target_arch = "wasm32"))]
pub use analysis::cache::FileInfo;

#[cfg(not(target_arch = "wasm32"))]
pub fn run_analyzer(demo_path: &PathBuf) -> Result<(FileInfo, Analysis), String> {
    run_analyzer_with_progress(demo_path, |_, _| {})
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run_analyzer_with_progress<F>(
    demo_path: &PathBuf,
    progress_cb: F,
) -> Result<(FileInfo, Analysis), String>
where
    F: FnMut(usize, usize),
{
    let mut file = fs::OpenOptions::new()
        .read(true)
        .open(demo_path)
        .map_err(|e| format!("Could not open {}: {}", demo_path.display(), e))?;

    let mut bytes: Vec<u8> = vec![];

    file.read_to_end(&mut bytes)
        .map_err(|e| format!("Could not read {}: {}", demo_path.display(), e))?;

    let analysis = Analysis::try_from_bytes_with_progress(bytes.as_slice(), progress_cb)?;
    let file_info = FileInfo::of(demo_path)?;

    Ok((file_info, analysis))
}

/// Where the analyzer cache lives: `analysis::cache` holds the format.
#[cfg(not(target_arch = "wasm32"))]
pub fn analyzer_cache_root() -> PathBuf {
    crate::shared::paths::get_appdata_dir().join("analyzer_cache")
}

/// Same as `run_analyzer_with_progress`, but backed by an on-disk JSON cache
/// keyed on the demo's canonicalized path, size, and mtime. A cache hit skips
/// straight to a ~10-15ms file read + deserialize instead of the full ~1.3s
/// parse; `progress_cb` is not invoked on the cache-hit path. Returns whether
/// the result came from cache as the third tuple element.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_analyzer_cached<F>(
    demo_path: &PathBuf,
    progress_cb: F,
) -> Result<(FileInfo, Analysis, bool), String>
where
    F: FnMut(usize, usize),
{
    let root = analyzer_cache_root();
    if let Some((file_info, analysis)) = analysis::cache::load(&root, demo_path) {
        return Ok((file_info, analysis, true));
    }
    let (file_info, analysis) = run_analyzer_with_progress(demo_path, progress_cb)?;
    store_in_analyzer_cache(&root, demo_path, &file_info, &analysis);
    Ok((file_info, analysis, false))
}

/// Deletes the `v<N>` folders under the analyzer cache that an older schema
/// left: `N` below `current`, and nothing written there for a week. A schema
/// bump makes every old entry a miss forever, so the folder is dead weight
/// (359 MB of it on one machine, 2026-09). The week spares an older build
/// still in use beside this one (an installed release next to a dev build),
/// whose cache would otherwise be deleted under it every run. Leaves anything
/// that isn't a `v<N>` folder alone. Best-effort.
#[cfg(not(target_arch = "wasm32"))]
fn sweep_stale_analyzer_caches(cache_root: &std::path::Path, current: u32) -> usize {
    sweep_stale_analyzer_caches_older_than(cache_root, current, STALE_CACHE_AGE)
}

#[cfg(not(target_arch = "wasm32"))]
const STALE_CACHE_AGE: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 60 * 60);

#[cfg(not(target_arch = "wasm32"))]
fn sweep_stale_analyzer_caches_older_than(
    cache_root: &std::path::Path,
    current: u32,
    min_age: std::time::Duration,
) -> usize {
    let Ok(entries) = fs::read_dir(cache_root) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(version) = name
            .to_str()
            .and_then(|n| n.strip_prefix('v'))
            .and_then(|v| v.parse::<u32>().ok())
        else {
            continue;
        };
        let idle_long_enough = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age >= min_age);
        if version < current
            && idle_long_enough
            && entry.file_type().is_ok_and(|t| t.is_dir())
            && fs::remove_dir_all(entry.path()).is_ok()
        {
            removed += 1;
        }
    }
    removed
}

/// Saves an analysis to the cache, and once per run clears out the folders
/// older schemas left: nothing reads them again, and they had grown to
/// hundreds of MB.
#[cfg(not(target_arch = "wasm32"))]
fn store_in_analyzer_cache(
    root: &std::path::Path,
    demo_path: &std::path::Path,
    file_info: &FileInfo,
    analysis: &Analysis,
) {
    static SWEPT: std::sync::Once = std::sync::Once::new();
    let sweep_root = root.to_path_buf();
    SWEPT.call_once(|| {
        std::thread::spawn(move || {
            sweep_stale_analyzer_caches(&sweep_root, analysis::cache::SCHEMA_VERSION)
        });
    });
    // Best-effort: a cache write failure must never fail the caller.
    let _ = analysis::cache::store(root, demo_path, file_info, analysis);
}

/// Writes `analysis` (already computed by a folder scan, e.g.
/// `scan_demo_for_highlights_with_analysis`) straight into the analyzer
/// cache, so a later `run_analyzer_cached` call for the same demo hits the
/// ~10-15ms cache path instead of re-parsing. Best-effort and silent on any
/// failure — cache warming must never affect the caller's own result.
#[cfg(not(target_arch = "wasm32"))]
pub fn warm_analyzer_cache(demo_path: &std::path::Path, analysis: &Analysis) {
    let Ok(file_info) = FileInfo::of(demo_path) else {
        return;
    };
    store_in_analyzer_cache(&analyzer_cache_root(), demo_path, &file_info, analysis);
}

#[cfg(not(target_arch = "wasm32"))]
static SESSION_HEADER_WRITTEN: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Date string (`YYYYMMDD`) `log_markdown` last wrote to, so a session
/// running past midnight can be detected and cross-referenced between the
/// two days' files instead of just silently stopping mid-file.
#[cfg(not(target_arch = "wasm32"))]
static LAST_LOG_DATE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// How many days' worth of activity logs to keep on disk — older files are
/// pruned the first time a given app launch logs anything.
#[cfg(not(target_arch = "wasm32"))]
const MAX_RETAINED_ACTIVITY_LOG_DAYS: usize = 30;

/// Directory `log_markdown` writes into.
#[cfg(not(target_arch = "wasm32"))]
pub fn activity_log_dir() -> std::path::PathBuf {
    redirected_log_dir().unwrap_or_else(|| crate::shared::paths::get_appdata_dir().join("logs"))
}

/// Where the activity log goes when it must not go to the user's own.
///
/// The activity log is this project's primary record of what a capture did —
/// the decal flush work was reconstructed from it repeatedly. Test runs
/// exercise real pipeline code that logs, so without this they append fixture
/// names like `no_such_demo_should_ever_be_read.dem` into
/// `%APPDATA%/dod-studio/logs` under their own session headers, indistinguishable
/// from a genuine capture that failed. It also means `cargo test` silently
/// mutates a user file outside the repo. See issue #64.
///
/// Two ways in. `DOD_STUDIO_LOG_DIR` redirects it for anyone who needs it —
/// an integration test, a packaging check, someone reproducing a bug without
/// stamping on their real record — and a `cfg(test)` build redirects itself.
/// The env var is checked first so a test binary compiled *without* `cfg(test)`
/// (an integration test, or another crate's tests linking this one as an
/// ordinary dependency) can still be pointed somewhere safe.
#[cfg(not(target_arch = "wasm32"))]
fn redirected_log_dir() -> Option<std::path::PathBuf> {
    if let Some(dir) = std::env::var_os("DOD_STUDIO_LOG_DIR") {
        return Some(std::path::PathBuf::from(dir));
    }
    if cfg!(test) {
        return Some(std::env::temp_dir().join("dod_studio_test_logs"));
    }
    None
}

/// Path `log_markdown` is currently writing to — one file per calendar day
/// (shared across every app launch that day), so this is recomputed from
/// the current date on every call rather than cached, and just starts
/// pointing at tomorrow's file on its own if a session runs past midnight.
/// Exposed so the frontend can offer a "View Logs" affordance without
/// duplicating the path logic.
#[cfg(not(target_arch = "wasm32"))]
pub fn activity_log_path() -> std::path::PathBuf {
    use chrono::Local;
    activity_log_dir().join(format!("activity_{}.md", Local::now().format("%Y%m%d")))
}

/// Deletes `activity_*.md` files beyond `MAX_RETAINED_ACTIVITY_LOG_DAYS` —
/// filenames are zero-padded `YYYYMMDD` dates, so lexical sort order is
/// chronological order.
#[cfg(not(target_arch = "wasm32"))]
fn prune_old_activity_logs(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("activity_") && n.ends_with(".md"))
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    if files.len() > MAX_RETAINED_ACTIVITY_LOG_DAYS {
        for old in &files[..files.len() - MAX_RETAINED_ACTIVITY_LOG_DAYS] {
            let _ = std::fs::remove_file(old);
        }
    }
}

pub fn log_markdown(msg: &str) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use chrono::Local;
        use std::io::Write;

        static LOG_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOG_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

        let dir = activity_log_dir();
        let _ = std::fs::create_dir_all(&dir);
        let today = Local::now().format("%Y%m%d").to_string();
        let log_path_md = dir.join(format!("activity_{}.md", today));
        let is_first_write =
            !SESSION_HEADER_WRITTEN.swap(true, std::sync::atomic::Ordering::SeqCst);

        let mut last_date = LAST_LOG_DATE.lock().unwrap_or_else(|e| e.into_inner());
        if is_first_write {
            prune_old_activity_logs(&dir);
        } else if last_date.as_deref().is_some_and(|prev| prev != today) {
            // Same session, but the date rolled over since the last log line —
            // leave a pointer in both files so this doesn't just look like the
            // session stopped mid-file to someone reading yesterday's log.
            let prev_path = dir.join(format!("activity_{}.md", last_date.as_deref().unwrap()));
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(&prev_path)
            {
                let _ = writeln!(
                    f,
                    "\n(session continues past midnight — see activity_{}.md)\n",
                    today
                );
                let _ = f.sync_all();
            }
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(&log_path_md)
            {
                let _ = writeln!(
                    f,
                    "(continued from activity_{}.md — same session, crossed midnight)\n",
                    last_date.as_deref().unwrap()
                );
                let _ = f.sync_all();
            }
        }
        *last_date = Some(today);
        drop(last_date);

        if let Ok(mut f) = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&log_path_md)
        {
            if is_first_write {
                let time_str = Local::now().format("%Y-%m-%d @ %H:%M %Z").to_string();
                let _ = writeln!(
                    f,
                    "\n\n========== New Session: {} ====================\n",
                    time_str
                );
            }
            let time_str = Local::now().format("%H:%M:%S").to_string();
            let _ = writeln!(f, "* [{}] {}", time_str, msg);
            let _ = f.sync_all();
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        log::info!("{}", msg);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod activity_log_tests {
    use super::*;

    /// `DOD_STUDIO_LOG_DIR` is process-global, not thread-local, but `cargo
    /// test` runs every test in this file on its own thread of the same
    /// process by default. Without this, `the_env_override_outranks_everything`
    /// temporarily overriding the var could interleave with
    /// `writing_a_line_lands_in_the_redirected_directory`'s write-then-read
    /// (both go through `redirected_log_dir()`), so the write landed under
    /// one directory and the read checked another — reproduced reliably in CI
    /// (2/2 runs) once these tests started running in the same binary as
    /// enough others to make the interleaving likely. Any test that reads or
    /// writes through `redirected_log_dir()` must hold this for its full
    /// touch-env-through-assert span, not just the `set_var` call.
    static ENV_VAR_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// The invariant: a test run must not write outside the repo and its
    /// scratch. This is the one that matters — everything else here just
    /// explains how it is held.
    #[test]
    fn a_test_run_never_logs_into_the_users_own_activity_log() {
        let real = crate::shared::paths::get_appdata_dir().join("logs");
        assert_ne!(
            activity_log_dir(),
            real,
            "cargo test is appending fixture sessions to the real capture record"
        );
        assert!(
            !activity_log_path().starts_with(&real),
            "the log file landed inside the user's own log directory"
        );
    }

    #[test]
    fn the_env_override_outranks_everything() {
        let _guard = ENV_VAR_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        // The override exists for the case cfg(test) cannot reach: a binary or
        // integration test that links this crate as an ordinary dependency, so
        // it is compiled without cfg(test) and would otherwise write to the
        // user's real log.
        let want = std::env::temp_dir().join("dod_studio_override_probe");
        let previous = std::env::var_os("DOD_STUDIO_LOG_DIR");
        unsafe { std::env::set_var("DOD_STUDIO_LOG_DIR", &want) };
        let got = redirected_log_dir();
        match previous {
            Some(p) => unsafe { std::env::set_var("DOD_STUDIO_LOG_DIR", p) },
            None => unsafe { std::env::remove_var("DOD_STUDIO_LOG_DIR") },
        }
        assert_eq!(got, Some(want));
    }

    #[test]
    fn writing_a_line_lands_in_the_redirected_directory() {
        let _guard = ENV_VAR_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        // Not just the path calculation — the whole write path, since
        // `log_markdown` computes the directory itself rather than taking one.
        log_markdown("activity log redirect probe");
        let path = activity_log_path();
        assert!(path.exists(), "nothing was written to {}", path.display());
        let body = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(body.contains("activity log redirect probe"));
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod analyzer_cache_tests {
    use super::*;
    use crate::test_support::Scratch;

    /// Old schema folders idle for the grace period go; the current one, a
    /// newer one, a recently used one, and anything that isn't a `v<N>`
    /// folder stay.
    #[test]
    fn stale_cache_versions_are_swept() {
        let root = Scratch::new("analyzer_cache_sweep");
        for dir in ["v1", "v2", "v3", "v4", "other"] {
            fs::create_dir_all(root.join(dir)).unwrap();
            fs::write(root.join(dir).join("x.json"), b"{}").unwrap();
        }
        fs::write(root.join("v9"), b"a file, not a folder").unwrap();

        // Just written, so a week's grace keeps everything.
        assert_eq!(sweep_stale_analyzer_caches(&root, 3), 0);
        assert!(root.join("v1").is_dir());

        let swept = sweep_stale_analyzer_caches_older_than(&root, 3, std::time::Duration::ZERO);
        assert_eq!(swept, 2);
        assert!(!root.join("v1").exists());
        assert!(!root.join("v2").exists());
        assert!(root.join("v3").join("x.json").is_file());
        // A newer build's cache is never this build's to delete.
        assert!(root.join("v4").is_dir());
        assert!(root.join("other").is_dir());
        assert!(root.join("v9").is_file());
    }

    /// Entries written before the cache moved to `analysis::cache` were named
    /// with `hl_demo_auditor`'s FNV-1a: the name must not change, or every
    /// cached demo is analysed again.
    #[test]
    fn cache_entries_keep_their_names() {
        let root = Scratch::new("analyzer_cache_names");
        let demo = root.join("named.dem");
        fs::write(&demo, b"demo").unwrap();
        let canonical = fs::canonicalize(&demo).unwrap();
        let hash = crate::utils::demo_hasher::fnv1a_hash(canonical.to_string_lossy().as_bytes());
        assert_eq!(
            analysis::cache::entry_path(&root, &demo).unwrap(),
            root.join(format!("v{}", analysis::cache::SCHEMA_VERSION))
                .join(format!("{hash:016x}.json"))
        );
    }
}
