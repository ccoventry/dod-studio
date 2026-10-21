use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Condvar, Mutex, OnceLock, PoisonError};
use std::time::{Duration, UNIX_EPOCH};

mod key_cache;
pub use key_cache::{KeyCache, KeyEntry};

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

#[derive(Debug, Clone)]
pub struct DuplicateGroup {
    pub key: FileKey,
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub enum AuditProgress {
    Scanning(String),
    /// How many demos [`find_duplicates`] looks at, once paths named twice
    /// are merged; sent before the first `Hashing`, so a bar can show how
    /// far the key pass has got. Each of them then sends one `Hashing`,
    /// whether it was read, remembered or never needed a key.
    Keying(usize),
    Hashing(String),
    Found(usize),
    Failed(String),
    Done(Vec<DuplicateGroup>, usize, u64),
}

/// Deterministic FNV-1a 64-bit hashing.
pub fn fnv1a_hash(data: &[u8]) -> u64 {
    let mut hash = Fnv1a::new();
    hash.update(data);
    hash.finish()
}

/// [`fnv1a_hash`] fed in pieces: the same result as one call over them all.
#[derive(Clone, Copy, Debug)]
pub struct Fnv1a(u64);

impl Default for Fnv1a {
    fn default() -> Self {
        Self::new()
    }
}

impl Fnv1a {
    pub const fn new() -> Self {
        Self(0xcbf29ce484222325)
    }

    pub fn update(&mut self, data: &[u8]) {
        for &byte in data {
            self.0 ^= byte as u64;
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }

    pub fn finish(self) -> u64 {
        self.0
    }
}

/// Threads a walk or key pass uses: separate drives list and read at once,
/// and an SSD answers several requests at a time even on one drive.
fn worker_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(2, 8)
}

/// Recursively scans a directory for `.dem` files with cancellation and progress reporting.
pub fn scan_dir(
    dir: &Path,
    files: &mut Vec<PathBuf>,
    cancel: &AtomicBool,
    progress: &Option<Sender<AuditProgress>>,
) {
    scan_dir_to_depth(dir, files, cancel, progress, 0);
}

/// Folder levels [`scan_dir`] reads below the folder it is given.
const SCAN_MAX_DEPTH: usize = 32;

/// Folders [`scan_dir`] never enters, by name at any depth: a demo in the
/// Recycle Bin is a deleted copy, not a duplicate to clean up (#843), System
/// Volume Information is usually unreadable, and `node_modules` and `.git`
/// are tens of thousands of folders that never hold a demo (#888). The same
/// list as `native::sys::folder_walk::SYSTEM_FOLDERS`, which can't be used
/// here (`native` depends on this crate); a test there checks they agree.
pub const SYSTEM_FOLDERS: [&str; 4] = [
    "$RECYCLE.BIN",
    "System Volume Information",
    "node_modules",
    ".git",
];

fn is_system_folder(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        let name = name.to_string_lossy();
        SYSTEM_FOLDERS.iter().any(|s| name.eq_ignore_ascii_case(s))
    })
}

/// A path as text for comparing: no `\\?\` prefix, backslashes, lower case,
/// no trailing separator.
pub fn comparable_path(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\");
    let text = match text.strip_prefix(r"\\?\UNC\") {
        Some(rest) => format!(r"\\{rest}"),
        None => text.strip_prefix(r"\\?\").unwrap_or(&text).to_string(),
    };
    text.trim_end_matches('\\').to_lowercase()
}

/// The Windows folder as [`comparable_path`] writes it, from `%SystemRoot%`
/// (what `GetWindowsDirectoryW` reports), so a PC that installed Windows
/// somewhere other than `C:\Windows` is still matched. `None` off Windows.
pub fn windows_folder() -> Option<&'static str> {
    static FOLDER: OnceLock<Option<String>> = OnceLock::new();
    FOLDER
        .get_or_init(|| {
            std::env::var_os("SystemRoot")
                .or_else(|| std::env::var_os("windir"))
                .map(|dir| comparable_path(Path::new(&dir)))
                .filter(|dir| !dir.is_empty())
        })
        .as_deref()
}

