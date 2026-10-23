//! What the Duplicates audit remembers about each demo between scans (#888):
//! its key and tail hash, and its whole-file hash once the delete's
//! byte-for-byte check has read it. Keyed by the demo's canonical path and
//! valid while its size and modified time are unchanged, the way the
//! analyzer cache is, so a second scan opens only new or changed demos.
//!
//! With "Look inside archives" (#929) it also remembers each archive's
//! (`.zip`, `.7z`, `.rar`) demo entries and their hashes, under the
//! archive's path, size and modified time, so an unchanged one is not
//! opened again.
//!
//! Studio keeps it in `audit_keys.json` in the app data folder. It is only a
//! cache: a file that is missing, unreadable or from another version starts
//! it empty, and a failed save loses nothing but time.

use crate::{ContentHashes, Stamp, ZipDemoEntry};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

/// Bumped when [`KeyEntry`] changes meaning; an older file is dropped.
const VERSION: u32 = 1;

/// One demo as the audit last saw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KeyEntry {
    pub size: u64,
    pub modified_ns: u64,
    /// [`crate::FileKey::header_hash`].
    pub head: u64,
    /// [`crate::tail_hash`], once a group needed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tail: Option<u64>,
    /// FNV-1a over the whole file, once something read all of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full: Option<u64>,
}

impl KeyEntry {
    fn holds_for(&self, stamp: Stamp) -> bool {
        stamp.modified_ns != 0 && self.size == stamp.size && self.modified_ns == stamp.modified_ns
    }
}

/// One zip as the audit last saw it: its `.dem` entries, or why it could
/// not be read.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ArchiveEntry {
    pub size: u64,
    pub modified_ns: u64,
    #[serde(default)]
    pub demos: Vec<ZipDemoEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ArchiveEntry {
    fn holds_for(&self, stamp: Stamp) -> bool {
        stamp.modified_ns != 0 && self.size == stamp.size && self.modified_ns == stamp.modified_ns
    }
}

/// `archives` sits beside `entries` without a version bump: an older Studio
/// ignores it, and a file without it reads as no zips remembered.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct CacheFile {
    version: u32,
    entries: HashMap<String, KeyEntry>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    archives: HashMap<String, ArchiveEntry>,
}

/// The remembered entries, plus what this run changed: [`KeyCache::save`]
/// writes only those changes over the file as it is then, so a delete that
/// saved while a scan ran keeps what it remembered.
#[derive(Debug, Default)]
pub struct KeyCache {
    entries: HashMap<String, KeyEntry>,
    changes: HashMap<String, Option<KeyEntry>>,
    archives: HashMap<String, ArchiveEntry>,
    archive_changes: HashMap<String, Option<ArchiveEntry>>,
    seen: HashSet<String>,
}

/// One save at a time in this process; the file is replaced whole.
static SAVE_LOCK: Mutex<()> = Mutex::new(());

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn read_file(file: &Path) -> CacheFile {
    std::fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<CacheFile>(&bytes).ok())
        .filter(|cache| cache.version == VERSION)
        .unwrap_or_default()
}

impl KeyCache {
    /// The cache saved at `file`, or an empty one.
    pub fn load(file: &Path) -> Self {
        let saved = read_file(file);
        Self {
            entries: saved.entries,
            archives: saved.archives,
            ..Self::default()
        }
    }

    /// What is remembered about the zip at `path`, if it still holds for
    /// `stamp`.
    pub fn archive(&self, path: &Path, stamp: Stamp) -> Option<&ArchiveEntry> {
        self.archives
            .get(&text(path))
            .filter(|e| e.holds_for(stamp))
    }

