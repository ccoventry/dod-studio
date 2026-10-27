//! The review's on-screen panel and answer keys (#660): while
//! `ds_demo_review` plays a highlight, a small box on the game screen says
//! which one it is, and when the highlight ends the box lists the keys that
//! answer it, so an answer doesn't need the ESC menu.
//!
//! ## The keys
//!
//! Y Yes, M Maybe, N No, R Replay, B Back, and ESC opens the DoD Studio
//! window on its Review tab, for the kill range and the note. They are
//! taken only while the review is paused at a highlight's end waiting for an
//! answer ([`AWAITING`]) and `ds_ui_review_keys` is on; at any other time
//! every key goes to the game as before. A taken key never reaches its
//! bind (Y is `messagemode` in DoD's default config).
//!
//! The keys come through `client.dll`'s `HUD_Key_Event` (slot 34 of its
//! `cldll_func_t` table), which returns 0 to swallow a key. The engine's
//! `Key_Event` asks it only while keys go to the game (`key_dest ==
//! key_game`: pre-Anniversary `hw.dll` `+0x61716`, 25th Anniversary
//! `+0x1d757c`), and only after vgui2 has passed the key on
//! (`ClientDLL_Key_Event`, `+0xb960` / `+0x1971b0`). So a key typed in the
//! console, in chat (`messagemode`) or in any box of the game's windows
//! never gets here, and can't answer. No bind is written: the game saves
//! binds into `config.cfg`, which is the user's.
//!
//! Once a key's press is taken, the rest of that press is too: its
//! autorepeat and its release, even after the answer has moved the review
//! on, so holding Y a moment too long doesn't open chat on the next
//! highlight ([`HELD`]).
//!
//! ## Threads
//!
//! `HUD_Key_Event` and `HUD_Redraw` run on the engine's thread, every key
//! and every frame. They read atomics and an `RwLock` the review's poll
//! writes on the same thread; the answer itself runs from the next poll
//! ([`take_pending`]), outside the key handler.

// The draw and the key hook are 32-bit only; a host build compiles the rest
// for the tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::demo_list_folders::Setting;
use crate::names::console_name;

/// `ds_ui_review_keys 1` (default): at a highlight's end the review waits
/// on the game screen for Y, M, N, R, B or ESC instead of opening the
/// Review tab. `0` opens the tab, as before #660.
pub static KEYS: Setting = Setting::new(console_name!("ui_review_keys"), true);
/// `ds_ui_review_info 1` (default): while a highlight loads and plays, the
/// box on screen says which one it is. The answer keys' box shows at a
/// highlight's end either way, while `ds_ui_review_keys` is on.
pub static INFO: Setting = Setting::new(console_name!("ui_review_info"), true);

pub fn keys_status() -> String {
    if KEYS.on() {
        "at a highlight's end, Y M N R B answer it on screen, ESC opens the Review tab".to_string()
    } else {
        "at a highlight's end, the Review tab opens for the answer".to_string()
    }
}

pub fn info_status() -> String {
    if INFO.on() {
        "while a highlight plays, a box on screen says which one it is".to_string()
    } else {
        "nothing on screen while a highlight plays".to_string()
    }
}

/// What a key asks the review to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Yes,
    Maybe,
    No,
    Replay,
    Back,
    /// ESC: the Review tab, for the kill range and note.
    OpenTab,
}

impl Action {
    fn code(self) -> u32 {
        self as u32 + 1
    }

    fn from_code(code: u32) -> Option<Action> {
        ACTIONS.iter().copied().find(|a| a.code() == code)
    }
}

const ACTIONS: [Action; 6] = [
    Action::Yes,
    Action::Maybe,
    Action::No,
    Action::Replay,
    Action::Back,
    Action::OpenTab,
];

/// The engine's key number for ESC (`K_ESCAPE`); letters are their
/// lower-case ASCII code.
const K_ESCAPE: i32 = 27;

/// The keys, in [`ACTIONS`]' order.
const KEY_CODES: [i32; 6] = [
    b'y' as i32,
    b'm' as i32,
    b'n' as i32,
    b'r' as i32,
    b'b' as i32,
    K_ESCAPE,
];

/// The action a key stands for, and its place in [`KEY_CODES`].
fn action_for(key: i32) -> Option<(usize, Action)> {
    KEY_CODES
        .iter()
        .position(|&k| k == key)
        .map(|i| (i, ACTIONS[i]))
}

/// Set by the review's poll while it waits for an answer with
/// `ds_ui_review_keys` on.
static AWAITING: AtomicBool = AtomicBool::new(false);
/// The action a key asked for, for the next poll; 0 for none.
static PENDING: AtomicU32 = AtomicU32::new(0);
/// One bit per key in [`KEY_CODES`] whose press was taken and not yet
/// released.
static HELD: AtomicU32 = AtomicU32::new(0);

