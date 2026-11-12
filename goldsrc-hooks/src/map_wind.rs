//! `ds_map_wind "<x> <y>"`: the wind that blows rain, snow and smoke, in
//! units a second along the map's x and y axes, or `0` for the map's own.
//!
//! ## What the game does
//!
//! DoD's client keeps one wind, `CDoDParticle::m_vGlobalWind` (a `Vector` at
//! `client+0x196c60`, the same `client.dll` in both installs; its name and
//! type are in the Linux `client.so`'s DWARF). Its one writer is
//! `CDoDParticle::SetGlobalWind` (`client+0x4d340`), which only the
//! `info_doddetect` parse calls (`client+0x2be67`, the BSP parse
//! `CHud::MsgFunc_InitHUD` runs at each map load): x and y from the entity's
//! `detect_wind_velocity_x/y`, 0 when the keys are missing. A map without
//! the entity keeps the last map's wind. Of the movie installs' maps, 17 set
//! a wind (25 25 on all but one) and the rest 0.
//!
//! Its readers add x and y to a particle's velocity once, as it is made, and
//! never read z:
//!
//! ```text
//!     client+0x4d360  CDoDParticle::AddGlobalWind: muzzle smoke, bazooka
//!                     smoke, leaves, the maps' smoke shooters
//!     client+0x4e1cc  CWeatherManager::CreateRainParticle (inlined copy)
//!     client+0x4e37c  CWeatherManager::CreateSnowParticle (inlined copy)
//!                     fld [wind.x]; fadd [p+0x80]; fstp [p+0x80]
//!                     fld [wind.y]; fadd [p+0x84]; fstp [p+0x84]
//! ```
//!
//! So a new wind reaches every particle made after it, the next rain drop
//! within 0.3 s; one already in the air keeps its own.
//!
//! ## The change
//!
//! As `map_weather` does for the weather type: the readers' six operands
//! are pointed at [`FORCED`], two floats of our own, rather than the game's
//! variable being written. The game's variable keeps the map's own wind
//! through every map load, and `0` points the reads back at it. A game that
//! never asks for a wind is never written to.
//!
//! The map's own wind goes into the log whenever the loaded map or its wind
//! changes and has held for half a second, so a log shows what the map asked
//! for (a load names the new level ~0.2 s before the map's wind is read).
//! That read is two float loads and a level-name hash a frame; the string
//! cvar is parsed again only when the engine hands it a new string.
//!
//! `tools/verify_map_wind_offsets.py` checks the patterns and the variable
//! in both installs' `client.dll`.

use std::ffi::CStr;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use crate::engine::{self, CvarSPartial};
use crate::names::console_name;
use crate::scan;
use crate::setting::Cvar;

pub const NAME: &str = console_name!("map_wind");
pub const DEFAULT: &str = "0";
pub static CVAR: Cvar = Cvar::new();

/// `CDoDParticle::AddGlobalWind`, the function every other effect calls.
const ADD_PATTERN: &str = "D9 05 ?? ?? ?? ?? D8 81 80 00 00 00 D9 99 80 00 00 00 \
                           D9 05 ?? ?? ?? ?? D8 81 84 00 00 00";
/// Its copy the compiler inlined into the rain and the snow particle
/// makers, byte for byte the same in both.
const INLINED_PATTERN: &str = "D9 05 ?? ?? ?? ?? D8 86 80 00 00 00 D9 9E 80 00 00 00 \
                               D9 05 ?? ?? ?? ?? D8 86 84 00 00 00";
const INLINED_COPIES: usize = 2;
/// The `fld`s' operands, wind.x and wind.y, within either pattern.
const X_AT: usize = 2;
const Y_AT: usize = 20;
const SITES: usize = 1 + INLINED_COPIES;

/// The wind the patched reads see, x and y as f32 bits. The client code
/// holds their addresses, so they stay plain 4-byte statics.
static FORCED: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];
/// Each site's x operand's address (its y operand is `Y_AT - X_AT` on), for
/// the `client.dll` base in [`SCANNED_BASE`].
static OPERANDS: [AtomicUsize; SITES] = [const { AtomicUsize::new(0) }; SITES];
static SCANNED_BASE: AtomicUsize = AtomicUsize::new(0);
/// The game's own `m_vGlobalWind`, read from the operands before any write.
static GAME_WIND: AtomicUsize = AtomicUsize::new(0);
/// Whether the reads were ever pointed at [`FORCED`].
static PATCHED: AtomicBool = AtomicBool::new(false);
static GAVE_UP: AtomicBool = AtomicBool::new(false);
/// The cvar string last acted on (the engine allocates a new one on every
/// change).
static SEEN_STRING: AtomicUsize = AtomicUsize::new(0);
/// The level-name hash and map wind last logged.
static LOGGED_LEVEL: AtomicU32 = AtomicU32::new(0);
static LOGGED_WIND: AtomicU64 = AtomicU64::new(u64::MAX);
/// A level and wind seen but not logged yet, and since when (ms).
static PENDING_LEVEL: AtomicU32 = AtomicU32::new(0);
static PENDING_WIND: AtomicU64 = AtomicU64::new(u64::MAX);
static PENDING_SINCE: AtomicU64 = AtomicU64::new(0);
/// How long a new level and wind must hold before they are logged.
const SETTLE_MS: u64 = 500;

