// messages/tool_versions.rs
// Activity-log lines saying which build of a third-party tool a launch used
// (#1492): HLAE and the game's engine here; Blender's is in studio's
// messages. Logging only, nothing reads a version to decide anything.

use std::fmt::Display;

/// What one file's version read gave: the version, or that it can't be read.
fn version_or_unread(version: Option<&str>) -> String {
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
/// Game): HLAE's and its hook DLL's file versions, and the game's engine.
/// A version that could not be read says so, and the launch goes ahead.
pub fn hlae_launch(
    hlae_exe: impl Display,
    hlae_version: Option<&str>,
    hook_dll_version: Option<&str>,
    engine: Option<&str>,
) -> String {
    format!(
        "[HLAE] Launching with {hlae_exe} ({}); AfxHookGoldSrc.dll ({}); game engine: {}",
        version_or_unread(hlae_version),
        version_or_unread(hook_dll_version),
        engine_build(engine)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launch_line_names_each_version() {
        assert_eq!(
            hlae_launch(
                "HLAE/HLAE.exe",
                Some("2.189.5.0"),
                Some("2.189.5.0"),
                Some("anniversary")
            ),
            "[HLAE] Launching with HLAE/HLAE.exe (v2.189.5.0); AfxHookGoldSrc.dll (v2.189.5.0); game engine: 25th Anniversary"
        );
    }

    #[test]
    fn a_version_that_cannot_be_read_says_so() {
        assert_eq!(
            hlae_launch("HLAE.exe", None, Some("2.1.0.0"), None),
            "[HLAE] Launching with HLAE.exe (version unreadable); AfxHookGoldSrc.dll (v2.1.0.0); game engine: build unreadable"
        );
        assert!(
            hlae_launch("HLAE.exe", Some("1.0.0.0"), None, Some("pre_anniversary"))
                .ends_with("AfxHookGoldSrc.dll (version unreadable); game engine: pre-Anniversary")
        );
    }
}
