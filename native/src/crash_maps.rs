//! Maps a game session crashed on, with a cause DoD Studio knows (#207).
//!
//! Some crashes can't be predicted from a demo file: the #384 hull trace
//! depends on which map was loaded before, the #374 temp entity on what was
//! on screen. When a capture batch ends because the game vanished, the hook
//! DLL's own log says why: its crash recorder writes a `CRASH: ... at
//! <module>+<offset>` block, and `level: maps/<map>.bsp` lines say where.
//! A crash at a site `KNOWN_CRASHES` lists is remembered by map in
//! `%APPDATA%\dod-studio\crash_maps.json`, and later batches that include
//! that map are warned before they start, with the cause and what to do.
//!
//! The list of known sites is `goldsrc-hooks/tools/crash_report.py`'s
//! `KNOWN`; a test keeps the two in step.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Crash sites DoD Studio knows, `module+offset` -> what it is and what to do.
pub const KNOWN_CRASHES: &[(&str, &str)] = &[
    (
        "client.dll+0x225cc",
        "DoD wrote into a missing temp entity (#374). The hook guards this; its tempent_fix log line says why it didn't.",
    ),
    (
        "client.dll+0x226a4",
        "DoD wrote into a missing temp entity (#374). The hook guards this; its tempent_fix log line says why it didn't.",
    ),
    (
        "client.dll+0xb23e",
        "DoD wrote into a missing temp entity (#374). The hook guards this; its tempent_fix log line says why it didn't.",
    ),
    (
        "client.dll+0xb2e2",
        "DoD wrote into a missing temp entity (#374). The hook guards this; its tempent_fix log line says why it didn't.",
    ),
    (
        "client.dll+0x31574",
        "DoD wrote into a missing temp entity (#374). The hook guards this; its tempent_fix log line says why it didn't.",
    ),
    (
        "client.dll+0x316ad",
        "DoD wrote into a missing temp entity (#374). The hook guards this; its tempent_fix log line says why it didn't.",
    ),
    (
        "core.dll+0x16d6",
        "An HLTV demo with more than 256 entities in one snapshot (#207). The pre-Anniversary engine can't play it; the 25th Anniversary one can.",
    ),
    (
        "hw.dll+0x6c839",
        "The engine's movement trace ran on the previous map's collision data (#384). The hook guards this; its hull_trace_guard log line says why it didn't.",
    ),
    (
        "hw.dll+0x6c8d1",
        "The engine's movement trace ran on the previous map's collision data (#384). The hook guards this; its hull_trace_guard log line says why it didn't.",
    ),
    (
        "hw.dll+0x1e2612",
        "The engine's movement trace ran on the previous map's collision data (#384, 25th Anniversary build). The hook guards this; its hull_trace_guard log line says why it didn't.",
    ),
];

/// What the cause of a crash at `site` is, when it is known.
pub fn known_cause(site: &str) -> Option<&'static str> {
    KNOWN_CRASHES
        .iter()
        .find(|(s, _)| *s == site)
        .map(|(_, cause)| *cause)
}

/// The crash in a hook log's last session, if it had one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCrash {
    /// `module+offset`.
    pub site: String,
    /// The fault, e.g. "access violation (0xc0000005)".
    pub what: String,
    /// The map loaded when it happened, without `maps/` or `.bsp`.
    pub map: Option<String>,
    /// "pre-Anniversary" or "25th Anniversary", from the hook's install lines.
    pub build: Option<String>,
}

const SESSION_MARK: &str = "========== new session";

/// The first crash in the last session of a hook log's text.
pub fn last_session_crash(log: &str) -> Option<SessionCrash> {
    let session = &log[log.rfind(SESSION_MARK).unwrap_or(0)..];
    let mut map = None;
    let mut build = None;
    for line in session.lines() {
        let msg = line
            .split("[dodstudio_goldsrc_hooks] ")
            .nth(1)
            .unwrap_or("");
        if build.is_none() {
            if msg.contains("25th Anniversary hw.dll") {
                build = Some("25th Anniversary".to_string());
            } else if msg.contains("pre-Anniversary hw.dll") {
                build = Some("pre-Anniversary".to_string());
            }
        }
        if let Some(level) = msg.strip_prefix("level: ") {
            let name = level
                .trim()
                .trim_start_matches("maps/")
                .trim_end_matches(".bsp");
            map = Some(name.to_string());
        }
        if let Some(rest) = msg.strip_prefix("CRASH: ") {
            // "<what> at <module+offset> (<address>) -- <detail>"; the
            // indented lines after it (registers, stack) start with spaces.
            if rest.starts_with(' ') {
                continue;
            }
            let (what, after) = rest.split_once(" at ")?;
            let site = after.split_whitespace().next()?.to_string();
            return Some(SessionCrash {
                site,
                what: what.to_string(),
                map,
                build,
            });
        }
    }
    None
}

