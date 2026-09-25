//! Downloads what building HD files needs: Real-ESRGAN ncnn-vulkan (the
//! upscaler) and the extra style models from the Upscayl project. The app's
//! equivalent of `goldsrc-hooks/tools/hd/setup_tools.py`, with the same URLs
//! and the same layout, so the scripts can use this copy too (point
//! `REALESRGAN` at [`upscaler_exe`]).
//!
//! Safe to run again: anything already there is skipped. Every download goes
//! to a `.part` file first and is renamed only once complete, so a cancelled
//! or failed run never leaves a truncated model that looks installed.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

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

/// `%APPDATA%\dod-studio\hd_tools\realesrgan`.
pub fn tools_dir() -> PathBuf {
    crate::shared::paths::get_appdata_dir()
        .join("hd_tools")
        .join("realesrgan")
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
    /// The zip is unpacked and deleted rather than kept.
    unpack: bool,
}

/// What still needs fetching in `dir`, in order.
fn jobs(dir: &Path) -> Vec<Job> {
    let mut jobs = Vec::new();
    if !upscaler_exe(dir).is_file() {
        jobs.push(Job {
            url: REALESRGAN_ZIP_URL.to_string(),
            target: dir.join("realesrgan-ncnn-vulkan.zip"),
            unpack: true,
        });
    }
    for (base, model) in MODELS {
        for ext in ["param", "bin"] {
            let target = dir.join("models").join(format!("{model}.{ext}"));
            if !target.is_file() {
                jobs.push(Job {
                    url: format!("{base}{model}.{ext}"),
                    target,
                    unpack: false,
                });
            }
        }
    }
    jobs
}

/// Fetches whatever is missing into `dir`. `progress` is called often (every
/// chunk); throttling it is the caller's job.
pub fn run(
    dir: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&SetupProgress),
) -> Result<SetupOutcome, String> {
    std::fs::create_dir_all(dir.join("models"))
        .map_err(|e| crate::messages::labeled(dir.display(), e))?;

    let jobs = jobs(dir);
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
        download(&job.url, &job.target, cancel, &mut |done, total| {
            report.bytes_done = done;
            report.bytes_total = total;
            progress(&report);
        })?;
        if job.unpack {
            report.unpacking = true;
            progress(&report);
            let unpacked = unzip(&job.target, dir, cancel);
            let _ = std::fs::remove_file(&job.target);
            unpacked?;
            if !upscaler_exe(dir).is_file() {
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

/// Streams `url` to `<target>.part`, then renames it into place.
fn download(
    url: &str,
    target: &Path,
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
            done += n as u64;
            progress(done, total);
        }
        if let Some(total) = total
            && done != total
        {
            return Err(crate::messages::hd_download_incomplete(url, done, total));
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
        let dir = Scratch::new("hd_setup_jobs");
        let jobs = jobs(&dir);
        assert_eq!(jobs.len(), 1 + MODELS.len() * 2);
        assert!(jobs[0].unpack && jobs[0].url == REALESRGAN_ZIP_URL);
        assert!(jobs.iter().all(|j| j.url.starts_with("https://")));
        assert!(jobs[1..].iter().all(|j| !j.unpack));
    }

    #[test]
    fn what_is_already_there_is_skipped() {
        let dir = Scratch::new("hd_setup_skip");
        std::fs::create_dir_all(dir.join("models")).unwrap();
        std::fs::write(upscaler_exe(&dir), b"").unwrap();
        std::fs::write(dir.join("models/ultrasharp-4x.param"), b"").unwrap();
        std::fs::write(dir.join("models/ultrasharp-4x.bin"), b"").unwrap();
        // Half a model is not a model.
        std::fs::write(dir.join("models/remacri-4x.param"), b"").unwrap();

        let jobs = jobs(&dir);
        assert!(jobs.iter().all(|j| !j.unpack));
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
        let dir = Scratch::new("hd_setup_real");
        let mut steps_seen = std::collections::BTreeSet::new();
        let outcome = run(&dir, &AtomicBool::new(false), &mut |p| {
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
        let leftovers: Vec<_> = walkdir::WalkDir::new(&*dir)
            .into_iter()
            .flatten()
            .filter(|e| {
                let name = e.file_name().to_string_lossy();
                name.ends_with(".part") || name.ends_with(".zip")
            })
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        // A second run finds everything in place.
        let again = run(&dir, &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert!(again.already_there);
    }

    #[test]
    fn a_complete_folder_fetches_nothing() {
        let dir = Scratch::new("hd_setup_complete");
        std::fs::create_dir_all(dir.join("models")).unwrap();
        std::fs::write(upscaler_exe(&dir), b"").unwrap();
        for (_, model) in MODELS {
            for ext in ["param", "bin"] {
                std::fs::write(dir.join(format!("models/{model}.{ext}")), b"").unwrap();
            }
        }
        let mut calls = 0;
        let outcome = run(&dir, &AtomicBool::new(false), &mut |_| calls += 1).unwrap();
        assert!(outcome.already_there);
        assert!(outcome.fetched.is_empty());
        assert_eq!(calls, 0);
    }
}
