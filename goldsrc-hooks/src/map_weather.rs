//! `ds_map_weather <0..3>`: rain or snow on any map, or none (#1129).
//!
//! ## What the game does
//!
//! DoD's client keeps one weather type, `g_iWeatherType` (`client+0x196c74`,
//! the same `client.dll` in both installs): 0 none, 1 rain, 2 snow. Its
//! writers are the map's `info_doddetect` entity (`detect_weather_type`, read
//! when `CHud::MsgFunc_InitHUD` parses the BSP, `client+0x2be1e`) and
//! `CWeatherManager::Init` / `::VidInit`, which zero it at every map load.
//! Its one reader is the weather updater, once a frame:
//!
//! ```text
//!     client+0x4c929  if (time >= g_flWeatherTime && cl_particlefx >= 2)
//!     client+0x4c951    mov eax, [g_iWeatherType]
//!                       1: rain, 350 drops    g_flWeatherTime = time + 0.3
//!                       2: snow               g_flWeatherTime = time + 0.7
//! ```
//!
//! The spawn runs by client time, and both steps are longer than a frame at
//! 60 fps, so a 300 fps capture gets the same density as 60. `VidInit` loads
//! `rain.spr`, `snowflake.spr`, `wsplash3.spr` and `ripple.spr` on every map,
//! so forcing a type needs nothing else loaded. The drops spawn only under
//! open sky (a trace up must hit a sky texture), as on a rainy map.
//!
//! ## The change
//!
//! The updater's `mov eax, [g_iWeatherType]` is pointed at [`FORCED`], an int
//! of our own, rather than the game's variable being written. So the map's
//! own value stays the game's: every map load still zeroes and re-reads it,
//! nothing has to be put back after `VidInit`, and setting 0 again (the
//! map's own) points the read back at the game's variable, which always holds
//! the loaded map's weather. A game that never asks for anything but 0 is
//! never written to. `client.dll` is never reloaded between demos (CLAUDE.md),
//! and a reloaded copy would be rescanned from its new base.
//!
//! Values: 0 the map's own, 1 rain, 2 snow, 3 none. 1 and 2 are the game's
//! own numbers for rain and snow. A value past 3 or below 0 is set back to
//! the nearest one, with a console line. Forcing rain or snow with
//! `cl_particlefx` below 2 says so in the console, since the game then draws
//! no weather at all.
//!
//! The patch runs from `commands::poll` on the engine thread. A frame where
//! the cvar hasn't changed costs one float compare.
//!
//! `tools/verify_map_weather_offsets.py` checks the pattern is unique and its
//! operand is `g_iWeatherType` in both installs' `client.dll`.
//!
//! Wind is not part of it yet: the map's `detect_wind_velocity_x/y` go to
//! `CDoDParticle::m_vGlobalWind` (`client+0x196c60`, a vec3 written only by
//! `client+0x4d340`, which the same BSP parse calls), so a forced rain falls
//! straight on a map with no wind of its own.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering};

use crate::engine::{self};
use crate::names::console_name;
use crate::scan;
use crate::setting::Cvar;

pub const NAME: &str = console_name!("map_weather");

/// What each setting asks for. `Some` is the weather type the updater reads
/// in its place; `None` is the map's own.
const FORCED_TYPE: [Option<i32>; 4] = [None, Some(RAIN), Some(SNOW), Some(NONE)];
pub const MAX_LEVEL: i32 = FORCED_TYPE.len() as i32 - 1;

/// The game's own weather types.
const NONE: i32 = 0;
const RAIN: i32 = 1;
const SNOW: i32 = 2;

/// The weather draws only with `cl_particlefx` at this or above (the
/// updater's `fcomp` against 2.0 at `client+0x4c942`).
const PARTICLEFX_NEEDED: f32 = 2.0;

/// From the `cl_particlefx` check's `fcomp` to the type's `dec eax; jne`; the
/// two absolute operands are wildcards, so a rescan finds the site patched or
/// not.
const PATTERN: &str = "D8 1D ?? ?? ?? ?? DF E0 25 00 01 00 00 75 31 \
                       A1 ?? ?? ?? ?? 48 74 14 48 75 26";
/// The `mov eax, [moffs32]`'s operand within [`PATTERN`].
const OPERAND_AT: usize = 16;

/// The weather type the patched read sees. The client code holds its
/// address, so it stays a plain 4-byte static.
static FORCED: AtomicI32 = AtomicI32::new(NONE);
pub static CVAR: Cvar = Cvar::new();
/// The cvar's raw f32 bits the last time it was acted on; NaN until then.
static SEEN: AtomicU32 = AtomicU32::new(0x7fc0_0000);
/// The operand's address, for the `client.dll` base in [`SCANNED_BASE`].
static ADDRESS: AtomicUsize = AtomicUsize::new(0);
static SCANNED_BASE: AtomicUsize = AtomicUsize::new(0);
/// The game's own operand (`g_iWeatherType`'s address), read before the
/// first write.
static STOCK_OPERAND: AtomicU32 = AtomicU32::new(0);
static GAVE_UP: AtomicBool = AtomicBool::new(false);

