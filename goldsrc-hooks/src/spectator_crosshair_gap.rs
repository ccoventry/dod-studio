//! The gap in POV's red crosshair (`cl_xhair_style 0`), worked out for the
//! spectated player (#308). Part of `ds_spec_match_pov`: `spectator_crosshair`
//! routes the spectator draw to the game's own style-0 draw, and this module
//! answers that draw's gap call (`crosshair_gap`) with the watched player's
//! gap instead of the local client's.
//!
//! ## What the gap is
//!
//! `crosshair_gap` has the game's function. `base`, `moving` and `alt` are
//! the weapon's row of `P_WpnInfo`, the table the spread function
//! (`client+0x2d070`) indexes by weapon id: 0x15c bytes a row, the three
//! floats at +0x150, after the row's third-person model (+0) and viewmodel
//! (+0x20). Nothing in it is prediction-only: the weapon, whether he is
//! deployed and how fast he moves are all things a spectator sees, so the
//! gap can follow the watched player rather than sit still.
//! `docs/goldsrc_crosshair.md` has the measured gaps.
//!
//! "Deployed" is not "prone": `IsInMGDeploy` is `g_iUser3 == 2` (prone with
//! the bipod down) or the sandbag flag, so a prone Garand keeps its standing
//! gap. The body sequence says it here (`bipod_*`, `sandbag_*`), as it does
//! for #310's machine-gun rule.
//!
//! ## For a spectator
//!
//! - **Weapon**: the viewmodel `anim_fix` already filters against the held
//!   model, matched to the table row's own viewmodel path. Each weapon has
//!   its own viewmodel, where its third-person model changes with the
//!   stance (`p_mg42bd`, `p_mg42pr`, ...).
//! - **Deployed**: the body sequence, as above.
//! - **Speed**: POV reads the local predicted velocity; HLTV sends no
//!   velocity, so it is the horizontal origin change over the newest
//!   [`WINDOW`] seconds of snapshots. Horizontal, because a stair step moves
//!   the origin up 18 units in one snapshot while the game's own velocity
//!   stays level. A plain fall therefore does not widen it, where POV would.
//!
//! The easing state is the game's own `[this+0x98]`, stepped the same 3 px a
//! frame, so the arms move at the frame rate as POV's do.
//!
//! ## How it is wired
//!
//! `spectator_crosshair` calls [`ready`] (the table) and then
//! `crosshair_gap::set_spectator_hook` before it routes the spectator draw
//! here. While spectating (`g_iUser1` not 0) the hook calls [`gap`], which
//! reads atomics only: [`note_watched`], from `anim_fix::apply` once a frame,
//! works the target out.

// [`gap`] is called only from the 32-bit hook; the host build (clippy,
// tests) compiles the rest.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::cell::RefCell;
use std::ffi::CStr;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering};

use crate::crosshair_gap::FIXED_GAP;
use crate::engine;

/// One `P_WpnInfo` row.
const STRIDE: usize = 0x15c;
/// The three floats' offset in a row; the table address points at them.
const FLOATS_AT: usize = 0x150;
/// The viewmodel path's offset in a row.
const VIEWMODEL_AT: usize = 0x20;
/// The longest path read out of a row (the slot is 32 bytes).
const PATH_MAX: usize = 32;
/// Weapon ids run 1..=40; this is a ceiling on the read, not the count.
const MAX_ID: usize = 64;
/// Rows checked before the table is trusted.
const KNOWN_ROWS: [(i32, &str); 2] = [(5, "models/v_garand.mdl"), (17, "models/v_mg42.mdl")];

/// Weapons with the fixed gap (the byte table at `client+0x2d050`).
const FIXED_IDS: [std::ops::RangeInclusive<i32>; 2] = [13..=16, 29..=31];
/// Faster than this, in u/s, adds the weapon's `moving` spread
/// (`.rdata+0xac43c`).
const MOVING_SPEED: f32 = 105.0;
/// How far the gap moves a frame.
const STEP: i32 = 3;
/// `CHudDoDCrossHair`'s last gap, which the game eases from.
const LAST_GAP_AT: usize = 0x98;
/// The span of snapshots the speed is read over, in seconds. The snapshots'
/// origins are uneven: over 0.15 s a player walking at a steady pace read
/// anywhere from 68 to 142 u/s, flipping the gap across the 105 u/s line
/// several times a second.
const WINDOW: f64 = 0.3;
/// A longer gap between snapshots starts afresh (as `snapshot_motion`).
const GAP_SECONDS: f64 = 0.5;

