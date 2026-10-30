// scan_manager.rs
// Scanning folders for demos: listing them, skipping the ones a project
// already knows, parsing the rest for highlights, and finding demos that have
// moved or changed since the project last saw them.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::capture_manager::SerializedStreak;
use native::log_markdown;
use native::patch::CaptureStreak;
use native::sys::folder_walk::{Kind, Step, Walker};
use serde::{Deserialize, Serialize};

/// Same shape as `capture_manager`'s `PATCH_CONCURRENCY`, for
/// `scan_directory_impl`'s Phase 2 --
/// each file's parse is independent (no shared mutable state between demos;
/// `native::warm_analyzer_cache` writes to a per-demo-path cache file, not a
/// single shared one, so concurrent writes for different demos don't
/// collide), so it is bounded with a fixed worker pool the same way. See #195.
///
/// Two, not four, and deliberately lower than `PATCH_CONCURRENCY`: a parse
/// holds a whole `Analysis` in memory, which measures roughly 14x the demo's
/// own size, so concurrency here multiplies something already large. Measured
/// over a 45-demo corpus (3.2GB, 72MB mean, 105MB largest) with
/// `native/examples/scan_mem_probe.rs`:
///
/// | workers | wall time | peak working set |
/// | ------- | --------- | ---------------- |
/// | 1       | 56.2s     | 1529 MB          |
/// | 2       | 40.8s     | 2414 MB          |
/// | 4       | 36.0s     | 4574 MB          |
/// | 8       | 49.7s     | 9157 MB          |
///
/// Four buys 1.56x the speed for 3x the memory; two gets 1.38x for 1.6x.
/// Eight is slower *and* uses 9GB, which is what says the ceiling here is
/// memory pressure rather than CPU. This is a desktop app that may be running
/// alongside the game, so the last 4.8 seconds is not worth 2.2GB. Patching
/// stays at 4 because it streams frames rather than holding a full analysis.
///
/// This is the default for the user's own setting (`AppSettings::scan_workers`,
/// #246): the right number depends on the machine's RAM.
pub const SCAN_CONCURRENCY: usize = 2;

/// The range the scan worker box accepts (#246), the same as Max Concurrent
/// Renders. Only the range: nothing clamps to the machine's RAM.
pub const SCAN_WORKERS_MAX: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedDemo {
    pub path: String,
    pub name: String,
    pub tickrate: f32,
    pub is_pov: bool,
    pub local_player_index: Option<usize>,
    pub playback_frames: i32,
    pub streaks: Vec<SerializedStreak>,
    /// `<size>-<hash>` of the file when it was scanned (see `demo_file_key`).
    /// A later scan skips this demo while path and key still match. `None`
    /// for demos from a project saved before the key existed, which are
    /// simply scanned again.
    #[serde(default)]
    pub file_key: Option<String>,
    /// The map, from the demo header, for the clip name's `{map}` and the
    /// Master Queue's search (#441).
    #[serde(default)]
    pub map_name: Option<String>,
    /// The file's modified time, for the clip name's `{date}` (#441).
    #[serde(default)]
    pub modified_unix_secs: Option<u64>,
    /// The most entities in one snapshot (#207): over the engine's
    /// `MAX_PACKET_ENTITIES` the game closes to the desktop. `None` for a
    /// demo from a project saved before it was counted.
    #[serde(default)]
    pub peak_packet_entities: Option<u32>,
    /// No playback frame carries a view (#920): a capture of it is a frozen
    /// camera. `None` for a demo from a project saved before it was checked.
    #[serde(default)]
    pub no_camera_data: Option<bool>,
    /// Who recorded this POV demo (#174): the analyzer's global id (a
    /// SteamID64 when the demo has one) and name. `None` for an HLTV demo,
    /// and for demos from a project saved before this existed, which the
    /// Master Queue looks up through the player index instead.
    #[serde(default)]
    pub recorder_id: Option<String>,
    #[serde(default)]
    pub recorder_name: Option<String>,
    /// Each playing side and the clan tag its players' names share (#445),
    /// feeding the project's Teams list. Missing from demos in a project
    /// saved before it existed; the frontend scans those again.
    #[serde(default)]
    pub teams: Vec<analysis::TeamTag>,
    /// The map of every signon (#217): two or more is a demo the game's
    /// `viewdemo` can't play past the first level change, so the Master Queue
    /// offers to split it. Missing from demos in a project saved before it
    /// existed; the frontend scans those again.
    #[serde(default)]
    pub signon_maps: Vec<String>,
    /// The `analysis::highlights::HIGHLIGHTS_VERSION` that listed this demo's
    /// highlights; 0 for a demo from a project saved before it was recorded.
    /// Opening a project offers to rescan demos below the current one.
    #[serde(default)]
    pub highlights_version: u32,
}

/// A demo already in the queue, as the frontend passes it to a scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownDemo {
    pub path: String,
    pub file_key: String,
}

/// What a scan returns: the demos it parsed, how many it skipped because
/// they were already in the queue, unchanged on disk, and the identical
/// copies it skipped without parsing (#21).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanOutcome {
    pub demos: Vec<SerializedDemo>,
    pub unchanged: u32,
    #[serde(default)]
    pub copies: Vec<ScanCopy>,
    /// Cancel stopped the scan; `demos` holds what was parsed before it,
    /// none when it stopped while listing folders (#843).
    #[serde(default)]
    pub cancelled: bool,
    /// How many of the demos read had a current analyzer cache entry, how
    /// many didn't (read in full, or an older entry upgraded), and how long
    /// those took, from the first starting to the scan's end: for the
    /// activity log's timing line (#926, #937).
    #[serde(default)]
    pub cached: u32,
    #[serde(default)]
    pub not_cached: u32,
    #[serde(default)]
    pub not_cached_ms: u64,
    /// Of the `not_cached` demos, the ones with no older entry to upgrade
    /// either, read in full, and their bytes: Studio learns this PC's read
    /// rate from them for the rescan prompt's estimate (#937). The upgrades
    /// take milliseconds, so `not_cached_ms` is their time near enough.
    #[serde(default)]
    pub full_reads: u32,
    #[serde(default)]
    pub full_read_bytes: u64,
    /// The folder walk's depth or entry bound stopped it short (#531).
    #[serde(default)]
    pub cap: ScanCap,
}

/// A scanned file with the same key as a queued demo at another path, or as
/// another file in the same scan: the same demo under another name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanCopy {
    pub path: String,
    pub same_as: String,
    /// `same_as` was already in the queue; otherwise it's the copy this scan
    /// kept.
    pub queued: bool,
}

/// What `scan_demo_for_highlights_with_analysis` lists for one demo, before
/// its analysis.
type ScannedHighlights = (
    f32,
    Vec<CaptureStreak>,
    bool,
    Option<usize>,
    i32,
    Option<i32>,
    std::sync::Arc<Vec<f32>>,
);

/// One scanned demo as the queue holds it, stamped with the version of
/// highlight detection that listed its highlights.
fn serialized_demo(
    file: &std::path::Path,
    file_name: String,
    file_key: Option<String>,
    highlights: ScannedHighlights,
    analysis: &::analysis::Analysis,
) -> SerializedDemo {
    let (
        tickrate,
        streaks,
        is_pov,
        local_player_index,
        playback_frames,
        match_start_tick,
        frame_times_arc,
    ) = highlights;
    let recorder = native::player_index::players_in(analysis)
        .players
        .into_iter()
        .find(|p| p.recorder);

    let serialized_streaks: Vec<SerializedStreak> = streaks
        .into_iter()
        .map(|mut s| {
            s.match_start_tick = match_start_tick;
            s.frame_times = frame_times_arc.clone();
            let facts = native::clip_facts::streak_facts(
                analysis,
                s.player_index,
                s.kills.first().map_or(s.start_tick, |k| k.0),
            )
            .unwrap_or_default();
            let mut serialized = SerializedStreak::from(s);
            serialized.faction = Some(facts.faction).filter(|f| !f.is_empty());
            serialized.victims = facts.victims;
            serialized.victim_factions = facts.victim_factions;
            serialized.headshots = facts.headshots;
            serialized
        })
        .collect();

    SerializedDemo {
        path: file.to_string_lossy().to_string(),
        name: file_name,
        tickrate,
        is_pov,
        local_player_index,
        playback_frames,
        streaks: serialized_streaks,
        file_key,
        map_name: Some(analysis.demo_info.map_name.clone()).filter(|m| !m.is_empty()),
        modified_unix_secs: std::fs::metadata(file)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs()),
        peak_packet_entities: analysis.demo_info.peak_packet_entities,
        no_camera_data: Some(analysis.demo_info.no_camera_data),
        recorder_id: recorder.as_ref().map(|p| p.id.clone()),
        recorder_name: recorder.map(|p| p.name),
        teams: ::analysis::team_tags(&analysis.state),
        signon_maps: analysis.state.signon_maps.clone(),
        highlights_version: ::analysis::highlights::HIGHLIGHTS_VERSION,
    }
}

