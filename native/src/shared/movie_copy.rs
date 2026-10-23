//! Setup's "Make a movie copy" (#939): copies the `dod` folder beside
//! `hl.exe` to a new folder for movies, so movie configs, models and sounds
//! stay out of the game the user plays online.
//!
//! The copy leaves out `logs\` and, unless asked, the demos (`*.dem`, most
//! of a played install's size). In the copy's `resource\`, every
//! `dod_<language>.txt` is also written as `<name>_<language>.txt`, the file
//! VGUI2 reads for a game folder of that name (#925); Studio may write it
//! because the folder is its own, made by this copy.
//!
//! It only reads `dod`, and it never overwrites: the new folder is made with
//! `create_dir` and every file with `create_new`, so anything that appears
//! at a target path mid-copy fails the copy instead of being replaced. A
//! cancelled or failed copy removes only what it created.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// The name Setup suggests.
pub const DEFAULT_COPY_NAME: &str = "dod_movies";

/// The folder copied.
pub const SOURCE_FOLDER: &str = "dod";

/// Space left free on the drive after the copy: a drive filled to the last
/// byte stalls Windows and the game alike.
pub const FREE_SPACE_HEADROOM_BYTES: u64 = 1024 * 1024 * 1024;

/// Bytes read and written at a time; the cancel flag is checked between
/// chunks, so Cancel stops a multi-GB demo within a few milliseconds.
const CHUNK_BYTES: usize = 1024 * 1024;

/// Windows device names: a folder can't carry them.
const RESERVED_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Why a copy can't take the name, for the name box to say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NameProblem {
    Empty,
    /// Anything other than ASCII letters, digits and `_`.
    BadChar,
    /// A Windows device name (`con`, `nul`, `com1`…).
    Reserved,
    /// A file or folder of that name is already in the Half-Life folder.
    Exists,
}

/// Why `name` can't be the copy's folder in `game_root`, or `None`.
pub fn copy_name_problem(game_root: &Path, name: &str) -> Option<NameProblem> {
    if name.is_empty() {
        return Some(NameProblem::Empty);
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Some(NameProblem::BadChar);
    }
    if RESERVED_NAMES.contains(&name.to_ascii_lowercase().as_str()) {
        return Some(NameProblem::Reserved);
    }
    if existing_names(game_root)
        .iter()
        .any(|n| n.eq_ignore_ascii_case(name))
    {
        return Some(NameProblem::Exists);
    }
    None
}

/// Every file and folder name in `game_root`, which the name box checks
/// against as the user types. Names match without case, as Windows does.
pub fn existing_names(game_root: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(game_root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .collect()
}

fn problem_text(problem: NameProblem, name: &str) -> String {
    match problem {
        NameProblem::Empty => "The movie copy needs a name".to_string(),
        NameProblem::BadChar => {
            format!("\"{name}\" can only hold letters, digits and _")
        }
        NameProblem::Reserved => format!("Windows doesn't allow a folder named \"{name}\""),
        NameProblem::Exists => {
            format!("There's already a \"{name}\" in the Half-Life folder")
        }
    }
}

/// One file the copy writes: `src` and `dst` relative to `dod` and the copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyEntry {
    pub src: PathBuf,
    pub dst: PathBuf,
    pub bytes: u64,
    pub demo: bool,
}

/// What a copy of `dod` would write, demos and all; [`CopyPlan::files`]
/// picks with or without them.
#[derive(Debug, Clone, Default)]
pub struct CopyPlan {
    /// Folders to make, relative to the copy, parents first.
    pub dirs: Vec<PathBuf>,
    pub entries: Vec<CopyEntry>,
}

impl CopyPlan {
    pub fn files(&self, include_demos: bool) -> impl Iterator<Item = &CopyEntry> {
        self.entries
            .iter()
            .filter(move |e| include_demos || !e.demo)
    }

    pub fn total_bytes(&self, include_demos: bool) -> u64 {
        self.files(include_demos).map(|e| e.bytes).sum()
    }

