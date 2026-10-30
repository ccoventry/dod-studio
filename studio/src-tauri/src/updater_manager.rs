use crate::download_progress::DownloadMeter;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::{Update, UpdaterExt};

/// Fixed manifest URLs — one GitHub Release per channel. `stable` is
/// published to the repo's `/releases/latest` alias (release_stable.yml,
/// cut from `main`); `experimental` is republished in place under a fixed
/// `experimental-latest` prerelease tag (release_experimental.yml, cut from
/// `dev` on demand).
/// See issue #133 / docs/archive is not relevant here — this is new.
const STABLE_ENDPOINT: &str =
    "https://github.com/ccoventry/dod-studio/releases/latest/download/latest.json";
const EXPERIMENTAL_ENDPOINT: &str =
    "https://github.com/ccoventry/dod-studio/releases/download/experimental-latest/latest.json";

fn endpoint_for_channel(channel: &str) -> Result<url::Url, String> {
    let raw = match channel {
        "experimental" => EXPERIMENTAL_ENDPOINT,
        "stable" => STABLE_ENDPOINT,
        other => return Err(crate::messages::unknown_update_channel(other)),
    };
    url::Url::parse(raw).map_err(crate::messages::invalid_updater_endpoint_url)
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
    pub pub_date: Option<String>,
}

/// Holds the `Update` handle a successful check produced, since it carries a
/// live download/signature-verify closure that can't cross the IPC boundary
/// — `download_and_install_update` reads it back out by channel.
#[derive(Default)]
pub struct UpdaterState {
    pub pending: Arc<Mutex<Option<Update>>>,
}

#[tauri::command]
pub async fn check_for_update(
    app: AppHandle,
    state: tauri::State<'_, UpdaterState>,
    channel: String,
) -> Result<Option<UpdateInfo>, String> {
    let endpoint = endpoint_for_channel(&channel)?;
    let updater = app
        .updater_builder()
        .endpoints(vec![endpoint])
        .map_err(crate::messages::failed_to_set_updater_endpoint)?
        // Default semver comparison only offers upgrades, but a channel is a
        // deliberate choice, not a version target — switching from
        // experimental back to stable is a legitimate "downgrade"
        // (experimental's version number is always ahead) that should still
        // be offered, not silently blocked.
        .version_comparator(|current, remote| remote.version != current)
        .build()
        .map_err(crate::messages::failed_to_build_updater)?;

    let update = updater.check().await.map_err(|e| {
        native::log_markdown(&format!("[update] check on {channel} failed: {e}"));
        e.to_string()
    })?;
    native::log_markdown(&check_log(
        &channel,
        update
            .as_ref()
            .map(|u| (u.current_version.as_str(), u.version.as_str())),
    ));

    match update {
        Some(update) => {
            let info = UpdateInfo {
                version: update.version.clone(),
                current_version: update.current_version.clone(),
                notes: update.body.clone(),
                pub_date: update.date.map(|d| d.to_string()),
            };
            let pending = Arc::clone(&state.pending);
            let mut guard = pending.lock().unwrap_or_else(|p| p.into_inner());
            *guard = Some(update);
            Ok(Some(info))
        }
        None => {
            let pending = Arc::clone(&state.pending);
            let mut guard = pending.lock().unwrap_or_else(|p| p.into_inner());
            *guard = None;
            Ok(None)
        }
    }
}

/// The activity log's line for a finished update check: the versions when
/// one was found.
fn check_log(channel: &str, found: Option<(&str, &str)>) -> String {
    match found {
        Some((current, new)) => {
            format!("[update] {channel} channel: {new} is available (running {current})")
        }
        None => format!("[update] {channel} channel: up to date"),
    }
}