const DYNAMIC_CVAR: &CStr = c"cl_dynamic_xhair";

/// One weapon's row of the table.
#[derive(Clone, Debug, PartialEq)]
struct Weapon {
    id: i32,
    /// The row's viewmodel path, lowercased with forward slashes.
    viewmodel: String,
    base: f32,
    moving: f32,
    alt: f32,
}

/// What the draw should do with the gap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    /// The game's fixed 4, without easing.
    Fixed,
    /// Ease toward this many pixels.
    Toward(i32),
}

impl Target {
    /// One atomic's worth: the fixed gap as -1, any other as itself.
    fn code(self) -> i32 {
        match self {
            Target::Fixed => -1,
            Target::Toward(gap) => gap.max(0),
        }
    }

    fn from_code(code: i32) -> Target {
        if code < 0 {
            Target::Fixed
        } else {
            Target::Toward(code)
        }
    }

    fn describe(self) -> String {
        match self {
            Target::Fixed => format!("fixed {FIXED_GAP} px"),
            Target::Toward(gap) => format!("{gap} px"),
        }
    }
}

/// The table address [`TABLE`] was read from, 0 before it.
static TABLE_READ_FROM: AtomicUsize = AtomicUsize::new(0);
/// The table, read once by [`ready`]. Written there only, so the per-frame
/// readers never wait.
static TABLE: RwLock<Vec<Weapon>> = RwLock::new(Vec::new());

/// This frame's target, as [`Target::code`].
static TARGET: AtomicI32 = AtomicI32::new(-1);
/// For `ds_debug_status`: the last gap drawn, the weapon id (0 for none),
/// deployed, and the speed as f32 bits.
static SHOWN: AtomicI32 = AtomicI32::new(-1);
static WEAPON_ID: AtomicI32 = AtomicI32::new(0);
static DEPLOYED: AtomicBool = AtomicBool::new(false);
static SPEED: AtomicU32 = AtomicU32::new(0);
/// Whether the x87 ran at single precision when the target was worked out.
static SINGLE: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// Runs on the engine's thread only (`anim_fix::apply`), like
    /// `snapshot_motion`.
    static SPEEDOMETER: RefCell<Speedometer> = RefCell::new(Speedometer::default());
}

/// The watched player, as `anim_fix` sees him this frame.
pub struct Watched<'a> {
    pub entity: i32,
    /// The viewmodel's path, e.g. `models/v_garand.mdl`.
    pub viewmodel: &'a str,
    /// On the bipod or a sandbag (`bipod_*` / `sandbag_*` body).
    pub deployed: bool,
    /// `curstate.msg_time` and `curstate.origin`.
    pub time: f64,
    pub origin: [f32; 3],
}

/// Works out this frame's gap for the watched player. Called once a frame by
/// `anim_fix::apply` while in a player's eyes.
pub fn note_watched(watched: &Watched) {
    let speed = SPEEDOMETER.with(|meter| {
        let mut meter = meter.borrow_mut();
        meter.snapshot(watched.entity, watched.time, watched.origin);
        meter.speed()
    });
    let precision = x87_precision();
    let weapon = TABLE.try_read().ok().and_then(|table| {
        weapon_for(watched.viewmodel, &table)
            .map(|w| (w.id, target_for(w, watched.deployed, speed, precision)))
    });
    SINGLE.store(precision == Precision::Single, Ordering::Relaxed);
    let target = if dynamic() {
        weapon.map_or(Target::Fixed, |(_, target)| target)
    } else {
        Target::Fixed
    };
    let id = weapon.map_or(0, |(id, _)| id);
    let changed = TARGET.swap(target.code(), Ordering::Relaxed) != target.code();
    WEAPON_ID.store(id, Ordering::Relaxed);
    DEPLOYED.store(watched.deployed, Ordering::Relaxed);
    SPEED.store(speed.to_bits(), Ordering::Relaxed);
    // Beside the held-model and body-sequence trail it reads off, behind
    // the same switch: it changes many times a minute.
    if changed && crate::anim_fix::LOG_HELD_MODELS.load(Ordering::Relaxed) {
        unsafe {
            crate::debug::report(&format!(
                "spectator_crosshair_gap: target {} (weapon id {id}, {}, {speed:.0} u/s)",
                target.describe(),
                if watched.deployed {
                    "deployed"
                } else {
                    "not deployed"
                }
            ))
        };
    }
}