    pub fn summary(&self) -> CopySummary {
        let demos = self.entries.iter().filter(|e| e.demo);
        CopySummary {
            files: self.files(false).count(),
            bytes: self.total_bytes(false),
            demo_files: demos.clone().count(),
            demo_bytes: demos.map(|e| e.bytes).sum(),
        }
    }
}

/// The sizes Setup shows before copying.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct CopySummary {
    /// Files and bytes without the demos.
    pub files: usize,
    pub bytes: u64,
    /// What "Also copy my demos" adds.
    pub demo_files: usize,
    pub demo_bytes: u64,
}

fn is_demo(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("dem"))
}

/// `<name>_<language>.txt` for a `dod_<language>.txt`, else `None`. The
/// language is letters only (`english`, `schinese`, `koreana`), so a
/// `dod_movies_english.txt` already in `dod` isn't taken for one.
fn language_copy_name(file: &str, name: &str) -> Option<String> {
    let lower = file.to_ascii_lowercase();
    if !lower.starts_with("dod_") || !lower.ends_with(".txt") {
        return None;
    }
    let language = &file[4..file.len() - 4];
    (!language.is_empty() && language.chars().all(|c| c.is_ascii_alphabetic()))
        .then(|| format!("{name}_{language}.txt"))
}

/// Walks `source` (the `dod` folder) for a copy named `name`. Leaves out
/// `logs\` at the top, and links and junctions anywhere (a junction can loop
/// back on itself, and the copy would hold the target's files twice).
pub fn plan_movie_copy(source: &Path, name: &str) -> io::Result<CopyPlan> {
    let mut plan = CopyPlan::default();
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        let dir = source.join(&rel);
        let mut children: Vec<_> = fs::read_dir(&dir)
            .map_err(|e| io::Error::new(e.kind(), format!("can't read {}: {e}", dir.display())))?
            .collect::<Result<_, _>>()?;
        children.sort_by_key(|e| e.file_name());
        let mut subdirs = Vec::new();
        for child in children {
            let file_type = child.file_type()?;
            if file_type.is_symlink() {
                continue;
            }
            let child_rel = rel.join(child.file_name());
            if file_type.is_dir() {
                if rel.as_os_str().is_empty() && child.file_name().eq_ignore_ascii_case("logs") {
                    continue;
                }
                subdirs.push(child_rel);
            } else if file_type.is_file() {
                let bytes = child.metadata()?.len();
                plan.entries.push(CopyEntry {
                    demo: is_demo(&child_rel),
                    src: child_rel.clone(),
                    dst: child_rel,
                    bytes,
                });
            }
        }
        // Reversed so the stack pops them in name order; each folder is
        // listed before its children are walked, so parents come first.
        for sub in subdirs.into_iter().rev() {
            plan.dirs.push(sub.clone());
            stack.push(sub);
        }
    }
    add_language_files(&mut plan, name);
    Ok(plan)
}

fn add_language_files(plan: &mut CopyPlan, name: &str) {
    let resource = Path::new("resource");
    let taken: std::collections::HashSet<String> = plan
        .entries
        .iter()
        .map(|e| e.dst.to_string_lossy().to_ascii_lowercase())
        .collect();
    let extra: Vec<CopyEntry> = plan
        .entries
        .iter()
        .filter(|e| {
            e.src
                .parent()
                .is_some_and(|p| p.as_os_str().eq_ignore_ascii_case(resource))
        })
        .filter_map(|e| {
            let file = e.src.file_name()?.to_str()?;
            let dst = e.src.with_file_name(language_copy_name(file, name)?);
            (!taken.contains(&dst.to_string_lossy().to_ascii_lowercase())).then(|| CopyEntry {
                src: e.src.clone(),
                dst,
                bytes: e.bytes,
                demo: false,
            })
        })
        .collect();
    plan.entries.extend(extra);
}

/// Whether `bytes` fit in `free` with [`FREE_SPACE_HEADROOM_BYTES`] to
/// spare. `u64::MAX` is what the disk query answers when it can't tell.
pub fn fits(bytes: u64, free: u64) -> bool {
    free == u64::MAX || bytes.saturating_add(FREE_SPACE_HEADROOM_BYTES) <= free
}

