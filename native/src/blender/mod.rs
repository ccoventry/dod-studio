//! Rebuilding a recorded highlight in Blender (#403).
//!
//! HLAE's `mirv_agr` records every drawn model's bones and the camera on each
//! frame into an `.agr` file. `blender/`'s three scripts turn one into a
//! rendered clip: import it into a scene, texture it and build the map around
//! it, then render and encode. This module runs them from the app -- finding
//! Blender, building each script's command line, turning its output into
//! progress, and stopping it on Cancel -- the way [`crate::hd::build`] runs
//! the HD scripts.
//!
//! Every step writes into one work folder beside the `.agr`
//! (`<agr folder>\<agr name>_blender\`), under fixed names ([`WorkFiles`]), so
//! each step finds the last one's output without being told.
//!
//! Blender 4.4 is required for now: the import step uses Blender Source Tools
//! 3.4.3 and afx-blender-scripts 1.14.6, which break on 5.0 (see
//! `blender/README.md`).

use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// How often the running Blender is polled, per CLAUDE.md's process rules.
const POLL: Duration = Duration::from_millis(16);
/// How many of Blender's last lines a failure reports.
const ERROR_TAIL: usize = 12;

/// The Blender major.minor the import add-ons work with.
pub const SUPPORTED_SERIES: &str = "4.4";

/// The add-ons the import step needs, as their folders are named.
pub const ADDONS: [&str; 2] = ["io_scene_valvesource", "advancedfx"];

/// The scripts as the repo has them, for a dev build, which has no bundled
/// copy.
pub fn dev_scripts_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("blender")
}

/// The scripts to run: the app's bundled copy (`blender-scripts` in its
/// resources), else the repo's; a debug build tries the repo's first, as
/// [`crate::hd::build::scripts_dir`] does. `None` when neither has them.
pub fn scripts_dir(bundled: Option<&Path>) -> Option<PathBuf> {
    let bundled = bundled.map(Path::to_path_buf);
    let order = if cfg!(debug_assertions) {
        [Some(dev_scripts_dir()), bundled]
    } else {
        [bundled, Some(dev_scripts_dir())]
    };
    order.into_iter().flatten().find(|dir| {
        ["agr_import.py", "agr_scene.py", "agr_render.py"]
            .iter()
            .all(|s| dir.join(s).is_file())
    })
}

/// `%APPDATA%\dod-studio\blender`, where the chosen `blender.exe` is kept.
pub fn tools_dir() -> PathBuf {
    crate::shared::paths::get_appdata_dir().join("blender")
}

fn chosen_file(tools: &Path) -> PathBuf {
    tools.join("blender.txt")
}

/// The `blender.exe` the user picked, if any.
pub fn chosen(tools: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(chosen_file(tools)).ok()?;
    let line = text.lines().next()?.trim();
    (!line.is_empty()).then(|| PathBuf::from(line))
}

/// Saves the user's pick, or forgets it with `None`.
pub fn set_chosen(tools: &Path, exe: Option<&Path>) -> Result<(), String> {
    let file = chosen_file(tools);
    match exe {
        Some(exe) => {
            std::fs::create_dir_all(tools)
                .map_err(|e| crate::messages::labeled(tools.display(), e))?;
            std::fs::write(&file, format!("{}\n", exe.display()))
                .map_err(|e| crate::messages::labeled(file.display(), e))
        }
        None => match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(crate::messages::labeled(file.display(), e))
            }
            _ => Ok(()),
        },
    }
}

/// Where Blender's installer puts it: `Program Files\Blender Foundation\
/// Blender <series>\blender.exe`, newest supported series first.
fn installed_candidates() -> Vec<PathBuf> {
    let Some(program_files) = std::env::var_os("ProgramFiles") else {
        return Vec::new();
    };
    let root = PathBuf::from(program_files).join("Blender Foundation");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path().join("blender.exe"))
        .filter(|p| p.is_file())
        .collect();
    // The supported series first, then the rest by name.
    found.sort_by_key(|p| {
        let name = p
            .parent()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        (!name.contains(SUPPORTED_SERIES), name)
    });
    found
}

