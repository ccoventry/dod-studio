//! Studio's own copies of map builds (#1547): one file per build of a map
//! name, so a demo can play on the build it was recorded on while the
//! installed map stays as it is.
//!
//! Each install keeps its own library, `<install>\dod_addon\dodstudio_map_builds\`
//! (only the addon folder is Studio's to write). A build's file is named
//! `<map>.<checksum>.bsp`, the checksum being `bsp::map_checksum`'s, the one
//! a demo states, as 8 hex digits: the name alone says which build it is.
//!
//! Builds come in two ways, both copying and never touching the source: a
//! `.bsp` the user picks, and a copy found beside the installed map
//! (`dod_armory_b5.61e9d38d.bsp.bak`-style backups).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use super::bsp;
use super::map_check::is_safe_map_name;
use crate::messages::map_builds as msg;

/// The library's folder name inside the addon folder.
pub const LIBRARY_FOLDER: &str = "dodstudio_map_builds";

/// How much of a file is read or written between two progress reports.
const CHUNK_BYTES: usize = 1 << 20;

/// `<install>\dod_addon\dodstudio_map_builds` for the install whose `hl.exe`
/// sits in `game_root`.
pub fn library_dir(game_root: &Path) -> PathBuf {
    crate::shared::paths::addon_dir(game_root).join(LIBRARY_FOLDER)
}

/// `<map>.<checksum>.bsp`, the library's name for one build.
pub fn build_file_name(map_name: &str, checksum: u32) -> String {
    format!("{map_name}.{checksum:08x}.bsp")
}

/// The map name and build a library file name says, or `None` for any other
/// file (the record of placed builds, a half-written copy).
pub fn parse_build_file_name(file_name: &str) -> Option<(String, u32)> {
    let lower = file_name.to_ascii_lowercase();
    let stem = lower.strip_suffix(".bsp")?;
    let (map_name, hex) = stem.rsplit_once('.')?;
    if hex.len() != 8 || !is_safe_map_name(map_name) {
        return None;
    }
    let checksum = u32::from_str_radix(hex, 16).ok()?;
    Some((map_name.to_string(), checksum))
}

/// Where build `checksum` of `map_name` lives in `library`, whether or not it
/// is there.
pub fn build_path(library: &Path, map_name: &str, checksum: u32) -> PathBuf {
    library.join(build_file_name(map_name, checksum))
}

/// The library's file for that build, when it holds it.
pub fn find_build(library: &Path, map_name: &str, checksum: u32) -> Option<PathBuf> {
    let path = build_path(library, map_name, checksum);
    path.is_file().then_some(path)
}

/// One build in the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryBuild {
    pub map_name: String,
    pub checksum: u32,
    pub path: PathBuf,
}

/// Every build in `library`, sorted by map then build. Empty when the folder
/// does not exist yet.
pub fn list_builds(library: &Path) -> Vec<LibraryBuild> {
    let Ok(entries) = std::fs::read_dir(library) else {
        return Vec::new();
    };
    let mut builds: Vec<LibraryBuild> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| {
            let (map_name, checksum) = parse_build_file_name(&e.file_name().to_string_lossy())?;
            Some(LibraryBuild {
                map_name,
                checksum,
                path: e.path(),
            })
        })
        .collect();
    builds.sort_by(|a, b| (&a.map_name, a.checksum).cmp(&(&b.map_name, b.checksum)));
    builds
}

/// Why a build could not be added.
#[derive(Debug)]
pub enum AddBuildError {
    /// The map name is not one a file can be named by.
    NotAMapName(String),
    /// The source could not be read.
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The source is not a map `bsp::map_checksum` can read.
    NotAMap { path: PathBuf, why: bsp::BspError },
    /// The library copy could not be written.
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for AddBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAMapName(name) => f.write_str(&msg::not_a_map_name(name)),
            Self::Read { path, source } => {
                f.write_str(&msg::could_not_read(path.display(), source))
            }
            Self::NotAMap { path, why } => f.write_str(&msg::not_a_map(path.display(), why)),
            Self::Write { path, source } => {
                f.write_str(&msg::could_not_write(path.display(), source))
            }
        }
    }
}

impl std::error::Error for AddBuildError {}

/// What adding a build did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedBuild {
    pub map_name: String,
    pub checksum: u32,
    /// The build's file in the library.
    pub path: PathBuf,
    /// The library already held this build; nothing was written.
    pub already_there: bool,
}