impl From<CaptureStreak> for SerializedStreak {
    fn from(c: CaptureStreak) -> Self {
        Self {
            start_tick: c.start_tick,
            end_tick: c.end_tick,
            source_demo: c.source_demo,
            target_player: c.target_player,
            kill_count: c.kill_count,
            timeline_string: c.timeline_string,
            duration_string: c.duration_string,
            player_index: c.player_index,
            kills: c.kills,
            start_index: c.start_index,
            end_index: c.end_index,
            total_demo_frames: c.total_demo_frames,
            demo_fps: c.demo_fps,
            viewdemo_times: c.viewdemo_times,
            // Flatten Arc<Vec<f32>> → Vec<f32> for JSON transport.
            // The inbound From<SerializedStreak> impl re-wraps it in Arc::new().
            frame_times: (*c.frame_times).clone(),
            match_start_tick: c.match_start_tick,
            faction: None,
            victims: Vec::new(),
            victim_factions: Vec::new(),
            headshots: Vec::new(),
            source_key: c.source_key,
            spectated: c.spectated,
            steam_id: c.steam_id,
            recorded_this_demo: Some(c.recorded_this_demo),
        }
    }
}

/// Threads for a scan of `total` demos: the user's setting, kept to
/// 1..=`SCAN_WORKERS_MAX`, and never more than there are demos.
fn scan_worker_count(workers: usize, total: usize) -> usize {
    workers.clamp(1, SCAN_WORKERS_MAX).min(total).max(1)
}

/// The scan status line: how many demos are done out of how many, how many
/// of them the analyzer cache has (they go first, #687), and one demo still
/// being parsed, when there is one. With several scan workers, "the current
/// file" is one of the current files. Studio adds the time left, and shows
/// the line as the Reading step of the queue's bar (#951).
fn scan_status_line(done: u32, total: u32, cached: u32, current: Option<&str>) -> String {
    let mut line = format!("Reading demos: {done} of {total}");
    if cached > 0 {
        line += &format!(" ({cached} from the analyzer cache)");
    }
    if let Some(name) = current {
        line += &format!(", {name}");
    }
    line
}

/// The activity log's line when a folder scan hit its bounds (#531).
fn scan_cap_log_line(cap: &ScanCap) -> String {
    let mut parts = Vec::new();
    if cap.depth {
        parts.push(format!(
            "folders more than {} levels down were not read",
            cap.max_depth
        ));
    }
    if cap.entries {
        parts.push(format!(
            "the folder walk stopped after {} files and folders",
            cap.max_entries
        ));
    }
    format!(
        "Scan: some demos may be missing: {}. Pick a folder further down to scan the rest.",
        parts.join("; ")
    )
}

/// The scan's step line while it lists folders, before any demo is parsed.
fn listing_status_line(folders: u32, demos: u32) -> String {
    format!("Looking for demos: {folders} folders read, {demos} demos found")
}

/// The scan's step line while it reads each listed demo's first 64 KB, to
/// skip the ones already queued and identical copies before any parse.
fn keying_status_line(done: u32, total: u32) -> String {
    format!("Checking which demos are new: {done} of {total}")
}

/// How deep, and how many entries, a folder scan with "Include subfolders"
/// walks (#531): the same bounds as the moved-demo search (#21), so a folder
/// with a deep or huge tree under it cannot turn a scan into a hang. Hitting
/// either is reported, never silent.
const SCAN_MAX_DEPTH: usize = LOCATE_MAX_DEPTH;
const SCAN_MAX_ENTRIES: usize = LOCATE_MAX_ENTRIES;

/// Which of the folder scan's bounds stopped it short (#531).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanCap {
    /// Folders deeper than [`SCAN_MAX_DEPTH`] levels were not read.
    pub depth: bool,
    /// The walk read [`SCAN_MAX_ENTRIES`] entries and stopped there.
    pub entries: bool,
    /// The two bounds, for the frontend's toast.
    pub max_depth: u32,
    pub max_entries: u32,
}

impl ScanCap {
    pub fn hit(&self) -> bool {
        self.depth || self.entries
    }
}

/// A folder's key for "already walked": lower case, `/` separators, no
/// trailing separator. Windows paths are case-insensitive.
fn folder_key(path: &Path) -> String {
    let mut key = path.to_string_lossy().replace('\\', "/").to_lowercase();
    while key.len() > 1 && key.ends_with('/') {
        key.pop();
    }
    key
}

/// The folders one scan has entered (#531), so none is walked twice: the
/// same folder picked twice, under two spellings, or once directly and once
/// through a junction to it. Each root is keyed by where it really is
/// (`canonicalize`); a folder under it by the root's key plus its own path
/// below the root. The walk enters no link, so that path below the root is
/// real too.
struct VisitedFolders {
    /// Each root as the walk spells it, and its key.
    roots: Vec<(PathBuf, String)>,
    seen: std::collections::HashSet<String>,
}

impl VisitedFolders {
    /// Keys `roots` by where they really are and drops a root already
    /// listed; returns the roots left to walk.
    fn new(roots: &[PathBuf]) -> (Self, Vec<PathBuf>) {
        let keyed = roots
            .iter()
            .map(|r| {
                let real = std::fs::canonicalize(r).unwrap_or_else(|_| r.clone());
                (r.clone(), folder_key(&real))
            })
            .collect();
        Self::with_keys(keyed)
    }

    fn with_keys(keyed: Vec<(PathBuf, String)>) -> (Self, Vec<PathBuf>) {
        let mut visited = Self {
            roots: Vec::new(),
            seen: std::collections::HashSet::new(),
        };
        let mut walk = Vec::new();
        for (root, key) in keyed {
            if visited.seen.insert(key.clone()) {
                walk.push(root.clone());
                visited.roots.push((root, key));
            }
        }
        (visited, walk)
    }

    /// True the first time the walk reaches `dir`. A folder that is one of
    /// the roots is false: it is walked as a root of its own.
    fn first_visit(&mut self, dir: &Path) -> bool {
        let under = self
            .roots
            .iter()
            .filter_map(|(root, key)| Some((dir.strip_prefix(root).ok()?, key)))
            .max_by_key(|(rest, _)| std::cmp::Reverse(rest.components().count()));
        let key = match under {
            Some((rest, root_key)) => {
                let rest = folder_key(rest);
                if rest.is_empty() {
                    root_key.clone()
                } else {
                    format!("{root_key}/{rest}")
                }
            }
            None => folder_key(dir),
        };
        self.seen.insert(key)
    }
}

/// How Phase 1's folder listing ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Listing {
    /// False when cancelled.
    listed: bool,
    cap: ScanCap,
}

