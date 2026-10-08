//! The HD page's style preview (#372 part 3): `tools/hd/compare.py`'s sheet,
//! made from the user's own maps and shown in the app.
//!
//! `compare.py --map <map>` samples a map's most detailed textures that have
//! an HD file, and its sky; `--auto` picks a few maps itself (from the ones
//! `hd_maps.txt` allows) and adds the default models, sprites and detail
//! textures. `--styles` limits the columns. The sheet is a PNG at 1:1 pixels,
//! read back and handed to the page as a `data:` URL.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::python::{self, PythonSource, Using};

/// How often the script is polled, per CLAUDE.md's process rules.
const POLL: Duration = Duration::from_millis(16);

/// A sheet takes a second or two; one still running after this is stuck.
const TIMEOUT: Duration = Duration::from_secs(180);

/// What to compare.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct PreviewRequest {
    /// Maps to sample; none means a few picked by `compare.py --auto`.
    pub maps: Vec<String>,
    /// The styles' columns; none means every style.
    pub styles: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    /// The sheet, as `data:image/png;base64,...`.
    pub image: String,
    pub samples: usize,
    /// The maps `--auto` picked, when it picked them.
    pub maps: Vec<String>,
    /// Samples and maps left out, with why.
    pub skipped: Vec<String>,
}

/// Where the sheet is written: `hd_tools\preview\compare.png`, replaced
/// each time.
pub fn sheet_path(hd_tools: &Path) -> PathBuf {
    hd_tools.join("preview").join("compare.png")
}

/// A map name `compare.py` can take: a `.bsp`'s stem, no path.
fn map_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.+!()[]".contains(&b))
        && !name.starts_with(['.', '-'])
}

/// The arguments after `compare.py <out.png>`.
fn args(request: &PreviewRequest) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    if request.maps.is_empty() {
        args.push("--auto".to_string());
    }
    for map in &request.maps {
        if !map_name_ok(map) {
            return Err(crate::messages::hd_preview_bad_map(map));
        }
        args.push("--map".to_string());
        args.push(map.clone());
    }
    if !request.styles.is_empty() {
        if let Some(bad) = request
            .styles
            .iter()
            .find(|s| !super::my_styles::name_ok(s))
        {
            return Err(crate::messages::hd_build_bad_style(bad));
        }
        args.push("--styles".to_string());
        args.push(request.styles.join(","));
    }
    Ok(args)
}

/// `hdcommon.MAP_LIST`: which maps get HD map textures and skies.
pub const MAP_LIST: &str = "hd_maps.txt";

/// `hdcommon.map_patterns`: the patterns in `hd_maps.txt`, lowercased and
/// without `.bsp`.
fn map_patterns(text: &str) -> Vec<String> {
    text.lines()
        .map(|raw| {
            raw.split('#')
                .next()
                .unwrap_or_default()
                .trim()
                .to_lowercase()
        })
        .map(|line| {
            line.strip_suffix(".bsp")
                .map(str::to_string)
                .unwrap_or(line)
        })
        .filter(|line| !line.is_empty())
        .collect()
}

/// `fnmatch.fnmatchcase` for the two wildcards `hd_maps.txt` documents: `*`
/// any run of characters, `?` exactly one.
fn wildcard_match(pattern: &[u8], name: &[u8]) -> bool {
    match (pattern.first(), name.first()) {
        (None, None) => true,
        (Some(b'*'), _) => {
            wildcard_match(&pattern[1..], name)
                || (!name.is_empty() && wildcard_match(pattern, &name[1..]))
        }
        (Some(b'?'), Some(_)) => wildcard_match(&pattern[1..], &name[1..]),
        (Some(p), Some(n)) if p == n => wildcard_match(&pattern[1..], &name[1..]),
        _ => false,
    }
}

/// The maps the preview can sample: every `.bsp` in `<game>\dod\maps`, as
/// `hd_maps.txt` (in `hd_root`, else beside the scripts from before #385)
/// narrows them, sorted. Those are the maps a build makes map textures for.
pub fn map_choices(game_exe: &Path, hd_root: &Path, scripts: Option<&Path>) -> Vec<String> {
    let Some(maps_dir) = game_exe.parent().map(|g| g.join("dod").join("maps")) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(maps_dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let stem = name
                .strip_suffix(".bsp")
                .or_else(|| name.strip_suffix(".BSP"))?;
            Some(stem.to_string())
        })
        .collect();
    let list = hd_root.join(MAP_LIST);
    let list = match scripts.map(|dir| dir.join(MAP_LIST)) {
        Some(old) if !list.exists() && old.is_file() => old,
        _ => list,
    };
    if let Ok(text) = std::fs::read_to_string(list) {
        let patterns = map_patterns(&text);
        names.retain(|name| {
            let lower = name.to_lowercase();
            patterns
                .iter()
                .any(|p| wildcard_match(p.as_bytes(), lower.as_bytes()))
        });
    }
    names.sort_by_key(|n| n.to_lowercase());
    names
}

