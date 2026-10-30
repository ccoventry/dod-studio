use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;

pub mod archive;
pub mod backup;
mod key_cache;
mod keys;
mod lanes;
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

use keys::head_hash;
use lanes::{Readers, drive_of};
use zips::{list_zips, read_zip_demos};

/// A demo file's identity: its size plus an FNV-1a hash of its first 64 KB.
///
/// The one definition in the workspace. The Demo Auditor groups duplicates
/// by it; Studio's scan (`capture_manager::demo_file_key`) uses its text form
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

/// A group of its own for a file that can't be read, as when keying fails.
fn unreadable_key(path: &Path) -> FileKey {
    FileKey {
        size: 0,
        header_hash: fnv1a_hash(path.to_string_lossy().as_bytes()),
    }
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

/// Paths resolved to their canonical form with their stamps, each once,
/// sorted.
fn resolve_paths(
    paths: &[PathBuf],
    readers: &Readers,
    cancel: &AtomicBool,
) -> Vec<(PathBuf, Option<Stamp>)> {
    // Canonicalizing opens each file, so it runs in parallel.
    let resolved = readers.map(
        paths,
        |p| drive_of(p),
        cancel,
        |path| {
            let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.clone());
            let stamp = stamp(&canonical);
            (canonical, stamp)
        },
    );
    let mut by_path: HashMap<PathBuf, Option<Stamp>> = HashMap::new();
    for (path, stamp) in resolved.into_iter().flatten() {
        by_path.entry(path).or_insert(stamp);
    }
    let mut out: Vec<(PathBuf, Option<Stamp>)> = by_path.into_iter().collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
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

/// [`find_duplicates_in`] reading as `readers` says: every read below, the
/// first and last 64 KiB and the archives, one at a time on a hard disk.
fn find_duplicates_with(
    files: Vec<PathBuf>,
    zips: Vec<PathBuf>,
    readers: &Readers,
    cancel: &AtomicBool,
    progress: &Option<Sender<AuditProgress>>,
    cache: &mut KeyCache,
) -> Audit {
    let mut reads = Reads::default();
    let mut skipped = Vec::new();
    let send = |msg: AuditProgress| {
        if let Some(tx) = progress {
            let _ = tx.send(msg);
        }
    };
    let hashing = |path: &Path| send(AuditProgress::Hashing(path.to_string_lossy().into_owned()));

    // Canonicalize and deduplicate paths to prevent counting/grouping a file
    // with itself.
    let files = resolve_paths(&files, readers, cancel);
    let zips = resolve_paths(&zips, readers, cancel);
    if cancel.load(Ordering::Relaxed) {
        return Audit::default();
    }
    for (path, _) in files.iter().chain(&zips) {
        cache.mark_seen(path);
    }
    let mut zip_demos = Vec::new();
    if !zips.is_empty() {
        send(AuditProgress::Archives(zips.len()));
        zip_demos = list_zips(
            &zips,
            readers,
            cancel,
            progress,
            cache,
            &mut reads,
            &mut skipped,
        );
        if cancel.load(Ordering::Relaxed) {
            return Audit::default();
        }
    }
    send(AuditProgress::Keying(files.len() + zip_demos.len()));

    let mut loose_sizes: HashMap<u64, usize> = HashMap::new();
    for (_, stamp) in &files {
        if let Some(s) = stamp {
            *loose_sizes.entry(s.size).or_default() += 1;
        }
    }
    let archived_sizes: HashSet<u64> = zip_demos.iter().map(|z| z.entry.size).collect();
    // A loose demo is opened only when another loose demo, or a demo in an
    // archive, has its size: otherwise it can't be a copy or backed up.
    let worth_keying = |size: u64| {
        loose_sizes.get(&size).copied().unwrap_or(0) >= 2 || archived_sizes.contains(&size)
    };
    let mut keys: Vec<Option<FileKey>> = vec![None; files.len()];
    let mut to_read = Vec::new();
    for (i, (path, stamp)) in files.iter().enumerate() {
        match stamp {
            None => {
                keys[i] = Some(unreadable_key(path));
                hashing(path);
            }
            Some(s) if !worth_keying(s.size) => hashing(path),
            Some(s) => match cache.entry(path, *s) {
                Some(entry) => {
                    keys[i] = Some(FileKey {
                        size: s.size,
                        header_hash: entry.head,
                    });
                    hashing(path);
                }
                None => to_read.push(i),
            },
        }
    }
    let heads = readers.map(
        &to_read,
        |&i| drive_of(&files[i].0),
        cancel,
        |&i| {
            let (path, stamp) = &files[i];
            hashing(path);
            head_hash(path, stamp.map_or(0, |s| s.size))
        },
    );
    for (&i, head) in to_read.iter().zip(heads) {
        let Some(head) = head else { continue };
        reads.heads += 1;
        let (path, stamp) = &files[i];
        let Some(stamp) = *stamp else { continue };
        keys[i] = Some(match head {
            Ok(head) => {
                cache.put(path, stamp, head);
                FileKey {
                    size: stamp.size,
                    header_hash: head,
                }
            }
            Err(_) => unreadable_key(path),
        });
    }

    // A demo in an archive is read in full or not at all: deflate, LZMA and
    // RAR can't seek to its last 64 KiB. Only an entry a loose demo shares a
    // size with is read, and each such entry is read itself, so every
    // archive holding a demo is listed: a size and CRC-32 in a header say
    // nothing about the data behind them, and a damaged archive listing the
    // same header must not show as a backup.
    let mut needed = Vec::new();
    for (j, z) in zip_demos.iter().enumerate() {
        if z.entry.hashes.is_some() || !loose_sizes.contains_key(&z.entry.size) {
            send(AuditProgress::Hashing(z.at(&zips).display()));
        } else {
            needed.push(j);
        }
    }
    if !needed.is_empty() && !cancel.load(Ordering::Relaxed) {
        for (j, read) in read_zip_demos(&needed, &zip_demos, &zips, readers, cancel, progress) {
            reads.zip_demos += 1;
            let (path, stamp) = &zips[zip_demos[j].zip];
            if let Some(stamp) = stamp {
                cache.set_archive_hashes(path, *stamp, &zip_demos[j].entry.name, read.clone());
            }
            match read {
                Ok(h) => zip_demos[j].entry.hashes = Some(h),
                Err(e) => {
                    skipped.push(format!("{}: {e}", zip_demos[j].at(&zips).shown()));
                    zip_demos[j].entry.error = Some(e);
                }
            }
        }
    }
    let index = BackupIndex::new(zip_demos.iter().filter_map(|z| {
        let hashes = z.entry.hashes.filter(|_| z.entry.error.is_none())?;
        Some(ArchivedCopy {
            at: z.at(&zips),
            hashes,
        })
    }));

    let mut groups: HashMap<FileKey, Vec<usize>> = HashMap::new();
    let mut singles: Vec<usize> = Vec::new();
    for (i, key) in keys.iter().enumerate() {
        match key {
            Some(key) => groups.entry(key.clone()).or_default().push(i),
            None => singles.push(i),
        }
    }

    // The key reads only the start, so two cuts of one recording can share
    // it. Files that end differently are different demos: split each group
    // by its last bytes too. Only grouped files, and files an archive holds
    // a demo with the same key, pay the extra read. The key itself stays as
    // it is, since project files save its text.
    let mut tails: Vec<Option<u64>> = vec![None; files.len()];
    let mut tail_reads = Vec::new();
    for (key, members) in &groups {
        if members.len() < 2 && !index.has_key(key) {
            continue;
        }
        for &i in members {
            let (path, stamp) = &files[i];
            match stamp
                .and_then(|s| cache.entry(path, s))
                .and_then(|e| e.tail)
            {
                Some(tail) => tails[i] = Some(tail),
                None => tail_reads.push((i, key.size)),
            }
        }
    }
    let drive = |&(i, _): &(usize, u64)| drive_of(&files[i].0);
    let read_tails = readers.map(&tail_reads, drive, cancel, |&(i, size)| {
        tail_hash(&files[i].0, size)
    });
    for (&(i, _), tail) in tail_reads.iter().zip(read_tails) {
        let Some(tail) = tail else { continue };
        reads.tails += 1;
        let (path, stamp) = &files[i];
        // Unreadable here: a group of its own, as when keying fails.
        tails[i] = Some(match tail {
            Ok(tail) => {
                if let Some(s) = stamp {
                    cache.set_tail(path, *s, tail);
                }
                tail
            }
            Err(_) => fnv1a_hash(path.to_string_lossy().as_bytes()),
        });
    }

    let mut backups = Vec::new();
    if !index.is_empty() {
        for (i, key) in keys.iter().enumerate() {
            let (Some(key), Some(tail)) = (key, tails[i]) else {
                continue;
            };
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let (path, stamp) = &files[i];
            // Read in full once, then remembered while its size and
            // modified time hold.
            let known = stamp
                .and_then(|s| cache.entry(path, s))
                .and_then(|e| e.full);
            let copies = index.copies_of(key, tail, || {
                known.or_else(|| read_full(path, *stamp, cache, &mut reads))
            });
            if !copies.is_empty() {
                backups.push(Backup {
                    demo: path.clone(),
                    copies,
                });
            }
        }
    }

    let mut split: Vec<(FileKey, Vec<usize>)> = Vec::new();
    for (key, members) in groups {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if members.len() < 2 {
            split.push((key, members));
            continue;
        }
        let mut by_tail: HashMap<u64, Vec<usize>> = HashMap::new();
        for i in members {
            if let Some(tail) = tails[i] {
                by_tail.entry(tail).or_default().push(i);
            }
        }
        for (_, same_tail) in by_tail {
            for part in split_by_full_hash(same_tail, &files, cache, &mut reads) {
                split.push((key.clone(), part));
            }
        }
    }
    for i in singles {
        split.push((
            FileKey {
                size: files[i].1.map_or(0, |s| s.size),
                header_hash: 0,
            },
            vec![i],
        ));
    }

    let mut listings: Vec<ArchiveListing> = zips
        .iter()
        .map(|(path, _)| ArchiveListing {
            archive: path.clone(),
            demos: Vec::new(),
        })
        .collect();
    for z in &zip_demos {
        if z.entry.error.is_none() {
            listings[z.zip].demos.push(z.entry.clone());
        }
    }
    let mut audit = Audit {
        reads,
        backups,
        archives: listings,
        zips: zips.len(),
        zip_demos: zip_demos.len(),
        skipped,
        ..Audit::default()
    };
    for (key, members) in split {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        // Sorted, so original vs duplicate selection is stable.
        let mut paths: Vec<PathBuf> = members.iter().map(|&i| files[i].0.clone()).collect();
        paths.sort();
        if let Some(first) = paths.first() {
            audit.unique_files.push(first.clone());
        }
        if paths.len() > 1 {
            let group = DuplicateGroup { key, files: paths };
            audit.copies += group.copies();
            audit.wasted_bytes += group.wasted_bytes();
            audit.groups.push(group);
        }
    }
    // Sort unique files alphabetically so traversal is deterministic
    audit.unique_files.sort();
    // Most wasted space first; then by path, so the order is stable.
    audit.groups.sort_by(|a, b| {
        b.wasted_bytes()
            .cmp(&a.wasted_bytes())
            .then_with(|| a.first_name().cmp(&b.first_name()))
    });
    audit
}

/// `path`'s whole-file hash, read now and remembered in `cache` while its
/// size holds; `None` when it can't be read.
fn read_full(
    path: &Path,
    stamp: Option<Stamp>,
    cache: &mut KeyCache,
    reads: &mut Reads,
) -> Option<u64> {
    reads.fulls += 1;
    let hashes = content_hashes(path).ok()?;
    if let Some(s) = stamp
        && s.size == hashes.size
    {
        cache.remember_contents(path, s, hashes);
    }
    Some(hashes.full)
}

/// One group (same key and tail) split by whole-file hash, but only when the
/// cache already proves two of its files differ: two remembered full hashes
/// that don't match. Its other files are then read in full to see which side
/// they belong to; a file that can't be read goes on its own. Without that
/// proof the group stays whole, as it was before the cache.
fn split_by_full_hash(
    members: Vec<usize>,
    files: &[(PathBuf, Option<Stamp>)],
    cache: &mut KeyCache,
    reads: &mut Reads,
) -> Vec<Vec<usize>> {
    if members.len() < 2 {
        return vec![members];
    }
    let known: Vec<Option<u64>> = members
        .iter()
        .map(|&i| {
            let (path, stamp) = &files[i];
            stamp
                .and_then(|s| cache.entry(path, s))
                .and_then(|e| e.full)
        })
        .collect();
    let distinct: HashSet<u64> = known.iter().flatten().copied().collect();
    if distinct.len() < 2 {
        return vec![members];
    }
    let mut parts: HashMap<u64, Vec<usize>> = HashMap::new();
    let mut alone = Vec::new();
    for (&i, full) in members.iter().zip(known) {
        let (path, stamp) = &files[i];
        match full.or_else(|| read_full(path, *stamp, cache, reads)) {
            Some(full) => parts.entry(full).or_default().push(i),
            None => alone.push(vec![i]),
        }
    }
    parts.into_values().chain(alone).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lanes::{Lane, plan_lanes, seek_penalty};
    use crate::walk::{scan_roots_with, walk_roots};
    use std::sync::Mutex;
    use std::time::Duration;

    #[test]
    fn the_text_form_round_trips_and_keeps_its_shape() {
        let key = FileKey {
            size: 5_242_880,
            header_hash: 0x00c0_ffee_00c0_ffee,
        };
        // Saved in project files: this exact shape must never change.
        assert_eq!(key.to_text(), "5242880-00c0ffee00c0ffee");
        assert_eq!(FileKey::from_text(&key.to_text()), Some(key));
        assert_eq!(FileKey::from_text("12-abc"), None);
        assert_eq!(FileKey::from_text("x-00c0ffee00c0ffee"), None);
        assert_eq!(FileKey::from_text(""), None);
    }

    #[test]
    fn a_file_that_ends_differently_is_not_grouped_with_copies() {
        let dir = std::env::temp_dir().join(format!("auditor_tail_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let bytes = vec![7u8; 3 * KEY_BYTES as usize];
        let mut other = bytes.clone();
        *other.last_mut().unwrap() = 8;
        let paths: Vec<PathBuf> = [("a.dem", &bytes), ("b.dem", &bytes), ("c.dem", &other)]
            .iter()
            .map(|(name, data)| {
                let p = dir.join(name);
                fs::write(&p, data).unwrap();
                p
            })
            .collect();
        // Same size and first 64 KB: one key for all three.
        assert_eq!(
            get_file_key(&paths[0]).unwrap(),
            get_file_key(&paths[2]).unwrap()
        );
        let (_, groups, count, _) = find_duplicates(paths.clone(), &AtomicBool::new(false), &None);
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(groups.len(), 1);
        assert_eq!(count, 1);
        let names: Vec<_> = groups[0]
            .files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["a.dem", "b.dem"]);
    }

    fn fixture_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../test-fixtures/ci_fixture.dem")
    }

    #[test]
    fn the_ci_fixture_passes_the_integrity_check() {
        let path = fixture_path();
        let (ok, line) = integrity_line(&path);
        assert!(ok, "{line}");
        assert_eq!(line, format!("{}: OK", path.display()));
    }

    /// The OBS capture test's bundled demo, cut at its end with
    /// `analysis/examples/trim_demo.rs`.
    #[test]
    fn the_benchmark_demo_passes_the_integrity_check() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../test-fixtures/dodstudio_benchmark_anzio.dem");
        let (ok, line) = integrity_line(&path);
        assert!(ok, "{line}");
    }

    #[test]
    fn an_unreadable_demo_fails_the_integrity_check_and_names_itself() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("no-such-demo.dem");
        let (ok, line) = integrity_line(&path);
        assert!(!ok);
        assert!(
            line.starts_with(&format!("{}: cannot read the file: ", path.display())),
            "{line}"
        );
    }

    #[test]
    fn collect_demos_takes_files_and_folders_and_reports_the_rest() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../test-fixtures");
        let nowhere = fixtures.join("no-such-folder");
        let (demos, missing) = collect_demos(&[fixture_path(), fixtures, nowhere.clone()]);
        // The fixture named directly and found in its folder is listed once,
        // beside the folder's other demo.
        let mut names: Vec<_> = demos
            .iter()
            .map(|d| d.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            ["ci_fixture.dem", "dodstudio_benchmark_anzio.dem"],
            "{demos:?}"
        );
        assert_eq!(missing, [nowhere]);
    }

    #[test]
    fn test_demos_are_unique() {
        let demos_dir = Path::new("../local/demos");
        if !demos_dir.exists() {
            return;
        }
        let mut files = vec![];
        let cancel = AtomicBool::new(false);
        scan_dir(demos_dir, &mut files, &cancel, &None);

        let (_, duplicates, dup_count, wasted) = find_duplicates(files, &cancel, &None);

        // Assert that none of our test demos (including both allied/axis POVs) are duplicates
        assert_eq!(
            dup_count, 0,
            "Demos in test folder should all be unique, but found duplicates: {:?}",
            duplicates
        );
        assert_eq!(wasted, 0);
    }

    #[test]
    fn test_duplicate_paths_deduplicated() {
        let demos_dir = Path::new("../local/demos");
        if !demos_dir.exists() {
            return;
        }
        let mut files = vec![];
        let cancel = AtomicBool::new(false);
        scan_dir(demos_dir, &mut files, &cancel, &None);

        // Intentionally duplicate the entire list of file paths
        let mut duplicated_list = files.clone();
        duplicated_list.extend(files);

        let (_, duplicates, dup_count, wasted) = find_duplicates(duplicated_list, &cancel, &None);

        // They should be deduplicated by path, resulting in 0 duplicate groups
        assert_eq!(
            dup_count, 0,
            "Duplicate paths should be resolved and not reported as duplicates: {:?}",
            duplicates
        );
        assert_eq!(wasted, 0);
    }

    #[cfg(windows)]
    #[test]
    fn a_junction_back_up_the_tree_is_not_followed() {
        let root = std::env::temp_dir().join(format!("auditor_loop_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("a.dem"), b"").unwrap();
        fs::write(root.join("sub/b.dem"), b"").unwrap();
        let link = root.join("sub").join("loop");
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&root)
            .output()
            .unwrap();
        assert!(made.status.success(), "mklink /J failed");

        let mut files = vec![];
        scan_dir(&root, &mut files, &AtomicBool::new(false), &None);
        assert_eq!(files.len(), 2, "{files:?}");
        let parallel = scan_roots(std::slice::from_ref(&root), &AtomicBool::new(false), &None);
        assert_eq!(parallel.len(), 2, "{parallel:?}");

        // A junction is a directory to `remove_dir`, which unlinks it only.
        fs::remove_dir(&link).unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_recycle_bin_and_system_volume_information_are_not_entered() {
        let root = std::env::temp_dir().join(format!("auditor_system_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("d/$Recycle.Bin/S-1-5")).unwrap();
        fs::create_dir_all(root.join("SYSTEM VOLUME INFORMATION")).unwrap();
        fs::write(root.join("a.dem"), b"").unwrap();
        fs::write(root.join("d/$Recycle.Bin/S-1-5/$R1A2B3C.dem"), b"").unwrap();
        fs::write(root.join("SYSTEM VOLUME INFORMATION/x.dem"), b"").unwrap();

        let mut files = vec![];
        scan_dir(&root, &mut files, &AtomicBool::new(false), &None);
        assert_eq!(files, [root.join("a.dem")]);
        let _ = fs::remove_dir_all(&root);
    }

    /// A folder tree with demos where a walk should find them and in every
    /// folder it should leave out.
    struct SkipTree(PathBuf);
    impl SkipTree {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("auditor_{name}_{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            for dir in [
                "repo/.git/objects",
                "app/NODE_MODULES/pkg",
                "OS/System32",
                "OS2",
                "keep/.github",
                "keep/node_modules_old",
                "venv/Lib/site-packages/torch/__pycache__",
            ] {
                fs::create_dir_all(root.join(dir)).unwrap();
            }
            for f in [
                "a.dem",
                "repo/.git/objects/x.dem",
                "app/NODE_MODULES/pkg/y.dem",
                "OS/System32/z.dem",
                "OS2/w.dem",
                "keep/.github/k.dem",
                "keep/node_modules_old/n.dem",
                "venv/Lib/site-packages/p.dem",
                "venv/Lib/site-packages/torch/__pycache__/q.dem",
                "venv/v.dem",
            ] {
                fs::write(root.join(f), b"").unwrap();
            }
            Self(root)
        }
        fn names(&self, mut found: Vec<PathBuf>) -> Vec<String> {
            found.sort();
            found
                .iter()
                .map(|p| {
                    p.strip_prefix(&self.0)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/")
                })
                .collect()
        }
    }
    impl Drop for SkipTree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn git_node_modules_and_the_windows_folder_are_skipped_below_a_walked_folder() {
        let tree = SkipTree::new("skip");
        // Pretend `OS` is the Windows folder, as %SystemRoot% would name it.
        let windows = comparable_path(&tree.0.join("os"));
        let cancel = AtomicBool::new(false);
        let found = scan_roots_with(
            std::slice::from_ref(&tree.0),
            &pool(4),
            Some(&windows),
            &cancel,
            &None,
        );
        assert_eq!(
            tree.names(found),
            [
                "OS2/w.dem",
                "a.dem",
                "keep/.github/k.dem",
                "keep/node_modules_old/n.dem",
                // A folder named venv is the user's to name: still read.
                "venv/v.dem"
            ]
        );
        // A folder picked inside the Windows folder is still read.
        let inside = tree.0.join("OS").join("System32");
        let found = scan_roots_with(
            std::slice::from_ref(&inside),
            &pool(4),
            Some(&windows),
            &cancel,
            &None,
        );
        assert_eq!(found, [inside.join("z.dem")]);
        // So is the Windows folder itself when it's the one picked.
        let found = scan_roots_with(
            &[tree.0.join("OS")],
            &pool(1),
            Some(&windows),
            &cancel,
            &None,
        );
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn the_windows_folder_matches_in_any_case_and_form() {
        let windows = Some(r"c:\windows");
        assert!(is_windows_folder_in(Path::new(r"C:\Windows"), windows));
        assert!(is_windows_folder_in(Path::new(r"\\?\C:\WINDOWS\"), windows));
        assert!(is_windows_folder_in(Path::new("C:/windows"), windows));
        assert!(!is_windows_folder_in(Path::new(r"D:\Windows"), windows));
        assert!(!is_windows_folder_in(Path::new(r"C:\Windows.old"), windows));
        assert!(!is_windows_folder_in(Path::new(r"C:\Windows"), None));
        assert_eq!(comparable_path(Path::new(r"\\?\UNC\nas\d\")), r"\\nas\d");
    }

    #[cfg(windows)]
    #[test]
    fn this_pcs_windows_folder_comes_from_the_system() {
        let windows = windows_folder().expect("SystemRoot is set on Windows");
        assert!(Path::new(windows).is_dir(), "{windows}");
        assert!(is_windows_folder(Path::new(windows)));
    }

    #[test]
    fn the_parallel_walk_finds_what_scan_dir_finds() {
        let tree = SkipTree::new("parallel");
        let mut one = Vec::new();
        let cancel = AtomicBool::new(false);
        // Two roots, one inside the other: both walks list its demos twice.
        let roots = [tree.0.clone(), tree.0.join("keep")];
        for root in &roots {
            scan_dir(root, &mut one, &cancel, &None);
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let many = scan_roots_with(&roots, &pool(8), None, &cancel, &Some(tx));
        assert_eq!(tree.names(many), tree.names(one));
        let folders = rx
            .try_iter()
            .filter(|m| matches!(m, AuditProgress::Scanning(_)))
            .count();
        // Every folder but the skipped .git, NODE_MODULES and site-packages
        // trees (venv and venv/Lib are read), keep's three twice.
        assert_eq!(folders, 14);
        // A cancel ends the walk at once.
        cancel.store(true, Ordering::Relaxed);
        assert!(scan_roots_with(&roots, &pool(4), None, &cancel, &None).is_empty());
    }

    /// Several roots walked into one list and keyed in one pass pair a copy
    /// in one with its original in the other (#844), and the key pass says
    /// how many files it will read first.
    #[test]
    fn one_key_pass_over_several_roots_groups_across_them() {
        let root = std::env::temp_dir().join(format!("auditor_roots_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("c/dod")).unwrap();
        fs::create_dir_all(root.join("f/downloads")).unwrap();
        fs::write(root.join("c/dod/match.dem"), b"same bytes").unwrap();
        fs::write(root.join("f/downloads/match (1).dem"), b"same bytes").unwrap();
        fs::write(root.join("f/downloads/other.dem"), b"other").unwrap();

        let cancel = AtomicBool::new(false);
        let mut files = vec![];
        for dir in [root.join("c"), root.join("f"), root.join("c/dod")] {
            scan_dir(&dir, &mut files, &cancel, &None);
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let (_, groups, copies, wasted) = find_duplicates(files, &cancel, &Some(tx));
        assert_eq!(groups.len(), 1, "{groups:?}");
        assert_eq!(groups[0].files.len(), 2);
        assert_eq!((copies, wasted), (1, 10));
        // c/dod was named twice; its demo is keyed once.
        let messages: Vec<_> = rx.try_iter().collect();
        assert!(
            matches!(messages.first(), Some(AuditProgress::Keying(3))),
            "{messages:?}"
        );
        // Every demo is counted once, the one with a size of its own too.
        let hashed = messages
            .iter()
            .filter(|m| matches!(m, AuditProgress::Hashing(_)))
            .count();
        assert_eq!(hashed, 3);
        let _ = fs::remove_dir_all(&root);
    }

    /// A scratch folder of demos for the key pass tests.
    struct Demos(PathBuf);
    impl Demos {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("auditor_{name}_{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, bytes).unwrap();
            fs::canonicalize(path).unwrap()
        }
    }
    impl Drop for Demos {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Same size and the same first and last 64 KiB; the middle byte is
    /// `middle`.
    fn long_demo(middle: u8) -> Vec<u8> {
        let mut bytes = vec![3u8; 4 * KEY_BYTES as usize];
        bytes[2 * KEY_BYTES as usize] = middle;
        bytes
    }

    fn key_pass(files: &[PathBuf], cache: &mut KeyCache) -> Audit {
        find_duplicates_cached(files.to_vec(), &AtomicBool::new(false), &None, cache)
    }

    fn group_names(audit: &Audit) -> Vec<Vec<String>> {
        let mut groups: Vec<Vec<String>> = audit
            .groups
            .iter()
            .map(|g| {
                g.files
                    .iter()
                    .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                    .collect()
            })
            .collect();
        groups.sort();
        groups
    }

    #[test]
    fn a_size_no_other_demo_has_is_never_read() {
        let d = Demos::new("lone_size");
        let files = [
            d.file("a.dem", b"twelve bytes"),
            d.file("b.dem", b"twelve bytes"),
            d.file("lone.dem", b"a size of its own"),
        ];
        let audit = key_pass(&files, &mut KeyCache::default());
        assert_eq!(group_names(&audit), [["a.dem", "b.dem"]]);
        assert_eq!(
            audit.reads,
            Reads {
                heads: 2,
                tails: 2,
                ..Reads::default()
            }
        );
        assert_eq!(audit.unique_files.len(), 2, "a/b's kept copy and lone");
        assert!(audit.unique_files.contains(&files[2]));
    }

    #[test]
    fn a_second_pass_reads_nothing_and_finds_the_same_groups() {
        let d = Demos::new("cache_hit");
        let files = [
            d.file("a.dem", &long_demo(0)),
            d.file("b.dem", &long_demo(0)),
            d.file("c.dem", &long_demo(1)),
            d.file("short1.dem", b"0123456789"),
            d.file("short2.dem", b"0123456789"),
        ];
        let mut cache = KeyCache::default();
        let first = key_pass(&files, &mut cache);
        assert_eq!(first.reads.heads, 5);
        let second = key_pass(&files, &mut cache);
        assert_eq!(second.reads, Reads::default());
        // No full hash is remembered yet: c's middle byte is not seen.
        assert_eq!(group_names(&first), group_names(&second));
        assert_eq!(
            group_names(&second),
            [
                vec!["a.dem", "b.dem", "c.dem"],
                vec!["short1.dem", "short2.dem"]
            ]
        );
    }

    #[test]
    fn a_changed_file_is_read_again() {
        let d = Demos::new("cache_miss");
        let a = d.file("a.dem", b"same size A");
        let b = d.file("b.dem", b"same size A");
        let mut cache = KeyCache::default();
        assert_eq!(
            group_names(&key_pass(&[a.clone(), b.clone()], &mut cache)).len(),
            1
        );
        // Same size, new bytes and a new modified time: no longer a copy.
        fs::write(&b, b"same size B").unwrap();
        let later = std::time::SystemTime::now() + Duration::from_secs(5);
        fs::File::options()
            .write(true)
            .open(&b)
            .unwrap()
            .set_modified(later)
            .unwrap();
        let audit = key_pass(&[a, b], &mut cache);
        assert_eq!(audit.reads.heads, 1, "only the changed file");
        assert!(audit.groups.is_empty());
    }

    #[test]
    fn a_remembered_whole_file_hash_splits_a_group_the_ends_cannot() {
        let d = Demos::new("full_memory");
        let keep = d.file("keep.dem", &long_demo(0));
        let copy = d.file("copy.dem", &long_demo(0));
        let other = d.file("other.dem", &long_demo(9));
        let later = d.file("later_copy.dem", &long_demo(0));
        let files = [keep.clone(), copy.clone(), other.clone()];
        let mut cache = KeyCache::default();
        assert_eq!(group_names(&key_pass(&files, &mut cache)).len(), 1);
        // The delete found `other` differs from `keep`, reading both in full.
        for path in [&keep, &other] {
            let hashes = content_hashes(path).unwrap();
            cache.remember_contents(path, stamp(path).unwrap(), hashes);
        }
        let audit = key_pass(&files, &mut cache);
        assert_eq!(group_names(&audit), [["copy.dem", "keep.dem"]]);
        assert_eq!(audit.reads.fulls, 1, "copy, to see which side it is on");
        // Known now: nothing is read in full again, and a new copy joins
        // the group it matches.
        let audit = key_pass(&[keep, copy, other, later], &mut cache);
        assert_eq!(audit.reads.fulls, 1, "only the new file");
        assert_eq!(
            group_names(&audit),
            [["copy.dem", "keep.dem", "later_copy.dem"]]
        );
    }

    #[test]
    fn one_remembered_whole_file_hash_proves_nothing() {
        let d = Demos::new("full_one");
        let a = d.file("a.dem", &long_demo(0));
        let b = d.file("b.dem", &long_demo(5));
        let mut cache = KeyCache::default();
        cache.remember_contents(&a, stamp(&a).unwrap(), content_hashes(&a).unwrap());
        let audit = key_pass(&[a, b], &mut cache);
        assert_eq!(group_names(&audit), [["a.dem", "b.dem"]]);
        assert_eq!(audit.reads.fulls, 0);
    }

    #[test]
    fn content_hashes_match_the_key_and_the_tail() {
        let d = Demos::new("content");
        let mut bytes: Vec<u8> = (0..(3 * KEY_BYTES as usize + 123))
            .map(|i| (i * 7 % 251) as u8)
            .collect();
        let long = d.file("long.dem", &bytes);
        let hashes = content_hashes(&long).unwrap();
        let key = get_file_key(&long).unwrap();
        assert_eq!(hashes.size, key.size);
        assert_eq!(hashes.head, key.header_hash);
        assert_eq!(hashes.tail, tail_hash(&long, key.size).unwrap());
        assert_eq!(hashes.full, fnv1a_hash(&bytes));
        bytes.truncate(100);
        let short = d.file("short.dem", &bytes);
        let hashes = content_hashes(&short).unwrap();
        assert_eq!(hashes.head, fnv1a_hash(&bytes));
        assert_eq!((hashes.tail, hashes.full), (0, fnv1a_hash(&bytes)));
    }

    #[test]
    fn par_map_keeps_the_order_and_stops_on_cancel() {
        let items: Vec<usize> = (0..100).collect();
        let cancel = AtomicBool::new(false);
        let out = par_map(&items, 4, &cancel, |i| i * 2);
        assert_eq!(out, items.iter().map(|i| Some(i * 2)).collect::<Vec<_>>());
        cancel.store(true, Ordering::Relaxed);
        assert!(
            par_map(&items, 4, &cancel, |i| *i)
                .iter()
                .all(Option::is_none)
        );
    }

    /// Readers that treat every drive as an SSD: `threads` at once.
    fn pool(threads: usize) -> Readers<'static> {
        Readers {
            threads,
            seeks: &|_| false,
        }
    }

    #[test]
    fn a_path_names_its_drive() {
        let drive = |p: &str| drive_of(Path::new(p));
        assert_eq!(drive(r"C:\demos\a.dem").as_deref(), Some("c:"));
        assert_eq!(drive(r"\\?\D:\demos\a.dem").as_deref(), Some("d:"));
        assert_eq!(drive("e:/a.dem").as_deref(), Some("e:"));
        assert_eq!(drive(r"\\nas\share\a.dem"), None);
        assert_eq!(drive(r"\\?\UNC\nas\share\a.dem"), None);
        assert_eq!(drive(r"demos\a.dem"), None);
    }

    #[test]
    fn each_hard_disk_gets_one_reader_and_the_ssds_share_the_pool() {
        let drives = [
            Some("c:"),
            Some("d:"),
            Some("e:"),
            None,
            Some("c:"),
            Some("d:"),
        ];
        let asked = Mutex::new(Vec::new());
        let seeks = |d: &str| {
            asked.lock().unwrap().push(d.to_string());
            d == "d:" || d == "e:"
        };
        let lanes = plan_lanes(drives.iter().map(|d| d.map(String::from)), 8, &seeks);
        assert_eq!(
            lanes,
            [
                Lane {
                    threads: 8,
                    items: vec![0, 3, 4]
                },
                Lane {
                    threads: 1,
                    items: vec![1, 5]
                },
                Lane {
                    threads: 1,
                    items: vec![2]
                },
            ]
        );
        // Each drive is asked once a pass.
        assert_eq!(*asked.lock().unwrap(), ["c:", "d:", "e:"]);
        // Only SSDs: one lane, as before #941.
        let lanes = plan_lanes(drives.iter().map(|d| d.map(String::from)), 8, &|_| false);
        assert_eq!(
            lanes,
            [Lane {
                threads: 8,
                items: (0..6).collect()
            }]
        );
        assert!(plan_lanes(std::iter::empty(), 8, &|_| false).is_empty());
    }

    #[test]
    fn a_hard_disk_is_read_one_file_at_a_time_beside_the_ssds() {
        // Fake paths: `f` only counts, it never opens them.
        let items: Vec<PathBuf> = (0..24)
            .map(|i| PathBuf::from(format!(r"{}:\demos\{i}.dem", ["c", "d", "e"][i % 3])))
            .collect();
        let counters = || -> HashMap<String, AtomicUsize> {
            ["c:", "d:", "e:"]
                .into_iter()
                .map(|d| (d.to_string(), AtomicUsize::new(0)))
                .collect()
        };
        let (in_flight, most) = (counters(), counters());
        let readers = Readers {
            threads: 4,
            seeks: &|d| d == "d:" || d == "e:",
        };
        let cancel = AtomicBool::new(false);
        let out = readers.map(
            &items,
            |p| drive_of(p),
            &cancel,
            |p| {
                let drive = drive_of(p).unwrap();
                let now = in_flight[&drive].fetch_add(1, Ordering::SeqCst) + 1;
                most[&drive].fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(5));
                in_flight[&drive].fetch_sub(1, Ordering::SeqCst);
                p.clone()
            },
        );
        // Every item, in order.
        assert_eq!(out, items.iter().cloned().map(Some).collect::<Vec<_>>());
        assert_eq!(most["d:"].load(Ordering::SeqCst), 1);
        assert_eq!(most["e:"].load(Ordering::SeqCst), 1);
        cancel.store(true, Ordering::Relaxed);
        assert!(
            readers
                .map(&items, |p| drive_of(p), &cancel, |p| p.clone())
                .iter()
                .all(Option::is_none)
        );
    }

    #[test]
    fn a_walk_and_key_pass_on_a_hard_disk_find_what_the_pool_finds() {
        let hdd = Readers {
            threads: 8,
            seeks: &|_| true,
        };
        let tree = SkipTree::new("hdd_walk");
        let cancel = AtomicBool::new(false);
        let roots = [tree.0.clone(), tree.0.join("keep")];
        let pooled = scan_roots_with(&roots, &pool(8), None, &cancel, &None);
        let one = scan_roots_with(&roots, &hdd, None, &cancel, &None);
        assert_eq!(tree.names(one), tree.names(pooled));

        let demos = Demos::new("hdd_keys");
        let files = vec![
            demos.file("a.dem", &long_demo(1)),
            demos.file("a (1).dem", &long_demo(1)),
            demos.file("b.dem", b"short"),
            demos.file("b (1).dem", b"short"),
            demos.file("c.dem", b"other"),
        ];
        let pass = |readers: &Readers| {
            let mut cache = KeyCache::default();
            let audit =
                find_duplicates_with(files.clone(), vec![], readers, &cancel, &None, &mut cache);
            (group_names(&audit), audit.reads)
        };
        let (groups, reads) = pass(&hdd);
        assert_eq!(groups.len(), 2, "{groups:?}");
        assert_eq!(pass(&pool(8)), (groups, reads));
    }

    /// What this PC's drives say (#941), for the PR: `cargo test -p
    /// hl-demo-auditor -- --ignored --nocapture drives_say`.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn print_what_this_pcs_drives_say() {
        for letter in b'a'..=b'z' {
            let drive = format!("{}:", letter as char);
            if Path::new(&format!(r"{drive}\")).exists() {
                println!("{drive} seek penalty: {:?}", seek_penalty::query(&drive));
            }
        }
    }

    use archive::tests::write_zip;

    /// A demo of `len` bytes whose content is set by `seed`.
    fn demo_of(len: usize, seed: u8) -> Vec<u8> {
        (0..len)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
            .collect()
    }

    fn zip_pass(
        files: &[PathBuf],
        zips: &[PathBuf],
        cache: &mut KeyCache,
    ) -> (Audit, Vec<AuditProgress>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let audit = find_duplicates_in(
            files.to_vec(),
            zips.to_vec(),
            &AtomicBool::new(false),
            &Some(tx),
            cache,
        );
        (audit, rx.try_iter().collect())
    }

    /// Each backed-up loose demo by name, with every archive entry holding
    /// it as `zip name › entry`.
    fn backup_names(audit: &Audit) -> Vec<(String, Vec<String>)> {
        audit
            .backups
            .iter()
            .map(|b| {
                let copies = b.copies.iter().map(|z| {
                    format!(
                        "{}{}{}",
                        z.zip.file_name().unwrap().to_string_lossy(),
                        archive::SEPARATOR,
                        z.entry
                    )
                });
                (
                    b.demo.file_name().unwrap().to_string_lossy().into_owned(),
                    copies.collect(),
                )
            })
            .collect()
    }

    fn backed_up(demo: &str, copies: &[&str]) -> (String, Vec<String>) {
        (
            demo.to_string(),
            copies
                .iter()
                .map(|c| c.replace(" > ", archive::SEPARATOR))
                .collect(),
        )
    }

    #[test]
    fn a_loose_demo_in_a_zip_is_backed_up_not_a_copy() {
        let d = Demos::new("zips_backup");
        let long = demo_of(3 * KEY_BYTES as usize + 500, 1);
        let short = demo_of(2000, 2);
        let other = demo_of(2000, 3);
        let loose = d.file("match.dem", &long);
        // Its size is its own among the loose demos: only the archive's copy
        // makes it worth reading.
        let loose_short = d.file("short.dem", &short);
        let lone = d.file("lone.dem", b"nobody has this size");
        let stored = d.0.join("stored.zip");
        write_zip(
            &stored,
            &[
                ("match.dem", &long, false),
                ("notes.txt", b"not a demo", false),
            ],
        );
        let deflated = d.0.join("deflated.zip");
        write_zip(
            &deflated,
            &[
                ("2025/match.dem", &long, true),
                ("short.dem", &short, true),
                ("other.dem", &other, true),
                ("only_here.dem", &demo_of(777, 4), true),
            ],
        );
        let corrupt = d.0.join("corrupt.zip");
        fs::write(&corrupt, b"PK\x05\x06 not a zip at all").unwrap();
        let zips = [stored, deflated, corrupt];

        let mut cache = KeyCache::default();
        let (audit, messages) = zip_pass(&[loose, loose_short, lone], &zips, &mut cache);
        // No group: an archive's copy is a backup, never a copy to delete.
        assert!(audit.groups.is_empty(), "{:?}", audit.groups);
        assert_eq!((audit.copies, audit.wasted_bytes), (0, 0));
        assert_eq!(audit.unique_files.len(), 3);
        // Every archive holding match.dem is listed.
        assert_eq!(
            backup_names(&audit),
            [
                backed_up(
                    "match.dem",
                    &["deflated.zip > 2025/match.dem", "stored.zip > match.dem"]
                ),
                backed_up("short.dem", &["deflated.zip > short.dem"]),
            ]
        );
        assert_eq!((audit.zips, audit.zip_demos), (3, 5));
        assert_eq!(audit.skipped.len(), 1, "{:?}", audit.skipped);
        assert!(
            audit.skipped[0].contains("corrupt.zip: "),
            "{:?}",
            audit.skipped
        );
        // Both match.dem entries are read; other.dem shares short's size, so
        // it is read too; only_here.dem has no loose demo of its size.
        assert_eq!(audit.reads.zips, 3);
        assert_eq!(audit.reads.zip_demos, 4);
        // Every archive's demos are listed as data, the unread one too.
        let listed: Vec<(String, usize, usize)> = audit
            .archives
            .iter()
            .map(|a| {
                (
                    a.archive
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    a.demos.len(),
                    a.demos.iter().filter(|e| e.hashes.is_some()).count(),
                )
            })
            .collect();
        assert_eq!(
            listed,
            [
                ("corrupt.zip".to_string(), 0, 0),
                ("deflated.zip".into(), 4, 3),
                ("stored.zip".into(), 1, 1),
            ]
        );

        // Every archive is reported read, then each demo, loose or archived,
        // once.
        assert!(matches!(messages.first(), Some(AuditProgress::Archives(3))));
        let count = |f: fn(&AuditProgress) -> bool| messages.iter().filter(|m| f(m)).count();
        assert_eq!(count(|m| matches!(m, AuditProgress::ReadingArchive(_))), 3);
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, AuditProgress::Keying(8)))
        );
        assert_eq!(count(|m| matches!(m, AuditProgress::Hashing(_))), 8);

        // A second pass opens no archive and finds the same backups.
        let files = [
            d.0.join("match.dem"),
            d.0.join("short.dem"),
            d.0.join("lone.dem"),
        ];
        let (again, _) = zip_pass(&files, &zips, &mut cache);
        assert_eq!(again.reads, Reads::default());
        assert_eq!(backup_names(&again), backup_names(&audit));
        assert_eq!(again.skipped, audit.skipped, "a bad zip is still reported");
    }

    #[test]
    fn loose_copies_group_as_without_archives_and_each_is_marked_backed_up() {
        let d = Demos::new("zips_loose_group");
        let bytes = demo_of(3 * KEY_BYTES as usize + 9, 5);
        let a = d.file("a.dem", &bytes);
        let b = d.file("b.dem", &bytes);
        let zip = d.0.join("pack.zip");
        write_zip(&zip, &[("m.dem", &bytes, true)]);
        let (audit, _) = zip_pass(&[a.clone(), b.clone()], &[zip], &mut KeyCache::default());
        let (_, plain, copies, wasted) =
            find_duplicates(vec![a, b], &AtomicBool::new(false), &None);
        // The same group as with no archive: one kept, one copy offered.
        assert_eq!(group_names(&audit), [["a.dem", "b.dem"]]);
        assert_eq!(plain.len(), 1);
        assert_eq!((audit.copies, audit.wasted_bytes), (copies, wasted));
        assert_eq!(audit.copies, 1);
        assert_eq!(
            backup_names(&audit),
            [
                backed_up("a.dem", &["pack.zip > m.dem"]),
                backed_up("b.dem", &["pack.zip > m.dem"]),
            ]
        );
    }

    #[test]
    fn a_demo_only_in_archives_forms_no_group_and_is_not_read() {
        let d = Demos::new("zips_only");
        let bytes = demo_of(5000, 9);
        let a = d.0.join("a.zip");
        let b = d.0.join("b.zip");
        write_zip(&a, &[("m.dem", &bytes, true)]);
        write_zip(&b, &[("m.dem", &bytes, false)]);
        let (audit, _) = zip_pass(&[], &[a, b], &mut KeyCache::default());
        assert!(audit.groups.is_empty());
        assert!(audit.backups.is_empty());
        assert_eq!(audit.reads.zip_demos, 0, "no loose demo has its size");
        // Still listed as data: which archives hold which demos.
        assert!(audit.archives.iter().all(|a| a.demos.len() == 1));
    }

    /// "Backed up" is a whole-file match: the same size and the same first
    /// and last 64 KiB are not enough, and the loose demo's full read is
    /// remembered.
    #[test]
    fn a_loose_demo_that_differs_only_in_the_middle_is_not_backed_up() {
        let d = Demos::new("zips_middle");
        let same = d.file("same.dem", &long_demo(0));
        let middle = d.file("middle.dem", &long_demo(1));
        let zip = d.0.join("pack.zip");
        write_zip(&zip, &[("m.dem", &long_demo(0), true)]);
        let files = [same, middle];
        let mut cache = KeyCache::default();
        let (audit, _) = zip_pass(&files, std::slice::from_ref(&zip), &mut cache);
        assert_eq!(
            backup_names(&audit),
            [backed_up("same.dem", &["pack.zip > m.dem"])]
        );
        assert_eq!(
            audit.reads.fulls, 2,
            "each loose candidate is read in full once"
        );
        // Their whole-file hashes, now known and different, split them, as
        // the cache does once the delete's check has read two files.
        assert!(audit.groups.is_empty(), "{:?}", audit.groups);
        let (again, _) = zip_pass(&files, &[zip], &mut cache);
        assert_eq!(again.reads, Reads::default());
        assert_eq!(backup_names(&again), backup_names(&audit));
    }

    #[test]
    fn a_changed_zip_is_read_again() {
        let d = Demos::new("zips_changed");
        let bytes = demo_of(5000, 9);
        let loose = d.file("m.dem", &bytes);
        let a = d.0.join("a.zip");
        let b = d.0.join("b.zip");
        write_zip(&a, &[("m.dem", &bytes, true)]);
        write_zip(&b, &[("m.dem", &bytes, false)]);
        let files = [loose];
        let mut cache = KeyCache::default();
        let (first, _) = zip_pass(&files, &[a.clone(), b.clone()], &mut cache);
        assert_eq!(first.reads.zip_demos, 2);
        assert_eq!(
            backup_names(&first),
            [backed_up("m.dem", &["a.zip > m.dem", "b.zip > m.dem"])]
        );

        write_zip(&b, &[("m.dem", &demo_of(5000, 10), false)]);
        let later = std::time::SystemTime::now() + Duration::from_secs(5);
        fs::File::options()
            .write(true)
            .open(&b)
            .unwrap()
            .set_modified(later)
            .unwrap();
        let (second, _) = zip_pass(&files, &[a, b], &mut cache);
        assert_eq!(second.reads.zips, 1, "only the changed zip");
        assert_eq!(second.reads.zip_demos, 1, "only its entry");
        assert_eq!(
            backup_names(&second),
            [backed_up("m.dem", &["a.zip > m.dem"])]
        );
    }

    #[test]
    fn the_walk_lists_zips_only_when_asked() {
        let d = Demos::new("zips_walk");
        d.file("a.dem", b"x");
        d.file("pack.ZIP", b"y");
        d.file("pack.7z", b"y");
        d.file("pack.rar", b"y");
        d.file("pack.tar", b"never");
        let cancel = AtomicBool::new(false);
        let roots = [d.0.clone()];
        let (demos, mut zips) = walk_roots(&roots, &pool(2), None, true, &cancel, &None);
        zips.sort();
        let names: Vec<_> = zips.iter().map(|z| z.file_name().unwrap()).collect();
        let rar = cfg!(all(windows, feature = "rar"));
        let expected: &[&str] = if rar {
            &["pack.7z", "pack.ZIP", "pack.rar"]
        } else {
            &["pack.7z", "pack.ZIP"]
        };
        assert_eq!(demos.len(), 1);
        assert_eq!(names, expected);
        assert_eq!(
            scan_roots_with(&roots, &pool(2), None, &cancel, &None).len(),
            1
        );
    }

    #[test]
    fn demos_inside_7z_and_rar_files_back_up_their_loose_copies() {
        use archive::sevenz::tests::write_7z;
        let d = Demos::new("7z_rar_backup");
        let long = demo_of(3 * KEY_BYTES as usize + 500, 4);
        // The same size, other bytes: read too, and matches nothing.
        let other = demo_of(3 * KEY_BYTES as usize + 500, 5);
        let loose = d.file("match.dem", &long);
        let solid = d.0.join("solid.7z");
        write_7z(
            &solid,
            &[("first.dem", &other), ("x/match.dem", &long)],
            true,
        );
        let plain = d.0.join("plain.7z");
        write_7z(&plain, &[("match.dem", &long)], false);
        let corrupt = d.0.join("corrupt.7z");
        fs::write(&corrupt, b"7z\xbc\xaf\x27\x1c not a 7z at all").unwrap();
        let mut files = vec![loose];
        let mut archives = vec![solid, plain, corrupt];
        let mut expected = vec![backed_up(
            "match.dem",
            &["plain.7z > match.dem", "solid.7z > x/match.dem"],
        )];
        let rar = cfg!(all(windows, feature = "rar"));
        if rar {
            // archive::rar's fixture: match/a.dem, readme.txt, and an
            // encrypted locked.dem.
            let pack = d.file("pack.rar", include_bytes!("archive/testdata/pack.rar"));
            let bytes: Vec<u8> = (0..3000).map(|i| ((i * 31 + 5 * 7) % 251) as u8).collect();
            files.push(d.file("a.dem", &bytes));
            archives.push(pack);
            expected.insert(0, backed_up("a.dem", &["pack.rar > match/a.dem"]));
        }

        let mut cache = KeyCache::default();
        let (audit, _) = zip_pass(&files, &archives, &mut cache);
        assert!(audit.groups.is_empty());
        assert_eq!(audit.copies, 0);
        assert_eq!(backup_names(&audit), expected);
        let skipped = audit.skipped.join("\n");
        assert!(skipped.contains("corrupt.7z: "), "{skipped}");
        // Plain words and paths: no Debug form, no verbatim prefix.
        assert!(!skipped.contains("(\""), "{skipped}");
        assert!(!skipped.contains(r"\\?\"), "{skipped}");
        assert_eq!(
            rar,
            skipped.contains("pack.rar › locked.dem: it is encrypted"),
            "{skipped}"
        );
        assert_eq!(audit.skipped.len(), if rar { 2 } else { 1 });
        // Both 7z copies of match.dem and first.dem, which shares its size.
        assert_eq!(audit.reads.zip_demos, if rar { 4 } else { 3 });

        // A second pass opens no archive and finds the same backups.
        let (again, _) = zip_pass(&files, &archives, &mut cache);
        assert_eq!(again.reads, Reads::default());
        assert_eq!(backup_names(&again), backup_names(&audit));
    }

    /// Two archives listing one entry with the same size and CRC-32, one of
    /// them damaged, in both orders and on a second (cached) pass: only the
    /// good one is listed as holding the loose demo.
    fn a_damaged_twin_never_shows_as_a_backup(
        ext: &str,
        good: &[u8],
        bad: &[u8],
        entry: &str,
        loose: &[u8],
    ) {
        for good_first in [true, false] {
            let d = Demos::new(&format!("twin_{ext}_{good_first}"));
            let (good_name, bad_name) = if good_first {
                (format!("a_good.{ext}"), format!("b_bad.{ext}"))
            } else {
                (format!("b_good.{ext}"), format!("a_bad.{ext}"))
            };
            let archives = [d.file(&good_name, good), d.file(&bad_name, bad)];
            let files = [d.file("loose.dem", loose)];
            let mut cache = KeyCache::default();
            for pass in ["first", "second"] {
                let (audit, _) = zip_pass(&files, &archives, &mut cache);
                assert_eq!(
                    backup_names(&audit),
                    [(
                        "loose.dem".to_string(),
                        vec![format!("{good_name}{}{entry}", archive::SEPARATOR)]
                    )],
                    "{ext}, good first: {good_first}, {pass} pass"
                );
                assert!(audit.groups.is_empty());
            }
        }
    }

    #[test]
    fn a_damaged_zip_with_the_same_header_never_shows_as_a_backup() {
        let d = Demos::new("twin_zip_src");
        let bytes = demo_of(3 * KEY_BYTES as usize + 500, 7);
        let zip = d.0.join("src.zip");
        write_zip(&zip, &[("m.dem", &bytes, false)]);
        let good = fs::read(&zip).unwrap();
        // Stored, so the middle of the file is the entry's data.
        let mut bad = good.clone();
        let mid = bad.len() / 2;
        bad[mid] ^= 0xff;
        a_damaged_twin_never_shows_as_a_backup("zip", &good, &bad, "m.dem", &bytes);
    }

    #[test]
    fn a_damaged_7z_with_the_same_header_never_shows_as_a_backup() {
        use archive::sevenz::tests::write_7z;
        let d = Demos::new("twin_7z_src");
        let bytes = demo_of(3 * KEY_BYTES as usize + 500, 8);
        let path = d.0.join("src.7z");
        write_7z(&path, &[("m.dem", &bytes)], false);
        let good = fs::read(&path).unwrap();
        // The packed data follows the 32-byte signature header; the header
        // that lists the entry is at the end.
        let mut bad = good.clone();
        bad[40] ^= 0xff;
        a_damaged_twin_never_shows_as_a_backup("7z", &good, &bad, "m.dem", &bytes);
    }

    /// The live check's case: `pack.rar` cut to 1500 bytes still lists
    /// match/a.dem with the good one's size and CRC-32.
    #[cfg(all(windows, feature = "rar"))]
    #[test]
    fn a_truncated_rar_with_the_same_header_never_shows_as_a_backup() {
        let good = include_bytes!("archive/testdata/pack.rar");
        let loose: Vec<u8> = (0..3000).map(|i| ((i * 31 + 5 * 7) % 251) as u8).collect();
        a_damaged_twin_never_shows_as_a_backup("rar", good, &good[..1500], "match/a.dem", &loose);
    }

    #[test]
    fn shown_paths_lose_the_verbatim_prefix_only() {
        assert_eq!(
            shown_path(Path::new(r"\\?\C:\Demos\A.dem")),
            r"C:\Demos\A.dem"
        );
        assert_eq!(
            shown_path(Path::new(r"\\?\UNC\nas\d\a.dem")),
            r"\\nas\d\a.dem"
        );
        assert_eq!(shown_path(Path::new(r"D:\a.dem")), r"D:\a.dem");
        let at = archive::ZipRef {
            zip: PathBuf::from(r"\\?\C:\packs\p.7z"),
            entry: "x/m.dem".into(),
        };
        assert_eq!(
            at.shown(),
            format!(r"C:\packs\p.7z{}x/m.dem", archive::SEPARATOR)
        );
    }
}
