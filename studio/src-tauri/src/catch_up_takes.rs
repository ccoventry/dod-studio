// catch_up_takes.rs
// Which takes of a batch nobody watched end are partial (#20).
//
// A watched batch knows from its markers which clip its end cut (#1081,
// #1124) and which OBS started late (#1109). A batch that ran on after
// Studio closed has only what is on disk, and a partial take there can look
// whole. Three things on disk still tell, one per kind of take:
//
// - The game's console log. Every launch passes `-condebug`, so the game
//   copies each `[dod-studio]` marker into `qconsole.log` beside `hl.exe`,
//   and only the end of a batch Studio watched clears it (Auto-clear Logs).
//   A `START_RECORD` with no `STOP_RECORD` after it is the clip the game was
//   recording when it closed, in any capture mode: the only sign an AGR
//   take has.
// - HLAE's `sound.wav`, whose size HLAE fills in when a recording stops
//   (`take_length::is_unfinished_take`), for when the log is gone.
// - OBS mode's take state (`obs::take_state`): Studio drives OBS, so a
//   recording Studio never stopped (it closed mid-clip, while the game went
//   on) and a late start are known only from what it wrote down.
//
// Each marks a take not captured, not renderable and left on disk, as a
// watched batch does.

use crate::capture_manager::{
    CaptureManifest, CutBy, Records, VerifiedBlock, demo_ordinals, mark_cut_short,
};
use native::log_markdown;
use native::obs::{Marker, MarkerKind};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// How much of the console log's end is read. A log nobody clears holds
/// every game session since (48 MB and 30 MB on two test installs,
/// measured 2026-10-30), and one batch's run took up to 5.8 MB of it, so this is over
/// ten times that.
const LOG_TAIL_BYTES: u64 = 64 * 1024 * 1024;

/// The last [`LOG_TAIL_BYTES`] of the console log at `path`, or `None` when
/// there is none.
pub(crate) fn read_console_log(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(LOG_TAIL_BYTES)))
        .ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// The clip recordings `manifest`'s batch made, as the markers in `log` say:
/// the last run in the log that is this batch's. `None` when none is.
pub(crate) fn records_in(manifest: &CaptureManifest, log: &str) -> Option<Records> {
    let markers: Vec<Marker> = log
        .lines()
        .filter_map(native::obs::log_tail::parse_marker)
        .collect();
    let demos = demo_ordinals(&manifest.blocks);
    // A run starts at its first demo's `DEMO_START`.
    let mut end = markers.len();
    for start in (0..markers.len()).rev() {
        if !is_first_demo(&markers[start]) {
            continue;
        }
        let run = &markers[start..end];
        end = start;
        if let Some(records) = replay(run, manifest, &demos) {
            return Some(records);
        }
    }
    None
}

fn is_first_demo(marker: &Marker) -> bool {
    marker.kind == MarkerKind::DemoStart && marker.demo_progress.is_some_and(|(job, _, _)| job == 1)
}

/// One run's recordings, when it is `manifest`'s batch: as many demos, at
/// least one clip, and every `START_RECORD` the start of one of its clips in
/// the demo playing. A run of another batch, a preview or someone's own game
/// fails one of those.
fn replay(run: &[Marker], manifest: &CaptureManifest, demos: &[u32]) -> Option<Records> {
    let (_, total, _) = run.first()?.demo_progress?;
    if Some(&total) != demos.last() {
        return None;
    }
    let is_clip_start = |demo: u32, tick: i64| {
        manifest
            .blocks
            .iter()
            .zip(demos)
            .any(|(b, &d)| d == demo && i64::from(b.record_start_tick) == tick)
    };
    let mut records = Records::default();
    let mut demo = 0;
    let mut starts = 0;
    for marker in run {
        match marker.kind {
            MarkerKind::DemoStart => {
                let (job, _, _) = marker.demo_progress?;
                demo = job;
                records.demo_started(job);
            }
            MarkerKind::StartRecord => {
                if !is_clip_start(demo, marker.tick?) {
                    return None;
                }
                starts += 1;
                records.clip_started(marker.tick);
            }
            MarkerKind::StopRecord => records.clip_stopped(),
            _ => {}
        }
    }
    (starts > 0).then_some(records)
}

