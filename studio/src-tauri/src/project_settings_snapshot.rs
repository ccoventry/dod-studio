// project_settings_snapshot.rs
// The one snapshot of Configuration that every project made before #1085
// starts from (build plan decision 6): taken the first time a build with
// project settings runs, then never changed, so an older project opened later
// gets the same settings however Configuration has changed since, and the
// order projects are opened in doesn't matter.
//
// The frontend builds the snapshot (project_settings_snapshot.js knows the
// settings block); this file only keeps it, once. It is its own file beside
// settings.json rather than a settings.json field for two reasons:
// - a build from before this one rewrites settings.json with only the fields
//   it knows (serde drops the rest), so a field would be lost the first time
//   the user ran an older build, and the next start would take a new snapshot
//   from whatever Configuration held then, which is the mix-up this prevents;
// - an installed Studio and a dev one can start at the same moment. The file
//   is linked into place only if none exists yet (`fs::hard_link` refuses to
//   replace), so whichever is first wins and the other reads that one back.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use serde::Serialize;
use serde_json::Value;

/// The snapshot's file name, in the app's settings folder.
const FILE_NAME: &str = "project_settings_snapshot.json";
/// Where an unreadable snapshot is moved before a new one is taken, so the
/// user can still look at it.
const DAMAGED_NAME: &str = "project_settings_snapshot.damaged.json";

/// What `keep_project_settings_snapshot` answers: the snapshot in effect, and
/// whether this call is the one that took it.
#[derive(Debug, Serialize)]
pub struct KeptSnapshot {
    pub snapshot: Value,
    pub taken: bool,
}

/// A snapshot as the frontend writes it: an object with a `settings` object.
fn is_usable(value: &Value) -> bool {
    value.get("settings").is_some_and(Value::is_object)
}

/// The snapshot at `path`: `Ok(None)` when there is no file, `Err` holding
/// the reason when there is one this can't use.
fn read_snapshot(path: &Path) -> Result<Option<Value>, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if is_usable(&value) {
        Ok(Some(value))
    } else {
        Err(crate::messages::settings::SNAPSHOT_NOT_A_SNAPSHOT.to_string())
    }
}

/// Puts `json` at `path` only if nothing is there yet: written in full to a
/// file of this call's own first, then hard-linked into place, so another
/// Studio never reads half a snapshot. `Ok(false)` when one was already there.
fn link_into_place(path: &Path, json: &str) -> Result<bool, String> {
    // Unique to this call: the process id keeps two Studios apart, the
    // counter two calls in one.
    static CALLS: AtomicU64 = AtomicU64::new(0);
    let call = CALLS.fetch_add(1, Ordering::Relaxed);
    let temp = path.with_extension(format!("{}-{call}.tmp", std::process::id()));
    fs::write(&temp, json)
        .map_err(|e| crate::messages::settings::snapshot_not_written(&temp, e))?;
    let linked = fs::hard_link(&temp, path);
    let _ = fs::remove_file(&temp);
    match linked {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(crate::messages::settings::snapshot_not_written(path, e)),
    }
}

/// Keeps the snapshot in `dir`: the one already there when there is one
/// (`candidate` is then dropped), else `candidate`, stored for good.
/// A file that can't be read as a snapshot is moved aside to
/// `project_settings_snapshot.damaged.json` and `candidate` replaces it.
pub fn keep_snapshot_in(dir: &Path, candidate: Value) -> Result<KeptSnapshot, String> {
    if !is_usable(&candidate) {
        return Err(crate::messages::settings::SNAPSHOT_NOT_A_SNAPSHOT.to_string());
    }
    let path = dir.join(FILE_NAME);
    match read_snapshot(&path) {
        Ok(Some(snapshot)) => {
            return Ok(KeptSnapshot {
                snapshot,
                taken: false,
            });
        }
        Ok(None) => {}
        Err(reason) => {
            let damaged = dir.join(DAMAGED_NAME);
            fs::rename(&path, &damaged)
                .map_err(|e| crate::messages::settings::snapshot_not_written(&path, e))?;
            native::log_markdown(&crate::messages::settings::snapshot_damaged(
                &path, &damaged, reason,
            ));
        }
    }
    fs::create_dir_all(dir).map_err(|e| crate::messages::settings::snapshot_not_written(dir, e))?;
    let json = serde_json::to_string_pretty(&candidate)
        .map_err(|e| crate::messages::settings::snapshot_not_written(&path, e))?;
    if link_into_place(&path, &json)? {
        return Ok(KeptSnapshot {
            snapshot: candidate,
            taken: true,
        });
    }
    // Another Studio took it between the read above and the link: theirs
    // stands, as if it had been there all along.
    match read_snapshot(&path) {
        Ok(Some(snapshot)) => Ok(KeptSnapshot {
            snapshot,
            taken: false,
        }),
        Ok(None) => Err(crate::messages::settings::snapshot_not_written(
            &path,
            crate::messages::settings::SNAPSHOT_NOT_A_SNAPSHOT,
        )),
        Err(reason) => Err(crate::messages::settings::snapshot_not_written(
            &path, reason,
        )),
    }
}

