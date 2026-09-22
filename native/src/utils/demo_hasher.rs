use std::fs;
use std::io::Read;
use std::path::Path;

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
    let read_size = std::cmp::min(size, 65536) as usize;
    let mut buffer = vec![0; read_size];
    file.read_exact(&mut buffer).ok()?;

    let hash = fnv1a_hash(&buffer);
    Some((size, hash))
}