/// No player in view this frame: the fixed gap, and no speed history.
pub fn forget() {
    TARGET.store(Target::Fixed.code(), Ordering::Relaxed);
    WEAPON_ID.store(0, Ordering::Relaxed);
    SPEEDOMETER.with(|meter| meter.borrow_mut().reset());
}

/// Whether `cl_dynamic_xhair` is on, read the way the game reads it.
fn dynamic() -> bool {
    let Some(engfuncs) = engine::engfuncs() else {
        return true;
    };
    // Safety: the engine's own cvar read, with a NUL-terminated name.
    let value = unsafe { (engfuncs.pfn_get_cvar_float)(DYNAMIC_CVAR.as_ptr()) };
    value.is_nan() || value >= 1.0
}

/// The row whose viewmodel is `viewmodel`, the first one for the duplicated
/// rows (the grenades, binoculars). Compared in place: this runs every frame.
fn weapon_for<'t>(viewmodel: &str, table: &'t [Weapon]) -> Option<&'t Weapon> {
    table.iter().find(|w| same_path(viewmodel, &w.viewmodel))
}

/// `path` against a path already lowercased with forward slashes.
fn same_path(path: &str, normalised: &str) -> bool {
    path.len() == normalised.len()
        && path.bytes().zip(normalised.bytes()).all(|(a, b)| {
            (if a == b'\\' {
                b'/'
            } else {
                a.to_ascii_lowercase()
            }) == b
        })
}

fn normalise(path: &str) -> String {
    path.replace('\\', "/").to_ascii_lowercase()
}

/// The precision the game's x87 sums run at: the control word's, which
/// this DLL's own SSE arithmetic does not share.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Precision {
    Single,
    Double,
}

/// The x87 precision control on this thread, the one the HUD draws on.
#[cfg(target_arch = "x86")]
fn x87_precision() -> Precision {
    let mut word: u16 = 0;
    // Safety: stores the control word into a local; changes nothing.
    unsafe {
        std::arch::asm!(
            "fnstcw word ptr [{}]",
            in(reg) &mut word,
            options(nostack, preserves_flags)
        )
    };
    // Bits 8-9: 00 is 24-bit, 10 53-bit, 11 64-bit.
    if (word >> 8) & 3 == 0 {
        Precision::Single
    } else {
        Precision::Double
    }
}

#[cfg(not(target_arch = "x86"))]
fn x87_precision() -> Precision {
    Precision::Double
}

/// The gap the game would ease toward for `weapon`, as `client+0x2cfb0`
/// and `client+0x2d070` compute it: `fld`, `fadd`, `fmul -200.0`, `ftol`.
/// The precision matters by a pixel: 0.02 is a hair under it as a float, so
/// a deployed .30 cal's `200 * 0.02` truncates to 3 at double precision and
/// rounds up to 4 at single.
fn target_for(weapon: &Weapon, deployed: bool, speed: f32, precision: Precision) -> Target {
    if FIXED_IDS.iter().any(|ids| ids.contains(&weapon.id)) {
        return Target::Fixed;
    }
    let spread = if deployed { weapon.alt } else { weapon.base };
    let moving = (speed > MOVING_SPEED).then_some(weapon.moving);
    let scaled = match precision {
        Precision::Single => ((spread + moving.unwrap_or(0.0)) * -200.0) as i32,
        Precision::Double => {
            ((f64::from(spread) + f64::from(moving.unwrap_or(0.0))) * -200.0) as i32
        }
    };
    Target::Toward(2 - scaled)
}

/// One frame's step from `last` toward `target`: at most [`STEP`].
fn ease(last: i32, target: i32) -> i32 {
    target.clamp(last - STEP, last + STEP)
}

/// Horizontal speed from the watched player's snapshots.
#[derive(Default)]
struct Speedometer {
    entity: i32,
    /// Newest last; `(time, x, y)`.
    snapshots: Vec<(f64, f32, f32)>,
}

