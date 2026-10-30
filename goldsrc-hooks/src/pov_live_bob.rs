//! `ds_view_use_cl_bob`: a POV demo bobs the view and the gun by the `cl_bob`,
//! `cl_bobcycle` and `cl_bobup` set now (#359), the way live play and an
//! HLTV demo watched in first person do. Off (0), a POV demo shows the bob it
//! recorded, the recording player's own, whatever `cl_bob` says here: 15 of
//! the 24 players in the `solifotw-*` demos recorded with DoD's default
//! `cl_bob 0.005`, the rest with none.
//!
//! ## What the engine does
//!
//! `client.dll`'s `V_CalcNormalRefdef` (`+0x4f430`, the same file in both
//! installs) works the bob out live in every demo: `V_CalcBob` (`+0x4ed10`)
//! gives `bob` from `cl_bob`, `cl_bobcycle`, `cl_bobup` and the player's
//! speed, clamped to -7..+4. Then DoD, unlike the Half-Life SDK, moves the
//! view sideways by `bob` along `right` and nothing up; the gun by `0.4 * bob`
//! along `forward` and nothing up or sideways; and tilts the gun by
//! `0.3 * bob` pitch, `0.5 * bob` yaw and `bob` roll, all subtracted. The
//! recorded gun of four players who recorded with a bob agrees: it moves
//! 0.39-0.43 x bob forward and -0.03..-0.06 x bob up (#889).
//!
//! But in POV playback `V_RenderView` throws that away: right after
//! `ClientDLL_CalcRefdef` it calls the demo view override (pre-Anniversary
//! `hw.dll+0xd75d`, 25th Anniversary `+0x198e90`; `playdemo` and `viewdemo`
//! alike), which copies the recorded `ref_params_s` over the live one and
//! puts the gun at the recorded position; the engine then sets the gun's
//! angles from the recorded view angles alone. So the view and gun carry the
//! recorder's bob, the gun's tilt is lost even for recorders who had one, and
//! changing `cl_bob` does nothing. HLTV demos skip the override (the engine's
//! spectator flag), which is why the cvars work there.
//!
//! ## The change
//!
//! With this on, the call to the override goes through [`after_override`]:
//! once the recorded view is in place, the recorded bob is measured from it
//! (the sideways part of `vieworg - simorg - viewheight`, which is all
//! `V_CalcRefdef` adds there besides a fixed 1/32 and a grenade's screen
//! shake, which `screen_effects::unshake` takes out first when
//! `ds_view_hide_screen_shake` is on) and replaced by the live one, the bob
//! `V_CalcBob` gives for the cvars set now, on the demo's own clock (below).
//! The camera moves sideways and the gun forward by the difference, so the
//! recorded path, its smoothing and everything else in the view stay as
//! recorded. The live tilt is added to the gun's angles in
//! [`before_view_model`], after the engine has set them and before the gun is
//! drawn.
//!
//! ## The bob runs on the demo's clock
//!
//! `V_CalcBob`'s phase is `bobtime`, a static it adds each frame's
//! `frametime` to since the game started, menus and earlier demos included.
//! So the client's own bob reaches a given demo moment at a different point
//! of its cycle on every run: two captures of one clip (a normal and a
//! wallhack pass) swayed out of step. [`demo_bob`] works the same formula out
//! itself (`cl_bob`, `cl_bobcycle`, `cl_bobup`, the recorded speed, the same
//! clamp) with the demo's clock as `bobtime`: the demo player's world clock
//! under `viewdemo` (the clock seeks and capture batches use), the recorded
//! client time under `playdemo`. Same amplitude and cycle as the client's, so
//! it looks the same; a moment of the demo is always at the same phase,
//! whichever way playback got there.
//!
//! That only holds while the recorded view is the recorder's own living eye.
//! While the recorder is dead or spectates a teammate in first person,
//! `vieworg` is someone else's eye and `simorg` still the recorder's body, so
//! the "bob" measured is a large, changing distance (a median of 2-15 units
//! on dead frames, past the clamp on half of them, in five demos), and the
//! live `V_CalcRefdef` has gone through the spectator camera too. [`decide`]
//! leaves the recorded view alone then: while spectating (`g_iUser1`), dead,
//! in an intermission, or when either measured bob is outside `V_CalcBob`'s
//! clamp.
//!
//! The call site is found by a byte pattern per build, each unique in its own
//! `hw.dll`, and redirected only once this setting or
//! `ds_view_hide_screen_shake` (which shares the call) is first turned on;
//! `tools/verify_pov_live_bob_offsets.py` re-derives both.
//!
//! `ds_debug_log_view_bob 1` logs each frame's numbers: in a POV demo the
//! demo clock, the live bob on it and the client's own (for comparison), the
//! recorded bob, why a frame was left alone, and where the gun ends up
//! relative to the camera; in an HLTV demo watched in first person the same
//! for the view `client.dll` works out live, to compare against.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::engine::{self, RefParamsPartial};
use crate::names::console_name;
use crate::scan;
use crate::setting::Cvar;

