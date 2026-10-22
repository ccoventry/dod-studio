//! The demo file key and its hash, from `hl-demo-auditor`, the one place they
//! are defined. Kept as a path for the callers that already use it.

use std::path::Path;

pub use hl_demo_auditor::{FileKey, fnv1a_hash};

/// `(size, hash)` of a demo's key; see [`hl_demo_auditor::FileKey`].
pub fn calculate_demo_key(path: &Path) -> Option<(u64, u64)> {
    hl_demo_auditor::get_file_key(path)
        .ok()
        .map(|key| (key.size, key.header_hash))
}

/// The key's text form, `<size>-<hash>`, as saved in project files.
pub fn demo_key_text(path: &Path) -> Option<String> {
    hl_demo_auditor::get_file_key(path)
        .ok()
        .map(|key| key.to_text())
}

/// `calculate_demo_key` for a demo whose start is already in memory: `head`
/// holds at least the file's first `KEY_BYTES` (or all of it), `size` is the
/// whole file's length.
pub fn demo_key_of_head(head: &[u8], size: u64) -> (u64, u64) {
    let prefix = &head[..head.len().min(hl_demo_auditor::KEY_BYTES as usize)];
    (size, fnv1a_hash(prefix))
}

/// A `(size, hash)` key as text, `<size>-<hash>`: the same text
/// `demo_key_text` gives for the file. Text rather than two numbers because it
/// round-trips through the frontend, and a JavaScript number loses a `u64`
/// hash's low bits.
pub fn key_text((size, header_hash): (u64, u64)) -> String {
    FileKey { size, header_hash }.to_text()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_in_memory_key_matches_the_on_disk_one() {
        let dir = std::env::temp_dir().join(format!("demo_key_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for len in [10usize, 70_000] {
            let bytes: Vec<u8> = (0..len).map(|i| (i * 7 % 251) as u8).collect();
            let path = dir.join(format!("{}.dem", len));
            std::fs::write(&path, &bytes).unwrap();
            assert_eq!(
                calculate_demo_key(&path),
                Some(demo_key_of_head(&bytes, bytes.len() as u64))
            );
            assert_eq!(
                demo_key_text(&path),
                Some(key_text(demo_key_of_head(&bytes, bytes.len() as u64)))
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_key_survives_as_text() {
        assert_eq!(key_text((123, 0xff)), "123-00000000000000ff");
    }
}
