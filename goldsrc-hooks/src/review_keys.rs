//! The review's on-screen panel and answer keys (#660): while
//! `ds_demo_review` plays a highlight, a small box on the game screen says
//! which one it is and lists the keys that answer it, so an answer doesn't
//! need the ESC menu, nor waiting for the highlight to end (#1141). Under
//! its title the box lists the highlight's kills, time, victim and weapon,
//! with the next one to come in the key colour behind a `>` (#1140).
//!
//! ## The keys
//!
//! One key for each of the Review tab's buttons, in its order ([`KEYS_LIST`]):
//! Y Yes, M Maybe, N No, R Replay, B Back, Space Next, S Stop. Next has no
//! letter of its own (N is No), so it takes Space, the "press Space to go
//! on" key, which only skips: B brings the highlight back. ESC brings up the
//! menu as always, and the review opens the DoD Studio window on its Review
//! tab with it, for the kill range and the note: vgui2 takes ESC before any
//! client hook sees it (seen live on PRE), so the review watches the menu
//! come up instead (`review::poll`). The keys are taken while the review
//! has a highlight loading, playing or waiting at its end for an answer
//! ([`MODE`]) and `ds_ui_review_keys` is on; at any other time every key goes
//! to the game as before. A taken key never reaches its bind (Y is
//! `messagemode`, Space `+jump` and S `+moveback` in DoD's default config).
//!
//! For [`review::LOCKOUT`](crate::review) after a highlight starts playing
//! (after an answer, Replay, Back or Next too, since each starts one) the
//! keys are taken but do nothing, and the box greys them ([`KeyMode::Locked`]):
//! a double press on the last highlight's answer can't answer the next one
//! before it's been seen (#1141). Taken, not passed on, so that second press
//! doesn't open chat or jump either.
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
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use crate::demo_list_folders::Setting;
use crate::names::console_name;

/// `ds_ui_review_keys 1` (default): while a highlight plays and at its end,
/// Y, M, N, R, B, Space or S answer it on the game screen, and at its end the
/// review waits there instead of opening the Review tab, which ESC opens.
/// `0` opens the tab at each end, as before #660.
pub static KEYS: Setting = Setting::new(console_name!("ui_review_keys"), true);
/// `ds_ui_review_info 1` (default): while a highlight loads and plays, the
/// box on screen says which one it is, with the answer keys while
/// `ds_ui_review_keys` is on. The answer keys' box shows at a highlight's end
/// either way; with this off, the keys still answer while a highlight plays,
/// only unseen.
pub static INFO: Setting = Setting::new(console_name!("ui_review_info"), true);

pub fn keys_status() -> String {
    if KEYS.on() {
        "while a highlight plays and at its end, Y M N R B Space S answer it on screen, ESC opens the Review tab"
            .to_string()
    } else {
        "at a highlight's end, the Review tab opens for the answer".to_string()
    }
}

pub fn info_status() -> String {
    if INFO.on() {
        "while a highlight plays, a box on screen says which one it is, with the answer keys"
            .to_string()
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
    Next,
    Stop,
}

impl Action {
    fn code(self) -> u32 {
        self as u32 + 1
    }

    fn from_code(code: u32) -> Option<Action> {
        KEYS_LIST
            .iter()
            .map(|k| k.action)
            .find(|a| a.code() == code)
    }
}

/// One key: the engine's key number, its name on the box, the button's word
/// and what it does.
pub(crate) struct Key {
    code: i32,
    pub label: &'static str,
    pub word: &'static str,
    action: Action,
}

/// The keys, in the Review tab's button order (`ui/Review.res`), which the
/// box lists them in too. The engine's key number for a letter is its
/// lower-case ASCII code, for Space 32.
pub(crate) const KEYS_LIST: [Key; 7] = [
    key(b'y', "Y", "Yes", Action::Yes),
    key(b'm', "M", "Maybe", Action::Maybe),
    key(b'n', "N", "No", Action::No),
    key(b'r', "R", "Replay", Action::Replay),
    key(b'b', "B", "Back", Action::Back),
    key(b' ', "Space", "Next", Action::Next),
    key(b's', "S", "Stop", Action::Stop),
];

const fn key(code: u8, label: &'static str, word: &'static str, action: Action) -> Key {
    Key {
        code: code as i32,
        label,
        word,
        action,
    }
}

/// The action a key stands for, and its place in [`KEYS_LIST`].
fn action_for(key: i32) -> Option<(usize, Action)> {
    KEYS_LIST
        .iter()
        .position(|k| k.code == key)
        .map(|i| (i, KEYS_LIST[i].action))
}

/// Whether the answer keys are the review's now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyMode {
    /// Every key is the game's: no review highlight on, or
    /// `ds_ui_review_keys 0`.
    Off,
    /// The keys are taken but do nothing: a highlight is loading, or has only
    /// just started (#1141's lockout).
    Locked,
    /// A key answers.
    Live,
}

