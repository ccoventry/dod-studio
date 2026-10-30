//! Reading a demo's key and its content hashes: the first 64 KB, the last
//! 64 KB, and the whole file, with the file's stamp that says when a
//! remembered hash is stale.

use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::UNIX_EPOCH;

use crate::{FileKey, KEY_BYTES, archive, fnv1a_hash};

/// Generates a `FileKey` for a file based on its size and an FNV-1a hash of its first 64KB.
pub fn get_file_key(path: &Path) -> Result<FileKey, std::io::Error> {
    let size = fs::metadata(path)?.len();
    Ok(FileKey {
        size,
        header_hash: head_hash(path, size)?,
    })
}

/// An FNV-1a hash of a file's first [`KEY_BYTES`] (all of it when shorter).
pub(crate) fn head_hash(path: &Path, size: u64) -> Result<u64, std::io::Error> {
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    archive::hashes_from_reader(&mut file, size)
}
