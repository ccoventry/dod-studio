//! How many demos each listed folder holds, counted off the game thread
//! (#409).
//!
//! Counting on the game thread froze the game for every list fill: measured
//! 2026-10-05 at 250 ms for `dod/` (filled three times on opening) and 2.2 s
//! for the Steam folder, mostly spent proving folders empty. So a fill lists
//! the folders straight away and asks this module to count the ones it
//! hasn't counted lately. A worker thread counts them, one folder at a time,
//! and the Demos tab shows a progress bar meanwhile. When it is done, the tab
//! lists again, and this time every folder's count is known: the counts fill
//! in, and empty folders drop out.
//!
//! A count is kept for [`FRESH_FOR`], so the fills that follow each other
//! (opening the tab, a refill, going back up a folder) cost nothing.

// Only the 32-bit build's hooks call it; a host build compiles it for the tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// How long a folder's count is reused before it is counted again (new
/// recordings).
const FRESH_FOR: Duration = Duration::from_secs(60);

/// How many entries one folder's count reads before it stops. Off the game
/// thread, so it can be generous.
const BUDGET: usize = 200_000;

/// What a folder holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FolderCount {
    pub demos: usize,
    /// False when the count stopped at the budget: at least `demos`.
    pub complete: bool,
}

/// One folder's counts: directly in it, and at any depth.
#[derive(Clone, Copy, Debug)]
struct Counted {
    direct: FolderCount,
    all: FolderCount,
    at: Instant,
}

static COUNTED: Mutex<Option<HashMap<String, Counted>>> = Mutex::new(None);

/// The job in hand: its generation (a newer job stops an older one), which
/// folder it is for, and how far it has got.
static JOB: AtomicUsize = AtomicUsize::new(0);
static JOB_FOR: Mutex<String> = Mutex::new(String::new());
static TOTAL: AtomicUsize = AtomicUsize::new(0);
static DONE: AtomicUsize = AtomicUsize::new(0);
static RUNNING: AtomicBool = AtomicBool::new(false);
/// Set when a job finishes, for the Demos tab to list again.
static FINISHED: AtomicBool = AtomicBool::new(false);

/// `path` with its `.` and `..` worked out, as written (no disk access):
/// the up row's `temp demos/../` is `dod/`.
pub(crate) fn normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    out
}

fn key(path: &Path) -> String {
    normalize(path)
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

/// The `.dem` files in `folder`: directly in it, or with `subfolders` at any
/// depth. Reads at most `budget` entries.
pub fn count_demos(folder: &Path, subfolders: bool, budget: usize) -> FolderCount {
    let mut queue = std::collections::VecDeque::from([folder.to_path_buf()]);
    let (mut demos, mut read) = (0usize, 0usize);
    while let Some(dir) = queue.pop_front() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            read += 1;
            if read > budget {
                return FolderCount {
                    demos,
                    complete: false,
                };
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if subfolders {
                    queue.push_back(entry.path());
                }
            } else if entry
                .file_name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .ends_with(".dem")
            {
                demos += 1;
            }
        }
    }
    FolderCount {
        demos,
        complete: true,
    }
}

/// The `.dem` files in `folder` at any depth, leaving out the folder `skip`
/// and everything in it. Reads at most `budget` entries.
pub fn count_demos_except(folder: &Path, skip: &Path, budget: usize) -> FolderCount {
    let skip = key(skip);
    let mut queue = std::collections::VecDeque::from([normalize(folder)]);
    let (mut demos, mut read) = (0usize, 0usize);
    while let Some(dir) = queue.pop_front() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            read += 1;
            if read > budget {
                return FolderCount {
                    demos,
                    complete: false,
                };
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if key(&entry.path()) != skip {
                    queue.push_back(entry.path());
                }
            } else if entry
                .file_name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .ends_with(".dem")
            {
                demos += 1;
            }
        }
    }
    FolderCount {
        demos,
        complete: true,
    }
}