/// "4.4.3" from `blender --version`'s first line, "Blender 4.4.3".
pub fn parse_version(output: &str) -> Option<String> {
    let first = output
        .lines()
        .find(|l| l.trim_start().starts_with("Blender "))?;
    let version = first
        .trim()
        .strip_prefix("Blender ")?
        .split_whitespace()
        .next()?;
    version
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit())
        .then(|| version.to_string())
}

/// "4.4" from "4.4.3".
pub fn series(version: &str) -> String {
    version.split('.').take(2).collect::<Vec<_>>().join(".")
}

fn probe_version(exe: &Path) -> Option<String> {
    let mut cmd = Command::new(exe);
    cmd.arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::hd::python::no_window(&mut cmd);
    let output = cmd.output().ok()?;
    parse_version(&String::from_utf8_lossy(&output.stdout))
}

/// Whether each import add-on is installed for `series`, in Blender's own
/// per-user add-on folder.
fn addons_installed(series: &str) -> Vec<(String, bool)> {
    let base = std::env::var_os("APPDATA").map(|a| {
        PathBuf::from(a)
            .join("Blender Foundation")
            .join("Blender")
            .join(series)
            .join("scripts")
            .join("addons")
    });
    ADDONS
        .iter()
        .map(|name| {
            let present = base.as_ref().is_some_and(|b| b.join(name).is_dir());
            (name.to_string(), present)
        })
        .collect()
}

/// What the Blender page shows about the Blender it would use.
#[derive(Debug, Clone, Serialize, Default)]
pub struct BlenderStatus {
    /// The `blender.exe` a job would run, if one was found.
    pub exe: Option<String>,
    /// True when it is the user's pick rather than one found.
    pub chosen: bool,
    pub version: Option<String>,
    /// True when `version` is the series the import add-ons work with.
    pub supported: bool,
    /// Each import add-on and whether it is installed for that version.
    pub addons: Vec<(String, bool)>,
    /// The scripts' folder, if found.
    pub scripts: Option<String>,
}

/// Finds Blender: the user's pick, else an installed one (the supported
/// series first), and says what it lacks.
pub fn resolve(tools: &Path, scripts: Option<&Path>) -> BlenderStatus {
    let picked = chosen(tools).filter(|p| p.is_file());
    let exe = picked
        .clone()
        .or_else(|| installed_candidates().into_iter().next());
    let version = exe.as_deref().and_then(probe_version);
    let addons = version
        .as_deref()
        .map(|v| addons_installed(&series(v)))
        .unwrap_or_default();
    BlenderStatus {
        exe: exe.map(|p| p.to_string_lossy().to_string()),
        chosen: picked.is_some(),
        supported: version
            .as_deref()
            .is_some_and(|v| series(v) == SUPPORTED_SERIES),
        version,
        addons,
        scripts: scripts.map(|s| s.to_string_lossy().to_string()),
    }
}

