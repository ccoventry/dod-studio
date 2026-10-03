//! The Demo Auditor's renamer (#469): what each demo's name can be built
//! from, renaming in place, and a log that undoes a batch.
//!
//! The names themselves are built in the frontend (`studio/src/demo_rename.js`,
//! on #441's template engine). This side reads the facts, does the renames
//! and keeps the log, and refuses anything that isn't a plain rename within
//! the demo's own folder or that would replace a file.
//!
//! A rename keeps the demo's size and modified time, so a project that
//! pointed at the old name finds it again by its key (#21). The analyzer
//! cache is keyed on the path, so its entry moves with the demo.

use std::path::{Path, PathBuf};

use analysis::Team;
use analysis::{Analysis, TeamTag};
use serde::{Deserialize, Serialize};

/// Where the logs go under the app's data folder.
pub const LOG_FOLDER: &str = "demo_rename_logs";

/// What a demo's new name can be built from.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct DemoFacts {
    pub path: String,
    /// With `.dem`.
    pub file_name: String,
    pub map: Option<String>,
    pub modified_unix_secs: Option<u64>,
    /// `"pov"` or `"hltv"`.
    pub demo_type: String,
    /// The recording player, in a POV demo whose recorder was found.
    pub name: Option<String>,
    pub kills: Option<i32>,
    pub deaths: Option<i32>,
    /// The recording player's side: "Allies", "British" or "Axis".
    pub side: Option<String>,
    /// Each playing side's clan tag (#445).
    pub teams: Vec<TeamTag>,
    /// Why the demo couldn't be read. Its other facts are then empty.
    pub error: Option<String>,
}

/// One player, as much of them as the facts need.
#[derive(Debug, Clone, PartialEq)]
struct PlayerFacts<'a> {
    client_id: Option<u8>,
    name: &'a str,
    team: Option<Team>,
    kills: i32,
    deaths: i32,
}

/// The recording player: the one in the POV slot. `None` in an HLTV demo,
/// or when nobody connected holds that slot.
fn recorder<'a, 'b>(
    pov_index: Option<u8>,
    players: &'b [PlayerFacts<'a>],
) -> Option<&'b PlayerFacts<'a>> {
    let slot = pov_index?;
    players.iter().find(|p| p.client_id == Some(slot))
}

fn side_name(team: &Team, allies_are_british: bool) -> Option<&'static str> {
    match team {
        Team::Allies if allies_are_british => Some("British"),
        Team::Allies => Some("Allies"),
        Team::British => Some("British"),
        Team::Axis => Some("Axis"),
        Team::Spectators | Team::Unassigned => None,
    }
}

/// The facts from a demo's analysis.
pub fn facts_from(path: &Path, analysis: &Analysis) -> DemoFacts {
    let state = &analysis.state;
    let players: Vec<PlayerFacts> = state
        .players
        .iter()
        .map(|p| PlayerFacts {
            client_id: match p.connection {
                analysis::Connection::Connected { client_id } => Some(client_id),
                analysis::Connection::Disconnected => None,
            },
            name: &p.name,
            team: p.team.clone(),
            kills: p.stats.1,
            deaths: p.stats.2,
        })
        .collect();
    let hltv = analysis.demo_info.demo_type.eq_ignore_ascii_case("HLTV");
    let own = if hltv {
        None
    } else {
        recorder(state.pov_player_index, &players)
    };
    let map = Some(analysis.demo_info.map_name.clone()).filter(|m| !m.is_empty());
    DemoFacts {
        path: path.to_string_lossy().to_string(),
        file_name: file_name(path),
        map,
        modified_unix_secs: modified_unix_secs(path),
        demo_type: if hltv { "hltv" } else { "pov" }.to_string(),
        name: own.map(|p| p.name.to_string()).filter(|n| !n.is_empty()),
        kills: own.map(|p| p.kills),
        deaths: own.map(|p| p.deaths),
        side: own
            .and_then(|p| p.team.as_ref())
            .and_then(|t| side_name(t, state.allies_are_british))
            .map(str::to_string),
        teams: analysis::team_tags(state),
        error: None,
    }
}

