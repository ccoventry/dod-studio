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
use std::path::{Path, PathBuf};

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

/// Whether a take's clip was finished but isn't where it was rendered any
/// more (moved, renamed or deleted by the user): every one of `streams` has
/// a last attempt that finished, and one of their files is gone. Such a take
/// stays, and the "Making room" warning says why.
pub fn clip_moved(
    history: &[RenderAttempt],
    streams: &[String],
    output_exists: impl Fn(&str) -> bool,
) -> bool {
    let mut missing = false;
    for stream in streams {
        let Some(last) = history
            .iter()
            .filter(|a| &a.stream == stream)
            .max_by_key(|a| a.started_unix_ms)
        else {
            return false;
        };
        if last.outcome != RenderOutcome::Finished || last.output_path.is_empty() {
            return false;
        }
        missing |= !output_exists(&last.output_path);
    }
    missing
}

/// The streams a take folder holds, as its render history records them: what
/// `finished_at` must find finished before the take may go. Read from the
/// disk, not from the Render tab's rows, since a row the user removed leaves
/// its stream unrendered but still in the folder.
pub fn streams_on_disk(take_folder: &Path) -> Vec<String> {
    let (clip_tx, clip_rx) = std::sync::mpsc::channel();
    let (status_tx, _) = std::sync::mpsc::channel::<String>();
    scan_folder_background(vec![take_folder.to_path_buf()], clip_tx, status_tx);
    let key = folder_key(&take_folder.to_string_lossy());
    clip_rx
        .try_iter()
        .filter(|clip| folder_key(&clip.take_folder) == key)
        .map(|clip| attempt_stream(&clip))
        .collect()
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

/// The volume a path is on (`sys::disk::volume_key`), so two Destinations
/// folders on one drive count its free space once, and a disk mounted as a
/// folder counts apart (#1041). Empty for a path with no drive, which every
/// such path then shares.
pub fn volume_of(path: &Path) -> String {
    crate::sys::disk::volume_key(path).unwrap_or_default()
}

/// Free space on the drives a job writes to, and the pieces it writes there.
///
/// Each piece (a render job, a capture block) is written whole to one drive,
/// so free space is never added up across drives: two drives with 60% of a
/// piece free each don't hold it. Pieces are placed largest first on the
/// first drive, in target order, with room past `headroom_bytes`: the rule
/// `build_batch_queue`'s first-fit-decreasing uses for capture blocks
/// (`MIN_DRIVE_HEADROOM_BYTES`), and close to a render's, which routes each
/// job to the first export drive with room plus a safety margin.
#[derive(Debug, Clone, PartialEq)]
pub struct RoomLedger {
    /// Each drive's free space, keyed by `volume_of`, in target order.
    drives: Vec<(String, u64)>,
    headroom_bytes: u64,
    /// Largest first.
    pieces: Vec<u64>,
}

/// What doesn't fit: the largest piece no drive has room for, and the most
/// room any one drive has left once the larger pieces are placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortfall {
    pub piece_bytes: u64,
    pub best_drive_bytes: u64,
}

impl RoomLedger {
    /// `drives` is each drive's free space, keyed by `volume_of`; a drive
    /// listed twice counts once, with its first entry.
    pub fn new(drives: Vec<(String, u64)>, headroom_bytes: u64, mut pieces: Vec<u64>) -> Self {
        let mut unique: Vec<(String, u64)> = Vec::with_capacity(drives.len());
        for (volume, free) in drives {
            if !unique.iter().any(|(v, _)| *v == volume) {
                unique.push((volume, free));
            }
        }
        pieces.sort_unstable_by(|a, b| b.cmp(a));
        Self {
            drives: unique,
            headroom_bytes,
            pieces,
        }
    }

    /// `None` when every piece has a drive, else what's left over.
    pub fn shortfall(&self) -> Option<Shortfall> {
        let mut usable: Vec<u64> = self
            .drives
            .iter()
            .map(|(_, f)| f.saturating_sub(self.headroom_bytes))
            .collect();
        for &piece in &self.pieces {
            match usable.iter_mut().find(|u| **u >= piece) {
                Some(u) => *u -= piece,
                None => {
                    return Some(Shortfall {
                        piece_bytes: piece,
                        best_drive_bytes: usable.iter().copied().max().unwrap_or(0),
                    });
                }
            }
        }
        None
    }

    pub fn has_room(&self) -> bool {
        self.shortfall().is_none()
    }

