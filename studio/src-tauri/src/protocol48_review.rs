//! A review of protocol-47 demos (#1549): the game plays each highlight with
//! `viewdemo`, which refuses a protocol-47 demo, so the queue names the
//! demo's converted copy (`protocol48_demos`). The game echoes that copy in
//! its answers and kill-range edits, and the page finds rows by the original,
//! so each answer is mapped back here before it is emitted.
//!
//! - [`prepare`] makes the copies before the queue is written; they come back
//!   pinned in a [`ReviewCopies`].
//! - [`sent`] makes them the latest review once the game has the queue: their
//!   pins replace the previous review's, the mapping keeps the previous
//!   reviews' pairs whose copies still exist (a review Studio restarted
//!   during has its answers read only once the next review starts its
//!   listener), and the pairs are written beside the queue.
//! - [`answered`] maps one event; [`ended`] lets the copies go when the review
//!   ends (pins dropped, the file cleared).
//! - [`keep_earlier_review`], at start, pins the last review's copies again,
//!   only while a game is running (it may still be reviewing).

use std::path::{Path, PathBuf};

use native::patch::copy_cache::{Pins, prune};
use native::patch::protocol48_copies::{KEEP_COPIES, Playable};
use native::review_queue::{ReviewEvent, ReviewHighlight};

use crate::protocol48_demos::{Folders, emit_preview_status, playable_file};

/// A review's converted copies: each `(copy, original)` pair, and the pins
/// that keep the copies from being pruned while the review may still play
/// them. Two originals can share one copy (identical files in two folders):
/// both pairs are kept, and an answer picks its original by the row key.
#[derive(Default)]
pub struct ReviewCopies {
    pins: Pins,
    pairs: Vec<(String, String)>,
}

impl ReviewCopies {
    /// Notes that the review plays `copy` for `original`, pinning it.
    fn add(&mut self, copy: &Path, original: &str) {
        let copy = copy.to_string_lossy().into_owned();
        if self.pairs.iter().any(|(c, o)| *c == copy && o == original) {
            return;
        }
        self.pins.add(Path::new(&copy));
        self.pairs.push((copy, original.to_string()));
    }
}

/// The latest review's copies, and the pairs of earlier ones still on disk.
static LATEST: std::sync::Mutex<Option<ReviewCopies>> = std::sync::Mutex::new(None);

fn latest() -> std::sync::MutexGuard<'static, Option<ReviewCopies>> {
    // Each change is one assignment, so the slot is whole after a panic.
    lock_recover::lock_or_recover(&LATEST)
}

/// `highlights` with each protocol-47 demo swapped for its converted copy,
/// and those copies, pinned. Reports each copy being made on the preview
/// status event, which the Review highlights button shows. Nothing changes
/// for the running review until [`sent`]. `Err` is the line saying which
/// demo can't play.
pub fn prepare(
    app: &tauri::AppHandle,
    highlights: Vec<ReviewHighlight>,
) -> Result<(Vec<ReviewHighlight>, ReviewCopies), String> {
    let folders = Folders::studio();
    let prepared = review_highlights(highlights, &folders, &mut |status| {
        emit_preview_status(app, status, false)
    })?;
    // The new copies are pinned in `prepared`, so they stay.
    prune(&folders.copies, KEEP_COPIES);
    Ok(prepared)
}

/// [`prepare`] with copies under `folders`: each demo is looked at once.
fn review_highlights(
    mut highlights: Vec<ReviewHighlight>,
    folders: &Folders,
    report: &mut dyn FnMut(String),
) -> Result<(Vec<ReviewHighlight>, ReviewCopies), String> {
    let mut seen: Vec<(String, String)> = Vec::new();
    let mut copies = ReviewCopies::default();
    for highlight in &mut highlights {
        if let Some((_, file)) = seen.iter().find(|(demo, _)| *demo == highlight.demo) {
            highlight.demo = file.clone();
            continue;
        }
        let (file, played) = playable_file(Path::new(&highlight.demo), folders, &mut *report)?;
        if matches!(played, Playable::Copy { .. }) {
            copies.add(&file, &highlight.demo);
        }
        let file = file.to_string_lossy().into_owned();
        seen.push((highlight.demo.clone(), file.clone()));
        highlight.demo = file;
    }
    Ok((highlights, copies))
}