pub const NAME: &str = console_name!("view_use_cl_bob");
pub const LOG_NAME: &str = console_name!("debug_log_view_bob");

/// One build's call to the demo view override in `V_RenderView`.
struct Site {
    build: &'static str,
    pattern: &'static str,
    /// Offset of the `E8 rel32` call.
    call_at: usize,
}

#[rustfmt::skip]
const SITES: &[Site] = &[
    Site {
        build: "pre-Anniversary",
        //         push viewmodel  lea ecx,[ebp-F4]   push viewent.origin push ecx call            fld [ebp-E8]
        pattern: "68 ?? ?? ?? ?? 8D 8D 0C FF FF FF 68 ?? ?? ?? ?? 51 E8 ?? ?? ?? ?? D9 85 18 FF FF FF",
        call_at: 17,
    },
    Site {
        build: "25th Anniversary",
        //         push viewmodel  lea eax,[ebp-EC]   push viewent.origin push eax call            movss xmm3,[ebp-E0]
        pattern: "68 ?? ?? ?? ?? 8D 85 14 FF FF FF 68 ?? ?? ?? ?? 50 E8 ?? ?? ?? ?? F3 0F 10 9D 20 FF FF FF",
        call_at: 17,
    },
];

/// `ref_params_s` (`common/ref_params.h`) up to `playernum`.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct View {
    pub vieworg: [f32; 3],
    pub viewangles: [f32; 3],
    pub forward: [f32; 3],
    pub right: [f32; 3],
    pub up: [f32; 3],
    pub frametime: f32,
    pub time: f32,
    pub intermission: i32,
    pub paused: i32,
    pub spectator: i32,
    pub onground: i32,
    pub waterlevel: i32,
    pub simvel: [f32; 3],
    pub simorg: [f32; 3],
    pub viewheight: [f32; 3],
    pub idealpitch: f32,
    pub cl_viewangles: [f32; 3],
    pub health: i32,
    pub crosshairangle: [f32; 3],
    pub viewsize: f32,
    pub punchangle: [f32; 3],
    pub maxclients: i32,
    pub viewentity: i32,
    pub playernum: i32,
}

/// `V_CalcRefdef` adds this to every axis of `vieworg`.
pub(crate) const NUDGE: f32 = 1.0 / 32.0;
/// `V_CalcBob`'s clamp (`client.dll+0x4ede0`, `+0x4ee05`), with room for
/// float error.
const BOB_MIN: f32 = -7.0 - 0.05;
const BOB_MAX: f32 = 4.0 + 0.05;
/// How far along `forward` DoD moves the gun per unit of bob
/// (`client.dll+0x4f85d`).
const GUN_FORWARD: f32 = 0.4;
/// DoD's gun tilt per unit of bob, subtracted from its pitch, yaw and roll
/// (`client.dll+0x4f86b`..`+0x4f8a1`).
const GUN_TILT: [f32; 3] = [0.3, 0.5, 1.0];
/// `V_CalcBob`'s clamp itself (`client.dll+0x4ede0`, `+0x4ee05`).
const CALC_BOB_MIN: f32 = -7.0;
const CALC_BOB_MAX: f32 = 4.0;
/// `V_CalcBob`'s share of the bob that stays and the share that swings
/// (`client.dll+0x4edcc`, `+0x4edd2`).
const BOB_STEADY: f32 = 0.3;
const BOB_SWING: f32 = 0.7;