    /// Whether freeing space on `volume` helps this job at all: a take on a
    /// drive the job doesn't write to frees nothing it can use.
    pub fn counts(&self, volume: &str) -> bool {
        self.drives.iter().any(|(v, _)| v == volume)
    }

    /// Counts `bytes` freed on `volume`.
    pub fn add(&mut self, volume: &str, bytes: u64) {
        if let Some((_, free)) = self.drives.iter_mut().find(|(v, _)| v == volume) {
            *free = free.saturating_add(bytes);
        }
    }
}

/// Picks which finished takes to delete so every piece in `ledger` fits,
/// counting each take's size only on its own drive. `volumes[i]` is
/// candidate `i`'s drive, candidates oldest first; `size_of(i)` sizes one
/// (`None` for a take the delete would refuse), and is asked only as far as
/// needed. Returns candidate indexes, oldest first.
///
/// The drive that fits the job by deleting the fewest bytes, oldest takes
/// first, wins; takes on the other drives stay. When no one drive can (only
/// possible with several pieces), the drive with the most to give is used up
/// and the search goes on, and takes that turn out not to be needed are put
/// back, newest first.
pub fn choose_deletions(
    ledger: &RoomLedger,
    volumes: &[String],
    mut size_of: impl FnMut(usize) -> Option<u64>,
) -> Result<Vec<usize>, Shortfall> {
    let with = |chosen: &[(usize, u64)]| {
        let mut l = ledger.clone();
        for &(i, bytes) in chosen {
            l.add(&volumes[i], bytes);
        }
        l
    };
    // Each drive's candidates, oldest first, and how far each is used.
    let per_drive: Vec<Vec<usize>> = ledger
        .drives
        .iter()
        .map(|(v, _)| (0..volumes.len()).filter(|&i| volumes[i] == *v).collect())
        .collect();
    let mut used = vec![0usize; per_drive.len()];
    let mut chosen: Vec<(usize, u64)> = Vec::new();

    while !with(&chosen).has_room() {
        let base = with(&chosen);
        // (drive, takes used up to, bytes freed, takes sized)
        let mut best: Option<(usize, usize, u64, Vec<(usize, u64)>)> = None;
        let mut fullest: Option<(usize, u64)> = None;
        for (d, list) in per_drive.iter().enumerate() {
            if used[d] >= list.len() {
                continue;
            }
            let mut trial = base.clone();
            let mut freed = 0u64;
            let mut taken = Vec::new();
            let mut fits_at = None;
            for (k, &i) in list.iter().enumerate().skip(used[d]) {
                if let Some(bytes) = size_of(i) {
                    trial.add(&volumes[i], bytes);
                    freed = freed.saturating_add(bytes);
                    taken.push((i, bytes));
                }
                if trial.has_room() {
                    fits_at = Some(k + 1);
                    break;
                }
            }
            match fits_at {
                Some(upto) => {
                    if best.as_ref().is_none_or(|b| freed < b.2) {
                        best = Some((d, upto, freed, taken));
                    }
                }
                None => {
                    if fullest.is_none_or(|(_, f)| freed > f) {
                        fullest = Some((d, freed));
                    }
                }
            }
        }
        if let Some((d, upto, _, taken)) = best {
            used[d] = upto;
            chosen.extend(taken);
            break;
        }
        let Some((d, _)) = fullest else {
            // Every candidate is in, and the job still doesn't fit.
            return Err(with(&chosen).shortfall().unwrap_or(Shortfall {
                piece_bytes: 0,
                best_drive_bytes: 0,
            }));
        };
        for &i in &per_drive[d][used[d]..] {
            if let Some(bytes) = size_of(i) {
                chosen.push((i, bytes));
            }
        }
        used[d] = per_drive[d].len();
    }

    // Put back what isn't needed, newest first.
    chosen.sort_by_key(|&(i, _)| i);
    let mut k = chosen.len();
    while k > 0 {
        k -= 1;
        let mut without = chosen.clone();
        without.remove(k);
        if with(&without).has_room() {
            chosen = without;
        }
    }
    Ok(chosen.into_iter().map(|(i, _)| i).collect())
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
    scan_takes(capture_dirs, busy).0
}

