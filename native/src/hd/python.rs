//! Which Python runs the HD build scripts (#372 part 2).
//!
//! The build is `goldsrc-hooks/tools/hd/`'s Python scripts, run as they are,
//! so the app and the command line always make the same files. They need
//! Python 3.10+ with numpy, Pillow and SciPy. In order, the page uses:
//!
//! 1. **The one the user chose** on the HD page ([`chosen_file`]), if it
//!    still works.
//! 2. **The app's own copy**, if [`super::setup`] has downloaded it: the
//!    official embeddable Python plus the three packages' wheels, unpacked
//!    into `%APPDATA%\dod-studio\hd_tools\python`. Nothing is installed
//!    system-wide, and the user's own Python is never touched.
//! 3. **One already on this PC**: the `py` launcher's default, then
//!    `python` on the PATH, as long as it has all three packages.
//!
//! Only when none of them works does the setup download the app's copy.
//! A found or chosen Python that lacks a package is reported with what is
//! missing, rather than having packages installed into it: that is the
//! user's environment, not the app's.
//!
//! Every download is pinned to one file and its SHA-256, so a changed or
//! tampered file fails the setup instead of being run.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

/// The embeddable build of the last 3.12 release with Windows binaries.
pub const PYTHON_ZIP_URL: &str =
    "https://www.python.org/ftp/python/3.12.10/python-3.12.10-embed-amd64.zip";
pub const PYTHON_ZIP_SHA256: &str =
    "4acbed6dd1c744b0376e3b1cf57ce906f9dc9e95e68824584c8099a63025a3c3";

/// The `._pth` file of that build: the embeddable Python's whole search path.
const PTH_NAME: &str = "python312._pth";

/// A package the scripts import, as the wheel that provides it.
#[derive(Debug, Clone, Copy)]
pub struct Wheel {
    /// What the scripts `import`, which is also its folder in
    /// `site-packages`.
    pub module: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
}

/// cp312 / win_amd64 wheels, the versions the scripts were last checked with.
pub const WHEELS: [Wheel; 3] = [
    Wheel {
        module: "numpy",
        url: "https://files.pythonhosted.org/packages/3c/a1/accf6d4f0c80c5d9ba9735d6b1550e444180599f34dec69ca01360f717ad/numpy-2.5.3-cp312-cp312-win_amd64.whl",
        sha256: "0a59a421a32580a009e8a1751345bf829631b990dc1794b80514ab722b435def",
    },
    Wheel {
        module: "PIL",
        url: "https://files.pythonhosted.org/packages/45/89/da2f7971a317f83d807fdd4065c0af40208e59e692cc43d315a71a0e96d1/pillow-12.3.0-cp312-cp312-win_amd64.whl",
        sha256: "a2b55dd6b2a4c4b7d87ffa56bdb33fdc5fdb9a462173861a7bc097f17d91cb09",
    },
    Wheel {
        module: "scipy",
        url: "https://files.pythonhosted.org/packages/39/e7/979fd14e75008623df31ba70d6bb144700f68feadcea042021c06a05bf82/scipy-1.18.1-cp312-cp312-win_amd64.whl",
        sha256: "5e4d44984abc0020154ea81b247adeddcc3ac5527b975ff798bd1ba0adc513c2",
    },
];

/// The oldest Python the scripts run on.
pub const MIN_VERSION: (u32, u32) = (3, 10);

/// How long one probe may take before it counts as not working. A first
/// `import scipy` from a cold disk can take a few seconds.
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

/// How often a running process is polled, per CLAUDE.md's process rules.
const POLL: Duration = Duration::from_millis(16);

/// Prints the interpreter's own path, its version, and the modules it lacks,
/// one per line, without importing them (`find_spec` only looks).
const PROBE: &str = "import sys, importlib.util\n\
print(sys.executable)\n\
print('%d.%d.%d' % sys.version_info[:3])\n\
print(','.join(m for m in ('numpy', 'PIL', 'scipy') if importlib.util.find_spec(m) is None))";

/// `%APPDATA%\dod-studio\hd_tools\python`.
pub fn app_python_dir(hd_tools: &Path) -> PathBuf {
    hd_tools.join("python")
}

pub fn app_python_exe(hd_tools: &Path) -> PathBuf {
    app_python_dir(hd_tools).join("python.exe")
}

pub fn site_packages(hd_tools: &Path) -> PathBuf {
    app_python_dir(hd_tools).join("Lib").join("site-packages")
}

/// Whether the app's copy has Python itself and every package.
pub fn app_python_complete(hd_tools: &Path) -> bool {
    app_python_exe(hd_tools).is_file()
        && WHEELS
            .iter()
            .all(|w| site_packages(hd_tools).join(w.module).is_dir())
}

/// Where the user's pick is kept: one line, the `python.exe` path.
pub fn chosen_file(hd_tools: &Path) -> PathBuf {
    hd_tools.join("python.txt")
}

pub fn chosen(hd_tools: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(chosen_file(hd_tools)).ok()?;
    let line = text.lines().next()?.trim();
    (!line.is_empty()).then(|| PathBuf::from(line))
}

