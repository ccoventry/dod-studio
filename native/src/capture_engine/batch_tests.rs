//! `spawn_capture_engine` from the outside, up to the launch: the events a
//! batch sends and what it leaves on disk when it stops before the game
//! starts. Windows only, since the batch links its folders with `mklink /J`.
//!
//! Every batch here stops before `Launcher::spawn`: none of them may start
//! HLAE or the game. The config's HLAE and game paths point at files that
//! don't exist, so a test that went wrong still couldn't launch anything.

use super::{CaptureJob, EngineEvent, spawn_capture_engine};
use crate::test_support::Scratch;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// A fake game folder, a media folder and a capture folder in one scratch.
struct Folders {
    root: Scratch,
}

impl Folders {
    fn new(tag: &str) -> Self {
        let root = Scratch::new(format!("capture_engine_{tag}"));
        std::fs::create_dir_all(root.path().join("game").join("dod")).unwrap();
        std::fs::create_dir_all(root.path().join("src")).unwrap();
        Self { root }
    }

    fn game(&self) -> PathBuf {
        self.root.path().join("game")
    }

    fn hl_exe(&self) -> PathBuf {
        self.game().join("hl.exe")
    }

    fn capture_dir(&self) -> PathBuf {
        self.root.path().join("cap0")
    }

    fn media(&self) -> PathBuf {
        self.root.path().join("media")
    }

    /// A patched demo outside the game folder, `bytes` long.
    fn demo(&self, name: &str, bytes: usize) -> CaptureJob {
        let path = self.root.path().join("src").join(name);
        std::fs::write(&path, vec![7u8; bytes]).unwrap();
        CaptureJob {
            patched_demo_path: path,
        }
    }

    fn config(&self) -> crate::patch::PatcherConfig {
        crate::patch::PatcherConfig {
            capture_directories: vec![self.capture_dir()],
            primary_media_dir: Some(self.media()),
            hlae_path: self
                .root
                .path()
                .join("no_hlae")
                .join("HLAE.exe")
                .display()
                .to_string(),
            game_path: self.hl_exe().display().to_string(),
            goldsrc_hooks_dll_path: Some(
                self.root.path().join("no_hooks.dll").display().to_string(),
            ),
            ..Default::default()
        }
    }
}

/// What a test hands the engine besides the jobs.
struct Run {
    hl_path: PathBuf,
    config: crate::patch::PatcherConfig,
    drive_headroom: Vec<crate::patch::DriveHeadroom>,
    cancelled: bool,
}

impl Run {
    fn new(folders: &Folders) -> Self {
        Self {
            hl_path: folders.hl_exe(),
            config: folders.config(),
            drive_headroom: Vec::new(),
            cancelled: false,
        }
    }

    /// Runs the batch and returns every event it sent, in order, once its
    /// thread has dropped the sender.
    fn events(self, jobs: Vec<CaptureJob>) -> Vec<String> {
        let (tx, rx) = std::sync::mpsc::channel();
        spawn_capture_engine(
            jobs,
            Arc::new(self.hl_path),
            tx,
            Arc::new(AtomicBool::new(self.cancelled)),
            self.config,
            self.drive_headroom,
            Vec::new(),
            None,
            Arc::new(crate::game_handoff::BatchStart::default()),
            Arc::new(AtomicBool::new(false)),
        );
        let mut events = Vec::new();
        loop {
            match rx.recv_timeout(Duration::from_secs(30)) {
                Ok(event) => events.push(label(&event)),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return events,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    panic!("the batch is still running after 30 s: {events:?}")
                }
            }
        }
    }
}

/// An event as text, with any path in it as `.display()` writes it.
fn label(event: &EngineEvent) -> String {
    match event {
        EngineEvent::Starting(n) => format!("Starting({n})"),
        EngineEvent::Launching(what) => format!("Launching({what})"),
        EngineEvent::Finished(what) => format!("Finished({what})"),
        EngineEvent::Error(msg) => format!("Error({msg})"),
        EngineEvent::AllCompleted => "AllCompleted".into(),
        EngineEvent::Cancelled => "Cancelled".into(),
        other => format!("{other:?}"),
    }
}

fn aborted(msg: &str) -> String {
    format!("Error(Capture Engine Aborted — {msg} (see View Logs for details))")
}

fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

#[test]
fn an_hl_exe_path_with_no_folder_is_refused() {
    let folders = Folders::new("no_parent");
    let mut run = Run::new(&folders);
    run.hl_path = PathBuf::new();
    assert_eq!(
        run.events(Vec::new()),
        [
            "Starting(0)".to_string(),
            aborted("Invalid hl.exe path: it has no parent folder"),
        ]
    );
}

/// Nothing to play: the batch links its folders, finds no demo and ends
/// without launching. The links go with it; the capture folder stays.
#[test]
fn a_batch_with_nothing_to_play_completes_without_launching() {
    let folders = Folders::new("nothing");
    let stale_done = folders.game().join("DOD_BATCH_DONE");
    std::fs::create_dir_all(&stale_done).unwrap();
    assert_eq!(
        Run::new(&folders).events(Vec::new()),
        ["Starting(0)", "AllCompleted"]
    );
    assert!(folders.capture_dir().is_dir(), "the capture folder is made");
    assert!(!exists(&stale_done), "a stale done folder is cleared");
    for link in ["dodstudio_session", "dod_pool_0", "DOD_STUDIO_EXIT_TRIGGER"] {
        assert!(!exists(&folders.game().join(link)), "{link} is left behind");
    }
}

#[test]
fn a_batch_cancelled_before_it_starts_copies_nothing() {
    let folders = Folders::new("cancelled");
    let mut run = Run::new(&folders);
    run.cancelled = true;
    let job = folders.demo("my-clip.dem", 64);
    assert_eq!(run.events(vec![job]), ["Starting(1)", "Cancelled"]);
    assert!(!folders.game().join("dod").join("my_clip.dem").exists());
}

/// A demo that can't be read sends an `Error` and the batch goes on to the
/// next one; with none left, it completes. Pinned as it is: the `Error` ends
/// the listener's batch while the engine carries on.
#[test]
fn a_demo_that_cant_be_copied_sends_an_error_and_the_batch_goes_on() {
    let folders = Folders::new("bad_demos");
    let unnamed = CaptureJob {
        patched_demo_path: PathBuf::from(".."),
    };
    let missing = CaptureJob {
        patched_demo_path: folders.root.path().join("src").join("gone.dem"),
    };
    let events = Run::new(&folders).events(vec![unnamed, missing]);
    assert_eq!(events.len(), 4, "{events:?}");
    assert_eq!(events[0], "Starting(2)");
    assert_eq!(events[1], aborted("Invalid demo path: .."));
    assert!(
        events[2]
            .starts_with("Error(Capture Engine Aborted — Failed to open source demo for copy: "),
        "{}",
        events[2]
    );
    assert_eq!(events[3], "AllCompleted");
}

/// A drive the plan found short of room stops the batch after `Launching`,
/// with the demos already copied into `dod/` (named with `_` for `-`).
#[test]
fn a_drive_without_headroom_stops_the_batch_before_the_launch() {
    let folders = Folders::new("headroom");
    let mut run = Run::new(&folders);
    run.drive_headroom = vec![crate::patch::DriveHeadroom {
        path: folders.capture_dir(),
        free_bytes: 0,
    }];
    let job = folders.demo("my-clip.dem", 64);
    assert_eq!(
        run.events(vec![job]),
        [
            "Starting(1)".to_string(),
            "Launching(Batch Queue)".to_string(),
            aborted(&format!(
                "{} has less than 15.0 GB free space. Free some space or pick another capture directory.",
                folders.capture_dir().display()
            )),
        ]
    );
    let copied = folders.game().join("dod").join("my_clip.dem");
    assert_eq!(std::fs::read(&copied).unwrap(), vec![7u8; 64]);
}

#[test]
fn obs_mode_with_no_planned_blocks_stops_before_the_launch() {
    let folders = Folders::new("obs_empty");
    let mut run = Run::new(&folders);
    run.config.capture_mode = crate::patch::CaptureMode::Obs;
    let job = folders.demo("clip.dem", 16);
    assert_eq!(
        run.events(vec![job]),
        [
            "Starting(1)".to_string(),
            "Launching(Batch Queue)".to_string(),
            aborted("OBS capture was requested but no blocks were planned"),
        ]
    );
}
