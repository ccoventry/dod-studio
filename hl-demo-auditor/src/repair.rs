//! Repairing damaged demos on disk (#15): finding them in folders, and writing
//! each one's repair next to it as `<name>_repaired.dem`. The repair itself
//! is `analysis::repair`, bytes in and bytes out; this is the files around it.
//!
//! The original is never changed, moved or deleted, and no existing file is
//! ever overwritten: the repair goes to the first free name.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use analysis::repair::{
    FirstHole, Refusal, RepairHooks, RepairOutcome, RepairReport, find_first_hole, repair_demo,
};

use crate::walk::scan_dir;

/// What a repair adds to the original's name.
const REPAIRED_SUFFIX: &str = "_repaired";
/// Names tried before giving up on finding a free one.
const MAX_NAME_TRIES: usize = 1000;

/// A demo with a hole in it.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct DamagedDemo {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub first_hole: FirstHole,
    /// A repair written next to it before (`<name>_repaired.dem`), if any.
    pub repaired: Option<PathBuf>,
}

/// What [`repair_file`] did.
#[derive(Clone, Debug, PartialEq)]
pub enum FileRepair {
    /// The demo has no hole: nothing written.
    NotDamaged,
    /// The repair, written to `path`.
    Repaired { path: PathBuf, report: RepairReport },
    /// The repair failed a check: nothing written.
    Refused {
        report: RepairReport,
        reasons: Vec<Refusal>,
    },
}

/// The name a repair of `original` is written to: `<stem>_repaired.dem` next
/// to it, or `_repaired-2.dem`, `-3` and so on when that is taken. `None`
/// when every name up to [`MAX_NAME_TRIES`] is.
pub fn repaired_path(original: &Path) -> Option<PathBuf> {
    let dir = original.parent().unwrap_or_else(|| Path::new(""));
    let stem = original.file_stem()?.to_string_lossy();
    (1..=MAX_NAME_TRIES)
        .map(|n| {
            let name = if n == 1 {
                format!("{stem}{REPAIRED_SUFFIX}.dem")
            } else {
                format!("{stem}{REPAIRED_SUFFIX}-{n}.dem")
            };
            dir.join(name)
        })
        .find(|p| !p.exists())
}

/// A repair already written next to `original`, by the first name
/// [`repaired_path`] gives.
fn existing_repair(original: &Path) -> Option<PathBuf> {
    let stem = original.file_stem()?.to_string_lossy();
    let path = original.with_file_name(format!("{stem}{REPAIRED_SUFFIX}.dem"));
    path.is_file().then_some(path)
}

/// Whether `path` is a repair this module wrote: its own holes were cut out,
/// so it is never listed as damaged.
fn is_repair(path: &Path) -> bool {
    path.file_stem()
        .map(|s| s.to_string_lossy().contains(REPAIRED_SUFFIX))
        .unwrap_or(false)
}

/// Repairs the demo at `path` and writes the result next to it
/// ([`repaired_path`]). Writes nothing for a demo with no hole or a repair
/// that fails a check.
///
/// # Errors
/// The file cannot be read, the repair cannot be made, or the result cannot
/// be written; each names the file.
pub fn repair_file(path: &Path, hooks: RepairHooks) -> Result<FileRepair, String> {
    let bytes = fs::read(path)
        .map_err(|e| crate::messages::cannot_read(&path.display().to_string(), &e))?;
    let outcome = repair_demo(&bytes, hooks)
        .map_err(|e| crate::messages::cannot_repair(&path.display().to_string(), &e))?;
    drop(bytes);
    match outcome {
        RepairOutcome::NotDamaged => Ok(FileRepair::NotDamaged),
        RepairOutcome::Refused { report, reasons } => Ok(FileRepair::Refused { report, reasons }),
        RepairOutcome::Repaired { bytes, report } => {
            let out = write_new(path, &bytes)?;
            Ok(FileRepair::Repaired { path: out, report })
        }
    }
}

/// Writes `bytes` to the first free repair name for `original`. Opened with
/// `create_new`, so a file that appears between choosing the name and
/// writing is never overwritten: the next name is tried instead.
fn write_new(original: &Path, bytes: &[u8]) -> Result<PathBuf, String> {
    for _ in 0..MAX_NAME_TRIES {
        let Some(out) = repaired_path(original) else {
            break;
        };
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&out);
        match file {
            Ok(mut f) => {
                return match f.write_all(bytes).and_then(|()| f.sync_all()) {
                    Ok(()) => Ok(out),
                    Err(e) => {
                        drop(f);
                        // A half-written repair would read as a damaged demo
                        // of its own; the file is ours, made a moment ago.
                        let _ = fs::remove_file(&out);
                        Err(crate::messages::cannot_write(
                            &out.display().to_string(),
                            &e,
                        ))
                    }
                };
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(crate::messages::cannot_write(
                    &out.display().to_string(),
                    &e,
                ));
            }
        }
    }
    Err(crate::messages::no_free_name(
        &original.display().to_string(),
    ))
}

/// The first hole in the demo at `path`, or `None` for one with none (or a
/// file that cannot be read: it is not listed).
pub fn first_hole_in(path: &Path) -> Option<FirstHole> {
    let bytes = fs::read(path).ok()?;
    find_first_hole(&bytes)
}