/// Phase 1 of a queue scan: every `.dem` under `dirs` goes into `list`, kept
/// in file-name order. A picked folder is read through even when it is a
/// link; links inside it are not, so a junction back up the tree cannot
/// loop, and no folder is read twice ([`VisitedFolders`]). The Recycle Bin
/// and System Volume Information are skipped (#843). Without
/// `include_subfolders` only each folder's own demos are listed (#531);
/// with it, the walk stops at [`SCAN_MAX_DEPTH`] / [`SCAN_MAX_ENTRIES`] and
/// says so in the result's `cap`.
/// `progress(folders, demos)` is called as the counts grow (the caller
/// throttles); `cancel` is checked at every entry.
fn list_scan_demos(
    dirs: &[PathBuf],
    include_subfolders: bool,
    list: &mut Vec<PathBuf>,
    cancel: &std::sync::atomic::AtomicBool,
    progress: &mut dyn FnMut(u32, u32),
) -> Listing {
    let (mut visited, dirs) = VisitedFolders::new(dirs);
    let max_depth = if include_subfolders {
        SCAN_MAX_DEPTH
    } else {
        1
    };
    let mut cap = ScanCap {
        max_depth: SCAN_MAX_DEPTH as u32,
        max_entries: SCAN_MAX_ENTRIES as u32,
        ..ScanCap::default()
    };
    let mut folders = dirs.len() as u32;
    progress(folders, list.len() as u32);
    let outcome = Walker::new()
        .skip_system_folders()
        .max_depth(max_depth)
        .max_entries(SCAN_MAX_ENTRIES)
        .walk_roots(&dirs, &mut |found| {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Step::Stop;
            }
            match found.kind {
                // Without subfolders, a folder is neither counted nor read.
                Kind::Dir if !include_subfolders => return Step::Skip,
                Kind::Dir => {
                    if !visited.first_visit(&found.path) {
                        return Step::Skip;
                    }
                    if found.depth >= max_depth {
                        // The walker does not enter a folder this deep.
                        cap.depth = true;
                        return Step::Skip;
                    }
                    folders += 1;
                }
                Kind::File if found.is_demo() => {
                    let insert_idx = list
                        .binary_search_by(|p: &PathBuf| {
                            p.file_name()
                                .unwrap_or_default()
                                .cmp(found.path.file_name().unwrap_or_default())
                        })
                        .unwrap_or_else(|pos| pos);
                    list.insert(insert_idx, found.path.clone());
                }
                _ => return Step::Enter,
            }
            progress(folders, list.len() as u32);
            Step::Enter
        });
    let cancelled = cancel.load(std::sync::atomic::Ordering::Relaxed);
    cap.entries = !outcome.complete && !cancelled;
    Listing {
        listed: outcome.complete || !cancelled,
        cap,
    }
}

/// True when a progress line may go out at `now_ms`: at most one per ~33 ms
/// (CLAUDE.md's telemetry throttle). `last_ms` holds the last emit time, or
/// `u32::MAX` before the first one.
pub(crate) fn progress_emit_due(last_ms: &std::sync::atomic::AtomicU32, now_ms: u32) -> bool {
    use std::sync::atomic::Ordering;
    let last = last_ms.load(Ordering::Relaxed);
    if last != u32::MAX && now_ms.saturating_sub(last) < 33 {
        return false;
    }
    last_ms
        .compare_exchange(last, now_ms, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
}

/// A demo file's identity for "has this changed since it was scanned": its
/// size plus an FNV-1a hash of its first 64 KB -- the same key the Demo
/// Auditor finds duplicates with (`native::utils::demo_hasher`), and the same
/// `<size>-<hash>` text form #196's `source_key` uses. Reads 64 KB, not the
/// whole demo, so checking a folder of 50 costs milliseconds.
fn demo_file_key(path: &Path) -> Option<String> {
    native::utils::demo_hasher::demo_key_text(path)
}

/// The demos whose file is still there but is no longer the one that was
/// scanned: its key (size + first 64 KB) differs from the saved one. Their
/// highlights are frame numbers in the old file, so they don't line up (#21).
/// Missing files and demos with no saved key are left out.
///
/// `checked(done, total)` runs after each demo, so Start Capture's line can
/// count them (#953). Every call; the caller throttles.
pub fn changed_demos_with_progress(
    demos: &[KnownDemo],
    mut checked: impl FnMut(usize, usize),
) -> Vec<String> {
    let mut changed = Vec::new();
    for (i, d) in demos.iter().enumerate() {
        if !d.file_key.is_empty()
            && Path::new(&d.path).is_file()
            && demo_file_key(Path::new(&d.path)).is_some_and(|key| key != d.file_key)
        {
            changed.push(d.path.clone());
        }
        checked(i + 1, demos.len());
    }
    changed
}

/// Whether a `changed_demos_progress` event goes out: the last demo always
/// (so the count ends on "12 of 12"), the others at most one per ~33 ms.
pub(crate) fn changed_progress_due(
    last_ms: &std::sync::atomic::AtomicU32,
    now_ms: u32,
    done: usize,
    total: usize,
) -> bool {
    if done == total {
        last_ms.store(now_ms, std::sync::atomic::Ordering::Relaxed);
        return true;
    }
    progress_emit_due(last_ms, now_ms)
}

/// A project demo that is no longer at its saved path, and the file that
/// looks like it moved there, if one was found (#21).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingDemo {
    pub path: String,
    pub candidate: Option<String>,
}

/// How deep, and how many entries, the search for a moved demo will walk, so
/// pointing it at a drive root cannot turn a project load into a hang.
const LOCATE_MAX_DEPTH: usize = 6;
const LOCATE_MAX_ENTRIES: usize = 50_000;

/// For every demo in `demos` whose file is gone, looks through `search_dirs`
/// (recursively, bounded) for a `.dem` with the same `demo_file_key`: the
/// same size and the same first 64 KB. Only files whose size matches are
/// read at all, so a folder of hundreds of demos costs one `stat` each.
///
/// Breadth-first across all of `search_dirs` together, in the order given:
/// every folder's own files, then every first-level subfolder, and so on. A
/// demo moved one folder down is found before the entry budget can be spent
/// deep inside some large folder listed earlier (a whole `dod/`, say), which
/// is what a depth-first walk did.
///
/// Suggests, never applies: the frontend asks before it moves anything. A
/// demo with no saved key (a project from before #456) can only be reported
/// missing.
///
/// `on_search(n)` is called once, before the walk, with the number of
/// missing demos it looks for (those with a saved key), so the page can say
/// so (#958); not at all when nothing needs looking for.
pub fn locate_missing_demos(
    demos: &[KnownDemo],
    search_dirs: &[PathBuf],
    on_search: &mut dyn FnMut(usize),
) -> Vec<MissingDemo> {
    locate_missing_demos_within(demos, search_dirs, LOCATE_MAX_ENTRIES, on_search)
}

fn locate_missing_demos_within(
    demos: &[KnownDemo],
    search_dirs: &[PathBuf],
    max_entries: usize,
    on_search: &mut dyn FnMut(usize),
) -> Vec<MissingDemo> {
    let mut missing: Vec<MissingDemo> = demos
        .iter()
        .filter(|d| !Path::new(&d.path).is_file())
        .map(|d| MissingDemo {
            path: d.path.clone(),
            candidate: None,
        })
        .collect();
    let wanted: Vec<(usize, &str, u64)> = demos
        .iter()
        .filter(|d| !Path::new(&d.path).is_file())
        .enumerate()
        .filter_map(|(i, d)| {
            let size = native::utils::demo_hasher::FileKey::from_text(&d.file_key)?.size;
            Some((i, d.file_key.as_str(), size))
        })
        .collect();
    if wanted.is_empty() {
        return missing;
    }
    on_search(wanted.len());

    // A search folder listed twice, or inside another, is read once.
    let mut visited = std::collections::HashSet::new();
    let roots: Vec<&PathBuf> = search_dirs
        .iter()
        .filter(|d| visited.insert(same_path_key(&d.to_string_lossy())))
        .collect();
    Walker::new()
        .breadth_first()
        // A deleted copy in the Recycle Bin has the same key (#843).
        .skip_system_folders()
        // Folders LOCATE_MAX_DEPTH below a search folder are still read.
        .max_depth(LOCATE_MAX_DEPTH + 1)
        .max_entries(max_entries)
        .walk_roots(&roots, &mut |found| {
            match found.kind {
                Kind::Dir => {
                    return visited
                        .insert(same_path_key(&found.path.to_string_lossy()))
                        .into();
                }
                Kind::Link => return Step::Skip,
                Kind::File => {}
            }
            if !found.is_demo() {
                return Step::Enter;
            }
            let Ok(meta) = std::fs::metadata(&found.path) else {
                return Step::Enter;
            };
            if !wanted.iter().any(|(_, _, size)| *size == meta.len()) {
                return Step::Enter;
            }
            let Some(key) = demo_file_key(&found.path) else {
                return Step::Enter;
            };
            for (i, wanted_key, _) in &wanted {
                if missing[*i].candidate.is_none() && key == *wanted_key {
                    missing[*i].candidate = Some(found.path.to_string_lossy().to_string());
                }
            }
            if missing.iter().all(|m| m.candidate.is_some()) {
                Step::Stop
            } else {
                Step::Enter
            }
        });
    missing
}

/// Paths compare the way Windows does: case-insensitive, either slash.
fn same_path_key(path: &str) -> String {
    path.replace('/', "\\").to_lowercase()
}

