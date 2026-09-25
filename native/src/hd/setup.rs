//! Downloads what building HD files needs: Real-ESRGAN ncnn-vulkan (the
//! upscaler) and the extra style models from the Upscayl project -- the app's
//! equivalent of `goldsrc-hooks/tools/hd/setup_tools.py`, with the same URLs
//! and the same layout, so the scripts can use this copy too (point
//! `REALESRGAN` at [`upscaler_exe`]) -- and, when there is no usable Python
//! on the PC, the app's own copy of one (see [`super::python`]).
//!
//! Safe to run again: anything already there is skipped. Every download goes
//! to a `.part` file first and is renamed only once complete, so a cancelled
//! or failed run never leaves a truncated model that looks installed.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::python;

/// The same release `setup_tools.py` fetches. The zip has no top folder: the
/// `.exe`, its DLLs and the `realesrgan-x4plus` models unpack straight into
/// the destination.
pub const REALESRGAN_ZIP_URL: &str = "https://github.com/xinntao/Real-ESRGAN/releases/download/v0.2.5.0/realesrgan-ncnn-vulkan-20220424-windows.zip";
const UPSCAYL: &str = "https://raw.githubusercontent.com/upscayl/upscayl/main/resources/models/";
const CUSTOM: &str = "https://raw.githubusercontent.com/upscayl/custom-models/main/models/";

/// The models not in the zip: (base URL, model file stem), as `setup_tools.py`.
pub const MODELS: [(&str, &str); 4] = [
    (UPSCAYL, "ultrasharp-4x"),
    (UPSCAYL, "remacri-4x"),
    (CUSTOM, "4x_NMKD-Siax_200k"),
    (CUSTOM, "RealESRGAN_General_x4_v3"),
];

const EXE_NAME: &str = "realesrgan-ncnn-vulkan.exe";

/// Far larger than anything this fetches (the zip is about 45 MB), so a
/// server sending something else entirely cannot fill the disk.
const MAX_DOWNLOAD_BYTES: u64 = 256 * 1024 * 1024;

/// How often an external process is polled, per CLAUDE.md's process rules.
const POLL: Duration = Duration::from_millis(16);

/// `%APPDATA%\dod-studio\hd_tools`: everything the build needs that isn't
/// the scripts.
pub fn hd_tools_dir() -> PathBuf {
    crate::shared::paths::get_appdata_dir().join("hd_tools")
}

/// `%APPDATA%\dod-studio\hd_tools\realesrgan`.
pub fn tools_dir() -> PathBuf {
    realesrgan_dir(&hd_tools_dir())
}

pub fn realesrgan_dir(hd_tools: &Path) -> PathBuf {
    hd_tools.join("realesrgan")
}

pub fn upscaler_exe(tools_dir: &Path) -> PathBuf {
    tools_dir.join(EXE_NAME)
}

/// Both halves of a model (`.param` and `.bin`) are in `models\`.
pub fn model_present(tools_dir: &Path, model: &str) -> bool {
    let models = tools_dir.join("models");
    ["param", "bin"]
        .iter()
        .all(|ext| models.join(format!("{model}.{ext}")).is_file())
}

/// What a run is doing, for a progress line.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SetupProgress {
    /// The file being fetched or unpacked.
    pub item: String,
    /// 1-based position among the files this run has to fetch.
    pub step: usize,
    pub steps: usize,
    pub bytes_done: u64,
    /// `None` when the server didn't say.
    pub bytes_total: Option<u64>,
    pub unpacking: bool,
}

/// What a finished run did.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SetupOutcome {
    pub dir: String,
    pub fetched: Vec<String>,
    pub already_there: bool,
}

/// One file to fetch.
struct Job {
    url: String,
    target: PathBuf,
    /// A zip (or wheel) is unpacked here and deleted rather than kept.
    unpack_to: Option<PathBuf>,
    /// Checked before anything is unpacked or kept.
    sha256: Option<&'static str>,
    /// Unpacked into a scratch folder first and moved into place only once
    /// whole ([`unpack_staged`]): the Python files, whose presence is what
    /// says they are installed.
    staged: bool,
}

