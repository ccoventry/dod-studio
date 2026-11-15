//! X-ray's team colours (#1297): `ds_spec_xray_allies_color` and
//! `ds_spec_xray_axis_color`, "R G B" with each 0-255, as the game's own
//! colour cvars take them (`con_color "255 180 30"`). Defaults: Allies green,
//! Axis red. A value that isn't three whole numbers 0-255 draws the default
//! and says why in the console, once per value typed.

use std::ffi::CStr;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use crate::engine::CvarSPartial;
use crate::names::console_name;
use crate::setting::Cvar;

pub const ALLIES_NAME: &str = console_name!("spec_xray_allies_color");
pub const AXIS_NAME: &str = console_name!("spec_xray_axis_color");

/// The defaults, as the cvars are registered with them: the hues of DoD's
/// kill-feed team colours (`deathmsg::backdrop`'s (76, 102, 76) and
/// (255, 63, 63)), green and red, at a strength that stays a colour rather
/// than white once the engine has lit the model (its brightness tables double
/// a model's light on the movie installs' `brightness 2`).
pub const ALLIES_DEFAULT: &str = "50 145 50";
pub const AXIS_DEFAULT: &str = "160 40 40";

pub static ALLIES: Cvar = Cvar::new();
pub static AXIS: Cvar = Cvar::new();

/// One team's colour cvar: the string it was last read from (its pointer;
/// the engine allocates a new one on every change) and what it gave.
pub(crate) struct Cached {
    string: AtomicUsize,
    rgb: AtomicU32,
}

impl Cached {
    pub(crate) const fn new() -> Self {
        Self {
            string: AtomicUsize::new(0),
            rgb: AtomicU32::new(0),
        }
    }
}

static ALLIES_CACHE: Cached = Cached::new();
static AXIS_CACHE: Cached = Cached::new();

/// "R G B", each a whole number 0-255, separated by spaces.
pub fn parse_rgb(text: &str) -> Result<[u8; 3], String> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    if parts.len() != 3 {
        return Err(format!("needs three numbers, R G B, got {}", parts.len()));
    }
    let mut rgb = [0u8; 3];
    for (out, part) in rgb.iter_mut().zip(&parts) {
        *out = part
            .parse::<u8>()
            .map_err(|_| format!("\"{part}\" is not a whole number 0-255"))?;
    }
    Ok(rgb)
}

fn pack(rgb: [u8; 3]) -> u32 {
    u32::from_le_bytes([rgb[0], rgb[1], rgb[2], 0])
}

fn unpack(packed: u32) -> [u8; 3] {
    let [r, g, b, _] = packed.to_le_bytes();
    [r, g, b]
}

/// The Allies' colour.
pub fn allies() -> [u8; 3] {
    read(&ALLIES, &ALLIES_CACHE, ALLIES_NAME, ALLIES_DEFAULT)
}

/// The Axis' colour.
pub fn axis() -> [u8; 3] {
    read(&AXIS, &AXIS_CACHE, AXIS_NAME, AXIS_DEFAULT)
}

fn default_rgb(default: &str) -> [u8; 3] {
    parse_rgb(default).unwrap_or([255, 255, 255])
}

/// The cvar's colour, parsed again only when its string changed; per frame
/// that is two loads and a compare.
pub(crate) fn read(cvar: &Cvar, cache: &Cached, name: &str, default: &str) -> [u8; 3] {
    let ptr: *mut CvarSPartial = cvar.ptr();
    if ptr.is_null() {
        return default_rgb(default);
    }
    // Safety: a registered cvar lives for the session.
    let string = unsafe { (*ptr).string };
    if string.is_null() {
        return default_rgb(default);
    }
    if cache.string.load(Ordering::Relaxed) == string as usize {
        return unpack(cache.rgb.load(Ordering::Relaxed));
    }
    // Safety: the engine's NUL-terminated copy of the value.
    let text = unsafe { CStr::from_ptr(string) }.to_string_lossy();
    let rgb = match parse_rgb(&text) {
        Ok(rgb) => rgb,
        Err(why) => {
            crate::commands::console_print(&format!(
                "{name} \"{text}\": {why}; using the default \"{default}\".\n"
            ));
            default_rgb(default)
        }
    };
    cache.rgb.store(pack(rgb), Ordering::Relaxed);
    cache.string.store(string as usize, Ordering::Relaxed);
    rgb
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_numbers_0_to_255_make_a_colour() {
        assert_eq!(parse_rgb("50 145 50"), Ok([50, 145, 50]));
        assert_eq!(parse_rgb("  0   0 255 "), Ok([0, 0, 255]));
        assert_eq!(parse_rgb("255\t255 255"), Ok([255, 255, 255]));
    }

    #[test]
    fn anything_else_says_why() {
        assert!(parse_rgb("").unwrap_err().contains("got 0"));
        assert!(parse_rgb("1 2").unwrap_err().contains("got 2"));
        assert!(parse_rgb("1 2 3 4").unwrap_err().contains("got 4"));
        assert!(parse_rgb("1 2 256").unwrap_err().contains("\"256\""));
        assert!(parse_rgb("1 -2 3").unwrap_err().contains("\"-2\""));
        assert!(parse_rgb("1 2.5 3").unwrap_err().contains("\"2.5\""));
        assert!(parse_rgb("#ff0000").is_err());
    }

    /// Allies green and Axis red by default (#1297).
    #[test]
    fn the_defaults_are_allies_green_and_axis_red() {
        let [r, g, b] = parse_rgb(ALLIES_DEFAULT).unwrap();
        assert!(g > r && g > b, "Allies green");
        let [r, g, b] = parse_rgb(AXIS_DEFAULT).unwrap();
        assert!(r > g && r > b, "Axis red");
    }

    #[test]
    fn colours_pack_and_unpack() {
        for rgb in [[0, 0, 0], [50, 145, 50], [255, 1, 128]] {
            assert_eq!(unpack(pack(rgb)), rgb);
        }
    }
}