/// What a value asks for: `None` the map's own (`0` or nothing), or x and
/// y. Anything else is an error saying why.
pub(crate) fn parse(text: &str) -> Result<Option<[f32; 2]>, String> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    let number = |part: &str| {
        part.parse::<f32>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| format!("\"{part}\" is not a number"))
    };
    match parts.as_slice() {
        [] => Ok(None),
        [one] if number(one)? == 0.0 => Ok(None),
        [x, y] => Ok(Some([number(x)?, number(y)?])),
        _ => Err(format!(
            "needs two numbers, x y, or 0 for the map's own; got {}",
            parts.len()
        )),
    }
}

fn describe(wind: Option<[f32; 2]>) -> String {
    match wind {
        None => "the map's own".to_string(),
        Some([x, y]) => format!("x {x} y {y} units a second"),
    }
}

fn pack(wind: [f32; 2]) -> u64 {
    u64::from(wind[0].to_bits()) | (u64::from(wind[1].to_bits()) << 32)
}

/// The two operands of one site, x first.
fn operand_pair(x_operand: usize) -> [usize; 2] {
    [x_operand, x_operand + (Y_AT - X_AT)]
}

/// Reads one site's operands; they must name one vector's x and y.
fn read_pair(x_operand: usize) -> [u32; 2] {
    // Safety: inside a pattern `scan_sites` matched in the mapped module.
    operand_pair(x_operand).map(|at| unsafe { (at as *const u32).read_unaligned() })
}

fn ours() -> [u32; 2] {
    [0, 1].map(|i| &FORCED[i] as *const AtomicU32 as usize as u32)
}

/// Finds the sites in a freshly loaded `client.dll` and the game's variable
/// they read.
fn scan_sites(base: usize) -> Result<(), String> {
    // Safety: a mapped module, for the session.
    let add = unsafe { scan::find_unique(base, ADD_PATTERN) }
        .map_err(|why| format!("could not find CDoDParticle::AddGlobalWind -- {why}"))?;
    let inlined = unsafe { scan::find_exactly(base, INLINED_PATTERN, INLINED_COPIES) }
        .map_err(|why| format!("could not find the rain and snow wind reads -- {why}"))?;
    let sites: Vec<usize> = std::iter::once(add)
        .chain(inlined)
        .map(|at| at + X_AT)
        .collect();
    let [x, y] = read_pair(sites[0]);
    // Safety: `base` is a mapped module.
    let size = unsafe { crate::pe::image_size(base as *mut u8) }.unwrap_or(0);
    let inside = (x as usize)
        .checked_sub(base)
        .is_some_and(|at| at + 8 <= size);
    if !inside || y != x.wrapping_add(4) {
        return Err(format!(
            "the wind reads name {x:#x} and {y:#x}, not one vector in client.dll -- something else has patched them"
        ));
    }
    if let Some(&odd) = sites.iter().find(|&&at| read_pair(at) != [x, y]) {
        return Err(format!(
            "the wind read at +{:#x} names another variable",
            odd - base
        ));
    }
    for (slot, at) in OPERANDS.iter().zip(&sites) {
        slot.store(*at, Ordering::Release);
    }
    GAME_WIND.store(x as usize, Ordering::Release);
    SCANNED_BASE.store(base, Ordering::Release);
    Ok(())
}

/// The game's own wind (the loaded map's), once the sites are known.
fn map_wind() -> Option<[f32; 2]> {
    let at = GAME_WIND.load(Ordering::Acquire);
    // Safety: `m_vGlobalWind` inside client.dll's image (`scan_sites`
    // checked it).
    (at != 0).then(|| unsafe { [0, 4].map(|o| *((at + o) as *const f32)) })
}

