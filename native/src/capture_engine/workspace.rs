//! What a batch puts in the game folder before the game starts, and takes
//! out again however it ends: the session and capture-folder junctions, the
//! patched demos copied into `dod/`, and the guards that clean up.

use super::leftovers::Leftovers;
use super::{CaptureJob, EngineEvent, abort};
use crate::log_markdown;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;

pub(super) struct CaptureCleanupGuard {
    exit_trigger: PathBuf,
    session_junction: PathBuf,
    auto_clear_logs: bool,
    auto_clear_temp_demos: bool,
    auto_clear_previews: bool,
    save_local_patched_copy: bool,
    /// OBS's connection, when this batch is driving one.
    ///
    /// Here rather than in the batch loop because this guard is the only thing
    /// that runs on every exit path the process survives — normal completion,
    /// cancel, a mid-batch crash of the game, an early return. Anywhere else
    /// and a cancelled batch leaves OBS recording indefinitely, filling the
    /// user's drive with no indication that anything is wrong.
    ///
    /// Not a panic, though: release builds are `panic = "abort"`, so nothing
    /// unwinds and no `Drop` runs. That gap, a hard kill and a power cut are
    /// all covered by `obs::recover` on the next start instead.
    pub(super) obs: Option<std::sync::Arc<std::sync::Mutex<Option<crate::obs::ObsClient>>>>,
    /// Copies of demos on another drive a hook-run batch plays from when its
    /// hook DLL is older than #779
    /// (`patch::source_copies`), removed however the batch ends.
    pub(super) source_copies: Vec<PathBuf>,
    _wake_lock: Option<keepawake::KeepAwake>,
}

impl CaptureCleanupGuard {
    pub(super) fn new(
        exit_trigger: PathBuf,
        session_junction: PathBuf,
        auto_clear_logs: bool,
        auto_clear_temp_demos: bool,
        auto_clear_previews: bool,
        save_local_patched_copy: bool,
    ) -> Self {
        // Pre-clean any stale signal dirs/junctions from a previous aborted run.
        if let Err(e) = std::fs::remove_dir_all(&exit_trigger)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            log_markdown(&format!(
                "[cleanup] Failed to pre-clean exit_trigger {}: {}",
                exit_trigger.display(),
                e
            ));
        }
        if let Err(e) = std::fs::remove_dir(&session_junction)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            log_markdown(&format!(
                "[cleanup] Failed to pre-clean session_junction {}: {}",
                session_junction.display(),
                e
            ));
        }
        Self {
            exit_trigger,
            session_junction,
            auto_clear_logs,
            auto_clear_temp_demos,
            auto_clear_previews,
            save_local_patched_copy,
            obs: None,
            source_copies: Vec::new(),
            _wake_lock: keepawake::Builder::default()
                .display(false)
                .idle(true)
                .sleep(true)
                .create()
                .ok(),
        }
    }
}

impl Drop for CaptureCleanupGuard {
    fn drop(&mut self) {
        // First, before anything else can fail: OBS must not be left
        // recording. Best-effort and silent by design — there is nobody left
        // to report to, and a cleanup path that panics is worse than one that
        // quietly does nothing.
        if let Some(obs) = self.obs.take()
            && let Ok(mut guard) = obs.lock()
        {
            if let Some(client) = guard.as_mut() {
                client.stop_record_quietly();
            }
            *guard = None;
        }

        if let Err(e) = std::fs::remove_dir_all(&self.exit_trigger)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            log_markdown(&format!(
                "[cleanup] Failed to remove exit_trigger {}: {}",
                self.exit_trigger.display(),
                e
            ));
        }
        if let Err(e) = std::fs::remove_dir(&self.session_junction)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            log_markdown(&format!(
                "[cleanup] Failed to remove session_junction {}: {}",
                self.session_junction.display(),
                e
            ));
        }

        if let Some(game_root) = self.exit_trigger.parent() {
            crate::shared::paths::clear_capture_scratch(
                game_root,
                self.auto_clear_logs,
                self.auto_clear_temp_demos,
                self.auto_clear_previews,
                self.save_local_patched_copy,
            );
        }
        // Whatever the auto-clear settings say: these are never the user's.
        crate::patch::source_copies::remove_all(self.source_copies.iter().map(PathBuf::as_path));
    }
}