/// The level a cvar value asks for, and whether it had to be clamped to get
/// there. A fraction rounds down; NaN and infinities read as 0.
pub(crate) fn level_for(raw: f32) -> (i32, bool) {
    if !raw.is_finite() {
        return (0, true);
    }
    let floor = raw.floor();
    if floor < 0.0 {
        (0, true)
    } else if floor > MAX_LEVEL as f32 {
        (MAX_LEVEL, true)
    } else {
        (floor as i32, false)
    }
}

/// The weather type a level forces, or `None` for the map's own.
pub(crate) fn forced_type(level: i32) -> Option<i32> {
    FORCED_TYPE[level.clamp(0, MAX_LEVEL) as usize]
}

/// A weather type in words, for the console and the log.
fn describe(weather_type: i32) -> &'static str {
    match weather_type {
        RAIN => "rain",
        SNOW => "snow",
        _ => "none",
    }
}

fn describe_level(level: i32) -> &'static str {
    forced_type(level).map_or("the map's own", describe)
}

/// The operand's address in the loaded `client.dll`, scanning once per base.
fn operand_address(base: usize) -> Result<usize, String> {
    if SCANNED_BASE.load(Ordering::Acquire) == base {
        let cached = ADDRESS.load(Ordering::Acquire);
        if cached != 0 {
            return Ok(cached);
        }
    }
    // Safety: a mapped module, for the session.
    let found = unsafe { scan::find_unique(base, PATTERN) }
        .map_err(|why| format!("could not find the weather updater's type read -- {why}"))?;
    let address = found + OPERAND_AT;
    // Safety: inside the pattern just matched.
    let operand = unsafe { (address as *const u32).read_unaligned() };
    let ours = &FORCED as *const AtomicI32 as usize as u32;
    if operand != ours {
        // The game's variable sits in client.dll's own image; anything else
        // means another patch got there first.
        // Safety: `base` is a mapped module.
        let size = unsafe { crate::pe::image_size(base as *mut u8) }.unwrap_or(0);
        let inside = (operand as usize)
            .checked_sub(base)
            .is_some_and(|at| at + 4 <= size);
        if !inside {
            return Err(format!(
                "the weather updater reads {operand:#x}, outside client.dll -- something else has patched it"
            ));
        }
        STOCK_OPERAND.store(operand, Ordering::Release);
    }
    ADDRESS.store(address, Ordering::Release);
    SCANNED_BASE.store(base, Ordering::Release);
    Ok(address)
}

/// Points the updater's read at the game's own variable for level 0 and at
/// [`FORCED`] otherwise. Returns whether it wrote code.
fn apply(level: i32) -> Result<bool, String> {
    let base = engine::client_module_base().ok_or("client.dll is not loaded yet")?;
    let address = operand_address(base)?;
    let ours = &FORCED as *const AtomicI32 as usize as u32;
    if let Some(weather_type) = forced_type(level) {
        FORCED.store(weather_type, Ordering::Release);
    }
    let stock = STOCK_OPERAND.load(Ordering::Acquire);
    let want = if forced_type(level).is_none() {
        stock
    } else {
        ours
    };
    // Safety: `operand_address` proved this is the operand's 4 bytes.
    let present = unsafe { (address as *const u32).read_unaligned() };
    if present == want {
        return Ok(false);
    }
    if present != stock && present != ours {
        return Err(format!(
            "the weather updater's operand holds {present:#x} -- something else has patched it"
        ));
    }
    if !unsafe { crate::patch::write_code_bytes(address, &want.to_le_bytes()) } {
        return Err("could not make the weather updater writable".to_string());
    }
    Ok(true)
}

/// `cl_particlefx`, or `None` before the engine is up.
fn particlefx() -> Option<f32> {
    let engfuncs = engine::engfuncs()?;
    Some(unsafe { (engfuncs.pfn_get_cvar_float)(c"cl_particlefx".as_ptr()) })
}

/// For `ds_debug_status`: what the weather updater reads now, once this
/// module has looked at the code (a setting above 0 was asked for) or given
/// up.
pub fn status_line() -> Option<String> {
    if GAVE_UP.load(Ordering::Relaxed) {
        return Some(format!(
            "weather: {NAME} could not be applied (see the log)"
        ));
    }
    let address = ADDRESS.load(Ordering::Acquire);
    if address == 0 {
        return None;
    }
    // Safety: the operand found by `operand_address`, for this session's client.dll.
    let present = unsafe { (address as *const u32).read_unaligned() };
    let ours = &FORCED as *const AtomicI32 as usize as u32;
    let what = if present == ours {
        format!("forced {}", describe(FORCED.load(Ordering::Relaxed)))
    } else if present != 0 && present == STOCK_OPERAND.load(Ordering::Acquire) {
        // Safety: the game's own variable, inside client.dll's image
        // (`operand_address` checked it).
        let own = unsafe { *(present as usize as *const i32) };
        format!("the map's own ({})", describe(own))
    } else {
        // Someone else rewrote the operand after us: don't follow it.
        format!("the updater reads {present:#x}, not the game's variable or ours")
    };
    let fx = particlefx().map_or_else(|| "?".to_string(), |v| v.to_string());
    Some(format!(
        "weather: {what}; cl_particlefx {fx} (draws at 2 or above)"
    ))
}

