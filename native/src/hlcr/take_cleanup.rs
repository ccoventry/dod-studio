//! Automatic take cleanup (#644): deletes a take once its clip is finished,
//! or only when a capture or finish job needs the room.
//!
//! The Tauri side decides *when* (a render batch ending, a job about to
//! start); this module decides *which* takes may go and *how many*. The
//! deleting itself is `take_delete`'s, so the automatic mode runs exactly the
//! same safety check as the Delete Take button: a real take folder, inside a
//! Destinations folder, with its render history kept.
//!
//! The one rule everything here serves: **only a take whose clip finished is
//! ever eligible.** Every stream of the take must have a last render attempt
//! that finished (#438's history in `dodstudio_take.json`), and that render's
//! file must still be on disk. A failed, cancelled, interrupted or
//! never-rendered take, or one whose clip file has since gone, is never
//! touched: the take would be the only copy left.

use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path, PathBuf};

use crate::hlcr::scanner::{ClipData, scan_folder_background};
use crate::hlcr::take_delete::{TakeDeletion, plan_take_deletion};
use crate::hlcr::take_meta::{self, RenderAttempt, RenderOutcome};

/// The "After a clip is finished" setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterFinish {
    Keep,
    Recycle,
    Delete,
}

impl AfterFinish {
    /// Reads the setting's stored value. Anything unknown reads as `Keep`: a
    /// value written by a newer build must never start deleting takes.
    pub fn from_setting(value: &str) -> Self {
        match value {
            "recycle" => Self::Recycle,
            "delete" => Self::Delete,
            _ => Self::Keep,
        }
    }
}

/// The setting and its "Only when space is needed" option, as the two moments
/// that act on them see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CleanupPolicy {
    pub after_finish: AfterFinish,
    pub only_when_space_needed: bool,
}

impl CleanupPolicy {
    /// How a take goes as soon as its clip finishes, or `None` when it stays.
    /// "Only when space is needed" holds a permanent delete back until a job
    /// needs the room; it means nothing for the Recycle Bin, which frees no
    /// space until it is emptied.
    pub fn on_finish(self) -> Option<AfterFinish> {
        match self.after_finish {
            AfterFinish::Keep => None,
            AfterFinish::Recycle => Some(AfterFinish::Recycle),
            AfterFinish::Delete if self.only_when_space_needed => None,
            AfterFinish::Delete => Some(AfterFinish::Delete),
        }
    }

    /// Whether finished takes are deleted to make room before a capture or
    /// finish job starts.
    pub fn makes_room(self) -> bool {
        self.after_finish == AfterFinish::Delete && self.only_when_space_needed
    }
}

/// The stream a render attempt is recorded under (#438): one take can be two
/// jobs, its picture and its HUD pair.
pub fn attempt_stream(clip: &ClipData) -> String {
    if clip.clip_type == "hud_only" {
        format!("hud:{}", clip.img_folder)
    } else {
        clip.img_folder.clone()
    }
}

/// When a take's clip was finished, or `None` when it isn't.
///
/// Finished means every one of `streams` has a last attempt (by start time)
/// that finished, with its output file still there (`output_exists`). The
/// time is the latest of those attempts' start times, which orders takes
/// oldest first.
pub fn finished_at(
    history: &[RenderAttempt],
    streams: &[String],
    output_exists: impl Fn(&str) -> bool,
) -> Option<u64> {
    if streams.is_empty() {
        return None;
    }
    let mut latest = 0;
    for stream in streams {
        let last = history
            .iter()
            .filter(|a| &a.stream == stream)
            .max_by_key(|a| a.started_unix_ms)?;
        let verified = last.outcome == RenderOutcome::Finished
            && !last.output_path.is_empty()
            && output_exists(&last.output_path);
        if !verified {
            return None;
        }
        latest = latest.max(last.started_unix_ms);
    }
    Some(latest)
}

