//! The analyzer cache every reader of a demo shares: Studio's Demo Analyzer,
//! its Master Queue scan and highlight scanner, and the in-game hook DLL's
//! Killstreaks tab (#565). One JSON file per demo,
//! `<root>/v<SCHEMA_VERSION>/<fnv1a of the canonical path>.json`, valid while
//! the demo's size and modified time match what it records. `<root>` is
//! `%APPDATA%\dod-studio\analyzer_cache`; callers pass it in, so this crate
//! needs no app-data lookup of its own.
//!
//! Studio and the DLL can be built from different commits. A schema bump puts
//! each in its own `v<N>` folder, so a mismatch costs a re-analysis, never a
//! misread.

use crate::Analysis;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Bump whenever `AnalyzerState`/`Player`/related computed fields change, so
/// caches written by an older schema are treated as a miss instead of
/// silently deserializing with new fields missing/defaulted.
pub const SCHEMA_VERSION: u32 = 4;

/// The demo file an analysis came from.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct FileInfo {
    /// The file's last modification time (the name is historical).
    pub created_at: SystemTime,
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
}

impl Default for FileInfo {
    fn default() -> Self {
        Self {
            created_at: SystemTime::UNIX_EPOCH,
            name: String::new(),
            path: String::new(),
            size_bytes: 0,
        }
    }
}

impl FileInfo {
    pub fn of(demo_path: &Path) -> Result<Self, String> {
        let metadata =
            std::fs::metadata(demo_path).map_err(|e| format!("Could not read metadata: {}", e))?;
        Ok(Self {
            created_at: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            name: demo_path
                .file_name()
                .and_then(|s| s.to_str())
                .map(String::from)
                .unwrap_or_default(),
            path: demo_path.to_str().map(String::from).unwrap_or_default(),
            size_bytes: metadata.len(),
        })
    }
}

/// A demo's size and modified time (whole seconds since the epoch): what an
/// entry stays valid for.
pub fn stamp(demo_path: &Path) -> Option<(u64, u64)> {
    let metadata = std::fs::metadata(demo_path).ok()?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    Some((metadata.len(), modified))
}

/// Where `demo_path`'s entry lives under `root`.
pub fn entry_path(root: &Path, demo_path: &Path) -> Option<PathBuf> {
    let canonical = std::fs::canonicalize(demo_path).ok()?;
    let hash = fnv1a(canonical.to_string_lossy().as_bytes());
    Some(
        root.join(format!("v{SCHEMA_VERSION}"))
            .join(format!("{hash:016x}.json")),
    )
}

#[derive(serde::Deserialize)]
struct Entry {
    size_bytes: u64,
    modified_unix_secs: u64,
    file_info: FileInfo,
    analysis: Analysis,
}

#[derive(serde::Serialize)]
struct EntryRef<'a> {
    size_bytes: u64,
    modified_unix_secs: u64,
    file_info: &'a FileInfo,
    analysis: &'a Analysis,
}

/// `demo_path`'s cached analysis, if there is one for the file as it is now.
pub fn load(root: &Path, demo_path: &Path) -> Option<(FileInfo, Analysis)> {
    let (size_bytes, modified_unix_secs) = stamp(demo_path)?;
    let bytes = std::fs::read(entry_path(root, demo_path)?).ok()?;
    let entry: Entry = serde_json::from_slice(&bytes).ok()?;
    (entry.size_bytes == size_bytes && entry.modified_unix_secs == modified_unix_secs)
        .then_some((entry.file_info, entry.analysis))
}

/// Saves `analysis` as `demo_path`'s entry, and returns where. Best-effort:
/// `None` on any failure, which must never fail the caller.
///
/// The file is written beside its final name and then renamed over it, so
/// Studio and the game writing the same demo at once, or one reading while
/// the other writes, never meet a half-written file.
pub fn store(
    root: &Path,
    demo_path: &Path,
    file_info: &FileInfo,
    analysis: &Analysis,
) -> Option<PathBuf> {
    let (size_bytes, modified_unix_secs) = stamp(demo_path)?;
    let path = entry_path(root, demo_path)?;
    std::fs::create_dir_all(path.parent()?).ok()?;
    let json = serde_json::to_vec(&EntryRef {
        size_bytes,
        modified_unix_secs,
        file_info,
        analysis,
    })
    .ok()?;
    let partial = path.with_extension(format!("json.{}.part", std::process::id()));
    std::fs::write(&partial, json).ok()?;
    if std::fs::rename(&partial, &path).is_err() {
        let _ = std::fs::remove_file(&partial);
        return None;
    }
    Some(path)
}

/// The same FNV-1a as `hl_demo_auditor::fnv1a_hash`, which named every entry
/// written before this module existed (`native` tests that the two agree).
fn fnv1a(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("analysis_cache_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_entry_is_read_back_while_the_demo_is_unchanged() {
        let dir = scratch("roundtrip");
        let demo = dir.join("x.dem");
        std::fs::write(&demo, b"not really a demo").unwrap();
        let root = dir.join("cache");
        let info = FileInfo::of(&demo).unwrap();
        let written = store(&root, &demo, &info, &Analysis::default()).unwrap();
        assert!(written.starts_with(root.join(format!("v{SCHEMA_VERSION}"))));
        let (read, _) = load(&root, &demo).unwrap();
        assert_eq!(read.size_bytes, 17);
        assert_eq!(read.name, "x.dem");
        // No partial file left beside it.
        assert_eq!(
            std::fs::read_dir(written.parent().unwrap())
                .unwrap()
                .count(),
            1
        );

        // A different size: the entry no longer describes the file.
        std::fs::write(&demo, b"a longer demo than before").unwrap();
        assert!(load(&root, &demo).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fnv1a_matches_the_published_test_vectors() {
        assert_eq!(fnv1a(b""), 0xcbf29ce484222325);
        assert_eq!(fnv1a(b"a"), 0xaf63dc4c8601ec8c);
    }
}
