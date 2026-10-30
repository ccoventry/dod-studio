//! The review's kill range and note form (#1139): while a review highlight
//! plays or waits at its end, E (or `ds_demo_review form`) opens a small form
//! over the demo, with the menu closed: the kill range's first and last kill
//! and a one-line note, as Highlight Details has them. Tab moves between the
//! boxes, Enter saves, Esc cancels (and the menu the engine brings up on that
//! same Esc goes away again). The keyboard is enough: the form takes
//! the keys while it shows, so typing a Y or an N in it never answers.
//! Clicking it works where the build passes the mouse on (pre-Anniversary).
//!
//! This file holds what the form shows, what it saved and the rules for the
//! typed range; `studio_panel`'s `review_form` builds and drives the vgui2
//! window, and `review` applies a save to the highlight and sends it to
//! DoD Studio.
//!
//! ## Threads
//!
//! Everything here runs on the engine's thread: the review's poll asks for
//! the form, the panel's poll opens it, and vgui2 calls the form's key and
//! button handlers from the engine's input. The atomics and the mutex only
//! hand values between those calls.

// The window is 32-bit only; a host build compiles the rest for the tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// What the form shows for one highlight.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FormView {
    /// Studio's key for the highlight: a save for another one is dropped.
    pub key: String,
    /// "Highlight 3 of 12 | m00cat | 4 kills".
    pub title: String,
    /// The kill range, from 1.
    pub from: usize,
    pub to: usize,
    /// How many kills the highlight has.
    pub kills: usize,
    pub note: String,
}

/// What Enter saved: the kill range (already checked against the kills) and
/// the note, for the highlight `key`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Saved {
    pub key: String,
    pub from: usize,
    pub to: usize,
    pub note: String,
}

/// Set by the review when E is pressed; the panel's next poll opens the form.
static OPEN_REQUESTED: AtomicBool = AtomicBool::new(false);
/// Whether the form shows now, set by the panel as it shows and hides it.
static SHOWN: AtomicBool = AtomicBool::new(false);
/// A save waiting for the review's next poll.
static SAVED: Mutex<Option<Saved>> = Mutex::new(None);

/// Asks for the form over the playing demo, on the next frame.
pub(crate) fn request_open() {
    OPEN_REQUESTED.store(true, Ordering::Release);
}

/// Whether the form was asked for since the last call.
pub(crate) fn take_open_request() -> bool {
    OPEN_REQUESTED.swap(false, Ordering::AcqRel)
}

/// Notes whether the form shows: while it does, the answer keys do nothing.
pub(crate) fn set_shown(shown: bool) {
    SHOWN.store(shown, Ordering::Release);
}

/// Whether the form shows now.
pub(crate) fn is_shown() -> bool {
    SHOWN.load(Ordering::Acquire)
}

/// How long after the form's Esc a menu coming up is Esc's doing. The engine
/// brings the menu up on the same Esc that cancels the form (seen live on
/// PRE), within a frame or two.
const ESC_MENU_WINDOW: Duration = Duration::from_millis(500);

/// When Esc last closed the form.
static ESC_AT: Mutex<Option<Instant>> = Mutex::new(None);

/// Notes that Esc closed the form: the menu the same Esc brings up is put
/// away again, and doesn't open the Review tab.
pub(crate) fn note_esc() {
    *ESC_AT.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
}

/// Whether Esc closed the form just now, so a menu coming up is its doing.
pub(crate) fn menu_is_from_esc() -> bool {
    ESC_AT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_some_and(|at| at.elapsed() < ESC_MENU_WINDOW)
}

/// Hands a save to the review's next poll; a second save before it replaces
/// the first.
pub(crate) fn save(saved: Saved) {
    *SAVED.lock().unwrap_or_else(|e| e.into_inner()) = Some(saved);
}

/// The save since the last call, if any.
pub(crate) fn take_saved() -> Option<Saved> {
    SAVED.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// The kill range typed in the From and To boxes, for a highlight with
/// `kills` kills whose range is `current` now. An empty box keeps its end of
/// the range; two ends typed the wrong way round are swapped. `Err` says what
/// to type instead, for the form to show while it stays open.
pub(crate) fn typed_range(
    from: &str,
    to: &str,
    current: (usize, usize),
    kills: usize,
) -> Result<(usize, usize), String> {
    let kill = |text: &str, keep: usize| -> Result<usize, String> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(keep);
        }
        text.parse::<usize>()
            .ok()
            .filter(|n| (1..=kills).contains(n))
            .ok_or_else(|| range_hint(kills))
    };
    let (a, b) = (kill(from, current.0)?, kill(to, current.1)?);
    Ok((a.min(b), a.max(b)))
}

/// What a kill number must be, said when one isn't.
fn range_hint(kills: usize) -> String {
    match kills {
        1 => "This highlight has one kill: type 1.".to_string(),
        n => format!("Type a kill from 1 to {n}."),
    }
}

/// The line under the boxes: how to use the form.
pub(crate) const KEYS_HINT: &str = "Tab next box   Enter save   Esc cancel";

/// "of 4 kills" beside the To box.
pub(crate) fn kill_count_text(kills: usize) -> String {
    match kills {
        1 => "of 1 kill".to_string(),
        n => format!("of {n} kills"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_boxes_keep_the_range() {
        assert_eq!(typed_range("", " ", (2, 3), 4), Ok((2, 3)));
        assert_eq!(typed_range("1", "", (2, 3), 4), Ok((1, 3)));
    }

    #[test]
    fn a_range_typed_backwards_is_swapped() {
        assert_eq!(typed_range("4", "2", (1, 4), 4), Ok((2, 4)));
    }

    #[test]
    fn a_kill_outside_the_highlight_is_refused_with_what_to_type() {
        // "12": a 2 typed after the 1 that was in the box.
        assert_eq!(
            typed_range("12", "4", (1, 4), 4),
            Err("Type a kill from 1 to 4.".to_string())
        );
        assert!(typed_range("0", "", (1, 4), 4).is_err());
        assert!(typed_range("x", "", (1, 4), 4).is_err());
        assert_eq!(
            typed_range("2", "", (1, 1), 1),
            Err("This highlight has one kill: type 1.".to_string())
        );
    }

    #[test]
    fn a_save_is_taken_once() {
        save(Saved {
            key: "k".into(),
            from: 1,
            to: 2,
            note: "n".into(),
        });
        assert_eq!(take_saved().map(|s| s.to), Some(2));
        assert_eq!(take_saved(), None);
    }

    #[test]
    fn the_kill_count_reads_as_a_phrase() {
        assert_eq!(kill_count_text(1), "of 1 kill");
        assert_eq!(kill_count_text(4), "of 4 kills");
    }
}
