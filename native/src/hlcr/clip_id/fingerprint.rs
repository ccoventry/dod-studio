//! The fallback that finds a clip with neither tag (#1442): its size plus a
//! hash of its first and last MB, the Demo Auditor's idea applied to videos.
//!
//! Two reads of 1 MB however big the video is, so hashing a ProRes clip of
//! several GB costs the same as a small one. The size is compared first
//! (free, from the folder listing), so only a file of exactly a recorded size
//! is ever read. A render's in-file tag sits in the `moov` atom at the end of
//! the file, so two renders of one take with the same settings still differ in
//! their last MB.

use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

/// How much of each end of the file is hashed.
const END_BYTES: u64 = 1024 * 1024;

/// Hex characters kept of the SHA-256: 128 bits, far past any chance of two
/// clips of one size colliding.
const HASH_HEX_CHARS: usize = 32;

/// `file`'s content hash: SHA-256 over its first MB then its last MB (the
/// whole file once when it is 2 MB or smaller), as 32 hex characters.
/// Fails when the file cannot be opened or read.
pub fn content_hash(file: &Path) -> io::Result<String> {
    let mut reader = File::open(file)?;
    let size = reader.metadata()?.len();
    let mut hasher = Sha256::new();
    if size <= 2 * END_BYTES {
        hash_range(&mut reader, 0, size, &mut hasher)?;
    } else {
        hash_range(&mut reader, 0, END_BYTES, &mut hasher)?;
        hash_range(&mut reader, size - END_BYTES, END_BYTES, &mut hasher)?;
    }
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    Ok(hex[..HASH_HEX_CHARS].to_string())
}

/// Feeds `len` bytes of `reader` from `start` to `hasher`.
fn hash_range(reader: &mut File, start: u64, len: u64, hasher: &mut Sha256) -> io::Result<()> {
    reader.seek(SeekFrom::Start(start))?;
    let mut buf = vec![0u8; len as usize];
    reader.read_exact(&mut buf)?;
    hasher.update(&buf);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    fn write(dir: &Scratch, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn a_copy_hashes_the_same_and_a_changed_end_does_not() {
        let dir = Scratch::new("fingerprint_same");
        let mut bytes = vec![7u8; 3 * END_BYTES as usize];
        let a = write(&dir, "a.mov", &bytes);
        let b = write(&dir, "b.mov", &bytes);
        *bytes.last_mut().unwrap() = 8;
        let c = write(&dir, "c.mov", &bytes);
        let hash = content_hash(&a).unwrap();
        assert_eq!(hash.len(), HASH_HEX_CHARS);
        assert_eq!(hash, content_hash(&b).unwrap());
        assert_ne!(hash, content_hash(&c).unwrap());
    }

    #[test]
    fn the_middle_of_a_big_file_is_not_read() {
        let dir = Scratch::new("fingerprint_middle");
        let mut bytes = vec![1u8; 3 * END_BYTES as usize];
        let a = write(&dir, "a.mov", &bytes);
        bytes[END_BYTES as usize + 10] = 2;
        let b = write(&dir, "b.mov", &bytes);
        assert_eq!(content_hash(&a).unwrap(), content_hash(&b).unwrap());
    }

    #[test]
    fn a_small_file_is_hashed_whole() {
        let dir = Scratch::new("fingerprint_small");
        let a = write(&dir, "a.mov", b"short clip");
        let b = write(&dir, "b.mov", b"short clap");
        assert_ne!(content_hash(&a).unwrap(), content_hash(&b).unwrap());
        assert!(content_hash(&dir.path().join("gone.mov")).is_err());
    }
}