/// A take folder's path as a comparable key: the scanner and the Render tab
/// can spell one folder with different case or slashes.
pub fn folder_key(path: &str) -> String {
    path.chars()
        .map(|c| {
            if c == '/' {
                '\\'
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect::<String>()
        .trim_end_matches('\\')
        .to_string()
}

/// The drive a path is on (`d:`, or `\\server\share`), so two Destinations
/// folders on one drive count its free space once. Empty for a path with no
/// drive, which every such path then shares.
pub fn volume_of(path: &Path) -> String {
    match path.components().next() {
        Some(Component::Prefix(prefix)) => {
            prefix.as_os_str().to_string_lossy().to_ascii_lowercase()
        }
        _ => String::new(),
    }
}

/// Free space on the drives a job writes to, and what the job needs there.
///
/// The drives are summed, each keeping `headroom_bytes` back: a capture
/// spreads its blocks over every Destinations drive and refuses to leave one
/// with less than `MIN_DRIVE_HEADROOM_BYTES`, and a render picks the first
/// export drive with room plus a safety margin.
#[derive(Debug, Clone, PartialEq)]
pub struct RoomLedger {
    free: BTreeMap<String, u64>,
    headroom_bytes: u64,
    needed_bytes: u64,
}

impl RoomLedger {
    /// `free` is each drive's free space, keyed by `volume_of`.
    pub fn new(free: BTreeMap<String, u64>, headroom_bytes: u64, needed_bytes: u64) -> Self {
        Self {
            free,
            headroom_bytes,
            needed_bytes,
        }
    }

    /// What the job can use: each drive's free space past its headroom.
    pub fn usable_bytes(&self) -> u64 {
        self.free
            .values()
            .map(|f| f.saturating_sub(self.headroom_bytes))
            .fold(0u64, u64::saturating_add)
    }

    pub fn has_room(&self) -> bool {
        self.usable_bytes() >= self.needed_bytes
    }

    pub fn needed_bytes(&self) -> u64 {
        self.needed_bytes
    }

    /// Whether freeing space on `volume` helps this job at all: a take on a
    /// drive the job doesn't write to frees nothing it can use.
    pub fn counts(&self, volume: &str) -> bool {
        self.free.contains_key(volume)
    }

    /// Counts `bytes` freed on `volume`.
    pub fn add(&mut self, volume: &str, bytes: u64) {
        if let Some(free) = self.free.get_mut(volume) {
            *free = free.saturating_add(bytes);
        }
    }
}

/// A take whose clip is finished, and when.
#[derive(Debug, Clone, PartialEq)]
pub struct FinishedTake {
    pub take_folder: PathBuf,
    pub finished_unix_ms: u64,
}

/// Every take under `capture_dirs` whose clip is finished, oldest first,
/// leaving out the folders in `busy` (keys from `folder_key`): a take queued
/// or rendering may have an older finished render in its history, and is
/// still never eligible.
pub fn finished_takes(capture_dirs: &[PathBuf], busy: &HashSet<String>) -> Vec<FinishedTake> {
    let (clip_tx, clip_rx) = std::sync::mpsc::channel();
    // The scan's per-take status lines aren't shown here; dropping the
    // receiver makes each send a no-op.
    let (status_tx, _) = std::sync::mpsc::channel::<String>();
    scan_folder_background(capture_dirs.to_vec(), clip_tx, status_tx);

    let mut streams_by_take: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for clip in clip_rx.try_iter() {
        streams_by_take
            .entry(clip.take_folder.clone())
            .or_default()
            .push(attempt_stream(&clip));
    }

    let mut takes: Vec<FinishedTake> = streams_by_take
        .into_iter()
        .filter(|(folder, _)| !busy.contains(&folder_key(folder)))
        .filter_map(|(folder, streams)| {
            let history = take_meta::read_history(Path::new(&folder));
            let at = finished_at(&history, &streams, |p| Path::new(p).is_file())?;
            Some(FinishedTake {
                take_folder: PathBuf::from(folder),
                finished_unix_ms: at,
            })
        })
        .collect();
    takes.sort_by(|a, b| {
        a.finished_unix_ms
            .cmp(&b.finished_unix_ms)
            .then_with(|| a.take_folder.cmp(&b.take_folder))
    });
    takes
}

/// What a job about to start needs: the drives it writes to, how much, and
/// how much each drive keeps back.
#[derive(Debug, Clone, PartialEq)]
pub struct SpaceNeed {
    pub targets: Vec<PathBuf>,
    pub needed_bytes: u64,
    pub headroom_bytes: u64,
}

/// Where `plan_room` is, for a progress line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomStep {
    /// The drives are short, and the Destinations folders are being scanned
    /// for finished takes.
    Looking,
    /// Finished takes are being sized, oldest first.
    Measuring { done: usize, total: usize },
}

/// What making room for a job comes to.
#[derive(Debug)]
pub enum RoomPlan {
    /// The drives already have the room; nothing goes.
    Enough,
    /// A drive's free space can't be read, so nothing is deleted on a guess.
    Unknown,
    /// Delete these, oldest first, and the job fits.
    Delete(Vec<TakeDeletion>),
    /// Every finished take together still isn't enough, so none is deleted
    /// for a job that won't fit anyway.
    Short {
        needed_bytes: u64,
        available_bytes: u64,
    },
}

/// Works out which finished takes to delete, oldest first, so the job in
/// `need` fits. Nothing is deleted here.
///
/// `free_of` is the free-space query (`sys::disk::get_available_bytes`, whose
/// `u64::MAX` means "can't tell"); `capture_dirs` are the Destinations
/// folders, the only place takes are looked for and deleted from; `progress`
/// hears each step, and nothing at all when the drives already have room.
pub fn plan_room(
    need: &SpaceNeed,
    capture_dirs: &[PathBuf],
    busy: &HashSet<String>,
    free_of: impl Fn(&Path) -> u64,
    mut progress: impl FnMut(RoomStep),
) -> RoomPlan {
    let mut free = BTreeMap::new();
    for target in &need.targets {
        let bytes = free_of(target);
        if bytes == u64::MAX {
            return RoomPlan::Unknown;
        }
        free.insert(volume_of(target), bytes);
    }
    if free.is_empty() {
        return RoomPlan::Unknown;
    }
    let mut ledger = RoomLedger::new(free, need.headroom_bytes, need.needed_bytes);
    if ledger.has_room() {
        return RoomPlan::Enough;
    }

    progress(RoomStep::Looking);
    let candidates: Vec<FinishedTake> = finished_takes(capture_dirs, busy)
        .into_iter()
        .filter(|t| ledger.counts(&volume_of(&t.take_folder)))
        .collect();
    let total = candidates.len();
    let mut chosen = Vec::new();
    for (i, take) in candidates.iter().enumerate() {
        progress(RoomStep::Measuring { done: i, total });
        // The same check the delete runs; a take it refuses is skipped here
        // rather than counted as space that will never come back.
        let Ok(plan) = plan_take_deletion(&take.take_folder, capture_dirs) else {
            continue;
        };
        ledger.add(&volume_of(&take.take_folder), plan.size_bytes);
        chosen.push(plan);
        if ledger.has_room() {
            progress(RoomStep::Measuring { done: total, total });
            return RoomPlan::Delete(chosen);
        }
    }
    progress(RoomStep::Measuring { done: total, total });
    RoomPlan::Short {
        needed_bytes: ledger.needed_bytes(),
        available_bytes: ledger.usable_bytes(),
    }
}

/// `bytes` as GB with one decimal, for the activity log.
pub fn format_gb(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    fn attempt(stream: &str, started: u64, outcome: RenderOutcome, output: &str) -> RenderAttempt {
        RenderAttempt {
            started_unix_ms: started,
            stream: stream.to_string(),
            codec: "prores".to_string(),
            custom_codec_args: String::new(),
            fps: 120,
            outcome,
            output_path: output.to_string(),
            output_size_bytes: None,
            error: None,
        }
    }

    fn streams(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    const EVERY_FILE_EXISTS: fn(&str) -> bool = |_| true;

    #[test]
    fn the_setting_maps_to_what_happens_after_finish_and_before_a_job() {
        let policy = |after: &str, only| CleanupPolicy {
            after_finish: AfterFinish::from_setting(after),
            only_when_space_needed: only,
        };
        assert_eq!(policy("keep", false).on_finish(), None);
        assert_eq!(policy("keep", true).on_finish(), None);
        assert!(!policy("keep", true).makes_room());
        assert_eq!(
            policy("recycle", false).on_finish(),
            Some(AfterFinish::Recycle)
        );
        // The Recycle Bin frees nothing until emptied, so the option is ignored.
        assert_eq!(
            policy("recycle", true).on_finish(),
            Some(AfterFinish::Recycle)
        );
        assert!(!policy("recycle", true).makes_room());
        assert_eq!(
            policy("delete", false).on_finish(),
            Some(AfterFinish::Delete)
        );
        assert!(!policy("delete", false).makes_room());
        assert_eq!(policy("delete", true).on_finish(), None);
        assert!(policy("delete", true).makes_room());
    }

    #[test]
    fn an_unknown_setting_value_keeps_takes() {
        assert_eq!(AfterFinish::from_setting(""), AfterFinish::Keep);
        assert_eq!(AfterFinish::from_setting("shred"), AfterFinish::Keep);
        assert_eq!(AfterFinish::from_setting("Delete"), AfterFinish::Keep);
    }

    #[test]
    fn a_take_with_a_finished_render_on_disk_is_finished() {
        let history = vec![attempt("all", 10, RenderOutcome::Finished, "E:/out/a.mov")];
        assert_eq!(
            finished_at(&history, &streams(&["all"]), EVERY_FILE_EXISTS),
            Some(10)
        );
    }

    #[test]
    fn never_rendered_failed_cancelled_and_unfinished_takes_are_not_finished() {
        let s = streams(&["all"]);
        assert_eq!(finished_at(&[], &s, EVERY_FILE_EXISTS), None);
        assert_eq!(finished_at(&[], &[], EVERY_FILE_EXISTS), None);
        for outcome in [
            RenderOutcome::Failed,
            RenderOutcome::Cancelled,
            RenderOutcome::Started,
        ] {
            let history = vec![attempt("all", 10, outcome, "E:/out/a.mov")];
            assert_eq!(
                finished_at(&history, &s, EVERY_FILE_EXISTS),
                None,
                "{outcome:?}"
            );
        }
    }

    #[test]
    fn the_last_attempt_decides_not_an_earlier_finished_one() {
        let s = streams(&["all"]);
        let history = vec![
            attempt("all", 10, RenderOutcome::Finished, "E:/out/a.mov"),
            attempt("all", 20, RenderOutcome::Cancelled, ""),
        ];
        assert_eq!(finished_at(&history, &s, EVERY_FILE_EXISTS), None);
        // Order in the file doesn't matter, start time does.
        let history = vec![
            attempt("all", 30, RenderOutcome::Finished, "E:/out/b.mov"),
            attempt("all", 20, RenderOutcome::Failed, ""),
        ];
        assert_eq!(finished_at(&history, &s, EVERY_FILE_EXISTS), Some(30));
    }

    #[test]
    fn a_take_whose_clip_file_is_gone_is_not_finished() {
        let history = vec![attempt("all", 10, RenderOutcome::Finished, "E:/out/a.mov")];
        assert_eq!(finished_at(&history, &streams(&["all"]), |_| false), None);
        let no_path = vec![attempt("all", 10, RenderOutcome::Finished, "")];
        assert_eq!(
            finished_at(&no_path, &streams(&["all"]), EVERY_FILE_EXISTS),
            None
        );
    }

    #[test]
    fn a_hud_pair_needs_both_halves_finished() {
        let s = streams(&["all", "hud:hudcolor"]);
        let one = vec![attempt("all", 10, RenderOutcome::Finished, "E:/out/a.mov")];
        assert_eq!(finished_at(&one, &s, EVERY_FILE_EXISTS), None);
        let both = vec![
            attempt("all", 10, RenderOutcome::Finished, "E:/out/a.mov"),
            attempt(
                "hud:hudcolor",
                12,
                RenderOutcome::Finished,
                "E:/out/a_hud.mov",
            ),
        ];
        assert_eq!(finished_at(&both, &s, EVERY_FILE_EXISTS), Some(12));
    }

    #[test]
    fn folder_keys_ignore_case_slashes_and_a_trailing_separator() {
        assert_eq!(
            folder_key("D:/Captures/S1/b0/take0000/"),
            folder_key("d:\\captures\\s1\\b0\\take0000")
        );
    }

    #[test]
    fn the_ledger_sums_drives_past_their_headroom() {
        let free = BTreeMap::from([("c:".to_string(), 30), ("d:".to_string(), 5)]);
        let mut ledger = RoomLedger::new(free, 10, 25);
        // 20 usable on C:, none on D: (5 is below its headroom).
        assert_eq!(ledger.usable_bytes(), 20);
        assert!(!ledger.has_room());
        // Freeing on D: first fills its headroom.
        ledger.add("d:", 5);
        assert_eq!(ledger.usable_bytes(), 20);
        ledger.add("d:", 5);
        assert!(ledger.has_room());
    }

    #[test]
    fn space_on_a_drive_the_job_does_not_use_does_not_count() {
        let free = BTreeMap::from([("c:".to_string(), 0)]);
        let mut ledger = RoomLedger::new(free, 0, 10);
        assert!(!ledger.counts("e:"));
        ledger.add("e:", 100);
        assert!(!ledger.has_room());
    }

    #[test]
    fn a_volume_is_the_drive_letter() {
        if cfg!(windows) {
            let mut path = PathBuf::from("D:");
            path.push("captures");
            path.push("s1");
            assert_eq!(volume_of(&path), "d:");
        }
        assert_eq!(volume_of(Path::new("relative")), "");
    }

    // ── Scanning a Destinations folder ───────────────────────────────────────

    /// A frame-sequence take under `<capture>/<session>/<block>/take0000`,
    /// with `history` recorded in its block folder. Returns the take folder.
    fn take_with_history(capture: &Scratch, block: &str, history: Vec<RenderAttempt>) -> PathBuf {
        let take = capture
            .join("session_20261011_120000")
            .join(block)
            .join("take0000");
        std::fs::create_dir_all(take.join("all")).unwrap();
        std::fs::write(take.join("sound.wav"), vec![0u8; 100]).unwrap();
        std::fs::write(take.join("all").join("00000.bmp"), vec![0u8; 900]).unwrap();
        let mut meta = take_meta::SessionMeta::new("s", 120);
        meta.renders = history;
        take_meta::write(take.parent().unwrap(), &meta).unwrap();
        take
    }

    fn rendered(capture: &Scratch, name: &str) -> String {
        let out = capture.join("exports");
        std::fs::create_dir_all(&out).unwrap();
        let file = out.join(name);
        std::fs::write(&file, b"clip").unwrap();
        file.to_string_lossy().into_owned()
    }

    #[test]
    fn only_finished_idle_takes_are_found_oldest_first() {
        let capture = Scratch::new("take_cleanup_scan");
        let a = rendered(&capture, "a.mov");
        let b = rendered(&capture, "b.mov");
        let newer = take_with_history(
            &capture,
            "chain_01_b0",
            vec![attempt("all", 200, RenderOutcome::Finished, &a)],
        );
        let older = take_with_history(
            &capture,
            "chain_01_b1",
            vec![attempt("all", 100, RenderOutcome::Finished, &b)],
        );
        take_with_history(
            &capture,
            "chain_01_b2",
            vec![attempt("all", 50, RenderOutcome::Failed, "")],
        );
        take_with_history(&capture, "chain_01_b3", Vec::new());
        let gone = capture.join("exports").join("missing.mov");
        take_with_history(
            &capture,
            "chain_01_b4",
            vec![attempt(
                "all",
                10,
                RenderOutcome::Finished,
                &gone.to_string_lossy(),
            )],
        );
        let dirs = vec![capture.path().to_path_buf()];

        let found = finished_takes(&dirs, &HashSet::new());
        let folders: Vec<_> = found.iter().map(|t| t.take_folder.clone()).collect();
        assert_eq!(folders, vec![older.clone(), newer.clone()]);

        let busy = HashSet::from([folder_key(&older.to_string_lossy())]);
        let found = finished_takes(&dirs, &busy);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].take_folder, newer);
    }

    fn need(capture: &Scratch, needed: u64) -> SpaceNeed {
        SpaceNeed {
            targets: vec![capture.path().to_path_buf()],
            needed_bytes: needed,
            headroom_bytes: 0,
        }
    }

    /// Each test take is 1000 bytes on disk (a 100-byte wav, a 900-byte frame).
    const TAKE_BYTES: u64 = 1000;

    #[test]
    fn room_is_made_by_the_oldest_finished_takes_only_as_far_as_needed() {
        let capture = Scratch::new("take_cleanup_plan");
        let a = rendered(&capture, "a.mov");
        let b = rendered(&capture, "b.mov");
        let c = rendered(&capture, "c.mov");
        take_with_history(
            &capture,
            "chain_01_b0",
            vec![attempt("all", 300, RenderOutcome::Finished, &a)],
        );
        let oldest = take_with_history(
            &capture,
            "chain_01_b1",
            vec![attempt("all", 100, RenderOutcome::Finished, &b)],
        );
        let second = take_with_history(
            &capture,
            "chain_01_b2",
            vec![attempt("all", 200, RenderOutcome::Finished, &c)],
        );
        let dirs = vec![capture.path().to_path_buf()];

        // 500 free, 2000 needed: the two oldest takes (2000 bytes) bring it to 2500.
        let plan = plan_room(
            &need(&capture, 2000),
            &dirs,
            &HashSet::new(),
            |_| 500,
            |_| {},
        );
        let RoomPlan::Delete(chosen) = plan else {
            panic!("expected deletions, got {plan:?}");
        };
        let folders: Vec<_> = chosen.iter().map(|d| d.take_folder.clone()).collect();
        assert_eq!(folders, vec![oldest, second]);
        assert!(chosen.iter().all(|d| d.size_bytes == TAKE_BYTES));
    }

    #[test]
    fn nothing_is_planned_when_there_is_room_or_free_space_is_unknown() {
        let capture = Scratch::new("take_cleanup_plan_enough");
        let a = rendered(&capture, "a.mov");
        take_with_history(
            &capture,
            "chain_01_b0",
            vec![attempt("all", 100, RenderOutcome::Finished, &a)],
        );
        let dirs = vec![capture.path().to_path_buf()];
        assert!(matches!(
            plan_room(
                &need(&capture, 100),
                &dirs,
                &HashSet::new(),
                |_| 100,
                |_| {}
            ),
            RoomPlan::Enough
        ));
        assert!(matches!(
            plan_room(
                &need(&capture, 100),
                &dirs,
                &HashSet::new(),
                |_| u64::MAX,
                |_| {}
            ),
            RoomPlan::Unknown
        ));
        let no_targets = SpaceNeed {
            targets: Vec::new(),
            needed_bytes: 100,
            headroom_bytes: 0,
        };
        assert!(matches!(
            plan_room(&no_targets, &dirs, &HashSet::new(), |_| 0, |_| {}),
            RoomPlan::Unknown
        ));
    }

    #[test]
    fn nothing_is_planned_when_every_finished_take_is_still_not_enough() {
        let capture = Scratch::new("take_cleanup_plan_short");
        let a = rendered(&capture, "a.mov");
        let take = take_with_history(
            &capture,
            "chain_01_b0",
            vec![attempt("all", 100, RenderOutcome::Finished, &a)],
        );
        let dirs = vec![capture.path().to_path_buf()];
        let plan = plan_room(
            &need(&capture, 10_000),
            &dirs,
            &HashSet::new(),
            |_| 0,
            |_| {},
        );
        let RoomPlan::Short {
            needed_bytes,
            available_bytes,
        } = plan
        else {
            panic!("expected short, got {plan:?}");
        };
        assert_eq!((needed_bytes, available_bytes), (10_000, TAKE_BYTES));
        assert!(take.join("sound.wav").is_file(), "planning deletes nothing");
    }

    #[test]
    fn progress_is_silent_with_room_and_steps_through_a_scan_without() {
        let capture = Scratch::new("take_cleanup_plan_progress");
        let a = rendered(&capture, "a.mov");
        take_with_history(
            &capture,
            "chain_01_b0",
            vec![attempt("all", 100, RenderOutcome::Finished, &a)],
        );
        let dirs = vec![capture.path().to_path_buf()];
        let mut steps = Vec::new();
        plan_room(
            &need(&capture, 10),
            &dirs,
            &HashSet::new(),
            |_| 10,
            |s| steps.push(s),
        );
        assert!(steps.is_empty(), "{steps:?}");
        plan_room(
            &need(&capture, 900),
            &dirs,
            &HashSet::new(),
            |_| 0,
            |s| steps.push(s),
        );
        assert_eq!(
            steps,
            vec![
                RoomStep::Looking,
                RoomStep::Measuring { done: 0, total: 1 },
                RoomStep::Measuring { done: 1, total: 1 },
            ]
        );
    }

    #[test]
    fn takes_outside_the_destinations_folders_are_never_planned() {
        let capture = Scratch::new("take_cleanup_plan_outside");
        let a = rendered(&capture, "a.mov");
        take_with_history(
            &capture,
            "chain_01_b0",
            vec![attempt("all", 100, RenderOutcome::Finished, &a)],
        );
        let other = Scratch::new("take_cleanup_plan_outside_other");
        let plan = plan_room(
            &need(&capture, 500),
            &[other.path().to_path_buf()],
            &HashSet::new(),
            |_| 0,
            |_| {},
        );
        assert!(matches!(plan, RoomPlan::Short { .. }), "{plan:?}");
    }
}
