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
    let Ok(read) = std::fs::read_dir(folder) else {
        return;
    };
    let mut dirs = Vec::new();
    for e in read.flatten() {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => dirs.push(p),
            Ok(t)
                if t.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dem")) =>
            {
                out.push(p);
            }
            _ => {}
        }
    }
    if recursive {
        for d in dirs {
            demos_in(&d, true, out);
        }
    }
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