/// Marks the clip the game was recording when it closed, as its console log
/// tells (`records_in`). Returns how many takes it marked (0 or 1).
pub(crate) fn apply_console_log(
    manifest: &CaptureManifest,
    blocks: &mut [VerifiedBlock],
    log: Option<&str>,
    log_path: &Path,
) -> usize {
    let Some(records) = log.and_then(|log| records_in(manifest, log)) else {
        log_markdown(&format!(
            "[batch-catch-up] session {}: the game's console log at {} has no run of this batch; \
             judging its takes by what they hold",
            manifest.session_id,
            log_path.display()
        ));
        return 0;
    };
    let captured: Vec<bool> = blocks.iter().map(|b| b.captured).collect();
    mark_cut_short(manifest, blocks, records, CutBy::GameClosed);
    let mut marked = 0;
    for (v, was) in blocks.iter().zip(captured) {
        if was && !v.captured {
            log_markdown(&format!(
                "[batch-catch-up] {} was recording when the game closed (its console log has no \
                 STOP_RECORD for it): not counted as captured. Its partial take is left at {}",
                v.take_key, v.take_folder
            ));
            marked += 1;
        }
    }
    marked
}

/// Marks each take HLAE never finished as cut short when the game closed. A
/// watched batch knows this from its markers (#1124); a batch nobody watched
/// end has only the take itself. Returns how many it marked.
pub(crate) fn apply_unfinished_takes(
    manifest: &CaptureManifest,
    blocks: &mut [VerifiedBlock],
) -> usize {
    let mut marked = 0;
    for (v, block) in blocks.iter_mut().zip(&manifest.blocks) {
        if !v.captured || v.rendered {
            continue;
        }
        if native::hlcr::take_length::is_unfinished_take(&block.take_folder) {
            v.captured = false;
            v.renderable = false;
            v.cut_short = true;
            v.cut_by = Some(CutBy::GameClosed);
            log_markdown(&format!(
                "[batch-catch-up] {} was cut short when the game closed: not counted as captured. Its partial take is left at {}",
                block.take_key,
                block.take_folder.display()
            ));
            marked += 1;
        }
    }
    marked
}