impl KeyMode {
    fn code(self) -> u8 {
        self as u8
    }

    fn from_code(code: u8) -> KeyMode {
        match code {
            1 => KeyMode::Locked,
            2 => KeyMode::Live,
            _ => KeyMode::Off,
        }
    }
}

/// The [`KeyMode`], set by the review's poll every frame.
static MODE: AtomicU8 = AtomicU8::new(0);
/// The action a key asked for, for the next poll; 0 for none.
static PENDING: AtomicU32 = AtomicU32::new(0);
/// One bit per key in [`KEYS_LIST`] whose press was taken and not yet
/// released.
static HELD: AtomicU32 = AtomicU32::new(0);

/// What happens to one key event, given the [`KeyMode`] and which taken keys
/// are still down. Returns whether the key is swallowed, the action to run,
/// and the new held set.
fn decide(down: bool, key: i32, mode: KeyMode, held: u32) -> (bool, Option<Action>, u32) {
    let Some((index, action)) = action_for(key) else {
        return (false, None, held);
    };
    let bit = 1u32 << index;
    if held & bit != 0 {
        // The rest of a press already taken: autorepeat or the release.
        let held = if down { held } else { held & !bit };
        return (true, None, held);
    }
    match mode {
        KeyMode::Live if down => (true, Some(action), held | bit),
        KeyMode::Locked if down => (true, None, held | bit),
        _ => (false, None, held),
    }
}

/// `HUD_Key_Event`'s check: `true` swallows the key. One atomic load for a
/// key that isn't one of ours.
pub fn key_event(down: i32, key: i32) -> bool {
    if action_for(key).is_none() {
        return false;
    }
    let held = HELD.load(Ordering::Relaxed);
    let mode = KeyMode::from_code(MODE.load(Ordering::Relaxed));
    let (swallow, action, held) = decide(down != 0, key, mode, held);
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
    /// Whether the keys answer now; `false` greys them all, during the
    /// lockout after a highlight starts (#1141).
    pub live: bool,
    /// Whether there is a highlight before this one; B Back is greyed when
    /// not, since it does nothing on the first.
    pub can_go_back: bool,
    /// The highlight's kills, one line each (#1140); empty lists none.
    pub kills: Vec<KillLine>,
}

/// Where a kill is against the demo clock, which sets how its line is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KillState {
    /// Already played: dimmed.
    Past,
    /// The next one to come: stood out.
    Next,
    /// Still to come after the next one.
    Later,
}

/// One kill on the box: `2  5:56  gskiLL  K98`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct KillLine {
    pub text: String,
    pub state: KillState,
}

impl Panel {
    /// Whether a key does something now, so the box draws it in full colour.
    pub(crate) fn key_live(&self, action: Action) -> bool {
        self.live && (action != Action::Back || self.can_go_back)
    }
}

static PANEL: RwLock<Option<Panel>> = RwLock::new(None);

