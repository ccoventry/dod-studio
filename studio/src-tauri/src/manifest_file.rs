// manifest_file.rs
// The capture manifest on disk (#19): `%APPDATA%\dod-studio\manifests\<session_id>.json`.
//
// A diagnostic record, not crash recovery (that is #20's lockfile). The
// in-memory `CaptureManifest` is what correlates takes to blocks while the app
// runs; once the process is gone nothing of it was left behind, so an
// unmatched take or an ordering bug could not be looked into after the fact.
//
// Written twice per batch: once when the batch is planned (`outcome:
// "planned"`, no verdicts), and again when it ends (`"complete"` or
// `"cancelled"`, with each block's verification verdict). A batch that dies in
// between leaves the planned file, which is itself the useful signal.
//
// Best-effort throughout: a failed write is logged and never fails a batch.

use crate::capture_manager::{CaptureManifest, VerifiedBlock};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Manifests kept; older ones are deleted when a new one is written.
const KEEP_MANIFESTS: usize = 50;

#[derive(Serialize)]
struct ManifestFile<'a> {
    session_id: &'a str,
    /// "planned", "complete" or "cancelled".
    outcome: &'a str,
    /// Seconds since the Unix epoch.
    written_at_unix: u64,
    capture_fps: i32,
    blocks: &'a [native::patch::CaptureBlock],
    /// Present once the batch has ended.
    #[serde(skip_serializing_if = "Option::is_none")]
    verified: Option<&'a [VerifiedBlock]>,
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn manifests_dir() -> PathBuf {
    native::shared::paths::get_appdata_dir().join("manifests")
}

/// `<session_id>.json`, with anything but ASCII letters, digits, `-` and `_`
/// replaced, since the id comes from the frontend.
fn file_name(session_id: &str) -> String {
    let safe: String = session_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if safe.is_empty() {
        format!("session_{}.json", unix_now())
    } else {
        format!("{safe}.json")
    }
}

/// Writes (or rewrites) this batch's manifest file. Logs and returns on error.
pub fn write(manifest: &CaptureManifest, outcome: &str, verified: Option<&[VerifiedBlock]>) {
    write_in(&manifests_dir(), manifest, outcome, verified);
}

fn write_in(
    dir: &Path,
    manifest: &CaptureManifest,
    outcome: &str,
    verified: Option<&[VerifiedBlock]>,
) -> Option<PathBuf> {
    let body = ManifestFile {
        session_id: &manifest.session_id,
        outcome,
        written_at_unix: unix_now(),
        capture_fps: manifest.capture_fps,
        blocks: &manifest.blocks,
        verified,
    };
    let result = (|| -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(file_name(&manifest.session_id));
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_vec_pretty(&body).map_err(std::io::Error::other)?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &path)?;
        Ok(path)
    })();
    match result {
        Ok(path) => {
            prune(dir, KEEP_MANIFESTS);
            Some(path)
        }
        Err(e) => {
            log::warn!(
                "Could not write the capture manifest to {}: {}",
                dir.display(),
                e
            );
            None
        }
    }
}

/// Deletes all but the `keep` most recently modified `.json` files in `dir`.
fn prune(dir: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    if files.len() <= keep {
        return;
    }
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    for (_, path) in files.into_iter().skip(keep) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    fn manifest(session_id: &str) -> CaptureManifest {
        CaptureManifest {
            session_id: session_id.to_string(),
            blocks: Vec::new(),
            capture_fps: 300,
            capture_mode: String::new(),
            agr_fps: 0,
        }
    }

    #[test]
    fn the_session_id_becomes_a_safe_file_name() {
        assert_eq!(
            file_name("session_20260928_010203"),
            "session_20260928_010203.json"
        );
        assert_eq!(file_name("../evil id"), "___evil_id.json");
        assert!(file_name("").starts_with("session_"));
    }

    #[test]
    fn a_manifest_is_written_then_rewritten_with_its_outcome() {
        let dir = Scratch::new("manifest_file_write");
        let m = manifest("session_a");

        let path = write_in(&dir, &m, "planned", None).expect("written");
        let planned: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(planned["outcome"], "planned");
        assert_eq!(planned["capture_fps"], 300);
        assert!(planned.get("verified").is_none());

        write_in(&dir, &m, "complete", Some(&[])).expect("rewritten");
        let done: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(done["outcome"], "complete");
        assert!(done["verified"].is_array());
    }

    #[test]
    fn only_the_newest_manifests_are_kept() {
        let dir = Scratch::new("manifest_file_prune");
        for i in 0..4 {
            std::fs::write(dir.path().join(format!("s{i}.json")), "{}").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        std::fs::write(dir.path().join("notes.txt"), "kept").unwrap();
        prune(&dir, 2);

        let mut left: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, vec!["notes.txt", "s2.json", "s3.json"]);
    }
}