/// Marks each OBS take Studio never stopped, or that OBS started late, from
/// the state the session kept beside it (`obs::take_state`). Returns how
/// many it marked.
pub(crate) fn apply_obs_take_states(
    manifest: &CaptureManifest,
    blocks: &mut [VerifiedBlock],
) -> usize {
    let mut marked = 0;
    for (v, block) in blocks.iter_mut().zip(&manifest.blocks) {
        if !v.captured || v.rendered {
            continue;
        }
        let Some(state) = native::obs::take_state::read(&block.take_folder) else {
            continue;
        };
        if !state.recording && state.started_late_ms.is_none() {
            continue;
        }
        v.captured = false;
        v.renderable = false;
        v.started_late_ms = state.started_late_ms;
        if state.recording {
            v.cut_short = true;
            v.cut_by = Some(CutBy::StudioClosed);
        }
        let why = match state.started_late_ms {
            Some(ms) if !state.recording => format!(
                "OBS started recording {ms} ms after the clip's first frame, so the take is missing its opening"
            ),
            _ => format!(
                "{}, so nobody stopped the recording at the clip's end",
                CutBy::StudioClosed.log_text()
            ),
        };
        log_markdown(&format!(
            "[batch-catch-up] {} {why}: not counted as captured. The take is left at {}",
            block.take_key,
            block.take_folder.display()
        ));
        marked += 1;
    }
    marked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;
    use native::obs::take_state::{self, ObsTakeState};
    use native::patch::CaptureBlock;

    fn block(take_folder: &Path, demo: &str, index: usize, start: i32) -> CaptureBlock {
        CaptureBlock {
            demo_name: demo.to_string(),
            block_index: index,
            drive_index: 0,
            take_folder: take_folder.to_path_buf(),
            take_key: format!("s/{demo}_b{index}"),
            source_streak_indices: vec![index],
            start_tick: 0,
            end_tick: 100,
            record_start_tick: start,
            record_stop_tick: start + 100,
            ramped_gaps: 0,
            plan: None,
        }
    }

    fn manifest(blocks: Vec<CaptureBlock>) -> CaptureManifest {
        CaptureManifest {
            session_id: "session_a".to_string(),
            capture_fps: 0,
            capture_mode: "agr".to_string(),
            agr_fps: 0,
            streak_uids: blocks
                .iter()
                .map(|b| format!("u{}", b.block_index))
                .collect(),
            streak_sources: blocks.iter().map(|_| "a.dem".to_string()).collect(),
            project_path: None,
            blocks,
        }
    }

    /// Two demos: chain_01's clips start at ticks 900 and 2000, chain_02's at 500.
    fn two_demos(folder: &Path) -> CaptureManifest {
        manifest(vec![
            block(&folder.join("b0"), "chain_01", 0, 900),
            block(&folder.join("b1"), "chain_01", 1, 2000),
            block(&folder.join("b2"), "chain_02", 0, 500),
        ])
    }

    fn verified(manifest: &CaptureManifest) -> Vec<VerifiedBlock> {
        manifest
            .blocks
            .iter()
            .map(|b| VerifiedBlock {
                take_key: b.take_key.clone(),
                take_folder: b.take_folder.to_string_lossy().into_owned(),
                demo_name: b.demo_name.clone(),
                block_index: b.block_index,
                source_streak_indices: b.source_streak_indices.clone(),
                captured: true,
                renderable: true,
                rendered: false,
                bytes: 1,
                cut_short: false,
                cut_by: None,
                short_take: None,
                started_late_ms: None,
                ramped_gaps: 0,
            })
            .collect()
    }

    fn log(lines: &[&str]) -> String {
        lines
            .iter()
            .map(|l| format!("[dod-studio] {l}\r\n"))
            .collect()
    }

    #[test]
    fn the_clip_with_no_stop_is_the_one_the_game_was_recording() {
        let m = two_demos(Path::new("D:/takes"));
        let text = log(&[
            "DEMO_START 1 2 2",
            "START_RECORD - Tick 900",
            "STOP_RECORD - Tick 1000",
            "START_RECORD - Tick 2000",
        ]);
        let mut blocks = verified(&m);
        assert_eq!(
            apply_console_log(&m, &mut blocks, Some(&text), Path::new("q.log")),
            1
        );
        let captured: Vec<bool> = blocks.iter().map(|b| b.captured).collect();
        assert_eq!(captured, vec![true, false, true]);
        assert!(blocks[1].cut_short);
        assert_eq!(blocks[1].cut_by, Some(CutBy::GameClosed));
    }

    #[test]
    fn a_batch_the_game_finished_marks_nothing() {
        let m = two_demos(Path::new("D:/takes"));
        let text = log(&[
            "DEMO_START 1 2 2",
            "START_RECORD - Tick 900",
            "STOP_RECORD - Tick 1000",
            "START_RECORD - Tick 2000",
            "STOP_RECORD - Tick 2100",
            "DEMO_START 2 2 1",
            "START_RECORD - Tick 500",
            "STOP_RECORD - Tick 600",
            "BATCH_COMPLETE",
        ]);
        let mut blocks = verified(&m);
        assert_eq!(
            apply_console_log(&m, &mut blocks, Some(&text), Path::new("q.log")),
            0
        );
        assert!(blocks.iter().all(|b| b.captured));
    }

    /// The same tick in another demo is another clip: chain_02 has no clip at
    /// 900, so this run is some other batch's.
    #[test]
    fn a_run_of_another_batch_is_not_read_as_this_one() {
        let m = two_demos(Path::new("D:/takes"));
        let other = log(&[
            "DEMO_START 1 2 2",
            "DEMO_START 2 2 1",
            "START_RECORD - Tick 900",
        ]);
        assert!(records_in(&m, &other).is_none());
        // Another number of demos.
        let other = log(&["DEMO_START 1 3 2", "START_RECORD - Tick 900"]);
        assert!(records_in(&m, &other).is_none());
        // No clip started: nothing to go by.
        assert!(records_in(&m, &log(&["DEMO_START 1 2 2"])).is_none());
        assert!(records_in(&m, "").is_none());
    }

    /// A later run of another batch (one started in the same game) doesn't
    /// hide this batch's run before it, which that start cut short.
    #[test]
    fn the_last_run_that_is_this_batch_is_read() {
        let m = two_demos(Path::new("D:/takes"));
        let text = format!(
            "noise\n{}{}",
            log(&["DEMO_START 1 2 2", "START_RECORD - Tick 900"]),
            log(&[
                "DEMO_START 1 1 1",
                "START_RECORD - Tick 4242",
                "STOP_RECORD - Tick 4300"
            ]),
        );
        let mut blocks = verified(&m);
        apply_console_log(&m, &mut blocks, Some(&text), Path::new("q.log"));
        let captured: Vec<bool> = blocks.iter().map(|b| b.captured).collect();
        assert_eq!(captured, vec![false, true, true]);
    }

    #[test]
    fn without_a_log_nothing_is_marked() {
        let m = two_demos(Path::new("D:/takes"));
        let mut blocks = verified(&m);
        assert_eq!(
            apply_console_log(&m, &mut blocks, None, Path::new("q.log")),
            0
        );
        assert!(blocks.iter().all(|b| b.captured));
    }

    #[test]
    fn the_console_log_is_read_from_its_end() {
        let dir = Scratch::new("catch_up_console_tail");
        let path = dir.path().join("qconsole.log");
        assert_eq!(read_console_log(&path), None);
        std::fs::write(&path, "[dod-studio] DEMO_START 1 2 2\n").unwrap();
        assert_eq!(
            read_console_log(&path).as_deref(),
            Some("[dod-studio] DEMO_START 1 2 2\n")
        );
    }

    #[test]
    fn an_obs_take_studio_never_stopped_or_that_started_late_is_not_captured() {
        let takes = Scratch::new("catch_up_obs_state");
        let m = manifest(vec![
            block(&takes.path().join("b0"), "chain_01", 0, 900),
            block(&takes.path().join("b1"), "chain_01", 1, 2000),
            block(&takes.path().join("b2"), "chain_01", 2, 3000),
        ]);
        for b in &m.blocks {
            std::fs::create_dir_all(&b.take_folder).unwrap();
        }
        take_state::save(
            &m.blocks[0].take_folder,
            ObsTakeState {
                recording: false,
                started_late_ms: Some(605),
            },
        );
        take_state::save(
            &m.blocks[1].take_folder,
            ObsTakeState {
                recording: true,
                started_late_ms: None,
            },
        );
        let mut blocks = verified(&m);
        assert_eq!(apply_obs_take_states(&m, &mut blocks), 2);
        assert!(!blocks[0].captured && !blocks[0].renderable);
        assert_eq!(blocks[0].started_late_ms, Some(605));
        assert!(!blocks[0].cut_short);
        assert!(!blocks[1].captured && blocks[1].cut_short);
        assert_eq!(blocks[1].cut_by, Some(CutBy::StudioClosed));
        // No state file: a whole take.
        assert!(blocks[2].captured);
    }
}
