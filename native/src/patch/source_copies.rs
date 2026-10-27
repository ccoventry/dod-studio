//! Demos on another drive than the game, copied as they are into the game's
//! `dod` folder for a Seek Between Clips batch (#434), when the hook DLL is
//! older than #779. A demo on the game's drive is played where it is, by a
//! path relative to that folder (`preview_in_place::viewdemo_arg`). One on
//! another drive has no such path: a hook DLL from #779 on sends it to
//! `viewdemo` by its full path (`preview_in_place::hook_plays_other_drives`),
//! so nothing is copied; an older DLL can't, and gets a copy. The copies are
//! the batch's own scratch: `capture_engine`'s cleanup guard removes them
//! however the batch ends, and the next batch removes any an aborted one left
//! behind.

use std::path::{Path, PathBuf};

/// The copies' name prefix: `dodstudio_src_<n>_<stem>.dem`, so a stray one
/// is recognisably Studio's and never mistaken for a user's demo.
pub const COPY_PREFIX: &str = "dodstudio_src_";

/// What the game's drive must keep free beyond the copies themselves.
pub const COPY_HEADROOM_BYTES: u64 = 1024 * 1024 * 1024;

/// One demo to copy before the batch, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceCopy {
    pub source: PathBuf,
    pub copy: PathBuf,
    pub bytes: u64,
}

/// `dodstudio_src_<n>_<stem>.dem`, the stem made safe the way a preview's is
/// (`playdemo_safe_stem`: letters, digits and `_`, shortened with a hash).
pub fn copy_file_name(index: usize, source: &Path) -> String {
    let stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!(
        "{COPY_PREFIX}{index:02}_{}.dem",
        crate::patch::playdemo_safe_stem(&stem)
    )
}

/// Whether `name` is one of these copies.
pub fn is_copy_file_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with(COPY_PREFIX) && lower.ends_with(".dem")
}

/// Which of `demos` have to be copied into `dod_dir`, in order, each once:
/// the ones `reachable` says the game can't open where they are. `bytes`
/// gives each file's size.
pub fn plan_with<'a>(
    dod_dir: &Path,
    demos: impl IntoIterator<Item = &'a str>,
    reachable: impl Fn(&Path) -> bool,
    bytes: impl Fn(&Path) -> Result<u64, String>,
) -> Result<Vec<SourceCopy>, String> {
    let mut plan: Vec<SourceCopy> = Vec::new();
    for demo in demos {
        let source = PathBuf::from(demo);
        if plan.iter().any(|c| c.source == source) || reachable(&source) {
            continue;
        }
        let bytes = bytes(&source)?;
        let copy = dod_dir.join(copy_file_name(plan.len() + 1, &source));
        plan.push(SourceCopy {
            source,
            copy,
            bytes,
        });
    }
    Ok(plan)
}

/// [`plan_with`] against the filesystem. With `other_drives` (the hook DLL
/// plays another drive's demo by its full path) only a demo `viewdemo` can't
/// reach either way is copied: a path too long or with a character the
/// console can't take.
pub fn plan<'a>(
    dod_dir: &Path,
    demos: impl IntoIterator<Item = &'a str>,
    other_drives: bool,
) -> Result<Vec<SourceCopy>, String> {
    plan_with(
        dod_dir,
        demos,
        |demo| {
            crate::preview_in_place::viewdemo_arg_any_drive(dod_dir, demo, other_drives).is_some()
        },
        |demo| {
            std::fs::metadata(demo)
                .map(|m| m.len())
                .map_err(|e| format!("can't read {}: {e}", demo.display()))
        },
    )
}

/// Whether `free` bytes on the game's drive take `needed` bytes of copies
/// and still leave [`COPY_HEADROOM_BYTES`]. `Err` says what's short.
pub fn space_check(free: u64, needed: u64) -> Result<(), String> {
    let wanted = needed.saturating_add(COPY_HEADROOM_BYTES);
    if free >= wanted {
        return Ok(());
    }
    let gib = |b: u64| b as f64 / (1024.0 * 1024.0 * 1024.0);
    Err(format!(
        "the game's drive has {:.1} GB free, and copying the demos there needs {:.1} GB plus {:.0} GB to spare",
        gib(free),
        gib(needed),
        gib(COPY_HEADROOM_BYTES)
    ))
}

/// Copies every planned demo, logging each. A failure removes the copies
/// made so far and says which demo failed.
pub fn copy_all(plan: &[SourceCopy]) -> Result<(), String> {
    for (i, item) in plan.iter().enumerate() {
        if let Err(e) = std::fs::copy(&item.source, &item.copy) {
            remove_all(plan[..i].iter().map(|c| c.copy.as_path()));
            let _ = std::fs::remove_file(&item.copy);
            return Err(format!(
                "could not copy {} to {}: {e}",
                item.source.display(),
                item.copy.display()
            ));
        }
        crate::log_markdown(&format!(
            "📋 **Copied for the batch** — `{}` is on another drive than the game, so it plays from `{}` ({:.0} MB); the copy goes when the batch ends",
            item.source.display(),
            item.copy.display(),
            item.bytes as f64 / (1024.0 * 1024.0)
        ));
    }
    Ok(())
}