/// `here`'s whole count, from what is already counted: the demos directly in
/// it plus each subfolder's total (counted now if it wasn't lately).
fn total_from_parts(here: &Path) -> FolderCount {
    let mut total = count_demos(here, false, BUDGET);
    for entry in std::fs::read_dir(here)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
    {
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            let sub = entry.path();
            let part = fresh(&sub).map_or_else(|| count_demos(&sub, true, BUDGET), |c| c.all);
            total.demos += part.demos;
            total.complete &= part.complete;
        }
    }
    total
}

/// The parent's counts for the up row, without counting `here` twice: `here`'s
/// total from its parts, plus everything else in the parent.
fn count_parent(parent: &Path, here: &Path) -> Counted {
    let mine = total_from_parts(here);
    let rest = count_demos_except(parent, here, BUDGET);
    Counted {
        direct: count_demos(&normalize(parent), false, BUDGET),
        all: FolderCount {
            demos: mine.demos + rest.demos,
            complete: mine.complete && rest.complete,
        },
        at: Instant::now(),
    }
}

fn fresh(path: &Path) -> Option<Counted> {
    let counted = COUNTED.lock().ok()?;
    let c = *counted.as_ref()?.get(&key(path))?;
    (c.at.elapsed() < FRESH_FOR).then_some(c)
}

/// `path`'s demo count, if counted lately: at any depth with `subfolders`,
/// otherwise only the demos directly in it.
pub fn count(path: &Path, subfolders: bool) -> Option<FolderCount> {
    fresh(path).map(|c| if subfolders { c.all } else { c.direct })
}

/// Whether `path` is known to hold no demo at any depth (counted lately, and
/// the count wasn't cut short). Not counted yet is not empty.
pub fn known_empty(path: &Path) -> bool {
    fresh(path).is_some_and(|c| c.all.complete && c.all.demos == 0)
}

/// Counts, on a worker thread, whichever of `folders` aren't counted lately,
/// then `up = (parent, here)`'s parent for the up row (see [`count_parent`]).
/// `listing` names the folder they are in: a fill of the same folder while
/// its job runs leaves that job alone, and any other stops it.
pub fn count_later(listing: &str, folders: Vec<PathBuf>, up: Option<(PathBuf, PathBuf)>) {
    let todo: Vec<PathBuf> = folders.into_iter().filter(|f| fresh(f).is_none()).collect();
    let up = up.filter(|(parent, _)| fresh(parent).is_none());
    if todo.is_empty() && up.is_none() {
        return;
    }
    {
        let Ok(mut job_for) = JOB_FOR.lock() else {
            return;
        };
        if RUNNING.load(Ordering::Acquire) && *job_for == listing {
            return;
        }
        *job_for = listing.to_string();
    }
    let job = JOB.fetch_add(1, Ordering::AcqRel) + 1;
    TOTAL.store(todo.len() + up.is_some() as usize, Ordering::Release);
    DONE.store(0, Ordering::Release);
    RUNNING.store(true, Ordering::Release);
    let listing = listing.to_string();
    std::thread::spawn(move || {
        let started = Instant::now();
        for folder in &todo {
            if JOB.load(Ordering::Acquire) != job {
                return;
            }
            let counted = Counted {
                direct: count_demos(folder, false, BUDGET),
                all: count_demos(folder, true, BUDGET),
                at: Instant::now(),
            };
            if let Ok(mut map) = COUNTED.lock() {
                map.get_or_insert_with(HashMap::new)
                    .insert(key(folder), counted);
            }
            DONE.fetch_add(1, Ordering::AcqRel);
        }
        if let Some((parent, here)) = &up
            && JOB.load(Ordering::Acquire) == job
        {
            let counted = count_parent(parent, here);
            if let Ok(mut map) = COUNTED.lock() {
                map.get_or_insert_with(HashMap::new)
                    .insert(key(parent), counted);
            }
            DONE.fetch_add(1, Ordering::AcqRel);
        }
        if JOB.load(Ordering::Acquire) == job {
            RUNNING.store(false, Ordering::Release);
            FINISHED.store(true, Ordering::Release);
            let line = format!(
                "folder_counts: {} -- {} folder(s) counted in {} ms, off the game thread",
                if listing.is_empty() { "dod/" } else { &listing },
                todo.len(),
                started.elapsed().as_millis()
            );
            unsafe { crate::debug::report(&line) };
        }
    });
}