/// What happens to one key event, given whether the review waits for an
/// answer and which taken keys are still down. Returns whether the key is
/// swallowed, the action to run, and the new held set.
fn decide(down: bool, key: i32, awaiting: bool, held: u32) -> (bool, Option<Action>, u32) {
    let Some((index, action)) = action_for(key) else {
        return (false, None, held);
    };
    let bit = 1u32 << index;
    if held & bit != 0 {
        // The rest of a press already taken: autorepeat or the release.
        let held = if down { held } else { held & !bit };
        return (true, None, held);
    }
    if down && awaiting {
        return (true, Some(action), held | bit);
    }
    (false, None, held)
}

/// `HUD_Key_Event`'s check: `true` swallows the key. One atomic load for a
/// key that isn't one of ours.
pub fn key_event(down: i32, key: i32) -> bool {
    if action_for(key).is_none() {
        return false;
    }
    let held = HELD.load(Ordering::Relaxed);
    let (swallow, action, held) = decide(down != 0, key, AWAITING.load(Ordering::Relaxed), held);
    HELD.store(held, Ordering::Relaxed);
    if let Some(action) = action {
        PENDING.store(action.code(), Ordering::Release);
    }
    swallow
}

/// The action a key asked for since the last poll.
pub(crate) fn take_pending() -> Option<Action> {
    Action::from_code(PENDING.swap(0, Ordering::AcqRel))
}

/// What the box shows: one line, or one line and the keys.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Panel {
    /// `Highlight 3 of 12 | m00cat | 4 kills`; empty hides the box.
    pub title: String,
    /// A second line in plain text ("Loading the demo..."), or empty.
    pub status: String,
    /// The answer keys' lines.
    pub keys: bool,
}

static PANEL: RwLock<Option<Panel>> = RwLock::new(None);

/// Called by the review's poll every frame with what the box should show
/// (`None` hides it) and whether keys answer now.
pub(crate) fn publish(panel: Option<Panel>, awaiting: bool) {
    AWAITING.store(awaiting, Ordering::Relaxed);
    if !awaiting {
        // A key pressed after the review moved on is dropped.
        PENDING.store(0, Ordering::Relaxed);
    }
    let same = PANEL.read().map(|p| *p == panel).unwrap_or(false);
    if !same && let Ok(mut p) = PANEL.write() {
        *p = panel;
    }
}

/// A segment of the keys line: the key, in the key colour, then its word.
pub(crate) const KEY_LINE: [(&str, &str); 5] = [
    ("Y", "Yes"),
    ("M", "Maybe"),
    ("N", "No"),
    ("R", "Replay"),
    ("B", "Back"),
];
pub(crate) const ESC_LINE: (&str, &str) = ("ESC", "kill range and note");

/// Draws the box. Runs after `client.dll`'s own `HUD_Redraw`, every frame.
///
/// # Safety
///
/// On the engine's thread, inside `HUD_Redraw`.
pub unsafe fn draw() {
    #[cfg(target_arch = "x86")]
    {
        let Ok(panel) = PANEL.try_read() else { return };
        let Some(panel) = panel.as_ref() else { return };
        // Safety: the caller's.
        unsafe { draw::panel(panel) };
    }
}

#[cfg(target_arch = "x86")]
mod draw {
    use std::ffi::CString;

    use super::{ESC_LINE, KEY_LINE, Panel};

    const WHITE: (f32, f32, f32) = (1.0, 1.0, 1.0);
    const GREY: (f32, f32, f32) = (0.75, 0.75, 0.75);
    /// The keys, in DoD's HUD amber.
    const KEY: (f32, f32, f32) = (1.0, 0.75, 0.25);
    const PADDING: i32 = 8;
    const GAP: i32 = 4;

    /// One run of text in one colour.
    struct Run {
        text: CString,
        colour: (f32, f32, f32),
    }

    fn run(text: &str, colour: (f32, f32, f32)) -> Option<Run> {
        CString::new(text).ok().map(|text| Run { text, colour })
    }

    fn lines(panel: &Panel) -> Vec<Vec<Run>> {
        let mut lines = Vec::new();
        lines.push(run(&panel.title, WHITE).into_iter().collect());
        if !panel.status.is_empty() {
            lines.push(run(&panel.status, GREY).into_iter().collect());
        }
        if panel.keys {
            let mut keys = Vec::new();
            for (i, (key, word)) in KEY_LINE.iter().enumerate() {
                let gap = if i == 0 { "" } else { "     " };
                keys.extend(run(&format!("{gap}{key}"), KEY));
                keys.extend(run(&format!(" {word}"), WHITE));
            }
            lines.push(keys);
            let mut esc = Vec::new();
            esc.extend(run(ESC_LINE.0, KEY));
            esc.extend(run(&format!(" {}", ESC_LINE.1), WHITE));
            lines.push(esc);
        }
        lines
    }