/// The console line for a level just applied, warning when the game won't
/// draw the weather asked for.
fn applied_message(level: i32, particlefx: Option<f32>) -> String {
    let mut line = format!("{NAME} {level}: {}", describe_level(level));
    let draws_weather = matches!(forced_type(level), Some(RAIN) | Some(SNOW));
    if draws_weather && particlefx.is_some_and(|v| v < PARTICLEFX_NEEDED) {
        line.push_str(
            " -- cl_particlefx is below 2, so the game draws no weather: set cl_particlefx 2",
        );
    }
    line.push('\n');
    line
}

/// Once a frame from `commands::poll`, on the engine thread.
pub fn poll() {
    let Some(raw) = CVAR.value() else {
        return;
    };
    if GAVE_UP.load(Ordering::Relaxed) {
        return;
    }
    if SEEN.load(Ordering::Relaxed) == raw.to_bits() {
        return;
    }
    let (level, clamped) = level_for(raw);
    if clamped {
        crate::commands::console_print(&format!(
            "{NAME} takes 0 to {MAX_LEVEL}; {raw} gives {level}\n"
        ));
        // The cvar then reads the level in effect; the next frame applies it.
        if let Ok(line) = std::ffi::CString::new(format!("{NAME} {level}")) {
            engine::client_cmd(&line);
        }
        SEEN.store(raw.to_bits(), Ordering::Relaxed);
        return;
    }
    // The map's own in a game that was never patched: nothing to put back.
    if level == 0 && STOCK_OPERAND.load(Ordering::Relaxed) == 0 {
        SEEN.store(raw.to_bits(), Ordering::Relaxed);
        return;
    }
    match apply(level) {
        Ok(wrote) => {
            SEEN.store(raw.to_bits(), Ordering::Relaxed);
            crate::commands::console_print(&applied_message(level, particlefx()));
            if wrote {
                unsafe {
                    crate::debug::report(&format!(
                        "map_weather: {NAME} = {level} ({})",
                        describe_level(level)
                    ))
                };
            }
        }
        // Retry next frame until client.dll is there.
        Err(_) if engine::client_module_base().is_none() => {}
        Err(why) => {
            GAVE_UP.store(true, Ordering::Relaxed);
            crate::commands::console_print(&format!("{NAME}: not applied -- {why}\n"));
            unsafe { crate::debug::report(&format!("map_weather: {why}")) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_level_forces_its_weather() {
        let forced: Vec<Option<i32>> = (0..=MAX_LEVEL).map(forced_type).collect();
        assert_eq!(forced, [None, Some(1), Some(2), Some(0)]);
        assert_eq!(MAX_LEVEL, 3);
    }

    /// 1 and 2 are the game's own numbers, so the setting reads the way the
    /// map's `detect_weather_type` does.
    #[test]
    fn rain_and_snow_keep_the_game_s_numbers() {
        assert_eq!(describe_level(1), "rain");
        assert_eq!(describe_level(2), "snow");
        assert_eq!(describe_level(3), "none");
        assert_eq!(describe_level(0), "the map's own");
    }

    #[test]
    fn values_in_range_are_kept_and_fractions_round_down() {
        for (raw, level) in [(0.0, 0), (1.0, 1), (2.0, 2), (3.0, 3), (2.9, 2)] {
            assert_eq!(level_for(raw), (level, false), "{raw}");
        }
    }

    #[test]
    fn values_out_of_range_clamp_and_say_so() {
        assert_eq!(level_for(4.0), (3, true));
        assert_eq!(level_for(-1.0), (0, true));
        assert_eq!(level_for(f32::NAN), (0, true));
        assert_eq!(level_for(f32::INFINITY), (0, true));
    }

    #[test]
    fn forcing_rain_or_snow_without_particles_says_so() {
        assert!(applied_message(1, Some(1.0)).contains("cl_particlefx 2"));
        assert!(applied_message(2, Some(0.0)).contains("cl_particlefx 2"));
        assert!(!applied_message(1, Some(2.0)).contains("cl_particlefx"));
        // No weather drawn either way, so nothing to warn about.
        assert!(!applied_message(3, Some(0.0)).contains("cl_particlefx"));
        assert!(!applied_message(0, Some(0.0)).contains("cl_particlefx"));
    }

    #[test]
    fn the_operand_is_the_type_read_s_moffs() {
        let tokens: Vec<&str> = PATTERN.split_whitespace().collect();
        // mov eax, [moffs32] is A1 followed by the address.
        assert_eq!(tokens[OPERAND_AT - 1], "A1");
        assert!(
            tokens[OPERAND_AT..OPERAND_AT + 4]
                .iter()
                .all(|t| *t == "??")
        );
        // ...and the dec/je/dec/jne on the type follows it.
        assert_eq!(tokens[OPERAND_AT + 4..].join(" "), "48 74 14 48 75 26");
    }
}
