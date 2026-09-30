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
