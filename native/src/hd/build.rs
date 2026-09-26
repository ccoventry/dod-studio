//! Runs the HD build from the app (#372 part 2).
//!
//! The build is `goldsrc-hooks/tools/hd/build_all.py`, run as it is by the
//! Python [`super::python`] picks, so the app and the command line make the
//! same files the same way. This module starts it, turns its output into
//! progress, and stops it on Cancel.
//!
//! - **Progress:** `build_all.py` prints `@@step <n> <of> <style> <type>`
//!   before each step and a log line after it; see [`parse_line`].
//! - **Resume:** every step skips files that already exist, so running again
//!   after a Cancel carries on. The scripts write each file under a `.part`
//!   name and rename it when whole, so a stopped step never leaves a broken
//!   file that counts as built.
//! - **Cancel** ends the whole process tree (`build_all.py` runs each step,
//!   and the upscaler, as child processes), via `taskkill /T`.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::Serialize;

use super::python::{self, PythonSource, Using};
use super::{ASSET_TYPES, BUILT_IN_STYLES, setup};

/// How often the running build is polled, per CLAUDE.md's process rules.
const POLL: Duration = Duration::from_millis(16);

/// How many of the build's last error lines a failure reports.
const ERROR_TAIL: usize = 12;

/// What to build: styles by name, asset types by folder name.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct BuildRequest {
    pub styles: Vec<String>,
    pub types: Vec<String>,
    /// The largest side to build, one of [`CAPS`] (`hdcommon.CAPS`). Files
    /// already built smaller than the cap makes them are built again.
    #[serde(default = "default_cap")]
    pub cap: u32,
}

/// The sizes a build can cap at, and the one everything was built at before
/// there was a choice.
pub const CAPS: [u32; 3] = [1024, 2048, 4096];

fn default_cap() -> u32 {
    CAPS[0]
}

/// Where a running build is, for the progress line.
#[derive(Debug, Clone, Default, Serialize)]
pub struct BuildProgress {
    /// 1-based step, of `steps` (one per style and type).
    pub step: usize,
    pub steps: usize,
    pub style: String,
    pub asset_type: String,
    /// The last line `build_all.py` logged, when this report is for one.
    pub line: Option<String>,
    pub elapsed_secs: u64,
}

/// What a finished build did.
#[derive(Debug, Clone, Serialize)]
pub struct BuildOutcome {
    pub steps: usize,
    pub elapsed_secs: u64,
    /// `build_all.log`, which has every step's counts.
    pub log_path: String,
}

/// One line of `build_all.py`'s output.
#[derive(Debug, PartialEq, Eq)]
pub enum Line {
    Step {
        step: usize,
        steps: usize,
        style: String,
        asset_type: String,
    },
    /// A log line, without its `[hh:mm:ss] ` time.
    Log(String),
}

/// The scripts as the repo has them, for a dev build (`cargo run`,
/// `tauri dev`), which has no bundled copy.
pub fn dev_scripts_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("goldsrc-hooks")
        .join("tools")
        .join("hd")
}

/// The scripts to run: the app's bundled copy (`hd-scripts` in its
/// resources), else the repo's. A debug build tries the repo's first: `tauri
/// dev` copies the resources too, but only when the app is rebuilt, so the
/// repo's are the ones being edited. `None` when neither has `build_all.py`.
pub fn scripts_dir(bundled: Option<&Path>) -> Option<PathBuf> {
    let bundled = bundled.map(Path::to_path_buf);
    let order = if cfg!(debug_assertions) {
        [Some(dev_scripts_dir()), bundled]
    } else {
        [bundled, Some(dev_scripts_dir())]
    };
    order
        .into_iter()
        .flatten()
        .find(|dir| dir.join("build_all.py").is_file())
}