    /// A fresh listing of the zip at `path`: its demos, or why it can't be
    /// read. Nothing is remembered for a zip with no modified time.
    pub fn put_archive(
        &mut self,
        path: &Path,
        stamp: Stamp,
        listing: Result<Vec<ZipDemoEntry>, String>,
    ) {
        if stamp.modified_ns == 0 {
            return;
        }
        let (demos, error) = match listing {
            Ok(demos) => (demos, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        self.set_archive(
            text(path),
            ArchiveEntry {
                size: stamp.size,
                modified_ns: stamp.modified_ns,
                demos,
                error,
            },
        );
    }

    /// What reading demo `name` of the zip at `path` in full found, beside a
    /// listing that still holds.
    pub fn set_archive_hashes(
        &mut self,
        path: &Path,
        stamp: Stamp,
        name: &str,
        read: Result<ContentHashes, String>,
    ) {
        let Some(mut entry) = self.archive(path, stamp).cloned() else {
            return;
        };
        let Some(demo) = entry.demos.iter_mut().find(|d| d.name == name) else {
            return;
        };
        match read {
            Ok(hashes) => demo.hashes = Some(hashes),
            Err(e) => demo.error = Some(e),
        }
        self.set_archive(text(path), entry);
    }

    fn set_archive(&mut self, key: String, entry: ArchiveEntry) {
        if self.archives.get(&key) != Some(&entry) {
            self.archives.insert(key.clone(), entry.clone());
            self.archive_changes.insert(key, Some(entry));
        }
    }

    /// What is remembered about `path`, if it still holds for `stamp`.
    pub fn entry(&self, path: &Path, stamp: Stamp) -> Option<KeyEntry> {
        self.entries
            .get(&text(path))
            .filter(|e| e.holds_for(stamp))
            .copied()
    }

    fn set(&mut self, key: String, entry: KeyEntry) {
        if self.entries.get(&key) != Some(&entry) {
            self.entries.insert(key.clone(), entry);
            self.changes.insert(key, Some(entry));
        }
    }

    /// A fresh key for `path`: what was remembered under an older stamp goes.
    pub fn put(&mut self, path: &Path, stamp: Stamp, head: u64) {
        if stamp.modified_ns == 0 {
            return;
        }
        self.set(
            text(path),
            KeyEntry {
                size: stamp.size,
                modified_ns: stamp.modified_ns,
                head,
                tail: None,
                full: None,
            },
        );
    }

    /// `path`'s tail hash, beside a key that still holds.
    pub fn set_tail(&mut self, path: &Path, stamp: Stamp, tail: u64) {
        if let Some(entry) = self.entry(path, stamp) {
            self.set(
                text(path),
                KeyEntry {
                    tail: Some(tail),
                    ..entry
                },
            );
        }
    }

    /// Everything a read of all of `path` found, stamped as it was read.
    /// `path` is its canonical form, as the key pass names files.
    pub fn remember_contents(&mut self, path: &Path, stamp: Stamp, hashes: ContentHashes) {
        if stamp.modified_ns == 0 || stamp.size != hashes.size {
            return;
        }
        self.set(
            text(path),
            KeyEntry {
                size: stamp.size,
                modified_ns: stamp.modified_ns,
                head: hashes.head,
                tail: Some(hashes.tail),
                full: Some(hashes.full),
            },
        );
    }

    /// Drops `path`, e.g. once it has been deleted.
    pub fn forget(&mut self, path: &Path) {
        let key = text(path);
        self.entries.remove(&key);
        self.changes.insert(key, None);
    }

    /// Notes that this run's walk found `path`.
    pub fn mark_seen(&mut self, path: &Path) {
        self.seen.insert(text(path));
    }

    /// Drops every entry under one of `roots` that this run's walk did not
    /// find: it was deleted, moved or renamed. Call only after a walk that
    /// finished. Zips are dropped only when the walk `listed_zips` (#929):
    /// a scan without "Look inside archives" keeps what the last one with
    /// it learned.
    pub fn prune_unseen_under(&mut self, roots: &[PathBuf], listed_zips: bool) {
        let roots: Vec<PathBuf> = roots
            .iter()
            .filter_map(|r| std::fs::canonicalize(r).ok())
            .collect();
        let gone = |keys: &mut dyn Iterator<Item = &String>| -> Vec<String> {
            keys.filter(|k| !self.seen.contains(*k))
                .filter(|k| roots.iter().any(|r| Path::new(k).starts_with(r)))
                .cloned()
                .collect()
        };
        let gone_demos = gone(&mut self.entries.keys());
        let gone_zips = if listed_zips {
            gone(&mut self.archives.keys())
        } else {
            Vec::new()
        };
        for key in gone_demos {
            self.entries.remove(&key);
            self.changes.insert(key, None);
        }
        for key in gone_zips {
            self.archives.remove(&key);
            self.archive_changes.insert(key, None);
        }
    }

    pub fn has_changes(&self) -> bool {
        !self.changes.is_empty() || !self.archive_changes.is_empty()
    }

    /// Writes this run's changes over `file` as it is now. A hash someone
    /// else saved for the same unchanged file is kept when this run did not
    /// learn it.
    pub fn save(&mut self, file: &Path) -> std::io::Result<()> {
        if !self.has_changes() {
            return Ok(());
        }
        let _one_at_a_time = SAVE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        let saved = read_file(file);
        let mut entries = saved.entries;
        let mut archives = saved.archives;
        for (key, change) in self.archive_changes.drain() {
            match change {
                Some(mut entry) => {
                    if let Some(old) = archives.get(&key)
                        && old.size == entry.size
                        && old.modified_ns == entry.modified_ns
                    {
                        // What another run read in full and this one didn't.
                        for demo in &mut entry.demos {
                            if demo.hashes.is_some() || demo.error.is_some() {
                                continue;
                            }
                            if let Some(was) = old.demos.iter().find(|d| d.name == demo.name) {
                                demo.hashes = was.hashes;
                                demo.error = was.error.clone();
                            }
                        }
                    }
                    archives.insert(key, entry);
                }
                None => {
                    archives.remove(&key);
                }
            }
        }
        for (key, change) in self.changes.drain() {
            match change {
                Some(mut entry) => {
                    if let Some(old) = entries.get(&key)
                        && old.size == entry.size
                        && old.modified_ns == entry.modified_ns
                    {
                        entry.tail = entry.tail.or(old.tail);
                        entry.full = entry.full.or(old.full);
                    }
                    entries.insert(key, entry);
                }
                None => {
                    entries.remove(&key);
                }
            }
        }
        let json = serde_json::to_vec(&CacheFile {
            version: VERSION,
            entries,
            archives,
        })
        .map_err(std::io::Error::other)?;
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = file.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAMP: Stamp = Stamp {
        size: 10,
        modified_ns: 1_000,
    };

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("auditor_cache_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_entry_holds_only_for_the_same_size_and_modified_time() {
        let mut cache = KeyCache::default();
        let a = Path::new("a.dem");
        cache.put(a, STAMP, 7);
        assert_eq!(cache.entry(a, STAMP).map(|e| e.head), Some(7));
        let resized = Stamp { size: 11, ..STAMP };
        let touched = Stamp {
            modified_ns: 2_000,
            ..STAMP
        };
        assert_eq!(cache.entry(a, resized), None);
        assert_eq!(cache.entry(a, touched), None);
        assert_eq!(cache.entry(Path::new("b.dem"), STAMP), None);
        // A file the system gives no modified time is never remembered.
        cache.put(
            Path::new("c.dem"),
            Stamp {
                size: 1,
                modified_ns: 0,
            },
            1,
        );
        assert_eq!(
            cache.entry(
                Path::new("c.dem"),
                Stamp {
                    size: 1,
                    modified_ns: 0
                }
            ),
            None
        );
    }

    #[test]
    fn a_new_key_drops_the_old_stamps_hashes() {
        let mut cache = KeyCache::default();
        let a = Path::new("a.dem");
        let hashes = ContentHashes {
            size: 10,
            head: 1,
            tail: 2,
            full: 3,
        };
        cache.remember_contents(a, STAMP, hashes);
        assert_eq!(cache.entry(a, STAMP).and_then(|e| e.full), Some(3));
        let touched = Stamp {
            modified_ns: 2_000,
            ..STAMP
        };
        cache.put(a, touched, 9);
        let entry = cache.entry(a, touched).unwrap();
        assert_eq!((entry.head, entry.tail, entry.full), (9, None, None));
        // A tail for a stamp that no longer holds is not kept.
        cache.set_tail(a, STAMP, 5);
        assert_eq!(cache.entry(a, touched).unwrap().tail, None);
        cache.set_tail(a, touched, 5);
        assert_eq!(cache.entry(a, touched).unwrap().tail, Some(5));
    }

    #[test]
    fn it_survives_a_save_and_load_and_a_bad_file_starts_empty() {
        let dir = scratch("roundtrip");
        let file = dir.join("audit_keys.json");
        let mut cache = KeyCache::default();
        cache.put(Path::new(r"C:\demos\a.dem"), STAMP, 7);
        assert!(cache.has_changes());
        cache.save(&file).unwrap();
        assert!(!cache.has_changes());
        let loaded = KeyCache::load(&file);
        assert_eq!(
            loaded
                .entry(Path::new(r"C:\demos\a.dem"), STAMP)
                .map(|e| e.head),
            Some(7)
        );
        std::fs::write(&file, b"{not json").unwrap();
        assert!(KeyCache::load(&file).entries.is_empty());
        std::fs::write(&file, br#"{"version":999,"entries":{}}"#).unwrap();
        assert!(KeyCache::load(&file).entries.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_save_keeps_what_another_run_saved_meanwhile() {
        let dir = scratch("merge");
        let file = dir.join("audit_keys.json");
        let (a, b) = (Path::new("a.dem"), Path::new("b.dem"));
        // A scan loads the cache, then a delete remembers a's full hash.
        let mut scan = KeyCache::load(&file);
        let mut delete = KeyCache::load(&file);
        let hashes = ContentHashes {
            size: 10,
            head: 7,
            tail: 2,
            full: 3,
        };
        delete.remember_contents(a, STAMP, hashes);
        delete.save(&file).unwrap();
        // The scan saves its own key for a, without the full hash, and b.
        scan.put(a, STAMP, 7);
        scan.put(b, STAMP, 8);
        scan.save(&file).unwrap();
        let both = KeyCache::load(&file);
        assert_eq!(both.entry(a, STAMP).and_then(|e| e.full), Some(3));
        assert_eq!(both.entry(b, STAMP).map(|e| e.head), Some(8));
        // A forgotten file goes from the file too.
        let mut later = KeyCache::load(&file);
        later.forget(b);
        later.save(&file).unwrap();
        assert_eq!(KeyCache::load(&file).entry(b, STAMP), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn zip_listing() -> Vec<ZipDemoEntry> {
        vec![ZipDemoEntry {
            name: "a.dem".into(),
            size: 10,
            crc32: 5,
            no_crc: false,
            hashes: None,
            error: None,
        }]
    }

    #[test]
    fn a_zips_listing_and_hashes_hold_for_its_stamp_and_survive_a_save() {
        let dir = scratch("zips");
        let file = dir.join("audit_keys.json");
        let zip = Path::new(r"C:\demos\pack.zip");
        let hashes = ContentHashes {
            size: 10,
            head: 1,
            tail: 0,
            full: 3,
        };
        let mut cache = KeyCache::default();
        cache.put_archive(zip, STAMP, Ok(zip_listing()));
        cache.set_archive_hashes(zip, STAMP, "a.dem", Ok(hashes));
        let touched = Stamp {
            modified_ns: 2_000,
            ..STAMP
        };
        assert!(cache.archive(zip, touched).is_none());
        // A scan that loaded before this save lists the zip again without
        // the hashes: the save keeps them.
        let mut other = KeyCache::load(&file);
        cache.save(&file).unwrap();
        other.put_archive(zip, STAMP, Ok(zip_listing()));
        other.save(&file).unwrap();
        let loaded = KeyCache::load(&file);
        let entry = loaded.archive(zip, STAMP).unwrap();
        assert_eq!(entry.demos[0].hashes, Some(hashes));
        // A zip that can't be read is remembered with why.
        let mut bad = KeyCache::default();
        bad.put_archive(zip, STAMP, Err("not a zip".into()));
        assert_eq!(
            bad.archive(zip, STAMP).unwrap().error.as_deref(),
            Some("not a zip")
        );
        // An older file, without zips, still loads.
        std::fs::write(&file, br#"{"version":1,"entries":{}}"#).unwrap();
        assert!(KeyCache::load(&file).archives.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn entries_under_a_walked_root_that_the_walk_missed_are_dropped() {
        let dir = std::fs::canonicalize(scratch("prune")).unwrap();
        let (here, gone) = (dir.join("here.dem"), dir.join("gone.dem"));
        let elsewhere = PathBuf::from(r"Z:\elsewhere\x.dem");
        let mut cache = KeyCache::default();
        for p in [&here, &gone, &elsewhere] {
            cache.put(p, STAMP, 1);
        }
        let gone_zip = dir.join("gone.zip");
        cache.put_archive(&gone_zip, STAMP, Ok(zip_listing()));
        cache.mark_seen(&here);
        // A walk that didn't list zips keeps them.
        cache.prune_unseen_under(std::slice::from_ref(&dir), false);
        assert!(cache.archive(&gone_zip, STAMP).is_some());
        assert!(cache.entry(&gone, STAMP).is_none());
        cache.prune_unseen_under(std::slice::from_ref(&dir), true);
        assert!(cache.archive(&gone_zip, STAMP).is_none());
        assert!(cache.entry(&here, STAMP).is_some());
        assert!(cache.entry(&gone, STAMP).is_none());
        assert!(cache.entry(&elsewhere, STAMP).is_some(), "not walked");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
