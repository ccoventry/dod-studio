//! Protocol-48 copies of protocol-47 demos (#1549), the Studio half: every
//! path that starts a demo with the game's `viewdemo` (an in-place Launch
//! Preview, a Seek Between Clips batch, a review) plays a converted copy of a
//! protocol-47 demo, since `viewdemo` refuses the original
//! (`native::patch::protocol48_copies`). A protocol-48 demo plays itself.
//! Patched previews and batches need nothing: `StreamPatcher` converts as it
//! copies. The original is never written.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use native::patch::PatchJob;
use native::patch::copy_cache::{Pins, prune};
use native::patch::protocol48_copies::{KEEP_COPIES, Playable, Step, prepare_playable_in};
use native::review_queue::ReviewHighlight;
use tauri::Emitter;

/// The least time between two progress reports (CLAUDE.md's ~33 ms).
const REPORT_EVERY: Duration = Duration::from_millis(33);

/// Where copies are made and where their analyzer cache entries go: Studio's
/// own folders, or a test's.
struct Folders {
    copies: PathBuf,
    analyzer: PathBuf,
}

impl Folders {
    fn studio() -> Self {
        Self {
            copies: native::patch::protocol48_copies::copies_root(),
            analyzer: native::analyzer_cache_root(),
        }
    }
}

fn file_name(demo: &Path) -> String {
    demo.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| demo.display().to_string())
}

/// Calls `report` with each step's line, at most every [`REPORT_EVERY`]
/// (a step change always reports).
fn throttled(name: String, mut report: impl FnMut(String)) -> impl FnMut(Step) {
    let mut last: Option<(Instant, std::mem::Discriminant<Step>)> = None;
    move |step: Step| {
        let kind = std::mem::discriminant(&step);
        if last.is_some_and(|(at, k)| k == kind && at.elapsed() < REPORT_EVERY) {
            return;
        }
        last = Some((Instant::now(), kind));
        report(crate::messages::converting_demo(&name, step));
    }
}

/// The file the game plays for `demo` under `folders`: its converted copy
/// when it is a protocol-47 demo, else `demo` itself. `report` gets each
/// step's line while a copy is made. `Err` is the line saying why it can't
/// play.
fn playable_file(
    demo: &Path,
    folders: &Folders,
    report: impl FnMut(String),
) -> Result<(PathBuf, Playable), String> {
    let mut progress = throttled(file_name(demo), report);
    let played = prepare_playable_in(demo, &folders.copies, &folders.analyzer, &mut progress)
        .map_err(|e| crate::messages::demo_cannot_be_converted(&file_name(demo), e))?;
    Ok((played.file(demo), played))
}

/// The file a Launch Preview plays in place for `demo` (the highlights'
/// demo, or its trimmed copy): its converted copy for a protocol-47 demo, or
/// `demo`. Reports each step on the preview's status event
/// (`trimmed_demos::PREVIEW_EVENT`, which the Launch Preview button shows
/// whatever copy is being made), and a last line when a copy plays. `Err`:
/// the preview must not start; the line is logged.
pub fn for_preview(app: &tauri::AppHandle, demo: &Path) -> Result<PathBuf, String> {
    let emit = |status: String, done: bool| {
        let _ = app.emit(
            crate::trimmed_demos::PREVIEW_EVENT,
            serde_json::json!({ "status": status, "done": done }),
        );
    };
    let folders = Folders::studio();
    match playable_file(demo, &folders, |status| emit(status, false)) {
        Ok((file, Playable::Original)) => Ok(file),
        Ok((file, Playable::Copy { .. })) => {
            // The copy was just stamped used, so it is the newest and stays.
            prune(&folders.copies, KEEP_COPIES);
            emit(
                crate::messages::PREVIEW_PLAYS_CONVERTED_COPY.to_string(),
                true,
            );
            Ok(file)
        }
        Err(line) => {
            native::log_markdown(&crate::messages::preview_start::preview_not_started(&line));
            Err(line)
        }
    }
}

/// A batch's converted copies: the copy each demo plays from, keyed by the
/// original's path as the batch's jobs name it (protocol-48 demos aren't in
/// it), and the pins that keep them on disk. Hold `pins` until the batch's
/// game is done with them.
#[derive(Default)]
pub struct BatchCopies {
    pub copies: HashMap<String, PathBuf>,
    pub pins: Pins,
}

