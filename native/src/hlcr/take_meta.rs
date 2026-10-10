//! What a take was actually captured at, recorded beside it.
//!
//! Render Studio's FPS is a per-job setting the user picks (the VirtualDub-style
//! model), and nothing ever connected it to the FPS the take was recorded at.
//! `-framerate` before the BMP input tells FFmpeg how to interpret the
//! sequence's timing, so a value disagreeing with the capture produces a wrong
//! computed duration and `-shortest` trims the audio against it: a 120fps take
//! rendered at 300 comes out 2.5x too fast, silently, and the render reports
//! success. Issue #14 has the full diagnosis — it was found
//! by ear, from a render that "sounds like a helicopter".
//!
//! There was no source of truth to check the render setting against. This is it:
//! one small file per take, written into its block folder after the batch has
//! been verified, recording the `mirv_movie_fps` it was produced at.
//!
//! Per take, not per session, so it travels with the take. Two batches at
//! different rates already land in different session folders, so a session-level
//! file would be correct for that — but only until somebody moves a take, at
//! which point it would inherit whatever the folder it landed in says. That is
//! this exact bug reintroduced one level up.
//!
//! Deliberately **advisory**. Nothing here overrides the user's render setting —
//! a take folder can be moved, copied or hand-assembled, and a renderer that
//! silently substituted a number found in a neighbouring file would be a worse
//! surprise than the one it fixes. It exists so the disagreement can be *stated*.

use std::path::{Path, PathBuf};

/// Bump when the shape changes. A file with an unrecognised format is ignored
/// rather than guessed at — an advisory check that mis-reads is worse than one
/// that stays quiet. 2 added `renders` (#438); a format-1 file still reads,
/// with no history.
pub const FORMAT: u32 = 2;

/// The oldest format this build still reads.
const OLDEST_FORMAT: u32 = 1;

/// Lives in the **block** folder — `<capture_dir>/<session>/chain_JJ_bN/` —
/// beside the take it describes, so it travels with the take when the folder is
/// moved or copied.
///
/// The obvious alternative is one file per session folder, and it is worse for
/// exactly the reason it looks tidier: it describes a *location*, not a take.
/// Drag a take into another session's folder and it silently inherits that
/// session's settings, which is the failure this whole feature exists to
/// prevent, reintroduced one level up.
///
/// Putting it in the block folder does collide with `take_folder_has_content`,
/// which decides a take was captured by asking whether that folder is non-empty
/// — so an empty block plus this file would look successful. That is handled by
/// `is_metadata`, which the emptiness check consults, rather than by writing
/// somewhere else and hoping nobody re-runs verification later.
pub const TAKE_FILE: &str = "dodstudio_take.json";

/// Whether a directory entry is one of ours, and so must not be mistaken for
/// captured output. `take_folder_has_content` asks this.
pub fn is_metadata(file_name: &std::ffi::OsStr) -> bool {
    file_name.eq_ignore_ascii_case(TAKE_FILE)
}

/// How far up from a take folder to look. HLAE nests its own `take0000` inside
/// the block folder, and callers hand out either — Render Studio's scanner
/// admits both — so the file is one level up about as often as it is in the
/// folder given. Two is enough to cover that and stops well short of the
/// session folder, where a file would describe a batch rather than this take.
const MAX_ANCESTOR_DEPTH: usize = 2;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct SessionMeta {
    pub format: u32,
    pub session_id: String,
    /// The `mirv_movie_fps` the batch was captured at. `0` when unknown: a
    /// take captured before this file existed gets one the first time it is
    /// rendered, to hold its history.
    pub capture_fps: i32,
    /// Every render of this take, oldest first (#438). Left out while empty,
    /// so a take that was never rendered keeps the same file as before.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub renders: Vec<RenderAttempt>,
    /// `CaptureMode::to_str_id` of a take that is not a video — today only
    /// `"agr"`. Absent for the movie modes, so their file is unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_mode: Option<String>,
    /// The rate an AGR take was recorded at (`host_framerate 1/agr_fps`).
    /// Absent unless `capture_mode` is `"agr"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agr_fps: Option<i32>,
}

impl SessionMeta {
    pub fn new(session_id: impl Into<String>, capture_fps: i32) -> Self {
        Self {
            format: FORMAT,
            session_id: session_id.into(),
            capture_fps,
            renders: Vec::new(),
            capture_mode: None,
            agr_fps: None,
        }
    }