/// Whether `path` is the folder `windows` names ([`windows_folder`]'s form).
/// Only a folder found inside a walked one is ever checked, so a walk from a
/// drive root skips the Windows folder (about 5 s of C:'s 20 s, and no demos,
/// #888), while a folder the user picks inside it is still read.
pub fn is_windows_folder_in(path: &Path, windows: Option<&str>) -> bool {
    let Some(windows) = windows else {
        return false;
    };
    let last = windows.rsplit('\\').next().unwrap_or(windows);
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(last))
        && comparable_path(path) == windows
}

/// [`is_windows_folder_in`] this PC's Windows folder.
pub fn is_windows_folder(path: &Path) -> bool {
    is_windows_folder_in(path, windows_folder())
}

/// A folder found inside a walked one that the walk leaves out.
fn skips_folder(path: &Path, windows: Option<&str>) -> bool {
    is_system_folder(path) || is_windows_folder_in(path, windows)
}

/// `file_type` never follows a junction or symlink, so a link is never
/// entered; with the depth cap, no folder layout can make this loop (#376).
fn scan_dir_to_depth(
    dir: &Path,
    files: &mut Vec<PathBuf>,
    cancel: &AtomicBool,
    progress: &Option<Sender<AuditProgress>>,
    depth: usize,
) {
    if cancel.load(Ordering::Relaxed) {
        return;
    }
    let mut subdirs = Vec::new();
    list_dir(
        dir,
        depth,
        windows_folder(),
        cancel,
        progress,
        &mut subdirs,
        files,
    );
    for (sub, sub_depth) in subdirs {
        scan_dir_to_depth(&sub, files, cancel, progress, sub_depth);
    }
}

/// One folder's listing: its demos into `demos`, the folders to walk next
/// into `subdirs`.
fn list_dir(
    dir: &Path,
    depth: usize,
    windows: Option<&str>,
    cancel: &AtomicBool,
    progress: &Option<Sender<AuditProgress>>,
    subdirs: &mut Vec<(PathBuf, usize)>,
    demos: &mut Vec<PathBuf>,
) {
    if let Some(tx) = progress {
        let _ = tx.send(AuditProgress::Scanning(dir.to_string_lossy().into_owned()));
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let path = entry.path();
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            if depth < SCAN_MAX_DEPTH && !skips_folder(&path, windows) {
                subdirs.push((path, depth + 1));
            }
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("dem"))
        {
            demos.push(path);
        }
    }
}

/// [`scan_dir`] over several roots at once, several folders listed at a
/// time (#888): every drive's walk runs alongside the others, and a big one
/// (C:) is shared out between the threads. Finds the same demos as calling
/// [`scan_dir`] on each root, in no particular order.
pub fn scan_roots(
    roots: &[PathBuf],
    cancel: &AtomicBool,
    progress: &Option<Sender<AuditProgress>>,
) -> Vec<PathBuf> {
    scan_roots_with(roots, worker_threads(), windows_folder(), cancel, progress)
}

