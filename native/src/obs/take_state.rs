//! What Studio saw of one OBS take, kept beside it on disk (#20, #1109).
//!
//! In OBS mode Studio starts and stops every clip's recording, and only
//! Studio knows when OBS started it late. Both live in the Studio that ran
//! the batch. When it closes or crashes mid-batch, the next start's catch-up
//! (`batch_catch_up.rs`) has only the takes on disk, and an OBS take reads as
//! whole however it ended: a recording nobody stopped, or one missing its
//! opening. So the session writes what it saw into the block folder as it
//! happens:
//!
//! - **recording** while OBS records the clip, cleared when Studio stops it.
//!   Still set after the batch means Studio never stopped that recording:
//!   it closed mid-clip, and the take is not the clip as planned.
//! - **started late**, kept after the stop: the take is missing the clip's
//!   opening.
//!
//! A whole take that started in time leaves no file. An older Studio that
//! finds one counts it as part of the take, as it counted the take before.

use std::path::Path;

/// The file, in the block folder beside `dodstudio_take.json`. Listed in
/// `take_meta::is_metadata`, so it never counts as captured output.
pub const FILE: &str = "dodstudio_obs_take.json";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ObsTakeState {
    /// OBS was recording this clip and Studio had not stopped it yet.
    #[serde(default)]
    pub recording: bool,
    /// OBS started recording this many ms after the clip's first frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_late_ms: Option<u64>,
}

/// Writes `state` into `block_folder`, or removes the file when there is
/// nothing to say. Best-effort: a recording must never fail over this file,
/// so a failure is logged and the batch carries on.
pub fn save(block_folder: &Path, state: ObsTakeState) {
    let path = block_folder.join(FILE);
    let result = if state == ObsTakeState::default() {
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    } else {
        serde_json::to_vec(&state)
            .map_err(std::io::Error::other)
            .and_then(|json| std::fs::write(&path, json))
    };
    if let Err(e) = result {
        crate::log_markdown(&format!(
            "⚠️ **OBS** — could not record the take's state at {}: {e}. A catch-up after a \
             restart may count this take as whole.",
            path.display()
        ));
    }
}

/// What the session recorded for the take in `block_folder`, or `None`: no
/// file (a whole take, or one from before this existed) or one that can't be
/// read.
pub fn read(block_folder: &Path) -> Option<ObsTakeState> {
    serde_json::from_slice(&std::fs::read(block_folder.join(FILE)).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    #[test]
    fn a_recording_is_kept_until_it_stops_and_a_late_start_after() {
        let block = Scratch::new("obs_take_state");
        assert_eq!(read(block.path()), None);

        save(
            block.path(),
            ObsTakeState {
                recording: true,
                started_late_ms: None,
            },
        );
        assert_eq!(read(block.path()).map(|s| s.recording), Some(true));

        let late = ObsTakeState {
            recording: false,
            started_late_ms: Some(605),
        };
        save(block.path(), late);
        assert_eq!(read(block.path()), Some(late));

        // Stopped in time: nothing left to say, no file.
        save(block.path(), ObsTakeState::default());
        assert!(!block.path().join(FILE).exists());
        // Saying nothing twice is fine.
        save(block.path(), ObsTakeState::default());
    }

    #[test]
    fn a_file_that_does_not_parse_says_nothing() {
        let block = Scratch::new("obs_take_state_bad");
        std::fs::write(block.path().join(FILE), b"not json").unwrap();
        assert_eq!(read(block.path()), None);
        // A newer build's extra fields don't stop an older one reading it.
        std::fs::write(
            block.path().join(FILE),
            br#"{"recording":true,"something_new":1}"#,
        )
        .unwrap();
        assert_eq!(read(block.path()).map(|s| s.recording), Some(true));
    }
}
