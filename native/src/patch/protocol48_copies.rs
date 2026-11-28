//! Protocol-48 copies of protocol-47 demos, for the game's `viewdemo` (#1549).
//!
//! `viewdemo` plays a demo through the HLTV proxy's `Core.dll`, whose
//! server-info parser disconnects on any protocol but 48 ("WARNING! Server
//! returned protocol version 47, not 48"), so a protocol-47 demo loads
//! nothing there; `playdemo` plays one with a warning
//! (`docs/goldsrc_protocol_47.md`). Every Studio path that starts a demo with
//! `viewdemo` (an in-place Launch Preview, a Seek Between Clips batch, a
//! review, and the in-game Load Demo window through Studio's game pipe)
//! therefore plays a copy converted by `protocol47::convert_in_place`
//! instead, and never the original.
//!
//! Copies live in Studio's own local folder, `%LOCALAPPDATA%\dod-studio\
//! protocol48_demos\v<N>-<key>\<stem>_p48.dem`, never in the game's folders.
//! The key is the demo's size-and-header key (`utils::demo_hasher`), so a copy
//! is made once per demo and reused by every later play, and a different file
//! under the same name gets its own. The stem is made safe for the console
//! (`playdemo_safe_stem`), so `viewdemo` always reaches a copy by name. They
//! share the trimmed copies' cache rules (`copy_cache`): the newest
//! [`KEEP_COPIES`] stay, a running batch's are pinned, a copy is saved whole
//! or not at all, and a new one gets the original's analyzer cache entry.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::patch::copy_cache::{mark_used, save_copy, seed_analysis};
use crate::patch::protocol47::{CONVERTED_SUFFIX, ConvertError, Converted, convert_in_place};

/// Under `%LOCALAPPDATA%\dod-studio`.
pub const COPIES_DIR: &str = "protocol48_demos";
/// Bumped when the conversion changes what it writes, so older copies are
/// made again rather than reused.
pub const COPY_VERSION: u32 = 1;
/// Copies kept, newest first by when something last played them. Each is
/// the size of its original (9 to 91 MB over the 78 archive demos).
pub const KEEP_COPIES: usize = 8;
/// Bytes read at a time, so a progress line can follow a big demo.
const READ_CHUNK_BYTES: usize = 4 * 1024 * 1024;
/// The bytes of a demo's start that say its protocol: the magic, the demo
/// protocol, then the network protocol.
const HEADER_PROTOCOL_BYTES: usize = 16;

/// What the game should play for a demo.
#[derive(Debug, Clone, PartialEq)]
pub enum Playable {
    /// The demo itself: it is not a protocol-47 demo (or not a demo at all,
    /// which the game then says).
    Original,
    /// The converted copy at `path`. `made` is false when an earlier play
    /// made it.
    Copy { path: PathBuf, made: bool },
}

impl Playable {
    /// The file to play for `demo`: the copy, or `demo` itself.
    pub fn file(&self, demo: &Path) -> PathBuf {
        match self {
            Self::Original => demo.to_path_buf(),
            Self::Copy { path, .. } => path.clone(),
        }
    }
}

/// Where [`prepare_playable`] is, for a progress line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Step {
    /// Reading the demo, how far (0.0-1.0).
    Reading(f32),
    /// Converting and saving the copy.
    Writing,
}