/// What still needs fetching under `hd_tools`, in order. `need_upscaler`
/// fills DoD Studio's own upscaler folder, for a PC with no complete one
/// elsewhere ([`super::upscaler`]); `need_python` adds the app's own Python,
/// for a PC with none the build can use.
fn jobs(hd_tools: &Path, need_upscaler: bool, need_python: bool) -> Vec<Job> {
    let dir = &realesrgan_dir(hd_tools);
    let mut jobs = Vec::new();
    if need_upscaler && !upscaler_exe(dir).is_file() {
        jobs.push(Job {
            url: REALESRGAN_ZIP_URL.to_string(),
            target: dir.join("realesrgan-ncnn-vulkan.zip"),
            unpack_to: Some(dir.clone()),
            sha256: None,
            staged: false,
        });
    }
    for (base, model) in MODELS {
        for ext in ["param", "bin"] {
            let target = dir.join("models").join(format!("{model}.{ext}"));
            if need_upscaler && !target.is_file() {
                jobs.push(Job {
                    url: format!("{base}{model}.{ext}"),
                    target,
                    unpack_to: None,
                    sha256: None,
                    staged: false,
                });
            }
        }
    }
    if need_python {
        if !python::app_python_exe(hd_tools).is_file() {
            jobs.push(Job {
                url: python::PYTHON_ZIP_URL.to_string(),
                target: hd_tools.join(file_name(python::PYTHON_ZIP_URL)),
                unpack_to: Some(python::app_python_dir(hd_tools)),
                sha256: Some(python::PYTHON_ZIP_SHA256),
                staged: true,
            });
        }
        for wheel in python::WHEELS {
            if !python::site_packages(hd_tools).join(wheel.module).is_dir() {
                jobs.push(Job {
                    url: wheel.url.to_string(),
                    target: hd_tools.join(file_name(wheel.url)),
                    unpack_to: Some(python::site_packages(hd_tools)),
                    sha256: Some(wheel.sha256),
                    staged: true,
                });
            }
        }
    }
    jobs
}

fn file_name(url: &str) -> &str {
    url.rsplit('/').next().unwrap_or(url)
}

/// Fetches whatever is missing under `hd_tools`. `progress` is called often
/// (every chunk); throttling it is the caller's job.
pub fn run(
    hd_tools: &Path,
    need_upscaler: bool,
    need_python: bool,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&SetupProgress),
) -> Result<SetupOutcome, String> {
    let dir = &realesrgan_dir(hd_tools);
    std::fs::create_dir_all(dir.join("models"))
        .map_err(|e| crate::messages::labeled(dir.display(), e))?;

    let jobs = jobs(hd_tools, need_upscaler, need_python);
    let steps = jobs.len();
    let mut fetched = Vec::new();
    for (index, job) in jobs.iter().enumerate() {
        let item = job
            .target
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut report = SetupProgress {
            item: item.clone(),
            step: index + 1,
            steps,
            bytes_done: 0,
            bytes_total: None,
            unpacking: false,
        };
        download(
            &job.url,
            &job.target,
            job.sha256,
            cancel,
            &mut |done, total| {
                report.bytes_done = done;
                report.bytes_total = total;
                progress(&report);
            },
        )?;
        if let Some(dest) = &job.unpack_to {
            report.unpacking = true;
            progress(&report);
            std::fs::create_dir_all(dest)
                .map_err(|e| crate::messages::labeled(dest.display(), e))?;
            let unpacked = if job.staged {
                unpack_staged(&job.target, dest, cancel)
            } else {
                unzip(&job.target, dest, cancel)
            };
            let _ = std::fs::remove_file(&job.target);
            unpacked?;
            if job.url == REALESRGAN_ZIP_URL && !upscaler_exe(dir).is_file() {
                return Err(crate::messages::hd_zip_missing_upscaler(dir.display()));
            }
        }
        fetched.push(item);
    }

    Ok(SetupOutcome {
        dir: dir.to_string_lossy().to_string(),
        already_there: fetched.is_empty(),
        fetched,
    })
}