type OverrideFn = unsafe extern "C" fn(*mut View, *mut [f32; 3], *mut i32);

pub static CVAR: Cvar = Cvar::new();
pub static LOG_CVAR: Cvar = Cvar::new();
/// The override's own address, once the call is redirected.
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static GAVE_UP: AtomicBool = AtomicBool::new(false);
/// The bob the live `V_CalcRefdef` worked out this frame, as f32 bits.
static LIVE_BOB: AtomicU32 = AtomicU32::new(0);
/// The live view's speed, for the log, as f32 bits.
static LIVE_SPEED: AtomicU32 = AtomicU32::new(0);
static HAVE_LIVE: AtomicBool = AtomicBool::new(false);
/// The live bob whose tilt [`before_view_model`] still has to put on the gun,
/// as f32 bits, while [`TILT_PENDING`].
static TILT_BOB: AtomicU32 = AtomicU32::new(0);
static TILT_PENDING: AtomicBool = AtomicBool::new(false);
/// The recorded view angles of the frame the tilt is for, for the log.
static TILT_VIEW_YAW: AtomicU32 = AtomicU32::new(0);
static TILT_VIEW_ROLL: AtomicU32 = AtomicU32::new(0);
/// `cl_bob`, `cl_bobcycle` and `cl_bobup`, once looked up.
static BOB_CVARS: [Cvar; 3] = [Cvar::new(), Cvar::new(), Cvar::new()];
const BOB_CVAR_NAMES: [&std::ffi::CStr; 3] = [c"cl_bob", c"cl_bobcycle", c"cl_bobup"];

fn on() -> bool {
    CVAR.on()
}

fn logging() -> bool {
    LOG_CVAR.on()
}

fn log(line: &str) {
    // Safety: plain file append; the log cvar is a diagnostic.
    unsafe { crate::debug::report(line) };
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn speed_of(view: &View) -> f32 {
    view.simvel[0].hypot(view.simvel[1])
}

/// The sideways offset `V_CalcRefdef` put into `vieworg`: its bob.
fn bob_of(view: &View) -> f32 {
    let side = [0, 1, 2].map(|i| view.vieworg[i] - view.simorg[i] - view.viewheight[i] - NUDGE);
    dot(side, view.right)
}

/// The cvars `V_CalcBob` reads.
#[derive(Clone, Copy, Debug, PartialEq)]
struct BobCvars {
    bob: f32,
    cycle: f32,
    up: f32,
}

/// The values of `cl_bob`, `cl_bobcycle` and `cl_bobup` now, or `None` until
/// the client has registered them.
fn bob_cvars() -> Option<BobCvars> {
    let mut values = [0.0; 3];
    for (i, slot) in BOB_CVARS.iter().enumerate() {
        if slot.ptr().is_null() {
            let engfuncs = engine::engfuncs()?;
            // Safety: a plain lookup by name; null when not registered.
            let cvar = unsafe { (engfuncs.pfn_get_cvar_pointer)(BOB_CVAR_NAMES[i].as_ptr()) };
            if cvar.is_null() {
                return None;
            }
            slot.set(cvar);
        }
        values[i] = slot.value()?;
    }
    let [bob, cycle, up] = values;
    Some(BobCvars { bob, cycle, up })
}

/// `V_CalcBob` (`client.dll+0x4ed10`, the Half-Life SDK's formula) with
/// `clock` in place of its `bobtime`, for a player moving at `speed`.
fn demo_bob(clock: f64, speed: f32, cvars: BobCvars) -> f32 {
    use std::f32::consts::PI;
    let mut cycle = if cvars.cycle > 0.0 {
        let period = f64::from(cvars.cycle);
        (clock.rem_euclid(period) / period) as f32
    } else {
        0.0
    };
    cycle = if cycle < cvars.up {
        PI * cycle / cvars.up
    } else {
        PI + PI * (cycle - cvars.up) / (1.0 - cvars.up)
    };
    let bob = speed * cvars.bob;
    (bob * BOB_STEADY + bob * BOB_SWING * cycle.sin()).clamp(CALC_BOB_MIN, CALC_BOB_MAX)
}

/// The clock the live bob runs on: the demo player's world clock under
/// `viewdemo`, else the client time the recorded view carries.
fn demo_clock(recorded: &View) -> f64 {
    crate::demo_seek::clock()
        .filter(|c| c.active)
        .map_or(f64::from(recorded.time), |c| c.now)
}

fn is_a_bob(bob: f32) -> bool {
    (BOB_MIN..=BOB_MAX).contains(&bob)
}

/// Why [`after_override`] left a frame's recorded view alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Skip {
    /// The camera is the spectator's (`g_iUser1`), on someone else.
    Spectating,
    Dead,
    Intermission,
    /// One of the two measured bobs is bigger than `V_CalcBob` ever gives.
    NotABob,
}