/// Reads `compare.py`'s output: the maps it picked, what it skipped, and
/// the sample count.
fn parse_output(stdout: &str) -> (Vec<String>, Vec<String>, usize) {
    let (mut maps, mut skipped, mut samples) = (Vec::new(), Vec::new(), 0);
    for line in stdout.lines().map(str::trim_end) {
        if let Some(list) = line.strip_prefix("maps: ") {
            maps = list
                .split(", ")
                .filter(|m| !m.starts_with('('))
                .map(str::to_string)
                .collect();
        } else if let Some(what) = line.strip_prefix("skipped ") {
            skipped.push(what.to_string());
        } else if line.starts_with("wrote ")
            && let Some(n) = line
                .rsplit_once(": ")
                .and_then(|(_, tail)| tail.split_whitespace().next())
                .and_then(|n| n.parse().ok())
        {
            samples = n;
        }
    }
    (maps, skipped, samples)
}

/// Makes the sheet with `compare.py` and reads it back. `game_exe` is the
/// `hl.exe` the app launches.
pub fn run(
    request: &PreviewRequest,
    python: &Using,
    hd_tools: &Path,
    scripts: &Path,
    game_exe: &Path,
    cancel: &AtomicBool,
) -> Result<Preview, String> {
    let game_dir = game_exe
        .parent()
        .ok_or_else(|| crate::messages::HD_BUILD_NO_GAME_FOLDER.to_string())?;
    let args = args(request)?;
    if python.source == PythonSource::App {
        python::write_pth(hd_tools, scripts)?;
    }
    let sheet = sheet_path(hd_tools);
    if let Some(dir) = sheet.parent() {
        std::fs::create_dir_all(dir).map_err(|e| crate::messages::labeled(dir.display(), e))?;
    }
    let _ = std::fs::remove_file(&sheet);

    let mut cmd = Command::new(&python.probe.exe);
    cmd.arg("-u")
        .arg(scripts.join("compare.py"))
        .arg(&sheet)
        .args(&args)
        .current_dir(scripts)
        .env("HD_GAME", game_dir)
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    python::no_window(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| crate::messages::labeled(&python.probe.exe, e))?;

    // Read both pipes on their own threads so neither fills and stalls it.
    let pipe = |mut from: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut text = Vec::new();
            let _ = from.read_to_end(&mut text);
            String::from_utf8_lossy(&text).to_string()
        })
    };
    let stdout = child.stdout.take().map(|s| pipe(Box::new(s)));
    let stderr = child.stderr.take().map(|s| pipe(Box::new(s)));

    let started = Instant::now();
    let status = loop {
        if cancel.load(Ordering::Relaxed) || started.elapsed() > TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(crate::messages::HD_SETUP_CANCELLED.to_string());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(POLL),
            Err(e) => return Err(crate::messages::labeled(&python.probe.exe, e)),
        }
    };
    let stdout = stdout.and_then(|t| t.join().ok()).unwrap_or_default();
    let stderr = stderr.and_then(|t| t.join().ok()).unwrap_or_default();
    if !status.success() {
        let detail = if stderr.trim().is_empty() {
            stdout
        } else {
            stderr
        };
        let tail: Vec<&str> = detail.lines().rev().take(12).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        return Err(crate::messages::hd_preview_failed(status, &tail.join("\n")));
    }

    let png = std::fs::read(&sheet).map_err(|e| crate::messages::labeled(sheet.display(), e))?;
    let (maps, skipped, samples) = parse_output(&stdout);
    use base64::Engine as _;
    Ok(Preview {
        image: format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png)
        ),
        samples,
        maps,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    fn request(maps: &[&str], styles: &[&str]) -> PreviewRequest {
        PreviewRequest {
            maps: maps.iter().map(|s| s.to_string()).collect(),
            styles: styles.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn a_request_becomes_compare_py_arguments() {
        assert_eq!(args(&request(&[], &[])).unwrap(), ["--auto"]);
        assert_eq!(
            args(&request(&["dod_anzio", "dod_caen_b2"], &["plain", "crisp"])).unwrap(),
            [
                "--map",
                "dod_anzio",
                "--map",
                "dod_caen_b2",
                "--styles",
                "plain,crisp"
            ]
        );
        for bad in ["../dod_anzio", "maps\\x", "a b", ".hidden", "--auto"] {
            assert!(args(&request(&[bad], &[])).is_err(), "{bad}");
        }
        assert!(args(&request(&[], &["Plain"])).is_err());
    }

    #[test]
    fn the_output_says_what_was_picked_and_skipped() {
        let (maps, skipped, samples) = parse_output(
            "maps: dod_aleutian, dod_armory_b3\r\n\
             skipped dod_nowhere: no HD map textures built for it\r\n\
             skipped model:v_bar.mdl: not found\r\n\
             wrote C:\\x: y\\compare.png: 13 sample(s)\r\n",
        );
        assert_eq!(maps, ["dod_aleutian", "dod_armory_b3"]);
        assert_eq!(
            skipped,
            [
                "dod_nowhere: no HD map textures built for it",
                "model:v_bar.mdl: not found"
            ]
        );
        assert_eq!(samples, 13);
        let (maps, ..) = parse_output("maps: (none with HD map textures built)\n");
        assert!(maps.is_empty());
    }

    #[test]
    fn hd_maps_txt_narrows_the_maps_as_the_scripts_do() {
        let dir = Scratch::new("hd_preview_maps");
        let maps = dir.join("dod").join("maps");
        std::fs::create_dir_all(&maps).unwrap();
        std::fs::write(dir.join("hl.exe"), b"").unwrap();
        for name in [
            "dod_anzio.bsp",
            "dod_Anzio2.bsp",
            "dod_caen.bsp",
            "dod_harrington.bsp",
            "notes.txt",
        ] {
            std::fs::write(maps.join(name), b"").unwrap();
        }
        let root = dir.join("dod").join("dodstudio_hd");
        let exe = dir.join("hl.exe");
        // No hd_maps.txt: every map.
        assert_eq!(
            map_choices(&exe, &root, None),
            ["dod_anzio", "dod_Anzio2", "dod_caen", "dod_harrington"]
        );
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(MAP_LIST),
            "# mine\nDOD_ANZIO*  # both\ndod_harr?ngton.bsp\ndod_nowhere\n",
        )
        .unwrap();
        assert_eq!(
            map_choices(&exe, &root, None),
            ["dod_anzio", "dod_Anzio2", "dod_harrington"]
        );
        for (pattern, name, hit) in [
            ("dod_*sherman*", "dod_sherman_b2", true),
            ("dod_*sherman*", "dod_caen", false),
            ("dod_caen", "dod_caen2", false),
            ("*", "", true),
            ("a?c", "ac", false),
        ] {
            assert_eq!(
                wildcard_match(pattern.as_bytes(), name.as_bytes()),
                hit,
                "{pattern} {name}"
            );
        }
        let common = include_str!("../../../goldsrc-hooks/tools/hd/hdcommon.py");
        assert!(common.contains(&format!("MAP_LIST = \"{MAP_LIST}\"")));
        assert!(common.contains("fnmatch.fnmatchcase(n.lower(), p)"));
    }

    /// The lines parsed here are the ones `compare.py` prints.
    #[test]
    fn compare_py_prints_what_is_parsed() {
        let script = include_str!("../../../goldsrc-hooks/tools/hd/compare.py");
        for line in [
            r#"print(f"maps: {', '.join(maps) or '(none with HD map textures built)'}", flush=True)"#,
            r#"print(f"skipped {m}: no HD map textures built for it", flush=True)"#,
            r#"print(f"skipped {spec}: not found")"#,
            r#"print(f"wrote {out}: {len(rows)} sample(s)")"#,
            r#"ap.add_argument("--map", action="append", default=[])"#,
            r#"ap.add_argument("--auto", action="store_true")"#,
            r#"ap.add_argument("--styles")"#,
        ] {
            assert!(script.contains(line), "{line}");
        }
    }

    /// The real thing, against the tiny fake install `build.rs`'s test
    /// uses: build its one texture in plain, then compare.
    #[test]
    #[ignore = "needs a Python with numpy, Pillow and SciPy"]
    fn a_sheet_is_made_for_real() {
        let tools = Scratch::new("hd_preview_real_tools");
        let using = python::resolve(&tools).using.expect("a usable Python");
        let game = Scratch::new("hd_preview_real_game");
        let maps = game.join("dod").join("maps");
        std::fs::create_dir_all(&maps).unwrap();
        std::fs::write(game.join("hl.exe"), b"").unwrap();
        std::fs::write(
            maps.join("test_map.bsp"),
            super::super::build::tests::tiny_bsp(),
        )
        .unwrap();
        let build = super::super::build::BuildRequest {
            styles: vec!["plain".into()],
            types: vec!["world".into()],
        };
        super::super::build::run(
            &build,
            &using,
            &tools,
            &super::super::setup::realesrgan_dir(&tools),
            &super::super::build::dev_scripts_dir(),
            &game.join("hl.exe"),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();

        let preview = run(
            &request(&["test_map"], &["plain"]),
            &using,
            &tools,
            &super::super::build::dev_scripts_dir(),
            &game.join("hl.exe"),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(preview.samples, 1, "{:?}", preview.skipped);
        assert!(
            preview
                .image
                .starts_with("data:image/png;base64,iVBORw0KGgo")
        );
    }
}
