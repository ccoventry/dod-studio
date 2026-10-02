//! Launch Preview without patching (#434, step 2): the game plays the
//! original demo, and the in-game DoD Studio window's Highlights tab (#565)
//! lists its highlights with Go, in place of the `<stem>_preview.dem` copy
//! with a bookmark per highlight.
//!
//! This is the pure half: where `viewdemo` finds a demo, and what to send.
//! It can't always: a demo on another drive than the game has no path
//! relative to the game's folder, and the engine copies `viewdemo`'s argument
//! into a 256-byte buffer. Those previews stay patched.

use std::path::{Component, Path};

/// The longest `viewdemo` argument used: the engine's buffer is 256 bytes,
/// and it adds `.dem`.
pub const MAX_DEMO_ARG: usize = 240;

/// The in-game window's Highlights tab, opened once the demo is playing.
pub const OPEN_HIGHLIGHTS: &str = "dodstudio_panel highlights";

/// `demo` as `viewdemo` finds it from the mod folder `mod_dir`: relative,
/// with `/`, without `.dem`. `None` when there is no such path (another
/// drive), it is too long, or it holds a character that would end the
/// console argument early or start a second command.
pub fn viewdemo_arg(mod_dir: &Path, demo: &Path) -> Option<String> {
    let base = normal_components(&std::fs::canonicalize(mod_dir).ok()?)?;
    let target = normal_components(&std::fs::canonicalize(demo).ok()?)?;
    relative_arg(&base, &target)
}

/// The components of an absolute path, the `\\?\` prefix folded away.
fn normal_components(path: &Path) -> Option<Vec<String>> {
    let mut out = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => {
                // `\\?\C:` and `C:` are the same drive.
                let text = prefix.as_os_str().to_string_lossy().to_string();
                out.push(text.trim_start_matches(r"\\?\").to_ascii_uppercase());
            }
            Component::RootDir => {}
            Component::Normal(part) => out.push(part.to_string_lossy().into_owned()),
            Component::CurDir | Component::ParentDir => return None,
        }
    }
    Some(out)
}

fn relative_arg(base: &[String], target: &[String]) -> Option<String> {
    // Different drives: no relative path.
    if !base.first()?.eq_ignore_ascii_case(target.first()?) {
        return None;
    }
    let common = base
        .iter()
        .zip(target)
        .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
        .count();
    let mut parts: Vec<String> =
        std::iter::repeat_n("..".to_string(), base.len() - common).collect();
    parts.extend(target[common..].iter().cloned());
    let mut arg = parts.join("/");
    if arg.to_ascii_lowercase().ends_with(".dem") {
        arg.truncate(arg.len() - 4);
    }
    let safe =
        !arg.is_empty() && arg.len() <= MAX_DEMO_ARG && !arg.contains(['"', ';', '\n', '\r']);
    safe.then_some(arg)
}

/// What the game is sent (or started with) for a preview of `arg`.
pub fn viewdemo_line(arg: &str) -> String {
    format!("viewdemo \"{arg}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(path: &str) -> Vec<String> {
        path.split('\\').map(str::to_string).collect()
    }

    #[test]
    fn a_demo_in_the_mod_folder_is_its_name() {
        let base = parts(r"C:\Games\Half-Life\dod");
        assert_eq!(
            relative_arg(&base, &parts(r"C:\Games\Half-Life\dod\match1.dem")).as_deref(),
            Some("match1")
        );
        assert_eq!(
            relative_arg(&base, &parts(r"C:\Games\Half-Life\dod\scrims\b.DEM")).as_deref(),
            Some("scrims/b")
        );
    }

    #[test]
    fn a_demo_elsewhere_on_the_drive_is_reached_with_dots() {
        let base = parts(r"C:\Games\Half-Life\dod");
        assert_eq!(
            relative_arg(&base, &parts(r"C:\demos\k4 philly\x.dem")).as_deref(),
            Some("../../../demos/k4 philly/x")
        );
        // Case differences in the shared part don't matter.
        assert_eq!(
            relative_arg(&base, &parts(r"c:\games\half-life\dod\y.dem")).as_deref(),
            Some("y")
        );
    }

    #[test]
    fn another_drive_or_a_bad_name_cannot_be_previewed_in_place() {
        let base = parts(r"C:\Games\Half-Life\dod");
        assert_eq!(relative_arg(&base, &parts(r"D:\demos\x.dem")), None);
        assert_eq!(relative_arg(&base, &parts(r"C:\demos\a;quit.dem")), None);
        assert_eq!(relative_arg(&base, &parts(r#"C:\demos\a"b.dem"#)), None);
        let long = format!(r"C:\{}\x.dem", "d".repeat(300));
        assert_eq!(relative_arg(&base, &parts(&long)), None);
    }

    #[test]
    fn the_line_quotes_the_path() {
        assert_eq!(
            viewdemo_line("../demos/k4 philly/x"),
            r#"viewdemo "../demos/k4 philly/x""#
        );
    }

    #[test]
    fn real_paths_resolve_through_the_filesystem() {
        let dir = std::env::temp_dir().join(format!("preview_in_place_{}", std::process::id()));
        let mod_dir = dir.join("hl").join("dod");
        let demos = dir.join("demos");
        std::fs::create_dir_all(&mod_dir).unwrap();
        std::fs::create_dir_all(&demos).unwrap();
        let demo = demos.join("x.dem");
        std::fs::write(&demo, b"HLDEMO").unwrap();
        assert_eq!(
            viewdemo_arg(&mod_dir, &demo).as_deref(),
            Some("../../demos/x")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