/// Why a protocol-47 demo has no copy to play.
#[derive(Debug)]
pub enum CopyError {
    /// The demo could not be read.
    Unreadable {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The conversion refused it (no server info to rewrite).
    Convert(ConvertError),
    /// The copy's drive hasn't room for it with
    /// [`FREE_SPACE_HEADROOM_BYTES`](crate::shared::movie_copy::FREE_SPACE_HEADROOM_BYTES) to spare.
    NoRoom {
        folder: PathBuf,
        free_bytes: u64,
        needed_bytes: u64,
    },
    /// The copy could not be written.
    Save {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for CopyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use crate::messages::protocol47 as m;
        match self {
            Self::Unreadable { path, source } => {
                f.write_str(&m::cannot_read(path.display(), source))
            }
            Self::Convert(e) => write!(f, "{e}"),
            Self::NoRoom {
                folder,
                free_bytes,
                needed_bytes,
            } => f.write_str(&m::copy_no_room(
                folder.display(),
                *free_bytes,
                *needed_bytes,
            )),
            Self::Save { path, source } => f.write_str(&m::cannot_write(path.display(), source)),
        }
    }
}

impl std::error::Error for CopyError {}

/// The folder copies live in.
pub fn copies_root() -> PathBuf {
    crate::shared::paths::get_local_appdata_dir().join(COPIES_DIR)
}

/// Where `demo`'s copy goes under `root`, or `None` when the demo can't be
/// read for its key.
pub fn copy_path(root: &Path, demo: &Path) -> Option<PathBuf> {
    let key = crate::utils::demo_hasher::demo_key_text(demo)?;
    let stem = demo.file_stem()?.to_string_lossy();
    Some(root.join(format!("v{COPY_VERSION}-{key}")).join(format!(
        "{}{CONVERTED_SUFFIX}.dem",
        crate::patch::playdemo_safe_stem(&stem)
    )))
}

/// Whether the file at `demo` is a protocol-47 demo, from its first bytes
/// only. A file that can't be read, or isn't a demo, is not one.
pub fn is_protocol_47_file(demo: &Path) -> bool {
    let mut head = [0u8; HEADER_PROTOCOL_BYTES];
    std::fs::File::open(demo)
        .and_then(|mut f| f.read_exact(&mut head))
        .is_ok_and(|()| crate::patch::protocol47::is_protocol_47(&head))
}

/// What the game should play for `demo` ([`prepare_playable_in`] with
/// Studio's own folders).
pub fn prepare_playable(
    demo: &Path,
    progress: &mut dyn FnMut(Step),
) -> Result<Playable, CopyError> {
    prepare_playable_in(
        demo,
        &copies_root(),
        &crate::analyzer_cache_root(),
        progress,
    )
}

/// What the game should play for `demo`: the demo itself unless it is a
/// protocol-47 demo, else its copy under `root`, made now if there is none
/// yet. Reports each step of a copy being made through `progress`. Writes the
/// copy, gives it the original's entry in the analyzer cache at
/// `analyzer_root`, stamps it used, and logs what it did to the activity log.
///
/// `Err` when a protocol-47 demo can't be read or converted, or its copy has
/// no room or can't be saved: the caller must not play the original then,
/// since `viewdemo` refuses it.
pub fn prepare_playable_in(
    demo: &Path,
    root: &Path,
    analyzer_root: &Path,
    progress: &mut dyn FnMut(Step),
) -> Result<Playable, CopyError> {
    if !is_protocol_47_file(demo) {
        return Ok(Playable::Original);
    }
    let unreadable = |source| CopyError::Unreadable {
        path: demo.to_path_buf(),
        source,
    };
    let path =
        copy_path(root, demo).ok_or_else(|| unreadable(std::io::ErrorKind::NotFound.into()))?;
    if path.is_file() {
        reuse_copy(demo, &path, analyzer_root);
        return Ok(Playable::Copy { path, made: false });
    }
    let started = Instant::now();
    let mut bytes = read_with_progress(demo, progress).map_err(unreadable)?;
    progress(Step::Writing);
    let converted = convert_in_place(&mut bytes).map_err(CopyError::Convert)?;
    check_room(&path, bytes.len() as u64)?;
    save_copy(&path, &bytes).map_err(|source| CopyError::Save {
        path: path.clone(),
        source,
    })?;
    drop(bytes);
    seed_analysis(analyzer_root, demo, &path, cached_peak(analyzer_root, demo));
    mark_used(&path);
    log_made(demo, &path, converted, started.elapsed());
    Ok(Playable::Copy { path, made: true })
}

/// Plays the copy already at `path`: seeds its cache entry if it has none
/// yet, stamps it used, and logs it.
fn reuse_copy(demo: &Path, path: &Path, analyzer_root: &Path) {
    if !analysis::cache::is_fresh(analyzer_root, path) {
        seed_analysis(analyzer_root, demo, path, cached_peak(analyzer_root, demo));
    }
    mark_used(path);
    crate::log_markdown(&crate::messages::protocol47::copy_in_use(
        file_name(demo),
        path.display(),
    ));
}

/// The original's most entities in one snapshot, from its cache entry: the
/// copy holds the same frames.
fn cached_peak(analyzer_root: &Path, demo: &Path) -> Option<u32> {
    analysis::cache::load(analyzer_root, demo)?
        .1
        .demo_info
        .peak_packet_entities
}

/// Reads all of `demo`, reporting how far after each chunk.
fn read_with_progress(demo: &Path, progress: &mut dyn FnMut(Step)) -> std::io::Result<Vec<u8>> {
    let mut file = std::fs::File::open(demo)?;
    let total = file.metadata()?.len();
    let mut bytes = Vec::with_capacity(usize::try_from(total).unwrap_or(0));
    let mut chunk = vec![0u8; READ_CHUNK_BYTES];
    loop {
        let read = file.read(&mut chunk)?;
        if read == 0 {
            return Ok(bytes);
        }
        bytes.extend_from_slice(&chunk[..read]);
        if total > 0 {
            progress(Step::Reading(
                (bytes.len() as f64 / total as f64).min(1.0) as f32
            ));
        }
    }
}

/// `Err` when the drive `copy` goes on hasn't `needed_bytes` free with the
/// headroom to spare. A drive whose free space can't be told is let through:
/// the write itself still fails cleanly.
fn check_room(copy: &Path, needed_bytes: u64) -> Result<(), CopyError> {
    // The free-space query wants a folder that exists.
    let Some(folder) = copy.ancestors().skip(1).find(|p| p.is_dir()) else {
        return Ok(());
    };
    let free = crate::sys::disk::get_available_bytes(folder);
    if crate::shared::movie_copy::fits(needed_bytes, free) {
        return Ok(());
    }
    Err(CopyError::NoRoom {
        folder: folder.to_path_buf(),
        free_bytes: free.unwrap_or(0),
        needed_bytes,
    })
}

fn log_made(demo: &Path, path: &Path, converted: Converted, took: Duration) {
    use crate::messages::protocol47 as m;
    crate::log_markdown(&m::copy_made(
        file_name(demo),
        converted.server_infos,
        path.display(),
        took.as_secs_f64(),
    ));
    if let Some(at) = converted.stopped_at {
        crate::log_markdown(&m::walk_stopped(file_name(demo), at));
    }
}

fn file_name(demo: &Path) -> String {
    demo.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| demo.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    /// `test-fixtures/ci_fixture.dem` (protocol 48) and the same demo made
    /// to say 47 where a 2004 recording does, as `protocol47`'s own tests
    /// make it.
    fn fixture_pair(dir: &Path) -> (PathBuf, PathBuf) {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../test-fixtures/ci_fixture.dem");
        let p48 = std::fs::read(&fixture).unwrap();
        let p47 = crate::patch::protocol47::tests_support::as_protocol_47(&p48);
        let old = dir.join("old demo-2004.dem");
        let new = dir.join("new.dem");
        std::fs::write(&old, &p47).unwrap();
        std::fs::write(&new, &p48).unwrap();
        (old, new)
    }

    #[test]
    fn a_protocol_48_demo_plays_itself_and_nothing_is_written() {
        let dir = Scratch::new("p48_copy_original");
        let (_, new) = fixture_pair(&dir);
        let root = dir.join("copies");
        let played = prepare_playable_in(&new, &root, &dir.join("cache"), &mut |_| {}).unwrap();
        assert_eq!(played, Playable::Original);
        assert_eq!(played.file(&new), new);
        assert!(!root.exists());
    }

    #[test]
    fn a_protocol_47_demo_plays_a_converted_copy_that_is_the_protocol_48_recording() {
        let dir = Scratch::new("p48_copy_made");
        let (old, new) = fixture_pair(&dir);
        let root = dir.join("copies");
        let mut steps = Vec::new();
        let played =
            prepare_playable_in(&old, &root, &dir.join("cache"), &mut |s| steps.push(s)).unwrap();
        let Playable::Copy { path, made: true } = &played else {
            panic!("{played:?}");
        };
        assert!(path.starts_with(&root));
        assert_eq!(
            path.file_name().unwrap().to_string_lossy(),
            "old_demo_2004_p48.dem",
            "a name the console can take"
        );
        assert_eq!(std::fs::read(path).unwrap(), std::fs::read(&new).unwrap());
        assert!(matches!(steps.first(), Some(Step::Reading(_))));
        assert_eq!(steps.last(), Some(&Step::Writing));
        assert!(
            std::fs::read(&old).unwrap() != std::fs::read(&new).unwrap(),
            "the original is unchanged"
        );
    }

    #[test]
    fn a_second_play_reuses_the_copy() {
        let dir = Scratch::new("p48_copy_reused");
        let (old, _) = fixture_pair(&dir);
        let root = dir.join("copies");
        let cache = dir.join("cache");
        let first = prepare_playable_in(&old, &root, &cache, &mut |_| {}).unwrap();
        let mut steps = 0;
        let second = prepare_playable_in(&old, &root, &cache, &mut |_| steps += 1).unwrap();
        assert_eq!(
            second,
            Playable::Copy {
                path: first.file(&old),
                made: false
            }
        );
        assert_eq!(steps, 0, "nothing read again");
    }

    #[test]
    fn a_different_file_under_the_same_name_gets_its_own_copy() {
        let dir = Scratch::new("p48_copy_key");
        let (old, _) = fixture_pair(&dir);
        let root = dir.join("copies");
        let first = copy_path(&root, &old).unwrap();
        let mut bytes = std::fs::read(&old).unwrap();
        bytes.extend_from_slice(b"more");
        std::fs::write(&old, bytes).unwrap();
        assert_ne!(copy_path(&root, &old).unwrap(), first);
    }

    #[test]
    fn a_protocol_47_demo_with_no_server_info_is_refused_and_nothing_is_written() {
        let dir = Scratch::new("p48_copy_refused");
        let demo = dir.join("header_only.dem");
        let mut bytes = vec![0u8; crate::patch::DEMO_HEADER_SIZE];
        bytes[..8].copy_from_slice(b"HLDEMO\0\0");
        bytes[8..12].copy_from_slice(&5i32.to_le_bytes());
        bytes[12..16].copy_from_slice(&47i32.to_le_bytes());
        std::fs::write(&demo, bytes).unwrap();
        let root = dir.join("copies");
        let e = prepare_playable_in(&demo, &root, &dir.join("cache"), &mut |_| {}).unwrap_err();
        assert!(
            matches!(e, CopyError::Convert(ConvertError::NoServerInfo)),
            "{e}"
        );
        assert!(!copy_path(&root, &demo).unwrap().exists());
    }

    #[test]
    fn only_a_protocol_47_header_counts() {
        let dir = Scratch::new("p48_copy_header");
        let (old, new) = fixture_pair(&dir);
        assert!(is_protocol_47_file(&old));
        assert!(!is_protocol_47_file(&new));
        assert!(!is_protocol_47_file(&dir.join("missing.dem")));
        std::fs::write(dir.join("short.dem"), b"HLDEMO").unwrap();
        assert!(!is_protocol_47_file(&dir.join("short.dem")));
    }
}
