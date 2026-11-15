//! Tells Studio what the kill feed's layout shows (#1339): when `layout even`
//! is asked for but can't draw, the game's own layout shows, and a capture
//! batch would record every clip with it. The hook knows why ([`super::even`]'s
//! `Wait`), so it sends that over the events pipe, where Studio writes it to
//! the activity log and warns while the batch runs.
//!
//! ## The line
//!
//! `[dod-studio] KILLFEED_LAYOUT` then tab-separated fields: the state
//! (`default`, `even` while it draws, or `waiting`), then for `waiting`
//! `reason=` (`files`, `hd_off`, `no_hd`, `no_names`, `no_hooks`), `style=`
//! (the HD style in force) and `why=` (the hook's own words, as
//! `ds_hud_deathmsg status` prints them). Studio skips fields it does not
//! know, so either side can add one.
//!
//! ## When
//!
//! Once per change, never per frame: a state is sent once it has held for
//! [`SETTLE_FRAMES`] frames with a level loaded, so a level load's
//! in-between frames (the atlas loaded, the names not read yet) never reach
//! Studio. Before the first level the layout can't draw at all, which is no
//! news. A batch's start ([`report_again`]) sends the state again, for the
//! next batch's Studio in a game left open (#725), which reads the pipe only
//! from its own start.
//!
//! Everything here runs on the engine's thread from `deathmsg::poll`, with
//! atomics only; the line is built only on a change.

use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};

/// What the line's label starts with (Studio's
/// `native::capture_engine::markers::killfeed_layout`).
pub(super) const LABEL: &str = "KILLFEED_LAYOUT";

/// Frames a state must hold before it is sent: a level load's settling
/// takes a few, and a second or so at any frame rate is still prompt.
pub(super) const SETTLE_FRAMES: u32 = 60;

/// The layout as one byte: `layout default`, or `layout even` drawing, or
/// [`WAITING`] plus the reason's own byte (`even::Wait`, never 0).
pub(super) const DEFAULT: u8 = 0;
pub(super) const DRAWING: u8 = 1;
pub(super) const WAITING: u8 = 0x10;
/// [`SENT`] after [`report_again`]: whatever settles next is sent, unless it
/// is `layout default`, which needs no word.
const AGAIN: u8 = 0xff;

/// The state seen last frame, how many frames running, and what Studio was
/// last sent.
static SEEN: AtomicU8 = AtomicU8::new(DEFAULT);
static STEADY: AtomicU32 = AtomicU32::new(0);
static SENT: AtomicU8 = AtomicU8::new(DEFAULT);

/// What a frame does about the report.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// Nothing new, or not settled yet.
    Wait,
    /// Send this state.
    Send(u8),
    /// Note this state as sent without a word (`layout default` after
    /// [`report_again`]).
    Note(u8),
}

/// What to do with `now`, this frame's state, given the state seen last
/// frame, how many frames it had held, and what was last sent. Returns the
/// new count with the step. `level` says whether a level is loaded; it is
/// asked only once every [`SETTLE_FRAMES`] frames of a settled state not
/// sent yet, so its engine call and copy are never a per-frame cost.
pub(super) fn settle(
    seen: u8,
    steady: u32,
    sent: u8,
    now: u8,
    level: impl FnOnce() -> bool,
) -> (u32, Step) {
    let steady = if now == seen {
        steady.saturating_add(1)
    } else {
        0
    };
    if steady < SETTLE_FRAMES || steady % SETTLE_FRAMES != 0 || now == sent || !level() {
        return (steady, Step::Wait);
    }
    if sent == AGAIN && now == DEFAULT {
        return (steady, Step::Note(now));
    }
    (steady, Step::Send(now))
}

