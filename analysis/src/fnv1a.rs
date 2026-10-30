//! 64-bit FNV-1a, the workspace's one copy: demo keys (`hl_demo_auditor`),
//! analyzer cache entry names, the demo bar `.res` marker and the open-files
//! pipe name all hash with it. Lives here because every crate that needs it
//! already depends on `analysis`. The values are stored on disk (cache file
//! names, project files' demo keys), so the algorithm must never change.

/// FNV-1a over `data` in one call.
pub fn fnv1a_hash(data: &[u8]) -> u64 {
    let mut hash = Fnv1a::new();
    hash.update(data);
    hash.finish()
}

/// [`fnv1a_hash`] fed in pieces: the same result as one call over them all.
#[derive(Clone, Copy, Debug)]
pub struct Fnv1a(u64);

impl Default for Fnv1a {
    fn default() -> Self {
        Self::new()
    }
}

impl Fnv1a {
    /// A hash of nothing yet: FNV-1a's offset basis.
    pub const fn new() -> Self {
        Self(0xcbf29ce484222325)
    }

    /// Feeds `data` in after everything fed so far.
    pub fn update(&mut self, data: &[u8]) {
        for &byte in data {
            self.0 ^= byte as u64;
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }

    /// The hash of everything fed in.
    pub fn finish(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_published_test_vectors() {
        assert_eq!(fnv1a_hash(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a_hash(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn hashing_in_pieces_matches_one_call() {
        let mut pieces = Fnv1a::new();
        pieces.update(b"ab");
        pieces.update(b"c");
        assert_eq!(pieces.finish(), fnv1a_hash(b"abc"));
    }
}
