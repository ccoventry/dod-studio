//! The analyzer cache every reader of a demo shares: Studio's Demo Analyzer,
//! its Master Queue scan and highlight scanner, and the in-game hook DLL's
//! Highlights tab (#565). One JSON file per demo,
//! `<root>/v<SCHEMA_VERSION>/<fnv1a of the canonical path>.json`, valid while
//! the demo's size and modified time match what it records. `<root>` is
//! `%APPDATA%\dod-studio\analyzer_cache`; callers pass it in, so this crate
//! needs no app-data lookup of its own.
//!
//! Beside each entry, `<hash>.players.json` holds its [`DemoPlayers`](crate::cache::DemoPlayers): who is
//! in the demo and who recorded it, a few hundred bytes, so a list of
//! hundreds of demos can be filtered by player without loading whole analyses
//! (#437, #174).
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
pub const SCHEMA_VERSION: u32 = 7;

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

/// Whether `demo_path` has an entry for the file as it is now, without
/// reading the whole entry (megabytes): its stamp is the entry's first two
/// keys, so the first few hundred bytes say. For ordering work, e.g. listing
/// the cached demos of a folder before the ones that need a parse.
pub fn is_fresh(root: &Path, demo_path: &Path) -> bool {
    use std::io::Read;
    let Some((size_bytes, modified_unix_secs)) = stamp(demo_path) else {
        return false;
    };
    let Some(Ok(file)) = entry_path(root, demo_path).map(std::fs::File::open) else {
        return false;
    };
    let mut head = Vec::with_capacity(200);
    if file.take(200).read_to_end(&mut head).is_err() {
        return false;
    }
    String::from_utf8_lossy(&head).starts_with(&format!(
        "{{\"size_bytes\":{size_bytes},\"modified_unix_secs\":{modified_unix_secs},"
    ))
}

/// The order to read a list of demos in, by index: the ones with a fresh
/// entry first (milliseconds each, so a count jumps to "cached of total" at
/// once), in list order; then the rest, largest file first, so the last
/// parses running on several workers are small ones and no worker sits idle
/// behind one big demo at the end. Results go back in list order; only the
/// work order changes.
pub fn work_order(fresh: &[bool], sizes: &[u64]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..fresh.len()).collect();
    order.sort_by_key(|&i| {
        let size = sizes.get(i).copied().unwrap_or(0);
        if fresh[i] {
            (0, 0)
        } else {
            (1, u64::MAX - size)
        }
    });
    order
}

/// How a scan reads a list of demos: which ones the cache has, each file's
/// size, and the [`work_order`]. List Demos, Cache all and the Master Queue
/// scan all build it here so their order can't drift apart (#687).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkPlan {
    /// Per demo, in list order: [`is_fresh`].
    pub fresh: Vec<bool>,
    /// Per demo, in list order: the file's size, 0 when it can't be read.
    pub sizes: Vec<u64>,
    /// Indices into the list, in the order to read them.
    pub order: Vec<usize>,
}

impl WorkPlan {
    /// Checks each demo against the cache under `root`: 200 bytes of entry
    /// per demo, so a whole folder takes a fraction of a second.
    pub fn new<P: AsRef<Path>>(root: &Path, demos: &[P]) -> Self {
        let fresh = demos.iter().map(|d| is_fresh(root, d.as_ref())).collect();
        let sizes = demos
            .iter()
            .map(|d| std::fs::metadata(d).map_or(0, |m| m.len()))
            .collect();
        Self::from_parts(fresh, sizes)
    }

    pub fn from_parts(fresh: Vec<bool>, sizes: Vec<u64>) -> Self {
        let order = work_order(&fresh, &sizes);
        Self {
            fresh,
            sizes,
            order,
        }
    }

    /// Demos the cache has.
    pub fn cached(&self) -> usize {
        self.fresh.iter().filter(|f| **f).count()
    }

    /// The bytes of the demos that need a parse. Progress divides the parse
    /// time so far by the bytes parsed for its time left: demo sizes vary too
    /// much (5 MB tests, 90 MB matches) for a time per demo.
    pub fn bytes_to_parse(&self) -> u64 {
        self.fresh
            .iter()
            .zip(&self.sizes)
            .filter(|(fresh, _)| !**fresh)
            .map(|(_, size)| size)
            .sum()
    }
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
    write_whole(&path, &json)?;
    let _ = write_players(&path, size_bytes, modified_unix_secs, analysis);
    Some(path)
}

/// Writes `bytes` beside `path` and renames it over, so no reader meets a
/// half-written file.
fn write_whole(path: &Path, bytes: &[u8]) -> Option<()> {
    let name = path.file_name()?.to_string_lossy();
    let partial = path.with_file_name(format!("{name}.{}.part", std::process::id()));
    std::fs::write(&partial, bytes).ok()?;
    if std::fs::rename(&partial, path).is_err() {
        let _ = std::fs::remove_file(&partial);
        return None;
    }
    Some(())
}

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
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DemoPlayers {
    /// "POV" or "HLTV".
    pub demo_type: String,
    pub players: Vec<DemoPlayer>,
}