/// Maps a `.agr` could have been recorded on: every `.bsp` in the game's
/// `dod\maps` and `dod_downloads\maps`, by name, sorted.
pub fn maps(game_exe: &Path) -> Vec<String> {
    let Some(game_dir) = game_exe.parent() else {
        return Vec::new();
    };
    let mut names: Vec<String> = ["dod", "dod_downloads"]
        .iter()
        .flat_map(|g| {
            std::fs::read_dir(game_dir.join(g).join("maps"))
                .into_iter()
                .flatten()
        })
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            path.extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("bsp"))
                .then(|| path.file_stem()?.to_str().map(str::to_string))
                .flatten()
        })
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// The HD styles built for models, which the scene step can use.
pub fn hd_styles(game_exe: &Path) -> Vec<String> {
    let Some(root) = crate::hd::hd_root(game_exe) else {
        return Vec::new();
    };
    let mut styles: Vec<String> = std::fs::read_dir(root.join("models"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .collect();
    styles.sort_unstable();
    styles
}

/// The work folder's fixed file names, so each step finds the last one's
/// output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkFiles {
    pub dir: PathBuf,
    pub imported: PathBuf,
    pub textured: PathBuf,
    pub preview: PathBuf,
    pub frames: PathBuf,
    pub frames_quick: PathBuf,
    pub clip: PathBuf,
    pub clip_quick: PathBuf,
}

impl WorkFiles {
    /// `<agr folder>\<agr name>_blender\` and the files in it.
    pub fn for_agr(agr: &Path) -> WorkFiles {
        let stem = agr
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "take".to_string());
        let dir = agr
            .parent()
            .unwrap_or(Path::new("."))
            .join(format!("{stem}_blender"));
        WorkFiles {
            imported: dir.join("imported.blend"),
            textured: dir.join("textured.blend"),
            preview: dir.join("preview"),
            frames: dir.join("frames"),
            frames_quick: dir.join("frames_720"),
            clip: dir.join(format!("{stem}.mp4")),
            clip_quick: dir.join(format!("{stem}_720.mp4")),
            dir,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Eevee,
    Cycles,
}

/// One step of the pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Step {
    /// `.agr` -> `imported.blend`.
    Import,
    /// `imported.blend` -> `textured.blend`, plus preview renders.
    Scene {
        engine: Engine,
        #[serde(default)]
        frames: Vec<u32>,
        #[serde(default)]
        samples: Option<u32>,
    },
    /// `textured.blend` -> a PNG sequence (720p when `quick`).
    Render {
        quick: bool,
        #[serde(default)]
        start: Option<u32>,
        #[serde(default)]
        end: Option<u32>,
        #[serde(default)]
        samples: Option<u32>,
    },
    /// The PNG sequence -> an MP4.
    Encode { quick: bool },
}

/// A job: which take, which map and style, and which step.
#[derive(Debug, Clone, Deserialize)]
pub struct JobRequest {
    pub agr: String,
    /// Crowbar's decompile folder, holding `models\`.
    pub assets: String,
    pub map: String,
    #[serde(default)]
    pub style: Option<String>,
    pub step: Step,
}

/// Map names are file stems: nothing that could leave the maps folder.
fn map_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
        && !name.contains("..")
}

/// Refuses a job that can't work, before Blender starts.
pub fn check(request: &JobRequest, files: &WorkFiles) -> Result<(), String> {
    if !Path::new(&request.agr).is_file() {
        return Err(crate::messages::blender_missing_input(&request.agr));
    }
    match &request.step {
        Step::Import => {
            if !Path::new(&request.assets).join("models").is_dir() {
                return Err(crate::messages::blender_no_assets(&request.assets));
            }
        }
        Step::Scene { .. } => {
            if !map_name_ok(&request.map) {
                return Err(crate::messages::blender_bad_map(&request.map));
            }
            if !files.imported.is_file() {
                return Err(crate::messages::BLENDER_IMPORT_FIRST.to_string());
            }
        }
        Step::Render { .. } => {
            if !files.textured.is_file() {
                return Err(crate::messages::BLENDER_SCENE_FIRST.to_string());
            }
        }
        Step::Encode { quick } => {
            let frames = if *quick {
                &files.frames_quick
            } else {
                &files.frames
            };
            if !frames.is_dir() {
                return Err(crate::messages::BLENDER_RENDER_FIRST.to_string());
            }
        }
    }
    Ok(())
}