impl Skip {
    fn word(self) -> &'static str {
        match self {
            Skip::Spectating => "spectating",
            Skip::Dead => "dead",
            Skip::Intermission => "intermission",
            Skip::NotABob => "not a bob",
        }
    }
}

/// Whether the recorded view is the recorder's own living eye, so its bob can
/// be swapped for `live`: the recorded bob when it is.
fn decide(recorded: &View, live: f32, spectating: bool) -> Result<f32, Skip> {
    if spectating || recorded.spectator != 0 {
        return Err(Skip::Spectating);
    }
    if recorded.intermission != 0 {
        return Err(Skip::Intermission);
    }
    if recorded.health <= 0 {
        return Err(Skip::Dead);
    }
    let bob = bob_of(recorded);
    if !is_a_bob(bob) || !is_a_bob(live) {
        return Err(Skip::NotABob);
    }
    Ok(bob)
}

/// What [`after_override`] adds to the recorded view and gun to swap the
/// recorded bob for `live`: (camera, gun) offsets.
fn shift(recorded: &View, recorded_bob: f32, live: f32) -> ([f32; 3], [f32; 3]) {
    let delta = live - recorded_bob;
    let camera = recorded.right.map(|r| r * delta);
    let gun = recorded.forward.map(|f| f * delta * GUN_FORWARD);
    (camera, gun)
}

/// The gun's tilt for a bob of `bob`, added to its pitch, yaw and roll.
fn tilt(bob: f32) -> [f32; 3] {
    GUN_TILT.map(|k| -k * bob)
}

/// The gun relative to the camera, along `forward`, `right` and world up.
fn gun_from_camera(view: &View, gun: [f32; 3]) -> [f32; 3] {
    let rel = [0, 1, 2].map(|i| gun[i] - view.vieworg[i]);
    [dot(rel, view.forward), dot(rel, view.right), rel[2]]
}

/// Called by `engine.rs` right after `client.dll`'s `V_CalcRefdef`, with the
/// view it worked out live.
pub fn after_calc_refdef(params: *const RefParamsPartial) {
    if params.is_null() {
        return;
    }
    let redirected = ORIGINAL.load(Ordering::Acquire) != 0;
    let logging = logging();
    if !redirected && !logging {
        return;
    }
    // Safety: the engine's ref_params_s for this frame, which is at least
    // `View` long.
    let view = unsafe { &*(params as *const View) };
    let bob = bob_of(view);
    LIVE_BOB.store(bob.to_bits(), Ordering::Relaxed);
    LIVE_SPEED.store(speed_of(view).to_bits(), Ordering::Relaxed);
    HAVE_LIVE.store(true, Ordering::Relaxed);
    // An HLTV demo watched in someone's eyes: the live view is the drawn one,
    // the reference the POV numbers are compared with.
    if logging
        && view.spectator != 0
        && let Some(target) = crate::spectator_target::in_eye_target()
    {
        log_hltv(view, bob, target);
    }
}