/// Splits `list` into the files still to scan (with their keys, in order),
/// a count of those skipped because `known` has the same path with the same
/// key, and the identical copies skipped: files at a path `known` doesn't
/// have whose key matches a queued demo, or another file in this scan. Of
/// several new files that are one demo, the one with the shortest name is
/// kept ("match.dem" over "match - Copy.dem" and "match (2).dem"), whatever
/// order the scan found them in. Only the key (size + first 64 KB) is read
/// for the rest, never the demo. That read still takes seconds over a
/// folder of hundreds of demos on a cold disk, so `on_key(done, total)`
/// reports each one.
fn skip_known(
    list: Vec<PathBuf>,
    known: &[KnownDemo],
    on_key: &mut dyn FnMut(u32, u32),
) -> (Vec<PathBuf>, Vec<Option<String>>, u32, Vec<ScanCopy>) {
    let known_by_path: std::collections::HashMap<String, &str> = known
        .iter()
        .map(|k| (same_path_key(&k.path), k.file_key.as_str()))
        .collect();
    let queued_by_key: std::collections::HashMap<&str, &str> = known
        .iter()
        .filter(|k| !k.file_key.is_empty())
        .map(|k| (k.file_key.as_str(), k.path.as_str()))
        .collect();
    let name_rank = |path: &Path| {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        (name.chars().count(), name)
    };

    let total = list.len() as u32;
    let entries: Vec<(PathBuf, Option<String>)> = list
        .into_iter()
        .enumerate()
        .map(|(i, path)| {
            let key = demo_file_key(&path);
            on_key(i as u32 + 1, total);
            (path, key)
        })
        .collect();
    // The copy to keep for each key that no queued demo has.
    let mut keeper_by_key: std::collections::HashMap<&str, &Path> =
        std::collections::HashMap::new();
    for (path, key) in &entries {
        let Some(key) = key.as_deref() else { continue };
        if known_by_path.contains_key(&same_path_key(&path.to_string_lossy()))
            || queued_by_key.contains_key(key)
        {
            continue;
        }
        keeper_by_key
            .entry(key)
            .and_modify(|best| {
                if name_rank(path) < name_rank(best) {
                    *best = path;
                }
            })
            .or_insert(path);
    }

    let mut keep = Vec::with_capacity(entries.len());
    let mut keys = Vec::with_capacity(entries.len());
    let mut unchanged = 0;
    let mut copies = Vec::new();
    for (path, key) in &entries {
        let path_text = path.to_string_lossy().into_owned();
        let queued_key = known_by_path.get(&same_path_key(&path_text));
        if key.as_deref().is_some_and(|k| queued_key == Some(&k)) {
            unchanged += 1;
            continue;
        }
        // A queued path that changed on disk is a rescan, never a copy.
        if queued_key.is_none()
            && let Some(k) = key.as_deref()
        {
            if let Some(same_as) = queued_by_key.get(k) {
                copies.push(ScanCopy {
                    path: path_text,
                    same_as: same_as.to_string(),
                    queued: true,
                });
                continue;
            }
            if let Some(keeper) = keeper_by_key
                .get(k)
                .filter(|keeper| **keeper != path.as_path())
            {
                copies.push(ScanCopy {
                    path: path_text,
                    same_as: keeper.to_string_lossy().into_owned(),
                    queued: false,
                });
                continue;
            }
        }
        keep.push(path.clone());
        keys.push(key.clone());
    }
    (keep, keys, unchanged, copies)
}

/// The paths in `paths` that do not exist on disk, in the order given.
fn missing_scan_paths(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter(|p| !std::path::Path::new(p).exists())
        .cloned()
        .collect()
}

/// Why the scan could not use `path`, in plain words, from the file itself
/// rather than the scanner's error text: shorter than a demo header, a header
/// that isn't `HLDEMO`, or a good header on a demo that fails to parse later.
/// A failure to open or read the file at all (`Failed to read file: ...`,
/// e.g. access denied) is already readable and passes through as `reason`.
fn plain_scan_failure(path: &Path, reason: &str) -> String {
    // Already readable: the file could not be opened at all, or the (never
    // firing, #247) HLTV guard refused it.
    if reason.starts_with("Failed to read file") || reason.starts_with("Unsupported HLTV") {
        return reason.to_string();
    }
    let mut head = [0u8; 8];
    let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if len < native::patch::DEMO_HEADER_SIZE as u64 {
        return crate::messages::SCAN_FAIL_TOO_SHORT.to_string();
    }
    let read_ok = std::fs::File::open(path)
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head))
        .is_ok();
    if !read_ok {
        return reason.to_string();
    }
    if &head != b"HLDEMO\0\0" {
        return crate::messages::SCAN_FAIL_NOT_A_DEMO.to_string();
    }
    crate::messages::SCAN_FAIL_CORRUPT.to_string()
}

/// The final `scan_progress` event's `skipped` list: `{name, reason}` per demo
/// the scan could not read, in file-name order (workers finish out of order).
fn skipped_demo_rows(mut skipped: Vec<(usize, String, String)>) -> Vec<serde_json::Value> {
    skipped.sort_by_key(|(idx, _, _)| *idx);
    skipped
        .into_iter()
        .map(|(_, name, reason)| serde_json::json!({ "name": name, "reason": reason }))
        .collect()
}