/// Points every read at the game's variable for `None` and at [`FORCED`]
/// otherwise. Returns whether it wrote code.
fn apply(wind: Option<[f32; 2]>) -> Result<bool, String> {
    if let Some(wind) = wind {
        for (slot, v) in FORCED.iter().zip(wind) {
            slot.store(v.to_bits(), Ordering::Release);
        }
    }
    let game = GAME_WIND.load(Ordering::Acquire) as u32;
    let stock = [game, game.wrapping_add(4)];
    let want = if wind.is_some() { ours() } else { stock };
    let mut wrote = false;
    for slot in &OPERANDS {
        let x_operand = slot.load(Ordering::Acquire);
        let present = read_pair(x_operand);
        if present == want {
            continue;
        }
        if present != stock && present != ours() {
            return Err(format!(
                "a wind read holds {:#x} -- something else has patched it",
                present[0]
            ));
        }
        for (at, value) in operand_pair(x_operand).into_iter().zip(want) {
            // Safety: one of the operands `scan_sites` proved.
            if !unsafe { crate::patch::write_code_bytes(at, &value.to_le_bytes()) } {
                return Err("could not make the wind reads writable".to_string());
            }
        }
        wrote = true;
    }
    PATCHED.fetch_or(wind.is_some(), Ordering::Relaxed);
    Ok(wrote)
}

/// The engine's level name (`maps/dod_kalt.bsp`), or `None` without one.
fn level_name() -> Option<&'static [u8]> {
    let engfuncs = engine::engfuncs()?;
    // Safety: a pointer into the engine's client state, valid for the
    // session; checked for null before reading.
    let raw = unsafe { (engfuncs.pfn_get_level_name)() };
    if raw.is_null() {
        return None;
    }
    let name = unsafe { CStr::from_ptr(raw) }.to_bytes();
    (!name.is_empty()).then_some(name)
}

fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5_u32, |h, &b| {
        (h ^ b as u32).wrapping_mul(0x0100_0193)
    })
}

fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Whether a level and wind not yet logged have held long enough to log:
/// the engine names the new level a moment before the map's
/// `info_doddetect` is parsed, so the first frames of a load still hold the
/// last map's wind.
fn settled(level: u32, wind: u64, now: u64) -> bool {
    if PENDING_LEVEL.load(Ordering::Relaxed) != level
        || PENDING_WIND.load(Ordering::Relaxed) != wind
    {
        PENDING_LEVEL.store(level, Ordering::Relaxed);
        PENDING_WIND.store(wind, Ordering::Relaxed);
        PENDING_SINCE.store(now, Ordering::Relaxed);
        return false;
    }
    now.saturating_sub(PENDING_SINCE.load(Ordering::Relaxed)) >= SETTLE_MS
}

/// Logs the map's own wind when the level or the wind changes.
fn log_map_wind() {
    let (Some(wind), Some(name)) = (map_wind(), level_name()) else {
        return;
    };
    let (bits, hash) = (pack(wind), fnv1a(name));
    if LOGGED_WIND.load(Ordering::Relaxed) == bits && LOGGED_LEVEL.load(Ordering::Relaxed) == hash {
        return;
    }
    if !settled(hash, bits, now_ms()) {
        return;
    }
    LOGGED_WIND.store(bits, Ordering::Relaxed);
    LOGGED_LEVEL.store(hash, Ordering::Relaxed);
    unsafe {
        crate::debug::report(&format!(
            "map_wind: {}: the map's own wind is x {} y {}",
            String::from_utf8_lossy(name),
            wind[0],
            wind[1]
        ))
    };
}

/// The cvar's string when it changed since the last frame, or `None`.
fn changed_value() -> Option<(usize, String)> {
    let ptr: *mut CvarSPartial = CVAR.ptr();
    if ptr.is_null() {
        return None;
    }
    // Safety: a registered cvar lives for the session.
    let string = unsafe { (*ptr).string };
    if string.is_null() || SEEN_STRING.load(Ordering::Relaxed) == string as usize {
        return None;
    }
    // Safety: the engine's NUL-terminated copy of the value.
    let text = unsafe { CStr::from_ptr(string) }
        .to_string_lossy()
        .into_owned();
    Some((string as usize, text))
}

/// For `ds_debug_status`: the wind the particles get, once the sites are
/// known.
pub fn status_line() -> Option<String> {
    if GAVE_UP.load(Ordering::Relaxed) {
        return Some(format!("wind: {NAME} could not be applied (see the log)"));
    }
    let own = map_wind()?;
    let x_operand = OPERANDS[0].load(Ordering::Acquire);
    let what = if read_pair(x_operand) == ours() {
        let forced = [0, 1].map(|i| f32::from_bits(FORCED[i].load(Ordering::Relaxed)));
        format!("forced {}", describe(Some(forced)))
    } else {
        "the map's own".to_string()
    };
    Some(format!(
        "wind: {what}; the map's own is x {} y {}",
        own[0], own[1]
    ))
}

