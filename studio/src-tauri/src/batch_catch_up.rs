// batch_catch_up.rs
// Catching up on a capture batch Studio stopped watching (#545, part 2).
//
// A batch keeps running after Studio closes (the hook holds the schedule), but
// the end-of-batch take check that marks highlights Captured ran in Studio.
// Such a batch leaves its manifest at outcome "planned". On the next start,
// Studio checks those batches' takes on disk the same way a live batch does at
// its end and hands the frontend what it found; `batch_catch_up.js` marks the
// highlights. The manifest is then rewritten as "unwatched", so a batch is
// caught up on once.
//
// While a game is running, a planned batch may still be going, so the check
// waits until no game is left. Reconnecting to that game's events pipe to
// resume live progress is not done: the batch's progress state lived in the
// Studio that closed.

use crate::capture_manager::{
    CaptureManifest, VerifiedBlock, record_capture_settings, verify_capture_takes_settling,
};
use native::log_markdown;
use serde::Serialize;
use std::path::Path;
use std::time::Duration;
use tauri::Emitter;

/// The outcome a caught-up batch's manifest is rewritten with.
const OUTCOME_UNWATCHED: &str = "unwatched";

/// How often the wait for a running game to close looks again. Only a
/// process staying alive is watched, so it need not be quick.
const GAME_POLL: Duration = Duration::from_millis(2000);

/// The event the deferred check sends once the game has closed.
pub const CATCH_UP_EVENT: &str = "batch_catch_up";

/// One caught-up batch, for the frontend to map back to highlights.
#[derive(Debug, Clone, Serialize)]
pub struct CaughtUpBatch {
    pub session_id: String,
    /// The project file the batch was started from, when it had one.
    pub project_path: Option<String>,
    /// Index-aligned with the batch's payload, as each block's
    /// `source_streak_indices` count.
    pub streak_uids: Vec<String>,
    pub streak_sources: Vec<String>,
    pub blocks: Vec<VerifiedBlock>,
}

/// What the startup check found.
#[derive(Debug, Clone, Serialize)]
pub struct CatchUpStart {
    /// Batches checked now.
    pub batches: Vec<CaughtUpBatch>,
    /// A game was running, so the remaining batches are checked once it
    /// closes and arrive as a [`CATCH_UP_EVENT`].
    pub waiting_for_game: bool,
}

/// Planned manifests in `dir` that can be mapped back to highlights. One
/// written before the highlight ids existed cannot, and is left alone.
fn catchable_in(dir: &Path) -> Vec<CaptureManifest> {
    crate::manifest_file::read_planned_in(dir)
        .into_iter()
        .filter(|m| !m.streak_uids.is_empty() && !m.blocks.is_empty())
        .collect()
}

/// Checks `manifest`'s takes on disk, records the capture settings beside
/// them as a watched batch does, and rewrites its manifest as unwatched.
fn catch_up_one(dir: &Path, manifest: CaptureManifest) -> CaughtUpBatch {
    // The batch ended long ago: no take is still being flushed.
    let blocks = verify_capture_takes_settling(&manifest, None, &mut |_| {});
    let captured = blocks.iter().filter(|b| b.captured).count();
    log_markdown(&format!(
        "[batch-catch-up] session {} ran while Studio was closed: {}/{} takes on disk",
        manifest.session_id,
        captured,
        blocks.len()
    ));
    record_capture_settings(&manifest, &blocks);
    crate::manifest_file::write_in(dir, &manifest, OUTCOME_UNWATCHED, Some(&blocks));
    CaughtUpBatch {
        session_id: manifest.session_id,
        project_path: manifest.project_path,
        streak_uids: manifest.streak_uids,
        streak_sources: manifest.streak_sources,
        blocks,
    }
}

/// Catches up on the planned batches in `dir`; with `only`, just those
/// sessions.
fn catch_up_in(dir: &Path, only: Option<&[String]>) -> Vec<CaughtUpBatch> {
    catchable_in(dir)
        .into_iter()
        .filter(|m| only.is_none_or(|ids| ids.contains(&m.session_id)))
        .map(|m| catch_up_one(dir, m))
        .collect()
}