fn scan_roots_with(
    roots: &[PathBuf],
    threads: usize,
    windows: Option<&str>,
    cancel: &AtomicBool,
    progress: &Option<Sender<AuditProgress>>,
) -> Vec<PathBuf> {
    /// Folders waiting to be listed, and how many are being listed now: the
    /// walk is over when both are none.
    struct Work {
        pending: Vec<(PathBuf, usize)>,
        busy: usize,
    }
    let work = Mutex::new(Work {
        pending: roots.iter().rev().map(|r| (r.clone(), 0)).collect(),
        busy: 0,
    });
    let ready = Condvar::new();
    let found = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..threads.max(1) {
            scope.spawn(|| {
                let mut demos = Vec::new();
                loop {
                    let next = {
                        let mut w = work.lock().unwrap_or_else(PoisonError::into_inner);
                        loop {
                            if cancel.load(Ordering::Relaxed) {
                                break None;
                            }
                            if let Some(item) = w.pending.pop() {
                                w.busy += 1;
                                break Some(item);
                            }
                            if w.busy == 0 {
                                break None;
                            }
                            // Timed, so a cancel is seen while others list.
                            w = ready
                                .wait_timeout(w, Duration::from_millis(50))
                                .unwrap_or_else(PoisonError::into_inner)
                                .0;
                        }
                    };
                    let Some((dir, depth)) = next else {
                        break;
                    };
                    let mut subdirs = Vec::new();
                    list_dir(
                        &dir,
                        depth,
                        windows,
                        cancel,
                        progress,
                        &mut subdirs,
                        &mut demos,
                    );
                    let mut w = work.lock().unwrap_or_else(PoisonError::into_inner);
                    w.pending.extend(subdirs.into_iter().rev());
                    w.busy -= 1;
                    drop(w);
                    ready.notify_all();
                }
                ready.notify_all();
                found
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .extend(demos);
            });
        }
    });
    found.into_inner().unwrap_or_else(PoisonError::into_inner)
}

/// The demos named on a command line: each file as given, each folder walked
/// with [`scan_dir`]. Sorted, so a run over the same folder prints in the same
/// order. Paths that are neither come back in the second list.
pub fn collect_demos(paths: &[PathBuf]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let cancel = AtomicBool::new(false);
    let mut demos = vec![];
    let mut missing = vec![];
    for path in paths {
        if path.is_file() {
            demos.push(path.clone());
        } else if path.is_dir() {
            scan_dir(path, &mut demos, &cancel, &None);
        } else {
            missing.push(path.clone());
        }
    }
    demos.sort();
    demos.dedup();
    (demos, missing)
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

/// Generates a `FileKey` for a file based on its size and an FNV-1a hash of its first 64KB.
pub fn get_file_key(path: &Path) -> Result<FileKey, std::io::Error> {
    let size = fs::metadata(path)?.len();
    Ok(FileKey {
        size,
        header_hash: head_hash(path, size)?,
    })
}

/// An FNV-1a hash of a file's first [`KEY_BYTES`] (all of it when shorter).
fn head_hash(path: &Path, size: u64) -> Result<u64, std::io::Error> {
    let mut file = fs::File::open(path)?;
    let mut buffer = vec![0; std::cmp::min(size, KEY_BYTES) as usize];
    file.read_exact(&mut buffer)?;
    Ok(fnv1a_hash(&buffer))
}

/// An FNV-1a hash of a file's last [`KEY_BYTES`], or 0 when the file is no
/// longer than that (its key already covers all of it).
pub fn tail_hash(path: &Path, size: u64) -> Result<u64, std::io::Error> {
    use std::io::{Seek, SeekFrom};
    if size <= KEY_BYTES {
        return Ok(0);
    }
    let mut file = fs::File::open(path)?;
    file.seek(SeekFrom::Start(size - KEY_BYTES))?;
    let mut buffer = vec![0; KEY_BYTES as usize];
    file.read_exact(&mut buffer)?;
    Ok(fnv1a_hash(&buffer))
}

/// A file's size and modified time, which say whether a remembered key
/// still holds. `modified_ns` is 0 when the system can't say, and such a
/// file is never remembered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub size: u64,
    pub modified_ns: u64,
}

/// `path`'s [`Stamp`], or `None` when it can't be read.
pub fn stamp(path: &Path) -> Option<Stamp> {
    let meta = fs::metadata(path).ok()?;
    let modified_ns = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos().min(u64::MAX as u128) as u64)
        .unwrap_or(0);
    Some(Stamp {
        size: meta.len(),
        modified_ns,
    })
}