    /// An AGR take (#450): the same record, plus the mode and the rate the
    /// `.agr` was recorded at, which is the number the Blender page needs.
    pub fn agr(session_id: impl Into<String>, capture_fps: i32, agr_fps: i32) -> Self {
        Self {
            capture_mode: Some("agr".to_string()),
            agr_fps: Some(agr_fps),
            ..Self::new(session_id, capture_fps)
        }
    }
}

/// How a render attempt ended. `Interrupted` is what an attempt says from the
/// moment it starts until it ends, so a crash mid-render leaves that behind
/// rather than nothing.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RenderOutcome {
    Interrupted,
    Finished,
    Failed,
    Cancelled,
}

/// One render of a take: when, with what, and what came of it (#438).
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct RenderAttempt {
    /// When it started; with `stream`, what identifies it.
    pub started_unix_ms: u64,
    /// The stream folder rendered (`all`, the HUD pair's colour half, ...):
    /// one take can be two jobs.
    pub stream: String,
    /// A `RenderCodec` id.
    pub codec: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub custom_codec_args: String,
    pub fps: u32,
    pub outcome: RenderOutcome,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub output_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_size_bytes: Option<u64>,
    /// The first line of the error, for a failed attempt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Writes a take's capture settings into its block folder.
///
/// Best-effort by design: a capture that succeeded must not be reported as
/// failed because a metadata file could not be written, so callers log the
/// error and carry on.
pub fn write(block_folder: &Path, meta: &SessionMeta) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(meta)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(block_folder.join(TAKE_FILE), json)
}

/// The capture settings recorded for a take, or `None` when there are none.
///
/// `None` is the normal answer for every take captured before this existed, and
/// for any folder assembled by hand — so it must read as "nothing to say", never
/// as a problem.
pub fn read_for_take(take_folder: &Path) -> Option<SessionMeta> {
    for dir in search_path(take_folder) {
        let candidate = dir.join(TAKE_FILE);
        let Ok(bytes) = std::fs::read(&candidate) else {
            continue;
        };
        let Ok(meta) = serde_json::from_slice::<SessionMeta>(&bytes) else {
            continue;
        };
        if (OLDEST_FORMAT..=FORMAT).contains(&meta.format) {
            return Some(meta);
        }
    }
    None
}

// ── Render history (#438) ────────────────────────────────────────────────────

/// Serialises history writes: a take's colour and HUD streams render as two
/// jobs at once, and both update the same file.
static HISTORY_WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Where a take's file is, or would go: the nearest existing one, else the
/// block folder (the folder given, or its parent for HLAE's nested `take*`).
/// `None` when the nearest file exists but can't be read, so it is never
/// overwritten with a fresh one.
fn history_file(take_folder: &Path) -> Option<(PathBuf, SessionMeta)> {
    for dir in search_path(take_folder) {
        let candidate = dir.join(TAKE_FILE);
        let Ok(bytes) = std::fs::read(&candidate) else {
            continue;
        };
        return match serde_json::from_slice::<SessionMeta>(&bytes) {
            Ok(meta) if (OLDEST_FORMAT..=FORMAT).contains(&meta.format) => Some((candidate, meta)),
            _ => None,
        };
    }
    let is_take_number_folder = take_folder
        .file_name()
        .is_some_and(|n| n.to_string_lossy().to_lowercase().starts_with("take"));
    let block = if is_take_number_folder {
        take_folder.parent()?
    } else {
        take_folder
    };
    Some((block.join(TAKE_FILE), SessionMeta::new(String::new(), 0)))
}

/// A take's render history, oldest first; empty when there is none.
pub fn read_history(take_folder: &Path) -> Vec<RenderAttempt> {
    read_for_take(take_folder)
        .map(|meta| meta.renders)
        .unwrap_or_default()
}

/// Applies `change` to a take's history and saves it. Best-effort, like
/// `write`: a render must never fail because its history couldn't be saved.
fn update_history(
    take_folder: &Path,
    change: impl FnOnce(&mut Vec<RenderAttempt>),
) -> std::io::Result<()> {
    let _guard = HISTORY_WRITE.lock().unwrap_or_else(|p| p.into_inner());
    let Some((path, mut meta)) = history_file(take_folder) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "the take's dodstudio_take.json is unreadable or from a newer build; left as it is",
        ));
    };
    meta.format = FORMAT;
    change(&mut meta.renders);
    let json = serde_json::to_vec_pretty(&meta)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, json)
}

/// Records an attempt as it starts (outcome `Interrupted`).
pub fn record_render_start(take_folder: &Path, attempt: &RenderAttempt) -> std::io::Result<()> {
    update_history(take_folder, |renders| renders.push(attempt.clone()))
}