/// Whether a repair of the demo in `bytes` has a hole to cut out: its frames
/// stop reading before the end ([`find_first_hole`], the repair's own test).
/// A demo the analyzer flags as damaged inside a frame whose header reads has
/// none, and [`repair_file`] would call it not damaged; Studio offers Repair
/// only where this says yes. Takes the bytes, not the path, so Studio reads a
/// damaged demo once for this and for its maps.
pub fn is_repairable(bytes: &[u8]) -> bool {
    find_first_hole(bytes).is_some()
}

/// Every demo with a hole in `folder` (and its subfolders when `recursive`),
/// sorted by path. Repairs this module wrote are skipped. `progress(done,
/// total, demo)` is called before each demo is read; `cancel` stops the
/// search, keeping what it found.
pub fn find_damaged_demos(
    folder: &Path,
    recursive: bool,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize, usize, &Path),
) -> Vec<DamagedDemo> {
    let mut demos = Vec::new();
    if recursive {
        scan_dir(folder, &mut demos, cancel, &None);
    } else if let Ok(entries) = fs::read_dir(folder) {
        demos.extend(
            entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dem"))
                }),
        );
    }
    demos.retain(|p| !is_repair(p));
    demos.sort();
    let total = demos.len();
    let mut found = Vec::new();
    for (done, path) in demos.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        progress(done, total, path);
        if let Some(first_hole) = first_hole_in(path) {
            found.push(DamagedDemo {
                size_bytes: fs::metadata(path).map(|m| m.len()).unwrap_or(0),
                repaired: existing_repair(path),
                path: path.clone(),
                first_hole,
            });
        }
    }
    progress(total, total, folder);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("dodstudio-repair-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fixture() -> Vec<u8> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../test-fixtures/ci_fixture.dem"
        );
        fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    fn damaged_fixture() -> Vec<u8> {
        let mut bytes = fixture();
        let at = bytes.len() / 2;
        bytes[at..at + 300_000].fill(0xEE);
        bytes
    }

    fn no_hooks() -> (
        impl FnMut(analysis::repair::RepairProgress),
        impl Fn() -> bool,
    ) {
        (|_| {}, || false)
    }

    #[test]
    fn a_repair_is_named_after_the_original_and_never_takes_a_used_name() {
        let dir = temp_dir("names");
        let demo = dir.join("match.dem");
        assert_eq!(repaired_path(&demo), Some(dir.join("match_repaired.dem")));
        fs::write(dir.join("match_repaired.dem"), b"x").unwrap();
        assert_eq!(repaired_path(&demo), Some(dir.join("match_repaired-2.dem")));
        fs::write(dir.join("match_repaired-2.dem"), b"x").unwrap();
        assert_eq!(repaired_path(&demo), Some(dir.join("match_repaired-3.dem")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_damaged_demo_is_repaired_beside_it_and_the_original_is_untouched() {
        let dir = temp_dir("write");
        let demo = dir.join("broken.dem");
        let damaged = damaged_fixture();
        fs::write(&demo, &damaged).unwrap();
        fs::write(dir.join("broken_repaired.dem"), b"someone else's file").unwrap();
        let (mut progress, cancel) = no_hooks();
        let outcome = repair_file(
            &demo,
            RepairHooks {
                progress: &mut progress,
                is_cancelled: &cancel,
            },
        )
        .unwrap();
        let FileRepair::Repaired { path, report } = outcome else {
            panic!("not repaired: {outcome:?}");
        };
        assert_eq!(path, dir.join("broken_repaired-2.dem"));
        assert_eq!(report.holes.len(), 1);
        assert_eq!(fs::read(&demo).unwrap(), damaged);
        assert_eq!(
            fs::read(dir.join("broken_repaired.dem")).unwrap(),
            b"someone else's file"
        );
        assert!(analysis::integrity::check_demo_bytes(&fs::read(&path).unwrap()).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_healthy_demo_gets_no_repair_written() {
        let dir = temp_dir("healthy");
        let demo = dir.join("fine.dem");
        fs::write(&demo, fixture()).unwrap();
        let (mut progress, cancel) = no_hooks();
        let hooks = RepairHooks {
            progress: &mut progress,
            is_cancelled: &cancel,
        };
        assert_eq!(repair_file(&demo, hooks), Ok(FileRepair::NotDamaged));
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_a_demo_with_a_hole_is_repairable() {
        assert!(is_repairable(&damaged_fixture()));
        assert!(!is_repairable(&fixture()));
    }

    #[test]
    fn only_damaged_demos_are_found_and_repairs_are_skipped() {
        let dir = temp_dir("find");
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("fine.dem"), fixture()).unwrap();
        fs::write(dir.join("sub").join("broken.dem"), damaged_fixture()).unwrap();
        fs::write(
            dir.join("sub").join("broken_repaired.dem"),
            damaged_fixture(),
        )
        .unwrap();
        let cancel = AtomicBool::new(false);
        let found = find_damaged_demos(&dir, true, &cancel, &mut |_, _, _| {});
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].path, dir.join("sub").join("broken.dem"));
        assert_eq!(
            found[0].repaired,
            Some(dir.join("sub").join("broken_repaired.dem"))
        );
        assert!(find_damaged_demos(&dir, false, &cancel, &mut |_, _, _| {}).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
