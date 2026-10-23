//! Demo Auditor › Duplicates: one key pass over several folders, or every
//! fixed drive, so a copy on one drive pairs with its original on another
//! (#844). Groups list the copy to keep first; deletes go to the Recycle Bin
//! unless permanent deletion is asked for. A group is keyed by size and the
//! first 64 KiB only, so each copy is compared byte for byte with the kept
//! one before it is deleted.
//!
//! Each demo's key is remembered between scans (`audit_keys.json`, #888), so
//! a second scan costs the folder walk and opens only new or changed demos.
//! A copy the delete finds different has its whole-file hash remembered
//! there too, and later scans no longer group it with the kept copy.
//!
//! "Show demos backed up in archives" (#929) also reads the demos inside
//! `.zip`, `.7z` and `.rar` files. An archive is a backup: a loose demo it
//! holds is marked as backed up, with every archive entry holding it, and
//! is never offered for deletion because of it; groups hold loose copies
//! only. The names here still say `zip` (`ZipAt`, `lookInZips`) for any of
//! the three: they are the wire and settings names zip support shipped with.

use hl_demo_auditor::{
    AuditProgress, Backup, DuplicateGroup, KeyCache, ZipRef, find_duplicates_in, scan_roots,
    scan_roots_and_zips,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Instant;
use tauri::{AppHandle, Emitter};

pub struct AuditManager {
    pub is_running: Arc<AtomicBool>,
    pub cancel_token: Arc<AtomicBool>,
}

impl Default for AuditManager {
    fn default() -> Self {
        Self {
            is_running: Arc::new(AtomicBool::new(false)),
            cancel_token: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// Why a copy should stay: the game or the open project reads it (#844).
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InUse {
    /// Inside an install's game folder (`dod`): the configured one, or any
    /// other `dod` beside an `hl.exe`.
    GameFolder,
    /// In the current project's queue.
    Queue,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct AuditFile {
    /// A loose demo's path.
    pub path: String,
    pub in_use: Option<InUse>,
    /// Every archive entry holding a copy of it (#929), when there is one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub backups: Vec<ZipAt>,
}

/// A demo inside an archive, as the frontend gets it.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ZipAt {
    pub zip: String,
    pub entry: String,
}

impl ZipAt {
    fn from_ref(z: &ZipRef) -> Self {
        Self {
            zip: z.zip.to_string_lossy().into_owned(),
            entry: z.entry.clone(),
        }
    }
}

#[derive(Serialize)]
pub struct SerializedDuplicateGroup {
    pub size: u64,
    pub header_hash: u64,
    /// The copy to keep first ([`order_for_keeping`]).
    pub files: Vec<AuditFile>,
}

/// The groups as the frontend gets them, each copy carrying the archive
/// entries that hold it, and every backed-up loose demo by path, in or out
/// of a group (#929).
fn audit_rows(
    groups: Vec<DuplicateGroup>,
    backups: &[Backup],
    rules: &KeepRules,
) -> (Vec<SerializedDuplicateGroup>, Vec<AuditFile>) {
    let entries = |b: &Backup| b.copies.iter().map(ZipAt::from_ref).collect::<Vec<_>>();
    let by_path: HashMap<String, &Backup> = backups
        .iter()
        .map(|b| (b.demo.to_string_lossy().into_owned(), b))
        .collect();
    let groups = groups
        .into_iter()
        .map(|g| {
            let mut files = order_for_keeping(g.files, rules);
            for file in &mut files {
                if let Some(b) = by_path.get(&file.path) {
                    file.backups = entries(b);
                }
            }
            SerializedDuplicateGroup {
                size: g.key.size,
                header_hash: g.key.header_hash,
                files,
            }
        })
        .collect();
    let backed_up = backups
        .iter()
        .map(|b| {
            let path = b.demo.to_string_lossy().into_owned();
            AuditFile {
                in_use: rules.in_use(&comparable(&b.demo)),
                path,
                backups: entries(b),
            }
        })
        .collect();
    (groups, backed_up)
}

#[derive(Serialize)]
pub struct AuditOutcome {
    pub groups: Vec<SerializedDuplicateGroup>,
    /// Every loose demo an archive holds a copy of (#929), by path, whether
    /// or not it is in a group.
    pub backed_up: Vec<AuditFile>,
    /// Demos keyed.
    pub demos: usize,
    /// Cancelled before the key pass ended: `groups` is empty, since a
    /// partial pass would list originals whose copies it never reached.
    pub cancelled: bool,
}

/// Every fixed drive's root (`C:\` and so on), sorted. Removable, network
/// and optical drives are left out: "All drives" means the PC's own disks.
pub fn fixed_drive_roots() -> Vec<String> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mut roots: Vec<String> = disks
        .list()
        .iter()
        .filter(|d| !d.is_removable())
        .map(|d| d.mount_point().to_string_lossy().into_owned())
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

/// A path as text for comparing: no `\\?\` prefix, backslashes, lower case,
/// no trailing separator. Windows paths are case-insensitive, and the key
/// pass hands back canonical (`\\?\`) paths while the queue holds plain ones.
fn comparable(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\");
    let text = match text.strip_prefix(r"\\?\UNC\") {
        Some(rest) => format!(r"\\{rest}"),
        None => text.strip_prefix(r"\\?\").unwrap_or(&text).to_string(),
    };
    text.trim_end_matches('\\').to_lowercase()
}

/// The canonical form when the path resolves, else the path as given.
fn resolved(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The folders and files whose copies an audit should keep.
#[derive(Default)]
pub struct KeepRules {
    game_folders: Vec<String>,
    queued: HashSet<String>,
}

impl KeepRules {
    /// Resolves each path, so a queued `C:\demos\a.dem` matches the key
    /// pass's `\\?\C:\demos\a.dem` and a folder reached through a link
    /// matches its target.
    pub fn new(game_folders: &[PathBuf], queued: &[String]) -> Self {
        Self {
            game_folders: game_folders
                .iter()
                .map(|f| comparable(&resolved(f)))
                .collect(),
            queued: queued
                .iter()
                .map(|q| comparable(&resolved(Path::new(q))))
                .collect(),
        }
    }

    fn in_use(&self, file: &str) -> Option<InUse> {
        let in_folder = |folder: &String| {
            file.strip_prefix(folder.as_str())
                .is_some_and(|rest| rest.starts_with('\\'))
        };
        if self.game_folders.iter().any(in_folder) {
            Some(InUse::GameFolder)
        } else if self.queued.contains(file) {
            Some(InUse::Queue)
        } else {
            None
        }
    }
}

/// The game folders (`<install>\dod`, beside an `hl.exe`) that any of
/// `files` sits in, each checked on disk once.
pub fn install_game_folders<'a>(files: impl Iterator<Item = &'a PathBuf>) -> Vec<PathBuf> {
    let mut checked: HashMap<PathBuf, bool> = HashMap::new();
    let mut found = Vec::new();
    for file in files {
        for dir in file.ancestors().skip(1) {
            let is_dod = dir
                .file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case(GAME_FOLDER_NAME));
            if !is_dod {
                continue;
            }
            let hit = *checked.entry(dir.to_path_buf()).or_insert_with(|| {
                let yes = dir.parent().is_some_and(|p| p.join("hl.exe").is_file());
                if yes {
                    found.push(dir.to_path_buf());
                }
                yes
            });
            if hit {
                break;
            }
        }
    }
    found
}

/// The game's own folder name in every install DoD Studio supports.
const GAME_FOLDER_NAME: &str = "dod";

/// One group's files, the copy to keep first: one the game folder or the
/// queue uses, then the shortest path, then by name so the pick is stable.
pub fn order_for_keeping(files: Vec<PathBuf>, rules: &KeepRules) -> Vec<AuditFile> {
    let mut ranked: Vec<(bool, usize, String, AuditFile)> = files
        .into_iter()
        .map(|path| {
            let shown = comparable(&path);
            let in_use = rules.in_use(&shown);
            let file = AuditFile {
                path: path.to_string_lossy().into_owned(),
                in_use,
                backups: Vec::new(),
            };
            (in_use.is_none(), shown.len(), shown, file)
        })
        .collect();
    ranked.sort_by(|a, b| (a.0, a.1, &a.2).cmp(&(b.0, b.1, &b.2)));
    ranked.into_iter().map(|(_, _, _, file)| file).collect()
}

/// The folders to walk: the ones picked, plus every fixed drive when asked,
/// without blanks or repeats (case-insensitive).
pub fn audit_roots(folders: &[String], drives: &[String]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    folders
        .iter()
        .chain(drives)
        .map(|f| f.trim())
        .filter(|f| !f.is_empty())
        .filter(|f| seen.insert(comparable(Path::new(f))))
        .map(PathBuf::from)
        .collect()
}

/// What `audit_progress` carries; the frontend words the line
/// (`STRINGS.AUDITOR`). `total` is 0 while folders are still being listed.
#[derive(Serialize, Clone, Default)]
struct AuditProgressEvent {
    folders: usize,
    /// Zips found and zips whose list of demos is read (#929); between the
    /// folder walk and the key pass, while `total` is still 0.
    zips_total: usize,
    zips_done: usize,
    total: usize,
    done: usize,
    current: String,
}

/// Where the audit remembers each demo's key between scans.
fn key_cache_file() -> PathBuf {
    native::shared::paths::get_appdata_dir().join("audit_keys.json")
}

/// What one audit did, for its end line in the activity log.
struct AuditTimes {
    cancelled: bool,
    walk_secs: f64,
    key_secs: f64,
    demos: usize,
    groups: usize,
    copies: usize,
    wasted_bytes: u64,
    reads: hl_demo_auditor::Reads,
    /// Archives looked inside, the demos in them, and the loose demos they
    /// back up (#929); `None` when the audit didn't look.
    zips: Option<(usize, usize, usize)>,
}

/// The activity log's line when an audit ends: how long the folder walk and
/// the key pass took, what it found, and how many files it had to open (the
/// rest were remembered from an earlier scan, #888).
fn audit_end_log(t: &AuditTimes) -> String {
    let total = t.walk_secs + t.key_secs;
    if t.cancelled {
        return format!("[audit] cancelled after {total:.1} s");
    }
    let zips = match t.zips {
        Some((zips, demos, backed_up)) => format!(
            "; looked inside {zips} archive(s) holding {demos} demo(s), opened {} archive(s) and \
             read {} demo(s) in them in full, the rest remembered; {backed_up} loose demo(s) \
             backed up in an archive",
            t.reads.zips, t.reads.zip_demos,
        ),
        None => String::new(),
    };
    format!(
        "[audit] finished after {total:.1} s (walk {:.1} s, keys {:.1} s): {} demo(s) checked, \
         {} group(s) with copies, {} copies, {:.2} GB; opened {} start(s), {} end(s), {} whole file(s), \
         the rest remembered{zips}",
        t.walk_secs,
        t.key_secs,
        t.demos,
        t.groups,
        t.copies,
        t.wasted_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
        t.reads.heads,
        t.reads.tails,
        t.reads.fulls,
    )
}

/// The most zips or zipped demos the activity log names as skipped in one
/// audit; the rest are counted.
const SKIPPED_LINES: usize = 20;

/// The activity log's lines for zips, or demos in them, the audit left out
/// because they couldn't be read (corrupt, encrypted).
fn skipped_log(skipped: &[String]) -> Vec<String> {
    let mut lines: Vec<String> = skipped
        .iter()
        .take(SKIPPED_LINES)
        .map(|s| format!("[audit] skipped {s}"))
        .collect();
    if skipped.len() > SKIPPED_LINES {
        lines.push(format!(
            "[audit] skipped {} more archive(s) or archived demo(s) that couldn't be read",
            skipped.len() - SKIPPED_LINES
        ));
    }
    lines
}

/// The activity log's line when an audit starts: what it walks.
fn audit_start_log(folders: &[String], drives: &[String], look_in_zips: bool) -> String {
    let mut what: Vec<String> = Vec::new();
    if !drives.is_empty() {
        what.push(format!("all drives ({})", drives.join(" ")));
    }
    if !folders.is_empty() {
        what.push(format!("{} folder(s)", folders.len()));
    }
    let zips = if look_in_zips {
        ", looking inside archives (.zip, .7z, .rar)"
    } else {
        ""
    };
    format!("[audit] started: {}{zips}", what.join(" and "))
}

/// Runs the audit off the main thread: lists every root's demos, keys them
/// all in one pass and orders each group for keeping.
pub async fn run_demo_audit_impl(
    app_handle: AppHandle,
    is_running: Arc<AtomicBool>,
    cancel_token: Arc<AtomicBool>,
    folders: Vec<String>,
    all_drives: bool,
    look_in_zips: bool,
    rules: (Vec<PathBuf>, Vec<String>),
) -> Result<AuditOutcome, String> {
    is_running.store(true, Ordering::SeqCst);
    cancel_token.store(false, Ordering::SeqCst);
    let is_running_end = Arc::clone(&is_running);

    let result = crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let drives = if all_drives {
            fixed_drive_roots()
        } else {
            Vec::new()
        };
        let roots = audit_roots(&folders, &drives);
        let missing: Vec<&PathBuf> = roots.iter().filter(|r| !r.is_dir()).collect();
        if roots.is_empty() || missing.len() == roots.len() {
            let why = match missing.first() {
                Some(root) => crate::messages::audit_folder_not_found(root),
                None => crate::messages::NO_AUDIT_FOLDERS.to_string(),
            };
            native::log_markdown(&format!("[audit] failed: {why}"));
            return Err(why);
        }
        native::log_markdown(&audit_start_log(&folders, &drives, look_in_zips));
        let (tx, rx) = mpsc::channel();
        let ah = app_handle.clone();
        // At most one event per ~33 ms, the first at once (CLAUDE.md).
        let progress_handle = std::thread::spawn(move || {
            let started = Instant::now();
            let last_emit = AtomicU32::new(u32::MAX);
            let mut event = AuditProgressEvent::default();
            while let Ok(msg) = rx.recv() {
                match msg {
                    AuditProgress::Scanning(p) => {
                        event.folders += 1;
                        event.current = p;
                    }
                    AuditProgress::Archives(total) => event.zips_total = total,
                    AuditProgress::ReadingArchive(p) => {
                        event.zips_done += 1;
                        event.current = p;
                    }
                    AuditProgress::Keying(total) => event.total = total,
                    AuditProgress::Hashing(p) => {
                        event.done += 1;
                        event.current = p;
                    }
                    _ => continue,
                }
                let now_ms = started.elapsed().as_millis().min(u32::MAX as u128 - 1) as u32;
                if crate::capture_manager::progress_emit_due(&last_emit, now_ms) {
                    let _ = ah.emit("audit_progress", event.clone());
                }
            }
            event
        });

        let walked: Vec<PathBuf> = roots.into_iter().filter(|r| r.is_dir()).collect();
        let walk_started = Instant::now();
        let (files, zips) = if look_in_zips {
            scan_roots_and_zips(&walked, &cancel_token, &Some(tx.clone()))
        } else {
            (
                scan_roots(&walked, &cancel_token, &Some(tx.clone())),
                Vec::new(),
            )
        };
        let walk_secs = walk_started.elapsed().as_secs_f64();
        let loose_demos = files.len();
        let key_started = Instant::now();
        let cache_file = key_cache_file();
        let mut cache = KeyCache::load(&cache_file);
        let audit = find_duplicates_in(files, zips, &cancel_token, &Some(tx), &mut cache);
        let demos = loose_demos + audit.zip_demos;
        let key_secs = key_started.elapsed().as_secs_f64();
        let last = progress_handle.join().unwrap_or_default();
        let cancelled = cancel_token.load(Ordering::Relaxed);
        if !cancelled {
            for line in skipped_log(&audit.skipped) {
                native::log_markdown(&line);
            }
        }
        native::log_markdown(&audit_end_log(&AuditTimes {
            cancelled,
            walk_secs,
            key_secs,
            demos,
            groups: audit.groups.len(),
            copies: audit.copies,
            wasted_bytes: audit.wasted_bytes,
            reads: audit.reads,
            zips: look_in_zips.then_some((audit.zips, audit.zip_demos, audit.backups.len())),
        }));
        let duplicate_groups = audit.groups;
        if !cancelled {
            cache.prune_unseen_under(&walked, look_in_zips);
        }
        // A cancelled pass still saves the keys it read. Only a cache: a
        // failed save costs the next scan time, nothing else.
        let _ = cache.save(&cache_file);

        let (groups, backed_up) = if cancelled {
            (Vec::new(), Vec::new())
        } else {
            // Every install the copies sit in counts, not only the one the
            // hl.exe setting names: a PC with a stock, a PRE and a POST
            // install keeps each one's demos.
            let mut game_folders = rules.0.clone();
            game_folders.extend(install_game_folders(
                duplicate_groups
                    .iter()
                    .flat_map(|g| g.files.iter())
                    .chain(audit.backups.iter().map(|b| &b.demo)),
            ));
            let keep_rules = KeepRules::new(&game_folders, &rules.1);
            audit_rows(duplicate_groups, &audit.backups, &keep_rules)
        };
        Ok(AuditOutcome {
            groups,
            backed_up,
            demos: last.done,
            cancelled,
        })
    }))
    .await;

    is_running_end.store(false, Ordering::SeqCst);
    result
}

/// One file the delete could not remove, and why.
#[derive(Serialize, Debug, PartialEq)]
pub struct DeleteFailure {
    pub path: String,
    pub error: String,
    /// It differs from the copy its group keeps: not a duplicate, so the
    /// table marks it rather than leaving it ticked.
    pub not_a_copy: bool,
}

/// Why [`check_same_contents`] refused a copy.
#[derive(Debug, PartialEq)]
enum Refusal {
    /// The bytes differ: a different demo.
    NotACopy(String),
    /// Anything else: the kept copy is gone, is the same file, can't be read.
    Other(String),
}

impl Refusal {
    fn into_failure(self, path: String) -> DeleteFailure {
        let (error, not_a_copy) = match self {
            Refusal::NotACopy(e) => (e, true),
            Refusal::Other(e) => (e, false),
        };
        DeleteFailure {
            path,
            error,
            not_a_copy,
        }
    }
}

#[derive(Serialize, Debug, Default)]
pub struct DeleteOutcome {
    pub deleted: Vec<String>,
    pub failed: Vec<DeleteFailure>,
}

/// Only a demo file is ever deleted: the paths come from the frontend.
fn check_deletable(path: &Path) -> Result<(), String> {
    let is_demo = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("dem"));
    let is_file = path.symlink_metadata().is_ok_and(|m| m.is_file());
    if is_demo && is_file {
        Ok(())
    } else {
        Err(crate::messages::not_a_demo_file(path))
    }
}