/// Run once when Studio starts, before any batch of its own. With no game
/// running, every planned batch is checked now. With one running, the
/// sessions planned at this moment are checked once no game is left, so a
/// batch this Studio starts in the meantime is never mistaken for one.
#[tauri::command]
pub async fn catch_up_unwatched_batches(app: tauri::AppHandle) -> Result<CatchUpStart, String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let dir = crate::manifest_file::manifests_dir();
        if crate::capture_manager::running_game_pids().is_empty() {
            return Ok(CatchUpStart {
                batches: catch_up_in(&dir, None),
                waiting_for_game: false,
            });
        }
        let ids: Vec<String> = catchable_in(&dir)
            .into_iter()
            .map(|m| m.session_id)
            .collect();
        if ids.is_empty() {
            return Ok(CatchUpStart {
                batches: Vec::new(),
                waiting_for_game: false,
            });
        }
        log_markdown(&format!(
            "[batch-catch-up] {} batch(es) ran while Studio was closed; checking their takes once the game closes",
            ids.len()
        ));
        std::thread::spawn(move || {
            while !crate::capture_manager::running_game_pids().is_empty() {
                std::thread::sleep(GAME_POLL);
            }
            let batches = catch_up_in(&dir, Some(&ids));
            if !batches.is_empty() {
                let _ = app.emit(CATCH_UP_EVENT, &batches);
            }
        });
        Ok(CatchUpStart {
            batches: Vec::new(),
            waiting_for_game: true,
        })
    }))
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;
    use native::patch::CaptureBlock;

    fn block(take_folder: &Path, index: usize, sources: Vec<usize>) -> CaptureBlock {
        CaptureBlock {
            demo_name: "dodstudio_chain_01".to_string(),
            block_index: index,
            drive_index: 0,
            take_folder: take_folder.to_path_buf(),
            take_key: format!("s/chain_01_b{index}"),
            source_streak_indices: sources,
            start_tick: 0,
            end_tick: 100,
            record_start_tick: 0,
            record_stop_tick: 100,
        }
    }

    fn manifest(session_id: &str, blocks: Vec<CaptureBlock>, uids: &[&str]) -> CaptureManifest {
        CaptureManifest {
            session_id: session_id.to_string(),
            blocks,
            capture_fps: 0,
            capture_mode: String::new(),
            agr_fps: 0,
            streak_uids: uids.iter().map(|s| s.to_string()).collect(),
            streak_sources: uids.iter().map(|_| "a.dem".to_string()).collect(),
            project_path: Some("p.json".to_string()),
        }
    }

    #[test]
    fn a_planned_batch_is_checked_on_disk_once() {
        let manifests = Scratch::new("catch_up_manifests");
        let takes = Scratch::new("catch_up_takes");
        let done = takes.path().join("b0");
        std::fs::create_dir_all(&done).unwrap();
        std::fs::write(done.join("00000.tga"), b"x").unwrap();
        let missing = takes.path().join("b1");

        let m = manifest(
            "session_a",
            vec![block(&done, 0, vec![0, 1]), block(&missing, 1, vec![2])],
            &["u0", "u1", "u2"],
        );
        crate::manifest_file::write_in(&manifests, &m, "planned", None).unwrap();

        let batches = catch_up_in(&manifests, None);
        assert_eq!(batches.len(), 1);
        let b = &batches[0];
        assert_eq!(b.session_id, "session_a");
        assert_eq!(b.project_path.as_deref(), Some("p.json"));
        assert_eq!(b.streak_uids, vec!["u0", "u1", "u2"]);
        let captured: Vec<bool> = b.blocks.iter().map(|v| v.captured).collect();
        assert_eq!(captured, vec![true, false]);

        // Rewritten as unwatched, so the next start leaves it alone.
        assert!(catch_up_in(&manifests, None).is_empty());
    }

    #[test]
    fn only_the_named_sessions_and_mappable_manifests_are_caught_up() {
        let manifests = Scratch::new("catch_up_only");
        let takes = Scratch::new("catch_up_only_takes");
        let folder = takes.path().join("b0");
        for id in ["session_a", "session_b"] {
            let m = manifest(id, vec![block(&folder, 0, vec![0])], &["u0"]);
            crate::manifest_file::write_in(&manifests, &m, "planned", None).unwrap();
        }
        // From before the highlight ids: nothing to map it back to.
        let old = manifest("session_old", vec![block(&folder, 0, vec![0])], &[]);
        crate::manifest_file::write_in(&manifests, &old, "planned", None).unwrap();

        let ids = vec!["session_b".to_string()];
        let batches = catch_up_in(&manifests, Some(&ids));
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].session_id, "session_b");

        let left: Vec<String> = crate::manifest_file::read_planned_in(&manifests)
            .into_iter()
            .map(|m| m.session_id)
            .collect();
        assert_eq!(left, vec!["session_a", "session_old"]);
    }
}