/// Adds `source` to `library` as a build of `map_name`, named by its checksum.
///
/// The map name is the caller's (the map the demos name), not read from the
/// file, which says no name: a backup called `dod_chemille_old.bsp` is still
/// a build of `dod_chemille`. Reads the whole file (a map is tens of MB at
/// most), computes its build, and writes the copy beside its final name
/// first, so a copy cut short never looks like a build. `source` is only
/// read. `progress` gets (bytes done, bytes total) over the read and the
/// write, about once a megabyte.
pub fn add_build(
    library: &Path,
    map_name: &str,
    source: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<AddedBuild, AddBuildError> {
    let map_name = map_name.trim().to_ascii_lowercase();
    if !is_safe_map_name(&map_name) {
        return Err(AddBuildError::NotAMapName(map_name));
    }
    let read_error = |source_err| AddBuildError::Read {
        path: source.to_path_buf(),
        source: source_err,
    };
    let size = std::fs::metadata(source).map_err(read_error)?.len();
    let total = size.saturating_mul(2);
    let bytes =
        read_with_progress(source, size, &mut |done| progress(done, total)).map_err(read_error)?;
    let checksum = bsp::map_checksum(&bytes).map_err(|why| AddBuildError::NotAMap {
        path: source.to_path_buf(),
        why,
    })?;
    let path = build_path(library, &map_name, checksum);
    if path.is_file() {
        progress(total, total);
        return Ok(AddedBuild {
            map_name,
            checksum,
            path,
            already_there: true,
        });
    }
    let write_error = |target: &Path| {
        let target = target.to_path_buf();
        move |source_err| AddBuildError::Write {
            path: target,
            source: source_err,
        }
    };
    std::fs::create_dir_all(library).map_err(write_error(library))?;
    // A name of this add's own: two adds of one build at once (two Studios,
    // two clicks) never write into each other's copy. The same build under
    // the final name either way, so the second rename changes nothing.
    let part = library.join(format!(
        "{}.part-{}-{}",
        build_file_name(&map_name, checksum),
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    write_with_progress(&part, &bytes, &mut |done| progress(size + done, total))
        .map_err(write_error(&part))?;
    if let Err(e) = std::fs::rename(&part, &path) {
        // Our own half of a copy: nothing else names it.
        let _ = std::fs::remove_file(&part);
        return Err(write_error(&path)(e));
    }
    Ok(AddedBuild {
        map_name,
        checksum,
        path,
        already_there: false,
    })
}

fn read_with_progress(
    path: &Path,
    size: u64,
    progress: &mut dyn FnMut(u64),
) -> std::io::Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)?;
    let mut bytes = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
    let mut chunk = vec![0u8; CHUNK_BYTES];
    loop {
        let read = file.read(&mut chunk)?;
        if read == 0 {
            return Ok(bytes);
        }
        bytes.extend_from_slice(&chunk[..read]);
        progress(bytes.len() as u64);
    }
}

fn write_with_progress(
    path: &Path,
    bytes: &[u8],
    progress: &mut dyn FnMut(u64),
) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    let mut done = 0u64;
    for chunk in bytes.chunks(CHUNK_BYTES) {
        file.write_all(chunk)?;
        done += chunk.len() as u64;
        progress(done);
    }
    file.sync_all()
}

/// At Studio's start: removes the unfinished copies an add left in `library`
/// (`<build>.bsp.part-<pid>-<nanos>`) when the process that wrote it is no
/// longer running: a Studio that closed or crashed mid-add. Returns how
/// many went.
pub fn sweep_unfinished_adds(library: &Path) -> usize {
    let sys = crate::sys::process::snapshot();
    let removed = sweep_unfinished_adds_where(library, &|pid| {
        sys.process(sysinfo::Pid::from_u32(pid)).is_some()
    });
    if removed > 0 {
        crate::log_markdown(&msg::library_parts_swept(removed, library.display()));
    }
    removed
}

/// [`sweep_unfinished_adds`] with `is_running` saying whether a process id
/// is alive.
fn sweep_unfinished_adds_where(library: &Path, is_running: &dyn Fn(u32) -> bool) -> usize {
    let Ok(entries) = std::fs::read_dir(library) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            unfinished_add_pid(&name).is_some_and(|pid| !is_running(pid))
        })
        .filter(|e| std::fs::remove_file(e.path()).is_ok())
        .count()
}

/// The process id in an unfinished add's name (`x.61e9d38d.bsp.part-<pid>-<nanos>`).
fn unfinished_add_pid(name: &str) -> Option<u32> {
    let (_, rest) = name.split_once(".bsp.part-")?;
    rest.split_once('-')?.0.parse().ok()
}