/// One copy to delete, and the copy its group keeps.
#[derive(Deserialize, Debug, Clone)]
pub struct DeleteCopy {
    pub path: String,
    pub keep: String,
}

/// Bytes read from each file per comparison step.
const COMPARE_CHUNK: usize = 1 << 20;

/// Fills `buf` from `file` until it is full or the file ends; how many bytes.
fn read_chunk(file: &mut dyn Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match file.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

/// Whether `copy` is a true copy of `keep`: another file with every byte
/// the same. The audit's key reads only the first 64 KiB, so two different
/// demos can share a group; this keeps the delete from trusting that.
fn check_same_contents(copy: &Path, keep: &Path) -> Result<(), Refusal> {
    let kept_is_there = keep.symlink_metadata().is_ok_and(|m| m.is_file());
    if !kept_is_there {
        return Err(Refusal::Other(crate::messages::kept_copy_missing(
            copy, keep,
        )));
    }
    if let (Ok(a), Ok(b)) = (std::fs::canonicalize(copy), std::fs::canonicalize(keep))
        && a == b
    {
        return Err(Refusal::Other(crate::messages::copy_is_the_kept_copy(copy)));
    }
    let unreadable =
        |e: std::io::Error| Refusal::Other(crate::messages::copy_compare_failed(copy, keep, e));
    let mut a = std::fs::File::open(copy).map_err(unreadable)?;
    let mut b = std::fs::File::open(keep).map_err(unreadable)?;
    let (len_a, len_b) = (
        a.metadata().map_err(unreadable)?.len(),
        b.metadata().map_err(unreadable)?.len(),
    );
    compare_readers(copy, keep, (&mut a, len_a), (&mut b, len_b))
}

/// Whether `a` (the copy, `len_a` bytes) and `b` (the kept copy) hold the
/// same bytes; `copy` and `keep` only name them in the refusal.
fn compare_readers(
    copy: &Path,
    keep: &Path,
    (a, len_a): (&mut dyn Read, u64),
    (b, len_b): (&mut dyn Read, u64),
) -> Result<(), Refusal> {
    let unreadable =
        |e: std::io::Error| Refusal::Other(crate::messages::copy_compare_failed(copy, keep, e));
    let differs =
        |at: u64| Refusal::NotACopy(crate::messages::copy_differs_from_kept(copy, keep, at));
    if len_a != len_b {
        return Err(differs(len_a.min(len_b)));
    }
    let mut buf_a = vec![0u8; COMPARE_CHUNK];
    let mut buf_b = vec![0u8; COMPARE_CHUNK];
    let mut offset: u64 = 0;
    loop {
        let n_a = read_chunk(a, &mut buf_a).map_err(unreadable)?;
        let n_b = read_chunk(b, &mut buf_b).map_err(unreadable)?;
        if let Some(i) = buf_a[..n_a.min(n_b)]
            .iter()
            .zip(&buf_b[..n_a.min(n_b)])
            .position(|(x, y)| x != y)
        {
            return Err(differs(offset + i as u64));
        }
        if n_a != n_b {
            // The file changed size while it was read.
            return Err(differs(offset + n_a.min(n_b) as u64));
        }
        if n_a == 0 {
            return Ok(());
        }
        offset += n_a as u64;
    }
}

/// Deletes the picked copies: to the Recycle Bin, or for good when
/// `permanent`. Each is first compared byte for byte with the copy its group
/// keeps, and one that differs is not deleted. Emits `audit_delete_progress`
/// (`step` "checking" then "deleting", `done`, `total`), throttled.
pub fn delete_audit_files_impl(
    app_handle: &AppHandle,
    copies: Vec<DeleteCopy>,
    permanent: bool,
) -> DeleteOutcome {
    let mut outcome = DeleteOutcome::default();
    let started = Instant::now();
    let last_emit = AtomicU32::new(u32::MAX);
    let emit = |step: &str, done: usize, total: usize| {
        let now_ms = started.elapsed().as_millis().min(u32::MAX as u128 - 1) as u32;
        if done == total || crate::capture_manager::progress_emit_due(&last_emit, now_ms) {
            let _ = app_handle.emit(
                "audit_delete_progress",
                serde_json::json!({ "step": step, "done": done, "total": total }),
            );
        }
    };
    let checking = copies.len();
    let cache_file = key_cache_file();
    let mut cache = KeyCache::load(&cache_file);
    let mut deletable = vec![];
    for (i, copy) in copies.into_iter().enumerate() {
        let path = Path::new(&copy.path);
        let keep = Path::new(&copy.keep);
        match check_deletable(path)
            .map_err(Refusal::Other)
            .and_then(|()| check_same_contents(path, keep))
        {
            Ok(()) => deletable.push(copy.path),
            Err(refusal) => {
                if matches!(refusal, Refusal::NotACopy(_)) {
                    remember_differing(&mut cache, path, keep);
                }
                outcome.failed.push(refusal.into_failure(copy.path))
            }
        }
        emit("checking", i + 1, checking);
    }
    let total = deletable.len();
    let mut report = |done: usize| emit("deleting", done, total);

    let errors: Vec<Option<String>> = if permanent {
        let mut errors = Vec::with_capacity(total);
        for (i, path) in deletable.iter().enumerate() {
            errors.push(std::fs::remove_file(path).err().map(|e| e.to_string()));
            report(i + 1);
        }
        errors
    } else {
        let list = deletable.iter().map(PathBuf::from).collect();
        native::sys::recycle_bin::recycle(list, &mut report)
    };
    for (path, error) in deletable.into_iter().zip(errors) {
        match error {
            None => {
                cache.forget(Path::new(&path));
                outcome.deleted.push(path)
            }
            Some(e) => outcome.failed.push(DeleteFailure {
                error: crate::messages::failed_to_delete_audit_file(Path::new(&path), e),
                path,
                not_a_copy: false,
            }),
        }
    }
    let _ = cache.save(&cache_file);
    outcome
}

/// After the check found `copy` and `keep` differ: both files' whole-file
/// hashes into `cache`, so the next scan no longer groups them (#888). The
/// check stops at the first difference, so this reads both again, but only
/// for a copy that turned out not to be one; the common case, a true copy,
/// costs nothing extra. Files of two sizes are never grouped, so they are
/// not read; a file that changes while it is read is not remembered.
fn remember_differing(cache: &mut KeyCache, copy: &Path, keep: &Path) {
    use hl_demo_auditor::stamp;
    let sizes = (stamp(copy).map(|s| s.size), stamp(keep).map(|s| s.size));
    if sizes.0.is_none() || sizes.0 != sizes.1 {
        return;
    }
    for path in [copy, keep] {
        remember_contents_of(cache, path);
    }
}

/// `path`'s whole-file hash into `cache`, unless it changes while read.
fn remember_contents_of(cache: &mut KeyCache, path: &Path) {
    use hl_demo_auditor::{content_hashes, stamp};
    let Ok(canonical) = std::fs::canonicalize(path) else {
        return;
    };
    if let (Some(before), Ok(hashes)) = (stamp(&canonical), content_hashes(&canonical))
        && stamp(&canonical) == Some(before)
    {
        cache.remember_contents(&canonical, before, hashes);
    }
}

/// Reveals a file in the OS file explorer, selecting it if the platform
/// supports that. Mirrors dev's `views/auditor.rs` cross-platform match.
pub fn reveal_in_explorer_impl(path: String) -> Result<(), String> {
    let path_buf = PathBuf::from(&path);

    if !path_buf.exists() {
        return Err(crate::messages::path_no_longer_exists(&path));
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg("/select,")
            .arg(&path_buf)
            .spawn()
            .map_err(crate::messages::failed_to_open_explorer)?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(&path_buf)
            .spawn()
            .map_err(crate::messages::failed_to_open_finder)?;
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        // Linux fallback: no universal "select this file" behavior, so just
        // open the parent directory via the desktop's default file manager.
        let parent = path_buf
            .parent()
            .ok_or(crate::messages::NO_PARENT_DIRECTORY_FOR_PATH)?;
        std::process::Command::new("xdg-open")
            .arg(parent)
            .spawn()
            .map_err(crate::messages::failed_to_open_folder)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hl_demo_auditor::find_duplicates_cached;

    #[test]
    fn the_audit_log_lines_say_what_ran_how_long_and_what_was_read() {
        assert_eq!(
            audit_start_log(&[], &["C:/".into(), "D:/".into()], false),
            "[audit] started: all drives (C:/ D:/)"
        );
        assert_eq!(
            audit_start_log(&["E:/demos".into()], &[], false),
            "[audit] started: 1 folder(s)"
        );
        assert_eq!(
            audit_start_log(&["E:/demos".into()], &[], true),
            "[audit] started: 1 folder(s), looking inside archives (.zip, .7z, .rar)"
        );
        let mut times = AuditTimes {
            cancelled: false,
            walk_secs: 5.04,
            key_secs: 1.55,
            demos: 3217,
            groups: 1007,
            copies: 1909,
            wasted_bytes: 129 * 1024 * 1024 * 1024,
            reads: hl_demo_auditor::Reads {
                heads: 2930,
                tails: 2919,
                fulls: 0,
                zips: 3,
                zip_demos: 2,
            },
            zips: None,
        };
        assert_eq!(
            audit_end_log(&times),
            "[audit] finished after 6.6 s (walk 5.0 s, keys 1.6 s): 3217 demo(s) checked, \
             1007 group(s) with copies, 1909 copies, 129.00 GB; opened 2930 start(s), 2919 end(s), \
             0 whole file(s), the rest remembered"
        );
        times.zips = Some((40, 120, 7));
        assert!(
            audit_end_log(&times).ends_with(
                "the rest remembered; looked inside 40 archive(s) holding 120 demo(s), opened 3 \
                 archive(s) and read 2 demo(s) in them in full, the rest remembered; 7 loose \
                 demo(s) backed up in an archive"
            ),
            "{}",
            audit_end_log(&times)
        );
        times.cancelled = true;
        assert_eq!(audit_end_log(&times), "[audit] cancelled after 6.6 s");
    }

    #[test]
    fn skipped_zips_are_named_up_to_a_limit() {
        let few = skipped_log(&["Z:\\a.zip: invalid Zip archive".to_string()]);
        assert_eq!(few, ["[audit] skipped Z:\\a.zip: invalid Zip archive"]);
        let many: Vec<String> = (0..25).map(|i| format!("Z:\\{i}.zip: bad")).collect();
        let lines = skipped_log(&many);
        assert_eq!(lines.len(), SKIPPED_LINES + 1);
        assert_eq!(
            lines.last().unwrap(),
            "[audit] skipped 5 more archive(s) or archived demo(s) that couldn't be read"
        );
    }

    /// Writes a zip at `path` with `entries` (name, bytes, deflated?).
    fn write_zip(path: &Path, entries: &[(&str, &[u8], bool)]) {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, bytes, deflate) in entries {
            let method = if *deflate {
                zip::CompressionMethod::Deflated
            } else {
                zip::CompressionMethod::Stored
            };
            let options = zip::write::SimpleFileOptions::default().compression_method(method);
            zip.start_file(*name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    /// An archive is a backup (#929): a loose demo it holds is marked with
    /// every archive entry holding it, in its group's row and in the list of
    /// backed-up demos; a demo with no loose copy is in that list only, and
    /// no archive entry is ever a row of a group.
    #[test]
    fn backed_up_demos_are_marked_and_never_offered_because_of_an_archive() {
        let s = Scratch::new("backups");
        let shared = demo_bytes(4);
        let alone = demo_bytes(5);
        let a = s.file("a.dem", &shared);
        let b = s.file("b.dem", &shared);
        let solo = s.file("solo.dem", &alone);
        let one = s.0.join("one.zip");
        let two = s.0.join("two.zip");
        write_zip(
            &one,
            &[("m.dem", &shared, true), ("solo.dem", &alone, true)],
        );
        write_zip(&two, &[("x/m.dem", &shared, false)]);
        let never = AtomicBool::new(false);
        let audit = find_duplicates_in(
            vec![a, b, solo],
            vec![one, two],
            &never,
            &None,
            &mut KeyCache::default(),
        );
        let (groups, backed_up) = audit_rows(audit.groups, &audit.backups, &KeepRules::default());
        let name = |p: &str| {
            Path::new(p)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        };
        let entries = |f: &AuditFile| -> Vec<String> {
            f.backups
                .iter()
                .map(|z| format!("{} {}", name(&z.zip), z.entry))
                .collect()
        };
        // a and b are a group of their own, as with no archive.
        assert_eq!(groups.len(), 1);
        let rows: Vec<(String, Vec<String>)> = groups[0]
            .files
            .iter()
            .map(|f| (name(&f.path), entries(f)))
            .collect();
        let both = vec!["one.zip m.dem".to_string(), "two.zip x/m.dem".into()];
        assert_eq!(
            rows,
            [
                ("a.dem".to_string(), both.clone()),
                ("b.dem".into(), both.clone())
            ]
        );
        let listed: Vec<(String, Vec<String>)> = backed_up
            .iter()
            .map(|f| (name(&f.path), entries(f)))
            .collect();
        assert_eq!(
            listed,
            [
                ("a.dem".to_string(), both.clone()),
                ("b.dem".into(), both),
                ("solo.dem".into(), vec!["one.zip solo.dem".into()]),
            ]
        );
        // Serialized for the frontend: `backups` only where there are some.
        let json = serde_json::to_value(&backed_up).unwrap();
        assert_eq!(json[2]["backups"][0]["entry"], "solo.dem");
        let plain = order_for_keeping(paths(&[r"Z:\x.dem"]), &KeepRules::default());
        assert!(
            serde_json::to_value(&plain).unwrap()[0]
                .get("backups")
                .is_none()
        );
    }

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    /// A fresh folder for one test, removed when dropped.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("audit_compare_{}_{}", name, std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Same size and first 64 KiB, so the audit groups them, but not the
    /// same demo past that.
    fn demo_bytes(tail: u8) -> Vec<u8> {
        let mut bytes = vec![7u8; COMPARE_CHUNK + 70_000];
        *bytes.last_mut().unwrap() = tail;
        bytes
    }

    #[test]
    fn a_byte_for_byte_copy_can_be_deleted() {
        let s = Scratch::new("same");
        let keep = s.file("keep.dem", &demo_bytes(1));
        let copy = s.file("copy.dem", &demo_bytes(1));
        assert_eq!(check_same_contents(&copy, &keep), Ok(()));
    }

    #[test]
    fn a_copy_that_differs_past_the_key_is_not_deleted() {
        let s = Scratch::new("differs");
        let keep = s.file("keep.dem", &demo_bytes(1));
        let copy = s.file("copy.dem", &demo_bytes(2));
        let Err(Refusal::NotACopy(err)) = check_same_contents(&copy, &keep) else {
            panic!("a different file must be refused as not a copy");
        };
        let last = (COMPARE_CHUNK + 70_000 - 1).to_string();
        assert!(err.contains(&format!("differ from byte {last}")), "{err}");
        let failure = Refusal::NotACopy(err).into_failure("copy.dem".into());
        assert!(failure.not_a_copy);
    }

    #[test]
    fn a_copy_of_another_size_is_not_deleted() {
        let s = Scratch::new("size");
        let keep = s.file("keep.dem", b"abcdef");
        let copy = s.file("copy.dem", b"abc");
        let Err(Refusal::NotACopy(err)) = check_same_contents(&copy, &keep) else {
            panic!("another size must be refused as not a copy");
        };
        assert!(err.contains("differ from byte 3"), "{err}");
    }

    /// A copy found different is remembered, and the next scan lists it
    /// apart from the copy it was grouped with (#888).
    #[test]
    fn a_copy_found_different_is_not_grouped_again() {
        let s = Scratch::new("remember");
        let keep = s.file("keep.dem", &demo_bytes(1));
        let copy = s.file("copy.dem", &demo_bytes(1));
        // Same first and last bytes as keep: only the middle differs.
        let mut middle = demo_bytes(1);
        middle[COMPARE_CHUNK / 2] = 9;
        let odd = s.file("odd.dem", &middle);
        let files = vec![keep.clone(), copy.clone(), odd.clone()];
        let never = AtomicBool::new(false);
        let mut cache = KeyCache::default();
        let before = find_duplicates_cached(files.clone(), &never, &None, &mut cache);
        assert_eq!(before.groups.len(), 1);
        assert_eq!(before.groups[0].files.len(), 3, "the ends match");

        assert!(matches!(
            check_same_contents(&odd, &keep),
            Err(Refusal::NotACopy(_))
        ));
        remember_differing(&mut cache, &odd, &keep);
        let after = find_duplicates_cached(files, &never, &None, &mut cache);
        assert_eq!(after.groups.len(), 1);
        let names: Vec<_> = after.groups[0]
            .files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["copy.dem", "keep.dem"]);

        // Files of two sizes are never grouped: nothing to remember.
        let short = s.file("short.dem", b"abc");
        let mut untouched = KeyCache::default();
        remember_differing(&mut untouched, &short, &keep);
        assert!(!untouched.has_changes());
    }

    #[test]
    fn the_kept_copy_itself_or_a_missing_one_is_never_a_reason_to_delete() {
        let s = Scratch::new("self");
        let keep = s.file("keep.dem", b"abc");
        let Err(Refusal::Other(err)) = check_same_contents(&keep, &keep) else {
            panic!("the kept file itself must be refused");
        };
        assert!(err.contains("is the copy being kept"), "{err}");
        let gone = s.0.join("gone.dem");
        let Err(Refusal::Other(err)) = check_same_contents(&keep, &gone) else {
            panic!("a missing kept copy must be refused");
        };
        assert!(err.contains("no longer there"), "{err}");
        assert!(!Refusal::Other(err).into_failure("x".into()).not_a_copy);
    }

    fn kept_order(files: &[&str], rules: &KeepRules) -> Vec<String> {
        order_for_keeping(paths(files), rules)
            .into_iter()
            .map(|f| f.path)
            .collect()
    }

    #[test]
    fn the_shortest_path_is_kept_when_nothing_uses_a_copy() {
        let rules = KeepRules::default();
        assert_eq!(
            kept_order(
                &[
                    r"F:\downloads\old\match.dem",
                    r"E:\match.dem",
                    r"D:\match.dem"
                ],
                &rules
            ),
            [
                r"D:\match.dem",
                r"E:\match.dem",
                r"F:\downloads\old\match.dem"
            ]
        );
    }

    #[test]
    fn a_copy_in_the_game_folder_is_kept_over_a_shorter_one() {
        let rules = KeepRules::new(&paths(&[r"Z:\no-such-install\Half-Life\dod"]), &[]);
        let files = order_for_keeping(
            paths(&[
                r"F:\m.dem",
                r"\\?\Z:\No-Such-Install\Half-Life\DOD\demos\m.dem",
            ]),
            &rules,
        );
        assert_eq!(
            files[0],
            AuditFile {
                path: r"\\?\Z:\No-Such-Install\Half-Life\DOD\demos\m.dem".into(),
                in_use: Some(InUse::GameFolder),
                backups: Vec::new(),
            }
        );
        assert_eq!(files[1].in_use, None);
    }

    #[test]
    fn every_install_the_copies_sit_in_is_a_game_folder() {
        let root =
            std::env::temp_dir().join(format!("dodstudio-audit-installs-{}", std::process::id()));
        let post = root.join("POST");
        let loose = root.join("loose");
        std::fs::create_dir_all(post.join("dod").join("sub")).unwrap();
        std::fs::create_dir_all(loose.join("dod")).unwrap();
        std::fs::write(post.join("hl.exe"), b"").unwrap();
        let files = [
            post.join("dod").join("sub").join("a.dem"),
            post.join("dod").join("b.dem"),
            // A folder named dod with no hl.exe beside it is not an install.
            loose.join("dod").join("c.dem"),
        ];
        let found = install_game_folders(files.iter());
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(found, vec![post.join("dod")]);
    }

    #[test]
    fn a_folder_that_only_starts_the_same_is_not_the_game_folder() {
        let rules = KeepRules::new(&paths(&[r"Z:\hl\dod"]), &[]);
        let files = order_for_keeping(paths(&[r"Z:\hl\dod_downloads\m.dem"]), &rules);
        assert_eq!(files[0].in_use, None);
    }

    #[test]
    fn a_queued_copy_is_kept_and_matches_its_canonical_form() {
        let rules = KeepRules::new(&[], &[r"Z:\projects\wsod\m.dem".to_string()]);
        let files = order_for_keeping(
            paths(&[r"\\?\A:\m.dem", r"\\?\Z:\Projects\WSOD\m.dem"]),
            &rules,
        );
        assert_eq!(files[0].path, r"\\?\Z:\Projects\WSOD\m.dem");
        assert_eq!(files[0].in_use, Some(InUse::Queue));
    }

    #[test]
    fn copies_both_in_use_fall_back_to_the_shortest_path() {
        let rules = KeepRules::new(&paths(&[r"Z:\hl\dod"]), &[r"Y:\q\m.dem".to_string()]);
        let files = order_for_keeping(paths(&[r"Z:\hl\dod\m.dem", r"Y:\q\m.dem"]), &rules);
        assert_eq!(files[0].path, r"Y:\q\m.dem");
        assert_eq!(files[1].in_use, Some(InUse::GameFolder));
    }

    #[test]
    fn roots_drop_blanks_and_repeats_and_add_the_drives() {
        let folders = [
            r"C:\Demos".to_string(),
            " ".to_string(),
            r"c:\demos\".to_string(),
        ];
        let drives = [r"C:\".to_string(), r"F:\".to_string()];
        assert_eq!(
            audit_roots(&folders, &drives),
            paths(&[r"C:\Demos", r"C:\", r"F:\"])
        );
    }

    #[test]
    fn comparable_strips_the_verbatim_prefix_and_case() {
        assert_eq!(comparable(Path::new(r"\\?\C:\A\b.dem")), r"c:\a\b.dem");
        assert_eq!(
            comparable(Path::new(r"\\?\UNC\nas\d\x.dem")),
            r"\\nas\d\x.dem"
        );
        assert_eq!(comparable(Path::new("C:/a/")), r"c:\a");
    }

    #[test]
    fn only_demo_files_are_deletable() {
        let dir = crate::test_support::Scratch::new("audit_deletable");
        let demo = dir.path().join("a.dem");
        let other = dir.path().join("a.txt");
        std::fs::write(&demo, b"x").unwrap();
        std::fs::write(&other, b"x").unwrap();
        assert!(check_deletable(&demo).is_ok());
        assert!(check_deletable(&other).is_err());
        assert!(check_deletable(dir.path()).is_err());
        let err = check_deletable(&dir.path().join("gone.dem")).unwrap_err();
        assert!(
            err.contains(&dir.path().join("gone.dem").display().to_string()),
            "{err}"
        );
    }

    #[test]
    fn every_fixed_drive_root_is_a_folder() {
        for root in fixed_drive_roots() {
            assert!(Path::new(&root).is_dir(), "{root}");
        }
    }
}
