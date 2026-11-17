// messages/tool_versions.rs
// Activity-log lines saying which build of a third-party tool a launch used
// (#1492): HLAE and the game's engine here; Blender's is in studio's
// messages. Logging only, nothing reads a version to decide anything.

use std::fmt::Display;

/// HLAE's version for a log line, or that it can't be read.
fn hlae_version_or_unread(version: Option<&str>) -> String {
    version.map_or_else(|| "version unreadable".to_string(), |v| format!("v{v}"))
}

/// The engine build's name for a log line.
pub fn engine_build(build: Option<&str>) -> &'static str {
    match build {
        Some("anniversary") => "25th Anniversary",
        Some("pre_anniversary") => "pre-Anniversary",
        _ => "build unreadable",
    }
}

/// Logged as HLAE launches the game (a capture batch, a preview, Launch
/// Game): HLAE's version (its product version, which carries the build's git
/// hash, else its file version) and the game's engine. A version that could
/// not be read says so, and the launch goes ahead.
pub fn hlae_launch(
    hlae_exe: impl Display,
    hlae_version: Option<&str>,
    engine: Option<&str>,
) -> String {
    format!(
        "[HLAE] Launching with {hlae_exe} ({}); game engine: {}",
        hlae_version_or_unread(hlae_version),
        engine_build(engine)
    )
}

/// [`hlae_launch`] for a batch that runs in a game that was already open, so
/// HLAE is not started: the versions are of the configured HLAE and of the
/// engine the open game's install has.
pub fn hlae_reused_game(
    hlae_exe: impl Display,
    hlae_version: Option<&str>,
    engine: Option<&str>,
) -> String {
    format!(
        "[HLAE] Running in a game that was already open; the configured HLAE is {hlae_exe} ({}); game engine: {}",
        hlae_version_or_unread(hlae_version),
        engine_build(engine)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launch_line_names_the_version_and_engine() {
        assert_eq!(
            hlae_launch(
                "HLAE/HLAE.exe",
                Some("2.192.4.0+55ebb71"),
                Some("anniversary")
            ),
            "[HLAE] Launching with HLAE/HLAE.exe (v2.192.4.0+55ebb71); game engine: 25th Anniversary"
        );
    }

    #[test]
    fn a_version_that_cannot_be_read_says_so() {
        assert_eq!(
            hlae_launch("HLAE.exe", None, None),
            "[HLAE] Launching with HLAE.exe (version unreadable); game engine: build unreadable"
        );
        assert!(
            hlae_launch("HLAE.exe", Some("1.0.0.0"), Some("pre_anniversary"))
                .ends_with("game engine: pre-Anniversary")
        );
    }

    #[test]
    fn a_reused_game_says_it_was_already_running() {
        assert_eq!(
            hlae_reused_game("HLAE.exe", Some("2.192.4.0"), Some("pre_anniversary")),
            "[HLAE] Running in a game that was already open; the configured HLAE is HLAE.exe (v2.192.4.0); game engine: pre-Anniversary"
        );
    }
}