/// Files in `maps_dir` that look like other builds of `map_name` kept beside
/// it: named `<map>.` and then anything with `.bsp` in it, other than the
/// installed `<map>.bsp` itself (`dod_chemille.bsp.bak`,
/// `dod_armory_b5.61e9d38d.bsp.bak`, `dod_anzio.old.bsp`). Sorted by name.
pub fn copies_beside(maps_dir: &Path, map_name: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(maps_dir) else {
        return Vec::new();
    };
    let installed = format!("{map_name}.bsp");
    let prefix = format!("{map_name}.");
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            name.starts_with(&prefix) && name.contains(".bsp") && name != installed
        })
        .map(|e| e.path())
        .collect();
    found.sort();
    found
}

/// The first copy beside the map, in any of `maps_dirs`, that is build
/// `checksum`. Reads each candidate whole, so call it for a map known to be
/// the wrong build, not for every demo.
pub fn copy_beside_with_build(
    maps_dirs: &[PathBuf],
    map_name: &str,
    checksum: u32,
) -> Option<PathBuf> {
    maps_dirs
        .iter()
        .flat_map(|dir| copies_beside(dir, map_name))
        .find(|path| bsp::map_checksum_of_file(path).is_ok_and(|found| found == checksum))
}

/// A map `bsp::map_checksum` reads, for tests: a v30 header, fifteen lumps,
/// the first data lump holding `seed`, so two seeds are two builds.
#[cfg(test)]
pub(crate) fn test_map_bytes(seed: u8) -> Vec<u8> {
    let header = 4 + 15 * 8;
    let mut bytes = vec![0u8; header + 4];
    bytes[0] = 30;
    // Lump 1 (planes): offset `header`, length 4.
    bytes[4 + 8..4 + 12].copy_from_slice(&(header as u32).to_le_bytes());
    bytes[4 + 12..4 + 16].copy_from_slice(&4u32.to_le_bytes());
    bytes[header..].copy_from_slice(&[seed; 4]);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    fn map_bytes(seed: u8) -> Vec<u8> {
        test_map_bytes(seed)
    }

    fn checksum(seed: u8) -> u32 {
        bsp::map_checksum(&map_bytes(seed)).unwrap()
    }

    #[test]
    fn a_build_file_is_named_by_its_map_and_eight_hex_digits() {
        assert_eq!(
            build_file_name("dod_armory_b5", 0x61e9_d38d),
            "dod_armory_b5.61e9d38d.bsp"
        );
        assert_eq!(build_file_name("dod_x", 0x0c32), "dod_x.00000c32.bsp");
    }

    #[test]
    fn a_build_file_name_reads_back_and_other_names_do_not() {
        assert_eq!(
            parse_build_file_name("dod_armory_b5.61e9d38d.bsp"),
            Some(("dod_armory_b5".to_string(), 0x61e9_d38d))
        );
        assert_eq!(
            parse_build_file_name("DOD_Chemille.BB690C32.BSP"),
            Some(("dod_chemille".to_string(), 0xbb69_0c32))
        );
        for other in [
            "placed.json",
            "dod_x.bsp",
            "dod_x.61e9d38.bsp",
            "dod_x.zzzzzzzz.bsp",
            "dod_x.61e9d38d.bsp.part",
            "../x.61e9d38d.bsp",
        ] {
            assert_eq!(parse_build_file_name(other), None, "{other}");
        }
    }

    #[test]
    fn adding_a_map_files_it_under_its_build_and_leaves_the_source_alone() {
        let dir = Scratch::new("build_library_add");
        let source = dir.join("picked.bsp");
        std::fs::write(&source, map_bytes(1)).unwrap();
        let library = dir.join("library");

        let mut last = (0, 0);
        let added =
            add_build(&library, "DOD_Chemille", &source, &mut |d, t| last = (d, t)).unwrap();

        assert_eq!(added.map_name, "dod_chemille");
        assert_eq!(added.checksum, checksum(1));
        assert!(!added.already_there);
        assert_eq!(
            added.path,
            build_path(&library, "dod_chemille", checksum(1))
        );
        assert_eq!(std::fs::read(&added.path).unwrap(), map_bytes(1));
        assert_eq!(std::fs::read(&source).unwrap(), map_bytes(1));
        let size = map_bytes(1).len() as u64;
        assert_eq!(last, (2 * size, 2 * size));
        assert_eq!(
            list_builds(&library),
            vec![LibraryBuild {
                map_name: "dod_chemille".into(),
                checksum: checksum(1),
                path: added.path.clone(),
            }]
        );
        assert_eq!(
            find_build(&library, "dod_chemille", checksum(1)),
            Some(added.path)
        );
        assert_eq!(find_build(&library, "dod_chemille", checksum(2)), None);
    }

    #[test]
    fn adding_a_build_the_library_has_writes_nothing() {
        let dir = Scratch::new("build_library_again");
        let source = dir.join("a.bsp");
        std::fs::write(&source, map_bytes(1)).unwrap();
        let library = dir.join("library");
        add_build(&library, "dod_x", &source, &mut |_, _| {}).unwrap();

        let again = add_build(&library, "dod_x", &source, &mut |_, _| {}).unwrap();
        assert!(again.already_there);
        assert_eq!(list_builds(&library).len(), 1);
    }

    #[test]
    fn two_builds_of_one_map_are_two_files() {
        let dir = Scratch::new("build_library_two");
        let library = dir.join("library");
        for seed in [1, 2] {
            let source = dir.join(format!("{seed}.bsp"));
            std::fs::write(&source, map_bytes(seed)).unwrap();
            add_build(&library, "dod_x", &source, &mut |_, _| {}).unwrap();
        }
        let builds = list_builds(&library);
        assert_eq!(builds.len(), 2);
        assert!(builds.iter().all(|b| b.map_name == "dod_x"));
    }

    #[test]
    fn a_file_that_is_not_a_map_or_a_name_that_is_not_one_is_refused() {
        let dir = Scratch::new("build_library_refused");
        let library = dir.join("library");
        let junk = dir.join("junk.bsp");
        std::fs::write(&junk, b"not a map").unwrap();
        assert!(matches!(
            add_build(&library, "dod_x", &junk, &mut |_, _| {}),
            Err(AddBuildError::NotAMap { .. })
        ));
        assert!(matches!(
            add_build(&library, "../escape", &junk, &mut |_, _| {}),
            Err(AddBuildError::NotAMapName(_))
        ));
        assert!(matches!(
            add_build(&library, "dod_x", &dir.join("gone.bsp"), &mut |_, _| {}),
            Err(AddBuildError::Read { .. })
        ));
        assert!(!library.exists(), "nothing written for a refused file");
    }

    #[test]
    fn unfinished_adds_of_a_closed_studio_are_swept_and_a_running_ones_stay() {
        let dir = Scratch::new("build_library_sweep");
        let library = dir.join("library");
        std::fs::create_dir_all(&library).unwrap();
        for name in [
            "dod_x.61e9d38d.bsp.part-111-5",
            "dod_x.61e9d38d.bsp.part-222-6",
            "dod_x.61e9d38d.bsp",
            "placed.json",
        ] {
            std::fs::write(library.join(name), b"x").unwrap();
        }
        assert_eq!(sweep_unfinished_adds_where(&library, &|pid| pid == 222), 1);
        assert!(!library.join("dod_x.61e9d38d.bsp.part-111-5").exists());
        assert!(library.join("dod_x.61e9d38d.bsp.part-222-6").exists());
        assert!(library.join("dod_x.61e9d38d.bsp").exists());
        assert!(library.join("placed.json").exists());
    }

    #[test]
    fn copies_beside_the_map_are_found_and_matched_by_build() {
        let dir = Scratch::new("build_library_beside");
        let maps = dir.join("maps");
        std::fs::create_dir_all(&maps).unwrap();
        std::fs::write(maps.join("dod_armory_b5.bsp"), map_bytes(1)).unwrap();
        std::fs::write(maps.join("dod_armory_b5.61e9d38d.bsp.bak"), map_bytes(2)).unwrap();
        std::fs::write(maps.join("dod_armory_b5.res"), b"res").unwrap();
        std::fs::write(maps.join("dod_armory_b5_detail.txt"), b"txt").unwrap();
        std::fs::write(maps.join("dod_armory_b6.bsp.bak"), map_bytes(3)).unwrap();

        assert_eq!(
            copies_beside(&maps, "dod_armory_b5"),
            vec![maps.join("dod_armory_b5.61e9d38d.bsp.bak")]
        );
        let dirs = [maps.clone()];
        assert_eq!(
            copy_beside_with_build(&dirs, "dod_armory_b5", checksum(2)),
            Some(maps.join("dod_armory_b5.61e9d38d.bsp.bak"))
        );
        assert_eq!(
            copy_beside_with_build(&dirs, "dod_armory_b5", checksum(3)),
            None
        );
    }
}