/// Style names are folder names, as `styles.py` and the hook accept them.
fn style_name_ok(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// Refuses a request the scripts would refuse, and one whose AI styles have
/// no upscaler or model to run, before anything starts.
pub fn check(request: &BuildRequest, realesrgan: &Path) -> Result<(), String> {
    if request.styles.is_empty() || request.types.is_empty() {
        return Err(crate::messages::HD_BUILD_NOTHING_CHOSEN.to_string());
    }
    if let Some(bad) = request.styles.iter().find(|s| !style_name_ok(s)) {
        return Err(crate::messages::hd_build_bad_style(bad));
    }
    if let Some(bad) = request
        .types
        .iter()
        .find(|t| !ASSET_TYPES.contains(&t.as_str()))
    {
        return Err(crate::messages::hd_build_bad_type(bad));
    }
    if !CAPS.contains(&request.cap) {
        return Err(crate::messages::hd_build_bad_cap(request.cap));
    }
    for style in &request.styles {
        let model = BUILT_IN_STYLES
            .iter()
            .find(|s| s.name == style)
            .and_then(|s| s.model);
        if let Some(model) = model
            && !(setup::upscaler_exe(realesrgan).is_file()
                && setup::model_present(realesrgan, model))
        {
            return Err(crate::messages::hd_build_needs_upscaler(style));
        }
    }
    Ok(())
}

/// Reads one line of `build_all.py`'s output.
pub fn parse_line(line: &str) -> Option<Line> {
    let line = line.trim_end();
    if let Some(rest) = line.strip_prefix("@@step ") {
        let mut words = rest.split_whitespace();
        return Some(Line::Step {
            step: words.next()?.parse().ok()?,
            steps: words.next()?.parse().ok()?,
            style: words.next()?.to_string(),
            asset_type: words.next()?.to_string(),
        });
    }
    // "[01:40:37] plain      sky     exit 0, ..."
    let rest = line.strip_prefix('[')?;
    let (time, message) = rest.split_once("] ")?;
    (time.len() == 8 && time.bytes().filter(|&b| b == b':').count() == 2)
        .then(|| Line::Log(message.to_string()))
}

/// Ends `pid` and everything it started, if it is still running when this
/// is dropped: the `kill_on_drop` of a process tree.
struct TreeGuard(Option<u32>);

impl TreeGuard {
    fn kill(&mut self) {
        if let Some(pid) = self.0.take() {
            let mut cmd = Command::new("taskkill");
            cmd.args(["/PID", &pid.to_string(), "/T", "/F"])
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            python::no_window(&mut cmd);
            let _ = cmd.status();
        }
    }
}

impl Drop for TreeGuard {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Runs the build and waits for it. `game_exe` is the `hl.exe` the app
/// launches, and `realesrgan` the upscaler folder
/// ([`super::upscaler::resolve`]); `progress` is called at each step and log
/// line (a few times a minute, so no throttling is needed).
#[allow(clippy::too_many_arguments)]
pub fn run(
    request: &BuildRequest,
    python: &Using,
    hd_tools: &Path,
    realesrgan: &Path,
    scripts: &Path,
    game_exe: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&BuildProgress),
) -> Result<BuildOutcome, String> {
    check(request, realesrgan)?;
    let game_dir = game_exe
        .parent()
        .ok_or_else(|| crate::messages::HD_BUILD_NO_GAME_FOLDER.to_string())?;
    if python.source == PythonSource::App {
        python::write_pth(hd_tools, scripts)?;
    }

    let mut cmd = Command::new(&python.probe.exe);
    cmd.arg("-u")
        .arg(scripts.join("build_all.py"))
        .arg("--game")
        .arg(game_dir)
        .arg("--types")
        .arg(request.types.join(","))
        .args(&request.styles)
        .current_dir(scripts)
        .env("HD_CAP", request.cap.to_string())
        .env("REALESRGAN", setup::upscaler_exe(realesrgan))
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    python::no_window(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| crate::messages::labeled(&python.probe.exe, e))?;
    let mut guard = TreeGuard(Some(child.id()));

    // Both pipes are read on their own threads, so neither fills up and
    // stalls the build while the other is being read.
    let (tx, rx) = mpsc::channel::<(bool, String)>();
    let readers: Vec<_> = [
        child
            .stdout
            .take()
            .map(|s| (false, Box::new(s) as Box<dyn std::io::Read + Send>)),
        child
            .stderr
            .take()
            .map(|s| (true, Box::new(s) as Box<dyn std::io::Read + Send>)),
    ]
    .into_iter()
    .flatten()
    .map(|(is_err, pipe)| {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(pipe).split(b'\n').map_while(Result::ok) {
                let text = String::from_utf8_lossy(&line).trim_end().to_string();
                if tx.send((is_err, text)).is_err() {
                    break;
                }
            }
        })
    })
    .collect();
    drop(tx);

    let started = Instant::now();
    let mut state = BuildProgress::default();
    let mut errors: Vec<String> = Vec::new();
    let mut last_log: Option<String> = None;
    let mut handle = |is_err: bool, text: String, state: &mut BuildProgress| {
        if is_err {
            errors.push(text);
            if errors.len() > ERROR_TAIL {
                errors.remove(0);
            }
            return;
        }
        match parse_line(&text) {
            Some(Line::Step {
                step,
                steps,
                style,
                asset_type,
            }) => {
                *state = BuildProgress {
                    step,
                    steps,
                    style,
                    asset_type,
                    line: None,
                    elapsed_secs: started.elapsed().as_secs(),
                };
                progress(state);
            }
            Some(Line::Log(message)) => {
                last_log = Some(message.clone());
                state.line = Some(message);
                state.elapsed_secs = started.elapsed().as_secs();
                progress(state);
            }
            None => {}
        }
    };

    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            guard.kill();
            let _ = child.wait();
            return Err(crate::messages::HD_SETUP_CANCELLED.to_string());
        }
        while let Ok((is_err, text)) = rx.try_recv() {
            handle(is_err, text, &mut state);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(POLL),
            Err(e) => return Err(crate::messages::labeled(&python.probe.exe, e)),
        }
    };
    guard.0 = None; // exited by itself: nothing left to end
    for reader in readers {
        let _ = reader.join();
    }
    while let Ok((is_err, text)) = rx.try_recv() {
        handle(is_err, text, &mut state);
    }

    if !status.success() {
        let detail = if errors.is_empty() {
            last_log.unwrap_or_default()
        } else {
            errors.join("\n")
        };
        return Err(crate::messages::hd_build_failed(status, &detail));
    }
    Ok(BuildOutcome {
        steps: state.steps,
        elapsed_secs: started.elapsed().as_secs(),
        log_path: super::hd_root(game_exe)
            .map(|root| root.join("build_all.log").to_string_lossy().to_string())
            .unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    #[test]
    fn step_and_log_lines_parse() {
        assert_eq!(
            parse_line("@@step 3 35 remacri world\r\n"),
            Some(Line::Step {
                step: 3,
                steps: 35,
                style: "remacri".into(),
                asset_type: "world".into()
            })
        );
        assert_eq!(
            parse_line("[01:40:37] plain      sky     exit 0, 0 new, 198 total, 0s"),
            Some(Line::Log(
                "plain      sky     exit 0, 0 new, 198 total, 0s".into()
            ))
        );
        assert_eq!(parse_line("wrote 3 replacement(s)"), None);
        assert_eq!(parse_line("@@step x 35 remacri world"), None);
        assert_eq!(parse_line("[not a time] hello"), None);
    }

    /// The marker `build_all.py` prints is the one parsed here.
    #[test]
    fn build_all_prints_the_step_marker() {
        let script = include_str!("../../../goldsrc-hooks/tools/hd/build_all.py");
        assert!(
            script.contains(r#"print(f"@@step {number} {len(steps)} {style} {kind}", flush=True)"#)
        );
        assert!(script.contains(r#"line = f"[{datetime.datetime.now():%H:%M:%S}] {msg}""#));
        assert!(script.contains("C.clear_partial(out)"));
    }

    #[test]
    fn the_dev_build_finds_the_repo_scripts() {
        assert!(dev_scripts_dir().join("build_all.py").is_file());
        assert_eq!(scripts_dir(None), Some(dev_scripts_dir()));
        let bundled = Scratch::new("hd_build_bundled_scripts");
        // A bundled folder without the scripts is passed over either way.
        assert_eq!(scripts_dir(Some(&bundled)), Some(dev_scripts_dir()));
        // With them, a release build uses it and a debug build the repo's.
        std::fs::write(bundled.join("build_all.py"), b"").unwrap();
        let expected = if cfg!(debug_assertions) {
            dev_scripts_dir()
        } else {
            bundled.to_path_buf()
        };
        assert_eq!(scripts_dir(Some(&bundled)), Some(expected));
    }

    /// Everything the scripts read at run time is bundled into
    /// `hd-scripts`, and nothing of the user's is (their own lists may
    /// still sit in tools/hd from before #385).
    #[test]
    fn the_app_bundles_every_script_and_none_of_the_users_files() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../../../studio/src-tauri/tauri.conf.json"))
                .unwrap();
        let resources = conf["bundle"]["resources"].as_object().unwrap();
        let bundled: Vec<&str> = resources
            .iter()
            .filter(|(_, to)| to.as_str().is_some_and(|t| t.starts_with("hd-scripts")))
            .map(|(from, _)| from.as_str())
            .collect();
        let tools = "../../goldsrc-hooks/tools/hd/";
        assert!(
            bundled.contains(&format!("{tools}*.py").as_str()),
            "{bundled:?}"
        );
        // The data file build_all.py passes to models_hd.py.
        assert!(
            bundled.contains(&format!("{tools}valve_models.txt").as_str()),
            "{bundled:?}"
        );
        for user_file in ["hd_maps.txt", "my_styles.txt"] {
            assert!(
                !bundled
                    .iter()
                    .any(|b| b.ends_with(user_file) || b.ends_with("*.txt")),
                "{user_file} could be bundled: {bundled:?}"
            );
        }
        let script = include_str!("../../../goldsrc-hooks/tools/hd/build_all.py");
        assert!(script.contains(r#"os.path.join(C.HERE, "valve_models.txt")"#));
    }

    #[test]
    fn requests_are_checked_before_anything_runs() {
        let tools = Scratch::new("hd_build_check");
        let realesrgan = setup::realesrgan_dir(&tools);
        let request = |styles: &[&str], types: &[&str]| BuildRequest {
            styles: styles.iter().map(|s| s.to_string()).collect(),
            types: types.iter().map(|s| s.to_string()).collect(),
            cap: 1024,
        };
        // The cap is one of the scripts' sizes.
        let sized = |cap| BuildRequest {
            cap,
            ..request(&["plain"], &["world"])
        };
        assert!(check(&sized(2048), &realesrgan).is_ok());
        assert!(check(&sized(4096), &realesrgan).is_ok());
        assert!(check(&sized(512), &realesrgan).is_err());
        assert!(check(&sized(1536), &realesrgan).is_err());
        let common = include_str!("../../../goldsrc-hooks/tools/hd/hdcommon.py");
        assert!(common.contains(&format!("CAPS = ({}, {}, {})", CAPS[0], CAPS[1], CAPS[2])));
        assert!(common.contains(r#"os.environ.get("HD_CAP", "1024")"#));
        assert!(check(&request(&[], &["world"]), &realesrgan).is_err());
        assert!(check(&request(&["plain"], &[]), &realesrgan).is_err());
        assert!(check(&request(&["../evil"], &["world"]), &realesrgan).is_err());
        assert!(check(&request(&["plain"], &["world", "maps"]), &realesrgan).is_err());
        // No AI needed: fine without the upscaler. A custom style is left to
        // the scripts, which know my_styles.txt.
        assert!(
            check(
                &request(&["plain", "blend", "crisp"], &["world", "sky"]),
                &realesrgan
            )
            .is_ok()
        );
        // An AI style needs the upscaler and its own model.
        assert!(check(&request(&["ultrasharp"], &["world"]), &realesrgan).is_err());
        std::fs::create_dir_all(realesrgan.join("models")).unwrap();
        std::fs::write(setup::upscaler_exe(&realesrgan), b"").unwrap();
        assert!(check(&request(&["ultrasharp"], &["world"]), &realesrgan).is_err());
        for ext in ["param", "bin"] {
            std::fs::write(realesrgan.join(format!("models/ultrasharp-4x.{ext}")), b"").unwrap();
        }
        assert!(check(&request(&["ultrasharp"], &["world"]), &realesrgan).is_ok());
    }

    #[test]
    fn style_names_are_folder_names() {
        for ok in ["ultrasharp", "sharp70", "my-style_2"] {
            assert!(style_name_ok(ok), "{ok}");
        }
        for bad in ["", "Ultra", "a b", "../x", "x/y", &"a".repeat(33)] {
            assert!(!style_name_ok(bad), "{bad}");
        }
    }

    /// The real thing: whatever Python this PC resolves to runs a real
    /// `build_all.py` against a tiny fake install (one embedded-texture map,
    /// nothing else), with the plain style, so no GPU or upscaler is needed.
    #[test]
    #[ignore = "needs a Python with numpy, Pillow and SciPy"]
    fn a_plain_build_runs_for_real() {
        let tools = Scratch::new("hd_build_real_tools");
        let status = python::resolve(&tools);
        let using = status.using.expect("a usable Python on this PC");

        let game = Scratch::new("hd_build_real_game");
        let maps = game.join("dod").join("maps");
        std::fs::create_dir_all(&maps).unwrap();
        std::fs::write(game.join("hl.exe"), b"").unwrap();
        std::fs::write(maps.join("test_map.bsp"), tiny_bsp()).unwrap();

        let request = BuildRequest {
            styles: vec!["plain".into()],
            types: vec!["world".into()],
            cap: 1024,
        };
        let mut seen = Vec::new();
        let outcome = run(
            &request,
            &using,
            &tools,
            &setup::realesrgan_dir(&tools),
            &dev_scripts_dir(),
            &game.join("hl.exe"),
            &AtomicBool::new(false),
            &mut |p| seen.push(p.clone()),
        )
        .unwrap();
        assert_eq!(outcome.steps, 1);
        assert!(seen.iter().any(|p| p.step == 1 && p.asset_type == "world"));
        let built: Vec<_> = std::fs::read_dir(game.join("dod/dodstudio_hd/world/plain"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(built.len(), 1, "{built:?}");
        assert!(built[0].starts_with("wall_") && built[0].ends_with(".tga"));
        assert!(Path::new(&outcome.log_path).is_file());
    }

    /// A BSP v30 with one lump that matters: the textures, holding one 16x16
    /// embedded texture named `wall`. `world_hd.py` reads nothing else.
    pub(super) fn tiny_bsp() -> Vec<u8> {
        const LUMPS: usize = 15;
        let header = 4 + LUMPS * 8;
        let (w, h) = (16u32, 16u32);
        let pixels = (w * h) as usize;
        // miptex: name[16], w, h, 4 offsets, then 4 mips, 2-byte count, palette.
        let mut miptex = Vec::new();
        let mut name = [0u8; 16];
        name[..4].copy_from_slice(b"wall");
        miptex.extend_from_slice(&name);
        miptex.extend_from_slice(&w.to_le_bytes());
        miptex.extend_from_slice(&h.to_le_bytes());
        let mut at = 40u32;
        for mip in 0..4 {
            miptex.extend_from_slice(&at.to_le_bytes());
            at += (pixels >> (2 * mip)) as u32;
        }
        for mip in 0..4 {
            miptex.extend((0..pixels >> (2 * mip)).map(|i| (i * 7 % 251) as u8));
        }
        miptex.extend_from_slice(&256u16.to_le_bytes());
        miptex.extend((0..768).map(|i| (i % 256) as u8));
        // The textures lump: a count, one offset, the miptex.
        let mut textures = Vec::new();
        textures.extend_from_slice(&1i32.to_le_bytes());
        textures.extend_from_slice(&8i32.to_le_bytes());
        textures.extend_from_slice(&miptex);
        let entities = b"{\n\"classname\" \"worldspawn\"\n}\n\0".to_vec();

        let mut lumps = vec![(0usize, 0usize); LUMPS];
        let mut body = Vec::new();
        lumps[0] = (header + body.len(), entities.len());
        body.extend_from_slice(&entities);
        lumps[2] = (header + body.len(), textures.len());
        body.extend_from_slice(&textures);
        let mut bsp = 30i32.to_le_bytes().to_vec();
        for (offset, length) in lumps {
            let (offset, length) = if length == 0 {
                (header, 0)
            } else {
                (offset, length)
            };
            bsp.extend_from_slice(&(offset as i32).to_le_bytes());
            bsp.extend_from_slice(&(length as i32).to_le_bytes());
        }
        bsp.extend_from_slice(&body);
        bsp
    }
}