/// The app's settings folder, where settings.json is.
fn settings_dir() -> PathBuf {
    crate::settings_manager::settings_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

/// The frontend's one call, at every start: keeps `snapshot` if none was
/// taken before, and answers with the one in effect. Logs the take, once.
#[tauri::command(async)]
pub async fn keep_project_settings_snapshot(snapshot: Value) -> Result<KeptSnapshot, String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let started = Instant::now();
        let dir = settings_dir();
        let kept = keep_snapshot_in(&dir, snapshot)?;
        if kept.taken {
            let taken_at = kept.snapshot["takenAt"].as_str().unwrap_or_default();
            native::log_markdown(&crate::messages::settings::snapshot_taken(
                &dir.join(FILE_NAME),
                taken_at,
                started.elapsed().as_millis(),
            ));
        }
        Ok(kept)
    }))
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;
    use serde_json::json;

    fn snapshot(width: u32) -> Value {
        json!({ "schema": 3, "takenAt": "2026-11-11T01:00:00.000Z", "appVersion": "0.12.0",
                "settings": { "picture": { "width": width } }, "folders": {} })
    }

    /// The first call stores its snapshot; every later one gets that one
    /// back, whatever it offers.
    #[test]
    fn the_first_snapshot_stands() {
        let dir = Scratch::new("snapshot_first");
        let first = keep_snapshot_in(&dir, snapshot(1920)).expect("first keep");
        assert!(first.taken);
        assert_eq!(first.snapshot["settings"]["picture"]["width"], 1920);
        let later = keep_snapshot_in(&dir, snapshot(2560)).expect("later keep");
        assert!(!later.taken);
        assert_eq!(later.snapshot["settings"]["picture"]["width"], 1920);
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .expect("read dir")
            .map(|e| e.expect("entry").file_name())
            .collect();
        assert_eq!(leftovers, vec![std::ffi::OsString::from(FILE_NAME)]);
    }

    /// Two Studios starting at once: one snapshot wins, and both get it.
    #[test]
    fn two_at_once_agree_on_one_snapshot() {
        let dir = Scratch::new("snapshot_race");
        let path = dir.path().to_path_buf();
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || keep_snapshot_in(&path, snapshot(1000 + i)))
            })
            .collect();
        let kept: Vec<KeptSnapshot> = threads
            .into_iter()
            .map(|t| t.join().expect("thread").expect("keep"))
            .collect();
        assert_eq!(kept.iter().filter(|k| k.taken).count(), 1);
        let widths: Vec<_> = kept
            .iter()
            .map(|k| k.snapshot["settings"]["picture"]["width"].clone())
            .collect();
        assert!(widths.iter().all(|w| *w == widths[0]), "{widths:?}");
    }

    /// An unreadable file is moved aside, not lost, and a new snapshot taken.
    #[test]
    fn a_damaged_snapshot_is_kept_aside_and_retaken() {
        let dir = Scratch::new("snapshot_damaged");
        fs::write(dir.path().join(FILE_NAME), "{ not json").expect("write");
        let kept = keep_snapshot_in(&dir, snapshot(1280)).expect("keep");
        assert!(kept.taken);
        assert_eq!(
            fs::read_to_string(dir.path().join(DAMAGED_NAME)).expect("damaged copy"),
            "{ not json"
        );
        let again = keep_snapshot_in(&dir, snapshot(640)).expect("keep again");
        assert_eq!(again.snapshot["settings"]["picture"]["width"], 1280);
    }

    /// Something that isn't a snapshot is refused, and nothing is written.
    #[test]
    fn a_candidate_without_settings_is_refused() {
        let dir = Scratch::new("snapshot_refused");
        assert!(keep_snapshot_in(&dir, json!({ "takenAt": "x" })).is_err());
        assert!(!dir.path().join(FILE_NAME).exists());
    }
}