/// Blender's command line for `request`: what it opens, which script, and
/// the script's own arguments after `--`.
pub fn arguments(
    request: &JobRequest,
    files: &WorkFiles,
    scripts: &Path,
    game_exe: &Path,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = Vec::new();
    let mut push = |a: &dyn AsRef<std::ffi::OsStr>| args.push(a.as_ref().to_os_string());
    // A script error reaches the app as an exit code, not just a log line.
    push(&"--python-exit-code");
    push(&"1");
    match &request.step {
        Step::Import => {
            // The importer needs a UI context: this one opens a window.
            push(&"--python");
            push(&scripts.join("agr_import.py"));
            push(&"--");
            push(&"--agr");
            push(&request.agr);
            push(&"--assets");
            push(&request.assets);
            push(&"--out");
            push(&files.imported);
            push(&"--quit");
        }
        Step::Scene {
            engine,
            frames,
            samples,
        } => {
            push(&"-b");
            push(&files.imported);
            push(&"--python");
            push(&scripts.join("agr_scene.py"));
            push(&"--");
            push(&"--game");
            push(&game_exe.parent().unwrap_or(Path::new(".")));
            push(&"--agr");
            push(&request.agr);
            push(&"--assets");
            push(&request.assets);
            push(&"--map");
            push(&request.map);
            push(&"--out");
            push(&files.textured);
            push(&"--preview");
            push(&files.preview);
            push(&"--log");
            push(&files.dir.join("scene_log.txt"));
            push(&"--engine");
            push(&match engine {
                Engine::Eevee => "eevee",
                Engine::Cycles => "cycles",
            });
            if let Some(style) = request.style.as_deref().filter(|s| !s.is_empty()) {
                push(&"--style");
                push(&style);
            }
            if let Some(samples) = samples {
                push(&"--samples");
                push(&samples.to_string());
            }
            if !frames.is_empty() {
                push(&"--frames");
                for f in frames {
                    push(&f.to_string());
                }
            }
        }
        Step::Render {
            quick,
            start,
            end,
            samples,
        } => {
            push(&"-b");
            push(&files.textured);
            push(&"--python");
            push(&scripts.join("agr_render.py"));
            push(&"--");
            push(&"--mode");
            push(&if *quick { "quick" } else { "render" });
            push(&"--frames-dir");
            push(&if *quick {
                &files.frames_quick
            } else {
                &files.frames
            });
            for (flag, value) in [("--start", start), ("--end", end), ("--samples", samples)] {
                if let Some(value) = value {
                    push(&flag);
                    push(&value.to_string());
                }
            }
        }
        Step::Encode { quick } => {
            push(&"-b");
            push(&"--python");
            push(&scripts.join("agr_render.py"));
            push(&"--");
            push(&"--mode");
            push(&"encode");
            push(&"--frames-dir");
            push(&if *quick {
                &files.frames_quick
            } else {
                &files.frames
            });
            push(&"--mp4");
            push(&if *quick {
                &files.clip_quick
            } else {
                &files.clip
            });
        }
    }
    args
}

/// One line of a script's output.
#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    Frame {
        frame: u32,
        done: u32,
        total: u32,
        secs: f32,
    },
    Saved(String),
    Image(String),
    Failed,
    Log(String),
}

/// Reads one line; blank ones are nothing.
pub fn parse_line(line: &str) -> Option<Line> {
    let line = line.trim_end();
    if line.trim().is_empty() {
        return None;
    }
    if let Some(rest) = line.strip_prefix("@@frame ") {
        let mut w = rest.split_whitespace();
        return Some(Line::Frame {
            frame: w.next()?.parse().ok()?,
            done: w.next()?.parse().ok()?,
            total: w.next()?.parse().ok()?,
            secs: w.next()?.parse().ok()?,
        });
    }
    if let Some(path) = line.strip_prefix("@@saved ") {
        return Some(Line::Saved(path.to_string()));
    }
    if let Some(path) = line.strip_prefix("@@image ") {
        return Some(Line::Image(path.to_string()));
    }
    if line == "@@failed" {
        return Some(Line::Failed);
    }
    Some(Line::Log(line.to_string()))
}

/// What the page shows while a step runs.
#[derive(Debug, Clone, Serialize, Default)]
pub struct Progress {
    /// The last line Blender printed.
    pub line: Option<String>,
    /// Frames rendered so far, and of how many, while rendering.
    pub done: Option<u32>,
    pub total: Option<u32>,
    /// Seconds the last frame took.
    pub frame_secs: Option<f32>,
    pub elapsed_secs: u64,
}

/// What a finished step made.
#[derive(Debug, Clone, Serialize, Default)]
pub struct Outcome {
    /// Files and folders it wrote (`@@saved`).
    pub saved: Vec<String>,
    /// Stills it rendered (`@@image`).
    pub images: Vec<String>,
    pub elapsed_secs: u64,
    pub work_dir: String,
}