/// The batch's links in the game folder and the guards that undo them.
///
/// Field order is drop order: `links` (the junctions) go before `cleanup`
/// (OBS, the exit trigger, the scratch files), the order the batch has
/// always cleaned up in.
pub(super) struct Workspace {
    _links: crate::patch::WorkspaceGuard,
    pub(super) cleanup: CaptureCleanupGuard,
    pub(super) exit_trigger: PathBuf,
    pub(super) session_junction: PathBuf,
    pub(super) pool_junctions: Vec<PathBuf>,
    /// Where the batch's media goes: the primary media folder, or Studio's
    /// own folder when none is set.
    pub(super) export_dir: PathBuf,
}

impl Workspace {
    /// Clears what a previous batch left, links `dodstudio_session` to this
    /// session's media folder and `dod_pool_N` to each capture folder, and
    /// arms the guards. `None` once a failure has been reported; the guard
    /// made so far cleans up as it drops, and `leftovers` holds the links
    /// already made (#1212).
    pub(super) fn prepare(
        config: &crate::patch::PatcherConfig,
        runtime_batch: Option<&crate::patch::runtime_batch::RuntimeBatch>,
        hl_exe_parent: &Path,
        tx: &Sender<EngineEvent>,
        leftovers: &mut Leftovers,
    ) -> Option<Self> {
        let dummy_path = hl_exe_parent.join("DOD_BATCH_DONE");
        std::fs::remove_dir_all(&dummy_path).ok();

        let exit_trigger = hl_exe_parent.join("DOD_STUDIO_EXIT_TRIGGER");
        let session_junction = hl_exe_parent.join("dodstudio_session");

        let mut cleanup = CaptureCleanupGuard::new(
            exit_trigger.clone(),
            session_junction.clone(),
            config.auto_clear_logs,
            config.auto_clear_temp_demos,
            config.auto_clear_previews,
            config.save_local_patched_copy,
        );
        if let Some(batch) = runtime_batch {
            cleanup.source_copies = batch.source_copies.clone();
        }

        let export_dir = config.primary_media_dir.clone().unwrap_or_else(|| {
            let exe_path = std::env::current_exe().expect("Failed to resolve absolute exe path");
            exe_path
                .parent()
                .expect("Exe has no parent directory")
                .to_path_buf()
        });
        let session_dir = if !config.session_id.is_empty() {
            export_dir.join(&config.session_id)
        } else {
            export_dir.clone()
        };
        if let Err(msg) = link_session(&session_junction, &session_dir) {
            abort(tx, &msg);
            return None;
        }

        // `build_batch_queue` creates a `_route_N` junction beside hl.exe for
        // every drive it routes blocks to. It has no way to clean them up —
        // it returns long before the capture ends — so they are listed here
        // by the same indices and unlinked with everything else. Indices this
        // batch never routed to simply are not there, which `remove_dir`
        // reports as NotFound and the guard ignores.
        let route_junctions: Vec<PathBuf> = (0..config.capture_directories.len())
            .map(|idx| hl_exe_parent.join(format!("_route_{}", idx)))
            .collect();
        if let Err(msg) =
            link_capture_folders(&config.capture_directories, hl_exe_parent, leftovers)
        {
            abort(tx, &msg);
            return None;
        }
        let pool_junctions = leftovers.pool_junctions().to_vec();

        let links = crate::patch::WorkspaceGuard {
            session_junction: session_junction.clone(),
            exit_trigger: exit_trigger.clone(),
            pool_junctions: pool_junctions.clone(),
            route_junctions,
            auto_clear_logs: config.auto_clear_logs,
            auto_clear_temp_demos: config.auto_clear_temp_demos,
            auto_clear_previews: config.auto_clear_previews,
            save_local_patched_copy: config.save_local_patched_copy,
        };
        Some(Self {
            _links: links,
            cleanup,
            exit_trigger,
            session_junction,
            pool_junctions,
            export_dir,
        })
    }
}

