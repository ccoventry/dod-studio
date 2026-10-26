//! `clear` and `clear_on`: emptying the kill feed, now or when a round
//! restarts or the watched player dies.
//!
//! ## How a line goes
//!
//! Nothing here deletes a slot. `Draw` already drops every notice whose
//! `flDisplayTime` is older than the HUD time it is handed (`fcomp` at
//! `+0x2af54`; equal is kept), memmoving the rest up and the spare slot in
//! behind them. So a cleared line is one whose stamp is pushed into the past
//! ([`EXPIRED`]), and the game's own next `Draw` takes it out exactly as if its
//! time had run out. The array stays in the shape the game keeps it in, which
//! is what `max`, `layout even`, `scale`, `font`, `outline` and `background`
//! all read it in, and a line that arrives in the same frame lands after the
//! expired ones and survives.
//!
//! ## Which lines
//!
//! A clear that waits (`clear_on ... <delay>`) takes the lines that were up
//! when its event happened, not whatever is up when it fires: a kill that
//! arrives during the delay is a new event, and having it vanish a moment
//! after it appeared looks like a glitch. Stamps are `m_flTime + lifetime`,
//! so a line from before the event never has a later stamp than one after it:
//! the event's highest stamp is the cut-off.
//!
//! ## The events
//!
//! - **restart**: `RoundState` 0, the reset that starts every new round. DoD
//!   sends it ~3 s after a round win (5 s on some servers), when a clan match
//!   goes live (both teams ready, the 10 s count), and every 20 s while a
//!   server waits for players. Measured over 176 WSOD25 demos, POV and HLTV;
//!   the PR that added this lists the counts.
//! - **death**: a `DeathMsg` whose victim is the player the view belongs to:
//!   the recorder of a POV demo, or the player an HLTV demo watches in eye.
//!   Its own line counts as up at the death, so it goes too.
//!
//! All of it runs on the engine's thread: user messages, console commands and
//! `HUD_Frame` alike. The state is atomics, so nothing here can block a frame.