/// The converted copies a Seek Between Clips batch of `jobs` plays from:
/// each demo's, or its trimmed copy's (`trimmed`, keyed by the original),
/// when that is a protocol-47 demo. Reports each step on the Capture footer
/// (`capture_status`). Prunes the copies past [`KEEP_COPIES`] however it ends.
/// `Err` is the batch's error line: a copy that couldn't be made, or Cancel.
pub fn for_batch(
    app: &tauri::AppHandle,
    jobs: &[PatchJob],
    trimmed: &HashMap<String, PathBuf>,
    cancel: &AtomicBool,
) -> Result<BatchCopies, String> {
    let folders = Folders::studio();
    let with_clips = jobs.iter().filter(|j| !j.blocks.is_empty());
    let played = played_demos(with_clips.map(|j| j.source_demo.as_str()), trimmed);
    let total = played.len() as u32;
    let made = batch_copies(
        &played,
        &folders,
        &|| cancel.load(Ordering::Relaxed),
        &mut |i, status| {
            let _ = app.emit(
                "capture_status",
                serde_json::json!({
                    "running": true,
                    "stage": "converting",
                    "index": i as u32,
                    "total": total,
                    "measured": false,
                    "status": status,
                }),
            );
        },
    );
    prune(&folders.copies, KEEP_COPIES);
    made
}

/// Each of `demos` (the batch's jobs' with clips), once, in order: the
/// original's path as the jobs name it, and the file the game would open for
/// it (the trimmed copy, else the original).
fn played_demos<'a>(
    demos: impl IntoIterator<Item = &'a str>,
    trimmed: &HashMap<String, PathBuf>,
) -> Vec<(String, PathBuf)> {
    let mut played: Vec<(String, PathBuf)> = Vec::new();
    for demo in demos {
        if played.iter().any(|(d, _)| d == demo) {
            continue;
        }
        let file = trimmed
            .get(demo)
            .cloned()
            .unwrap_or_else(|| PathBuf::from(demo));
        played.push((demo.to_string(), file));
    }
    played
}

/// [`for_batch`]'s copies of `played` under `folders`, pinning each as soon
/// as it is ready. `report` gets the demo's index and each step's line.
fn batch_copies(
    played: &[(String, PathBuf)],
    folders: &Folders,
    cancelled: &dyn Fn() -> bool,
    report: &mut dyn FnMut(usize, String),
) -> Result<BatchCopies, String> {
    let mut batch = BatchCopies::default();
    for (i, (demo, file)) in played.iter().enumerate() {
        if cancelled() {
            return Err(crate::messages::CAPTURE_CANCELLED_WHILE_CONVERTING.to_string());
        }
        match playable_file(file, folders, |status| report(i, status)) {
            Ok((_, Playable::Original)) => {}
            Ok((copy, Playable::Copy { .. })) => {
                batch.pins.add(&copy);
                batch.copies.insert(demo.clone(), copy);
            }
            Err(line) => {
                native::log_markdown(&crate::messages::batch::batch_not_started(&line));
                return Err(line);
            }
        }
    }
    Ok(batch)
}

/// What a hook-run batch plays for each demo, keyed by the original: its
/// converted copy, else its trimmed copy (both absent: the original).
pub fn plays_from(
    trimmed: &HashMap<String, PathBuf>,
    converted: &HashMap<String, PathBuf>,
) -> HashMap<String, PathBuf> {
    let mut plays = trimmed.clone();
    plays.extend(converted.iter().map(|(k, v)| (k.clone(), v.clone())));
    plays
}

/// `highlights` with each protocol-47 demo swapped for its converted copy,
/// for a review (the game plays each with `viewdemo`). The game answers by
/// each row's key, so a copy's answer still reaches its row. `Err` is the
/// line saying which demo can't play.
pub fn for_review(highlights: Vec<ReviewHighlight>) -> Result<Vec<ReviewHighlight>, String> {
    let folders = Folders::studio();
    let reviewed = review_highlights(highlights, &folders)?;
    prune(&folders.copies, KEEP_COPIES);
    Ok(reviewed)
}