/// Links `session_junction` to `session_dir` with `mklink /J`.
fn link_session(session_junction: &Path, session_dir: &Path) -> Result<(), String> {
    let session_junction_str = session_junction.to_str().unwrap_or_default();
    let session_dir_str = session_dir.to_str().unwrap_or_default();
    if session_junction_str.is_empty() || session_dir_str.is_empty() {
        return Err(format!(
            "Can't link {} to {}: the path has characters mklink can't take",
            session_junction.display(),
            session_dir.display()
        ));
    }

    match std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J", session_junction_str, session_dir_str])
        .output()
    {
        Ok(out) if !out.status.success() => Err(format!(
            "Could not link {} to {} (mklink): {}",
            session_junction.display(),
            session_dir.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        Err(e) => Err(format!(
            "Could not run mklink to link {}: {}",
            session_junction.display(),
            e
        )),
        _ => Ok(()),
    }
}

/// Creates each capture folder and links `dod_pool_N` beside hl.exe to it,
/// in order, adding each junction to `leftovers` as it is made; the first
/// failure stops it.
fn link_capture_folders(
    capture_directories: &[PathBuf],
    hl_exe_parent: &Path,
    leftovers: &mut Leftovers,
) -> Result<(), String> {
    for (idx, target_dir) in capture_directories.iter().enumerate() {
        let junction_path = hl_exe_parent.join(format!("dod_pool_{}", idx));
        let _ = std::fs::remove_dir(&junction_path);
        if let Err(e) = std::fs::create_dir_all(target_dir) {
            return Err(format!(
                "Failed to create capture directory {}: {}",
                target_dir.display(),
                e
            ));
        }
        let junction_str = junction_path.to_str().unwrap_or_default();
        let target_str = target_dir.to_str().unwrap_or_default();
        if junction_str.is_empty() || target_str.is_empty() {
            return Err(format!(
                "Can't link {} to capture directory {}: the path has characters mklink can't take",
                junction_path.display(),
                target_dir.display()
            ));
        }
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J", junction_str, target_str])
            .output();
        match status {
            Ok(out) if out.status.success() => {
                log_markdown(&format!(
                    "[pool] Junction created: {} -> {}",
                    junction_path.display(),
                    target_dir.display()
                ));
                leftovers.add_pool_junction(junction_path);
            }
            Ok(out) => {
                let err_msg = String::from_utf8_lossy(&out.stderr);
                return Err(format!(
                    "Could not link {} to capture directory {} (mklink): {}",
                    junction_path.display(),
                    target_dir.display(),
                    err_msg.trim()
                ));
            }
            Err(e) => {
                return Err(format!(
                    "Could not run mklink to link {}: {}",
                    junction_path.display(),
                    e
                ));
            }
        }
    }
    Ok(())
}

/// Copies each patched demo into `dod/`, where `playdemo` finds it, named
/// with `_` for `-`, adding each copy to `leftovers`. `None` when the batch
/// was cancelled first (`Cancelled` sent) or a demo couldn't be copied (its
/// `Error` sent): the listener has ended the batch by then, so the game
/// must not start (#1212).
pub(super) fn copy_demos_into_game(
    jobs: &[CaptureJob],
    dod_dir: &Path,
    save_local_patched_copy: bool,
    cancel_token: &std::sync::atomic::AtomicBool,
    tx: &Sender<EngineEvent>,
    leftovers: &mut Leftovers,
) -> Option<()> {
    for job in jobs {
        if cancel_token.load(Ordering::Relaxed) {
            let _ = tx.send(EngineEvent::Cancelled);
            return None;
        }
        leftovers.add_demo_copy(copy_demo(job, dod_dir, save_local_patched_copy, tx)?);
    }
    Some(())
}

/// One demo into `dod/`: its path there, or `None` once the failure is
/// reported.
fn copy_demo(
    job: &CaptureJob,
    dod_dir: &Path,
    save_local_patched_copy: bool,
    tx: &Sender<EngineEvent>,
) -> Option<PathBuf> {
    let demo_filename = match job.patched_demo_path.file_name() {
        Some(name) => name.to_string_lossy().replace("-", "_"),
        None => {
            abort(
                tx,
                &format!("Invalid demo path: {}", job.patched_demo_path.display()),
            );
            return None;
        }
    };

    let dest_demo_path = dod_dir.join(&demo_filename);
    let source_path_str = job.patched_demo_path.to_string_lossy().to_lowercase();
    let dest_path_str = dest_demo_path.to_string_lossy().to_lowercase();

    if source_path_str != dest_path_str {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let mut src_file = match std::fs::OpenOptions::new()
                .read(true)
                .share_mode(1)
                .open(&job.patched_demo_path)
            {
                Ok(f) => f,
                Err(e) => {
                    abort(tx, &format!("Failed to open source demo for copy: {}", e));
                    return None;
                }
            };

            let mut dest_file_opt = None;
            for i in 0..5 {
                match std::fs::File::create(&dest_demo_path) {
                    Ok(f) => {
                        dest_file_opt = Some(f);
                        break;
                    }
                    Err(e) => {
                        if e.raw_os_error() == Some(32) && i < 4 {
                            std::thread::sleep(std::time::Duration::from_millis(150));
                            continue;
                        }
                        break;
                    }
                }
            }

            let mut dest_file = match dest_file_opt {
                Some(f) => f,
                None => {
                    abort(
                        tx,
                        &format!(
                            "Could not create {} after several tries (copying from {}). Is the file open in another program?",
                            dest_demo_path.display(),
                            job.patched_demo_path.display()
                        ),
                    );
                    return None;
                }
            };

            log_markdown(&format!(
                "- [IO] Copying (Windows) from {}",
                source_path_str
            ));
            log_markdown(&format!("- [IO] Copying (Windows) to {}", dest_path_str));
            match std::io::copy(&mut src_file, &mut dest_file) {
                Ok(bytes) => {
                    log_markdown(&format!("- [IO] Copy SUCCESS! Bytes written: {}", bytes))
                }
                Err(e) => {
                    log_markdown(&format!("- [IO] Copy FAILED! Error: {}", e));
                    abort(tx, &format!("Failed to copy demo to game folder: {}", e));
                    return None;
                }
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            log_markdown(&format!("- [IO] Copying (*nix) from {}", source_path_str));
            log_markdown(&format!("- [IO] Copying (*nix) to {}", dest_path_str));
            match std::fs::copy(&job.patched_demo_path, &dest_demo_path) {
                Ok(bytes) => {
                    log_markdown(&format!("- [IO] Copy SUCCESS! Bytes written: {}", bytes))
                }
                Err(e) => {
                    log_markdown(&format!("- [IO] Copy FAILED! Error: {}", e));
                    abort(tx, &format!("Failed to copy demo to game folder: {}", e));
                    return None;
                }
            }
        }
    }

    if save_local_patched_copy {
        let exe_path = std::env::current_exe().expect("Failed to resolve absolute exe path");
        let base_dir = exe_path
            .parent()
            .expect("Exe has no parent directory")
            .to_path_buf();
        let demos_dir = base_dir.join("demos");
        let _ = std::fs::create_dir_all(&demos_dir);
        let local_dest = demos_dir.join(&demo_filename);
        match std::fs::copy(&job.patched_demo_path, &local_dest) {
            Ok(_) => log_markdown(&format!(
                "- [IO] Saved local copy to demos/{}",
                demo_filename
            )),
            Err(e) => log_markdown(&format!(
                "- [IO] Failed to save local patched copy to {}: {}",
                local_dest.display(),
                e
            )),
        }
    }

    Some(dest_demo_path)
}

/// Deletes the demos the batch copied into `dod/`.
pub(super) fn remove_demo_copies(active_dest_paths: &[PathBuf]) {
    for path in active_dest_paths {
        let _ = std::fs::remove_file(path);
    }
}

/// The files and links a launched batch removes when it ends, before its
/// guards run.
pub(super) struct BatchFiles<'a> {
    /// The demos copied into `dod/`, which only a failed or cancelled batch
    /// removes here.
    pub(super) demo_copies: &'a [PathBuf],
    pub(super) cfg_path: &'a Path,
    pub(super) dummy_path: &'a Path,
    pub(super) exit_trigger: &'a Path,
    pub(super) session_junction: &'a Path,
    pub(super) pool_junctions: &'a [PathBuf],
}

impl BatchFiles<'_> {
    /// The demo copies first, then the rest.
    pub(super) fn remove_with_demo_copies(&self) {
        remove_demo_copies(self.demo_copies);
        self.remove();
    }

    /// Everything but the demo copies.
    pub(super) fn remove(&self) {
        std::fs::remove_file(self.cfg_path).ok();
        std::fs::remove_dir_all(self.dummy_path).ok();
        std::fs::remove_dir_all(self.exit_trigger).ok();
        let _ = std::fs::remove_dir(self.session_junction);
        for junction in self.pool_junctions {
            let _ = std::fs::remove_dir(junction);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    /// A regression guard for #198's fix, not the retry behaviour itself --
    /// `remove_file_retrying` (`shared/paths.rs`) already covers that in
    /// isolation. This just confirms the guard still calls it correctly and
    /// leaves everything else in `dod/` untouched.
    #[test]
    fn dropping_the_guard_removes_primer_and_chain_demos_when_enabled() {
        let root = Scratch::new("cleanup_guard_on");
        let dod = root.join("dod");
        std::fs::create_dir_all(&dod).unwrap();
        std::fs::write(dod.join("dodstudio_primer.dem"), b"x").unwrap();
        std::fs::write(dod.join("dodstudio_chain_01.dem"), b"x").unwrap();
        std::fs::write(dod.join("dodstudio_chain_02.dem"), b"x").unwrap();
        std::fs::write(dod.join("not_a_chain_demo.dem"), b"x").unwrap();

        {
            let _guard = CaptureCleanupGuard::new(
                root.join("exit_trigger"),
                root.join("session_junction"),
                false,
                true,
                false,
                false,
            );
        }

        assert!(!dod.join("dodstudio_primer.dem").exists());
        assert!(!dod.join("dodstudio_chain_01.dem").exists());
        assert!(!dod.join("dodstudio_chain_02.dem").exists());
        assert!(
            dod.join("not_a_chain_demo.dem").exists(),
            "must not touch an unrelated file"
        );
    }

    /// `save_local_patched_copy` means the user asked to keep these files --
    /// the guard must not clean them up even with `auto_clear_temp_demos` on.
    #[test]
    fn dropping_the_guard_keeps_demos_when_a_local_copy_was_requested() {
        let root = Scratch::new("cleanup_guard_keep");
        let dod = root.join("dod");
        std::fs::create_dir_all(&dod).unwrap();
        std::fs::write(dod.join("dodstudio_primer.dem"), b"x").unwrap();
        std::fs::write(dod.join("dodstudio_chain_01.dem"), b"x").unwrap();

        {
            let _guard = CaptureCleanupGuard::new(
                root.join("exit_trigger"),
                root.join("session_junction"),
                false,
                true,
                false,
                true,
            );
        }

        assert!(dod.join("dodstudio_primer.dem").exists());
        assert!(dod.join("dodstudio_chain_01.dem").exists());
    }

    /// A hook-run batch's copies of demos from another drive go with the
    /// guard, whatever the auto-clear settings say (#434).
    #[test]
    fn dropping_the_guard_removes_the_batchs_source_copies() {
        let root = Scratch::new("cleanup_guard_copies");
        let dod = root.join("dod");
        std::fs::create_dir_all(&dod).unwrap();
        let copy = dod.join("dodstudio_src_01_match.dem");
        std::fs::write(&copy, b"x").unwrap();
        std::fs::write(dod.join("match.dem"), b"x").unwrap();

        {
            let mut guard = CaptureCleanupGuard::new(
                root.join("exit_trigger"),
                root.join("session_junction"),
                false,
                false,
                false,
                false,
            );
            guard.source_copies = vec![copy.clone()];
        }

        assert!(!copy.exists());
        assert!(dod.join("match.dem").exists());
    }

    /// A launched batch's end removes its quit cfg, both signal folders, the
    /// session link and every capture-folder link, and leaves what they
    /// point at alone.
    #[cfg(windows)]
    #[test]
    fn a_batchs_files_and_links_are_removed_but_not_what_they_point_at() {
        let root = Scratch::new("capture_engine_batch_files");
        let game = root.join("game");
        let target = root.join("takes");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("take.avi"), b"x").unwrap();
        std::fs::create_dir_all(&game).unwrap();
        let mut leftovers = Leftovers::default();
        link_capture_folders(std::slice::from_ref(&target), &game, &mut leftovers).unwrap();
        let pools = leftovers.pool_junctions().to_vec();
        leftovers.launched();
        assert_eq!(pools, [game.join("dod_pool_0")]);
        let session = game.join("dodstudio_session");
        link_session(&session, &target).unwrap();
        let cfg = game.join("dod_quit.cfg");
        std::fs::write(&cfg, "quit\n").unwrap();
        let dummy = root.join("DOD_BATCH_DONE");
        let trigger = game.join("DOD_STUDIO_EXIT_TRIGGER");
        std::fs::create_dir_all(dummy.join("x")).unwrap();
        std::fs::create_dir_all(trigger.join("x")).unwrap();
        let demo = game.join("dodstudio_chain_01.dem");
        std::fs::write(&demo, b"x").unwrap();
        let demo_copies = [demo.clone()];

        let files = BatchFiles {
            demo_copies: &demo_copies,
            cfg_path: &cfg,
            dummy_path: &dummy,
            exit_trigger: &trigger,
            session_junction: &session,
            pool_junctions: &pools,
        };
        files.remove();
        for gone in [&cfg, &dummy, &trigger, &session, &pools[0]] {
            assert!(
                std::fs::symlink_metadata(gone).is_err(),
                "{} is left",
                gone.display()
            );
        }
        assert!(demo.exists(), "a finished batch's demo copies stay here");
        assert!(target.join("take.avi").exists());

        files.remove_with_demo_copies();
        assert!(!demo.exists());
    }

    /// A capture folder that can't be made partway through stops the
    /// linking, and the links already made go with the batch (#1212).
    #[cfg(windows)]
    #[test]
    fn a_link_that_fails_partway_leaves_the_earlier_links_to_be_removed() {
        let root = Scratch::new("workspace_link_partway");
        let game = root.join("game");
        std::fs::create_dir_all(&game).unwrap();
        let good = root.join("takes");
        let blocker = root.join("a_file");
        std::fs::write(&blocker, b"x").unwrap();
        let mut leftovers = Leftovers::default();
        let err = link_capture_folders(
            &[good.clone(), blocker.join("takes")],
            &game,
            &mut leftovers,
        )
        .unwrap_err();
        assert!(
            err.starts_with("Failed to create capture directory"),
            "{err}"
        );
        assert_eq!(leftovers.pool_junctions(), [game.join("dod_pool_0")]);
        drop(leftovers);
        assert!(std::fs::symlink_metadata(game.join("dod_pool_0")).is_err());
        assert!(good.is_dir(), "the capture folder itself stays");
    }
}
