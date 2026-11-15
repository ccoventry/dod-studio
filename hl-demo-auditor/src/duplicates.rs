//! The duplicate pass behind [`crate::find_duplicates_in`]: paths in, an
//! [`Audit`] out, in stages that each have a contract of their own. The
//! order of the stages, and so of every read, progress message and
//! cancellation check, is the order in [`find_duplicates_with`].

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use crate::keys::head_hash;
use crate::lanes::{Readers, drive_of};
use crate::zips::{ZipDemo, list_zips, read_zip_demos};
use crate::{
    ArchiveListing, ArchivedCopy, Audit, AuditProgress, Backup, BackupIndex, DuplicateGroup,
    FileKey, KeyCache, Reads, Stamp, content_hashes, fnv1a_hash, stamp, tail_hash,
};

/// A path made canonical, with its stamp (`None` when it can't be read).
type Resolved = (PathBuf, Option<Stamp>);

/// A file's members of a group: indices into the resolved loose files.
type Members = Vec<usize>;

/// A group of its own for a file that can't be read, as when keying fails.
fn unreadable_key(path: &Path) -> FileKey {
    FileKey {
        size: 0,
        header_hash: fnv1a_hash(path.to_string_lossy().as_bytes()),
    }
}

/// Paths resolved to their canonical form with their stamps, each once,
/// sorted.
fn resolve_paths(paths: &[PathBuf], readers: &Readers, cancel: &AtomicBool) -> Vec<Resolved> {
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
    let mut out: Vec<Resolved> = by_path.into_iter().collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// How many readable loose files have each size.
fn loose_size_counts(files: &[Resolved]) -> HashMap<u64, usize> {
    let mut sizes: HashMap<u64, usize> = HashMap::new();
    for (_, stamp) in files {
        if let Some(s) = stamp {
            *sizes.entry(s.size).or_default() += 1;
        }
    }
    sizes
}

/// Loose files grouped by key; `singles` are the ones with no key (a cancel
/// skipped their read).
fn group_by_key(keys: &[Option<FileKey>]) -> (HashMap<FileKey, Members>, Members) {
    let mut groups: HashMap<FileKey, Members> = HashMap::new();
    let mut singles: Members = Vec::new();
    for (i, key) in keys.iter().enumerate() {
        match key {
            Some(key) => groups.entry(key.clone()).or_default().push(i),
            None => singles.push(i),
        }
    }
    (groups, singles)
}

/// The archived copies a loose demo can be compared with: every entry that
/// was read in full and has no error.
fn backup_index(zip_demos: &[ZipDemo], zips: &[Resolved]) -> BackupIndex {
    BackupIndex::new(zip_demos.iter().filter_map(|z| {
        let hashes = z.entry.hashes.filter(|_| z.entry.error.is_none())?;
        Some(ArchivedCopy {
            at: z.at(zips),
            hashes,
        })
    }))
}

/// Each archive with the entries that could be listed.
fn archive_listings(zips: &[Resolved], zip_demos: &[ZipDemo]) -> Vec<ArchiveListing> {
    let mut listings: Vec<ArchiveListing> = zips
        .iter()
        .map(|(path, _)| ArchiveListing {
            archive: path.clone(),
            demos: Vec::new(),
        })
        .collect();
    for z in zip_demos {
        if z.entry.error.is_none() {
            listings[z.zip].demos.push(z.entry.clone());
        }
    }
    listings
}

/// Turns the split groups into the audit's lists: each group's first sorted
/// path is the one kept, groups of two or more are duplicates, the most
/// wasted space first and then by path. Stops at a cancel with what it has.
fn collect_groups(
    audit: &mut Audit,
    split: Vec<(FileKey, Members)>,
    files: &[Resolved],
    cancel: &AtomicBool,
) {
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
    members: Members,
    files: &[Resolved],
    cache: &mut KeyCache,
    reads: &mut Reads,
) -> Vec<Members> {
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
    let mut parts: HashMap<u64, Members> = HashMap::new();
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

/// What the stages of one pass share: how to read, when to stop, where to
/// report, the cache, and what has been read and skipped so far.
struct Pass<'a> {
    readers: &'a Readers<'a>,
    cancel: &'a AtomicBool,
    progress: &'a Option<Sender<AuditProgress>>,
    cache: &'a mut KeyCache,
    reads: Reads,
    skipped: Vec<String>,
}

/// Sends `msg` to whoever listens, if anyone.
fn send(progress: &Option<Sender<AuditProgress>>, msg: AuditProgress) {
    if let Some(tx) = progress {
        let _ = tx.send(msg);
    }
}

/// Reports a demo as looked at, whether it was read, remembered or never
/// needed a key.
fn hashing(progress: &Option<Sender<AuditProgress>>, path: &Path) {
    send(
        progress,
        AuditProgress::Hashing(path.to_string_lossy().into_owned()),
    );
}

impl Pass<'_> {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// The archives' lists of demo entries (from the cache where it can),
    /// after announcing how many there are. `None` when a cancel came
    /// during the read.
    fn list_archives(&mut self, zips: &[Resolved]) -> Option<Vec<ZipDemo>> {
        if zips.is_empty() {
            return Some(Vec::new());
        }
        send(self.progress, AuditProgress::Archives(zips.len()));
        let zip_demos = list_zips(
            zips,
            self.readers,
            self.cancel,
            self.progress,
            self.cache,
            &mut self.reads,
            &mut self.skipped,
        );
        (!self.cancelled()).then_some(zip_demos)
    }

    /// Every loose file's key (its size and first 64 KiB) where one is
    /// needed: a file nothing else shares a size with can't be a copy or
    /// backed up, so it gets none. Remembered keys come from the cache; the
    /// rest are read, a cancel leaving some `None`.
    fn key_loose(
        &mut self,
        files: &[Resolved],
        loose_sizes: &HashMap<u64, usize>,
        zip_demos: &[ZipDemo],
    ) -> Vec<Option<FileKey>> {
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
                    hashing(self.progress, path);
                }
                Some(s) if !worth_keying(s.size) => hashing(self.progress, path),
                Some(s) => match self.cache.entry(path, *s) {
                    Some(entry) => {
                        keys[i] = Some(FileKey {
                            size: s.size,
                            header_hash: entry.head,
                        });
                        hashing(self.progress, path);
                    }
                    None => to_read.push(i),
                },
            }
        }
        self.read_heads(files, &to_read, &mut keys);
        keys
    }

    /// Reads the first 64 KiB of the `to_read` files, by drive, into `keys`
    /// and the cache; a file that can't be read gets a group of its own.
    fn read_heads(&mut self, files: &[Resolved], to_read: &[usize], keys: &mut [Option<FileKey>]) {
        let progress = self.progress;
        let heads = self.readers.map(
            to_read,
            |&i| drive_of(&files[i].0),
            self.cancel,
            |&i| {
                let (path, stamp) = &files[i];
                hashing(progress, path);
                head_hash(path, stamp.map_or(0, |s| s.size))
            },
        );
        for (&i, head) in to_read.iter().zip(heads) {
            let Some(head) = head else { continue };
            self.reads.heads += 1;
            let (path, stamp) = &files[i];
            let Some(stamp) = *stamp else { continue };
            keys[i] = Some(match head {
                Ok(head) => {
                    self.cache.put(path, stamp, head);
                    FileKey {
                        size: stamp.size,
                        header_hash: head,
                    }
                }
                Err(_) => unreadable_key(path),
            });
        }
    }

    /// Reads the archive entries a loose demo shares a size with, in full,
    /// into `zip_demos` and the cache; an entry that can't be read is
    /// skipped and named in `self.skipped`.
    ///
    /// A demo in an archive is read in full or not at all: deflate, LZMA and
    /// RAR can't seek to its last 64 KiB. Each such entry is read itself, so
    /// every archive holding a demo is listed: a size and CRC-32 in a header
    /// say nothing about the data behind them, and a damaged archive listing
    /// the same header must not show as a backup.
    fn read_archived(
        &mut self,
        zip_demos: &mut [ZipDemo],
        zips: &[Resolved],
        loose_sizes: &HashMap<u64, usize>,
    ) {
        let mut needed = Vec::new();
        for (j, z) in zip_demos.iter().enumerate() {
            if z.entry.hashes.is_some() || !loose_sizes.contains_key(&z.entry.size) {
                send(self.progress, AuditProgress::Hashing(z.at(zips).display()));
            } else {
                needed.push(j);
            }
        }
        if needed.is_empty() || self.cancelled() {
            return;
        }
        let read = read_zip_demos(
            &needed,
            zip_demos,
            zips,
            self.readers,
            self.cancel,
            self.progress,
        );
        for (j, read) in read {
            self.reads.zip_demos += 1;
            let (path, stamp) = &zips[zip_demos[j].zip];
            if let Some(stamp) = stamp {
                self.cache
                    .set_archive_hashes(path, *stamp, &zip_demos[j].entry.name, read.clone());
            }
            match read {
                Ok(h) => zip_demos[j].entry.hashes = Some(h),
                Err(e) => {
                    self.skipped
                        .push(format!("{}: {e}", zip_demos[j].at(zips).shown()));
                    zip_demos[j].entry.error = Some(e);
                }
            }
        }
    }

    /// The hash of each file's last 64 KiB, for the files the key alone
    /// can't settle.
    ///
    /// The key reads only the start, so two cuts of one recording can share
    /// it. Files that end differently are different demos: only grouped
    /// files, and files an archive holds a demo with the same key, pay the
    /// extra read. The key itself stays as it is, since project files save
    /// its text. A file that can't be read here is a group of its own.
    fn tails(
        &mut self,
        files: &[Resolved],
        groups: &HashMap<FileKey, Members>,
        index: &BackupIndex,
    ) -> Vec<Option<u64>> {
        let mut tails: Vec<Option<u64>> = vec![None; files.len()];
        let mut tail_reads = Vec::new();
        for (key, members) in groups {
            if members.len() < 2 && !index.has_key(key) {
                continue;
            }
            for &i in members {
                let (path, stamp) = &files[i];
                match stamp
                    .and_then(|s| self.cache.entry(path, s))
                    .and_then(|e| e.tail)
                {
                    Some(tail) => tails[i] = Some(tail),
                    None => tail_reads.push((i, key.size)),
                }
            }
        }
        let drive = |&(i, _): &(usize, u64)| drive_of(&files[i].0);
        let read_tails = self
            .readers
            .map(&tail_reads, drive, self.cancel, |&(i, size)| {
                tail_hash(&files[i].0, size)
            });
        for (&(i, _), tail) in tail_reads.iter().zip(read_tails) {
            let Some(tail) = tail else { continue };
            self.reads.tails += 1;
            let (path, stamp) = &files[i];
            tails[i] = Some(match tail {
                Ok(tail) => {
                    if let Some(s) = stamp {
                        self.cache.set_tail(path, *s, tail);
                    }
                    tail
                }
                Err(_) => fnv1a_hash(path.to_string_lossy().as_bytes()),
            });
        }
        tails
    }

    /// Every loose demo an archive holds a copy of, in file order. A demo
    /// whose key and tail match an archived one is read in full once (then
    /// remembered while its size and modified time hold) to be sure.
    fn find_backups(
        &mut self,
        files: &[Resolved],
        keys: &[Option<FileKey>],
        tails: &[Option<u64>],
        index: &BackupIndex,
    ) -> Vec<Backup> {
        let mut backups = Vec::new();
        if index.is_empty() {
            return backups;
        }
        for (i, key) in keys.iter().enumerate() {
            let (Some(key), Some(tail)) = (key, tails[i]) else {
                continue;
            };
            if self.cancelled() {
                break;
            }
            let (path, stamp) = &files[i];
            let known = stamp
                .and_then(|s| self.cache.entry(path, s))
                .and_then(|e| e.full);
            let copies = index.copies_of(key, tail, || {
                known.or_else(|| read_full(path, *stamp, self.cache, &mut self.reads))
            });
            if !copies.is_empty() {
                backups.push(Backup {
                    demo: path.clone(),
                    copies,
                });
            }
        }
        backups
    }

    /// The groups of loose files that are copies of one another: each key's
    /// members split by tail, then by whole-file hash where the cache proves
    /// two differ ([`split_by_full_hash`]), followed by a one-file group for
    /// each file with no key. Stops at a cancel with what it has.
    fn split_groups(
        &mut self,
        groups: HashMap<FileKey, Members>,
        singles: Members,
        files: &[Resolved],
        tails: &[Option<u64>],
    ) -> Vec<(FileKey, Members)> {
        let mut split: Vec<(FileKey, Members)> = Vec::new();
        for (key, members) in groups {
            if self.cancelled() {
                break;
            }
            if members.len() < 2 {
                split.push((key, members));
                continue;
            }
            let mut by_tail: HashMap<u64, Members> = HashMap::new();
            for i in members {
                if let Some(tail) = tails[i] {
                    by_tail.entry(tail).or_default().push(i);
                }
            }
            for (_, same_tail) in by_tail {
                for part in split_by_full_hash(same_tail, files, self.cache, &mut self.reads) {
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
        split
    }
}

/// [`crate::find_duplicates_in`] reading as `readers` says: every read below,
/// the first and last 64 KiB and the archives, one at a time on a hard disk.
pub(crate) fn find_duplicates_with(
    files: Vec<PathBuf>,
    zips: Vec<PathBuf>,
    readers: &Readers,
    cancel: &AtomicBool,
    progress: &Option<Sender<AuditProgress>>,
    cache: &mut KeyCache,
) -> Audit {
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
    let mut pass = Pass {
        readers,
        cancel,
        progress,
        cache,
        reads: Reads::default(),
        skipped: Vec::new(),
    };
    let Some(mut zip_demos) = pass.list_archives(&zips) else {
        return Audit::default();
    };
    send(
        progress,
        AuditProgress::Keying(files.len() + zip_demos.len()),
    );

    let loose_sizes = loose_size_counts(&files);
    let keys = pass.key_loose(&files, &loose_sizes, &zip_demos);
    pass.read_archived(&mut zip_demos, &zips, &loose_sizes);
    let index = backup_index(&zip_demos, &zips);

    let (groups, singles) = group_by_key(&keys);
    let tails = pass.tails(&files, &groups, &index);
    let backups = pass.find_backups(&files, &keys, &tails, &index);
    let split = pass.split_groups(groups, singles, &files, &tails);

    let mut audit = Audit {
        reads: pass.reads,
        backups,
        archives: archive_listings(&zips, &zip_demos),
        zips: zips.len(),
        zip_demos: zip_demos.len(),
        skipped: pass.skipped,
        ..Audit::default()
    };
    collect_groups(&mut audit, split, &files, cancel);
    audit
}