/// The players file's layout. The same file and layout as PR #486's
/// `native::player_index` (Studio's Demo Analyzer and Master Queue player
/// filters), so the game and Studio share one index.
#[derive(serde::Serialize, serde::Deserialize)]
struct PlayersEntry {
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
                && matches!(p.connection, crate::Connection::Connected { client_id }
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

/// Where `entry`'s players file lives: beside it.
fn players_path_of(entry: &Path) -> PathBuf {
    entry.with_extension("players.json")
}

fn write_players(
    entry: &Path,
    size_bytes: u64,
    modified_unix_secs: u64,
    analysis: &Analysis,
) -> Option<()> {
    let players = PlayersEntry {
        size_bytes,
        modified_unix_secs,
        demo: players_in(analysis),
    };
    write_whole(&players_path_of(entry), &serde_json::to_vec(&players).ok()?)
}

/// `demo_path`'s players, if its players file is current.
pub fn load_players(root: &Path, demo_path: &Path) -> Option<DemoPlayers> {
    let (size_bytes, modified_unix_secs) = stamp(demo_path)?;
    let bytes = std::fs::read(players_path_of(&entry_path(root, demo_path)?)).ok()?;
    let entry: PlayersEntry = serde_json::from_slice(&bytes).ok()?;
    (entry.size_bytes == size_bytes && entry.modified_unix_secs == modified_unix_secs)
        .then_some(entry.demo)
}

/// Writes the players file of every entry under `root` that has none yet
/// (entries saved before players files existed), and returns how many it
/// wrote. Reads each such entry whole, once.
pub fn fill_missing_players(root: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(root.join(format!("v{SCHEMA_VERSION}"))) else {
        return 0;
    };
    let mut written = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let is_entry = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".json") && n.matches('.').count() == 1);
        // Skip one already current; rewrite one in an older layout.
        let current = std::fs::read(players_path_of(&path))
            .ok()
            .is_some_and(|bytes| serde_json::from_slice::<PlayersEntry>(&bytes).is_ok());
        if !is_entry || current {
            continue;
        }
        let Some(cached) = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Entry>(&bytes).ok())
        else {
            continue;
        };
        if write_players(
            &path,
            cached.size_bytes,
            cached.modified_unix_secs,
            &cached.analysis,
        )
        .is_some()
        {
            written += 1;
        }
    }
    written
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
        // Its players file beside it, and no partial file.
        assert_eq!(
            std::fs::read_dir(written.parent().unwrap())
                .unwrap()
                .count(),
            2
        );
        let players = load_players(&root, &demo).unwrap();
        assert!(players.players.is_empty());

        // An entry saved before players files existed gets one.
        std::fs::remove_file(written.with_extension("players.json")).unwrap();
        assert!(load_players(&root, &demo).is_none());
        assert_eq!(fill_missing_players(&root), 1);
        assert!(load_players(&root, &demo).is_some());
        assert_eq!(fill_missing_players(&root), 0);

        // A different size: the entry no longer describes the file.
        assert!(is_fresh(&root, &demo));
        std::fs::write(&demo, b"a longer demo than before").unwrap();
        assert!(load(&root, &demo).is_none());
        assert!(!is_fresh(&root, &demo));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn work_order_reads_cached_demos_first_then_the_largest_parse() {
        // 0: 5 MB uncached, 1: cached, 2: 90 MB uncached, 3: cached, 4: 40 MB uncached.
        let fresh = [false, true, false, true, false];
        let sizes = [5, 80, 90, 10, 40];
        assert_eq!(work_order(&fresh, &sizes), [1, 3, 2, 4, 0]);
        // Same-size parses keep list order.
        assert_eq!(work_order(&[false, false], &[7, 7]), [0, 1]);
        assert!(work_order(&[], &[]).is_empty());
    }

    #[test]
    fn a_work_plan_counts_the_cached_demos_and_the_bytes_to_parse() {
        let plan = WorkPlan::from_parts(vec![false, true, false], vec![5, 80, 90]);
        assert_eq!(plan.order, [1, 2, 0]);
        assert_eq!(plan.cached(), 1);
        assert_eq!(plan.bytes_to_parse(), 95);

        let dir = scratch("plan");
        let root = dir.join("cache");
        let (cached, uncached) = (dir.join("a.dem"), dir.join("b.dem"));
        std::fs::write(&cached, b"cached demo").unwrap();
        std::fs::write(&uncached, b"a demo never analysed").unwrap();
        store(
            &root,
            &cached,
            &FileInfo::of(&cached).unwrap(),
            &Analysis::default(),
        )
        .unwrap();
        let plan = WorkPlan::new(&root, &[&uncached, &cached]);
        assert_eq!(plan.fresh, [false, true]);
        assert_eq!(plan.sizes, [21, 11]);
        assert_eq!(plan.order, [1, 0]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_demo_with_no_entry_is_not_fresh() {
        let dir = scratch("fresh");
        let demo = dir.join("y.dem");
        std::fs::write(&demo, b"never analysed").unwrap();
        assert!(!is_fresh(&dir.join("cache"), &demo));
        assert!(!is_fresh(&dir.join("cache"), &dir.join("missing.dem")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_a_17_digit_id_is_a_steam_id() {
        assert_eq!(
            steam_id_of("76561197975574370"),
            Some("76561197975574370".to_string())
        );
        assert_eq!(steam_id_of("PLAYER_2761379"), None);
        assert_eq!(steam_id_of("CONNECTION_4"), None);
    }

    /// The layout PR #486's `native::player_index` reads and writes.
    #[test]
    fn the_players_file_is_486s_layout() {
        let json = r#"{"size_bytes":1,"modified_unix_secs":2,"demo":{"demo_type":"POV","players":[{"id":"76561197975574370","steam_id":"76561197975574370","name":"chris","recorder":true}]}}"#;
        let entry: PlayersEntry = serde_json::from_str(json).unwrap();
        assert!(entry.demo.players[0].recorder);
        assert_eq!(serde_json::to_string(&entry).unwrap(), json);
    }

    #[test]
    fn fnv1a_matches_the_published_test_vectors() {
        assert_eq!(fnv1a(b""), 0xcbf29ce484222325);
        assert_eq!(fnv1a(b"a"), 0xaf63dc4c8601ec8c);
    }
}