/// The update a check found, left in place: a failed download re-enables
/// Download & Install (`updater_pane.js`), and a retry needs it still there.
/// Taking it out made every retry fail with "no update available".
fn pending_update<T: Clone>(slot: &Mutex<Option<T>>) -> Option<T> {
    slot.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

#[tauri::command]
pub async fn download_and_install_update(
    app: AppHandle,
    state: tauri::State<'_, UpdaterState>,
) -> Result<(), String> {
    // A debug build is a `tauri dev` session or a `--debug` bundle made from
    // the repo. Installing from it would quit it and replace the *installed*
    // app, so refuse; the frontend never offers it either.
    if cfg!(debug_assertions) {
        return Err(crate::messages::LOCAL_BUILD_CANNOT_INSTALL_UPDATE.to_string());
    }
    let update =
        pending_update(&state.pending).ok_or(crate::messages::NO_UPDATE_AVAILABLE_TO_INSTALL)?;

    // Download, then install, rather than `download_and_install`: its
    // download-finished callback fires before the signature check and the
    // install, too early for "Installing" or "Ready" (#957).
    let mut meter = DownloadMeter::default();
    native::log_markdown(&format!("[update] downloading {}", update.version));
    let bytes = update
        .download(
            |chunk_length, content_length| {
                if let Some(p) = meter.chunk(chunk_length as u64, content_length, Instant::now()) {
                    let _ = app.emit("update_download_progress", p);
                }
            },
            || {},
        )
        .await
        .map_err(|e| {
            native::log_markdown(&format!("[update] download failed: {e}"));
            e.to_string()
        })?;
    // The throttle may have held back the last chunk: the bar ends full.
    let _ = app.emit("update_download_progress", meter.progress());
    let _ = app.emit("update_installing", ());
    // On Windows this starts the installer and quits Studio, so nothing after
    // it runs there.
    native::log_markdown("[update] downloaded, installing");
    update.install(bytes).map_err(|e| {
        native::log_markdown(&format!("[update] install failed: {e}"));
        e.to_string()
    })?;
    let _ = app.emit("update_ready", ());

    Ok(())
}

/// Earlier versions' release notes for the What's new window (#1013):
/// GitHub's release list, saved in the app's data folder, or that saved copy
/// when GitHub can't be read. `offline` reads only the saved copy, for a
/// Studio whose update checks are off. Off the async runtime: it waits on the
/// network.
#[tauri::command]
pub async fn release_history(
    offline: bool,
) -> Result<native::shared::release_history::History, String> {
    use native::shared::release_history as history;
    crate::messages::spawn_blocking_result(tokio::task::spawn_blocking(move || {
        let dir = native::shared::paths::get_appdata_dir();
        if offline {
            history::History {
                releases: history::read_cache(&dir),
                from_cache: true,
                error: None,
            }
        } else {
            history::fetch_or_cached(history::RELEASES_URL, &dir)
        }
    }))
    .await
}

/// `cfg!(debug_assertions)` is compile-time info baked into the binary — the
/// frontend has no other way to tell a `tauri build --debug` bundle (still a
/// real installed build, unlike `npm run tauri dev`) apart from a genuine
/// `--release` build, since both go through the same production Vite build
/// and report `import.meta.env.DEV === false`.
#[tauri::command]
pub fn is_debug_build() -> bool {
    cfg!(debug_assertions)
}

/// The git branch of the source tree this binary was built from, for the
/// window title of a build made on this PC (e.g. `local build -
/// test/capture-batch`). Read at call time, so it's the branch checked out
/// when the app launched.
///
/// The path comes from `CARGO_MANIFEST_DIR`, baked in at compile time. On
/// this PC that's the repo; for a CI-built installer it's a CI runner path
/// that doesn't exist on the user's machine, so this returns `None` and the
/// title is unchanged. Plain file reads, no `git` process.
#[tauri::command(async)]
pub fn local_git_branch() -> Option<String> {
    git_branch_from(std::path::Path::new(env!("CARGO_MANIFEST_DIR")))
}

/// Whether this build uses Configuration's GoldSrc Hooks DLL override: a
/// debug build, or one made on this PC (its source tree is readable, as for
/// [`local_git_branch`]). An installed release always runs the hook DLL it
/// ships with, so an override saved while testing can't apply out of sight
/// (the row is hidden there, `hook_dll_row.js`).
#[tauri::command(async)]
pub fn hook_dll_override_allowed() -> bool {
    cfg!(debug_assertions) || local_git_branch().is_some()
}

/// `path` when this build uses the override, else `None` (the bundled DLL).
pub(crate) fn allowed_hook_dll(path: Option<String>) -> Option<String> {
    path.filter(|_| hook_dll_override_allowed())
}

/// Walks up from `start` to the nearest `.git` and reads its `HEAD`.
/// Handles a worktree, where `.git` is a file holding `gitdir: <path>`.
/// A detached HEAD gives the short commit id instead of a branch.
fn git_branch_from(start: &std::path::Path) -> Option<String> {
    let dot_git = start
        .ancestors()
        .map(|d| d.join(".git"))
        .find(|p| p.exists())?;
    let git_dir = if dot_git.is_dir() {
        dot_git
    } else {
        let text = std::fs::read_to_string(&dot_git).ok()?;
        let target = text.trim().strip_prefix("gitdir:")?.trim();
        let target = std::path::PathBuf::from(target);
        if target.is_absolute() {
            target
        } else {
            dot_git.parent()?.join(target)
        }
    };
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    if let Some(branch) = head.strip_prefix("ref: refs/heads/") {
        return (!branch.is_empty()).then(|| branch.to_string());
    }
    let is_commit = head.len() >= 7 && head.bytes().all(|b| b.is_ascii_hexdigit());
    is_commit.then(|| head[..7].to_string())
}

#[cfg(test)]
mod git_branch_tests {
    use super::git_branch_from;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dodstudio_git_branch_{name}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_the_branch_from_a_normal_repo_above_the_start_folder() {
        let root = scratch("normal");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(
            root.join(".git/HEAD"),
            "ref: refs/heads/test/capture-batch\n",
        )
        .unwrap();
        let deep = root.join("studio/src-tauri");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(
            git_branch_from(&deep).as_deref(),
            Some("test/capture-batch")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn follows_a_worktree_gitdir_file() {
        let root = scratch("worktree");
        let gitdir = root.join("main/.git/worktrees/wt");
        std::fs::create_dir_all(&gitdir).unwrap();
        std::fs::write(
            gitdir.join("HEAD"),
            "ref: refs/heads/feat/title-branch-name\n",
        )
        .unwrap();
        let wt = root.join("wt");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
        assert_eq!(
            git_branch_from(&wt).as_deref(),
            Some("feat/title-branch-name")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_detached_head_gives_the_short_commit() {
        let root = scratch("detached");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(
            root.join(".git/HEAD"),
            "39230c9c0123456789abcdef0123456789abcdef\n",
        )
        .unwrap();
        assert_eq!(git_branch_from(&root).as_deref(), Some("39230c9"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn no_repo_gives_none() {
        let root = scratch("none");
        // temp_dir() itself is not inside a repo on any machine this runs on.
        assert_eq!(git_branch_from(&root.join("nowhere")), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[tauri::command]
pub fn restart_app(app: AppHandle) {
    app.restart();
}

#[cfg(test)]
mod check_log_tests {
    use super::check_log;

    #[test]
    fn the_check_line_names_the_channel_and_the_versions() {
        assert_eq!(
            check_log("stable", Some(("1.0.0", "1.1.0"))),
            "[update] stable channel: 1.1.0 is available (running 1.0.0)"
        );
        assert_eq!(
            check_log("experimental", None),
            "[update] experimental channel: up to date"
        );
    }
}

#[cfg(test)]
mod pending_update_tests {
    use super::pending_update;
    use std::sync::Mutex;

    /// A download that fails leaves the update for Download & Install's retry.
    #[test]
    fn reading_the_pending_update_leaves_it_for_a_retry() {
        let slot = Mutex::new(Some("v0.3.0"));
        assert_eq!(pending_update(&slot), Some("v0.3.0"));
        assert_eq!(pending_update(&slot), Some("v0.3.0"));
        assert_eq!(pending_update(&Mutex::new(None::<&str>)), None);
    }
}