/// The newest hook log in `dir` (`dodstudio_goldsrc_hooks_YYYYMMDD.log`).
pub fn newest_hook_log(dir: &Path) -> Option<PathBuf> {
    let mut logs: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .is_some_and(|n| n.starts_with("dodstudio_goldsrc_hooks_") && n.ends_with(".log"))
        })
        .collect();
    logs.sort();
    logs.pop()
}

/// One remembered crash on one map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashMap {
    pub map: String,
    pub site: String,
    pub cause: String,
    #[serde(default)]
    pub build: Option<String>,
    /// How many batches it ended, and when last (seconds since 1970).
    pub count: u32,
    pub last_unix_secs: u64,
}

fn store_path(dir: &Path) -> PathBuf {
    dir.join("crash_maps.json")
}

/// Every remembered crash, from `dir` (the app's data folder).
pub fn load(dir: &Path) -> Vec<CrashMap> {
    std::fs::read(store_path(dir))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Remembers `crash` when its cause is known and it has a map. Returns the
/// entry it kept, or `None` for a crash it can't place.
pub fn record(dir: &Path, crash: &SessionCrash, now_unix_secs: u64) -> Option<CrashMap> {
    let cause = known_cause(&crash.site)?;
    let map = crash.map.clone()?;
    let mut all = load(dir);
    let entry = match all
        .iter_mut()
        .find(|e| e.map.eq_ignore_ascii_case(&map) && e.site == crash.site)
    {
        Some(e) => {
            e.count += 1;
            e.last_unix_secs = now_unix_secs;
            e.build = crash.build.clone().or(e.build.take());
            e.clone()
        }
        None => {
            let e = CrashMap {
                map,
                site: crash.site.clone(),
                cause: cause.to_string(),
                build: crash.build.clone(),
                count: 1,
                last_unix_secs: now_unix_secs,
            };
            all.push(e.clone());
            e
        }
    };
    let json = serde_json::to_vec_pretty(&all).ok()?;
    std::fs::write(store_path(dir), json).ok()?;
    Some(entry)
}

/// The remembered crashes on any of `maps` (case doesn't matter).
pub fn on_maps(dir: &Path, maps: &[String]) -> Vec<CrashMap> {
    load(dir)
        .into_iter()
        .filter(|e| maps.iter().any(|m| m.eq_ignore_ascii_case(&e.map)))
        .collect()
}

/// Forgets every remembered crash on `map`.
pub fn forget(dir: &Path, map: &str) -> std::io::Result<()> {
    let kept: Vec<CrashMap> = load(dir)
        .into_iter()
        .filter(|e| !e.map.eq_ignore_ascii_case(map))
        .collect();
    std::fs::write(store_path(dir), serde_json::to_vec_pretty(&kept)?)
}

/// What to add to a batch's failure message when the hook log in `log_dir`
/// shows a crash, recording it in `data_dir` when its cause is known. A log
/// last written before `since` (the batch's start) is an earlier session's:
/// the hook didn't run this time, so it says nothing about this crash.
pub fn explain_and_record(
    log_dir: &Path,
    data_dir: &Path,
    since: std::time::SystemTime,
    now_unix_secs: u64,
) -> Option<String> {
    let path = newest_hook_log(log_dir)?;
    if std::fs::metadata(&path).and_then(|m| m.modified()).ok()? < since {
        return None;
    }
    let log = std::fs::read(path).ok()?;
    let crash = last_session_crash(&String::from_utf8_lossy(&log))?;
    record(data_dir, &crash, now_unix_secs);
    Some(crate::messages::crash_explained(
        &crash.what,
        &crash.site,
        crash.map.as_deref(),
        known_cause(&crash.site),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    const LOG: &str = "\
[12:00:00.000] [dodstudio_goldsrc_hooks] ========== new session, 2026-10-02 ==========
[12:00:01.000] [demo     1.000] [dodstudio_goldsrc_hooks] level: maps/dod_old.bsp
[12:00:02.000] [demo     2.000] [dodstudio_goldsrc_hooks] CRASH: access violation (0xc0000005) at hw.dll+0x6c8d1 (0x1) -- reading 0x0
[13:00:00.000] [dodstudio_goldsrc_hooks] ========== new session, 2026-10-03 ==========
[13:00:01.000] [dodstudio_goldsrc_hooks] hull_trace_guard: PM_RecursiveHullCheck guarded against stale hulls (pre-Anniversary hw.dll, +0x6c830) (#384)
[13:00:02.000] [demo     2.000] [dodstudio_goldsrc_hooks] level: maps/dod_anzio.bsp
[13:00:03.000] [demo     3.000] [dodstudio_goldsrc_hooks] level: maps/dod_harrington.bsp
[13:00:04.000] [demo     4.000] [dodstudio_goldsrc_hooks] CRASH: stack overflow (0xc00000fd) at hw.dll+0x6c839 (0x3d06c839)
[13:00:04.001] [demo     4.000] [dodstudio_goldsrc_hooks] CRASH:   eip=0x3d06c839 eax=0x0
[13:00:04.002] [demo     4.000] [dodstudio_goldsrc_hooks] CRASH:   [esp+0x010] hw.dll+0x6c900
";

    #[test]
    fn reads_the_last_sessions_crash_and_the_map_it_was_on() {
        let crash = last_session_crash(LOG).unwrap();
        assert_eq!(
            crash,
            SessionCrash {
                site: "hw.dll+0x6c839".into(),
                what: "stack overflow (0xc00000fd)".into(),
                map: Some("dod_harrington".into()),
                build: Some("pre-Anniversary".into()),
            }
        );
        // A session with no crash says so, even after an earlier one that had.
        let clean = format!(
            "{LOG}[14:00:00.000] [dodstudio_goldsrc_hooks] ========== new session, x ==========\n"
        );
        assert_eq!(last_session_crash(&clean), None);
    }

    #[test]
    fn remembers_known_crashes_by_map_and_counts_repeats() {
        let dir = Scratch::new("crash_maps_record");
        let crash = last_session_crash(LOG).unwrap();
        let first = record(&dir, &crash, 100).unwrap();
        assert_eq!((first.map.as_str(), first.count), ("dod_harrington", 1));
        assert!(first.cause.contains("#384"));
        let again = record(&dir, &crash, 200).unwrap();
        assert_eq!((again.count, again.last_unix_secs), (2, 200));
        assert_eq!(load(&dir).len(), 1);
        assert_eq!(on_maps(&dir, &["DOD_HARRINGTON".to_string()]).len(), 1);
        assert!(on_maps(&dir, &["dod_anzio".to_string()]).is_empty());
        forget(&dir, "dod_harrington").unwrap();
        assert!(load(&dir).is_empty());
    }

    #[test]
    fn an_unknown_crash_or_one_with_no_map_is_not_remembered() {
        let dir = Scratch::new("crash_maps_unknown");
        let unknown = SessionCrash {
            site: "hw.dll+0x1".into(),
            what: "x".into(),
            map: Some("dod_x".into()),
            build: None,
        };
        assert_eq!(record(&dir, &unknown, 1), None);
        let nowhere = SessionCrash {
            site: "hw.dll+0x6c839".into(),
            what: "x".into(),
            map: None,
            build: None,
        };
        assert_eq!(record(&dir, &nowhere, 1), None);
        assert!(load(&dir).is_empty());
    }

    #[test]
    fn explains_the_crash_from_the_newest_log() {
        let dir = Scratch::new("crash_maps_explain");
        std::fs::write(dir.join("dodstudio_goldsrc_hooks_20261002.log"), "nothing").unwrap();
        std::fs::write(dir.join("dodstudio_goldsrc_hooks_20261003.log"), LOG).unwrap();
        let long_ago = std::time::UNIX_EPOCH;
        let text = explain_and_record(&dir, &dir, long_ago, 5).unwrap();
        assert!(text.contains("hw.dll+0x6c839"), "{text}");
        assert!(text.contains("dod_harrington"), "{text}");
        assert!(text.contains("#384"), "{text}");
        assert_eq!(load(&dir).len(), 1);
        // A log from before the batch started is another session's.
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
        assert_eq!(explain_and_record(&dir, &dir, later, 6), None);
    }

    /// The sites here are crash_report.py's; a site known to one and not the
    /// other is how the two drift apart.
    #[test]
    fn the_known_sites_match_crash_report_py() {
        let script = include_str!("../../goldsrc-hooks/tools/crash_report.py");
        let known = &script[script.find("KNOWN = {").unwrap()..];
        let known = &known[..known.find("\n}").unwrap()];
        let mut theirs: Vec<&str> = known
            .lines()
            .filter_map(|l| l.trim().strip_prefix('"'))
            .filter_map(|l| l.split('"').next())
            .filter(|s| s.contains(".dll+0x"))
            .collect();
        theirs.sort();
        let mut ours: Vec<&str> = KNOWN_CRASHES.iter().map(|(s, _)| *s).collect();
        ours.sort();
        assert_eq!(ours, theirs);
    }
}
