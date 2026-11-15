// messages/game_reuse.rs
// Why a batch cannot run in the game that is already open
// (`patch/batch_reuse.rs`, #725, #237). One sentence each for the activity
// log, starting lower case; the caller writes them.

use std::fmt::Display;

pub const NO_GAME_RUNNING: &str = "no game is running";

pub const GAME_HAS_OTHER_LAUNCH_SETTINGS: &str =
    "the running game was started with other launch settings";

pub const SEEK_BETWEEN_CLIPS_OFF: &str =
    "Seek Between Clips is off, and a patched batch starts the game on its first demo";

pub const OBS_NEEDS_FRESH_START: &str =
    "OBS mode starts the game with its sound through waveOut, which only a fresh start sets";

pub const NO_HOOK_DLL_IN_STUDIO: &str = "Studio has no hook DLL to run the batch with";

pub fn game_dlls_unreadable(pid: impl Display) -> String {
    format!("the game's DLLs (PID {pid}) could not be read")
}

pub const GAME_HAS_NO_HOOK_DLL: &str =
    "the running game has no DoD Studio hook DLL (not started by Studio)";

pub const GAME_HAS_NO_HLAE: &str =
    "the running game has no HLAE in it, which every capture records through";

pub const GAME_DOES_NOT_TAKE_COMMANDS: &str = "the running game does not take commands from Studio";

pub const GAME_STARTED_BY_OTHER_STUDIO: &str =
    "the running game was started by another DoD Studio that is still open";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_reuse_reasons_match_their_original_inline_text() {
        assert_eq!(NO_GAME_RUNNING, "no game is running");
        assert_eq!(
            GAME_HAS_OTHER_LAUNCH_SETTINGS,
            "the running game was started with other launch settings"
        );
        assert_eq!(
            SEEK_BETWEEN_CLIPS_OFF,
            "Seek Between Clips is off, and a patched batch starts the game on its first demo"
        );
        assert_eq!(
            OBS_NEEDS_FRESH_START,
            "OBS mode starts the game with its sound through waveOut, which only a fresh start sets"
        );
        assert_eq!(
            NO_HOOK_DLL_IN_STUDIO,
            "Studio has no hook DLL to run the batch with"
        );
        assert_eq!(
            game_dlls_unreadable(12),
            "the game's DLLs (PID 12) could not be read"
        );
        assert_eq!(
            GAME_HAS_NO_HOOK_DLL,
            "the running game has no DoD Studio hook DLL (not started by Studio)"
        );
        assert_eq!(
            GAME_HAS_NO_HLAE,
            "the running game has no HLAE in it, which every capture records through"
        );
        assert_eq!(
            GAME_DOES_NOT_TAKE_COMMANDS,
            "the running game does not take commands from Studio"
        );
        assert_eq!(
            GAME_STARTED_BY_OTHER_STUDIO,
            "the running game was started by another DoD Studio that is still open"
        );
    }
}