    /// # Safety
    ///
    /// On the engine's thread, inside `HUD_Redraw`.
    pub(super) unsafe fn panel(panel: &Panel) {
        if panel.title.is_empty() {
            return;
        }
        let Some(e) = crate::engine::engfuncs() else {
            return;
        };
        let mut info = crate::engine::ScreenInfo {
            size: size_of::<crate::engine::ScreenInfo>() as i32,
            width: 0,
            height: 0,
            flags: 0,
            char_height: 0,
            char_widths: [0; 256],
        };
        // Safety: the engine's own calls, on its thread, in a HUD draw.
        unsafe {
            (e.pfn_get_screen_info)(&mut info);
            if info.width <= 0 || info.height <= 0 {
                return;
            }
            let lines = lines(panel);
            let mut sizes = Vec::with_capacity(lines.len());
            for line in &lines {
                let (mut width, mut height) = (0, 0);
                for r in line {
                    let (mut w, mut h) = (0, 0);
                    (e.pfn_draw_console_string_len)(r.text.as_ptr(), &mut w, &mut h);
                    width += w;
                    height = height.max(h);
                }
                sizes.push((width, height));
            }
            let width = sizes.iter().map(|s| s.0).max().unwrap_or(0);
            let height: i32 =
                sizes.iter().map(|s| s.1).sum::<i32>() + GAP * (sizes.len() as i32 - 1);
            // Left, a fifth of the way down: clear of the HLTV bars at the
            // top, the chat at the bottom left and the kill feed at the
            // top right.
            let x = info.width / 40;
            let y = info.height / 5;
            crate::deathmsg::fill_box(
                [
                    x - PADDING,
                    y - PADDING,
                    x + width + PADDING,
                    y + height + PADDING,
                ],
                (0, 0, 0),
                160,
            );
            let mut top = y;
            for (line, (_, h)) in lines.iter().zip(&sizes) {
                let mut left = x;
                for r in line {
                    (e.pfn_draw_set_text_color)(r.colour.0, r.colour.1, r.colour.2);
                    left = (e.pfn_draw_console_string)(left, top, r.text.as_ptr());
                }
                top += h + GAP;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const Y: i32 = b'y' as i32;

    #[test]
    fn the_keys_answer_only_while_the_review_waits() {
        assert_eq!(decide(true, Y, false, 0), (false, None, 0));
        assert_eq!(decide(true, Y, true, 0), (true, Some(Action::Yes), 1));
        assert_eq!(
            decide(true, K_ESCAPE, true, 0),
            (true, Some(Action::OpenTab), 1 << 5)
        );
        // Any other key goes to the game, waiting or not.
        assert_eq!(decide(true, b'w' as i32, true, 0), (false, None, 0));
        assert_eq!(decide(true, b'e' as i32, true, 0), (false, None, 0));
    }

    #[test]
    fn a_taken_press_stays_taken_until_its_release() {
        // Y answered; the review moved on (not waiting), but Y is still
        // down: its autorepeat and its release must not reach `messagemode`.
        let (_, _, held) = decide(true, Y, true, 0);
        assert_eq!(decide(true, Y, false, held), (true, None, held));
        assert_eq!(decide(false, Y, false, held), (true, None, 0));
        // The next press, with the review playing, is the game's again.
        assert_eq!(decide(true, Y, false, 0), (false, None, 0));
    }

    #[test]
    fn a_release_alone_is_never_taken() {
        // Pressed before the review waited, released while it waits.
        assert_eq!(decide(false, Y, true, 0), (false, None, 0));
    }

    #[test]
    fn each_key_has_its_own_action() {
        let keys: Vec<_> = b"ymnrb"
            .iter()
            .map(|&k| action_for(k as i32).map(|(_, a)| a))
            .collect();
        assert_eq!(
            keys,
            vec![
                Some(Action::Yes),
                Some(Action::Maybe),
                Some(Action::No),
                Some(Action::Replay),
                Some(Action::Back)
            ]
        );
        // Upper case never comes from the engine; Shift is its own key.
        assert_eq!(action_for(b'Y' as i32), None);
        for a in ACTIONS {
            assert_eq!(Action::from_code(a.code()), Some(a));
        }
        assert_eq!(Action::from_code(0), None);
    }

    #[test]
    fn the_keys_line_names_every_letter_key() {
        let letters: String = KEY_LINE.iter().map(|(k, _)| *k).collect();
        let codes: String = KEY_CODES[..5].iter().map(|&k| k as u8 as char).collect();
        assert_eq!(letters.to_ascii_lowercase(), codes);
    }
}