/// The game has the review of `copies`: they become the latest review (see
/// the module doc), and the pairs are written beside the queue.
pub fn sent(copies: ReviewCopies) {
    let mut slot = latest();
    let merged = with_earlier_pairs(copies, slot.take(), |copy| Path::new(copy).is_file());
    // Best-effort: without the file, only a Studio started again during
    // this review loses track of its copies.
    let _ = std::fs::write(review_copies_file(), pairs_text(&merged.pairs));
    *slot = Some(merged);
}

/// `new` with the pairs of `earlier` whose copy `exists` added after its
/// own; `earlier`'s pins are dropped.
fn with_earlier_pairs(
    mut new: ReviewCopies,
    earlier: Option<ReviewCopies>,
    exists: impl Fn(&str) -> bool,
) -> ReviewCopies {
    for pair in earlier.map(|e| e.pairs).unwrap_or_default() {
        if exists(&pair.0) && !new.pairs.contains(&pair) {
            new.pairs.push(pair);
        }
    }
    new
}

/// The review has ended: its copies are no longer pinned, and a Studio
/// started later doesn't pin them again. The pairs stay, for an answer
/// still on its way.
pub fn ended() {
    if let Some(review) = latest().as_mut() {
        review.pins = Pins::default();
    }
    // Our own file: when it can't be removed, the next start only pins
    // copies a little longer.
    let _ = std::fs::remove_file(review_copies_file());
}

/// `event` as the page needs it: an answer or a kill-range edit for a
/// converted copy names the copy's original, the demo the page's rows are
/// keyed by. Any other event is unchanged.
pub fn answered(event: ReviewEvent) -> ReviewEvent {
    let slot = latest();
    let pairs = slot.as_ref().map(|r| r.pairs.as_slice()).unwrap_or(&[]);
    original_named(event, pairs)
}

/// [`answered`]'s rule for `pairs`: the original of the event's copy; with
/// two for one copy, the one the row key starts with (Studio's keys begin
/// with the demo's path), else the first.
fn original_named(mut event: ReviewEvent, pairs: &[(String, String)]) -> ReviewEvent {
    if let ReviewEvent::Answer { demo, key, .. } | ReviewEvent::Edit { demo, key, .. } = &mut event
    {
        let mut originals = pairs.iter().filter(|(c, _)| c == demo).map(|(_, o)| o);
        let first = originals.clone().next();
        let by_key = originals.find(|o| key.starts_with(o.as_str()));
        if let Some(original) = by_key.or(first) {
            *demo = original.clone();
        }
    }
    event
}

/// Beside the review queue: the latest review's pairs, one
/// `copy<TAB>original` per line, for [`keep_earlier_review`].
const REVIEW_COPIES_FILE: &str = "review_copies.tsv";

fn review_copies_file() -> PathBuf {
    native::shared::paths::get_appdata_dir().join(REVIEW_COPIES_FILE)
}

fn pairs_text(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(copy, original)| format!("{copy}\t{original}\n"))
        .collect()
}

fn parse_pairs_text(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(copy, original)| (copy.to_string(), original.to_string()))
        .collect()
}

/// At start, while a game is running (it may still be reviewing): pins the
/// copies of the review Studio last sent and restores their pairs, so the
/// copies aren't pruned under it and its answers, read once the next review
/// starts a listener, find their rows. Copies already gone are left out.
/// With no game running the review is over, and nothing is pinned.
pub fn keep_earlier_review() {
    if native::sys::process::game_pids().is_empty() {
        return;
    }
    let Ok(text) = std::fs::read_to_string(review_copies_file()) else {
        return;
    };
    let restored = restored_copies(&text, &Folders::studio().copies);
    let mut slot = latest();
    if slot.is_none() && !restored.pairs.is_empty() {
        *slot = Some(restored);
    }
}