/// Saves the user's pick, or forgets it with `None`.
pub fn set_chosen(hd_tools: &Path, exe: Option<&Path>) -> Result<(), String> {
    let file = chosen_file(hd_tools);
    match exe {
        Some(exe) => {
            std::fs::create_dir_all(hd_tools)
                .map_err(|e| crate::messages::labeled(hd_tools.display(), e))?;
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

/// Where a Python came from, for the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PythonSource {
    Chosen,
    App,
    Found,
}

/// A Python that answered the probe.
#[derive(Debug, Clone, Serialize)]
pub struct Probe {
    pub exe: String,
    pub version: String,
    /// The modules it doesn't have, of `numpy`, `PIL` and `scipy`.
    pub missing: Vec<String>,
}

impl Probe {
    /// New enough and has every package.
    pub fn usable(&self) -> bool {
        self.missing.is_empty() && version_ok(&self.version)
    }
}

fn version_ok(version: &str) -> bool {
    let mut parts = version.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let major = parts.next().unwrap_or(0);
    let minor = parts.next().unwrap_or(0);
    (major, minor) >= MIN_VERSION
}

/// The Python a build would use, and where it came from.
#[derive(Debug, Clone, Serialize)]
pub struct Using {
    pub source: PythonSource,
    #[serde(flatten)]
    pub probe: Probe,
}

/// Everything the page shows about Python.
#[derive(Debug, Clone, Serialize)]
pub struct PythonStatus {
    pub using: Option<Using>,
    /// The user's pick, whether or not it works.
    pub chosen: Option<String>,
    /// What is wrong with the pick, when it doesn't work.
    pub chosen_problem: Option<Probe>,
    /// What was found on this PC but can't be used, so the page can say why
    /// (e.g. "Python 3.13 is installed but has no SciPy").
    pub found_unusable: Option<Probe>,
    pub app_copy_present: bool,
}

/// Runs the probe with `exe` (plus `args` in front of it, for `py -3`).
/// `None` when it can't be started, times out, or prints something else.
pub fn probe(exe: &Path, args: &[&str]) -> Option<Probe> {
    let mut cmd = Command::new(exe);
    cmd.args(args)
        // Not `-I`: the build runs without it, so the probe must see the same
        // packages (a `pip install --user` numpy, say).
        .args(["-c", PROBE])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    no_window(&mut cmd);
    let mut child = cmd.spawn().ok()?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < PROBE_TIMEOUT => std::thread::sleep(POLL),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    if !status.success() {
        return None;
    }
    parse_probe(&out)
}

fn parse_probe(out: &str) -> Option<Probe> {
    let mut lines = out.lines().map(str::trim);
    let exe = lines.next().filter(|l| !l.is_empty())?.to_string();
    let version = lines.next().filter(|l| l.contains('.'))?.to_string();
    let missing = lines
        .next()
        .unwrap_or("")
        .split(',')
        .filter(|m| !m.is_empty())
        .map(str::to_string)
        .collect();
    Some(Probe {
        exe,
        version,
        missing,
    })
}

/// The Pythons already on this PC, best first: the `py` launcher's default,
/// then `python` on the PATH. Microsoft Store's `python.exe` stub (which
/// opens the Store instead of running anything) is skipped.
fn found_candidates() -> Vec<(PathBuf, Vec<&'static str>)> {
    let mut found = Vec::new();
    if let Some(windir) = std::env::var_os("SystemRoot") {
        let py = PathBuf::from(windir).join("py.exe");
        if py.is_file() {
            found.push((py, vec!["-3"]));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let exe = dir.join("python.exe");
            let stub = dir
                .to_string_lossy()
                .to_ascii_lowercase()
                .contains("\\windowsapps");
            if exe.is_file() && !stub {
                found.push((exe, Vec::new()));
            }
        }
    }
    found
}

/// Works out which Python a build would use (see the module doc).
pub fn resolve(hd_tools: &Path) -> PythonStatus {
    let chosen_path = chosen(hd_tools);
    let mut status = PythonStatus {
        using: None,
        chosen: chosen_path.as_ref().map(|p| p.display().to_string()),
        chosen_problem: None,
        found_unusable: None,
        app_copy_present: app_python_complete(hd_tools),
    };

    if let Some(path) = &chosen_path {
        match probe(path, &[]) {
            Some(p) if p.usable() => {
                status.using = Some(Using {
                    source: PythonSource::Chosen,
                    probe: p,
                });
                return status;
            }
            other => {
                status.chosen_problem = Some(other.unwrap_or_else(|| Probe {
                    exe: path.display().to_string(),
                    version: String::new(),
                    missing: Vec::new(),
                }))
            }
        }
    }

    if status.app_copy_present
        && let Some(p) = probe(&app_python_exe(hd_tools), &[])
        && p.usable()
    {
        status.using = Some(Using {
            source: PythonSource::App,
            probe: p,
        });
        return status;
    }

    for (exe, args) in found_candidates() {
        if let Some(p) = probe(&exe, &args) {
            if p.usable() {
                status.using = Some(Using {
                    source: PythonSource::Found,
                    probe: p,
                });
                return status;
            }
            status.found_unusable.get_or_insert(p);
        }
    }
    status
}

/// Points the app's copy at `scripts`, so the scripts can import each other.
///
/// The embeddable Python reads its whole search path from the `._pth` file
/// and ignores `PYTHONPATH`, the script's own folder included, which is what
/// keeps it isolated from any other Python on the PC. Rewritten before every
/// build, since the scripts' folder is the app's (or, in a dev build, the
/// repo's) and can move.
pub fn write_pth(hd_tools: &Path, scripts: &Path) -> Result<(), String> {
    let file = app_python_dir(hd_tools).join(PTH_NAME);
    let text = format!(
        "python312.zip\r\n.\r\nLib\\site-packages\r\n{}\r\n",
        scripts.display()
    );
    std::fs::write(&file, text).map_err(|e| crate::messages::labeled(file.display(), e))
}

pub(crate) fn no_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    #[test]
    fn a_probe_line_set_parses() {
        let p = parse_probe("C:\\Python312\\python.exe\r\n3.12.4\r\nscipy\r\n").unwrap();
        assert_eq!(p.exe, "C:\\Python312\\python.exe");
        assert_eq!(p.version, "3.12.4");
        assert_eq!(p.missing, vec!["scipy"]);
        assert!(!p.usable());

        let p = parse_probe("C:\\p\\python.exe\n3.14.7\n\n").unwrap();
        assert!(p.missing.is_empty() && p.usable());

        assert!(parse_probe("").is_none());
        assert!(parse_probe("C:\\p\\python.exe\nnot a version\n").is_none());
    }

    #[test]
    fn too_old_a_python_is_not_usable() {
        assert!(!version_ok("3.9.18"));
        assert!(version_ok("3.10.0"));
        assert!(version_ok("3.14.7"));
        assert!(!version_ok("2.7.18"));
    }

    #[test]
    fn the_pick_is_saved_and_forgotten() {
        let dir = Scratch::new("hd_python_pick");
        assert_eq!(chosen(&dir), None);
        set_chosen(&dir, Some(Path::new("D:\\Py\\python.exe"))).unwrap();
        assert_eq!(chosen(&dir), Some(PathBuf::from("D:\\Py\\python.exe")));
        set_chosen(&dir, None).unwrap();
        assert_eq!(chosen(&dir), None);
        // Forgetting twice is fine.
        set_chosen(&dir, None).unwrap();
    }

    #[test]
    fn a_pick_that_does_not_run_is_reported_not_used() {
        let dir = Scratch::new("hd_python_bad_pick");
        set_chosen(&dir, Some(&dir.join("no_such_python.exe"))).unwrap();
        let status = resolve(&dir);
        assert!(status.chosen.is_some());
        let problem = status.chosen_problem.expect("reported");
        assert!(problem.version.is_empty(), "it never answered");
        assert_ne!(status.using.map(|u| u.source), Some(PythonSource::Chosen));
    }

    #[test]
    fn the_app_copy_needs_python_and_every_package() {
        let dir = Scratch::new("hd_python_app");
        assert!(!app_python_complete(&dir));
        std::fs::create_dir_all(app_python_dir(&dir)).unwrap();
        std::fs::write(app_python_exe(&dir), b"").unwrap();
        for wheel in &WHEELS[..2] {
            std::fs::create_dir_all(site_packages(&dir).join(wheel.module)).unwrap();
        }
        assert!(!app_python_complete(&dir), "scipy is missing");
        std::fs::create_dir_all(site_packages(&dir).join(WHEELS[2].module)).unwrap();
        assert!(app_python_complete(&dir));
    }

    #[test]
    fn the_pth_file_puts_the_scripts_on_the_path() {
        let dir = Scratch::new("hd_python_pth");
        std::fs::create_dir_all(app_python_dir(&dir)).unwrap();
        write_pth(&dir, Path::new("C:\\Studio\\hd-scripts")).unwrap();
        let text = std::fs::read_to_string(app_python_dir(&dir).join(PTH_NAME)).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines,
            [
                "python312.zip",
                ".",
                "Lib\\site-packages",
                "C:\\Studio\\hd-scripts"
            ]
        );
        assert!(!text.contains("import site"), "stays isolated");
    }

    /// The modules checked here are the ones the scripts import.
    #[test]
    fn the_wheels_cover_what_the_scripts_import() {
        let requirements = include_str!("../../../goldsrc-hooks/tools/hd/requirements.txt");
        for (requirement, module) in [("pillow", "PIL"), ("numpy", "numpy"), ("scipy", "scipy")] {
            assert!(requirements.contains(requirement), "{requirement}");
            assert!(WHEELS.iter().any(|w| w.module == module), "{module}");
            assert!(PROBE.contains(&format!("'{module}'")), "{module}");
        }
        for wheel in WHEELS {
            assert!(
                wheel.url.contains("-cp312-cp312-win_amd64.whl"),
                "{}",
                wheel.url
            );
            assert_eq!(wheel.sha256.len(), 64);
        }
        assert!(PYTHON_ZIP_URL.contains("3.12.") && PTH_NAME == "python312._pth");
    }
}