/// How far the copy is.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CopyProgress {
    pub done_bytes: u64,
    pub total_bytes: u64,
    pub done_files: usize,
    pub total_files: usize,
    /// The file being copied, relative to the copy.
    pub file: String,
}

/// How a copy ended.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CopyOutcome {
    Done {
        folder: String,
        target: String,
        files: usize,
        bytes: u64,
    },
    /// Cancel stopped it, and what it had made is gone.
    Cancelled,
}

/// Copies `game_root\dod` to `game_root\<name>` (see the module docs).
/// `progress` hears every chunk; throttle it at the caller.
pub fn copy_movie_folder(
    game_root: &Path,
    name: &str,
    include_demos: bool,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&CopyProgress),
) -> Result<CopyOutcome, String> {
    copy_with_free_space(
        game_root,
        name,
        include_demos,
        crate::sys::disk::get_available_bytes(game_root),
        cancel,
        progress,
    )
}

/// [`copy_movie_folder`] with the drive's free space given, for tests.
pub fn copy_with_free_space(
    game_root: &Path,
    name: &str,
    include_demos: bool,
    free_bytes: u64,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&CopyProgress),
) -> Result<CopyOutcome, String> {
    if let Some(problem) = copy_name_problem(game_root, name) {
        return Err(problem_text(problem, name));
    }
    let source = game_root.join(SOURCE_FOLDER);
    if !source.is_dir() {
        return Err(format!(
            "There's no dod folder to copy at {}",
            source.display()
        ));
    }
    let plan = plan_movie_copy(&source, name)
        .map_err(|e| format!("Couldn't list the dod folder to copy it: {e}"))?;
    let total_bytes = plan.total_bytes(include_demos);
    if !fits(total_bytes, free_bytes) {
        return Err(format!(
            "The copy needs {} and the drive holding {} has {} free (DoD Studio leaves 1 GB spare)",
            gb(total_bytes),
            game_root.display(),
            gb(free_bytes)
        ));
    }
    let target = game_root.join(name);
    let mut made = Made::default();
    let result = run_copy(
        &source,
        &target,
        &plan,
        include_demos,
        cancel,
        progress,
        &mut made,
    );
    match result {
        Ok(true) => Ok(CopyOutcome::Done {
            folder: name.to_string(),
            target: target.display().to_string(),
            files: plan.files(include_demos).count(),
            bytes: total_bytes,
        }),
        Ok(false) => {
            made.remove();
            Ok(CopyOutcome::Cancelled)
        }
        Err(e) => {
            let left = made.remove();
            let tail = if left {
                format!(
                    "; something else wrote into {} meanwhile, so it was left",
                    target.display()
                )
            } else {
                "; what it had copied was removed".to_string()
            };
            Err(format!("{e}{tail}"))
        }
    }
}

/// In 1024³ units, as Setup's sizes and Explorer show them.
fn gb(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / (1u64 << 30) as f64)
}

/// What this copy created, so a cancel removes that and nothing else.
#[derive(Default)]
struct Made {
    dirs: Vec<PathBuf>,
    files: Vec<PathBuf>,
}

impl Made {
    /// Removes the files, then the folders, deepest first. A folder that
    /// isn't empty after that holds something this copy didn't make, and is
    /// left; returns whether anything was.
    fn remove(&mut self) -> bool {
        for file in self.files.drain(..).rev() {
            let _ = fs::remove_file(&file);
        }
        let mut left = false;
        for dir in self.dirs.drain(..).rev() {
            if fs::remove_dir(&dir).is_err() && dir.exists() {
                left = true;
            }
        }
        left
    }
}

