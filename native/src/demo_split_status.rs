//! Whether a demo that recorded more than one map was already split: the
//! queue row and the Demo Analyzer stop suggesting a split once every map a
//! split would write has its file next to the demo.
//!
//! Checked when it is shown, never stored at scan time: a split can happen
//! after the scan, and the pieces can be deleted again.
//!
//! A file-existence check answers the usual cases on its own: every map has
//! its piece (split), or none has (not split). Only a demo with some pieces
//! and not others is read, for its maps' lengths, since an auto split leaves
//! out a map under `min_seconds` ([`keep_at_least`]).

use std::path::Path;

use crate::demo_maps_scan::split_files;
use crate::demo_split::{keep_at_least, quick_demo_segments};

/// What [`split_status`] found.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct SplitStatus {
    /// Every map a split would write has its file next to the demo.
    pub split: bool,
    /// The pieces found next to the demo, file names only, in map order.
    pub files: Vec<String>,
}

/// Whether the demo at `demo`, which recorded `maps` (one per signon, as the
/// scan lists them), was already split next to it. `min_seconds` is the
/// shortest map an auto split keeps (Studio's `SHORT_MAP_SECONDS`).
///
/// Reads only the folder, unless some maps have a piece and others don't:
/// then the demo's frame headers, for the maps' lengths. Fails only when
/// that read fails.
pub fn split_status(demo: &Path, maps: &[String], min_seconds: f32) -> Result<SplitStatus, String> {
    let found = split_files(demo, maps);
    let files: Vec<String> = found.iter().flatten().cloned().collect();
    let split = if files.is_empty() {
        false
    } else if files.len() == found.len() {
        true
    } else {
        let segments = quick_demo_segments(demo)?;
        // The pieces are named after the maps the demo's own signons give.
        let segment_maps: Vec<String> = segments.iter().map(|s| s.map.clone()).collect();
        every_kept_map_found(&split_files(demo, &segment_maps), &keep_at_least(&segments, min_seconds))
    };
    Ok(SplitStatus { split, files })
}

/// Whether each map in `keep` (indexes into `found`) has its piece.
fn every_kept_map_found(found: &[Option<String>], keep: &[usize]) -> bool {
    !keep.is_empty() && keep.iter().all(|&i| found.get(i).is_some_and(Option::is_some))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of its own, with `m.dem` (not a real demo: these cases never
    /// read it) and the named pieces next to it.
    fn folder_with(name: &str, pieces: &[&str]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("demo_split_status_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("m.dem"), b"x").unwrap();
        for piece in pieces {
            std::fs::write(dir.join(piece), b"x").unwrap();
        }
        dir
    }

    fn maps() -> Vec<String> {
        vec!["dod_armory".to_string(), "dod_anzio".to_string()]
    }

    #[test]
    fn a_demo_with_every_piece_next_to_it_is_split() {
        let dir = folder_with("all", &["m_split1_armory.dem", "m_split2_anzio.dem"]);
        let status = split_status(&dir.join("m.dem"), &maps(), 60.0).unwrap();
        assert_eq!(
            status,
            SplitStatus {
                split: true,
                files: vec!["m_split1_armory.dem".to_string(), "m_split2_anzio.dem".to_string()],
            }
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_demo_with_no_piece_next_to_it_is_not_split() {
        let dir = folder_with("none", &["other_split1_armory.dem"]);
        let status = split_status(&dir.join("m.dem"), &maps(), 60.0).unwrap();
        assert_eq!(status, SplitStatus { split: false, files: vec![] });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pieces_under_the_name_splits_used_before_count() {
        let dir = folder_with("old", &["m_dod_armory.dem", "m_dod_anzio.dem"]);
        let status = split_status(&dir.join("m.dem"), &maps(), 60.0).unwrap();
        assert!(status.split);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn some_pieces_on_an_unreadable_demo_fail_rather_than_guess() {
        let dir = folder_with("some", &["m_split1_armory.dem"]);
        assert!(split_status(&dir.join("m.dem"), &maps(), 60.0).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn split_when_every_map_a_split_keeps_has_its_piece() {
        let found = [Some("a.dem".to_string()), None];
        assert!(every_kept_map_found(&found, &[0]), "the short second map is left out");
        assert!(!every_kept_map_found(&found, &[0, 1]), "the long second map has no piece");
        assert!(!every_kept_map_found(&found, &[]));
        assert!(!every_kept_map_found(&found, &[5]), "an index past the list");
    }
}