fn log_hltv(view: &View, bob: f32, target: i32) {
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    // Safety: the engine's own viewmodel entity, placed by V_CalcRefdef.
    let viewmodel = unsafe { (engfuncs.get_view_model)() };
    if viewmodel.is_null() {
        return;
    }
    let (origin, angles) = unsafe { ((*viewmodel).origin, (*viewmodel).angles) };
    let [f, r, u] = gun_from_camera(view, [origin.x, origin.y, origin.z]);
    log(&format!(
        "{LOG_NAME}: hltv t {:.3} player {target} speed {:.0} onground {} bob {bob:+.3} gun f {f:+.3} r {r:+.3} u {u:+.3} \
         gun yaw-view {:+.3} roll-view {:+.3}",
        view.time,
        speed_of(view),
        view.onground,
        angles.y - view.viewangles[1],
        angles.z - view.viewangles[2],
    ));
}

/// Stands in for the engine's demo view override at its one call site.
unsafe extern "C" fn after_override(view: *mut View, gun: *mut [f32; 3], viewmodel: *mut i32) {
    let original = ORIGINAL.load(Ordering::Acquire);
    // Safety: the address the call went to before it was redirected.
    let original: OverrideFn = unsafe { std::mem::transmute(original) };
    // Safety: the engine's ref_params_s, still holding the live view.
    let live = (!view.is_null()).then(|| unsafe { *view });
    unsafe { original(view, gun, viewmodel) };
    TILT_PENDING.store(false, Ordering::Relaxed);
    if view.is_null() || gun.is_null() {
        return;
    }
    if let Some(live) = live {
        // Safety: as below.
        unsafe { crate::screen_effects::unshake(&live, &mut *view, &mut *gun) };
    }
    if !on() || !HAVE_LIVE.swap(false, Ordering::Relaxed) {
        return;
    }
    let client_bob = f32::from_bits(LIVE_BOB.load(Ordering::Relaxed));
    let spectating = crate::spectator_target::mode().is_some_and(|mode| mode != 0);
    // Safety: the engine's ref_params_s and the gun entity's origin, both
    // live for this call.
    let (view, gun) = unsafe { (&mut *view, &mut *gun) };
    let clock = demo_clock(view);
    // The client's own bob only until its cvars are registered.
    let live = bob_cvars().map_or(client_bob, |cvars| demo_bob(clock, speed_of(view), cvars));
    let decision = decide(view, live, spectating);
    if let Ok(recorded) = decision {
        let (camera, gun_shift) = shift(view, recorded, live);
        for i in 0..3 {
            view.vieworg[i] += camera[i];
            gun[i] += gun_shift[i];
        }
        TILT_BOB.store(live.to_bits(), Ordering::Relaxed);
        TILT_VIEW_YAW.store(view.viewangles[1].to_bits(), Ordering::Relaxed);
        TILT_VIEW_ROLL.store(view.viewangles[2].to_bits(), Ordering::Relaxed);
        TILT_PENDING.store(true, Ordering::Relaxed);
    }
    if logging() {
        let [f, r, u] = gun_from_camera(view, *gun);
        let action = match decision {
            Ok(recorded) => format!("swapped, recorded bob {recorded:+.3}"),
            Err(why) => format!(
                "left alone ({}), recorded bob {:+.3}",
                why.word(),
                bob_of(view)
            ),
        };
        log(&format!(
            "{LOG_NAME}: pov t {:.3} clock {clock:.4} speed {:.0} (live {:.0}) onground {} health {} g_iUser1 {} \
             live bob {live:+.4} (client's {client_bob:+.3}), {action}; gun f {f:+.4} r {r:+.4} u {u:+.4}",
            view.time,
            speed_of(view),
            f32::from_bits(LIVE_SPEED.load(Ordering::Relaxed)),
            view.onground,
            view.health,
            crate::spectator_target::mode().unwrap_or(-1),
        ));
    }
}

