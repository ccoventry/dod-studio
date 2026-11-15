//! Launch Preview without patching (#434, step 2): the game plays the
//! original demo, and the in-game DoD Studio window's Highlights tab (#565)
//! lists its highlights with Go, in place of the `<stem>_preview.dem` copy
//! with a bookmark per highlight.
//!
//! This is the pure half: where `viewdemo` finds a demo, and what to send.
//! A demo on the game's drive goes by a path relative to the game's folder.
//! One on another drive has none; a hook DLL from #779 on opens it by its
//! full path, `viewdemo "D:/folder/demo"` (goldsrc-hooks
//! `demo_other_drives`, #776), and for an older DLL the preview stays
//! patched. The engine copies `viewdemo`'s argument into a 256-byte buffer,
//! so a longer path stays patched too.

use std::path::{Component, Path};

/// The longest `viewdemo` argument used: the engine's buffer is 256 bytes,
/// and it adds `.dem`.
pub const MAX_DEMO_ARG: usize = 240;

/// The in-game window's Highlights tab, opened once the demo is playing.
pub const OPEN_HIGHLIGHTS: &str = "ds_ui_panel highlights";

/// `demo` as `viewdemo` finds it from the mod folder `mod_dir`: relative,
/// with `/`, without `.dem`. `None` when there is no such path (another
/// drive), it is too long, or it holds a character that would end the
/// console argument early or start a second command.
pub fn viewdemo_arg(mod_dir: &Path, demo: &Path) -> Option<String> {
    let base = normal_components(&std::fs::canonicalize(mod_dir).ok()?)?;
    let target = normal_components(&std::fs::canonicalize(demo).ok()?)?;
    relative_arg(&base, &target)
}

/// The bytes a hook DLL that plays another drive's demo by its full path
/// carries (goldsrc-hooks `demo_other_drives::FULL_PATH_MARKER`, #779): its
/// batch, review and Go send such a demo to `viewdemo` as `D:/folder/demo`.
pub const HOOK_FULL_PATH_MARKER: &str = "ds-viewdemo-full-path-779";

/// The line that turns the hook's other-drive opens on, sent ahead of a
/// demo played by its full path in case the player turned them off.
pub const OTHER_DRIVES_ON: &str = "ds_demo_other_drives 1";

/// Whether the hook DLL at `dll` plays a demo on another drive by its full
/// path ([`HOOK_FULL_PATH_MARKER`]). An older DLL, or one that can't be
/// read, doesn't.
pub fn hook_plays_other_drives(dll: &Path) -> bool {
    let needle = HOOK_FULL_PATH_MARKER.as_bytes();
    std::fs::read(dll).is_ok_and(|bytes| bytes.windows(needle.len()).any(|w| w == needle))
}

/// [`viewdemo_arg`], and with `other_drives` (the hook DLL plays another
/// drive's demo, [`hook_plays_other_drives`]) a demo on another drive by
/// its full path, `D:/folder/demo`.
pub fn viewdemo_arg_any_drive(mod_dir: &Path, demo: &Path, other_drives: bool) -> Option<String> {
    let base = normal_components(&std::fs::canonicalize(mod_dir).ok()?)?;
    let target = normal_components(&std::fs::canonicalize(demo).ok()?)?;
    relative_arg(&base, &target).or_else(|| {
        other_drives
            .then(|| full_path_arg(&base, &target))
            .flatten()
    })
}

/// Whether `arg` names a demo by a drive letter, `D:/...`.
pub fn is_full_path_arg(arg: &str) -> bool {
    matches!(arg.as_bytes(), [letter, b':', b'/', ..] if letter.is_ascii_alphabetic())
}

/// `target` on another drive than `base`, by its full path: `D:/folder/demo`.
/// `None` on the same drive (that goes by [`relative_arg`]), without a drive
/// letter (a network path), or for a name [`safe_arg`] refuses.
fn full_path_arg(base: &[String], target: &[String]) -> Option<String> {
    let drive = target.first()?;
    let has_letter = matches!(drive.as_bytes(), [letter, b':'] if letter.is_ascii_alphabetic());
    if !has_letter || target.len() < 2 || base.first()?.eq_ignore_ascii_case(drive) {
        return None;
    }
    safe_arg(target.join("/"))
}