/// `finished_takes`, plus the idle takes kept because their clip moved
/// (`clip_moved`).
fn scan_takes(
    capture_dirs: &[PathBuf],
    busy: &HashSet<String>,
) -> (Vec<FinishedTake>, Vec<PathBuf>) {
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

    let mut takes: Vec<FinishedTake> = Vec::new();
    let mut moved: Vec<PathBuf> = Vec::new();
    let is_file = |p: &str| Path::new(p).is_file();
    for (folder, streams) in streams_by_take {
        if busy.contains(&folder_key(&folder)) {
            continue;
        }
        let history = take_meta::read_history(Path::new(&folder));
        if let Some(at) = finished_at(&history, &streams, is_file) {
            takes.push(FinishedTake {
                take_folder: PathBuf::from(folder),
                finished_unix_ms: at,
            });
        } else if clip_moved(&history, &streams, is_file) {
            moved.push(PathBuf::from(folder));
        }
    }
    takes.sort_by(|a, b| {
        a.finished_unix_ms
            .cmp(&b.finished_unix_ms)
            .then_with(|| a.take_folder.cmp(&b.take_folder))
    });
    (takes, moved)
}

/// What a job about to start needs: the drives it writes to, the pieces it
/// writes (each render job, or each capture block, whole on one drive), and
/// how much each drive keeps back.
#[derive(Debug, Clone, PartialEq)]
pub struct SpaceNeed {
    pub targets: Vec<PathBuf>,
    pub pieces: Vec<u64>,
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
    /// Even with every finished take on the job's drives deleted, a piece
    /// has no drive with room for it, so none is deleted for a job that won't
    /// fit anyway. `needed_bytes` is that piece; `available_bytes` is the most
    /// room one drive would have; `moved_takes` counts the takes on those
    /// drives kept because their clip isn't where it was rendered.
    Short {
        needed_bytes: u64,
        available_bytes: u64,
        moved_takes: usize,
    },
}

/// Works out which finished takes to delete, oldest first, so the job in
/// `need` fits. Nothing is deleted here.
///
/// `free_of` is the free-space query (`sys::disk::get_available_bytes`, whose
/// `None` means "can't tell"); `capture_dirs` are the Destinations
/// folders, the only place takes are looked for and deleted from; `progress`
/// hears each step, and nothing at all when the drives already have room.
pub fn plan_room(
    need: &SpaceNeed,
    capture_dirs: &[PathBuf],
    busy: &HashSet<String>,
    free_of: impl Fn(&Path) -> Option<u64>,
    mut progress: impl FnMut(RoomStep),
) -> RoomPlan {
    let mut drives = Vec::new();
    for target in &need.targets {
        let Some(bytes) = free_of(target) else {
            return RoomPlan::Unknown;
        };
        drives.push((volume_of(target), bytes));
    }
    if drives.is_empty() {
        return RoomPlan::Unknown;
    }
    let ledger = RoomLedger::new(drives, need.headroom_bytes, need.pieces.clone());
    if ledger.has_room() {
        return RoomPlan::Enough;
    }

    progress(RoomStep::Looking);
    let (finished, moved) = scan_takes(capture_dirs, busy);
    let candidates: Vec<FinishedTake> = finished
        .into_iter()
        .filter(|t| ledger.counts(&volume_of(&t.take_folder)))
        .collect();
    let moved_takes = moved
        .iter()
        .filter(|t| ledger.counts(&volume_of(t)))
        .count();
    let volumes: Vec<String> = candidates
        .iter()
        .map(|t| volume_of(&t.take_folder))
        .collect();
    let total = candidates.len();
    // Each take is sized once, when the search first needs it.
    let mut sized: Vec<Option<Option<TakeDeletion>>> = (0..total).map(|_| None).collect();
    let mut measured = 0usize;
    let result = choose_deletions(&ledger, &volumes, |i| {
        if sized[i].is_none() {
            progress(RoomStep::Measuring {
                done: measured,
                total,
            });
            measured += 1;
            // The same check the delete runs; a take it refuses is skipped
            // rather than counted as space that will never come back.
            sized[i] = Some(plan_take_deletion(&candidates[i].take_folder, capture_dirs).ok());
        }
        sized[i]
            .as_ref()
            .and_then(|p| p.as_ref())
            .map(|p| p.size_bytes)
    });
    progress(RoomStep::Measuring { done: total, total });
    match result {
        Ok(chosen) => RoomPlan::Delete(
            chosen
                .into_iter()
                .filter_map(|i| sized[i].take().flatten())
                .collect(),
        ),
        Err(short) => RoomPlan::Short {
            needed_bytes: short.piece_bytes,
            available_bytes: short.best_drive_bytes,
            moved_takes,
        },
    }
}

/// `bytes` as GB with one decimal, for the activity log.
pub fn format_gb(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
}

#[cfg(test)]
#[path = "take_cleanup_tests.rs"]
mod tests;