fn give_up(why: &str) {
    GAVE_UP.store(true, Ordering::Relaxed);
    crate::commands::console_print(&format!("{NAME}: not applied -- {why}\n"));
    unsafe { crate::debug::report(&format!("map_wind: {why}")) };
}

/// Once a frame from `commands::poll`, on the engine thread.
pub fn poll() {
    if GAVE_UP.load(Ordering::Relaxed) {
        return;
    }
    let Some(base) = engine::client_module_base() else {
        return;
    };
    if SCANNED_BASE.load(Ordering::Acquire) != base {
        if let Err(why) = scan_sites(base) {
            give_up(&why);
            return;
        }
        // A reloaded client.dll has the stock reads: apply the setting again.
        SEEN_STRING.store(0, Ordering::Relaxed);
        PATCHED.store(false, Ordering::Relaxed);
    }
    log_map_wind();
    let Some((string, text)) = changed_value() else {
        return;
    };
    SEEN_STRING.store(string, Ordering::Relaxed);
    let wind = parse(&text).unwrap_or_else(|why| {
        crate::commands::console_print(&format!(
            "{NAME} \"{text}\": {why}; keeping the map's own.\n"
        ));
        None
    });
    // The map's own in a game that was never patched: nothing to put back.
    if wind.is_none() && !PATCHED.load(Ordering::Relaxed) {
        return;
    }
    match apply(wind) {
        Ok(wrote) => {
            crate::commands::console_print(&format!("{NAME}: {}\n", describe(wind)));
            if wrote {
                unsafe { crate::debug::report(&format!("map_wind: {NAME} = {}", describe(wind))) };
            }
        }
        Err(why) => give_up(&why),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_or_zero_is_the_map_s_own() {
        for text in ["", "  ", "0", "0.0", "-0"] {
            assert_eq!(parse(text), Ok(None), "{text:?}");
        }
    }

    /// `0 0` is a calm, not the map's own: on a map with wind, it stops it.
    #[test]
    fn two_numbers_are_a_wind() {
        assert_eq!(parse("0 0"), Ok(Some([0.0, 0.0])));
        assert_eq!(parse("300 -150.5"), Ok(Some([300.0, -150.5])));
        assert_eq!(parse(" 25\t25 "), Ok(Some([25.0, 25.0])));
    }

    #[test]
    fn anything_else_says_why() {
        assert!(parse("25").unwrap_err().contains("two numbers"));
        assert!(parse("1 2 3").unwrap_err().contains("two numbers"));
        assert!(parse("east 10").unwrap_err().contains("\"east\""));
        assert!(parse("nan 10").unwrap_err().contains("\"nan\""));
        assert!(parse("10 inf").unwrap_err().contains("\"inf\""));
    }

    #[test]
    fn both_patterns_read_x_then_y_with_fld() {
        for pattern in [ADD_PATTERN, INLINED_PATTERN] {
            let tokens: Vec<&str> = pattern.split_whitespace().collect();
            for at in [X_AT, Y_AT] {
                // fld dword [moffs32] is D9 05 followed by the address.
                assert_eq!(tokens[at - 2..at], ["D9", "05"], "{pattern}");
                assert!(tokens[at..at + 4].iter().all(|t| *t == "??"));
            }
            // ...each then added to the particle's velocity x (+0x80), y (+0x84).
            assert_eq!(tokens[X_AT + 6], "80");
            assert_eq!(tokens[Y_AT + 6], "84");
        }
    }

    /// A load's first frames still hold the last map's wind: only a pair
    /// that held for `SETTLE_MS` is logged.
    #[test]
    fn a_new_level_and_wind_are_logged_once_they_hold() {
        assert!(!settled(7, 1, 1_000));
        assert!(!settled(7, 1, 1_000 + SETTLE_MS - 1));
        assert!(settled(7, 1, 1_000 + SETTLE_MS));
        // The wind read 0.2 s into the load starts the wait again.
        assert!(!settled(7, 2, 1_200 + SETTLE_MS));
        assert!(settled(7, 2, 1_200 + 2 * SETTLE_MS));
    }

    #[test]
    fn a_site_s_y_operand_follows_its_x() {
        assert_eq!(operand_pair(0x1000), [0x1000, 0x1012]);
    }
}
