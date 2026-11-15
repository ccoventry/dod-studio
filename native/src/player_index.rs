//! Who is in a demo, without loading its whole analysis (#437, #174).
//!
//! Filtering a folder of demos by player needs each demo's player list. The
//! analyzer cache holds full analyses, a few megabytes each; reading a folder
//! of those to find names would be slow. So a small players file sits beside
//! each cache entry, keyed on the demo's size and modified time exactly like
//! the cache. `analysis::cache` owns that file: it writes it with every entry
//! it stores. This module only builds one on demand, by analysing a demo
//! that has none.

use std::path::Path;

pub use analysis::cache::{DemoPlayer, DemoPlayers, players_in};

/// `demo_path`'s players, and whether they came from its players file.
/// Otherwise the demo is analysed (through the cache, which also writes the
/// players file for next time).
pub fn demo_players(demo_path: &Path) -> Result<(DemoPlayers, bool), String> {
    if let Some(players) = analysis::cache::load_players(&crate::analyzer_cache_root(), demo_path) {
        return Ok((players, true));
    }
    let (_, analysis, _) = crate::run_analyzer_cached(&demo_path.to_path_buf(), |_, _| {})?;
    Ok((players_in(&analysis), false))
}

/// A folder's demos sorted by what reading their players costs (#992).
#[derive(Debug, Default, PartialEq, serde::Serialize)]
pub struct PlayerLookup {
    /// Demos whose players file is current, with their players: no read left.
    pub found: Vec<FoundPlayers>,
    /// Demos to read one by one: an analysis, or a cache entry with no
    /// players file yet.
    pub to_read: Vec<String>,
    /// Demos with no cache entry on a cloud drive: reading one downloads it,
    /// so they wait for the user to ask (Cache all, or opening the demo).
    pub in_cloud: Vec<String>,
}

#[derive(Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundPlayers {
    pub path: String,
    pub demo_type: String,
    pub players: Vec<DemoPlayer>,
}

/// Sorts `paths` into [`PlayerLookup`]'s three lists in one pass, opening no
/// demo: a stat and a small players file for each, and for a demo with
/// neither players nor a cache entry, its attributes.
pub fn lookup_players(paths: &[String]) -> PlayerLookup {
    let mut cloud = crate::sys::cloud_file::CloudCheck::new();
    lookup_players_in(&crate::analyzer_cache_root(), paths, |path| {
        cloud.is_online_only(path)
    })
}

fn lookup_players_in(
    root: &Path,
    paths: &[String],
    mut online_only: impl FnMut(&Path) -> bool,
) -> PlayerLookup {
    let mut lookup = PlayerLookup::default();
    for path in paths {
        let demo = Path::new(path);
        if let Some(players) = analysis::cache::load_players(root, demo) {
            lookup.found.push(FoundPlayers {
                path: path.clone(),
                demo_type: players.demo_type,
                players: players.players,
            });
        } else if !analysis::cache::is_fresh(root, demo) && online_only(demo) {
            lookup.in_cloud.push(path.clone());
        } else {
            lookup.to_read.push(path.clone());
        }
    }
    lookup
}

#[cfg(test)]
mod tests {
    use super::*;
    use analysis::Analysis;
    use analysis::cache::FileInfo;

    #[test]
    fn cached_demos_are_answered_and_cloud_ones_wait() {
        let dir = std::env::temp_dir().join(format!("player_lookup_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.join("cache");
        let demo = |name: &str| {
            let path = dir.join(name);
            std::fs::write(&path, name.as_bytes()).unwrap();
            path
        };
        let cached = demo("cached.dem");
        let info = FileInfo::of(&cached).unwrap();
        analysis::cache::store(&root, &cached, &info, &Analysis::default()).unwrap();
        // Cached in the cloud: its players file answers it, nothing downloads.
        let cached_cloud = demo("cached_cloud.dem");
        let info = FileInfo::of(&cached_cloud).unwrap();
        analysis::cache::store(&root, &cached_cloud, &info, &Analysis::default()).unwrap();
        let local = demo("local.dem");
        let cloud = demo("cloud.dem");
        let paths: Vec<String> = [&cached, &cached_cloud, &local, &cloud]
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();

        let lookup = lookup_players_in(&root, &paths, |p| {
            p.file_name().unwrap().to_string_lossy().contains("cloud")
        });
        let found: Vec<&str> = lookup.found.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(found, [paths[0].as_str(), paths[1].as_str()]);
        assert_eq!(lookup.to_read, [paths[2].clone()]);
        assert_eq!(lookup.in_cloud, [paths[3].clone()]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