/// The facts for one demo, from the analyzer cache or a parse.
pub fn facts(path: &Path) -> DemoFacts {
    match crate::run_analyzer_cached(&path.to_path_buf(), |_, _| {}) {
        Ok((_, analysis, _)) => facts_from(path, &analysis),
        Err(error) => DemoFacts {
            path: path.to_string_lossy().to_string(),
            file_name: file_name(path),
            modified_unix_secs: modified_unix_secs(path),
            error: Some(error),
            ..DemoFacts::default()
        },
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn modified_unix_secs(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

/// Every `.dem` under `folder`, subfolders too, sorted by path.
pub fn demos_in(folder: &Path) -> Vec<PathBuf> {
    let mut demos: Vec<PathBuf> = walkdir::WalkDir::new(folder)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .filter(|p| {
            p.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("dem"))
        })
        .collect();
    demos.sort();
    demos
}

/// One rename: full paths.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RenamePair {
    pub from: String,
    pub to: String,
}

/// A rename that didn't happen, and why.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RenameFailure {
    pub from: String,
    pub to: String,
    pub error: String,
}

/// What a batch (or its undo) did.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct RenameOutcome {
    pub renamed: Vec<RenamePair>,
    pub failed: Vec<RenameFailure>,
    /// The log that undoes this batch, when anything was renamed.
    pub log: Option<String>,
}

/// A batch's log file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RenameLog {
    created_unix_secs: u64,
    renames: Vec<RenamePair>,
    #[serde(default)]
    undone: bool,
}

/// The batch the Undo button would reverse.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UndoableBatch {
    pub log: String,
    pub count: usize,
    pub created_unix_secs: u64,
}

/// Characters Windows refuses in a file name, as `clip_name.js` refuses them.
const INVALID_CHARS: &[char] = &['\\', '/', ':', '*', '?', '"', '<', '>', '|'];

/// Why `pair` can't be done, or `None` when it can.
fn refusal(pair: &RenamePair) -> Option<String> {
    let from = Path::new(&pair.from);
    let to = Path::new(&pair.to);
    let new_name = to.file_name()?.to_string_lossy().to_string();
    if from.parent() != to.parent() {
        return Some(crate::messages::RENAME_NOT_IN_PLACE.to_string());
    }
    if new_name
        .chars()
        .any(|c| INVALID_CHARS.contains(&c) || c.is_control())
        || new_name.trim_end_matches(['.', ' ']) != new_name
        || !new_name.to_ascii_lowercase().ends_with(".dem")
        || new_name.len() <= ".dem".len()
    {
        return Some(crate::messages::rename_bad_name(&new_name));
    }
    if !from.is_file() {
        return Some(crate::messages::RENAME_SOURCE_GONE.to_string());
    }
    // A change of case alone names the same file on Windows, so "exists" is
    // true for it; that one is allowed.
    if to.exists() && !same_file(from, to) {
        return Some(crate::messages::RENAME_TARGET_EXISTS.to_string());
    }
    None
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Renames one demo, moving its analyzer cache entry with it.
fn rename_one(pair: &RenamePair) -> Result<(), String> {
    if let Some(why) = refusal(pair) {
        return Err(why);
    }
    let from = PathBuf::from(&pair.from);
    let to = PathBuf::from(&pair.to);
    let old_cache = crate::analyzer_cache_path(&from);
    std::fs::rename(&from, &to).map_err(|e| e.to_string())?;
    if let Some(old_cache) = old_cache {
        crate::move_analyzer_cache_entry(&old_cache, &to);
    }
    Ok(())
}

/// Renames each pair in order. Never replaces a file, never moves one to
/// another folder. A failure is reported and the rest carry on. What was
/// renamed is logged in `log_dir`, newest batch last.
pub fn apply(pairs: &[RenamePair], log_dir: &Path) -> RenameOutcome {
    let mut outcome = RenameOutcome::default();
    for pair in pairs.iter().filter(|p| p.from != p.to) {
        match rename_one(pair) {
            Ok(()) => outcome.renamed.push(pair.clone()),
            Err(error) => outcome.failed.push(RenameFailure {
                from: pair.from.clone(),
                to: pair.to.clone(),
                error,
            }),
        }
    }
    if !outcome.renamed.is_empty() {
        let log = RenameLog {
            created_unix_secs: now_unix_secs(),
            renames: outcome.renamed.clone(),
            undone: false,
        };
        outcome.log = write_log(log_dir, &log).map(|p| p.to_string_lossy().to_string());
    }
    outcome
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn write_log(log_dir: &Path, log: &RenameLog) -> Option<PathBuf> {
    std::fs::create_dir_all(log_dir).ok()?;
    // Seconds plus a counter, so two batches in one second keep both logs.
    let mut n = 0;
    let path = loop {
        let candidate = log_dir.join(format!("rename_{}_{n:03}.json", log.created_unix_secs));
        if !candidate.exists() {
            break candidate;
        }
        n += 1;
    };
    let json = serde_json::to_vec_pretty(log).ok()?;
    std::fs::write(&path, json).ok()?;
    Some(path)
}

/// The logs in `log_dir`, newest first (their names sort by time).
fn logs(log_dir: &Path) -> Vec<(PathBuf, RenameLog)> {
    let Ok(entries) = std::fs::read_dir(log_dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("rename_"))
                && p.extension().is_some_and(|e| e == "json")
        })
        .collect();
    paths.sort();
    paths.reverse();
    paths
        .into_iter()
        .filter_map(|p| {
            let log = serde_json::from_slice(&std::fs::read(&p).ok()?).ok()?;
            Some((p, log))
        })
        .collect()
}