pub async fn scan_directory_impl(
    app_handle: tauri::AppHandle,
    is_scanning: Arc<std::sync::atomic::AtomicBool>,
    cancel_token: Arc<std::sync::atomic::AtomicBool>,
    paths: Vec<String>,
    known: Vec<KnownDemo>,
    workers: usize,
    include_subfolders: bool,
) -> Result<ScanOutcome, String> {
    // ── Reset state flags ─────────────────────────────────────────────────────
    cancel_token.store(false, std::sync::atomic::Ordering::SeqCst);
    is_scanning.store(true, std::sync::atomic::Ordering::SeqCst);

    let is_scanning_end = Arc::clone(&is_scanning);

    let result = crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        use native::patch::scan_demo_for_highlights_with_analysis;
        use tauri::Emitter;

        let mut list = Vec::new();
        let mut dir_stack = Vec::new();

        // A picked folder deleted since the picker last saw it used to scan to
        // "0 demos found" with no reason given (#432).
        let missing = missing_scan_paths(&paths);
        if !missing.is_empty() {
            if missing.len() == paths.len() {
                return Err(crate::messages::scan_path_not_found(&missing[0]));
            }
            log_markdown(&format!(
                "[scan] Skipped paths that no longer exist: {}",
                missing.join(", ")
            ));
        }

        // ── Phase 1: collect all .dem file paths ─────────────────────────────
        for path_str in paths {
            let path_buf = PathBuf::from(path_str);
            if path_buf.is_dir() {
                dir_stack.push(path_buf);
            } else if path_buf.is_file()
                && path_buf
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("dem"))
            {
                let insert_idx = list
                    .binary_search_by(|p: &PathBuf| {
                        p.file_name()
                            .unwrap_or_default()
                            .cmp(path_buf.file_name().unwrap_or_default())
                    })
                    .unwrap_or_else(|pos| pos);
                list.insert(insert_idx, path_buf);
            }
        }

        // A whole drive takes a minute or more to list (#843): say how far
        // it got, at most one line per ~33 ms, the first at once.
        let listing_started = std::time::Instant::now();
        let last_listing_emit = std::sync::atomic::AtomicU32::new(u32::MAX);
        let listing = list_scan_demos(
            &dir_stack,
            include_subfolders,
            &mut list,
            &cancel_token,
            &mut |folders, demos| {
                let now_ms = listing_started
                    .elapsed()
                    .as_millis()
                    .min(u32::MAX as u128 - 1) as u32;
                if !progress_emit_due(&last_listing_emit, now_ms) {
                    return;
                }
                let _ = app_handle.emit(
                    "scan_progress",
                    serde_json::json!({
                        "scanned": 0,
                        "found": 0,
                        "status": listing_status_line(folders, demos),
                        "cancelled": false,
                        "listing": true,
                        "folders": folders,
                        "demos": demos,
                    }),
                );
            },
        );
        let cap = listing.cap;
        if cap.hit() {
            log_markdown(&scan_cap_log_line(&cap));
        }
        if !listing.listed {
            // Cancelled while listing: nothing is parsed, nothing added.
            let _ = app_handle.emit(
                "scan_progress",
                serde_json::json!({
                    "scanned": 0,
                    "found": 0,
                    "status": "Cancelled",
                    "cancelled": true,
                    "unchanged": 0,
                    "skipped": [],
                }),
            );
            return Ok(ScanOutcome {
                demos: Vec::new(),
                unchanged: 0,
                copies: Vec::new(),
                cancelled: true,
                cached: 0,
                not_cached: 0,
                not_cached_ms: 0,
                full_reads: 0,
                full_read_bytes: 0,
                cap,
            });
        }

        // Demos already in the queue and unchanged on disk are not parsed
        // again: re-adding a folder to pick up one new demo used to re-parse
        // every demo in it, 5-10 s each.
        let keying_started = std::time::Instant::now();
        let last_keying_emit = std::sync::atomic::AtomicU32::new(u32::MAX);
        let (list, file_keys, unchanged, copies) = skip_known(list, &known, &mut |done, total| {
            let now_ms = keying_started
                .elapsed()
                .as_millis()
                .min(u32::MAX as u128 - 1) as u32;
            if done < total && !progress_emit_due(&last_keying_emit, now_ms) {
                return;
            }
            let _ = app_handle.emit(
                "scan_progress",
                serde_json::json!({
                    "scanned": 0,
                    "found": 0,
                    "status": keying_status_line(done, total),
                    "cancelled": false,
                    "keying": true,
                    "keyed": done,
                    "total": total,
                }),
            );
        });

        let total_files = list.len() as u32;

        // ── Phase 2: parse each .dem file, up to `workers` at once ───
        // `list` is already sorted by filename (Phase 1's binary-search
        // insert), so each worker writes its result into a pre-sized slot at
        // its own original index -- the output comes out in sorted order for
        // free, with no re-sort needed once concurrent workers finish out of
        // order.
        //
        // The work goes in another order (#687): the demos the analyzer cache
        // has first, so the count jumps to "cached of total" at once, then
        // the parses largest first, so no worker idles behind one big demo at
        // the end.
        let total = list.len();
        let cache_root = native::analyzer_cache_root();
        let plan = ::analysis::cache::WorkPlan::new(&cache_root, &list);
        let cached = plan.cached() as u32;
        let bytes_to_parse = plan.bytes_to_parse();
        let parsed = std::sync::atomic::AtomicU32::new(0);
        let bytes_parsed = std::sync::atomic::AtomicU64::new(0);
        let full_reads = std::sync::atomic::AtomicU32::new(0);
        let full_read_bytes = std::sync::atomic::AtomicU64::new(0);
        // When the first demo without a current cache entry started.
        let first_parse: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        let slots: Mutex<Vec<Option<SerializedDemo>>> =
            Mutex::new((0..total).map(|_| None).collect());
        let next_index = std::sync::atomic::AtomicUsize::new(0);
        // Completed-count progress rather than positional index -- same
        // reason the patch loop's `capture_status` event uses one: files
        // finish out of order once scanned concurrently.
        let completed = std::sync::atomic::AtomicU32::new(0);
        let found = std::sync::atomic::AtomicU32::new(0);
        // Demos that could not be read, as (index, file name, reason). They
        // used to vanish from the queue with no explanation (#23).
        let skipped: Mutex<Vec<(usize, String, String)>> = Mutex::new(Vec::new());
        // Indices being parsed right now, so every progress line names a demo
        // still in flight rather than one that just finished (#433).
        let in_flight: Mutex<Vec<usize>> = Mutex::new(Vec::new());
        // ~33 ms telemetry throttle (CLAUDE.md), as ms since the scan began.
        // Starts at u32::MAX so the first line always goes out. `force` skips
        // it for the lines a count must not wait on: the end of the cached
        // batch and each parse's end.
        let scan_started = std::time::Instant::now();
        let last_emit_ms = std::sync::atomic::AtomicU32::new(u32::MAX);
        let emit_progress = |done: u32, current: Option<&str>, force: bool| {
            let now_ms = scan_started.elapsed().as_millis().min(u32::MAX as u128 - 1) as u32;
            if force {
                last_emit_ms.store(now_ms, std::sync::atomic::Ordering::Relaxed);
            } else if !progress_emit_due(&last_emit_ms, now_ms) {
                return;
            }
            let _ = app_handle.emit(
                "scan_progress",
                serde_json::json!({
                    "scanned": done,
                    "found": found.load(std::sync::atomic::Ordering::Relaxed),
                    "status": scan_status_line(done, total_files, cached, current),
                    "cancelled": false,
                    "total": total_files,
                    "cached": cached,
                    "parsed": parsed.load(std::sync::atomic::Ordering::Relaxed),
                    "bytes_to_parse": bytes_to_parse,
                    "bytes_parsed": bytes_parsed.load(std::sync::atomic::Ordering::Relaxed),
                }),
            );
        };
        std::thread::scope(|scope| {
            let worker_count = scan_worker_count(workers, total);
            for _ in 0..worker_count {
                let cancel_token = &cancel_token;
                let list = &list;
                let file_keys = &file_keys;
                let slots = &slots;
                let next_index = &next_index;
                let completed = &completed;
                let found = &found;
                let skipped = &skipped;
                let in_flight = &in_flight;
                let emit_progress = &emit_progress;
                let plan = &plan;
                let cache_root = &cache_root;
                let parsed = &parsed;
                let bytes_parsed = &bytes_parsed;
                let full_reads = &full_reads;
                let full_read_bytes = &full_read_bytes;
                let first_parse = &first_parse;
                let file_name_at = move |i: usize| {
                    list[i]
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string()
                };
                scope.spawn(move || {
                    loop {
                        // Honour cancellation before starting the next parse
                        // (I/O can be slow) -- a parse already in flight is not
                        // interrupted, same blind spot the patch loop's
                        // decal-flush pass has (#193).
                        if cancel_token.load(std::sync::atomic::Ordering::SeqCst) {
                            break;
                        }
                        let Some(&idx) = plan
                            .order
                            .get(next_index.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
                        else {
                            break;
                        };
                        let file = &list[idx];
                        let file_name = file
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        // Sent when a demo starts too, not only when one finishes:
                        // before #433 a one-demo folder showed a bare "Scanning..."
                        // for the whole parse, since its first finish was its last.
                        // The first of these also carries the total (`0 / N`).
                        in_flight
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .push(idx);
                        emit_progress(
                            completed.load(std::sync::atomic::Ordering::Relaxed),
                            Some(&file_name),
                            false,
                        );

                        if !plan.fresh[idx] {
                            first_parse.get_or_init(std::time::Instant::now);
                            if !::analysis::cache::has_older_entry(cache_root, file) {
                                full_reads.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                full_read_bytes.fetch_add(
                                    plan.sizes[idx],
                                    std::sync::atomic::Ordering::Relaxed,
                                );
                            }
                        }
                        let scanned = scan_demo_for_highlights_with_analysis(file);
                        if let Err(reason) = &scanned {
                            log_markdown(&format!("[scan] Skipped {}: {}", file.display(), reason));
                            skipped.lock().unwrap_or_else(|p| p.into_inner()).push((
                                idx,
                                file_name.clone(),
                                plain_scan_failure(file, reason),
                            ));
                        }
                        let serialized = scanned.ok().map(|(highlights, analysis)| {
                            // Pre-warm the analyzer cache with the Analysis
                            // this scan already computed, so opening this
                            // demo in the Demo Analyzer afterward hits the
                            // cache path instead of re-parsing.
                            // Best-effort/silent, and safe under concurrency:
                            // keyed per demo path, not one shared cache.
                            native::warm_analyzer_cache(file, &analysis);
                            serialized_demo(
                                file,
                                file_name.clone(),
                                file_keys[idx].clone(),
                                highlights,
                                &analysis,
                            )
                        });

                        if serialized.is_some() {
                            found.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                        slots.lock().unwrap_or_else(|p| p.into_inner())[idx] = serialized;

                        let was_cached = plan.fresh[idx];
                        if !was_cached {
                            parsed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            bytes_parsed
                                .fetch_add(plan.sizes[idx], std::sync::atomic::Ordering::Relaxed);
                        }
                        let done = completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                        let still_running = {
                            let mut running = in_flight.lock().unwrap_or_else(|p| p.into_inner());
                            running.retain(|&i| i != idx);
                            running.first().copied()
                        };
                        let current = still_running.map(file_name_at);
                        // A parse takes seconds, so each one reports; so does
                        // the last cached demo, which the throttle could
                        // otherwise hold back until the first parse ends.
                        emit_progress(done, current.as_deref(), !was_cached || done == cached);
                    }
                });
            }
        });

        let results: Vec<SerializedDemo> = slots
            .into_inner()
            .unwrap_or_else(|p| p.into_inner())
            .into_iter()
            .flatten()
            .collect();
        let was_cancelled = cancel_token.load(std::sync::atomic::Ordering::SeqCst);
        let skipped = skipped_demo_rows(skipped.into_inner().unwrap_or_else(|p| p.into_inner()));

        // ── Final progress event (complete or cancelled) ────────────────────
        let _ = app_handle.emit(
            "scan_progress",
            serde_json::json!({
                "scanned": completed.load(std::sync::atomic::Ordering::Relaxed),
                "found": results.len() as u32,
                "status": if was_cancelled { "Cancelled" } else { "Complete" },
                "cancelled": was_cancelled,
                "unchanged": unchanged,
                "skipped": skipped
            }),
        );

        Ok(ScanOutcome {
            demos: results,
            unchanged,
            copies,
            cancelled: was_cancelled,
            cached,
            not_cached: parsed.load(std::sync::atomic::Ordering::Relaxed),
            not_cached_ms: first_parse
                .get()
                .map_or(0, |t| t.elapsed().as_millis() as u64),
            full_reads: full_reads.load(std::sync::atomic::Ordering::Relaxed),
            full_read_bytes: full_read_bytes.load(std::sync::atomic::Ordering::Relaxed),
            cap,
        })
    }))
    .await;

    is_scanning_end.store(false, std::sync::atomic::Ordering::SeqCst);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    #[test]
    fn a_skipped_demo_gets_a_plain_reason_from_its_contents() {
        let dir = Scratch::new("plain_scan_failure");
        let short = dir.path().join("short.dem");
        let text = dir.path().join("text.dem");
        let corrupt = dir.path().join("corrupt.dem");
        std::fs::write(&short, b"hello").unwrap();
        std::fs::write(&text, vec![b'x'; 2048]).unwrap();
        let mut demo = b"HLDEMO\0\0".to_vec();
        demo.resize(2048, 0xAB);
        std::fs::write(&corrupt, demo).unwrap();

        let r = "Failed to read demo header: failed to fill whole buffer";
        assert_eq!(
            plain_scan_failure(&short, r),
            crate::messages::SCAN_FAIL_TOO_SHORT
        );
        assert_eq!(
            plain_scan_failure(&text, "Failed to parse demo: x"),
            crate::messages::SCAN_FAIL_NOT_A_DEMO
        );
        assert_eq!(
            plain_scan_failure(&corrupt, "Failed to parse demo: x"),
            crate::messages::SCAN_FAIL_CORRUPT
        );
        let io = "Failed to read file: Access is denied. (os error 5)";
        assert_eq!(plain_scan_failure(&corrupt, io), io);
    }

    #[test]
    fn skipped_demos_are_reported_in_file_order_with_their_reason() {
        let rows = skipped_demo_rows(vec![
            (5, "b.dem".into(), "Failed to parse demo: eof".into()),
            (2, "a.dem".into(), "Failed to read file: denied".into()),
        ]);
        assert_eq!(rows[0]["name"], "a.dem");
        assert_eq!(rows[0]["reason"], "Failed to read file: denied");
        assert_eq!(rows[1]["name"], "b.dem");
    }

    #[test]
    fn the_scan_uses_the_worker_setting_within_its_range() {
        assert_eq!(scan_worker_count(5, 100), 5);
        assert_eq!(scan_worker_count(0, 100), 1);
        assert_eq!(scan_worker_count(20, 100), SCAN_WORKERS_MAX);
        // Never more threads than demos.
        assert_eq!(scan_worker_count(8, 3), 3);
        assert_eq!(scan_worker_count(4, 0), 1);
    }

    #[test]
    fn the_scan_status_line_names_a_demo_only_while_one_is_being_parsed() {
        assert_eq!(
            scan_status_line(0, 1, 0, Some("a.dem")),
            "Reading demos: 0 of 1, a.dem"
        );
        assert_eq!(scan_status_line(1, 1, 0, None), "Reading demos: 1 of 1");
        assert_eq!(
            scan_status_line(3, 10, 2, Some("a.dem")),
            "Reading demos: 3 of 10 (2 from the analyzer cache), a.dem"
        );
    }

    #[test]
    fn scan_progress_is_throttled_to_one_line_per_33_ms() {
        let last = std::sync::atomic::AtomicU32::new(u32::MAX);
        assert!(
            progress_emit_due(&last, 0),
            "the first line always goes out"
        );
        assert!(!progress_emit_due(&last, 20));
        assert!(progress_emit_due(&last, 33));
        assert!(!progress_emit_due(&last, 40));
    }

    #[test]
    fn the_listing_line_counts_folders_and_demos() {
        assert_eq!(
            listing_status_line(12, 3),
            "Looking for demos: 12 folders read, 3 demos found"
        );
    }

    /// A drive-like tree: two demos to find, and deleted ones in the
    /// Recycle Bin and System Volume Information.
    fn drive_like(scratch: &Scratch) -> PathBuf {
        let root = scratch.path().to_path_buf();
        for dir in [
            "games/dod",
            "$RECYCLE.BIN/S-1-5-21",
            "System Volume Information",
        ] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for f in [
            "b.dem",
            "games/dod/a.dem",
            "$RECYCLE.BIN/S-1-5-21/$R1A2B3C.dem",
            "System Volume Information/c.dem",
        ] {
            std::fs::write(root.join(f), b"").unwrap();
        }
        root
    }

    #[test]
    fn listing_finds_demos_in_name_order_and_skips_system_folders() {
        let scratch = Scratch::new("list_scan_demos");
        let root = drive_like(&scratch);
        let mut list = Vec::new();
        let mut last = (0, 0);
        let listing = list_scan_demos(
            std::slice::from_ref(&root),
            true,
            &mut list,
            &std::sync::atomic::AtomicBool::new(false),
            &mut |folders, demos| last = (folders, demos),
        );
        assert!(listing.listed);
        assert!(!listing.cap.hit());
        assert_eq!(list, [root.join("games/dod/a.dem"), root.join("b.dem")]);
        // The root, games and games/dod; the two system folders not counted.
        assert_eq!(last, (3, 2));
    }

    #[test]
    fn cancel_stops_the_listing_and_says_so() {
        let scratch = Scratch::new("list_scan_cancel");
        let root = drive_like(&scratch);
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut list = Vec::new();
        let mut calls = 0;
        let listing = list_scan_demos(
            std::slice::from_ref(&root),
            true,
            &mut list,
            &cancel,
            &mut |_, _| {
                calls += 1;
                // The first call is the start line; cancel on the next.
                if calls == 2 {
                    cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            },
        );
        assert!(!listing.listed);
        assert!(!listing.cap.hit(), "a cancel is not a cap");
        assert_eq!(calls, 2, "nothing more is read once cancelled");

        // Cancelled before it starts: nothing is listed.
        let mut list = Vec::new();
        assert!(
            !list_scan_demos(
                std::slice::from_ref(&root),
                true,
                &mut list,
                &cancel,
                &mut |_, _| {}
            )
            .listed
        );
        assert!(list.is_empty());
    }

    /// Lists `dirs` with nothing cancelled; the demos relative to `root`.
    fn list_rel(root: &Path, dirs: &[PathBuf], subfolders: bool) -> (Vec<String>, Listing) {
        let mut list = Vec::new();
        let listing = list_scan_demos(
            dirs,
            subfolders,
            &mut list,
            &std::sync::atomic::AtomicBool::new(false),
            &mut |_, _| {},
        );
        let mut rel: Vec<String> = list
            .iter()
            .map(|p| {
                p.strip_prefix(root)
                    .unwrap_or(p)
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/")
            })
            .collect();
        rel.sort();
        (rel, listing)
    }

    #[test]
    fn without_subfolders_only_the_folders_own_demos_are_listed() {
        let scratch = Scratch::new("list_no_subfolders");
        let root = drive_like(&scratch);
        std::fs::write(root.join("UPPER.DEM"), b"").unwrap();
        let (demos, listing) = list_rel(&root, std::slice::from_ref(&root), false);
        assert_eq!(demos, ["UPPER.DEM", "b.dem"]);
        assert!(listing.listed);
        assert!(!listing.cap.hit(), "no subfolders is a choice, not a cap");
    }

    #[test]
    fn an_upper_case_dem_is_listed() {
        let scratch = Scratch::new("list_upper_dem");
        let root = scratch.path().to_path_buf();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/MATCH.DEM"), b"").unwrap();
        std::fs::write(root.join("Final.Dem"), b"").unwrap();
        let (demos, _) = list_rel(&root, std::slice::from_ref(&root), true);
        assert_eq!(demos, ["Final.Dem", "sub/MATCH.DEM"]);
    }

    #[test]
    fn the_depth_cap_stops_the_walk_and_says_so() {
        let scratch = Scratch::new("list_depth_cap");
        let root = scratch.path().to_path_buf();
        // d1/.../d6 is SCAN_MAX_DEPTH (6) levels down: its demo is not read.
        let mut deep = root.clone();
        for level in 1..=SCAN_MAX_DEPTH {
            deep = deep.join(format!("d{level}"));
            std::fs::create_dir_all(&deep).unwrap();
            std::fs::write(deep.join(format!("at{level}.dem")), b"").unwrap();
        }
        let (demos, listing) = list_rel(&root, std::slice::from_ref(&root), true);
        assert_eq!(demos.len(), SCAN_MAX_DEPTH - 1, "{demos:?}");
        assert!(demos.iter().all(|d| !d.ends_with("at6.dem")));
        assert!(listing.listed);
        assert!(listing.cap.depth);
        assert!(!listing.cap.entries);
        assert_eq!(listing.cap.max_depth, 6);
        assert_eq!(
            scan_cap_log_line(&listing.cap),
            "Scan: some demos may be missing: folders more than 6 levels down were not read. Pick a folder further down to scan the rest."
        );
    }

    #[test]
    fn a_folder_picked_twice_or_inside_another_is_listed_once() {
        let scratch = Scratch::new("list_twice");
        let root = drive_like(&scratch);
        let dod = root.join("games/dod");
        let upper = PathBuf::from(root.to_string_lossy().to_uppercase());
        let (demos, _) = list_rel(&root, &[root.clone(), dod, upper], true);
        assert_eq!(demos, ["b.dem", "games/dod/a.dem"]);
    }

    #[test]
    fn visited_folders_key_each_folder_by_where_it_really_is() {
        // `L:/link` is a junction to `D:/demos`: both roots key the same,
        // so the second is not walked, and a folder below either is one.
        let (mut visited, walk) = VisitedFolders::with_keys(vec![
            (PathBuf::from("D:/demos"), "d:/demos".into()),
            (PathBuf::from("L:/link"), "d:/demos".into()),
            (PathBuf::from("D:/demos/sub"), "d:/demos/sub".into()),
        ]);
        assert_eq!(
            walk,
            [PathBuf::from("D:/demos"), PathBuf::from("D:/demos/sub")]
        );
        // Another root, met inside the first: walked as a root of its own.
        assert!(!visited.first_visit(Path::new("D:/demos/sub")));
        assert!(visited.first_visit(Path::new("D:/demos/sub/2024")));
        assert!(!visited.first_visit(Path::new("D:/Demos/SUB/2024")));
        assert!(visited.first_visit(Path::new("D:/demos/other")));
        assert!(!visited.first_visit(Path::new("D:/demos/other/")));
    }

    /// A junction loop (`sub/loop` → the root) and the same folder picked
    /// through a junction: every demo once, and the walk ends.
    #[cfg(windows)]
    #[test]
    fn a_junction_loop_or_a_junction_root_lists_every_demo_once() {
        let scratch = Scratch::new("list_junction");
        let root = drive_like(&scratch);
        let link = root.join("games/loop");
        native::sys::junction::create(&link, &root).unwrap();
        let (demos, listing) = list_rel(&root, &[root.clone(), link.clone()], true);
        let _ = native::sys::junction::remove(&link);
        assert_eq!(demos, ["b.dem", "games/dod/a.dem"]);
        assert!(!listing.cap.hit());
    }

    #[test]
    fn a_known_unchanged_demo_is_skipped_and_a_changed_one_is_not() {
        let scratch = Scratch::new("skip_known");
        let a = scratch.path().join("a.dem");
        let b = scratch.path().join("b.dem");
        let c = scratch.path().join("c.dem");
        std::fs::write(&a, b"HLDEMO aaaa").unwrap();
        std::fs::write(&b, b"HLDEMO bbbb").unwrap();
        std::fs::write(&c, b"HLDEMO cccc").unwrap();
        let key_b_before = demo_file_key(&b).unwrap();
        std::fs::write(&b, b"HLDEMO bbbb, re-recorded").unwrap();

        let known = vec![
            // Same file, different case and slashes: still a match.
            KnownDemo {
                path: a.to_string_lossy().to_uppercase().replace('\\', "/"),
                file_key: demo_file_key(&a).unwrap(),
            },
            // Changed on disk since it was queued: scanned again.
            KnownDemo {
                path: b.to_string_lossy().into_owned(),
                file_key: key_b_before,
            },
        ];
        let mut reported = Vec::new();
        let (keep, keys, unchanged, copies) =
            skip_known(vec![a, b.clone(), c.clone()], &known, &mut |done, total| {
                reported.push((done, total))
            });

        // Each key read is reported, so the queue's line moves while a big
        // folder's keys are read.
        assert_eq!(reported, [(1, 3), (2, 3), (3, 3)]);
        assert_eq!(unchanged, 1);
        assert_eq!(keep, vec![b.clone(), c.clone()]);
        assert_eq!(keys, vec![demo_file_key(&b), demo_file_key(&c)]);
        assert!(copies.is_empty());
    }

    /// A copy of a queued demo, and a second copy within the scan, are
    /// reported without being kept for parsing. Within the scan the shorter
    /// name is kept even though it sorts after its copy.
    #[test]
    fn identical_copies_are_skipped_before_parsing() {
        let scratch = Scratch::new("skip_copies");
        let queued = scratch.path().join("queued.dem");
        let copy = scratch.path().join("queued copy.dem");
        let fresh = scratch.path().join("fresh.dem");
        let fresh_copy = scratch.path().join("fresh - Copy.dem");
        std::fs::write(&queued, b"HLDEMO the queued one").unwrap();
        std::fs::write(&copy, b"HLDEMO the queued one").unwrap();
        std::fs::write(&fresh, b"HLDEMO a new one").unwrap();
        std::fs::write(&fresh_copy, b"HLDEMO a new one").unwrap();
        let known = vec![KnownDemo {
            path: queued.to_string_lossy().into_owned(),
            file_key: demo_file_key(&queued).unwrap(),
        }];

        let (keep, _, unchanged, copies) = skip_known(
            vec![
                // "fresh - Copy.dem" sorts first (' ' < '.'), as the scan's
                // list is.
                copy.clone(),
                fresh_copy.clone(),
                fresh.clone(),
                queued.clone(),
            ],
            &known,
            &mut |_, _| {},
        );

        assert_eq!(keep, vec![fresh.clone()]);
        assert_eq!(unchanged, 1);
        assert_eq!(
            copies,
            vec![
                ScanCopy {
                    path: copy.to_string_lossy().into_owned(),
                    same_as: queued.to_string_lossy().into_owned(),
                    queued: true,
                },
                ScanCopy {
                    path: fresh_copy.to_string_lossy().into_owned(),
                    same_as: fresh.to_string_lossy().into_owned(),
                    queued: false,
                },
            ]
        );
    }

    #[test]
    fn a_moved_demo_is_found_by_its_key_in_a_subfolder() {
        let scratch = Scratch::new("locate_moved");
        let old = scratch.path().join("old");
        let new = scratch.path().join("archive").join("2026");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        let moved = old.join("match.dem");
        let kept = old.join("kept.dem");
        std::fs::write(&moved, b"HLDEMO the moved one").unwrap();
        std::fs::write(&kept, b"HLDEMO still here").unwrap();
        let key = demo_file_key(&moved).unwrap();
        std::fs::rename(&moved, new.join("renamed.dem")).unwrap();
        // Same size, different content: must not be taken for it.
        std::fs::write(new.join("decoy.dem"), b"HLDEMO the moved two").unwrap();

        let demos = vec![
            KnownDemo {
                path: moved.to_string_lossy().into_owned(),
                file_key: key,
            },
            KnownDemo {
                path: kept.to_string_lossy().into_owned(),
                file_key: demo_file_key(&kept).unwrap(),
            },
        ];
        let mut searched = Vec::new();
        let found = locate_missing_demos(&demos, &[scratch.path().to_path_buf()], &mut |n| {
            searched.push(n)
        });

        assert_eq!(found.len(), 1, "only the missing demo is reported");
        // The page is told once, before the walk, how many it looks for.
        assert_eq!(searched, vec![1]);
        assert_eq!(found[0].path, moved.to_string_lossy());
        assert_eq!(
            found[0].candidate.as_deref(),
            Some(new.join("renamed.dem").to_string_lossy().as_ref())
        );
    }

    /// A big folder listed first (a whole `dod/`) must not use up the entry
    /// budget before a demo one folder down from its old place is reached:
    /// the live failure that broke #477's test.
    #[test]
    fn a_nearby_move_is_found_even_behind_a_big_folder() {
        let scratch = Scratch::new("locate_budget");
        let big = scratch.path().join("big");
        let mut deep = big.clone();
        for level in 0..4 {
            deep = deep.join(format!("level{level}"));
            std::fs::create_dir_all(&deep).unwrap();
            for i in 0..20 {
                std::fs::write(deep.join(format!("f{i}.txt")), b"x").unwrap();
            }
        }
        let project = scratch.path().join("project");
        std::fs::create_dir_all(project.join("subfolder")).unwrap();
        let moved = project.join("match.dem");
        std::fs::write(&moved, b"HLDEMO moved one level down").unwrap();
        let key = demo_file_key(&moved).unwrap();
        std::fs::rename(&moved, project.join("subfolder").join("match.dem")).unwrap();

        let demos = vec![KnownDemo {
            path: moved.to_string_lossy().into_owned(),
            file_key: key,
        }];
        // 30 entries: the big tree has 84, the project folder 1, then its
        // subfolder 1. Depth-first from `big` spent them all first.
        let found = locate_missing_demos_within(&demos, &[project.clone(), big], 30, &mut |_| {});
        assert_eq!(
            found[0].candidate.as_deref(),
            Some(
                project
                    .join("subfolder")
                    .join("match.dem")
                    .to_string_lossy()
                    .as_ref()
            )
        );
    }

    #[test]
    fn a_demo_replaced_in_place_is_changed_and_an_untouched_one_is_not() {
        let scratch = Scratch::new("changed_demos");
        let same = scratch.path().join("same.dem");
        let replaced = scratch.path().join("replaced.dem");
        std::fs::write(&same, b"HLDEMO the scanned file").unwrap();
        std::fs::write(&replaced, b"HLDEMO the scanned file, too").unwrap();
        let demos = vec![
            KnownDemo {
                path: same.to_string_lossy().into_owned(),
                file_key: demo_file_key(&same).unwrap(),
            },
            KnownDemo {
                path: replaced.to_string_lossy().into_owned(),
                file_key: demo_file_key(&replaced).unwrap(),
            },
            KnownDemo {
                path: scratch
                    .path()
                    .join("gone.dem")
                    .to_string_lossy()
                    .into_owned(),
                file_key: "1-00".into(),
            },
        ];
        std::fs::write(&replaced, b"HLDEMO a different, shorter one").unwrap();
        assert_eq!(
            changed_demos_with_progress(&demos, |_, _| {}),
            vec![replaced.to_string_lossy().into_owned()]
        );
    }

    #[test]
    fn the_changed_demo_check_counts_every_demo_it_reads() {
        let scratch = Scratch::new("changed_count");
        let same = scratch.path().join("same.dem");
        std::fs::write(&same, b"HLDEMO the scanned one").unwrap();
        let demos = vec![
            KnownDemo {
                path: same.to_string_lossy().into_owned(),
                file_key: demo_file_key(&same).unwrap(),
            },
            KnownDemo {
                path: scratch
                    .path()
                    .join("gone.dem")
                    .to_string_lossy()
                    .into_owned(),
                file_key: "1-00".into(),
            },
        ];
        let mut seen = Vec::new();
        let changed = changed_demos_with_progress(&demos, |done, total| seen.push((done, total)));
        assert!(changed.is_empty());
        assert_eq!(seen, vec![(1, 2), (2, 2)]);
    }

    #[test]
    fn the_changed_demo_count_is_throttled_but_always_ends_on_the_last() {
        use std::sync::atomic::AtomicU32;
        let last = AtomicU32::new(u32::MAX);
        assert!(changed_progress_due(&last, 0, 1, 10));
        assert!(!changed_progress_due(&last, 10, 2, 10));
        assert!(!changed_progress_due(&last, 32, 3, 10));
        assert!(changed_progress_due(&last, 33, 4, 10));
        // The last demo goes out however soon after the one before.
        assert!(changed_progress_due(&last, 34, 10, 10));
    }

    #[test]
    fn a_missing_demo_with_no_key_or_no_match_is_reported_without_a_candidate() {
        let scratch = Scratch::new("locate_none");
        std::fs::write(scratch.path().join("other.dem"), b"HLDEMO other").unwrap();
        let gone = scratch.path().join("gone.dem");
        let demos = vec![
            KnownDemo {
                path: gone.to_string_lossy().into_owned(),
                file_key: String::new(),
            },
            KnownDemo {
                path: scratch
                    .path()
                    .join("gone2.dem")
                    .to_string_lossy()
                    .into_owned(),
                file_key: "999-0000000000000000".to_string(),
            },
        ];
        let mut searched = Vec::new();
        let found = locate_missing_demos(&demos, &[scratch.path().to_path_buf()], &mut |n| {
            searched.push(n)
        });
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|m| m.candidate.is_none()), "{found:?}");
        // Only the demo with a key can be looked for (#958).
        assert_eq!(searched, vec![1]);
    }

    #[test]
    fn nothing_missing_means_no_search_to_report() {
        let scratch = Scratch::new("locate_present");
        let here = scratch.path().join("here.dem");
        std::fs::write(&here, b"HLDEMO here").unwrap();
        let demos = vec![KnownDemo {
            path: here.to_string_lossy().into_owned(),
            file_key: demo_file_key(&here).unwrap(),
        }];
        let mut searched = Vec::new();
        let found = locate_missing_demos(&demos, &[scratch.path().to_path_buf()], &mut |n| {
            searched.push(n)
        });
        assert!(found.is_empty());
        assert!(searched.is_empty());
    }

    #[test]
    fn missing_scan_paths_names_only_the_paths_that_are_gone() {
        let scratch = Scratch::new("missing_scan_paths");
        let here = scratch.path().to_string_lossy().to_string();
        let gone = scratch.path().join("deleted").to_string_lossy().to_string();
        assert_eq!(missing_scan_paths(&[here, gone.clone()]), vec![gone]);
    }

    fn fixture_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-fixtures/ci_fixture.dem")
    }

    #[test]
    fn a_scanned_demo_carries_the_current_highlights_version() {
        let path = fixture_path();
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let analysis = ::analysis::Analysis::try_from_bytes(&bytes).unwrap();
        let highlights = (
            100.0,
            Vec::new(),
            true,
            None,
            0,
            None,
            std::sync::Arc::new(Vec::new()),
        );
        let demo = serialized_demo(&path, "ci_fixture.dem".into(), None, highlights, &analysis);
        assert_eq!(
            demo.highlights_version,
            ::analysis::highlights::HIGHLIGHTS_VERSION
        );
        assert!(
            demo.highlights_version > 0,
            "0 means 'scanned before it was recorded'"
        );
        let json = serde_json::to_value(&demo).unwrap();
        assert_eq!(
            json["highlights_version"],
            ::analysis::highlights::HIGHLIGHTS_VERSION
        );
    }

    #[test]
    fn a_demo_from_an_older_project_has_highlights_version_0() {
        let old = r#"{"path":"D:/demos/a.dem","name":"a.dem","tickrate":100.0,"is_pov":true,
            "local_player_index":null,"playback_frames":10,"streaks":[]}"#;
        let demo: SerializedDemo = serde_json::from_str(old).unwrap();
        assert_eq!(demo.highlights_version, 0);
    }

    #[test]
    fn the_frontend_mirrors_the_highlights_version() {
        let js = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/scan_version.js"),
        )
        .unwrap();
        let line = format!(
            "export const HIGHLIGHTS_VERSION = {};",
            ::analysis::highlights::HIGHLIGHTS_VERSION
        );
        assert!(
            js.contains(&line),
            "studio/src/scan_version.js must say `{line}`"
        );
    }
}
