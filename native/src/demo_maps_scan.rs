//! Which demos in a folder recorded more than one map (#624), for the Demo
//! Auditor's Split tab.
//!
//! Three sources, cheapest first:
//!
//! 1. **This scan's own cache** (`demo_maps.json` in the app data folder):
//!    a demo's maps, valid while its size and modified time are unchanged.
//! 2. **The analyzer cache**: an entry written since `signon_maps` existed
//!    lists every signon's map. Only that one key is read out of the entry,
//!    not the whole analysis.
//! 3. **The demo itself**, byte-scanned for `SvcServerInfo`
//!    (`demo_split::quick_maps`), about 30 ms for an 85 MB demo once read.
//!
//! What 2 and 3 find goes into 1.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::demo_split::{header_protocol, quick_maps};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DemoMaps {
    pub path: String,
    pub size_bytes: u64,
    /// One per signon, in order.
    pub maps: Vec<String>,
    /// Where the answer came from: "cache", "analyzer" or "scan".
    pub source: String,
    /// For each map, the file a split already wrote for it next to the
    /// demo, if there is one (the name Split Maps gives it).
    #[serde(default)]
    pub split_files: Vec<Option<String>>,
}

/// For each of `maps`, the piece a split already wrote next to `demo`, by
/// the name Split Maps gives it (`<demo>_split<N>_<map>.dem`) or the one it
/// used before.
pub fn split_files(demo: &Path, maps: &[String]) -> Vec<Option<String>> {
    let (Some(dir), Some(stem)) = (demo.parent(), demo.file_stem()) else {
        return vec![None; maps.len()];
    };
    let stem = stem.to_string_lossy();
    let stems: Vec<String> = maps
        .iter()
        .map(|m| crate::demo_split::map_stem(m))
        .collect();
    let stems: Vec<&str> = stems.iter().map(String::as_str).collect();
    // Today's name first, then the one splits used before `_split<N>`.
    (0..maps.len())
        .map(|i| {
            [
                crate::demo_split::split_name_at(&stem, &stems, i),
                crate::demo_split::old_split_name_at(&stem, &stems, i),
            ]
            .into_iter()
            .map(|n| format!("{n}.dem"))
            .find(|name| dir.join(name).is_file())
        })
        .collect()
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
struct Remembered {
    size_bytes: u64,
    modified: u64,
    maps: Vec<String>,
}

fn cache_file() -> PathBuf {
    crate::shared::paths::get_appdata_dir().join("demo_maps.json")
}

fn load_remembered(path: &Path) -> HashMap<String, Remembered> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_remembered(path: &Path, map: &HashMap<String, Remembered>) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_vec(map) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

/// `"signon_maps":[...]` out of an analyzer cache entry's text, or `None`
/// when the entry predates the key.
pub fn signon_maps_in_entry(json: &[u8]) -> Option<Vec<String>> {
    const KEY: &[u8] = b"\"signon_maps\":";
    let at = json.windows(KEY.len()).position(|w| w == KEY)? + KEY.len();
    let rest = &json[at..];
    let end = rest.iter().position(|&b| b == b']')? + 1;
    let maps: Vec<String> = serde_json::from_slice(&rest[..end]).ok()?;
    (!maps.is_empty()).then_some(maps)
}

fn from_analyzer_cache(root: &Path, demo: &Path) -> Option<Vec<String>> {
    let (size, modified) = analysis::cache::stamp(demo)?;
    let entry = std::fs::read(analysis::cache::entry_path(root, demo)?).ok()?;
    // The stamp is the entry's first two keys.
    let head = String::from_utf8_lossy(&entry[..entry.len().min(200)]).to_string();
    let valid = head.contains(&format!("\"size_bytes\":{size},"))
        && head.contains(&format!("\"modified_unix_secs\":{modified},"));
    if !valid {
        return None;
    }
    signon_maps_in_entry(&entry)
}

fn demos_in(folder: &Path, recursive: bool, out: &mut Vec<PathBuf>) {
    let walker = crate::sys::folder_walk::Walker::new().skip_system_folders();
    let walker = if recursive {
        walker
    } else {
        walker.max_depth(1)
    };
    out.extend(walker.demos(folder));
}

/// Every demo under `folder` with more than one map. `progress(done, total,
/// demo)` is called before each demo; `cancel` stops between demos.
pub fn find_multi_map_demos(
    folder: &Path,
    recursive: bool,
    analyzer_cache_root: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize, usize, &Path),
) -> Vec<DemoMaps> {
    let mut demos = Vec::new();
    demos_in(folder, recursive, &mut demos);
    demos.sort();
    let cache_path = cache_file();
    let mut remembered = load_remembered(&cache_path);
    let mut found = Vec::new();
    let mut changed = false;
    for (i, demo) in demos.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        progress(i, demos.len(), demo);
        let Some((size, modified)) = analysis::cache::stamp(demo) else {
            continue;
        };
        let key = demo.to_string_lossy().to_string();
        let (maps, source) = match remembered.get(&key) {
            Some(r) if r.size_bytes == size && r.modified == modified => (r.maps.clone(), "cache"),
            _ => {
                let (maps, source) = match from_analyzer_cache(analyzer_cache_root, demo) {
                    Some(m) => (m, "analyzer"),
                    None => {
                        let Ok(bytes) = std::fs::read(demo) else {
                            continue;
                        };
                        (
                            quick_maps(&bytes, header_protocol(&bytes).unwrap_or(48)),
                            "scan",
                        )
                    }
                };
                remembered.insert(
                    key.clone(),
                    Remembered {
                        size_bytes: size,
                        modified,
                        maps: maps.clone(),
                    },
                );
                changed = true;
                (maps, source)
            }
        };
        if maps.len() > 1 {
            found.push(DemoMaps {
                split_files: split_files(demo, &maps),
                path: key,
                size_bytes: size,
                maps,
                source: source.to_string(),
            });
        }
    }
    progress(demos.len(), demos.len(), folder);
    if changed {
        save_remembered(&cache_path, &remembered);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_piece_already_written_next_to_the_demo_is_found() {
        let dir = std::env::temp_dir().join(format!("demo_maps_split_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let demo = dir.join("m.dem");
        std::fs::write(&demo, b"x").unwrap();
        std::fs::write(dir.join("m_dod_anzio_1.dem"), b"x").unwrap();
        let maps = vec!["dod_anzio".to_string(), "maps/dod_anzio.bsp".to_string()];
        assert_eq!(
            split_files(&demo, &maps),
            vec![Some("m_dod_anzio_1.dem".to_string()), None]
        );
        // A map that doesn't repeat has no number.
        std::fs::write(dir.join("m_dod_flash.dem"), b"x").unwrap();
        let two = vec!["dod_anzio".to_string(), "dod_flash".to_string()];
        assert_eq!(
            split_files(&demo, &two),
            vec![None, Some("m_dod_flash.dem".to_string())]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_only_the_signon_maps_key() {
        let entry = br#"{"size_bytes":5,"modified_unix_secs":6,"file_info":{},"analysis":{"state":{"map_changed":true,"signon_maps":["dod_anzio","dod_flash"],"x":[1,2]}}}"#;
        assert_eq!(
            signon_maps_in_entry(entry),
            Some(vec!["dod_anzio".to_string(), "dod_flash".to_string()])
        );
    }

    #[test]
    fn an_older_entry_has_no_answer() {
        assert_eq!(signon_maps_in_entry(br#"{"analysis":{"state":{}}}"#), None);
        assert_eq!(signon_maps_in_entry(br#"{"signon_maps":[]}"#), None);
    }
}