/// Everything the audit hashes in a file, from one read of all of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContentHashes {
    pub size: u64,
    /// [`FileKey::header_hash`].
    pub head: u64,
    /// [`tail_hash`].
    pub tail: u64,
    /// FNV-1a over every byte.
    pub full: u64,
}

/// [`ContentHashes`] for `path`, read in 1 MiB pieces. An error when the
/// file changes size while it is read.
pub fn content_hashes(path: &Path) -> Result<ContentHashes, std::io::Error> {
    let mut file = fs::File::open(path)?;
    let size = file.metadata()?.len();
    let head_end = size.min(KEY_BYTES);
    let tail_start = if size > KEY_BYTES {
        size - KEY_BYTES
    } else {
        u64::MAX
    };
    let (mut head, mut tail, mut full) = (Fnv1a::new(), Fnv1a::new(), Fnv1a::new());
    let mut buf = vec![0u8; 1 << 20];
    let mut offset = 0u64;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let chunk = &buf[..n];
        let end = offset + n as u64;
        if offset < head_end {
            head.update(&chunk[..(head_end.min(end) - offset) as usize]);
        }
        if end > tail_start {
            tail.update(&chunk[(tail_start.saturating_sub(offset)) as usize..]);
        }
        full.update(chunk);
        offset = end;
    }
    if offset != size {
        return Err(std::io::Error::other("the file changed size while read"));
    }
    Ok(ContentHashes {
        size,
        head: head.finish(),
        tail: if size > KEY_BYTES { tail.finish() } else { 0 },
        full: full.finish(),
    })
}

/// How many files each step of a key pass had to open: the rest were
/// remembered ([`KeyCache`]) or, for the first 64 KiB, never needed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reads {
    pub heads: usize,
    pub tails: usize,
    pub fulls: usize,
}

/// What [`find_duplicates_cached`] found.
#[derive(Debug, Default)]
pub struct Audit {
    pub unique_files: Vec<PathBuf>,
    pub groups: Vec<DuplicateGroup>,
    pub copies: usize,
    pub wasted_bytes: u64,
    pub reads: Reads,
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
    let threads = worker_threads();
    let mut reads = Reads::default();
    let hashing = |path: &Path| {
        if let Some(tx) = progress {
            let _ = tx.send(AuditProgress::Hashing(path.to_string_lossy().into_owned()));
        }
    };

    // Canonicalize and deduplicate paths to prevent counting/grouping a file
    // with itself. Canonicalizing opens each file, so it runs in parallel.
    let resolved = par_map(&files, threads, cancel, |path| {
        let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        let stamp = stamp(&canonical);
        (canonical, stamp)
    });
    if cancel.load(Ordering::Relaxed) {
        return Audit::default();
    }
    let mut by_path: HashMap<PathBuf, Option<Stamp>> = HashMap::new();
    for (path, stamp) in resolved.into_iter().flatten() {
        by_path.entry(path).or_insert(stamp);
    }
    let mut files: Vec<(PathBuf, Option<Stamp>)> = by_path.into_iter().collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    if let Some(tx) = progress {
        let _ = tx.send(AuditProgress::Keying(files.len()));
    }
    for (path, _) in &files {
        cache.mark_seen(path);
    }

