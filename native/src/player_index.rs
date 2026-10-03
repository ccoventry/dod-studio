//! Who is in a demo, without loading its whole analysis (#437, #174).
//!
//! Filtering a folder of demos by player needs each demo's player list. The
//! analyzer cache holds full analyses, a few megabytes each; reading a folder
//! of those to find names would be slow. So a small index sits beside each
//! cache entry, `<hash>.players.json`, keyed on the demo's size and modified
//! time exactly like the cache: written whenever a scan or the analyzer
//! caches a demo, and built on demand (from the cache, or by parsing) for a
//! demo that has none.

use std::fs;
use std::path::{Path, PathBuf};

use analysis::{Analysis, Connection};

/// One player seen in a demo.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DemoPlayer {
    /// The analyzer's global id: a SteamID64 when the demo has `*sid`,
    /// otherwise `PLAYER_<fid>` or a per-demo `CONNECTION_<n>`.
    pub id: String,
    /// The SteamID64, when `id` is one.
    pub steam_id: Option<String>,
    /// The last name the player had in this demo.
    pub name: String,
    /// The player who recorded this POV demo. Never set in an HLTV demo.
    pub recorder: bool,
}

/// A demo's players and type, as the index stores them.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct DemoPlayers {
    /// "POV" or "HLTV".
    pub demo_type: String,
    pub players: Vec<DemoPlayer>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct IndexEntry {
    size_bytes: u64,
    modified_unix_secs: u64,
    demo: DemoPlayers,
}

fn steam_id_of(id: &str) -> Option<String> {
    (id.len() == 17 && id.bytes().all(|b| b.is_ascii_digit())).then(|| id.to_string())
}

/// The players an analysis saw, recorder first, then by name.
pub fn players_in(analysis: &Analysis) -> DemoPlayers {
    let demo_type = analysis.demo_info.demo_type.clone();
    let pov = demo_type == "POV";
    let recorder_slot = analysis.state.pov_player_index;
    let mut players: Vec<DemoPlayer> = analysis
        .state
        .players
        .iter()
        .map(|p| {
            let id = p.id.to_string();
            let recorder = pov
                && matches!(p.connection, Connection::Connected { client_id }
                    if Some(client_id) == recorder_slot);
            DemoPlayer {
                steam_id: steam_id_of(&id),
                id,
                name: p.name.clone(),
                recorder,
            }
        })
        .collect();
    players.sort_by(|a, b| {
        b.recorder
            .cmp(&a.recorder)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    DemoPlayers { demo_type, players }
}

fn stamp(path: &Path) -> Option<(u64, u64)> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some((metadata.len(), modified))
}

fn index_path(demo_path: &Path) -> Option<PathBuf> {
    crate::analyzer_cache_path(&demo_path.to_path_buf())
        .map(|cache| cache.with_extension("players.json"))
}

/// Writes the index for `demo_path` from an analysis already in hand.
/// Best-effort, like the analyzer cache itself.
pub fn write_index(demo_path: &Path, analysis: &Analysis) {
    let (Some(path), Some((size_bytes, modified_unix_secs))) =
        (index_path(demo_path), stamp(demo_path))
    else {
        return;
    };
    let entry = IndexEntry {
        size_bytes,
        modified_unix_secs,
        demo: players_in(analysis),
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_vec(&entry) {
        let _ = fs::write(path, json);
    }
}

/// The players in `demo_path`: from its index when that is current, else
/// from the analyzer cache or a full parse (which also writes the index).
/// The bool is true when it was read from the index.
pub fn demo_players(demo_path: &Path) -> Result<(DemoPlayers, bool), String> {
    if let (Some(path), Some((size_bytes, modified_unix_secs))) =
        (index_path(demo_path), stamp(demo_path))
        && let Ok(bytes) = fs::read(&path)
        && let Ok(entry) = serde_json::from_slice::<IndexEntry>(&bytes)
        && entry.size_bytes == size_bytes
        && entry.modified_unix_secs == modified_unix_secs
    {
        return Ok((entry.demo, true));
    }
    let (_, analysis, _) = crate::run_analyzer_cached(&demo_path.to_path_buf(), |_, _| {})?;
    write_index(demo_path, &analysis);
    Ok((players_in(&analysis), false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_17_digit_id_is_a_steam_id() {
        assert_eq!(
            steam_id_of("76561197977930126"),
            Some("76561197977930126".to_string())
        );
        assert_eq!(steam_id_of("PLAYER_2761379"), None);
        assert_eq!(steam_id_of("CONNECTION_4"), None);
        assert_eq!(steam_id_of("7656119797793012"), None);
    }
}
