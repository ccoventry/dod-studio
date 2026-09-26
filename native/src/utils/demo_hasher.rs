use std::fs;
use std::io::Read;
use std::path::Path;

/// How much of a demo's start `calculate_demo_key` hashes.
const KEY_PREFIX_LEN: u64 = 65536;

pub fn fnv1a_hash(data: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

pub fn calculate_demo_key(path: &Path) -> Option<(u64, u64)> {
    let metadata = fs::metadata(path).ok()?;
    let size = metadata.len();

    let mut file = fs::File::open(path).ok()?;
    let read_size = std::cmp::min(size, KEY_PREFIX_LEN) as usize;
    let mut buffer = vec![0; read_size];
    file.read_exact(&mut buffer).ok()?;

    let hash = fnv1a_hash(&buffer);
    Some((size, hash))
}

/// `calculate_demo_key` for a demo already read into memory.
pub fn demo_key_of_bytes(bytes: &[u8]) -> (u64, u64) {
    let prefix = &bytes[..bytes.len().min(KEY_PREFIX_LEN as usize)];
    (bytes.len() as u64, fnv1a_hash(prefix))
}

/// A demo key as text, `<size>-<hash>`. Text rather than two numbers because
/// it round-trips through the frontend, and a JavaScript number loses a
/// `u64` hash's low bits.
pub fn demo_key_text((size, hash): (u64, u64)) -> String {
    format!("{}-{:016x}", size, hash)
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
            assert_eq!(calculate_demo_key(&path), Some(demo_key_of_bytes(&bytes)));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_key_survives_as_text() {
        assert_eq!(demo_key_text((123, 0xff)), "123-00000000000000ff");
    }
}