/// Puts the live bob's tilt on the gun of a POV demo, once the engine has set
/// its angles from the recorded view and before it is drawn. Called from
/// `HUD_DrawTransparentTriangles`, which the engine runs before
/// `R_DrawViewModel`; once per [`after_override`] that swapped the bob.
pub fn before_view_model() {
    if !TILT_PENDING.swap(false, Ordering::Relaxed) {
        return;
    }
    let Some(engfuncs) = engine::engfuncs() else {
        return;
    };
    // Safety: the engine's own viewmodel entity, valid for the frame.
    let viewmodel = unsafe { (engfuncs.get_view_model)() };
    if viewmodel.is_null() {
        return;
    }
    let bob = f32::from_bits(TILT_BOB.load(Ordering::Relaxed));
    let [pitch, yaw, roll] = tilt(bob);
    // Safety: as above; the renderer reads these next.
    let entity = unsafe { &mut *viewmodel };
    for angles in [&mut entity.angles, &mut entity.curstate.angles] {
        angles.x += pitch;
        angles.y += yaw;
        angles.z += roll;
    }
    if logging() {
        log(&format!(
            "{LOG_NAME}: pov tilt bob {bob:+.3} gun yaw-view {:+.3} roll-view {:+.3}",
            entity.angles.y - f32::from_bits(TILT_VIEW_YAW.load(Ordering::Relaxed)),
            entity.angles.z - f32::from_bits(TILT_VIEW_ROLL.load(Ordering::Relaxed)),
        ));
    }
}

fn redirect() -> Result<&'static str, String> {
    let base = engine::engine_module_base().ok_or("hw.dll is not loaded yet")?;
    let mut errors = Vec::new();
    for site in SITES {
        // Safety: `base` is a module handle the loader gave us.
        let found = match unsafe { scan::find_unique(base, site.pattern) } {
            Ok(found) => found,
            Err(why) => {
                errors.push(format!("{}: {why}", site.build));
                continue;
            }
        };
        let call = found + site.call_at;
        // Safety: the scan matched an `E8 rel32` here.
        let rel = unsafe { std::ptr::read_unaligned((call + 1) as *const i32) };
        let original = (call as isize + 5 + rel as isize) as usize;
        let ours = after_override as *const () as usize;
        let new_rel = (ours as isize - (call as isize + 5)) as i32;
        ORIGINAL.store(original, Ordering::Release);
        if !unsafe { crate::patch::write_code_bytes(call + 1, &new_rel.to_le_bytes()) } {
            ORIGINAL.store(0, Ordering::Release);
            return Err("could not make the demo view override's call writable".into());
        }
        return Ok(site.build);
    }
    Err(format!(
        "the demo view override's call wasn't found ({})",
        errors.join("; ")
    ))
}