/// The newest batch not undone yet.
pub fn undoable(log_dir: &Path) -> Option<UndoableBatch> {
    logs(log_dir)
        .into_iter()
        .find(|(_, log)| !log.undone)
        .map(|(path, log)| UndoableBatch {
            log: path.to_string_lossy().to_string(),
            count: log.renames.len(),
            created_unix_secs: log.created_unix_secs,
        })
}

/// Puts the newest batch not undone yet back, last rename first. A demo that
/// has since moved or been renamed again is reported and left alone. The log
/// is then marked undone, so the next Undo reaches the batch before it.
pub fn undo_last(log_dir: &Path) -> RenameOutcome {
    let Some((path, mut log)) = logs(log_dir).into_iter().find(|(_, log)| !log.undone) else {
        return RenameOutcome::default();
    };
    let mut outcome = RenameOutcome::default();
    for pair in log.renames.iter().rev() {
        let back = RenamePair {
            from: pair.to.clone(),
            to: pair.from.clone(),
        };
        match rename_one(&back) {
            Ok(()) => outcome.renamed.push(back),
            Err(error) => outcome.failed.push(RenameFailure {
                from: back.from,
                to: back.to,
                error,
            }),
        }
    }
    log.undone = true;
    if let Ok(json) = serde_json::to_vec_pretty(&log) {
        let _ = std::fs::write(&path, json);
    }
    outcome.log = Some(path.to_string_lossy().to_string());
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    fn player(client_id: Option<u8>, name: &str, team: Option<Team>) -> PlayerFacts<'_> {
        PlayerFacts {
            client_id,
            name,
            team,
            kills: 7,
            deaths: 3,
        }
    }

    #[test]
    fn the_recorder_is_the_player_in_the_pov_slot() {
        let players = [
            player(Some(1), "other", Some(Team::Axis)),
            player(None, "left", Some(Team::Allies)),
            player(Some(4), "me", Some(Team::Allies)),
        ];
        assert_eq!(recorder(Some(4), &players).unwrap().name, "me");
        // A slot nobody connected holds, and an HLTV demo's missing slot.
        assert_eq!(recorder(Some(9), &players), None);
        assert_eq!(recorder(None, &players), None);
    }

    #[test]
    fn allies_read_british_on_a_british_map() {
        assert_eq!(side_name(&Team::Allies, false), Some("Allies"));
        assert_eq!(side_name(&Team::Allies, true), Some("British"));
        assert_eq!(side_name(&Team::Axis, true), Some("Axis"));
        assert_eq!(side_name(&Team::Spectators, false), None);
    }

    fn pair(dir: &Path, from: &str, to: &str) -> RenamePair {
        RenamePair {
            from: dir.join(from).to_string_lossy().to_string(),
            to: dir.join(to).to_string_lossy().to_string(),
        }
    }

    #[test]
    fn a_batch_renames_logs_and_undoes() {
        let dir = Scratch::new("demo_rename_batch");
        let logs = dir.join("logs");
        std::fs::write(dir.join("a.dem"), b"aaaa").unwrap();
        std::fs::write(dir.join("b.dem"), b"bb").unwrap();

        let outcome = apply(
            &[
                pair(&dir, "a.dem", "me_v_them.dem"),
                pair(&dir, "b.dem", "b.dem"),
            ],
            &logs,
        );
        assert_eq!(outcome.renamed, vec![pair(&dir, "a.dem", "me_v_them.dem")]);
        assert!(outcome.failed.is_empty());
        assert_eq!(std::fs::read(dir.join("me_v_them.dem")).unwrap(), b"aaaa");
        assert!(!dir.join("a.dem").exists());

        let batch = undoable(&logs).unwrap();
        assert_eq!(batch.count, 1);
        assert_eq!(Some(batch.log.clone()), outcome.log);

        let undone = undo_last(&logs);
        assert_eq!(undone.renamed, vec![pair(&dir, "me_v_them.dem", "a.dem")]);
        assert!(dir.join("a.dem").exists());
        assert_eq!(undoable(&logs), None);
        // Nothing left to undo is not an error.
        assert_eq!(undo_last(&logs), RenameOutcome::default());
    }

    #[test]
    fn undo_reaches_the_batch_before_once_the_newest_is_undone() {
        let dir = Scratch::new("demo_rename_two_batches");
        let logs = dir.join("logs");
        std::fs::write(dir.join("a.dem"), b"a").unwrap();
        apply(&[pair(&dir, "a.dem", "b.dem")], &logs);
        apply(&[pair(&dir, "b.dem", "c.dem")], &logs);
        undo_last(&logs);
        assert!(dir.join("b.dem").exists());
        undo_last(&logs);
        assert!(dir.join("a.dem").exists());
    }

    #[test]
    fn never_replaces_a_file_or_leaves_the_folder() {
        let dir = Scratch::new("demo_rename_refusals");
        let logs = dir.join("logs");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.dem"), b"a").unwrap();
        std::fs::write(dir.join("taken.dem"), b"t").unwrap();

        let outcome = apply(
            &[
                pair(&dir, "a.dem", "taken.dem"),
                pair(&dir, "a.dem", "sub/a.dem"),
                pair(&dir, "a.dem", "a.txt"),
                pair(&dir, "a.dem", "what?.dem"),
                pair(&dir, "a.dem", "trailing.dem."),
                pair(&dir, "missing.dem", "x.dem"),
            ],
            &logs,
        );
        assert!(outcome.renamed.is_empty());
        assert_eq!(outcome.failed.len(), 6);
        assert_eq!(outcome.log, None);
        assert_eq!(std::fs::read(dir.join("taken.dem")).unwrap(), b"t");
        assert!(dir.join("a.dem").exists());
        assert_eq!(undoable(&logs), None);
    }

    #[test]
    fn a_change_of_case_alone_is_allowed() {
        let dir = Scratch::new("demo_rename_case");
        std::fs::write(dir.join("anzio.dem"), b"a").unwrap();
        let outcome = apply(&[pair(&dir, "anzio.dem", "Anzio.dem")], &dir.join("logs"));
        assert_eq!(outcome.failed, vec![]);
        let names: Vec<String> = std::fs::read_dir(&*dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".dem"))
            .collect();
        assert_eq!(names, vec!["Anzio.dem"]);
    }

    #[test]
    fn demos_are_found_in_subfolders_and_nothing_else_is() {
        let dir = Scratch::new("demo_rename_list");
        std::fs::create_dir_all(dir.join("half2")).unwrap();
        std::fs::write(dir.join("one.dem"), b"").unwrap();
        std::fs::write(dir.join("half2/two.DEM"), b"").unwrap();
        std::fs::write(dir.join("notes.txt"), b"").unwrap();
        let found: Vec<String> = demos_in(&dir).iter().map(|p| file_name(p)).collect();
        assert_eq!(found, vec!["two.DEM", "one.dem"]);
    }
}