/// Records how the attempt that started at `started_unix_ms` on `stream`
/// ended.
pub fn record_render_end(
    take_folder: &Path,
    started_unix_ms: u64,
    stream: &str,
    outcome: RenderOutcome,
    output_path: &str,
    output_size_bytes: Option<u64>,
    error: Option<String>,
) -> std::io::Result<()> {
    update_history(take_folder, |renders| {
        if let Some(attempt) = renders
            .iter_mut()
            .rev()
            .find(|a| a.started_unix_ms == started_unix_ms && a.stream == stream)
        {
            attempt.outcome = outcome;
            attempt.output_path = output_path.to_string();
            attempt.output_size_bytes = output_size_bytes;
            attempt.error = error;
        }
    })
}

/// The folder itself first, then its parents — nearest wins, so a take carrying
/// its own file is never overruled by one further up.
fn search_path(take_folder: &Path) -> Vec<PathBuf> {
    take_folder
        .ancestors()
        .take(MAX_ANCESTOR_DEPTH + 1)
        .map(PathBuf::from)
        .collect()
}

/// The warning to show when a render is about to interpret a take at a rate it
/// was not captured at, or `None` when there is nothing to say.
///
/// Returns a message rather than logging, so the same wording can go to the
/// activity log and to a job's own render log without drifting apart.
pub fn fps_mismatch_warning(take_folder: &Path, render_fps: u32) -> Option<String> {
    let meta = read_for_take(take_folder)?;
    // A non-positive recorded rate is meaningless and would divide by zero, so
    // there is nothing trustworthy to compare against. The cast below is safe
    // only because of this guard.
    if meta.capture_fps <= 0 || meta.capture_fps as u32 == render_fps {
        return None;
    }
    // Frames are interpreted at the render rate, so the output runs fast when
    // the render rate is the higher of the two and slow when it is lower.
    let ratio = render_fps as f32 / meta.capture_fps as f32;
    Some(format!(
        "this take was captured at {} fps but is being rendered at {} — FFmpeg reads the frame \
         sequence at the render rate, so the result will run {:.2}x {} and the audio will be cut \
         to match. Set the render FPS to {} unless this is deliberate.",
        meta.capture_fps,
        render_fps,
        if ratio > 1.0 { ratio } else { 1.0 / ratio },
        if ratio > 1.0 { "fast" } else { "slow" },
        meta.capture_fps,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    /// The real layout: `<capture>/<session>/chain_JJ_bN/take0000/`. Returns the
    /// block folder (where the file goes) and the nested take folder HLAE makes
    /// -- and the guard, which the caller must hold: dropping it here would
    /// delete the tree before the test had looked at it.
    fn block_with_take(name: &str, session: &str, block: &str) -> (Scratch, PathBuf, PathBuf) {
        let root = Scratch::new(format_args!("take_meta_{name}"));
        let block_folder = root.join(session).join(block);
        let take = block_folder.join("take0000");
        std::fs::create_dir_all(&take).expect("take dirs");
        (root, block_folder, take)
    }

    #[test]
    fn a_take_is_found_from_either_folder_render_studio_hands_out() {
        let (_root, block, take) =
            block_with_take("finds", "session_20260827_120000", "dodstudio_chain_01_b0");
        write(&block, &SessionMeta::new("session_20260827_120000", 120)).expect("write");

        // The scanner admits a take at the block folder or at the nested
        // `take0000` inside it, depending on how it was found, so both resolve.
        assert_eq!(read_for_take(&block).map(|m| m.capture_fps), Some(120));
        assert_eq!(read_for_take(&take).map(|m| m.capture_fps), Some(120));
    }

    #[test]
    fn two_batches_at_different_rates_do_not_contaminate_each_other() {
        // The question this design has to answer: capture some highlights at
        // 120, then more at 300. Each batch gets its own session folder and each
        // take carries its own file, so neither can speak for the other.
        let root = Scratch::new("take_meta_two_batches");
        let slow = root
            .join("session_20260827_120000")
            .join("dodstudio_chain_01_b0");
        let fast = root
            .join("session_20260827_130000")
            .join("dodstudio_chain_01_b0");
        std::fs::create_dir_all(&slow).expect("dirs");
        std::fs::create_dir_all(&fast).expect("dirs");
        write(&slow, &SessionMeta::new("session_20260827_120000", 120)).expect("write");
        write(&fast, &SessionMeta::new("session_20260827_130000", 300)).expect("write");

        assert_eq!(read_for_take(&slow).map(|m| m.capture_fps), Some(120));
        assert_eq!(read_for_take(&fast).map(|m| m.capture_fps), Some(300));
        assert_eq!(fps_mismatch_warning(&slow, 120), None);
        assert!(fps_mismatch_warning(&fast, 120).is_some());
    }

    #[test]
    fn a_take_moved_into_another_session_keeps_its_own_settings() {
        // The reason the file is per take rather than per session. Somebody
        // consolidating takes by hand would otherwise silently relabel them with
        // whatever folder they were dropped into — the exact bug this feature
        // exists to catch, one level up.
        let root = Scratch::new("take_meta_moved");
        let origin = root.join("session_A").join("dodstudio_chain_01_b0");
        let elsewhere = root.join("session_B");
        std::fs::create_dir_all(&origin).expect("dirs");
        std::fs::create_dir_all(&elsewhere).expect("dirs");
        write(&origin, &SessionMeta::new("session_A", 120)).expect("write");
        // A neighbour in the destination that says something different.
        let neighbour = elsewhere.join("dodstudio_chain_02_b0");
        std::fs::create_dir_all(&neighbour).expect("dirs");
        write(&neighbour, &SessionMeta::new("session_B", 300)).expect("write");

        let moved = elsewhere.join("dodstudio_chain_01_b0");
        std::fs::rename(&origin, &moved).expect("move the take");

        assert_eq!(
            read_for_take(&moved).map(|m| m.capture_fps),
            Some(120),
            "the take was relabelled by the folder it was moved into"
        );
    }

    #[test]
    fn a_take_with_no_metadata_is_not_a_problem() {
        // Every take captured before this existed, and any folder assembled by
        // hand. Silence is the correct answer, not a warning.
        let (_root, _block, take) = block_with_take("absent", "session_x", "dodstudio_chain_01_b0");
        assert_eq!(read_for_take(&take), None);
        assert_eq!(fps_mismatch_warning(&take, 300), None);
    }

    #[test]
    fn an_unreadable_or_future_format_is_ignored_rather_than_guessed() {
        let (_root, block, take) = block_with_take("garbage", "session_x", "dodstudio_chain_01_b0");
        std::fs::write(block.join(TAKE_FILE), b"{not json").expect("write");
        assert_eq!(read_for_take(&take), None);

        let ahead = format!(
            r#"{{"format":{},"session_id":"s","capture_fps":120}}"#,
            FORMAT + 1
        );
        std::fs::write(block.join(TAKE_FILE), ahead).expect("write");
        assert_eq!(read_for_take(&take), None, "a newer format was read anyway");
        // Nor is it overwritten by a render's history.
        let attempt = attempt_at(1, "all");
        assert!(record_render_start(&take, &attempt).is_err());
        assert!(
            std::fs::read_to_string(block.join(TAKE_FILE))
                .unwrap()
                .contains(&format!("\"format\":{}", FORMAT + 1))
        );
    }

    fn attempt_at(started_unix_ms: u64, stream: &str) -> RenderAttempt {
        RenderAttempt {
            started_unix_ms,
            stream: stream.to_string(),
            codec: "prores".to_string(),
            custom_codec_args: String::new(),
            fps: 300,
            outcome: RenderOutcome::Interrupted,
            output_path: String::new(),
            output_size_bytes: None,
            error: None,
        }
    }

    /// #438: an attempt reads as interrupted until it ends, both streams of
    /// a take keep their own entries, and capture settings survive.
    #[test]
    fn render_history_records_each_attempt_and_its_outcome() {
        let (_root, block, take) = block_with_take("history", "session_x", "dodstudio_chain_01_b0");
        write(&block, &SessionMeta::new("session_x", 120)).expect("write");

        record_render_start(&take, &attempt_at(10, "all")).expect("start all");
        record_render_start(&take, &attempt_at(10, "hudcolor")).expect("start hud");
        assert_eq!(read_history(&take)[0].outcome, RenderOutcome::Interrupted);

        record_render_end(
            &take,
            10,
            "all",
            RenderOutcome::Finished,
            "D:/out/a.mov",
            Some(42),
            None,
        )
        .expect("end all");
        record_render_end(
            &take,
            10,
            "hudcolor",
            RenderOutcome::Failed,
            "",
            None,
            Some("ffmpeg exited 1".to_string()),
        )
        .expect("end hud");

        let history = read_history(&take);
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].outcome, RenderOutcome::Finished);
        assert_eq!(history[0].output_size_bytes, Some(42));
        assert_eq!(history[1].outcome, RenderOutcome::Failed);
        assert_eq!(history[1].error.as_deref(), Some("ffmpeg exited 1"));
        let meta = read_for_take(&take).expect("meta");
        assert_eq!((meta.capture_fps, meta.format), (120, FORMAT));
    }

    /// A take captured before the file existed gets one, in its block folder,
    /// and an older format-1 file keeps its capture rate.
    #[test]
    fn history_creates_the_file_and_reads_format_one() {
        let (_root, block, take) =
            block_with_take("history_new", "session_x", "dodstudio_chain_01_b0");
        record_render_start(&take, &attempt_at(5, "all")).expect("start");
        assert!(block.join(TAKE_FILE).is_file());
        assert!(!take.join(TAKE_FILE).exists());
        assert_eq!(read_history(&take).len(), 1);
        assert_eq!(
            fps_mismatch_warning(&take, 300),
            None,
            "an unknown rate scolded"
        );

        let (_root2, block2, take2) =
            block_with_take("format_one", "session_y", "dodstudio_chain_01_b0");
        std::fs::write(
            block2.join(TAKE_FILE),
            r#"{"format":1,"session_id":"session_y","capture_fps":120}"#,
        )
        .expect("write");
        assert!(read_history(&take2).is_empty());
        assert!(fps_mismatch_warning(&take2, 300).is_some());
    }

    #[test]
    fn a_movie_take_file_is_unchanged_and_an_agr_take_says_what_it_is() {
        // The two new fields are left out entirely for a movie take, so a file
        // written today is byte-identical to one written before AGR mode.
        let movie = serde_json::to_value(SessionMeta::new("s", 120)).unwrap();
        assert_eq!(
            movie,
            serde_json::json!({"format": FORMAT, "session_id": "s", "capture_fps": 120})
        );

        let (_root, block, take) = block_with_take("agr", "session_x", "dodstudio_chain_01_b0");
        write(&block, &SessionMeta::agr("s", 300, 60)).expect("write");
        let read = read_for_take(&take).expect("an AGR take reads back");
        assert_eq!(read.capture_mode.as_deref(), Some("agr"));
        assert_eq!(read.agr_fps, Some(60));
    }

    #[test]
    fn a_matching_rate_says_nothing() {
        let (_root, block, take) = block_with_take("match", "session_x", "dodstudio_chain_01_b0");
        write(&block, &SessionMeta::new("s", 120)).expect("write");
        assert_eq!(fps_mismatch_warning(&take, 120), None);
    }

    #[test]
    fn the_warning_states_the_direction_and_the_factor() {
        let (_root, block, take) =
            block_with_take("mismatch", "session_x", "dodstudio_chain_01_b0");
        write(&block, &SessionMeta::new("s", 120)).expect("write");

        // The bug as it actually happened: captured at 120, rendered at 300.
        let fast = fps_mismatch_warning(&take, 300).expect("a mismatch must be reported");
        assert!(fast.contains("2.50x fast"), "{}", fast);
        assert!(fast.contains("Set the render FPS to 120"), "{}", fast);

        // And the other direction, which is just as wrong and reads differently.
        let slow = fps_mismatch_warning(&take, 60).expect("a mismatch must be reported");
        assert!(slow.contains("2.00x slow"), "{}", slow);
    }

    #[test]
    fn a_nonsense_recorded_rate_is_not_used_to_scold_the_user() {
        // A zero would divide by zero and a negative is meaningless; either way
        // there is nothing trustworthy to compare against.
        let (_root, block, take) = block_with_take("zero", "session_x", "dodstudio_chain_01_b0");
        write(&block, &SessionMeta::new("s", 0)).expect("write");
        assert_eq!(fps_mismatch_warning(&take, 300), None);
    }

    #[test]
    fn the_walk_stops_before_it_reaches_a_capture_drive() {
        // A file this far up describes a batch, or a drive, not this take.
        let root = Scratch::new("take_meta_too_far");
        let deep = root.join("a").join("b").join("c").join("take0000");
        std::fs::create_dir_all(&deep).expect("dirs");
        write(&root, &SessionMeta::new("s", 120)).expect("write");
        assert_eq!(read_for_take(&deep), None);
    }

    #[test]
    fn our_own_file_is_recognisable_so_it_cannot_pass_as_captured_output() {
        // `take_folder_has_content` decides a take landed by asking whether its
        // folder is non-empty. Without this, an empty block plus our file would
        // report a capture that never happened.
        assert!(is_metadata(std::ffi::OsStr::new(TAKE_FILE)));
        assert!(is_metadata(std::ffi::OsStr::new("DODSTUDIO_TAKE.JSON")));
        assert!(!is_metadata(std::ffi::OsStr::new("00000.bmp")));
        assert!(!is_metadata(std::ffi::OsStr::new("sound.wav")));
    }
}