/// Called by the review's poll every frame with what the box should show
/// (`None` hides it) and what the keys do now.
pub(crate) fn publish(panel: Option<Panel>, mode: KeyMode) {
    MODE.store(mode.code(), Ordering::Relaxed);
    if mode != KeyMode::Live {
        // A key pressed after the review moved on is dropped.
        PENDING.store(0, Ordering::Relaxed);
    }
    let same = PANEL.read().map(|p| *p == panel).unwrap_or(false);
    if !same && let Ok(mut p) = PANEL.write() {
        *p = panel;
    }
}

/// The line under the keys: the key, in the key colour, then what it does.
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

    use super::{ESC_LINE, KEYS_LIST, KillState, Panel};

    const WHITE: (f32, f32, f32) = (1.0, 1.0, 1.0);
    const GREY: (f32, f32, f32) = (0.75, 0.75, 0.75);
    /// The keys, in DoD's HUD amber.
    const KEY: (f32, f32, f32) = (1.0, 0.75, 0.25);
    /// A key that does nothing now, and its word (B Back on the first
    /// highlight): greyed rather than hidden, so the keys keep their places.
    const DIM: (f32, f32, f32) = (0.4, 0.4, 0.4);
    /// A kill already played (#1140): readable, but quieter than the rest.
    const PAST: (f32, f32, f32) = (0.55, 0.55, 0.55);
    /// The mark in front of the next kill, and the gap that keeps the other
    /// lines in step with it.
    const NEXT_MARK: &str = "> ";
    const NO_MARK: &str = "   ";
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
        // The console font has one size and no bold, so the next kill stands
        // out by colour and a mark (#1140).
        for kill in &panel.kills {
            let (mark, colour) = match kill.state {
                KillState::Next => (NEXT_MARK, KEY),
                KillState::Later => (NO_MARK, WHITE),
                KillState::Past => (NO_MARK, PAST),
            };
            lines.push(
                run(&format!("{mark}{}", kill.text), colour)
                    .into_iter()
                    .collect(),
            );
        }
        if panel.keys {
            let mut keys = Vec::new();
            // Each key in the key colour, then its button's word.
            for (i, k) in KEYS_LIST.iter().enumerate() {
                let gap = if i == 0 { "" } else { "    " };
                let (key, word) = if panel.key_live(k.action) {
                    (KEY, WHITE)
                } else {
                    (DIM, DIM)
                };
                keys.extend(run(&format!("{gap}{}", k.label), key));
                keys.extend(run(&format!(" {}", k.word), word));
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
    fn the_keys_answer_only_while_live() {
        assert_eq!(decide(true, Y, KeyMode::Off, 0), (false, None, 0));
        assert_eq!(
            decide(true, Y, KeyMode::Live, 0),
            (true, Some(Action::Yes), 1)
        );
        assert_eq!(
            decide(true, b'b' as i32, KeyMode::Live, 0),
            (true, Some(Action::Back), 1 << 4)
        );
        // Any other key goes to the game, live or not; ESC included,
        // which vgui2 takes before this hook anyway.
        assert_eq!(decide(true, 27, KeyMode::Live, 0), (false, None, 0));
        assert_eq!(
            decide(true, b'w' as i32, KeyMode::Live, 0),
            (false, None, 0)
        );
        assert_eq!(
            decide(true, b'e' as i32, KeyMode::Live, 0),
            (false, None, 0)
        );
    }

    #[test]
    fn a_taken_press_stays_taken_until_its_release() {
        // Y answered the last highlight and the review ended, but Y is still
        // down: its autorepeat and its release must not reach `messagemode`.
        let (_, _, held) = decide(true, Y, KeyMode::Live, 0);
        assert_eq!(decide(true, Y, KeyMode::Off, held), (true, None, held));
        assert_eq!(decide(false, Y, KeyMode::Off, held), (true, None, 0));
        // The next press, with no review on, is the game's again.
        assert_eq!(decide(true, Y, KeyMode::Off, 0), (false, None, 0));
    }

    #[test]
    fn a_press_during_the_lockout_is_taken_but_does_nothing() {
        // The second press of a double Y: neither an answer nor chat.
        let (swallow, action, held) = decide(true, Y, KeyMode::Locked, 0);
        assert!(swallow);
        assert_eq!(action, None);
        // Still down when the lockout ends: its autorepeat doesn't answer,
        // its release is taken, and only a fresh press answers.
        assert_eq!(decide(true, Y, KeyMode::Live, held), (true, None, held));
        assert_eq!(decide(false, Y, KeyMode::Live, held), (true, None, 0));
        assert_eq!(
            decide(true, Y, KeyMode::Live, 0),
            (true, Some(Action::Yes), 1)
        );
        // Any other key is the game's during the lockout too.
        assert_eq!(
            decide(true, b'w' as i32, KeyMode::Locked, 0),
            (false, None, 0)
        );
    }

    #[test]
    fn a_key_mode_survives_its_atomic() {
        for mode in [KeyMode::Off, KeyMode::Locked, KeyMode::Live] {
            assert_eq!(KeyMode::from_code(mode.code()), mode);
        }
    }

    #[test]
    fn the_lockout_greys_every_key() {
        let panel = Panel {
            title: "t".into(),
            keys: true,
            live: false,
            can_go_back: true,
            ..Panel::default()
        };
        for k in &KEYS_LIST {
            assert!(!panel.key_live(k.action));
        }
    }

    #[test]
    fn a_release_alone_is_never_taken() {
        // Pressed before the review took the keys, released after.
        assert_eq!(decide(false, Y, KeyMode::Live, 0), (false, None, 0));
    }

    #[test]
    fn each_key_has_its_own_action() {
        let keys: Vec<_> = b"ymnrb s"
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
                Some(Action::Back),
                Some(Action::Next),
                Some(Action::Stop)
            ]
        );
        // Upper case never comes from the engine; Shift is its own key.
        assert_eq!(action_for(b'Y' as i32), None);
        for k in &KEYS_LIST {
            assert_eq!(Action::from_code(k.action.code()), Some(k.action));
        }
        assert_eq!(Action::from_code(0), None);
    }

    #[test]
    fn next_and_stop_are_taken_only_during_a_review_highlight() {
        let space = b' ' as i32;
        let s = b's' as i32;
        // Space is +jump and S +moveback with no review highlight on.
        assert_eq!(decide(true, space, KeyMode::Off, 0), (false, None, 0));
        assert_eq!(decide(true, s, KeyMode::Off, 0), (false, None, 0));
        assert_eq!(
            decide(true, space, KeyMode::Live, 0),
            (true, Some(Action::Next), 1 << 5)
        );
        assert_eq!(
            decide(true, s, KeyMode::Live, 0),
            (true, Some(Action::Stop), 1 << 6)
        );
        // Next held past the move on: its release is taken, not +jump's.
        assert_eq!(decide(false, space, KeyMode::Off, 1 << 5), (true, None, 0));
    }

    #[test]
    fn every_key_and_label_is_its_own() {
        for (i, a) in KEYS_LIST.iter().enumerate() {
            for b in &KEYS_LIST[i + 1..] {
                assert_ne!(a.code, b.code, "{} and {}", a.label, b.label);
                assert_ne!(a.label, b.label);
                assert_ne!(a.action, b.action);
            }
            // A letter's name on the box is the letter itself.
            if a.label.len() == 1 {
                assert_eq!(a.label.to_ascii_lowercase().as_bytes()[0] as i32, a.code);
            }
        }
        // The held set has a bit for each key.
        assert!(KEYS_LIST.len() <= 32);
    }

    #[test]
    fn the_keys_follow_the_review_tabs_buttons() {
        // Review.res lists its buttons in order; the box lists the keys in
        // the same order, each under the button's own word.
        let res = include_str!("../ui/Review.res");
        let mut at = 0;
        for k in &KEYS_LIST {
            let label = format!("\"labelText\"\t\t\"{}\"", k.word);
            let found = res[at..]
                .find(&label)
                .unwrap_or_else(|| panic!("no {} button after the previous key's", k.word));
            at += found + label.len();
        }
        // And no button is left without a key.
        assert_eq!(res.matches("\"Button\"").count(), KEYS_LIST.len());
    }
}