/// Follows the layout's state, every frame. `why` gives a `waiting` state's
/// reason word and the hook's own words; it runs only on a change.
pub(super) fn follow(
    now: u8,
    level: impl FnOnce() -> bool,
    why: impl FnOnce() -> (&'static str, String),
) {
    let seen = SEEN.swap(now, Ordering::Relaxed);
    let (steady, step) = settle(
        seen,
        STEADY.load(Ordering::Relaxed),
        SENT.load(Ordering::Relaxed),
        now,
        level,
    );
    STEADY.store(steady, Ordering::Relaxed);
    match step {
        Step::Wait => {}
        Step::Note(state) => SENT.store(state, Ordering::Relaxed),
        Step::Send(state) => {
            SENT.store(state, Ordering::Relaxed);
            let (reason, words) = if state >= WAITING {
                why()
            } else {
                ("", String::new())
            };
            let label = label(
                state,
                reason,
                &crate::texture_hires::style_in_force(),
                &words,
            );
            crate::events::send(&label);
            #[cfg(target_arch = "x86")]
            unsafe {
                crate::debug::report(&format!("{} {label}", crate::events::TAG))
            };
        }
    }
}

/// Sends the layout's state again once it settles: a batch has started, and
/// its Studio reads the pipe only from now (#725).
pub(crate) fn report_again() {
    SENT.store(AGAIN, Ordering::Relaxed);
}

/// A field's value with no tab or line break, which would end it or the line.
fn field(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

/// The line's label for `state`; `reason`, `style` and `why` only count for
/// a `waiting` one.
pub(super) fn label(state: u8, reason: &str, style: &str, why: &str) -> String {
    match state {
        DEFAULT => format!("{LABEL}\tdefault"),
        DRAWING => format!("{LABEL}\teven"),
        _ => format!(
            "{LABEL}\twaiting\treason={}\tstyle={}\twhy={}",
            field(reason),
            field(style),
            field(why)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `frames` frames of `now` from `(seen, steady, sent)`, returning
    /// every step that wasn't a wait and the state left.
    fn run(state: &mut (u8, u32, u8), now: u8, level: bool, frames: u32) -> Vec<Step> {
        let mut steps = Vec::new();
        for _ in 0..frames {
            let (steady, step) = settle(state.0, state.1, state.2, now, || level);
            *state = (now, steady, state.2);
            match step {
                Step::Wait => {}
                Step::Send(s) | Step::Note(s) => {
                    state.2 = s;
                    steps.push(step);
                }
            }
        }
        steps
    }

    const NO_HD: u8 = WAITING + 3;
    const HD_OFF: u8 = WAITING + 2;

    #[test]
    fn a_reason_is_sent_once_it_settles_and_never_again_while_it_holds() {
        let mut s = (DEFAULT, 0, DEFAULT);
        assert!(run(&mut s, NO_HD, true, SETTLE_FRAMES).is_empty());
        assert_eq!(run(&mut s, NO_HD, true, 1), vec![Step::Send(NO_HD)]);
        assert!(run(&mut s, NO_HD, true, 10_000).is_empty());
        // Fixed: the even layout draws, said once.
        assert_eq!(
            run(&mut s, DRAWING, true, SETTLE_FRAMES + 5),
            vec![Step::Send(DRAWING)]
        );
    }

    /// A level load's in-between frames, and the menu before any level,
    /// never reach Studio.
    #[test]
    fn nothing_is_sent_before_a_level_or_for_a_passing_state() {
        let mut s = (DEFAULT, 0, DEFAULT);
        assert!(run(&mut s, WAITING + 1, false, 10_000).is_empty());
        assert!(run(&mut s, WAITING + 4, true, SETTLE_FRAMES - 1).is_empty());
        assert_eq!(
            run(&mut s, DRAWING, true, SETTLE_FRAMES + 1),
            vec![Step::Send(DRAWING)]
        );
        // `layout default` with nothing sent before: no word.
        let mut s = (DEFAULT, 0, DEFAULT);
        assert!(run(&mut s, DEFAULT, true, 10_000).is_empty());
    }

    #[test]
    fn a_new_batch_hears_the_state_again_but_not_a_default_layout() {
        let mut s = (HD_OFF, 10_000, AGAIN);
        assert_eq!(
            run(&mut s, HD_OFF, true, SETTLE_FRAMES),
            vec![Step::Send(HD_OFF)]
        );
        let mut s = (DEFAULT, 10_000, AGAIN);
        assert_eq!(
            run(&mut s, DEFAULT, true, SETTLE_FRAMES),
            vec![Step::Note(DEFAULT)]
        );
        assert!(run(&mut s, DEFAULT, true, 100).is_empty());
    }

    /// The level check calls the engine, so it is asked once
    /// every [`SETTLE_FRAMES`] frames at most, and only while there is
    /// something to send.
    #[test]
    fn the_level_is_asked_rarely_and_only_with_something_to_send() {
        let mut asked = 0;
        let (mut seen, mut steady, sent) = (DEFAULT, 0, DEFAULT);
        for _ in 0..SETTLE_FRAMES * 10 {
            let now = WAITING + 1;
            let (s, step) = settle(seen, steady, sent, now, || {
                asked += 1;
                false
            });
            assert_eq!(step, Step::Wait);
            (seen, steady) = (now, s);
        }
        assert_eq!(asked, 9);
        let mut asked = 0;
        for _ in 0..SETTLE_FRAMES * 3 {
            let _ = settle(DEFAULT, steady, DEFAULT, DEFAULT, || {
                asked += 1;
                true
            });
        }
        assert_eq!(asked, 0, "nothing to send");
    }

    #[test]
    fn the_line_names_the_reason_style_and_the_hooks_words() {
        assert_eq!(
            label(DEFAULT, "", "ultrasharp", ""),
            "KILLFEED_LAYOUT\tdefault"
        );
        assert_eq!(
            label(DRAWING, "", "ultrasharp", ""),
            "KILLFEED_LAYOUT\teven"
        );
        assert_eq!(
            label(NO_HD, "no_hd", "ultrasharp", "none\tfor\nthis style"),
            "KILLFEED_LAYOUT\twaiting\treason=no_hd\tstyle=ultrasharp\twhy=none for this style"
        );
    }
}