/// Redirects the call the first time this setting or
/// `ds_view_hide_screen_shake` is on. Every frame.
pub fn poll() {
    if ORIGINAL.load(Ordering::Acquire) != 0
        || GAVE_UP.load(Ordering::Relaxed)
        || !(on() || crate::screen_effects::shake_on())
    {
        return;
    }
    let line = match redirect() {
        Ok(build) => format!("demo view override redirected ({build} hw.dll)"),
        Err(why) => {
            GAVE_UP.store(true, Ordering::Relaxed);
            format!("{NAME}, {}: {why}", crate::screen_effects::SHAKE_NAME)
        }
    };
    unsafe { crate::debug::report(&line) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(vieworg: [f32; 3], simorg: [f32; 3]) -> View {
        View {
            vieworg,
            viewangles: [0.0; 3],
            forward: [1.0, 0.0, 0.0],
            right: [0.0, -1.0, 0.0],
            up: [0.0, 0.0, 1.0],
            frametime: 0.0,
            time: 0.0,
            intermission: 0,
            paused: 0,
            spectator: 0,
            onground: 1,
            waterlevel: 0,
            simvel: [0.0; 3],
            simorg,
            viewheight: [0.0, 0.0, 22.0],
            idealpitch: 0.0,
            cl_viewangles: [0.0; 3],
            health: 100,
            crosshairangle: [0.0; 3],
            viewsize: 120.0,
            punchangle: [0.0; 3],
            maxclients: 32,
            viewentity: 3,
            playernum: 2,
        }
    }

    /// A living recorder's view with a recorded sideways bob of `bob`.
    fn bobbing(bob: f32) -> View {
        view([NUDGE, -bob + NUDGE, 22.0 + NUDGE], [0.0; 3])
    }

    #[test]
    fn the_view_is_as_long_as_ref_params_up_to_playernum() {
        assert_eq!(std::mem::offset_of!(View, health), 0x8c);
        assert_eq!(std::mem::offset_of!(View, viewentity), 0xb0);
        assert_eq!(std::mem::size_of::<View>(), 0xb8);
    }

    #[test]
    fn the_bob_is_the_sideways_part_of_the_eye_offset() {
        assert!((bob_of(&bobbing(2.0)) - 2.0).abs() < 1e-5);
    }

    #[test]
    fn the_camera_moves_sideways_and_the_gun_forward_only() {
        let recorded = bobbing(1.0);
        let (camera, gun) = shift(&recorded, bob_of(&recorded), 3.0);
        assert!((camera[1] + 2.0).abs() < 1e-5, "{camera:?}");
        // DoD's gun has no vertical bob, unlike the Half-Life SDK's.
        assert!((gun[0] - 0.8).abs() < 1e-5, "{gun:?}");
        assert!(gun[1].abs() < 1e-5 && gun[2].abs() < 1e-5, "{gun:?}");
        let (camera, gun) = shift(&recorded, bob_of(&recorded), 1.0);
        assert!(camera.iter().chain(&gun).all(|v| v.abs() < 1e-5));
    }

    #[test]
    fn the_gun_tilts_as_dod_tilts_it() {
        let [pitch, yaw, roll] = tilt(2.0);
        assert!(
            (pitch + 0.6).abs() < 1e-5 && (yaw + 1.0).abs() < 1e-5 && (roll + 2.0).abs() < 1e-5
        );
    }

    #[test]
    fn a_living_recorders_own_view_is_swapped() {
        let recorded = bobbing(0.5);
        let got = decide(&recorded, 1.5, false).expect("swapped");
        assert!((got - 0.5).abs() < 1e-5);
        // In the air V_CalcBob keeps its last bob, recorded and live alike.
        let mut airborne = bobbing(0.5);
        airborne.onground = 0;
        assert!(decide(&airborne, 1.5, false).is_ok());
        // Both ends of the clamp are still a bob.
        assert!(decide(&bobbing(-7.0), 4.0, false).is_ok());
    }

    #[test]
    fn spectating_a_teammate_leaves_the_recorded_view_alone() {
        // In-eye on a teammate 40 units to the side of the recorder's body:
        // well inside the clamp is no proof, the camera mode decides.
        let teammate = view([0.0, -3.0 + NUDGE, 22.0 + NUDGE], [0.0; 3]);
        assert_eq!(decide(&teammate, 1.0, true), Err(Skip::Spectating));
        let mut hltv = bobbing(0.5);
        hltv.spectator = 1;
        assert_eq!(decide(&hltv, 1.0, false), Err(Skip::Spectating));
    }

    #[test]
    fn a_dead_recorder_is_left_alone() {
        for health in [0, -40] {
            let mut dead = bobbing(0.5);
            dead.health = health;
            assert_eq!(decide(&dead, 1.0, false), Err(Skip::Dead));
        }
    }

    #[test]
    fn an_intermission_is_left_alone() {
        let mut over = bobbing(0.5);
        over.intermission = 1;
        assert_eq!(decide(&over, 1.0, false), Err(Skip::Intermission));
    }

    #[test]
    fn anything_past_the_clamp_is_not_a_bob() {
        // The recorded eye 40 units from the recorder's body.
        assert_eq!(decide(&bobbing(40.0), 1.0, false), Err(Skip::NotABob));
        assert_eq!(decide(&bobbing(-7.5), 1.0, false), Err(Skip::NotABob));
        assert_eq!(decide(&bobbing(4.5), 1.0, false), Err(Skip::NotABob));
        // A live view that isn't the player's own eye either.
        assert_eq!(decide(&bobbing(0.5), -25.0, false), Err(Skip::NotABob));
        assert_eq!(decide(&bobbing(0.5), f32::NAN, false), Err(Skip::NotABob));
    }

    const DOD_DEFAULT: BobCvars = BobCvars {
        bob: 0.005,
        cycle: 0.8,
        up: 0.5,
    };

    #[test]
    fn the_live_bob_follows_v_calc_bobs_formula() {
        // Standing still: no bob at any phase.
        assert_eq!(demo_bob(12.3, 0.0, DOD_DEFAULT), 0.0);
        // Running at 200 u/s, bob 1.0: 0.3 steady plus 0.7 swinging, the
        // swing's top a quarter of the cycle in (half of the up half).
        let top = demo_bob(0.2, 200.0, DOD_DEFAULT);
        assert!((top - 1.0).abs() < 1e-5, "{top}");
        let start = demo_bob(0.0, 200.0, DOD_DEFAULT);
        assert!((start - 0.3).abs() < 1e-5, "{start}");
        // Three quarters in, the down half's bottom.
        let bottom = demo_bob(0.6, 200.0, DOD_DEFAULT);
        assert!((bottom + 0.4).abs() < 1e-5, "{bottom}");
        // Clamped as V_CalcBob clamps.
        assert_eq!(demo_bob(0.2, 1e5, DOD_DEFAULT), 4.0);
        assert_eq!(demo_bob(0.6, 1e5, DOD_DEFAULT), -7.0);
    }

    #[test]
    fn a_demo_moment_is_always_at_the_same_phase() {
        // Same clock, same bob, whatever came before; one cycle on, too.
        let a = demo_bob(1234.567, 180.0, DOD_DEFAULT);
        assert_eq!(a, demo_bob(1234.567, 180.0, DOD_DEFAULT));
        let b = demo_bob(1234.567 + 0.8, 180.0, DOD_DEFAULT);
        assert!((a - b).abs() < 1e-3, "{a} {b}");
    }

    #[test]
    fn odd_cvars_give_no_nan() {
        let no_cycle = BobCvars {
            cycle: 0.0,
            ..DOD_DEFAULT
        };
        assert!(demo_bob(3.0, 200.0, no_cycle).is_finite());
        let all_up = BobCvars {
            up: 1.0,
            ..DOD_DEFAULT
        };
        assert!(demo_bob(0.7, 200.0, all_up).is_finite());
        let no_up = BobCvars {
            up: 0.0,
            ..DOD_DEFAULT
        };
        assert!(demo_bob(0.7, 200.0, no_up).is_finite());
    }

    #[test]
    fn the_gun_is_measured_from_the_camera() {
        let v = bobbing(0.0);
        let [f, r, u] = gun_from_camera(&v, [2.0 + NUDGE, -1.0 + NUDGE, 21.0 + NUDGE]);
        assert!((f - 2.0).abs() < 1e-5 && (r - 1.0).abs() < 1e-5 && (u + 1.0).abs() < 1e-5);
    }

    #[test]
    fn each_call_offset_lands_on_the_call() {
        for site in SITES {
            let bytes: Vec<&str> = site.pattern.split(' ').collect();
            assert_eq!(bytes[site.call_at], "E8", "{}", site.build);
            assert!(
                bytes[site.call_at + 1..site.call_at + 5]
                    .iter()
                    .all(|b| *b == "??")
            );
        }
    }
}