/// The pairs in `text` whose copy is a file under `root`, pinned.
fn restored_copies(text: &str, root: &Path) -> ReviewCopies {
    let mut copies = ReviewCopies::default();
    for (copy, original) in parse_pairs_text(text) {
        let path = Path::new(&copy);
        if path.starts_with(root) && path.is_file() {
            copies.add(path, &original);
        }
    }
    copies
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol48_demos::test_scratch::Scratch;
    use native::review_queue::{format_queue, parse_event};

    fn highlight(demo: String, key: &str) -> ReviewHighlight {
        ReviewHighlight {
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
        }
    }

    /// Makes `folder`'s copy look last used a day ago.
    fn backdate(folder: &Path) {
        let marker = std::fs::File::options()
            .write(true)
            .open(folder.join(native::patch::copy_cache::USED_MARKER))
            .unwrap();
        marker
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(86_400))
            .unwrap();
    }

    #[test]
    fn a_review_swaps_each_protocol_47_demo_for_its_copy_and_keeps_the_keys() {
        let s = Scratch::new("review");
        let (reviewed, copies) = review_highlights(
            vec![
                highlight(s.old(), "a"),
                highlight(s.new_demo(), "b"),
                highlight(s.old(), "c"),
            ],
            &s.folders(),
            &mut |_| {},
        )
        .unwrap();
        assert!(Path::new(&reviewed[0].demo).starts_with(s.dir.join("copies")));
        assert_eq!(reviewed[1].demo, s.new_demo());
        assert_eq!(reviewed[2].demo, reviewed[0].demo);
        let keys: Vec<&str> = reviewed.iter().map(|h| h.key.as_str()).collect();
        assert_eq!(keys, ["a", "b", "c"]);

        // The review's copy outlives a prune to nothing until its pins go
        // (made long ago, so only the pin keeps it).
        let folder = Path::new(&reviewed[0].demo).parent().unwrap().to_path_buf();
        backdate(&folder);
        assert_eq!(prune(&s.dir.join("copies"), 0), 0);
        assert!(Path::new(&reviewed[0].demo).is_file());
        drop(copies);
        assert_eq!(prune(&s.dir.join("copies"), 0), 1);
    }

    #[test]
    fn an_answer_for_a_reviewed_copy_names_the_original_again() {
        let s = Scratch::new("review_answer");
        let (reviewed, copies) =
            review_highlights(vec![highlight(s.old(), "row-1")], &s.folders(), &mut |_| {})
                .unwrap();
        let copy = reviewed[0].demo.clone();
        assert_ne!(copy, s.old());
        // The queue the game reads names the copy...
        let queue = format_queue(&reviewed, None, None);
        assert!(queue.lines().nth(1).unwrap().starts_with(&copy));
        // ...and the game echoes it back, as goldsrc-hooks review.rs writes
        // its answer and form lines.
        for line in [
            format!("[dod-studio] REVIEW\t{copy}\trow-1\tyes\t1\t1\t"),
            format!("[dod-studio] REVIEW_EDIT\t{copy}\trow-1\t1\t1\tnice"),
        ] {
            let event = original_named(parse_event(&line).expect(&line), &copies.pairs);
            match event {
                ReviewEvent::Answer { demo, key, .. } | ReviewEvent::Edit { demo, key, .. } => {
                    assert_eq!(demo, s.old(), "{line}");
                    assert_eq!(key, "row-1");
                }
                other => panic!("{other:?}"),
            }
        }
        // A protocol-48 demo's answer is left alone.
        let line = format!("[dod-studio] REVIEW\t{}\trow-2\tno\t1\t1\t", s.new_demo());
        let event = original_named(parse_event(&line).unwrap(), &copies.pairs);
        assert!(matches!(event, ReviewEvent::Answer { demo, .. } if demo == s.new_demo()));
    }

    #[test]
    fn two_originals_sharing_one_copy_each_get_their_own_answers() {
        let pairs = vec![
            (
                "C:/copies/k/a_p48.dem".to_string(),
                "D:/one/a.dem".to_string(),
            ),
            (
                "C:/copies/k/a_p48.dem".to_string(),
                "E:/two/a.dem".to_string(),
            ),
        ];
        let answer = |key: &str| ReviewEvent::Answer {
            demo: "C:/copies/k/a_p48.dem".to_string(),
            key: key.to_string(),
            verdict: "yes".to_string(),
            from: 1,
            to: 1,
            note: String::new(),
        };
        for (key, original) in [
            ("D:/one/a.dem#8#100#200", "D:/one/a.dem"),
            ("E:/two/a.dem#8#100#200", "E:/two/a.dem"),
            ("some other key", "D:/one/a.dem"),
        ] {
            let ReviewEvent::Answer { demo, .. } = original_named(answer(key), &pairs) else {
                panic!("not an answer");
            };
            assert_eq!(demo, original, "{key}");
        }
        // Both pairs are kept when they are added.
        let mut copies = ReviewCopies::default();
        copies.add(Path::new("C:/copies/k/a_p48.dem"), "D:/one/a.dem");
        copies.add(Path::new("C:/copies/k/a_p48.dem"), "E:/two/a.dem");
        copies.add(Path::new("C:/copies/k/a_p48.dem"), "E:/two/a.dem");
        assert_eq!(copies.pairs.len(), 2);
    }

    #[test]
    fn a_new_review_keeps_the_earlier_pairs_whose_copies_still_exist() {
        let pair = |c: &str, o: &str| (c.to_string(), o.to_string());
        let earlier = ReviewCopies {
            pins: Pins::default(),
            pairs: vec![pair("kept_p48.dem", "a.dem"), pair("gone_p48.dem", "b.dem")],
        };
        let new = ReviewCopies {
            pins: Pins::default(),
            pairs: vec![pair("new_p48.dem", "c.dem")],
        };
        let merged = with_earlier_pairs(new, Some(earlier), |c| c != "gone_p48.dem");
        assert_eq!(
            merged.pairs,
            vec![pair("new_p48.dem", "c.dem"), pair("kept_p48.dem", "a.dem")]
        );
    }

    #[test]
    fn the_review_copies_file_reads_back_what_was_written() {
        let pairs = vec![
            (
                "C:/copies/v1-1-2/a_p48.dem".to_string(),
                "D:/demos/a b.dem".to_string(),
            ),
            (
                "C:/copies/v1-1-2/a_p48.dem".to_string(),
                "E:/x/a b.dem".to_string(),
            ),
            (
                "C:/copies/v1-3-4/c_p48.dem".to_string(),
                "D:/demos/c.dem".to_string(),
            ),
        ];
        assert_eq!(parse_pairs_text(&pairs_text(&pairs)), pairs);
        assert!(parse_pairs_text("no tab here\n").is_empty());
    }

    #[test]
    fn only_copies_still_under_the_root_are_restored() {
        let s = Scratch::new("review_restore");
        let (reviewed, _held) =
            review_highlights(vec![highlight(s.old(), "a")], &s.folders(), &mut |_| {}).unwrap();
        let copy = reviewed[0].demo.clone();
        let text = format!(
            "{copy}\t{}\n{}\tx.dem\nC:/elsewhere/y_p48.dem\ty.dem\n",
            s.old(),
            s.dir
                .join("copies")
                .join("gone")
                .join("z_p48.dem")
                .display()
        );
        let restored = restored_copies(&text, &s.dir.join("copies"));
        assert_eq!(restored.pairs, vec![(copy, s.old())]);
    }
}
