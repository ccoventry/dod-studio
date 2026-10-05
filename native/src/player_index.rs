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
