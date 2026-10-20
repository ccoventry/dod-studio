//! The review library (#848): every highlight's Review mark, note and Kill
//! Range, kept in the app data folder (`review_library.json`) whether or not
//! a project is open, so a review with no project isn't lost and a project
//! that adds the demo later can bring the marks in.
//!
//! Keyed on the demo's contents (`source_key`, `demo_hasher::key_text`: its
//! size and head hash), not its path, so a moved, renamed or copied demo
//! still matches. Each highlight entry is the frontend's (`review_library.js`):
//! this module stores it as it comes and never reads inside it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const FILE: &str = "review_library.json";
const VERSION: u32 = 1;

/// One writer at a time: each update reads, changes and rewrites the file.
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Library {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub demos: BTreeMap<String, LibraryDemo>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryDemo {
    /// The demo's file name when last marked, for the person reading the file.
    #[serde(default)]
    pub name: String,
    /// Highlight id (player slot and first/last kill tick) -> its entry.
    #[serde(default)]
    pub highlights: BTreeMap<String, Value>,
}

fn library_path() -> PathBuf {
    native::shared::paths::get_appdata_dir().join(FILE)
}

/// Reads the library; a missing file is an empty one. An unreadable one is
/// an error, never treated as empty, so the next write can't wipe it.
fn read(path: &Path) -> Result<Library, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| crate::messages::review_library_unreadable(path, e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Library::default()),
        Err(e) => Err(crate::messages::review_library_unreadable(path, e)),
    }
}

/// Writes through a temporary file, so a crash mid-write leaves the old one.
fn write(path: &Path, library: &Library) -> Result<(), String> {
    let text = serde_json::to_vec_pretty(library)
        .map_err(|e| crate::messages::review_library_unwritable(path, e))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| crate::messages::review_library_unwritable(path, e))?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| crate::messages::review_library_unwritable(&tmp, e))?;
    std::fs::rename(&tmp, path).map_err(|e| crate::messages::review_library_unwritable(path, e))
}

/// The entries of the demos in `keys` that have any.
fn entries_for(library: &Library, keys: &[String]) -> BTreeMap<String, LibraryDemo> {
    keys.iter()
        .filter_map(|k| library.demos.get(k).map(|d| (k.clone(), d.clone())))
        .collect()
}

/// Puts each changed highlight in the demo's entry (`None` removes it, a
/// highlight whose work was cleared), and drops a demo left with none.
fn apply_update(
    library: &mut Library,
    key: &str,
    name: &str,
    changes: BTreeMap<String, Option<Value>>,
) {
    library.version = VERSION;
    let demo = library.demos.entry(key.to_string()).or_default();
    if !name.is_empty() {
        demo.name = name.to_string();
    }
    for (id, entry) in changes {
        match entry {
            Some(entry) => {
                demo.highlights.insert(id, entry);
            }
            None => {
                demo.highlights.remove(&id);
            }
        }
    }
    if demo.highlights.is_empty() {
        library.demos.remove(key);
    }
}

/// The saved marks of the demos whose `source_key` is in `keys`.
#[tauri::command]
pub async fn review_library_get(
    keys: Vec<String>,
) -> Result<BTreeMap<String, LibraryDemo>, String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        Ok(entries_for(&read(&library_path())?, &keys))
    }))
    .await
}

/// Saves one demo's changed highlights (see [`apply_update`]).
#[tauri::command]
pub async fn review_library_update(
    key: String,
    name: String,
    changes: BTreeMap<String, Option<Value>>,
) -> Result<(), String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = library_path();
        let mut library = read(&path)?;
        apply_update(&mut library, &key, &name, changes);
        write(&path, &library)
    }))
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;
    use serde_json::json;

    fn changes(list: &[(&str, Option<Value>)]) -> BTreeMap<String, Option<Value>> {
        list.iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn an_update_adds_replaces_and_removes_highlights() {
        let mut lib = Library::default();
        apply_update(
            &mut lib,
            "k1",
            "a.dem",
            changes(&[
                ("0#1#2", Some(json!({"curation": "Keep"}))),
                ("0#5#9", Some(json!({"curation": "Skip"}))),
            ]),
        );
        apply_update(
            &mut lib,
            "k1",
            "",
            changes(&[
                ("0#1#2", Some(json!({"curation": "Maybe"}))),
                ("0#5#9", None),
            ]),
        );
        let demo = &lib.demos["k1"];
        assert_eq!(demo.name, "a.dem", "an empty name keeps the old one");
        assert_eq!(demo.highlights.len(), 1);
        assert_eq!(demo.highlights["0#1#2"], json!({"curation": "Maybe"}));
        assert_eq!(lib.version, VERSION);
    }

    #[test]
    fn a_demo_left_with_no_highlights_is_dropped() {
        let mut lib = Library::default();
        apply_update(&mut lib, "k1", "a.dem", changes(&[("h", Some(json!({})))]));
        apply_update(&mut lib, "k1", "a.dem", changes(&[("h", None)]));
        assert!(lib.demos.is_empty());
    }

    #[test]
    fn get_returns_only_the_asked_demos_that_have_marks() {
        let mut lib = Library::default();
        apply_update(&mut lib, "k1", "a.dem", changes(&[("h", Some(json!(1)))]));
        apply_update(&mut lib, "k2", "b.dem", changes(&[("h", Some(json!(2)))]));
        let got = entries_for(&lib, &["k2".to_string(), "missing".to_string()]);
        assert_eq!(got.keys().collect::<Vec<_>>(), ["k2"]);
    }

    #[test]
    fn the_file_round_trips_and_a_missing_one_is_empty() {
        let dir = Scratch::new("review_library_round_trip");
        let path = dir.path().join(FILE);
        assert!(read(&path).unwrap().demos.is_empty());
        let mut lib = Library::default();
        apply_update(
            &mut lib,
            "k1",
            "a.dem",
            changes(&[("h", Some(json!({"notes": "x"})))]),
        );
        write(&path, &lib).unwrap();
        let back = read(&path).unwrap();
        assert_eq!(back.demos["k1"], lib.demos["k1"]);
        assert!(!path.with_extension("json.tmp").exists());
    }

    /// A damaged file is reported, not read as empty: the next save would
    /// otherwise replace every mark in it with one.
    #[test]
    fn a_damaged_file_is_an_error() {
        let dir = Scratch::new("review_library_damaged");
        let path = dir.path().join(FILE);
        std::fs::write(&path, b"{ not json").unwrap();
        let err = read(&path).unwrap_err();
        assert!(err.contains(FILE), "{err}");
    }
}