/// Streams `url` to `<target>.part`, then renames it into place -- only if
/// its SHA-256 is `sha256`, when one is given.
fn download(
    url: &str,
    target: &Path,
    sha256: Option<&str>,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(), String> {
    let mut response = ureq::get(url)
        .call()
        .map_err(|e| crate::messages::labeled(url, e))?;
    let status = response.status();
    if !status.is_success() {
        return Err(crate::messages::url_returned_status(url, status));
    }
    let total = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    let part = target.with_extension(format!(
        "{}.part",
        target
            .extension()
            .map(|e| e.to_string_lossy())
            .unwrap_or_default()
    ));
    let mut file =
        std::fs::File::create(&part).map_err(|e| crate::messages::labeled(part.display(), e))?;
    let mut reader = response
        .body_mut()
        .with_config()
        .limit(MAX_DOWNLOAD_BYTES)
        .reader();

    let result = (|| {
        let mut chunk = vec![0u8; 64 * 1024];
        let mut done = 0u64;
        let mut hasher = Sha256::new();
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(crate::messages::HD_SETUP_CANCELLED.to_string());
            }
            let n = reader
                .read(&mut chunk)
                .map_err(|e| crate::messages::labeled(url, e))?;
            if n == 0 {
                break;
            }
            file.write_all(&chunk[..n])
                .map_err(|e| crate::messages::labeled(part.display(), e))?;
            hasher.update(&chunk[..n]);
            done += n as u64;
            progress(done, total);
        }
        if let Some(total) = total
            && done != total
        {
            return Err(crate::messages::hd_download_incomplete(url, done, total));
        }
        if let Some(want) = sha256 {
            let got: String = hasher
                .finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            if got != want {
                return Err(crate::messages::hd_download_checksum(url, &got, want));
            }
        }
        file.flush()
            .map_err(|e| crate::messages::labeled(part.display(), e))
    })();
    drop(file);

    match result {
        Ok(()) => std::fs::rename(&part, target).map_err(|e| {
            let _ = std::fs::remove_file(&part);
            crate::messages::labeled(target.display(), e)
        }),
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(e)
        }
    }
}

/// Unpacks `zip` into a scratch folder beside `dest`, then moves each of its
/// top-level entries into `dest`. A cancelled or failed unpack leaves nothing
/// in `dest`, so a half-unpacked package never looks installed; the next run
/// clears the scratch folder and starts again.
fn unpack_staged(zip: &Path, dest: &Path, cancel: &AtomicBool) -> Result<(), String> {
    let staging = dest.with_extension("unpacking");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)
        .map_err(|e| crate::messages::labeled(staging.display(), e))?;
    let result = unzip(zip, &staging, cancel).and_then(|()| {
        let entries = std::fs::read_dir(&staging)
            .map_err(|e| crate::messages::labeled(staging.display(), e))?;
        for entry in entries.flatten() {
            let to = dest.join(entry.file_name());
            if to.is_dir() {
                let _ = std::fs::remove_dir_all(&to);
            }
            std::fs::rename(entry.path(), &to)
                .map_err(|e| crate::messages::labeled(to.display(), e))?;
        }
        Ok(())
    });
    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// Unpacks `zip` into `dest` with Windows' own `tar.exe` (bsdtar, which reads