impl Speedometer {
    fn snapshot(&mut self, entity: i32, time: f64, origin: [f32; 3]) {
        if entity != self.entity {
            self.reset();
            self.entity = entity;
        }
        if let Some(&(last, ..)) = self.snapshots.last() {
            if time == last {
                return;
            }
            // A clock that went backwards is a seek or a new demo.
            if time < last || time - last > GAP_SECONDS {
                self.snapshots.clear();
            }
        }
        self.snapshots.push((time, origin[0], origin[1]));
        // Keep the newest one older than the window, to measure across it.
        let newest = time;
        while self.snapshots.len() > 2 && newest - self.snapshots[1].0 >= WINDOW {
            self.snapshots.remove(0);
        }
    }

    /// Over the newest `WINDOW` seconds, or the newest interval if that is
    /// longer; 0 with fewer than two snapshots.
    fn speed(&self) -> f32 {
        let (Some(&(t0, x0, y0)), Some(&(t1, x1, y1))) =
            (self.snapshots.first(), self.snapshots.last())
        else {
            return 0.0;
        };
        let dt = (t1 - t0) as f32;
        if dt <= 0.0 {
            return 0.0;
        }
        (x1 - x0).hypot(y1 - y0) / dt
    }

    fn reset(&mut self) {
        self.snapshots.clear();
    }
}

/// Reads `P_WpnInfo` from the loaded module, refusing a table whose known
/// rows are not where they should be.
///
/// # Safety
///
/// `table` is the operand the spread function indexes; each row is checked
/// readable before it is read.
unsafe fn read_table(table: usize) -> Result<Vec<Weapon>, String> {
    let mut weapons = Vec::new();
    for id in 1..MAX_ID {
        let floats = table + STRIDE * id;
        let row = floats - FLOATS_AT;
        if !crate::crash::readable(row, STRIDE) {
            break;
        }
        let path =
            unsafe { std::slice::from_raw_parts((row + VIEWMODEL_AT) as *const u8, PATH_MAX) };
        let Some(path) = path
            .split(|b| *b == 0)
            .next()
            .and_then(|p| std::str::from_utf8(p).ok())
        else {
            break;
        };
        if !path.starts_with("models/v_") {
            break;
        }
        let read = |at: usize| unsafe { ((floats + at) as *const f32).read_unaligned() };
        weapons.push(Weapon {
            id: id as i32,
            viewmodel: normalise(path),
            base: read(0),
            moving: read(4),
            alt: read(8),
        });
    }
    for (id, viewmodel) in KNOWN_ROWS {
        if !weapons
            .iter()
            .any(|w| w.id == id && w.viewmodel == viewmodel)
        {
            return Err(format!(
                "the weapon table does not have {viewmodel} as weapon {id} -- its layout is not the one this module knows"
            ));
        }
    }
    Ok(weapons)
}

/// Reads the weapon table out of the loaded `client.dll`, once per table
/// address. `spectator_crosshair` calls it before routing the red crosshair:
/// an error (the code not found, or a table laid out differently) keeps the
/// stock spectator dot.
pub fn ready() -> Result<(), String> {
    let table = crate::crosshair_gap::table_address()?;
    if TABLE_READ_FROM.load(Ordering::Acquire) == table {
        return Ok(());
    }
    // Safety: the operand the spread function indexes the table by.
    let weapons = unsafe { read_table(table) }?;
    match TABLE.write() {
        Ok(mut slot) => *slot = weapons,
        Err(_) => return Err("the weapon table's lock is poisoned".to_string()),
    }
    TABLE_READ_FROM.store(table, Ordering::Release);
    Ok(())
}

/// The watched player's gap, eased in the game's own slot: the hook's answer
/// while spectating.
///
/// # Safety
///
/// `this` is the `CHudDoDCrossHair` the style-0 draw passed.
pub unsafe fn gap(this: *mut u8) -> i32 {
    let gap = match Target::from_code(TARGET.load(Ordering::Relaxed)) {
        Target::Fixed => FIXED_GAP,
        Target::Toward(target) => {
            // Safety: the game's own easing slot in the object it passed.
            let slot = unsafe { this.add(LAST_GAP_AT) } as *mut i32;
            let gap = ease(unsafe { slot.read_unaligned() }, target);
            unsafe { slot.write_unaligned(gap) };
            gap
        }
    };
    SHOWN.store(gap, Ordering::Relaxed);
    gap
}

