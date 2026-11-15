//! `ds_view_hide_headshot_red_screen` and `ds_view_hide_screen_shake`: keep two
//! server effects out of a POV demo's picture (#606).
//!
//! ## What they are
//!
//! Both are user messages the server sent to the recording player, so they are
//! in the demo and replay with it (43 POV demos scanned, see #606):
//!
//! - **The red head-hit flash:** a `ScreenFade` of colour (128,0,0) at alpha
//!   200, flags 0, 3 s fade with a 2 s hold. It comes with every hit on the
//!   head hitbox, fatal or not, grenades included. The only other fades in
//!   the demos are black: the clear at respawn (alpha 0) and a rare end-of-map
//!   one, which this leaves alone.
//! - **The blast shake:** a `ScreenShake`, always amplitude 16 (0xffff in 4.12 fixed point), 1 s, from
//!   grenades going off near the player (no WSOD25 map has an `env_shake`).
//!
//! ## How
//!
//! `client.dll` registers neither message: `hw.dll` hooks both itself in
//! `ClientDLL_Init` (`V_ScreenFade` pre-Anniversary `hw.dll+0xc2580`, 25th
//! Anniversary `+0x232160`; `V_ScreenShake` `+0xc24e0` / `+0x2322c0`). The
//! engine's own handler is found at the registration site (`push handler;
//! push "ScreenFade"; call HookUserMsg`), and ours is hooked in front of it
//! the way `deathmsg.rs` does for `DeathMsg`: `pfnHookUserMsg` prepends, the
//! dispatcher stops at the first match, and re-hooking every frame survives a
//! reconnect. Everything that isn't dropped goes to the engine's handler
//! untouched. `tools/verify_screen_effects_offsets.py` checks the lookup on
//! both builds.
//!
//! ## The shake is also in the recorded view (#910)
//!
//! Dropping the message stops the live shake, but a POV demo doesn't draw
//! the live view: the demo view override puts the recorded `ref_params_s`
//! over it (`pov_live_bob.rs`), and the recorder's own client had already
//! shaken that one. Measured over 36 POV demos from 24 players (the
//! `solifotw-*` set, `analysis/examples/screen_shake_probe.rs`): inside a
//! shake the recorded eye sits a median 2 units (p95 13, up to 30) off
//! `simorg + viewheight`, jumping every frame on all three axes, with the
//! roll jumping by up to 6 degrees; outside one the forward/up offset and the
//! roll are 0 on most frames. The recorded gun moves with it (by about 0.9
//! of the camera's shake).
//!
//! So while a dropped shake runs, [`unshake`] (called from the override's
//! redirected call) rebuilds the recorded view's eye from what the live
//! `V_CalcRefdef` put on top of its own eye, which never saw the shake:
//! `vieworg = recorded simorg + viewheight + (live vieworg - live simorg -
//! live viewheight)`. The live offset keeps the stair smoothing; its sideways
//! bob is scaled to the recorder's, learnt from the frames outside shakes
//! (recorded over live bob), so a recorder who played without bob gets none
//! for that second either. The roll gets the same treatment, and the gun
//! moves by 0.9 of the camera's correction. It leaves the view alone while
//! the recorder (or the live view) is dead or spectating (the death view
//! freezes, it doesn't shake), when the live eye offset is more than bob and
//! stair smoothing, and on any frame where the correction would be bigger
//! than the shake itself could be.
//!
//! Measured in game on both builds (solifotw-greetz1, five blasts): the drawn
//! eye moved a p95 6 units per frame with the setting at 0 (and at 1 before
//! this), and 0.9 with it on; the roll p95 1.1 degrees against 0.