/// zips; in every Windows 10 1803+ install), so no zip library is needed.
fn unzip(zip: &Path, dest: &Path, cancel: &AtomicBool) -> Result<(), String> {
    let tar = std::env::var_os("SystemRoot")
        .map(|root| PathBuf::from(root).join("System32").join("tar.exe"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("tar.exe"));
    let mut cmd = std::process::Command::new(&tar);
    cmd.arg("-xf").arg(zip).arg("-C").arg(dest);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| crate::messages::labeled(tar.display(), e))?;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(crate::messages::HD_SETUP_CANCELLED.to_string());
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(crate::messages::hd_unzip_failed(zip.display(), status));
            }
            Ok(None) => std::thread::sleep(POLL),
            Err(e) => {
                let _ = child.kill();
                return Err(crate::messages::labeled(tar.display(), e));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    #[test]
    fn an_empty_folder_needs_the_zip_and_both_halves_of_every_model() {
        let tools = Scratch::new("hd_setup_jobs");
        // A complete upscaler elsewhere: nothing to fetch here.
        assert!(jobs(&tools, false, false).is_empty());
        let jobs = jobs(&tools, true, false);
        assert_eq!(jobs.len(), 1 + MODELS.len() * 2);
        assert!(jobs[0].unpack_to.is_some() && jobs[0].url == REALESRGAN_ZIP_URL);
        assert!(jobs.iter().all(|j| j.url.starts_with("https://")));
        assert!(jobs[1..].iter().all(|j| j.unpack_to.is_none()));
    }

    #[test]
    fn what_is_already_there_is_skipped() {
        let tools = Scratch::new("hd_setup_skip");
        let dir = realesrgan_dir(&tools);
        std::fs::create_dir_all(dir.join("models")).unwrap();
        std::fs::write(upscaler_exe(&dir), b"").unwrap();
        std::fs::write(dir.join("models/ultrasharp-4x.param"), b"").unwrap();
        std::fs::write(dir.join("models/ultrasharp-4x.bin"), b"").unwrap();
        // Half a model is not a model.
        std::fs::write(dir.join("models/remacri-4x.param"), b"").unwrap();

        let jobs = jobs(&tools, true, false);
        assert!(jobs.iter().all(|j| j.unpack_to.is_none()));
        assert_eq!(jobs.len(), (MODELS.len() - 1) * 2 - 1);
        assert!(model_present(&dir, "ultrasharp-4x"));
        assert!(!model_present(&dir, "remacri-4x"));
    }

    /// Same downloads as the script, so the two stay interchangeable.
    #[test]
    fn the_urls_match_setup_tools_py() {
        let script = include_str!("../../../goldsrc-hooks/tools/hd/setup_tools.py");
        let flat: String = script.split_whitespace().collect::<Vec<_>>().join(" ");
        // The script splits the zip URL over two string literals.
        let (head, tail) = REALESRGAN_ZIP_URL.split_at(REALESRGAN_ZIP_URL.rfind('/').unwrap() + 1);
        assert!(flat.contains(head) && flat.contains(tail));
        assert!(script.contains(UPSCAYL) && script.contains(CUSTOM));
        for (base, model) in MODELS {
            let var = if base == UPSCAYL { "UPSCAYL" } else { "CUSTOM" };
            assert!(script.contains(&format!("({var}, \"{model}\")")), "{model}");
        }
    }

    #[test]
    fn the_models_match_the_ai_styles_that_are_not_in_the_zip() {
        // x4plus ships in the Real-ESRGAN zip; every other AI style needs one
        // of these downloads.
        for style in super::super::BUILT_IN_STYLES {
            if let Some(model) = style.model
                && model != "realesrgan-x4plus"
            {
                assert!(MODELS.iter().any(|(_, m)| *m == model), "{model}");
            }
        }
    }

    /// Windows' `tar.exe` makes the zip too (`-a` picks the format from the
    /// extension), so this needs no fixture file.
    #[cfg(windows)]
    #[test]
    fn a_zip_unpacks_with_windows_own_tar() {
        let dir = Scratch::new("hd_setup_unzip");
        let src = dir.join("src");
        std::fs::create_dir_all(src.join("models")).unwrap();
        std::fs::write(src.join(EXE_NAME), b"exe").unwrap();
        std::fs::write(src.join("models/realesrgan-x4plus.bin"), b"bin").unwrap();
        let zip = dir.join("test.zip");
        let made = std::process::Command::new("tar")
            .arg("-a")
            .arg("-cf")
            .arg(&zip)
            .arg("-C")
            .arg(&src)
            .args([EXE_NAME, "models"])
            .status()
            .unwrap();
        assert!(made.success());

        let out = dir.join("out");
        std::fs::create_dir_all(&out).unwrap();
        unzip(&zip, &out, &AtomicBool::new(false)).unwrap();
        assert_eq!(std::fs::read(upscaler_exe(&out)).unwrap(), b"exe");
        assert!(out.join("models/realesrgan-x4plus.bin").is_file());
    }

    /// The real thing, against the real servers (~180 MB). Ignored by default;
    /// run with `cargo test -p native hd::setup -- --ignored`.
    #[test]
    #[ignore = "downloads ~180 MB"]
    fn downloads_and_unpacks_everything_for_real() {
        let tools = Scratch::new("hd_setup_real");
        let dir = realesrgan_dir(&tools);
        let mut steps_seen = std::collections::BTreeSet::new();
        let outcome = run(&tools, true, false, &AtomicBool::new(false), &mut |p| {
            steps_seen.insert(p.step);
        })
        .unwrap();
        assert_eq!(outcome.fetched.len(), 1 + MODELS.len() * 2);
        assert_eq!(steps_seen.len(), outcome.fetched.len());
        assert!(upscaler_exe(&dir).is_file());
        assert!(model_present(&dir, "realesrgan-x4plus"), "from the zip");
        for (_, model) in MODELS {
            assert!(model_present(&dir, model), "{model}");
        }
        // Nothing half-written left behind, and the zip is gone.
        let leftovers: Vec<_> = walkdir::WalkDir::new(&dir)
            .into_iter()
            .flatten()
            .filter(|e| {
                let name = e.file_name().to_string_lossy();
                name.ends_with(".part") || name.ends_with(".zip")
            })
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        // A second run finds everything in place.
        let again = run(&tools, true, false, &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert!(again.already_there);
    }

    #[test]
    fn a_complete_folder_fetches_nothing() {
        let tools = Scratch::new("hd_setup_complete");
        let dir = realesrgan_dir(&tools);
        std::fs::create_dir_all(dir.join("models")).unwrap();
        std::fs::write(upscaler_exe(&dir), b"").unwrap();
        for (_, model) in MODELS {
            for ext in ["param", "bin"] {
                std::fs::write(dir.join(format!("models/{model}.{ext}")), b"").unwrap();
            }
        }
        let mut calls = 0;
        let outcome = run(&tools, true, false, &AtomicBool::new(false), &mut |_| {
            calls += 1
        })
        .unwrap();
        assert!(outcome.already_there);
        assert!(outcome.fetched.is_empty());
        assert_eq!(calls, 0);
    }

    #[test]
    fn python_is_fetched_only_when_asked_and_only_what_is_missing() {
        let tools = Scratch::new("hd_setup_python_jobs");
        let without = jobs(&tools, true, false).len();
        let with = jobs(&tools, true, true);
        assert_eq!(with.len(), without + 1 + python::WHEELS.len());
        let python_jobs = &with[without..];
        assert!(python_jobs.iter().all(|j| j.staged && j.sha256.is_some()));
        assert_eq!(python_jobs[0].url, python::PYTHON_ZIP_URL);
        assert_eq!(
            python_jobs[0].unpack_to.as_deref(),
            Some(python::app_python_dir(&tools).as_path())
        );

        // Python itself and numpy already there: only Pillow and SciPy left.
        std::fs::create_dir_all(python::site_packages(&tools).join("numpy")).unwrap();
        std::fs::write(python::app_python_exe(&tools), b"").unwrap();
        let left: Vec<String> = jobs(&tools, true, true)[without..]
            .iter()
            .map(|j| file_name(&j.url).to_string())
            .collect();
        assert_eq!(left.len(), 2);
        assert!(left[0].starts_with("pillow-") && left[1].starts_with("scipy-"));
    }

    /// A staged unpack puts the archive's top-level entries into `dest` and
    /// leaves no scratch folder behind.
    #[cfg(windows)]
    #[test]
    fn a_staged_unpack_moves_everything_into_place() {
        let dir = Scratch::new("hd_setup_staged");
        let src = dir.join("src");
        std::fs::create_dir_all(src.join("numpy")).unwrap();
        std::fs::write(src.join("numpy/__init__.py"), b"").unwrap();
        std::fs::write(src.join("numpy-2.5.3.dist-info"), b"").unwrap();
        let zip = dir.join("w.zip");
        let made = std::process::Command::new("tar")
            .arg("-a")
            .arg("-cf")
            .arg(&zip)
            .arg("-C")
            .arg(&src)
            .args(["numpy", "numpy-2.5.3.dist-info"])
            .status()
            .unwrap();
        assert!(made.success());

        let dest = dir.join("site-packages");
        std::fs::create_dir_all(&dest).unwrap();
        unpack_staged(&zip, &dest, &AtomicBool::new(false)).unwrap();
        assert!(dest.join("numpy/__init__.py").is_file());
        assert!(dest.join("numpy-2.5.3.dist-info").is_file());
        assert!(!dest.with_extension("unpacking").exists());

        // Cancelled before it starts: nothing lands in dest.
        let other = dir.join("other");
        std::fs::create_dir_all(&other).unwrap();
        assert!(unpack_staged(&zip, &other, &AtomicBool::new(true)).is_err());
        assert_eq!(std::fs::read_dir(&other).unwrap().count(), 0);
        assert!(!other.with_extension("unpacking").exists());
    }

    /// The app's Python for real (~70 MB): every file matches its SHA-256,
    /// and the result imports all three packages and runs the scripts.
    /// `cargo test -p native hd::setup -- --ignored`.
    #[test]
    #[ignore = "downloads ~70 MB"]
    fn downloads_a_working_python_for_real() {
        let tools = Scratch::new("hd_setup_python_real");
        // Skip the upscaler half: pretend it is there.
        let dir = realesrgan_dir(&tools);
        std::fs::create_dir_all(dir.join("models")).unwrap();
        std::fs::write(upscaler_exe(&dir), b"").unwrap();
        for (_, model) in MODELS {
            for ext in ["param", "bin"] {
                std::fs::write(dir.join(format!("models/{model}.{ext}")), b"").unwrap();
            }
        }
        let outcome = run(&tools, true, true, &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(outcome.fetched.len(), 1 + python::WHEELS.len());
        assert!(python::app_python_complete(&tools));

        let scripts = crate::hd::build::dev_scripts_dir();
        python::write_pth(&tools, &scripts).unwrap();
        let probe = python::probe(&python::app_python_exe(&tools), &[]).unwrap();
        assert!(probe.usable(), "{probe:?}");
        let imports = std::process::Command::new(python::app_python_exe(&tools))
            .args([
                "-c",
                "import numpy, PIL.Image, scipy.ndimage, hdcommon, styles",
            ])
            .status()
            .unwrap();
        assert!(imports.success());
    }
}
