//! Capture history (#439): what is still on disk for the takes a highlight was
//! captured into. The project's take record keeps when and how a take was
//! captured; this answers the two things only the disk knows, whether the take
//! folder is still there and what its renders came to (#438), so the history
//! list can mark a take gone while its line stays.

use crate::render_manager::{HistoryEntry, history_for};
use serde::Serialize;
use std::path::Path;

/// One take folder's state on disk.
#[derive(Serialize)]
pub struct TakeCaptureState {
    /// The take folder is there.
    exists: bool,
    /// Its renders, oldest first; empty when the folder is gone.
    renders: Vec<HistoryEntry>,
}

fn state_of(take_folder: &str) -> TakeCaptureState {
    if take_folder.is_empty() || !Path::new(take_folder).is_dir() {
        return TakeCaptureState {
            exists: false,
            renders: Vec::new(),
        };
    }
    TakeCaptureState {
        exists: true,
        renders: history_for(take_folder),
    }
}

/// One state per folder, in order. Off the main thread: a take on a sleeping
/// network or cloud drive can take seconds to answer (#789).
#[tauri::command(async)]
pub fn take_capture_states(take_folders: Vec<String>) -> Vec<TakeCaptureState> {
    take_folders.iter().map(|f| state_of(f)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_or_unrecorded_folder_is_gone_with_no_renders() {
        let gone = state_of("C:/made-up/session/chain_00_b0");
        assert!(!gone.exists && gone.renders.is_empty());
        assert!(!state_of("").exists);
    }

    #[test]
    fn a_folder_that_is_there_exists_even_without_history() {
        let dir = std::env::temp_dir().join("dod_capture_history_state_test");
        std::fs::create_dir_all(&dir).unwrap();
        let state = state_of(&dir.to_string_lossy());
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(state.exists && state.renders.is_empty());
    }
}
