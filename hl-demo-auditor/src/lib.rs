// Panicking `unwrap()` / `expect()` stay out of non-test code; tests are allowed in clippy.toml (#1179).
#![warn(clippy::unwrap_used, clippy::expect_used)]
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;

pub mod archive;
pub mod backup;
mod duplicates;
mod key_cache;
mod keys;
mod lanes;
pub mod messages;
pub mod repair;
mod walk;
mod zips;
/// The demo key's hash, defined once in `analysis` (#1107).
pub use analysis::fnv1a::{Fnv1a, fnv1a_hash};
pub use archive::{ZipDemoEntry, ZipRef};
pub use backup::{ArchiveListing, ArchivedCopy, Backup, BackupIndex};
pub use key_cache::{ArchiveEntry, KeyCache, KeyEntry};
pub use keys::{ContentHashes, Stamp, content_hashes, get_file_key, stamp, tail_hash};
pub use walk::{
    SYSTEM_FOLDERS, collect_demos, comparable_path, is_windows_folder, is_windows_folder_in,
    scan_dir, scan_roots, scan_roots_and_zips, shown_path, windows_folder,
};

use duplicates::find_duplicates_with;
use lanes::Readers;

/// A demo file's identity: its size plus an FNV-1a hash of its first 64 KB.
///
/// The one definition in the workspace. The Demo Auditor groups duplicates
/// by it; Studio's scan (`scan_manager::demo_file_key`) uses its text form
/// to skip demos already queued and unchanged, and saves that text in project
/// files, so [`FileKey::to_text`] must never change shape. Reads 64 KB, not
/// the whole demo, so keying a folder of hundreds costs milliseconds.
#[derive(Eq, PartialEq, Hash, Debug, Clone)]
pub struct FileKey {
    pub size: u64,
    pub header_hash: u64,
}

impl FileKey {
    /// `<size>-<hash as 16 hex digits>`, e.g. `5242880-00c0ffee00c0ffee`.
    pub fn to_text(&self) -> String {
        format!("{}-{:016x}", self.size, self.header_hash)
    }

    /// The inverse of [`FileKey::to_text`]; `None` for anything else.
    pub fn from_text(text: &str) -> Option<Self> {
        let (size, hash) = text.split_once('-')?;
        if hash.len() != 16 {
            return None;
        }
        Some(Self {
            size: size.parse().ok()?,
            header_hash: u64::from_str_radix(hash, 16).ok()?,
        })
    }
}

/// How much of a file [`get_file_key`] hashes.
pub const KEY_BYTES: u64 = 64 * 1024;

/// Loose copies of one demo. A copy inside an archive is never a member
/// (#929): archives are backups, listed in [`Audit::backups`] instead.
#[derive(Debug, Clone)]
pub struct DuplicateGroup {
    pub key: FileKey,
    /// The loose copies, sorted.
    pub files: Vec<PathBuf>,
}

impl DuplicateGroup {
    /// The copies a delete could free: all but the one kept.
    pub fn copies(&self) -> usize {
        self.files.len().saturating_sub(1)
    }

    fn wasted_bytes(&self) -> u64 {
        self.key.size * self.copies() as u64
    }

