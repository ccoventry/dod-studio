//! Bakes this build's id into the DLL (`DS_HOOK_BUILD_ID`, read by
//! `src/build_id.rs`), so Studio can tell two builds at the same path apart
//! (#1143): the crate version and the git commit, and for a build with
//! uncommitted changes in the crates the DLL is built from, `dirty` and the
//! build time in UTC.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// The crates the DLL is compiled from, relative to this crate: a change in
/// any of them changes the DLL, so it reruns this script and counts as dirty.
const SOURCE_DIRS: &[&str] = &[".", "../analysis", "../dod", "../dem-patch"];

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();

    for dir in SOURCE_DIRS {
        for part in ["src", "Cargo.toml"] {
            emit_rerun(&manifest.join(dir).join(part));
        }
    }
    emit_rerun(&manifest.join("build.rs"));
    // Committing or checking out moves HEAD or the branch's ref.
    for spec in ["HEAD", "index"] {
        if let Some(path) = git(&manifest, &["rev-parse", "--git-path", spec]) {
            emit_rerun(&manifest.join(path));
        }
    }
    if let Some(head_ref) = git(&manifest, &["symbolic-ref", "-q", "HEAD"])
        && let Some(path) = git(&manifest, &["rev-parse", "--git-path", &head_ref])
    {
        emit_rerun(&manifest.join(path));
    }

    let id = match git(&manifest, &["rev-parse", "--short=9", "HEAD"]) {
        Some(commit) if is_dirty(&manifest) => {
            format!("{version}+{commit}.dirty.{}", utc_stamp())
        }
        Some(commit) => format!("{version}+{commit}"),
        None => format!("{version}+nogit.{}", utc_stamp()),
    };
    println!("cargo:rustc-env=DS_HOOK_BUILD_ID={id}");
}

/// A `rerun-if-changed` line for `path`, only when it exists: cargo reruns
/// the script on every build for a path that doesn't.
fn emit_rerun(path: &Path) {
    if path.exists() {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

/// `git <args>` run in `dir`, its trimmed output, or `None` when git is
/// missing or fails (a source tarball has no `.git`).
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    let text = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !text.is_empty()).then_some(text)
}

/// Uncommitted changes to tracked files in the crates the DLL is built from.
fn is_dirty(manifest: &Path) -> bool {
    let mut args = vec!["status", "--porcelain", "--untracked-files=no", "--"];
    args.extend(SOURCE_DIRS);
    git(manifest, &args).is_some()
}

/// Now as `YYYYMMDDTHHMMSSZ`.
fn utc_stamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}
