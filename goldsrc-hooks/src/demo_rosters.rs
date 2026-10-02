//! Who is in each demo, for the Demos tab's Player filter (#565, toward #437):
//! the players files `analysis::cache` keeps beside each analysis (written by
//! Studio's Demo Analyzer, its Master Queue scan, and the Killstreaks tab).
//!
//! A demo nobody has analysed has no players file, and a Player filter hides
//! it: the tab says how many demos it could look in.
//!
//! Analyses cached before players files existed get theirs the first time the
//! filter is used, on a thread of its own ([`ensure_filled`]); the list is
//! filtered again when that finishes ([`generation`]).

// Only the 32-bit build has the window; a host check still compiles this.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use analysis::cache::DemoPlayers;

/// Bumped when players files were written, so a filtered list is filtered
/// again.
static GENERATION: AtomicU64 = AtomicU64::new(0);
static FILLING: AtomicBool = AtomicBool::new(false);

/// Each demo's players (or none) by its path, with the file's modified time
/// and the [`GENERATION`] they were read at.
type Cached = (String, u64, u64, Option<DemoPlayers>);
static CACHE: Mutex<Vec<Cached>> = Mutex::new(Vec::new());

pub fn generation() -> u64 {
    GENERATION.load(Ordering::Acquire)
}

/// Writes the players files missing from the analyzer cache, once per
/// session, in the background.
pub fn ensure_filled() {
    if FILLING.swap(true, Ordering::AcqRel) {
        return;
    }
    let Some(root) = crate::streaks::cache_root() else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("dodstudio-players".into())
        .spawn(move || {
            let written = analysis::cache::fill_missing_players(&root);
            if written > 0 {
                GENERATION.fetch_add(1, Ordering::AcqRel);
            }
            unsafe {
                crate::debug::report(&format!(
                    "demo_rosters: wrote {written} missing players file(s) under {}",
                    root.display()
                ))
            };
        });
}

/// `path`'s players, read once per file version.
pub fn players_for(path: &Path) -> Option<DemoPlayers> {
    let key = path.to_string_lossy().into_owned();
    let modified = std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let generation = generation();
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, _, _, players)) = cache
        .iter()
        .find(|(p, m, g, _)| *p == key && *m == modified && *g == generation)
    {
        return players.clone();
    }
    let players =
        crate::streaks::cache_root().and_then(|root| analysis::cache::load_players(&root, path));
    cache.retain(|(p, _, _, _)| *p != key);
    cache.push((key, modified, generation, players.clone()));
    players
}

/// Whether someone matching `player` (every word, in their name or SteamID)
/// is in the demo, and recorded it when `recorded` is asked for.
pub fn has_player(demo: &DemoPlayers, player: &str, recorded: bool) -> bool {
    demo.players.iter().any(|p| {
        let haystack = format!("{} {}", p.name, p.id).to_ascii_lowercase();
        player
            .split_whitespace()
            .all(|word| haystack.contains(&word.to_ascii_lowercase()))
            && (!recorded || p.recorder)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use analysis::cache::DemoPlayer;

    fn demo() -> DemoPlayers {
        let player = |name: &str, id: &str, recorder| DemoPlayer {
            id: id.to_string(),
            steam_id: Some(id.to_string()),
            name: name.to_string(),
            recorder,
        };
        DemoPlayers {
            demo_type: "POV".to_string(),
            players: vec![
                player("{$T} Brain", "76561197960265729", true),
                player("dicE[: :]element", "76561197960265730", false),
            ],
        }
    }

    #[test]
    fn a_player_is_found_by_name_or_steamid() {
        assert!(has_player(&demo(), "brain", false));
        assert!(has_player(&demo(), "ELEMENT", false));
        assert!(has_player(&demo(), "76561197960265730", false));
        assert!(!has_player(&demo(), "milo", false));
    }

    #[test]
    fn recorded_it_needs_the_recorder() {
        assert!(has_player(&demo(), "brain", true));
        assert!(!has_player(&demo(), "element", true));
    }
}