/// For `spectator_crosshair::status`: the gap and what it came from.
pub fn status() -> String {
    let speed = f32::from_bits(SPEED.load(Ordering::Relaxed));
    let shown = SHOWN.load(Ordering::Relaxed);
    let shown = if shown < 0 {
        "not drawn yet".to_string()
    } else {
        format!("{shown} px")
    };
    let target = Target::from_code(TARGET.load(Ordering::Relaxed)).describe();
    let weapon = match WEAPON_ID.load(Ordering::Relaxed) {
        0 => "no known weapon".to_string(),
        id => format!("weapon id {id}"),
    };
    format!(
        "gap {shown}, toward {target} ({weapon}, {}, {speed:.0} u/s, x87 at {} precision)",
        if DEPLOYED.load(Ordering::Relaxed) {
            "deployed"
        } else {
            "not deployed"
        },
        if SINGLE.load(Ordering::Relaxed) {
            "single"
        } else {
            "double"
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: i32, viewmodel: &str, base: f32, moving: f32, alt: f32) -> Weapon {
        Weapon {
            id,
            viewmodel: viewmodel.to_string(),
            base,
            moving,
            alt,
        }
    }

    /// Rows as `client.dll` ships them (`tools/verify_spectator_crosshair_offsets.py`).
    fn table() -> Vec<Weapon> {
        vec![
            row(5, "models/v_garand.mdl", 0.016, 0.1, 0.0),
            row(7, "models/v_tommy.mdl", 0.025, 0.1, 0.0),
            row(10, "models/v_98k.mdl", 0.014, 0.15, 0.0),
            row(11, "models/v_bar.mdl", 0.025, 0.1, 0.02),
            row(13, "models/v_grenade.mdl", 0.0, 0.1, 0.0),
            row(16, "models/v_grenade.mdl", 0.0, 0.1, 0.0),
            row(17, "models/v_mg42.mdl", 0.2, 0.1, 0.025),
            row(18, "models/v_30cal.mdl", 0.2, 0.1, 0.02),
            row(29, "models/v_bazooka.mdl", 0.0, 0.1, 0.0),
        ]
    }

    fn gap_at(precision: Precision, viewmodel: &str, deployed: bool, speed: f32) -> Target {
        let table = table();
        target_for(
            weapon_for(viewmodel, &table).unwrap(),
            deployed,
            speed,
            precision,
        )
    }

    /// Where the precision makes no difference.
    fn gap(viewmodel: &str, deployed: bool, speed: f32) -> Target {
        let single = gap_at(Precision::Single, viewmodel, deployed, speed);
        assert_eq!(
            single,
            gap_at(Precision::Double, viewmodel, deployed, speed),
            "{viewmodel}"
        );
        single
    }

    /// The gaps the binary gives, worked by hand from its constants:
    /// `2 + trunc(200 * spread)`.
    #[test]
    fn the_gaps_are_the_games() {
        assert_eq!(gap("models/v_garand.mdl", false, 0.0), Target::Toward(5));
        assert_eq!(gap("models/v_garand.mdl", false, 250.0), Target::Toward(25));
        assert_eq!(gap("models/v_tommy.mdl", false, 0.0), Target::Toward(7));
        assert_eq!(gap("models/v_98k.mdl", false, 200.0), Target::Toward(34));
        assert_eq!(gap("models/v_mg42.mdl", false, 0.0), Target::Toward(42));
        assert_eq!(gap("models/v_mg42.mdl", true, 0.0), Target::Toward(7));
    }

    /// 0.02 as a float is 0.0199999995, so `200 * 0.02` is 3.9999999: kept
    /// at double precision and truncated to 3, rounded to 4.0 at single.
    #[test]
    fn a_spread_of_0_02_depends_on_the_precision() {
        for viewmodel in ["models/v_30cal.mdl", "models/v_bar.mdl"] {
            assert_eq!(
                gap_at(Precision::Single, viewmodel, true, 0.0),
                Target::Toward(6)
            );
            assert_eq!(
                gap_at(Precision::Double, viewmodel, true, 0.0),
                Target::Toward(5)
            );
        }
    }

    /// Deployed is the bipod or a sandbag, not prone: a Garand has no alt
    /// row worth the name, and is never deployed.
    #[test]
    fn deployed_swaps_the_base_spread_for_the_alt_one() {
        assert_eq!(gap("models/v_mg42.mdl", true, 300.0), Target::Toward(27));
        assert_eq!(gap("models/v_bar.mdl", false, 0.0), Target::Toward(7));
    }

    #[test]
    fn walking_pace_is_not_moving() {
        assert_eq!(gap("models/v_garand.mdl", false, 105.0), Target::Toward(5));
        assert_eq!(gap("models/v_garand.mdl", false, 105.5), Target::Toward(25));
    }

    #[test]
    fn grenades_and_rockets_are_fixed() {
        assert_eq!(gap("models/v_grenade.mdl", false, 300.0), Target::Fixed);
        assert_eq!(gap("models/v_bazooka.mdl", false, 0.0), Target::Fixed);
    }

    #[test]
    fn the_viewmodel_is_matched_whole_and_in_any_case() {
        let table = table();
        assert_eq!(
            weapon_for("models\\V_Garand.mdl", &table).map(|w| w.id),
            Some(5)
        );
        assert_eq!(weapon_for("models/v_garand_l.mdl", &table), None);
        assert_eq!(
            weapon_for("models/v_grenade.mdl", &table).map(|w| w.id),
            Some(13)
        );
    }

    /// `client+0x2cfb0`'s easing: up or down by 3 at most, landing on the
    /// target once within reach.
    #[test]
    fn the_gap_eases_three_pixels_a_frame() {
        assert_eq!(ease(5, 25), 8);
        assert_eq!(ease(23, 25), 25);
        assert_eq!(ease(25, 5), 22);
        assert_eq!(ease(6, 5), 5);
        assert_eq!(ease(5, 5), 5);
        let mut last = 42;
        let mut frames = 0;
        while last != 7 {
            last = ease(last, 7);
            frames += 1;
        }
        assert_eq!(frames, 12);
    }

    #[test]
    fn a_target_survives_its_code() {
        for target in [Target::Fixed, Target::Toward(0), Target::Toward(62)] {
            assert_eq!(Target::from_code(target.code()), target);
        }
    }

    #[test]
    fn the_speed_is_horizontal_over_the_window() {
        let mut meter = Speedometer::default();
        // 200 u/s along x, 20 snapshots a second, with a stair step.
        for i in 0..10 {
            let t = f64::from(i) * 0.05;
            let z = if i >= 5 { 18.0 } else { 0.0 };
            meter.snapshot(3, t, [200.0 * t as f32, 0.0, z]);
        }
        assert!((meter.speed() - 200.0).abs() < 0.5, "{}", meter.speed());
        assert!(meter.snapshots.len() <= 8, "{}", meter.snapshots.len());
    }

    #[test]
    fn standing_still_or_one_snapshot_is_no_speed() {
        let mut meter = Speedometer::default();
        meter.snapshot(3, 1.0, [10.0, 10.0, 0.0]);
        assert_eq!(meter.speed(), 0.0);
        meter.snapshot(3, 1.05, [10.0, 10.0, 0.0]);
        assert_eq!(meter.speed(), 0.0);
    }

    #[test]
    fn another_player_a_seek_or_a_long_gap_starts_afresh() {
        let mut meter = Speedometer::default();
        meter.snapshot(3, 1.0, [0.0, 0.0, 0.0]);
        meter.snapshot(3, 1.05, [10.0, 0.0, 0.0]);
        meter.snapshot(4, 1.10, [500.0, 0.0, 0.0]);
        assert_eq!(meter.speed(), 0.0);
        meter.snapshot(4, 0.5, [0.0, 0.0, 0.0]);
        assert_eq!(meter.snapshots.len(), 1);
        meter.snapshot(4, 2.0, [900.0, 0.0, 0.0]);
        assert_eq!(meter.speed(), 0.0);
    }

    #[test]
    fn the_known_rows_are_real_viewmodels() {
        for (_, path) in KNOWN_ROWS {
            assert!(path.starts_with("models/v_") && path.len() < PATH_MAX);
        }
    }
}