use std::ffi::{CString, c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::engine;

use super::{COMMAND, FIELD_DISPLAY_TIME, FIELD_ID, ITEM, current_array};

/// A stamp `Draw` always finds older than its HUD time.
const EXPIRED: f32 = -1.0e9;

/// The longest delay accepted, in seconds: long enough for any cut, short
/// enough that a typo like `20000` is refused rather than silently waiting.
pub(super) const MAX_DELAY: f32 = 600.0;

/// `fadd dword ptr [&gHUD.m_flTime]` in `MsgFunc_DeathMsg`, the stamp's own
/// clock: the opcode at this RVA, the address after it.
const HUD_TIME_FADD_RVA: usize = 0x2_b3c6;
const FADD_M32: [u8; 2] = [0xd8, 0x05];
/// `gHUD.m_flTime`, which the operand above must point at.
const HUD_TIME_RVA: usize = 0x10_80cc;

// `Draw` keeps a line whose stamp equals its time, so an expired stamp must be
// strictly less than any, even for a demo whose clock starts at 0.
const _: () = assert!(EXPIRED < 0.0 && EXPIRED.is_finite());
// `fadd m32` is two opcode bytes then the address, inside the stamping code
// that runs from +0x2b3be to the store at +0x2b3cc.
const _: () = assert!(HUD_TIME_FADD_RVA + 6 <= 0x2_b3cc);

/// One `clear_on` trigger: its setting, and a clear it has waiting.
struct Trigger {
    /// `restart` or `death`, for the log.
    what: &'static str,
    on: AtomicBool,
    /// Seconds after the event, as `f32` bits.
    delay: AtomicU32,
    armed: AtomicBool,
    /// HUD time of the event, the time to clear at, and the highest stamp up
    /// at the event, all `f32` bits.
    set_at: AtomicU32,
    fire_at: AtomicU32,
    cutoff: AtomicU32,
}

impl Trigger {
    const fn new(what: &'static str) -> Self {
        Self {
            what,
            on: AtomicBool::new(false),
            delay: AtomicU32::new(0),
            armed: AtomicBool::new(false),
            set_at: AtomicU32::new(0),
            fire_at: AtomicU32::new(0),
            cutoff: AtomicU32::new(0),
        }
    }

    fn enabled(&self) -> bool {
        self.on.load(Ordering::Relaxed)
    }

    fn delay(&self) -> f32 {
        f32::from_bits(self.delay.load(Ordering::Relaxed))
    }

    fn set(&self, on: bool, delay: f32) {
        self.on.store(on, Ordering::Relaxed);
        self.delay.store(delay.to_bits(), Ordering::Relaxed);
        if !on {
            self.armed.store(false, Ordering::Relaxed);
        }
    }

    /// The event happened at HUD time `now`, with the feed's highest stamp
    /// `cutoff` (`None`: the feed is empty, so there is nothing to clear).
    fn fire(&self, now: Option<f32>, cutoff: Option<f32>) {
        if !self.enabled() {
            return;
        }
        let Some(cutoff) = cutoff else {
            log(&format!(
                "clear_on {}: the feed is already empty",
                self.what
            ));
            return;
        };
        let delay = self.delay();
        let (Some(now), true) = (now, delay > 0.0) else {
            self.log(now, expire_up_to(cutoff));
            return;
        };
        log(&format!(
            "clear_on {}: at HUD time {now:.2}, the lines up now go at {:.2}",
            self.what,
            now + delay
        ));
        // A second event before the first one's clear keeps the first one's
        // lines in the cut.
        let cutoff = if self.armed.load(Ordering::Relaxed) {
            cutoff.max(f32::from_bits(self.cutoff.load(Ordering::Relaxed)))
        } else {
            cutoff
        };
        self.set_at.store(now.to_bits(), Ordering::Relaxed);
        self.fire_at
            .store((now + delay).to_bits(), Ordering::Relaxed);
        self.cutoff.store(cutoff.to_bits(), Ordering::Relaxed);
        self.armed.store(true, Ordering::Relaxed);
    }

    /// Every frame: clears once the delay has run.
    fn poll(&self, now: Option<f32>) {
        if !self.armed.load(Ordering::Relaxed) {
            return;
        }
        let Some(now) = now else {
            return;
        };
        let set_at = f32::from_bits(self.set_at.load(Ordering::Relaxed));
        let fire_at = f32::from_bits(self.fire_at.load(Ordering::Relaxed));
        match decide(now, set_at, fire_at) {
            Decision::Wait => {}
            Decision::Fire => {
                self.armed.store(false, Ordering::Relaxed);
                let count = expire_up_to(f32::from_bits(self.cutoff.load(Ordering::Relaxed)));
                self.log(Some(now), count);
            }
            Decision::Drop => {
                self.armed.store(false, Ordering::Relaxed);
                log(&format!(
                    "clear_on {}: dropped, the HUD clock went back to {now:.2}",
                    self.what
                ));
            }
        }
    }

    fn log(&self, now: Option<f32>, count: usize) {
        let when = now.map_or("an unknown HUD time".to_string(), |t| {
            format!("HUD time {t:.2}")
        });
        log(&format!(
            "clear_on {}: cleared {count} line(s) at {when}",
            self.what
        ));
    }

    fn describe(&self, what: &str) -> String {
        match (self.enabled(), self.delay()) {
            (false, _) => format!("{what} off"),
            (true, d) if d > 0.0 => format!("{what} after {d} s"),
            (true, _) => format!("{what} at once"),
        }
    }
}

static RESTART: Trigger = Trigger::new("restart");
static DEATH: Trigger = Trigger::new("death");

/// The hook log only, never the console: a clear is visible on screen.
fn log(line: &str) {
    unsafe { crate::debug::report(&format!("deathmsg: {line}")) };
}

#[derive(Debug, PartialEq, Eq)]
enum Decision {
    Wait,
    Fire,
    Drop,
}

/// What a waiting clear does at HUD time `now`. The HUD clock going back past
/// the event means another demo, or a seek to before it: its lines are not
/// the ones up now, so the clear is dropped rather than taking new ones.
fn decide(now: f32, set_at: f32, fire_at: f32) -> Decision {
    if now < set_at {
        Decision::Drop
    } else if now >= fire_at {
        Decision::Fire
    } else {
        Decision::Wait
    }
}

/// `gHUD.m_flTime`: the clock `flDisplayTime` is stamped in and `Draw`
/// compares against. `None` unless `MsgFunc_DeathMsg`'s own `fadd` still
/// reads it at the address this module expects.
fn hud_time() -> Option<f32> {
    let base = engine::client_module_base()?;
    // Safety: both RVAs are inside client.dll's code, which `deathmsg`'s
    // stock check covers; the operand is read before it is trusted.
    let (opcode, operand) = unsafe {
        (
            std::ptr::read_unaligned((base + HUD_TIME_FADD_RVA) as *const [u8; 2]),
            std::ptr::read_unaligned((base + HUD_TIME_FADD_RVA + 2) as *const u32),
        )
    };
    let address = base + HUD_TIME_RVA;
    if opcode != FADD_M32 || operand as usize != address {
        return None;
    }
    // Safety: the game's own code reads this float at this address.
    Some(unsafe { std::ptr::read_volatile(address as *const f32) })
}

/// Calls `each` with every occupied slot's address, in order. The game keeps
/// them packed from slot 0, so the first empty one ends the list.
fn for_each_line(mut each: impl FnMut(usize)) {
    let Some(base) = engine::client_module_base() else {
        return;
    };
    let (array, slots) = current_array(base);
    for slot in 0..slots as usize {
        let item = array + slot * ITEM;
        // Safety: `array` is the live array with `slots` slots, the leaked
        // buffer or client.dll's own `.data`.
        if unsafe { std::ptr::read_unaligned((item + FIELD_ID) as *const i32) } == 0 {
            break;
        }
        each(item);
    }
}

fn stamp(item: usize) -> f32 {
    // Safety: `item` is a slot `for_each_line` handed out.
    unsafe { std::ptr::read_unaligned((item + FIELD_DISPLAY_TIME) as *const f32) }
}

/// The highest stamp among the lines up now, or `None` for an empty feed.
fn newest_stamp() -> Option<f32> {
    let mut newest = None::<f32>;
    for_each_line(|item| {
        let s = stamp(item);
        newest = Some(newest.map_or(s, |n| n.max(s)));
    });
    newest
}

/// Which of `stamps` a clear with this cut-off takes.
fn cleared(stamps: &[f32], cutoff: f32) -> Vec<bool> {
    stamps.iter().map(|&s| s <= cutoff).collect()
}

/// Expires every line stamped no later than `cutoff`. Returns how many.
fn expire_up_to(cutoff: f32) -> usize {
    let mut items = Vec::new();
    let mut stamps = Vec::new();
    for_each_line(|item| {
        items.push(item);
        stamps.push(stamp(item));
    });
    let mut count = 0;
    for (item, clear) in items.into_iter().zip(cleared(&stamps, cutoff)) {
        if clear {
            // Safety: as `stamp`.
            unsafe { std::ptr::write_unaligned((item + FIELD_DISPLAY_TIME) as *mut f32, EXPIRED) };
            count += 1;
        }
    }
    count
}

/// `clear`: every line goes on the next frame.
pub(super) fn clear_now() -> String {
    match expire_up_to(f32::INFINITY) {
        0 => format!("{COMMAND}: the kill feed is already empty\n"),
        1 => format!("{COMMAND}: cleared 1 line\n"),
        n => format!("{COMMAND}: cleared {n} lines\n"),
    }
}

/// Whether `DeathMsg` has to be watched for `clear_on death`.
pub(super) fn wants_death_msg() -> bool {
    DEATH.enabled()
}

/// After a `DeathMsg` has been handed to the game: `victim` as recorded.
pub(super) fn on_death(victim: i32) {
    if !DEATH.enabled() || !is_watched_player(victim) {
        return;
    }
    DEATH.fire(hud_time(), newest_stamp());
}

/// Whether `victim` is the player whose view the demo shows.
fn is_watched_player(victim: i32) -> bool {
    if victim <= 0 {
        return false;
    }
    if super::is_hltv() {
        crate::spectator_target::in_eye_target() == Some(victim)
    } else {
        super::resolve(victim, false).is_own_pov
    }
}

/// `RoundState`'s first byte: 0 is the reset that starts a round.
const ROUND_RESET: u8 = 0;

/// A `RoundState` payload, from our handler or `ds_debug_msglog`'s.
pub(crate) fn on_round_state(payload: &[u8]) {
    if payload.first() == Some(&ROUND_RESET) {
        RESTART.fire(hud_time(), newest_stamp());
    }
}

const ROUND_STATE: &str = "RoundState";

unsafe extern "C" fn hooked_round_state(name: *const c_char, size: i32, buf: *mut c_void) -> i32 {
    let handled = match crate::msglog::original_thunk(ROUND_STATE) {
        Some(original) => unsafe { original(name, size, buf) },
        // 1 is what the engine's own dispatcher treats as handled.
        None => 1,
    };
    if size > 0 && !buf.is_null() {
        // Safety: the engine hands over `size` bytes of payload.
        on_round_state(unsafe { std::slice::from_raw_parts(buf as *const u8, size as usize) });
    }
    handled
}

/// Every frame, from `deathmsg::poll`.
pub(super) fn poll() {
    if RESTART.enabled() && !crate::msglog::watching(ROUND_STATE) {
        // Same shape as `deathmsg::install_hook`: the engine returns early
        // while ours is already the head record, and re-prepending is how it
        // survives the list being freed on disconnect. `ds_debug_msglog`
        // calls `on_round_state` itself while it watches the message.
        if let (Some(engfuncs), Ok(name)) = (engine::engfuncs(), CString::new(ROUND_STATE)) {
            unsafe { (engfuncs.pfn_hook_user_msg)(name.as_ptr(), hooked_round_state) };
        }
    }
    if !RESTART.armed.load(Ordering::Relaxed) && !DEATH.armed.load(Ordering::Relaxed) {
        return;
    }
    let now = hud_time();
    RESTART.poll(now);
    DEATH.poll(now);
}

/// For `status`.
pub(super) fn describe() -> String {
    format!(
        "clear on {}, {}",
        RESTART.describe("restart"),
        DEATH.describe("death")
    )
}

/// `clear_on <restart|death> <0|1> [delay]`, or `clear_on default`.
#[derive(Debug, PartialEq)]
pub(super) enum ClearOn {
    Restart(bool, f32),
    Death(bool, f32),
    Default,
}

pub(super) fn parse_clear_on(rest: &[String]) -> Result<ClearOn, String> {
    let usage = || {
        format!(
            "expected restart <0|1> [delay], death <0|1> [delay] (delay in seconds, 0..{MAX_DELAY}), or default"
        )
    };
    let Some(which) = rest.first() else {
        return Err(usage());
    };
    if which.eq_ignore_ascii_case("default") && rest.len() == 1 {
        return Ok(ClearOn::Default);
    }
    let on = match rest.get(1).map(String::as_str) {
        Some("1") => true,
        Some("0") => false,
        _ => return Err(usage()),
    };
    let delay = match rest.get(2) {
        None => 0.0,
        Some(text) => match text.parse::<f32>() {
            Ok(d) if (0.0..=MAX_DELAY).contains(&d) => d,
            _ => {
                return Err(format!(
                    "the delay is seconds, 0..{MAX_DELAY}, not {text:?}"
                ));
            }
        },
    };
    if rest.len() > 3 {
        return Err(usage());
    }
    match which.to_ascii_lowercase().as_str() {
        "restart" => Ok(ClearOn::Restart(on, delay)),
        "death" => Ok(ClearOn::Death(on, delay)),
        _ => Err(usage()),
    }
}

pub(super) fn apply(asked: ClearOn) -> String {
    match asked {
        ClearOn::Restart(on, delay) => RESTART.set(on, delay),
        ClearOn::Death(on, delay) => {
            DEATH.set(on, delay);
            if on {
                // The DeathMsg handler is what sees the death.
                super::install_hook();
            }
        }
        ClearOn::Default => {
            RESTART.set(false, 0.0);
            DEATH.set(false, 0.0);
        }
    }
    describe()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn clear_on_takes_a_toggle_and_an_optional_delay() {
        assert_eq!(
            parse_clear_on(&args(&["restart", "1"])),
            Ok(ClearOn::Restart(true, 0.0))
        );
        assert_eq!(
            parse_clear_on(&args(&["restart", "1", "2"])),
            Ok(ClearOn::Restart(true, 2.0))
        );
        assert_eq!(
            parse_clear_on(&args(&["DEATH", "1", "1.5"])),
            Ok(ClearOn::Death(true, 1.5))
        );
        assert_eq!(
            parse_clear_on(&args(&["death", "0"])),
            Ok(ClearOn::Death(false, 0.0))
        );
        assert_eq!(parse_clear_on(&args(&["default"])), Ok(ClearOn::Default));
    }

    #[test]
    fn clear_on_refuses_what_it_cannot_mean() {
        for bad in [
            &[][..],
            &["restart"][..],
            &["restart", "2"][..],
            &["restart", "on"][..],
            &["restart", "1", "-1"][..],
            &["restart", "1", "601"][..],
            &["restart", "1", "NaN"][..],
            &["restart", "1", "2", "3"][..],
            &["round", "1"][..],
            &["default", "1"][..],
        ] {
            assert!(parse_clear_on(&args(bad)).is_err(), "{bad:?} parsed");
        }
    }

    #[test]
    fn a_waiting_clear_fires_on_time_and_drops_when_the_clock_goes_back() {
        assert_eq!(decide(10.0, 10.0, 12.0), Decision::Wait);
        assert_eq!(decide(11.99, 10.0, 12.0), Decision::Wait);
        assert_eq!(decide(12.0, 10.0, 12.0), Decision::Fire);
        assert_eq!(decide(40.0, 10.0, 12.0), Decision::Fire, "a seek past it");
        assert_eq!(decide(3.0, 10.0, 12.0), Decision::Drop, "another demo");
    }

    #[test]
    fn a_clear_takes_the_lines_up_at_the_event_and_keeps_later_ones() {
        // Three lines up at the event (newest stamp 25.0), then one more
        // during the delay.
        let stamps = [20.0, 22.5, 25.0, 27.0];
        assert_eq!(cleared(&stamps, 25.0), vec![true, true, true, false]);
        assert_eq!(cleared(&stamps, f32::INFINITY), vec![true; 4], "clear");
    }
}