/// `Ok(false)` when cancelled.
fn run_copy(
    source: &Path,
    target: &Path,
    plan: &CopyPlan,
    include_demos: bool,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&CopyProgress),
    made: &mut Made,
) -> Result<bool, String> {
    let make_dir = |dir: &Path, made: &mut Made| {
        fs::create_dir(dir).map_err(|e| match e.kind() {
            io::ErrorKind::AlreadyExists => format!("{} appeared during the copy", dir.display()),
            _ => format!("couldn't make {}: {e}", dir.display()),
        })?;
        made.dirs.push(dir.to_path_buf());
        Ok::<_, String>(())
    };
    make_dir(target, made)?;
    for dir in &plan.dirs {
        make_dir(&target.join(dir), made)?;
    }

    let mut state = CopyProgress {
        done_bytes: 0,
        total_bytes: plan.total_bytes(include_demos),
        done_files: 0,
        total_files: plan.files(include_demos).count(),
        file: String::new(),
    };
    let mut read_only = Vec::new();
    let mut buf = vec![0u8; CHUNK_BYTES];
    for entry in plan.files(include_demos) {
        if cancel.load(Ordering::Relaxed) {
            return Ok(false);
        }
        let from = source.join(&entry.src);
        let to = target.join(&entry.dst);
        state.file = entry.dst.display().to_string();
        progress(&state);
        let mut input =
            File::open(&from).map_err(|e| format!("couldn't read {}: {e}", from.display()))?;
        let meta = input
            .metadata()
            .map_err(|e| format!("couldn't read {}: {e}", from.display()))?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&to)
            .map_err(|e| match e.kind() {
                io::ErrorKind::AlreadyExists => {
                    format!("{} appeared during the copy", to.display())
                }
                _ => format!("couldn't write {}: {e}", to.display()),
            })?;
        made.files.push(to.clone());
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Ok(false);
            }
            let n = input
                .read(&mut buf)
                .map_err(|e| format!("couldn't read {}: {e}", from.display()))?;
            if n == 0 {
                break;
            }
            output
                .write_all(&buf[..n])
                .map_err(|e| format!("couldn't write {}: {e}", to.display()))?;
            state.done_bytes += n as u64;
            progress(&state);
        }
        // Demo dates come from the file time, so the copy keeps them.
        if let Ok(modified) = meta.modified() {
            let _ = output.set_modified(modified);
        }
        drop(output);
        if meta.permissions().readonly() {
            read_only.push((to, meta.permissions()));
        }
        state.done_files += 1;
    }
    // Read-only only once nothing can fail, so a cancel never meets a file
    // it can't remove. config.cfg is often read-only.
    for (path, perms) in read_only {
        let _ = fs::set_permissions(&path, perms);
    }
    progress(&state);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    /// A small Half-Life folder: `dod` with configs, a demo at the top and
    /// one in a subfolder, `logs\`, two language files, and a sibling.
    fn fake_install(tag: &str) -> Scratch {
        let root = Scratch::new(tag);
        let dod = root.join("dod");
        for dir in ["cl_dlls", "logs", "resource", "maps", "demos"] {
            fs::create_dir_all(dod.join(dir)).unwrap();
        }
        fs::write(dod.join("liblist.gam"), b"game \"Day of Defeat\"").unwrap();
        fs::write(dod.join("cl_dlls").join("client.dll"), b"MZ").unwrap();
        fs::write(dod.join("config.cfg"), b"name m00cat").unwrap();
        fs::write(dod.join("match.dem"), vec![7u8; 3 * CHUNK_BYTES + 5]).unwrap();
        fs::write(dod.join("demos").join("old.DEM"), b"HLDEMO").unwrap();
        fs::write(dod.join("logs").join("L1023000.log"), b"log").unwrap();
        fs::write(dod.join("maps").join("dod_anzio.bsp"), b"bsp").unwrap();
        fs::write(dod.join("resource").join("dod_english.txt"), b"en").unwrap();
        fs::write(dod.join("resource").join("dod_german.txt"), b"de").unwrap();
        fs::write(dod.join("resource").join("dod_textscheme.res"), b"res").unwrap();
        fs::create_dir_all(root.join("valve")).unwrap();
        fs::write(root.join("valve").join("keep.txt"), b"valve").unwrap();
        root
    }

    fn copy(root: &Path, name: &str, demos: bool) -> Result<CopyOutcome, String> {
        copy_with_free_space(
            root,
            name,
            demos,
            u64::MAX,
            &AtomicBool::new(false),
            &mut |_| {},
        )
    }

    /// Every file under `dir`, relative, `/`-separated, sorted.
    fn files_under(dir: &Path) -> Vec<String> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    let rel = p.strip_prefix(dir).unwrap().to_string_lossy();
                    out.push(rel.replace(std::path::MAIN_SEPARATOR, "/"));
                }
            }
        }
        out.sort();
        out
    }

    #[test]
    fn names_are_letters_digits_and_underscores_only() {
        let root = fake_install("movie_copy_names");
        assert_eq!(copy_name_problem(&root, "dod_movies"), None);
        assert_eq!(copy_name_problem(&root, "Movies_2"), None);
        assert_eq!(copy_name_problem(&root, ""), Some(NameProblem::Empty));
        for bad in [
            "dod movies",
            "dod-movies",
            "../dod",
            "a\\b",
            "dod.movies",
            "filmé",
        ] {
            assert_eq!(
                copy_name_problem(&root, bad),
                Some(NameProblem::BadChar),
                "{bad}"
            );
        }
        assert_eq!(copy_name_problem(&root, "CON"), Some(NameProblem::Reserved));
        // Existing folders, without case, as Windows matches them.
        assert_eq!(copy_name_problem(&root, "dod"), Some(NameProblem::Exists));
        assert_eq!(copy_name_problem(&root, "VALVE"), Some(NameProblem::Exists));
    }

    #[test]
    fn a_name_already_taken_is_refused_and_nothing_is_written() {
        let root = fake_install("movie_copy_taken");
        let before = files_under(&root);
        let err = copy(&root, "valve", false).unwrap_err();
        assert!(err.contains("already a \"valve\""), "{err}");
        assert_eq!(files_under(&root), before);
    }

    #[test]
    fn the_copy_leaves_out_demos_and_logs_and_adds_the_language_files() {
        let root = fake_install("movie_copy_default");
        let source_before = files_under(&root.join("dod"));
        let outcome = copy(&root, "dod_movies", false).unwrap();
        let CopyOutcome::Done { folder, files, .. } = outcome else {
            panic!("not done: {outcome:?}")
        };
        assert_eq!(folder, "dod_movies");
        let copied = files_under(&root.join("dod_movies"));
        assert_eq!(
            copied,
            [
                "cl_dlls/client.dll",
                "config.cfg",
                "liblist.gam",
                "maps/dod_anzio.bsp",
                "resource/dod_english.txt",
                "resource/dod_german.txt",
                "resource/dod_movies_english.txt",
                "resource/dod_movies_german.txt",
                "resource/dod_textscheme.res",
            ]
        );
        assert_eq!(files, copied.len());
        // The empty demos\ and no logs\ at all.
        assert!(root.join("dod_movies").join("demos").is_dir());
        assert!(!root.join("dod_movies").join("logs").exists());
        assert_eq!(
            fs::read(root.join("dod_movies/resource/dod_movies_german.txt")).unwrap(),
            b"de"
        );
        // dod itself is untouched, and so is its neighbour.
        assert_eq!(files_under(&root.join("dod")), source_before);
        assert_eq!(fs::read(root.join("valve/keep.txt")).unwrap(), b"valve");
        // Now the name is taken.
        assert_eq!(
            copy_name_problem(&root, "dod_movies"),
            Some(NameProblem::Exists)
        );
    }

    #[test]
    fn demos_come_along_when_asked_byte_for_byte() {
        let root = fake_install("movie_copy_demos");
        copy(&root, "movies", true).unwrap();
        let copied = files_under(&root.join("movies"));
        assert!(copied.contains(&"match.dem".to_string()));
        assert!(copied.contains(&"demos/old.DEM".to_string()));
        assert!(copied.contains(&"resource/movies_english.txt".to_string()));
        assert!(!copied.iter().any(|f| f.starts_with("logs/")));
        assert_eq!(
            fs::read(root.join("movies/match.dem")).unwrap(),
            fs::read(root.join("dod/match.dem")).unwrap()
        );
    }

    #[test]
    fn the_summary_splits_out_the_demos() {
        let root = fake_install("movie_copy_summary");
        let plan = plan_movie_copy(&root.join("dod"), "dod_movies").unwrap();
        let s = plan.summary();
        assert_eq!(s.demo_files, 2);
        assert_eq!(s.demo_bytes, (3 * CHUNK_BYTES + 5 + 6) as u64);
        assert_eq!(s.files, 9);
        assert_eq!(
            plan.total_bytes(true),
            s.bytes + s.demo_bytes,
            "with demos is the two together"
        );
    }

    #[test]
    fn a_copy_that_does_not_fit_is_refused() {
        let root = fake_install("movie_copy_space");
        let err = copy_with_free_space(
            &root,
            "dod_movies",
            false,
            FREE_SPACE_HEADROOM_BYTES,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap_err();
        assert!(err.contains("free"), "{err}");
        assert!(!root.join("dod_movies").exists());
        assert!(fits(10, 10 + FREE_SPACE_HEADROOM_BYTES));
        assert!(!fits(11, 10 + FREE_SPACE_HEADROOM_BYTES));
        assert!(
            fits(u64::MAX, u64::MAX),
            "an unknown free space doesn't block"
        );
    }

    #[test]
    fn cancel_removes_the_half_made_folder_and_nothing_else() {
        let root = fake_install("movie_copy_cancel");
        let before = files_under(&root);
        let cancel = AtomicBool::new(false);
        let mut heard = 0u64;
        let outcome =
            copy_with_free_space(&root, "dod_movies", true, u64::MAX, &cancel, &mut |p| {
                heard = p.done_bytes;
                // Partway into the big demo.
                if p.done_bytes >= 2 * CHUNK_BYTES as u64 {
                    cancel.store(true, Ordering::Relaxed);
                }
            })
            .unwrap();
        assert_eq!(outcome, CopyOutcome::Cancelled);
        assert!(heard >= 2 * CHUNK_BYTES as u64);
        assert!(!root.join("dod_movies").exists());
        assert_eq!(files_under(&root), before);
    }

    #[test]
    fn a_file_that_appears_mid_copy_is_never_overwritten() {
        let root = fake_install("movie_copy_race");
        let intruder = root.join("dod_movies").join("maps").join("dod_anzio.bsp");
        let mut planted = false;
        let err = copy_with_free_space(
            &root,
            "dod_movies",
            false,
            u64::MAX,
            &AtomicBool::new(false),
            &mut |p| {
                // Something else writes the map just before the copy gets to it.
                if !planted && p.file.ends_with("dod_anzio.bsp") {
                    fs::write(&intruder, b"theirs").unwrap();
                    planted = true;
                }
            },
        )
        .unwrap_err();
        assert!(err.contains("appeared during the copy"), "{err}");
        assert!(err.contains("was left"), "{err}");
        // Theirs stays, and so does the folder holding it; ours is gone.
        assert_eq!(fs::read(&intruder).unwrap(), b"theirs");
        assert_eq!(
            files_under(&root.join("dod_movies")),
            ["maps/dod_anzio.bsp"]
        );
    }

    #[test]
    fn progress_counts_bytes_up_to_the_total() {
        let root = fake_install("movie_copy_progress");
        let mut last = None;
        copy_with_free_space(
            &root,
            "dod_movies",
            true,
            u64::MAX,
            &AtomicBool::new(false),
            &mut |p| last = Some(p.clone()),
        )
        .unwrap();
        let last = last.unwrap();
        assert_eq!(last.done_bytes, last.total_bytes);
        assert_eq!(last.done_files, last.total_files);
        assert!(last.total_bytes > 3 * CHUNK_BYTES as u64);
    }

    #[test]
    fn language_copies_take_letters_only_languages() {
        assert_eq!(
            language_copy_name("dod_english.txt", "m").as_deref(),
            Some("m_english.txt")
        );
        assert_eq!(
            language_copy_name("DOD_Koreana.TXT", "m").as_deref(),
            Some("m_Koreana.txt")
        );
        assert_eq!(language_copy_name("dod_movies_english.txt", "m"), None);
        assert_eq!(language_copy_name("dod_.txt", "m"), None);
        assert_eq!(language_copy_name("gameui_english.txt", "m"), None);
        assert_eq!(language_copy_name("dod_english.res", "m"), None);
    }
}