/// [`for_review`] with copies under `folders`: each demo is looked at once.
fn review_highlights(
    mut highlights: Vec<ReviewHighlight>,
    folders: &Folders,
) -> Result<Vec<ReviewHighlight>, String> {
    let mut seen: HashMap<String, String> = HashMap::new();
    for highlight in &mut highlights {
        if let Some(file) = seen.get(&highlight.demo) {
            highlight.demo = file.clone();
            continue;
        }
        let (file, _) = playable_file(Path::new(&highlight.demo), folders, |_| {})?;
        let file = file.to_string_lossy().into_owned();
        seen.insert(highlight.demo.clone(), file.clone());
        highlight.demo = file;
    }
    Ok(highlights)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch folder with the CI fixture (protocol 48) and the same demo
    /// made to say 47 in its header and server info, as a 2004 recording
    /// does. Removed when the test ends.
    struct Scratch {
        dir: PathBuf,
        old: PathBuf,
        new: PathBuf,
    }

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("dodstudio-p48-demos-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let fixture =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-fixtures/ci_fixture.dem");
            let p48 = std::fs::read(fixture).unwrap();
            let mut p47 = p48.clone();
            p47[12..16].copy_from_slice(&47i32.to_le_bytes());
            let server_info = [11u8, 48, 0, 0, 0];
            let mut flipped = 0;
            for at in 0..p47.len() - 40 {
                if p47[at..at + 5] == server_info && &p47[at + 32..at + 36] == b"dod\0" {
                    p47[at + 1] = 47;
                    flipped += 1;
                }
            }
            assert!(flipped >= 1, "the fixture has a server info");
            let old = dir.join("old.dem");
            let new = dir.join("new.dem");
            std::fs::write(&old, p47).unwrap();
            std::fs::write(&new, p48).unwrap();
            Self { dir, old, new }
        }

        fn folders(&self) -> Folders {
            Folders {
                copies: self.dir.join("copies"),
                analyzer: self.dir.join("cache"),
            }
        }

        fn old(&self) -> String {
            self.old.to_string_lossy().into_owned()
        }

        fn new_demo(&self) -> String {
            self.new.to_string_lossy().into_owned()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn a_preview_of_a_protocol_47_demo_plays_its_copy_and_of_a_48_demo_the_demo() {
        let s = Scratch::new("preview");
        let mut lines = Vec::new();
        let (file, played) = playable_file(&s.old, &s.folders(), |l| lines.push(l)).unwrap();
        assert!(matches!(played, Playable::Copy { made: true, .. }));
        assert!(file.starts_with(s.dir.join("copies")), "{}", file.display());
        assert_eq!(
            std::fs::read(&file).unwrap(),
            std::fs::read(&s.new).unwrap()
        );
        assert!(lines.iter().all(|l| l.contains("old.dem")), "{lines:?}");

        let (file, played) = playable_file(&s.new, &s.folders(), |_| {}).unwrap();
        assert_eq!((file, played), (s.new.clone(), Playable::Original));
    }

    #[test]
    fn a_batch_plays_a_copy_only_for_its_protocol_47_demo() {
        let s = Scratch::new("batch");
        let played = vec![(s.old(), s.old.clone()), (s.new_demo(), s.new.clone())];
        let batch = batch_copies(&played, &s.folders(), &|| false, &mut |_, _| {}).unwrap();
        assert_eq!(batch.copies.len(), 1);
        let copy = &batch.copies[&s.old()];
        assert!(copy.starts_with(s.dir.join("copies")));
        assert!(!batch.copies.contains_key(&s.new_demo()));
    }

    #[test]
    fn a_batch_converts_the_trimmed_copy_it_plays() {
        let s = Scratch::new("batch_trimmed");
        let original = s
            .dir
            .join("over the limit.dem")
            .to_string_lossy()
            .into_owned();
        let trimmed = HashMap::from([(original.clone(), s.old.clone())]);
        let played = played_demos([original.as_str(), original.as_str()], &trimmed);
        assert_eq!(played, vec![(original.clone(), s.old.clone())], "once");
        let batch = batch_copies(&played, &s.folders(), &|| false, &mut |_, _| {}).unwrap();
        let plays = plays_from(&trimmed, &batch.copies);
        assert!(plays[&original].starts_with(s.dir.join("copies")));
    }

    #[test]
    fn a_cancelled_batch_makes_no_copy() {
        let s = Scratch::new("batch_cancel");
        let played = vec![(s.old(), s.old.clone())];
        let e = batch_copies(&played, &s.folders(), &|| true, &mut |_, _| {})
            .err()
            .unwrap();
        assert_eq!(e, crate::messages::CAPTURE_CANCELLED_WHILE_CONVERTING);
        assert!(!s.dir.join("copies").exists());
    }

    #[test]
    fn a_review_swaps_each_protocol_47_demo_for_its_copy_and_keeps_the_keys() {
        let s = Scratch::new("review");
        let highlight = |demo: String, key: &str| ReviewHighlight {
            demo,
            key: key.to_string(),
            player: String::new(),
            kill_times: vec![1.0],
            from: 1,
            to: 1,
            answered: None,
            note: String::new(),
            spectate: None,
            kills: Vec::new(),
        };
        let reviewed = review_highlights(
            vec![
                highlight(s.old(), "a"),
                highlight(s.new_demo(), "b"),
                highlight(s.old(), "c"),
            ],
            &s.folders(),
        )
        .unwrap();
        assert!(Path::new(&reviewed[0].demo).starts_with(s.dir.join("copies")));
        assert_eq!(reviewed[1].demo, s.new_demo());
        assert_eq!(reviewed[2].demo, reviewed[0].demo);
        let keys: Vec<&str> = reviewed.iter().map(|h| h.key.as_str()).collect();
        assert_eq!(keys, ["a", "b", "c"]);
    }

    #[test]
    fn a_protocol_47_demo_that_cannot_be_converted_says_why() {
        let s = Scratch::new("refused");
        let bad = s.dir.join("header only.dem");
        let mut bytes = vec![0u8; 544];
        bytes[..8].copy_from_slice(b"HLDEMO\0\0");
        bytes[12..16].copy_from_slice(&47i32.to_le_bytes());
        std::fs::write(&bad, bytes).unwrap();
        let e = playable_file(&bad, &s.folders(), |_| {}).unwrap_err();
        assert!(
            e.starts_with("header only.dem is an older protocol-47 demo"),
            "{e}"
        );
        assert!(e.contains("no server info"), "{e}");
    }
}