/// How far the running job has got, `(done, total)`, or `None` when none runs.
pub fn progress() -> Option<(usize, usize)> {
    RUNNING
        .load(Ordering::Acquire)
        .then(|| (DONE.load(Ordering::Acquire), TOTAL.load(Ordering::Acquire)))
}

/// Whether a job finished since the last call: the list should be filled
/// again to show its counts.
pub fn take_finished() -> bool {
    FINISHED.swap(false, Ordering::AcqRel)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("dodstudio_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("rounds/r1")).unwrap();
        std::fs::create_dir_all(root.join("empty/deeper")).unwrap();
        for f in [
            "a.dem",
            "rounds/r1/m1.DEM",
            "rounds/r1/m2.dem",
            "empty/deeper/notes.txt",
        ] {
            std::fs::write(root.join(f), b"").unwrap();
        }
        root
    }

    #[test]
    fn counts_direct_or_with_subfolders() {
        let root = scratch("count");
        assert_eq!(
            count_demos(&root, false, 1000),
            FolderCount {
                demos: 1,
                complete: true
            }
        );
        assert_eq!(
            count_demos(&root, true, 1000),
            FolderCount {
                demos: 3,
                complete: true
            }
        );
        assert!(!count_demos(&root, true, 2).complete, "stops at the budget");
        assert_eq!(count_demos(&root.join("missing"), true, 1000).demos, 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dots_are_worked_out_without_the_disk() {
        assert_eq!(
            key(Path::new("C:/a/dod/temp demos/../")),
            key(Path::new("C:/a/dod"))
        );
        assert_eq!(
            key(Path::new("C:/a/dod/./x/")),
            key(Path::new("c:/A/DOD/x"))
        );
        assert_eq!(key(Path::new("C:/a/dod/../../")), key(Path::new("C:/")));
    }

    #[test]
    fn the_up_row_counts_the_parent_without_counting_here_twice() {
        let root = scratch("up");
        std::fs::create_dir_all(root.join("sibling")).unwrap();
        std::fs::write(root.join("sibling/s.dem"), b"").unwrap();
        let here = root.join("rounds");
        // Without here: a.dem and sibling/s.dem.
        assert_eq!(count_demos_except(&root, &here, 1000).demos, 2);
        let parent = count_parent(&here.join(".."), &here);
        assert_eq!(
            parent.all,
            FolderCount {
                demos: 4,
                complete: true
            }
        );
        assert_eq!(
            parent.direct,
            FolderCount {
                demos: 1,
                complete: true
            }
        );
        assert_eq!(
            parent.all,
            FolderCount {
                demos: count_demos(&root, true, 1000).demos,
                complete: true
            }
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_job_counts_in_the_background_then_says_so() {
        let root = scratch("job");
        let (rounds, empty) = (root.join("rounds"), root.join("empty"));
        assert_eq!(count(&rounds, true), None, "not counted yet");
        assert!(!known_empty(&empty), "not counted is not empty");
        count_later("test", vec![rounds.clone(), empty.clone()], None);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !take_finished() {
            assert!(Instant::now() < deadline, "the job never finished");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(progress(), None);
        assert_eq!(
            count(&rounds, true),
            Some(FolderCount {
                demos: 2,
                complete: true
            })
        );
        assert_eq!(
            count(&rounds, false),
            Some(FolderCount {
                demos: 0,
                complete: true
            })
        );
        assert!(known_empty(&empty));
        assert!(!known_empty(&rounds));
        // Counted lately: a second fill starts nothing.
        count_later("test", vec![rounds, empty], None);
        assert_eq!(progress(), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
