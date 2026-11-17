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
    /// Loose demos keyed.
    pub demos: usize,
    /// Demos inside archives, counted apart so the done line doesn't add
    /// them to the loose ones.
    pub archive_demos: usize,
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
        return format!(
            "[audit] cancelled after {}",
            native::utils::duration_text::duration_text(total)
        );
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
        "[audit] finished after {} (walk {}, keys {}): {} demo(s) checked, \
         {} group(s) with copies, {} copies, {:.2} GB; opened {} start(s), {} end(s), {} whole file(s), \
         the rest remembered{zips}",
        native::utils::duration_text::duration_text(total),
        native::utils::duration_text::duration_text(t.walk_secs),
        native::utils::duration_text::duration_text(t.key_secs),
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
                if crate::scan_manager::progress_emit_due(&last_emit, now_ms) {
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
        let archive_demos = audit.zip_demos;
        let demos = loose_demos + archive_demos;
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
            demos: last.done.saturating_sub(archive_demos),
            archive_demos,
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
        if done == total || crate::scan_manager::progress_emit_due(&last_emit, now_ms) {
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
#[path = "audit_manager_tests.rs"]
mod tests;