/// `arg` without `.dem`, when it fits the engine's buffer and holds nothing
/// that would end the console argument early or start a second command, and
/// is all ASCII.
fn safe_arg(mut arg: String) -> Option<String> {
    if arg.to_ascii_lowercase().ends_with(".dem") {
        arg.truncate(arg.len() - 4);
    }
    // ASCII only: `viewdemo` lower-cases the name byte by byte before opening
    // it, which breaks any other character (`Grüße` became `gr???e`, and
    // "Coudn't open demo file", measured on POST, #779).
    let safe = !arg.is_empty()
        && arg.is_ascii()
        && arg.len() <= MAX_DEMO_ARG
        && !arg.contains(['"', ';', '\n', '\r']);
    safe.then_some(arg)
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
    safe_arg(parts.join("/"))
}

/// What the game is sent (or started with) for a preview of `arg`.
pub fn viewdemo_line(arg: &str) -> String {
    format!("viewdemo \"{arg}\"")
}

/// The console lines a preview of `arg` sends: [`viewdemo_line`], after
/// [`OTHER_DRIVES_ON`] for a demo on another drive.
pub fn preview_lines(arg: &str) -> Vec<String> {
    let mut lines = Vec::new();
    if is_full_path_arg(arg) {
        lines.push(OTHER_DRIVES_ON.to_string());
    }
    lines.push(viewdemo_line(arg));
    lines
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
    fn another_drive_goes_by_its_full_path() {
        let base = parts(r"C:\Games\Half-Life\dod");
        assert_eq!(
            full_path_arg(&base, &parts(r"H:\WSOD 25\demos\x.dem")).as_deref(),
            Some("H:/WSOD 25/demos/x")
        );
        // viewdemo can't open a name with anything but ASCII, on any drive.
        assert_eq!(full_path_arg(&base, &parts(r"H:\Grüße\x.dem")), None);
        assert_eq!(
            relative_arg(&base, &parts(r"C:\Games\Half-Life\dod\Grüße.dem")),
            None
        );
        // A non-ASCII folder the game and the demo share is not in the path.
        assert_eq!(
            relative_arg(
                &parts(r"C:\Spiele\Größe\dod"),
                &parts(r"C:\Spiele\Größe\dod\x.dem")
            )
            .as_deref(),
            Some("x")
        );
        // A demo at the drive's root.
        assert_eq!(
            full_path_arg(&base, &parts(r"D:\x.DEM")).as_deref(),
            Some("D:/x")
        );
        // The game's own drive goes by the relative path, not this.
        assert_eq!(full_path_arg(&base, &parts(r"c:\demos\x.dem")), None);
        assert_eq!(full_path_arg(&base, &parts(r"D:\a;quit.dem")), None);
        let long = format!(r"D:\{}\x.dem", "d".repeat(240));
        assert_eq!(full_path_arg(&base, &parts(&long)), None);
    }

    #[test]
    fn a_full_path_preview_turns_the_hooks_other_drives_on_first() {
        assert!(is_full_path_arg("D:/x"));
        assert!(!is_full_path_arg("../../x"));
        assert!(!is_full_path_arg("x"));
        assert_eq!(
            preview_lines("D:/demos/x"),
            [OTHER_DRIVES_ON, r#"viewdemo "D:/demos/x""#]
        );
        assert_eq!(preview_lines("../x"), [r#"viewdemo "../x""#]);
    }

    #[test]
    fn the_marker_is_the_hooks_own() {
        let hook = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../goldsrc-hooks/src/demo_other_drives.rs");
        let source = std::fs::read_to_string(&hook).unwrap();
        assert!(
            source.contains(&format!(
                "pub const FULL_PATH_MARKER: &str = \"{HOOK_FULL_PATH_MARKER}\";"
            )),
            "{} has another marker",
            hook.display()
        );
        assert!(source.contains(&format!(
            "console_name!(\"{}\")",
            OTHER_DRIVES_ON
                .trim_start_matches("ds_")
                .trim_end_matches(" 1")
        )));
    }

    #[test]
    fn a_dll_says_whether_it_plays_other_drives() {
        let dir = crate::test_support::Scratch::new("hook_marker");
        let new = dir.join("new.dll");
        let old = dir.join("old.dll");
        std::fs::write(&new, format!("MZ..{HOOK_FULL_PATH_MARKER}..")).unwrap();
        std::fs::write(&old, b"MZ..ds_demo_other_drives..").unwrap();
        assert!(hook_plays_other_drives(&new));
        assert!(!hook_plays_other_drives(&old));
        assert!(!hook_plays_other_drives(&dir.join("missing.dll")));
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
