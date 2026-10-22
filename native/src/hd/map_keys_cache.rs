//! Each map's HD file keys, kept between scans (#426).
//!
//! Working out a map's keys reads its texture lump and every texture it
//! takes from its wads: seconds for an install's 250 maps, on every visit to
//! the HD page. The keys change only when the `.bsp` or one of its wads
//! does, so this keeps them per `.bsp` with each file's size and modified
//! time, and reads a map again only when one of those differs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use super::map_textures::{MapKeys, wad_path};

/// Bumped whenever the keys a map comes to change for the same files (the
/// hashing, the naming, what the build skips), so an older file is dropped.
const VERSION: u32 = 1;

/// Where the app keeps it: beside its other data, for every install.
pub fn default_path() -> PathBuf {
    crate::shared::paths::get_appdata_dir().join("hd_map_keys.json")
}

/// A file's size and modified time, as far as the cache can tell it apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Stamp {
    len: u64,
    modified_ns: u64,
}

fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    let modified_ns = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
    Some(Stamp {
        len: meta.len(),
        modified_ns,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    bsp: Stamp,
    /// Each wad the map lists, and its stamp where it was found (`None`:
    /// not found, which counts as a stamp too, since adding it changes the
    /// keys).
    wads: Vec<(String, Option<Stamp>)>,
    keys: Vec<String>,
}

/// The keys of every map scanned, by `.bsp` path (so two installs keep
/// theirs apart).
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct KeysCache {
    version: u32,
    maps: HashMap<String, Entry>,
    #[serde(skip)]
    changed: bool,
}

impl KeysCache {
    /// The cache at `path`; empty when there is none, it doesn't parse, or
    /// it is from another version.
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .filter(|cache| cache.version == VERSION)
            .unwrap_or_default()
    }

    /// Writes the cache to `path` when a scan changed it, dropping maps whose
    /// `.bsp` is gone.
    pub fn save(&mut self, path: &Path) -> Result<(), String> {
        let before = self.maps.len();
        self.maps.retain(|bsp, _| Path::new(bsp).is_file());
        if !self.changed && self.maps.len() == before {
            return Ok(());
        }
        self.version = VERSION;
        let json = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| crate::messages::labeled(dir.display(), e))?;
        }
        let part = path.with_extension("json.part");
        std::fs::write(&part, json).map_err(|e| crate::messages::labeled(part.display(), e))?;
        std::fs::rename(&part, path).map_err(|e| crate::messages::labeled(path.display(), e))?;
        self.changed = false;
        Ok(())
    }

    /// The keys of the map at `bsp` in the install at `game`: the kept ones
    /// while the `.bsp` and its wads are unchanged, else `read()`'s, kept for
    /// next time. A map `read` fails on is not kept.
    pub fn keys(
        &mut self,
        game: &Path,
        bsp: &Path,
        read: impl FnOnce() -> Result<MapKeys, String>,
    ) -> Result<Vec<String>, String> {
        let id = bsp.to_string_lossy().to_string();
        let bsp_stamp = stamp(bsp);
        if let (Some(entry), Some(now)) = (self.maps.get(&id), bsp_stamp)
            && entry.bsp == now
            && entry
                .wads
                .iter()
                .all(|(wad, was)| wad_path(game, wad).and_then(|p| stamp(&p)) == *was)
        {
            return Ok(entry.keys.clone());
        }
        let keys = read()?;
        if let Some(bsp_stamp) = bsp_stamp {
            let wads = keys
                .wads
                .iter()
                .map(|wad| (wad.clone(), wad_path(game, wad).and_then(|p| stamp(&p))))
                .collect();
            self.maps.insert(
                id,
                Entry {
                    bsp: bsp_stamp,
                    wads,
                    keys: keys.keys.iter().cloned().collect(),
                },
            );
            self.changed = true;
        }
        Ok(keys.keys.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    fn keys_of(names: &[&str], wads: &[&str]) -> MapKeys {
        MapKeys {
            keys: names.iter().map(|s| s.to_string()).collect(),
            wads: wads.iter().map(|s| s.to_string()).collect(),
            ..MapKeys::default()
        }
    }

    #[test]
    fn a_map_is_read_again_only_when_its_bsp_or_a_wad_changes() {
        let dir = Scratch::new("hd_map_keys_cache");
        let maps = dir.join("dod").join("maps");
        std::fs::create_dir_all(&maps).unwrap();
        std::fs::create_dir_all(dir.join("valve")).unwrap();
        let bsp = maps.join("dod_test.bsp");
        std::fs::write(&bsp, b"map").unwrap();
        std::fs::write(dir.join("valve").join("a.wad"), b"wad").unwrap();

        let mut cache = KeysCache::default();
        let mut reads = 0;
        let mut scan = |cache: &mut KeysCache, names: &[&str]| {
            cache
                .keys(&dir, &bsp, || {
                    reads += 1;
                    Ok(keys_of(names, &["a.wad", "b.wad"]))
                })
                .unwrap()
        };
        assert_eq!(scan(&mut cache, &["x_1", "y_2"]), ["x_1", "y_2"]);
        // Unchanged files: the kept keys, not a new read.
        assert_eq!(scan(&mut cache, &["new_3"]), ["x_1", "y_2"]);

        // Kept across a save and a load.
        let file = dir.join("cache").join("keys.json");
        cache.save(&file).unwrap();
        let mut cache = KeysCache::load(&file);
        assert_eq!(scan(&mut cache, &["new_3"]), ["x_1", "y_2"]);

        // A wad that appears (b.wad was missing) or changes size reads again.
        std::fs::write(dir.join("dod").join("b.wad"), b"wad").unwrap();
        assert_eq!(scan(&mut cache, &["b_4"]), ["b_4"]);
        std::fs::write(dir.join("valve").join("a.wad"), b"longer wad").unwrap();
        assert_eq!(scan(&mut cache, &["a_5"]), ["a_5"]);
        // So does a changed .bsp.
        std::fs::write(&bsp, b"other map").unwrap();
        assert_eq!(scan(&mut cache, &["m_6"]), ["m_6"]);
        assert_eq!(reads, 4);
    }

    #[test]
    fn a_failed_read_is_not_kept_and_a_bad_file_loads_empty() {
        let dir = Scratch::new("hd_map_keys_cache_bad");
        let bsp = dir.join("dod_bad.bsp");
        std::fs::write(&bsp, b"junk").unwrap();
        let mut cache = KeysCache::default();
        assert!(cache.keys(&dir, &bsp, || Err("not a BSP".into())).is_err());
        assert!(cache.maps.is_empty());

        let file = dir.join("keys.json");
        std::fs::write(&file, b"{ not json").unwrap();
        assert!(KeysCache::load(&file).maps.is_empty());
        // Another version's keys may be wrong for this one.
        std::fs::write(
            &file,
            br#"{"version":0,"maps":{"x":{"bsp":{"len":1,"modified_ns":1},"wads":[],"keys":["k"]}}}"#,
        )
        .unwrap();
        assert!(KeysCache::load(&file).maps.is_empty());
        assert!(KeysCache::load(&dir.join("absent.json")).maps.is_empty());
    }

    #[test]
    fn maps_whose_bsp_is_gone_are_dropped_on_save() {
        let dir = Scratch::new("hd_map_keys_cache_gone");
        let bsp = dir.join("dod_gone.bsp");
        std::fs::write(&bsp, b"map").unwrap();
        let mut cache = KeysCache::default();
        cache
            .keys(&dir, &bsp, || Ok(keys_of(&["k_1"], &[])))
            .unwrap();
        std::fs::remove_file(&bsp).unwrap();
        let file = dir.join("keys.json");
        cache.save(&file).unwrap();
        assert!(KeysCache::load(&file).maps.is_empty());
    }
}