use std::ffi::{CString, c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::engine::{self, UserMsgHookFn};
use crate::names::console_name;
use crate::pov_live_bob::View;
use crate::setting::Cvar;

pub const FLASH_NAME: &str = console_name!("view_hide_headshot_red_screen");
pub const SHAKE_NAME: &str = console_name!("view_hide_screen_shake");

pub static FLASH: Cvar = Cvar::new();
pub static SHAKE: Cvar = Cvar::new();

/// The engine's handlers; 0 before a look, 1 when one couldn't be found.
static FADE_HANDLER: AtomicUsize = AtomicUsize::new(0);
static SHAKE_HANDLER: AtomicUsize = AtomicUsize::new(0);
/// Set once either cvar has been on: from then on our handlers stay in front
/// (forwarding everything while the cvars are 0).
static WANTED: AtomicBool = AtomicBool::new(false);

pub fn shake_on() -> bool {
    SHAKE.on()
}

/// The first 4 bytes (amplitude, duration) of the last `ScreenShake` dropped
/// and not yet seen by [`unshake`]; 0 for none.
static PENDING: AtomicU32 = AtomicU32::new(0);
/// [`CALCS`] when [`PENDING`] came in.
static PENDING_AT: AtomicU32 = AtomicU32::new(0);
/// How many times the live `V_CalcRefdef` has run, in any demo or live play.
static CALCS: AtomicU32 = AtomicU32::new(0);
/// How many `V_CalcRefdef` runs a dropped shake may wait for [`unshake`].
/// The message comes in before the frame's view is worked out, so a POV demo
/// takes it one run later; a shake dropped in an HLTV demo or live play,
/// where the override never runs, is stale by the next POV demo and must not
/// open a window there. Room for a few views per frame (HLAE's streams).
const PENDING_RUNS: u32 = 16;
/// The shake being taken out of the recorded view: start and end on the
/// client clock, and its amplitude, as f32 bits.
static WINDOW: [AtomicU32; 3] = [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];

/// How long past a shake's own end to keep correcting the view.
const MARGIN: f32 = 0.1;
/// Room on top of the shake's own reach for what else the eye offset holds
/// (bob, stair smoothing).
const SLACK: f32 = 8.0;
/// How much of the camera's shake `V_CalcRefdef` gives the gun.
const GUN_SHARE: f32 = 0.9;

/// (amplitude, duration in seconds) from a `ScreenShake`'s first 4 bytes:
/// two unsigned 4.12 fixed-point numbers.
fn decode(head: u32) -> (f32, f32) {
    let amplitude = (head & 0xffff) as f32 / 4096.0;
    let duration = (head >> 16) as f32 / 4096.0;
    (amplitude, duration)
}

/// The window after a frame at `now`, with a shake that just came in, if any.
/// A shake during another one stretches it; a clock that went back before
/// the window's start (a seek) leaves it shut.
fn advance(window: [f32; 3], now: f32, new: Option<(f32, f32)>) -> [f32; 3] {
    let [start, end, amplitude] = window;
    match new {
        Some((amp, duration)) if amp > 0.0 && duration > 0.0 => {
            if now >= start && now <= end {
                [start, end.max(now + duration), amplitude.max(amp)]
            } else {
                [now, now + duration, amp]
            }
        }
        _ => window,
    }
}

/// Called by `engine.rs` after every live `V_CalcRefdef`.
pub fn after_calc_refdef() {
    CALCS.fetch_add(1, Ordering::Relaxed);
}

/// Whether a shake dropped at run `at` is still this frame's at run `calcs`.
fn fresh(calcs: u32, at: u32) -> bool {
    calcs.wrapping_sub(at) <= PENDING_RUNS
}

fn in_window(window: [f32; 3], now: f32) -> bool {
    // The clock may step back a little right after the shake came in.
    window[2] > 0.0 && now >= window[0] - MAX_BACK && now <= window[1] + MARGIN
}

fn wrap(degrees: f32) -> f32 {
    (degrees + 180.0).rem_euclid(360.0) - 180.0
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Where `V_CalcRefdef` put the eye: `vieworg - simorg - viewheight`.
fn eye_offset(view: &View) -> [f32; 3] {
    [0, 1, 2].map(|i| view.vieworg[i] - view.simorg[i] - view.viewheight[i])
}

fn alive(view: &View) -> bool {
    view.health > 0 && view.spectator == 0 && view.intermission == 0
}

/// How far the live eye offset may sit from the eye and still be bob and
/// stair smoothing: sideways bob stays within 7 units, the stair smoothing
/// within 18 down.
const LIVE_SIDEWAYS: f32 = 8.0;
const LIVE_DOWN: f32 = 20.0;

/// How fast [`learn`] forgets, per frame.
const BOB_MEMORY: f32 = 0.99;

/// The recorded and live sideways bob, summed over the frames outside a
/// shake, to scale the live bob into the recorder's: a recorder who played
/// with `cl_bob 0` gets none, one with the viewer's `cl_bob` gets the live
/// bob as it is. (sum of recorded x live, sum of live squared), as f32 bits.
static BOB_SUMS: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];

fn learn(sums: [f32; 2], live: &View, recorded: &View) -> [f32; 2] {
    if !alive(live) || !alive(recorded) {
        return sums;
    }
    let live_side = dot(eye_offset(live), live.right);
    let recorded_side = dot(eye_offset(recorded), recorded.right);
    // Written so a NaN fails it too: one would stay in the sums for good.
    if !(live_side.abs() <= LIVE_SIDEWAYS && recorded_side.abs() <= LIVE_SIDEWAYS) {
        return sums;
    }
    [
        sums[0] * BOB_MEMORY + recorded_side * live_side,
        sums[1] * BOB_MEMORY + live_side * live_side,
    ]
}

/// The client clock at the last [`unshake`], as f32 bits.
static LAST_NOW: AtomicU32 = AtomicU32::new(0);
/// The most the clock moves between two frames of one demo played on; at
/// `dem_speed 4` and 25 fps a frame is 0.16 s.
const MAX_STEP: f32 = 1.0;
/// Room for a clock that settles back between frames: in a POV demo the
/// client clock steps back by up to 0.14 s every second or so, in normal
/// play as well as fast-forward (PRE, solifotw-greetz1).
const MAX_BACK: f32 = 0.5;

/// Whether the clock at `now` carries on from `last` in the same demo. A
/// clock that went back or jumped is another demo, a restart or a seek: the
/// window and the learnt bob (maybe another recorder's) start over.
fn continues(last: f32, now: f32) -> bool {
    (-MAX_BACK..=MAX_STEP).contains(&(now - last))
}

/// The recorder's bob over the live one, from [`learn`]'s sums.
fn bob_scale(sums: [f32; 2]) -> f32 {
    if sums[1] > 1e-3 {
        (sums[0] / sums[1]).clamp(0.0, 3.0)
    } else {
        0.0
    }
}

/// The recorded view without the shake: (vieworg, gun shift, roll), or
/// `None` to leave the frame as recorded. `bob_scale` scales the live
/// sideways bob into the recorder's.
fn correction(
    live: &View,
    recorded: &View,
    amplitude: f32,
    bob_scale: f32,
) -> Option<([f32; 3], [f32; 3], f32)> {
    if !alive(recorded) || !alive(live) {
        return None;
    }
    let mut offset = eye_offset(live);
    if offset[0].hypot(offset[1]) > LIVE_SIDEWAYS || offset[2].abs() > LIVE_DOWN {
        // Not an eye the shake shook: the live view is somewhere else.
        return None;
    }
    let side = dot(offset, live.right) * (bob_scale - 1.0);
    for (o, r) in offset.iter_mut().zip(live.right) {
        *o += r * side;
    }
    let vieworg = [0, 1, 2].map(|i| recorded.simorg[i] + recorded.viewheight[i] + offset[i]);
    let fix = [0, 1, 2].map(|i| vieworg[i] - recorded.vieworg[i]);
    let size = dot(fix, fix).sqrt();
    if !size.is_finite() || size > amplitude * 3f32.sqrt() + SLACK {
        return None;
    }
    let roll = recorded.cl_viewangles[2] + wrap(live.viewangles[2] - live.cl_viewangles[2]);
    if !roll.is_finite() {
        return None;
    }
    Some((vieworg, fix.map(|v| v * GUN_SHARE), roll))
}

/// Takes a dropped shake out of the recorded view the demo view override
/// just put in place (`pov_live_bob::after_override`). `live` is the view as
/// the live `V_CalcRefdef` left it, before the override.
pub(crate) fn unshake(live: &View, view: &mut View, gun: &mut [f32; 3]) {
    if !SHAKE.on() {
        return;
    }
    let now = live.time;
    let pending = PENDING.swap(0, Ordering::Relaxed);
    let pending = if fresh(
        CALCS.load(Ordering::Relaxed),
        PENDING_AT.load(Ordering::Relaxed),
    ) {
        pending
    } else {
        0
    };
    let last = f32::from_bits(LAST_NOW.swap(now.to_bits(), Ordering::Relaxed));
    let same_demo = continues(last, now);
    let window = if same_demo {
        WINDOW
            .each_ref()
            .map(|w| f32::from_bits(w.load(Ordering::Relaxed)))
    } else {
        [0.0; 3]
    };
    let window = advance(window, now, (pending != 0).then(|| decode(pending)));
    for (slot, value) in WINDOW.iter().zip(window) {
        slot.store(value.to_bits(), Ordering::Relaxed);
    }
    let sums = if same_demo {
        BOB_SUMS
            .each_ref()
            .map(|s| f32::from_bits(s.load(Ordering::Relaxed)))
    } else {
        for slot in &BOB_SUMS {
            slot.store(0, Ordering::Relaxed);
        }
        [0.0; 2]
    };
    // Spectating a teammate in first person (`g_iUser1`): the recorded eye is
    // theirs while `simorg` is still the recorder's body, so neither the bob
    // nor a shake can be measured from it (as `pov_live_bob::decide`).
    if crate::spectator_target::mode().is_some_and(|mode| mode != 0) {
        return;
    }
    if !in_window(window, now) {
        for (slot, value) in BOB_SUMS.iter().zip(learn(sums, live, view)) {
            slot.store(value.to_bits(), Ordering::Relaxed);
        }
        return;
    }
    if let Some((vieworg, gun_shift, roll)) = correction(live, view, window[2], bob_scale(sums)) {
        view.vieworg = vieworg;
        view.viewangles[2] = roll;
        for i in 0..3 {
            gun[i] += gun_shift[i];
        }
    }
}

/// Whether a `ScreenFade` payload is the head-hit flash: (128,0,0) with flags
/// 0, the only red fade a DoD server sends.
fn is_head_hit_flash(payload: &[u8]) -> bool {
    // u16 duration, u16 hold time, i16 flags, u8 r, g, b, a.
    payload.len() >= 10 && payload[4..6] == [0, 0] && payload[6..9] == [128, 0, 0]
}

/// The handler `hw.dll` registers for `message` (with its trailing NUL), from
/// the `push handler; push "<message>"; call` at its registration.
fn engine_handler(message: &[u8]) -> Option<usize> {
    let base = engine::engine_module_base()?;
    // Safety: a module handle the loader gave us, mapped up to its image size.
    let image = unsafe {
        let size = crate::pe::image_size(base as *mut u8)?;
        std::slice::from_raw_parts(base as *const u8, size)
    };
    // The string itself: `message` right after a NUL, so "XScreenFade" can't
    // match.
    let at = image
        .windows(message.len() + 1)
        .position(|w| w[0] == 0 && &w[1..] == message)?
        + 1;
    let string = (base + at) as u32;
    let s = string.to_le_bytes();
    let pattern = format!(
        "68 ?? ?? ?? ?? 68 {:02X} {:02X} {:02X} {:02X} E8",
        s[0], s[1], s[2], s[3]
    );
    // Safety: as above.
    let push = unsafe { crate::scan::find_unique(base, &pattern) }.ok()?;
    let handler = unsafe { std::ptr::read_unaligned((push + 1) as *const u32) } as usize;
    (base..base + image.len())
        .contains(&handler)
        .then_some(handler)
}

fn handler(slot: &AtomicUsize, message: &[u8]) -> Option<UserMsgHookFn> {
    let found = match slot.load(Ordering::Acquire) {
        0 => {
            let found = engine_handler(message).unwrap_or(1);
            slot.store(found, Ordering::Release);
            if found == 1 {
                let name = String::from_utf8_lossy(&message[..message.len() - 1]);
                unsafe {
                    crate::debug::report(&format!(
                        "screen_effects: hw.dll's {name} handler wasn't found, so it can't be filtered"
                    ))
                };
            }
            found
        }
        found => found,
    };
    // Safety: the engine's own pfnUserMsgHook for this message.
    (found != 1).then(|| unsafe { std::mem::transmute::<usize, UserMsgHookFn>(found) })
}

unsafe extern "C" fn hooked_fade(name: *const c_char, size: i32, buf: *mut c_void) -> i32 {
    if FLASH.on() && size >= 10 && !buf.is_null() {
        // Safety: the engine hands over `size` bytes of payload.
        let payload = unsafe { std::slice::from_raw_parts(buf as *const u8, size as usize) };
        if is_head_hit_flash(payload) {
            // 1 is what the dispatcher treats as handled.
            return 1;
        }
    }
    match handler(&FADE_HANDLER, b"ScreenFade\0") {
        Some(original) => unsafe { original(name, size, buf) },
        None => 1,
    }
}

unsafe extern "C" fn hooked_shake(name: *const c_char, size: i32, buf: *mut c_void) -> i32 {
    if SHAKE.on() {
        if size >= 4 && !buf.is_null() {
            // Safety: the engine hands over `size` bytes of payload.
            let head = unsafe { std::ptr::read_unaligned(buf as *const [u8; 4]) };
            PENDING_AT.store(CALCS.load(Ordering::Relaxed), Ordering::Relaxed);
            PENDING.store(u32::from_le_bytes(head), Ordering::Relaxed);
        }
        return 1;
    }
    match handler(&SHAKE_HANDLER, b"ScreenShake\0") {
        Some(original) => unsafe { original(name, size, buf) },
        None => 1,
    }
}

/// Every frame: hooks both messages once either setting has been on.
pub fn poll() {
    if !WANTED.load(Ordering::Relaxed) {
        if !FLASH.on() && !SHAKE.on() {
            return;
        }
        WANTED.store(true, Ordering::Relaxed);
    }
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    // Found before hooking, while the engine's own record is still the one
    // the registration site names.
    if handler(&FADE_HANDLER, b"ScreenFade\0").is_none()
        || handler(&SHAKE_HANDLER, b"ScreenShake\0").is_none()
    {
        return;
    }
    for (message, ours) in [
        ("ScreenFade", hooked_fade as UserMsgHookFn),
        ("ScreenShake", hooked_shake as UserMsgHookFn),
    ] {
        if let Ok(name) = CString::new(message) {
            // Safety: the engine's own hook function; it returns early when
            // the first record for the name already has this handler.
            unsafe { (engfuncs.pfn_hook_user_msg)(name.as_ptr(), ours) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pov_live_bob::NUDGE;

    fn fade(duration: u16, hold: u16, flags: i16, rgba: [u8; 4]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&duration.to_le_bytes());
        v.extend_from_slice(&hold.to_le_bytes());
        v.extend_from_slice(&flags.to_le_bytes());
        v.extend_from_slice(&rgba);
        v
    }

    #[test]
    fn the_red_flash_is_recognised() {
        assert!(is_head_hit_flash(&fade(12288, 8192, 0, [128, 0, 0, 200])));
    }

    #[test]
    fn the_black_fades_pass() {
        // The respawn clear and the end-of-map fade.
        assert!(!is_head_hit_flash(&fade(4, 0, 0, [0, 0, 0, 0])));
        assert!(!is_head_hit_flash(&fade(4096, 61440, 5, [0, 0, 0, 0])));
        // Red but with flags set: not the server's flash.
        assert!(!is_head_hit_flash(&fade(12288, 8192, 1, [128, 0, 0, 200])));
        assert!(!is_head_hit_flash(&[128, 0, 0]));
    }

    fn view(vieworg: [f32; 3], simorg: [f32; 3], roll: f32) -> View {
        View {
            vieworg,
            viewangles: [10.0, 90.0, roll],
            forward: [0.0, 1.0, 0.0],
            right: [1.0, 0.0, 0.0],
            up: [0.0, 0.0, 1.0],
            frametime: 0.01,
            time: 5.0,
            intermission: 0,
            paused: 0,
            spectator: 0,
            onground: 1,
            waterlevel: 0,
            simvel: [0.0; 3],
            simorg,
            viewheight: [0.0, 0.0, 22.0],
            idealpitch: 0.0,
            cl_viewangles: [10.0, 90.0, 0.0],
            health: 100,
            crosshairangle: [0.0; 3],
            viewsize: 120.0,
            punchangle: [0.0; 3],
            maxclients: 32,
            viewentity: 3,
            playernum: 2,
        }
    }

    #[test]
    fn the_blast_shake_decodes() {
        // The DoD blast: amplitude 0xffff (just under 16), 1 s.
        let (amplitude, duration) = decode(u32::from_le_bytes([0xff, 0xff, 0x00, 0x10]));
        assert!((amplitude - 16.0).abs() < 0.01 && (duration - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_window_opens_stretches_and_shuts() {
        let shut = [0.0; 3];
        assert!(!in_window(shut, 0.05), "no shake yet");
        let open = advance(shut, 10.0, Some((16.0, 1.0)));
        assert_eq!(open, [10.0, 11.0, 16.0]);
        assert!(in_window(open, 10.0) && in_window(open, 11.05));
        // The client clock stepping back just after the shake came in.
        assert!(in_window(open, 9.9));
        assert!(!in_window(open, 11.2) && !in_window(open, 9.0));
        // A second blast half way through runs the window on.
        assert_eq!(advance(open, 10.5, Some((8.0, 1.0))), [10.0, 11.5, 16.0]);
        // One after the first ended starts a new window.
        assert_eq!(advance(open, 20.0, Some((8.0, 1.0))), [20.0, 21.0, 8.0]);
        assert_eq!(advance(open, 10.5, None), open);
        assert_eq!(advance(open, 10.5, Some((0.0, 1.0))), open);
    }

    #[test]
    fn the_shaken_view_goes_back_to_the_live_offset() {
        // Live: the eye plus a stair-smoothing dip of 3 and the nudge.
        let live = view([100.0, 200.0, 319.0 + NUDGE], [100.0, 200.0, 300.0], 0.5);
        // Recorded: a few units of interpolation away, shaken by (5, -4, 6)
        // and 2 degrees of roll.
        let recorded = view([107.0, 196.0, 331.0], [102.0, 200.0, 303.0], 2.0);
        let (vieworg, gun, roll) = correction(&live, &recorded, 16.0, 1.0).unwrap();
        let want = [102.0, 200.0, 322.0 + NUDGE];
        for i in 0..3 {
            assert!((vieworg[i] - want[i]).abs() < 1e-4, "{vieworg:?}");
            let fix = want[i] - recorded.vieworg[i];
            assert!((gun[i] - 0.9 * fix).abs() < 1e-4, "{gun:?}");
        }
        assert!((roll - 0.5).abs() < 1e-5, "{roll}");
    }

    #[test]
    fn a_dead_or_far_off_view_is_left_alone() {
        let live = view([0.0, 0.0, 22.0], [0.0; 3], 0.0);
        let mut dead = view([5.0, 0.0, 22.0], [0.0; 3], 0.0);
        dead.health = 0;
        assert!(correction(&live, &dead, 16.0, 1.0).is_none());
        let mut spectating = view([5.0, 0.0, 22.0], [0.0; 3], 0.0);
        spectating.spectator = 1;
        assert!(correction(&live, &spectating, 16.0, 1.0).is_none());
        // Further off than a 16-unit shake can reach: not the shake.
        let far = view([60.0, 0.0, 22.0], [0.0; 3], 0.0);
        assert!(correction(&live, &far, 16.0, 1.0).is_none());
        assert!(correction(&live, &view([20.0, 0.0, 22.0], [0.0; 3], 0.0), 16.0, 1.0).is_some());
    }

    #[test]
    fn a_live_view_away_from_the_eye_is_not_used() {
        let recorded = view([5.0, 0.0, 22.0], [0.0; 3], 0.0);
        // The live view already in the death view, 30 units off the eye.
        let off = view([-12.0, -27.0, 18.0], [0.0; 3], 0.0);
        assert!(correction(&off, &recorded, 16.0, 1.0).is_none());
        let mut dead = view([0.0, 0.0, 22.0], [0.0; 3], 0.0);
        dead.health = -36;
        assert!(correction(&dead, &recorded, 16.0, 1.0).is_none());
        // A stair-smoothing dip is fine.
        let dip = view([0.0, 0.0, 6.0], [0.0; 3], 0.0);
        assert!(correction(&dip, &recorded, 16.0, 1.0).is_some());
    }

    #[test]
    fn the_live_bob_is_scaled_to_the_recorders() {
        // Live bob 1 unit along `right` (+x).
        let live = view([1.0, 0.0, 22.0], [0.0; 3], 0.0);
        let recorded = view([4.0, 3.0, 25.0], [0.0; 3], 0.0);
        let (none, _, _) = correction(&live, &recorded, 16.0, 0.0).unwrap();
        assert!(none[0].abs() < 1e-5, "{none:?}");
        let (half, _, _) = correction(&live, &recorded, 16.0, 0.5).unwrap();
        assert!((half[0] - 0.5).abs() < 1e-5, "{half:?}");

        // A recorder with no bob, then one with half the viewer's.
        let flat = view([0.0, 0.0, 22.0], [0.0; 3], 0.0);
        let halfway = view([0.5, 0.0, 22.0], [0.0; 3], 0.0);
        let mut sums = [0.0; 2];
        for _ in 0..200 {
            sums = learn(sums, &live, &flat);
        }
        assert!(bob_scale(sums).abs() < 1e-3);
        for _ in 0..2000 {
            sums = learn(sums, &live, &halfway);
        }
        assert!((bob_scale(sums) - 0.5).abs() < 1e-2, "{}", bob_scale(sums));
        // No live bob to compare with: none.
        assert_eq!(bob_scale([0.0; 2]), 0.0);
        // A NaN view is skipped, not summed in for good.
        let mut broken = live;
        broken.vieworg[0] = f32::NAN;
        let kept = learn(sums, &broken, &halfway);
        assert_eq!(kept, sums);
        assert_eq!(learn(sums, &live, &broken), sums);
    }

    #[test]
    fn a_jump_in_the_clock_is_another_demo() {
        assert!(continues(10.0, 10.01));
        // Paused: the clock stands still.
        assert!(continues(10.0, 10.0));
        // A restart, a seek back, a demo whose clock starts later.
        assert!(!continues(10.0, 0.5));
        assert!(continues(10.0, 10.0 - 0.01), "settling back a little");
        assert!(continues(10.0, 10.0 - 0.14), "as measured in game");
        assert!(!continues(10.0, 9.0));
        assert!(!continues(10.0, 250.0));
        assert!(!continues(10.0, f32::NAN));
    }

    #[test]
    fn a_shake_left_over_from_an_hltv_demo_is_stale() {
        // Dropped this frame, taken by the override one view later.
        assert!(fresh(101, 100));
        // Across the counter's wrap.
        assert!(fresh(2, u32::MAX));
        // Dropped where no override ran, seconds of frames ago.
        assert!(!fresh(5000, 100));
    }

    #[test]
    fn roll_wraps() {
        assert!((wrap(350.0) + 10.0).abs() < 1e-4);
        assert!((wrap(-190.0) - 170.0).abs() < 1e-4);
        assert!((wrap(3.0) - 3.0).abs() < 1e-6);
    }
}