/// Removes the copies, retrying briefly for one the game still holds open,
/// and logs the ones that stayed.
pub fn remove_all<'a>(copies: impl IntoIterator<Item = &'a Path>) {
    for copy in copies {
        if let Some(e) = crate::shared::paths::remove_file_retrying(copy) {
            crate::log_markdown(&format!(
                "⚠️ **Cleanup** — could not remove the batch's copy {}: {e}",
                copy.display()
            ));
        }
    }
}

/// Removes copies an earlier batch left in `dod_dir` (Studio closed or
/// crashed before its cleanup ran). Returns how many it removed.
pub fn remove_stale(dod_dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dod_dir) else {
        return 0;
    };
    let stale: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| is_copy_file_name(&n.to_string_lossy()))
        })
        .collect();
    remove_all(stale.iter().map(PathBuf::as_path));
    stale.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_is_named_after_its_demo_with_a_safe_stem() {
        assert_eq!(
            copy_file_name(1, Path::new(r"D:\demos\k4 philly-vs-x.dem")),
            "dodstudio_src_01_k4_philly_vs_x.dem"
        );
        assert!(is_copy_file_name("dodstudio_src_01_k4_philly_vs_x.dem"));
        assert!(is_copy_file_name("DODSTUDIO_SRC_02_a.DEM"));
        assert!(!is_copy_file_name("dodstudio_chain_01.dem"));
        assert!(!is_copy_file_name("dodstudio_src_01_notes.txt"));
    }

    #[test]
    fn only_unreachable_demos_are_planned_each_once_in_order() {
        let dod = Path::new(r"C:\game\dod");
        let plan = plan_with(
            dod,
            [r"D:\a.dem", r"C:\demos\b.dem", r"D:\a.dem", r"E:\c.dem"],
            |p| p.starts_with(r"C:\"),
            |p| Ok(if p.ends_with("a.dem") { 100 } else { 50 }),
        )
        .unwrap();
        assert_eq!(
            plan,
            vec![
                SourceCopy {
                    source: PathBuf::from(r"D:\a.dem"),
                    copy: dod.join("dodstudio_src_01_a.dem"),
                    bytes: 100,
                },
                SourceCopy {
                    source: PathBuf::from(r"E:\c.dem"),
                    copy: dod.join("dodstudio_src_02_c.dem"),
                    bytes: 50,
                },
            ]
        );
    }

    #[test]
    fn a_demo_whose_size_cannot_be_read_fails_the_plan() {
        let e = plan_with(
            Path::new(r"C:\game\dod"),
            [r"D:\gone.dem"],
            |_| false,
            |p| Err(format!("no {}", p.display())),
        )
        .unwrap_err();
        assert!(e.contains("gone.dem"), "{e}");
    }

    #[test]
    fn the_space_check_wants_the_copies_plus_the_headroom() {
        let mb = 1024 * 1024;
        assert!(space_check(COPY_HEADROOM_BYTES + 300 * mb, 300 * mb).is_ok());
        let e = space_check(COPY_HEADROOM_BYTES + 299 * mb, 300 * mb).unwrap_err();
        assert!(e.contains("GB free"), "{e}");
        assert!(space_check(u64::MAX, u64::MAX).is_ok());
    }

    #[test]
    fn copies_are_made_then_removed_and_stale_ones_swept() {
        let dir = crate::test_support::Scratch::new("source_copies");
        let dod = dir.path().join("dod");
        std::fs::create_dir_all(&dod).unwrap();
        let src = dir.path().join("x.dem");
        std::fs::write(&src, b"HLDEMO").unwrap();
        let plan = vec![SourceCopy {
            source: src.clone(),
            copy: dod.join(copy_file_name(1, &src)),
            bytes: 6,
        }];
        copy_all(&plan).unwrap();
        assert_eq!(std::fs::read(&plan[0].copy).unwrap(), b"HLDEMO");

        std::fs::write(dod.join("keep.dem"), b"x").unwrap();
        assert_eq!(remove_stale(&dod), 1);
        assert!(!plan[0].copy.exists());
        assert!(dod.join("keep.dem").exists());

        // Removing what is already gone is not an error.
        remove_all(plan.iter().map(|c| c.copy.as_path()));
    }

    #[test]
    fn a_failed_copy_leaves_nothing_behind() {
        let dir = crate::test_support::Scratch::new("source_copies_fail");
        let dod = dir.path().join("dod");
        std::fs::create_dir_all(&dod).unwrap();
        let good = dir.path().join("good.dem");
        std::fs::write(&good, b"HLDEMO").unwrap();
        let plan = vec![
            SourceCopy {
                source: good.clone(),
                copy: dod.join(copy_file_name(1, &good)),
                bytes: 6,
            },
            SourceCopy {
                source: dir.path().join("missing.dem"),
                copy: dod.join("dodstudio_src_02_missing.dem"),
                bytes: 0,
            },
        ];
        let e = copy_all(&plan).unwrap_err();
        assert!(e.contains("missing.dem"), "{e}");
        assert!(!plan[0].copy.exists());
    }
}