    /// The first path in the group, for a stable order.
    fn first_name(&self) -> PathBuf {
        self.files.first().cloned().unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub enum AuditProgress {
    Scanning(String),
    /// How many demos [`find_duplicates`] looks at, once paths named twice
    /// are merged; sent before the first `Hashing`, so a bar can show how
    /// far the key pass has got. Each of them then sends one `Hashing`,
    /// whether it was read, remembered or never needed a key.
    Keying(usize),
    /// How many archives [`find_duplicates_in`] looks inside (#929), sent before
    /// the first `ReadingArchive`; each of them then sends one, whether its
    /// list of entries was read or remembered. All of this comes before
    /// `Keying`.
    Archives(usize),
    ReadingArchive(String),
    Hashing(String),
    Found(usize),
    Failed(String),
    Done(Vec<DuplicateGroup>, usize, u64),
}

/// One demo's delta-continuity verdict (#41) as `--integrity` prints it:
/// whether it is OK, and `<path>: <verdict>`. A file that cannot be read or
/// parsed is not OK either.
pub fn integrity_line(path: &Path) -> (bool, String) {
    let (ok, verdict) = match fs::read(path) {
        Ok(bytes) => match analysis::integrity::check_demo_bytes(&bytes) {
            Ok(report) => (report.is_ok(), report.verdict()),
            Err(e) => (false, e),
        },
        Err(e) => (false, format!("cannot read the file: {e}")),
    };
    (ok, format!("{}: {verdict}", path.display()))
}

/// How many files each step of a key pass had to open: the rest were
/// remembered ([`KeyCache`]) or, for the first 64 KiB, never needed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reads {
    pub heads: usize,
    pub tails: usize,
    pub fulls: usize,
    /// Archives whose list of entries was read (#929).
    pub zips: usize,
    /// Demos inside archives decompressed in full (#929).
    pub zip_demos: usize,
}

/// What [`find_duplicates_cached`] found.
#[derive(Debug, Default)]
pub struct Audit {
    /// One loose file per demo: each group's first.
    pub unique_files: Vec<PathBuf>,
    /// Loose copies only: an archive's copy is never in a group (#929).
    pub groups: Vec<DuplicateGroup>,
    pub copies: usize,
    pub wasted_bytes: u64,
    pub reads: Reads,
    /// Every loose demo an archive holds a copy of, by path (#929), whether
    /// or not it has loose copies too.
    pub backups: Vec<Backup>,
    /// Each archive looked inside and its `.dem` entries that could be
    /// listed, by path: their sizes and CRC-32s, and their hashes where read.
    pub archives: Vec<ArchiveListing>,
    /// Archives looked inside, and the demos found in them that could be read.
    pub zips: usize,
    pub zip_demos: usize,
    /// A zip, or a demo inside one, that couldn't be read and was left out
    /// (corrupt, encrypted): `<path>: <why>`.
    pub skipped: Vec<String>,
}

/// `f` over every item on `threads` threads; `None` for items a cancel
/// skipped.
fn par_map<T: Sync, R: Send>(
    items: &[T],
    threads: usize,
    cancel: &AtomicBool,
    f: impl Fn(&T) -> R + Sync,
) -> Vec<Option<R>> {
    let next = AtomicUsize::new(0);
    let mut out: Vec<Option<R>> = std::iter::repeat_with(|| None).take(items.len()).collect();
    let done: Vec<Vec<(usize, R)>> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads.clamp(1, items.len().max(1)))
            .map(|_| {
                scope.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        if cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(i) else {
                            break;
                        };
                        mine.push((i, f(item)));
                    }
                    mine
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|w| w.join().unwrap_or_default())
            .collect()
    });
    for (i, r) in done.into_iter().flatten() {
        out[i] = Some(r);
    }
    out
}

/// Identifies duplicates within a list of file paths.
/// Returns unique files, structured duplicate groups, total duplicates count, and total wasted space in bytes.
pub fn find_duplicates(
    files: Vec<PathBuf>,
    cancel: &AtomicBool,
    progress: &Option<Sender<AuditProgress>>,
) -> (Vec<PathBuf>, Vec<DuplicateGroup>, usize, u64) {
    let audit = find_duplicates_cached(files, cancel, progress, &mut KeyCache::default());
    (
        audit.unique_files,
        audit.groups,
        audit.copies,
        audit.wasted_bytes,
    )
}

/// [`find_duplicates`] that reads and fills `cache` (#888): a demo whose
/// size and modified time are unchanged since the last pass is not opened
/// again, and a demo whose size no other demo has is never opened at all,
/// since it can't be a copy. The groups are the ones [`find_duplicates`]
/// finds without a cache, except that files the cache knows differ in full
/// ([`KeyCache::remember_contents`], from the delete's byte-for-byte check)
/// are split apart.
pub fn find_duplicates_cached(
    files: Vec<PathBuf>,
    cancel: &AtomicBool,
    progress: &Option<Sender<AuditProgress>>,
    cache: &mut KeyCache,
) -> Audit {
    find_duplicates_in(files, Vec::new(), cancel, progress, cache)
}

/// [`find_duplicates_cached`] that also looks inside `zips` (`.zip`, `.7z`,
/// `.rar`; #929). An archive is a backup: its demos never join a duplicate
/// group, so the groups are the ones the loose demos alone make. Each loose
/// demo an archive holds a copy of is listed in [`Audit::backups`] with every
/// archive entry holding it ([`BackupIndex::copies_of`]). An entry is
/// decompressed only when a loose demo has its size, and what it held is
/// remembered under the archive's path, size and modified time. An archive,
/// or an entry, that can't be read is left out and named in
/// [`Audit::skipped`].
pub fn find_duplicates_in(
    files: Vec<PathBuf>,
    zips: Vec<PathBuf>,
    cancel: &AtomicBool,
    progress: &Option<Sender<AuditProgress>>,
    cache: &mut KeyCache,
) -> Audit {
    find_duplicates_with(files, zips, &Readers::system(), cancel, progress, cache)
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