/// Runs one step and waits for it. `progress` is called for each line (a
/// render prints one a frame, every few seconds, so no throttling is needed).
pub fn run(
    request: &JobRequest,
    blender: &Path,
    scripts: &Path,
    game_exe: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&Progress),
) -> Result<Outcome, String> {
    let files = WorkFiles::for_agr(Path::new(&request.agr));
    check(request, &files)?;
    std::fs::create_dir_all(&files.dir)
        .map_err(|e| crate::messages::labeled(files.dir.display(), e))?;

    let mut cmd = Command::new(blender);
    cmd.args(arguments(request, &files, scripts, game_exe))
        .current_dir(scripts)
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // The import step opens Blender's window on purpose; the rest run
    // without one, and no console window flashes up for either.
    crate::hd::python::no_window(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| crate::messages::labeled(blender.display(), e))?;
    let mut guard = crate::hd::build::TreeGuard(Some(child.id()));

    let (tx, rx) = mpsc::channel::<String>();
    let readers: Vec<_> = [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
    ]
    .into_iter()
    .flatten()
    .map(|pipe| {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(pipe).split(b'\n').map_while(Result::ok) {
                if tx
                    .send(String::from_utf8_lossy(&line).trim_end().to_string())
                    .is_err()
                {
                    break;
                }
            }
        })
    })
    .collect();
    drop(tx);

    let started = Instant::now();
    let mut state = Progress::default();
    let mut outcome = Outcome {
        work_dir: files.dir.to_string_lossy().to_string(),
        ..Outcome::default()
    };
    let mut tail: Vec<String> = Vec::new();
    let mut failed = false;
    let mut handle = |text: String, state: &mut Progress, outcome: &mut Outcome| {
        let Some(line) = parse_line(&text) else {
            return;
        };
        match line {
            Line::Frame {
                done, total, secs, ..
            } => {
                state.done = Some(done);
                state.total = Some(total);
                state.frame_secs = Some(secs);
            }
            Line::Saved(path) => outcome.saved.push(path),
            Line::Image(path) => outcome.images.push(path),
            Line::Failed => failed = true,
            Line::Log(text) => {
                tail.push(text.clone());
                if tail.len() > ERROR_TAIL {
                    tail.remove(0);
                }
                state.line = Some(text);
            }
        }
        state.elapsed_secs = started.elapsed().as_secs();
        progress(state);
    };

    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            guard.kill();
            let _ = child.wait();
            return Err(crate::messages::BLENDER_CANCELLED.to_string());
        }
        while let Ok(text) = rx.try_recv() {
            handle(text, &mut state, &mut outcome);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(POLL),
            Err(e) => return Err(crate::messages::labeled(blender.display(), e)),
        }
    };
    guard.0 = None; // exited by itself: nothing left to end
    for reader in readers {
        let _ = reader.join();
    }
    while let Ok(text) = rx.try_recv() {
        handle(text, &mut state, &mut outcome);
    }

    if failed || !status.success() {
        return Err(crate::messages::blender_step_failed(
            status,
            &tail.join("\n"),
        ));
    }
    outcome.elapsed_secs = started.elapsed().as_secs();
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(step: Step) -> JobRequest {
        JobRequest {
            agr: r"C:\takes\anzio streak.agr".to_string(),
            assets: r"C:\assets".to_string(),
            map: "dod_anzio".to_string(),
            style: Some("ultrasharp".to_string()),
            step,
        }
    }

    fn args(step: Step) -> Vec<String> {
        let r = request(step);
        let files = WorkFiles::for_agr(Path::new(&r.agr));
        arguments(
            &r,
            &files,
            Path::new(r"C:\scripts"),
            Path::new(r"C:\Half-Life\hl.exe"),
        )
        .into_iter()
        .map(|a| a.to_string_lossy().to_string())
        .collect()
    }

    #[test]
    fn the_work_folder_sits_beside_the_take() {
        let f = WorkFiles::for_agr(Path::new(r"C:\takes\anzio streak.agr"));
        assert_eq!(f.dir, PathBuf::from(r"C:\takes\anzio streak_blender"));
        assert_eq!(f.imported, f.dir.join("imported.blend"));
        assert_eq!(f.clip, f.dir.join("anzio streak.mp4"));
        assert_eq!(f.clip_quick, f.dir.join("anzio streak_720.mp4"));
    }

    #[test]
    fn import_runs_with_a_window_and_quits() {
        let a = args(Step::Import);
        assert!(
            !a.contains(&"-b".to_string()),
            "the importer needs a UI context: {a:?}"
        );
        assert_eq!(&a[..2], ["--python-exit-code", "1"]);
        assert!(a.iter().any(|x| x.ends_with("agr_import.py")));
        let after: Vec<&String> = a.iter().skip_while(|x| *x != "--").collect();
        assert!(after.contains(&&"--quit".to_string()));
        assert!(after.contains(&&r"C:\takes\anzio streak.agr".to_string()));
    }

    #[test]
    fn the_scene_step_opens_the_imported_file_in_the_background() {
        let a = args(Step::Scene {
            engine: Engine::Cycles,
            frames: vec![180, 977],
            samples: Some(64),
        });
        let dash = a.iter().position(|x| x == "--").unwrap();
        let before = &a[..dash];
        assert!(before.contains(&"-b".to_string()));
        assert!(
            before[before.iter().position(|x| x == "-b").unwrap() + 1].ends_with("imported.blend")
        );
        let after = &a[dash + 1..];
        let value = |flag: &str| after[after.iter().position(|x| x == flag).unwrap() + 1].clone();
        assert_eq!(value("--game"), r"C:\Half-Life");
        assert_eq!(value("--map"), "dod_anzio");
        assert_eq!(value("--engine"), "cycles");
        assert_eq!(value("--style"), "ultrasharp");
        assert_eq!(value("--samples"), "64");
        assert!(after.ends_with(&["--frames".to_string(), "180".to_string(), "977".to_string()]));
    }

    #[test]
    fn render_and_encode_use_the_quick_folder_when_quick() {
        let r = args(Step::Render {
            quick: true,
            start: Some(10),
            end: None,
            samples: None,
        });
        assert!(r.iter().any(|x| x.ends_with("frames_720")));
        assert!(r.contains(&"quick".to_string()));
        assert!(r.contains(&"--start".to_string()) && !r.contains(&"--end".to_string()));
        let e = args(Step::Encode { quick: false });
        assert!(e.iter().any(|x| x.ends_with(r"\frames")));
        assert!(e.iter().any(|x| x.ends_with("anzio streak.mp4")));
    }

    #[test]
    fn script_lines_parse() {
        assert_eq!(
            parse_line("@@frame 181 2 4 1.9"),
            Some(Line::Frame {
                frame: 181,
                done: 2,
                total: 4,
                secs: 1.9
            })
        );
        assert_eq!(
            parse_line(r"@@saved C:\x\textured.blend"),
            Some(Line::Saved(r"C:\x\textured.blend".to_string()))
        );
        assert_eq!(parse_line("@@failed"), Some(Line::Failed));
        assert_eq!(
            parse_line("skybox morningdew_: 6/6 faces"),
            Some(Line::Log("skybox morningdew_: 6/6 faces".to_string()))
        );
        assert_eq!(parse_line("   "), None);
    }

    #[test]
    fn blender_versions_parse() {
        assert_eq!(
            parse_version("Blender 4.4.3\n\tbuild date: 2025-04-29\n"),
            Some("4.4.3".to_string())
        );
        assert_eq!(series("4.4.3"), "4.4");
        assert_eq!(
            parse_version("Color management: ...\nBlender 5.0.0 Alpha\n"),
            Some("5.0.0".to_string())
        );
        assert_eq!(parse_version("nothing here"), None);
    }

    #[test]
    fn map_names_stay_in_the_maps_folder() {
        assert!(map_name_ok("dod_anzio"));
        assert!(map_name_ok("dod_railroad2_s9a"));
        assert!(!map_name_ok(""));
        assert!(!map_name_ok("../../x"));
        assert!(!map_name_ok(r"maps\dod_anzio"));
    }

    #[test]
    fn a_step_needs_the_last_one_done() {
        let scratch = crate::test_support::Scratch::new("blender_check");
        let agr = scratch.path().join("take.agr");
        std::fs::write(&agr, b"AGR").unwrap();
        let mut r = request(Step::Render {
            quick: true,
            start: None,
            end: None,
            samples: None,
        });
        r.agr = agr.to_string_lossy().to_string();
        let files = WorkFiles::for_agr(&agr);
        assert_eq!(
            check(&r, &files),
            Err(crate::messages::BLENDER_SCENE_FIRST.to_string())
        );
        std::fs::create_dir_all(&files.dir).unwrap();
        std::fs::write(&files.textured, b"").unwrap();
        assert_eq!(check(&r, &files), Ok(()));
    }

    #[test]
    fn the_dev_build_finds_the_repo_scripts() {
        assert!(
            scripts_dir(None).is_some(),
            "blender/ should hold all three scripts"
        );
    }
}