    // A size no other demo has can't be a copy: such a file is not read.
    let mut sizes: HashMap<u64, usize> = HashMap::new();
    for (_, stamp) in &files {
        if let Some(s) = stamp {
            *sizes.entry(s.size).or_default() += 1;
        }
    }
    let mut keys: Vec<Option<FileKey>> = vec![None; files.len()];
    let mut to_read = Vec::new();
    for (i, (path, stamp)) in files.iter().enumerate() {
        match stamp {
            None => {
                keys[i] = Some(unreadable_key(path));
                hashing(path);
            }
            Some(s) if sizes.get(&s.size).copied().unwrap_or(0) < 2 => hashing(path),
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
    let heads = par_map(&to_read, threads, cancel, |&i| {
        let (path, stamp) = &files[i];
        hashing(path);
        head_hash(path, stamp.map_or(0, |s| s.size))
    });
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
    // by its last bytes too. Only grouped files pay the extra read. The key
    // itself stays as it is, since project files save its text.
    let mut tails: Vec<Option<u64>> = vec![None; files.len()];
    let mut tail_reads = Vec::new();
    for (key, members) in &groups {
        if members.len() < 2 {
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
    let read_tails = par_map(&tail_reads, threads, cancel, |&(i, size)| {
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

    let mut audit = Audit {
        reads,
        ..Audit::default()
    };
    for (key, members) in split {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        // Sorted, so original vs duplicate selection is stable.
        let mut paths: Vec<PathBuf> = members.into_iter().map(|i| files[i].0.clone()).collect();
        paths.sort();
        let Some(first) = paths.first() else { continue };
        audit.unique_files.push(first.clone());
        if paths.len() > 1 {
            audit.copies += paths.len() - 1;
            audit.wasted_bytes += key.size * (paths.len() - 1) as u64;
            audit.groups.push(DuplicateGroup { key, files: paths });
        }
    }
    // Sort unique files alphabetically so traversal is deterministic
    audit.unique_files.sort();
    // Most wasted space first; then by path, so the order is stable.
    audit.groups.sort_by(|a, b| {
        let space_a = a.key.size * (a.files.len() - 1) as u64;
        let space_b = b.key.size * (b.files.len() - 1) as u64;
        space_b
            .cmp(&space_a)
            .then_with(|| a.files[0].cmp(&b.files[0]))
    });
    audit
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
    let mut parts: HashMap<Option<u64>, Vec<usize>> = HashMap::new();
    let mut alone = Vec::new();
    for (&i, full) in members.iter().zip(known) {
        let full = full.or_else(|| {
            let (path, stamp) = &files[i];
            reads.fulls += 1;
            let hashes = content_hashes(path).ok()?;
            if let Some(s) = stamp
                && s.size == hashes.size
            {
                cache.remember_contents(path, *s, hashes);
            }
            Some(hashes.full)
        });
        match full {
            Some(_) => parts.entry(full).or_default().push(i),
            None => alone.push(vec![i]),
        }
    }
    parts.into_values().chain(alone).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn fnv_matches_the_reference_vectors() {
        assert_eq!(fnv1a_hash(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a_hash(b"a"), 0xaf63_dc4c_8601_ec8c);
        let mut pieces = Fnv1a::new();
        pieces.update(b"ab");
        pieces.update(b"c");
        assert_eq!(pieces.finish(), fnv1a_hash(b"abc"));
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
        // The fixture named directly and found in its folder is listed once.
        assert_eq!(demos.len(), 1, "{demos:?}");
        assert!(demos[0].ends_with("ci_fixture.dem"));
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
            4,
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
                "keep/node_modules_old/n.dem"
            ]
        );
        // A folder picked inside the Windows folder is still read.
        let inside = tree.0.join("OS").join("System32");
        let found = scan_roots_with(
            std::slice::from_ref(&inside),
            4,
            Some(&windows),
            &cancel,
            &None,
        );
        assert_eq!(found, [inside.join("z.dem")]);
        // So is the Windows folder itself when it's the one picked.
        let found = scan_roots_with(&[tree.0.join("OS")], 1, Some(&windows), &cancel, &None);
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
        let many = scan_roots_with(&roots, 8, None, &cancel, &Some(tx));
        assert_eq!(tree.names(many), tree.names(one));
        let folders = rx
            .try_iter()
            .filter(|m| matches!(m, AuditProgress::Scanning(_)))
            .count();
        // Every folder but the skipped .git and NODE_MODULES trees, keep's
        // three twice.
        assert_eq!(folders, 12);
        // A cancel ends the walk at once.
        cancel.store(true, Ordering::Relaxed);
        assert!(scan_roots_with(&roots, 4, None, &cancel, &None).is_empty());
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
                fulls: 0
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
}
