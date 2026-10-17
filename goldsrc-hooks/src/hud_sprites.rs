//! `dodstudio_hide_asset` for HUD sprites (#808): the sheets DoD's HUD draws
//! in 2D (`sprites/hud_icons.spr`, `clip_bullets.spr`, `hud_mgbarrels.spr`...)
//! and the icons `sprites/hud.txt` cuts out of them.
//!
//! The HUD never goes through `HUD_AddEntity`, an effect or a particle: it
//! loads a sheet with the engine's `SPR_Load`, picks it with `SPR_Set`, and
//! draws a rectangle of it with `SPR_Draw`, `SPR_DrawHoles` or
//! `SPR_DrawAdditive` (`cl_enginefunc_t` slots 0 and 4..7). `hud.txt` names
//! every icon with its sheet and rectangle, and DoD's HUD code finds icons by
//! those names (`GetSpriteIndex`), so a user can name one the same way:
//!
//! - `dodstudio_hide_asset add sprites/hud_icons.spr`: everything drawn from
//!   that sheet.
//! - `dodstudio_hide_asset add icon_bandage`: that one `hud.txt` entry.
//!
//! ## How
//!
//! Five pointers in `client.dll`'s own `gEngfuncs` copy are redirected here
//! (the pattern `engine::watch_client_commands` uses for `pfnClientCmd`: a
//! struct client.dll owns, not the engine's table). `SPR_Load` records which
//! handle is which path; `SPR_Set` remembers the handle the HUD picked and
//! whether its sheet is hidden; the three draws return early when the sheet
//! is hidden, or when the rectangle they were given is a hidden `hud.txt`
//! entry of that sheet. Several entries can share an origin (`number_0` and
//! `icon_mg_deploy` both start at 0,0 of `hud_icons`), so the whole
//! rectangle is the key, not the corner.
//!
//! Installed on the first `add`, like `particle_hide`. The slot numbers are
//! checked against the binary by `tools/verify_hud_sprite_slots.py`.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code, unused_imports))]

use std::ffi::{CStr, c_char, c_void};
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

use crate::hide_asset::DrawnAs;

/// `cl_enginefunc_t` slots, in declaration order from the SDK's
/// `cdll_int.h` (`engine.rs` holds the same table from
/// `pfnRegisterVariable` on).
pub const ENGFUNCS_SLOT_SPR_LOAD: usize = 0;
pub const ENGFUNCS_SLOT_SPR_SET: usize = 4;
pub const ENGFUNCS_SLOT_SPR_DRAW: usize = 5;
pub const ENGFUNCS_SLOT_SPR_DRAW_HOLES: usize = 6;
pub const ENGFUNCS_SLOT_SPR_DRAW_ADDITIVE: usize = 7;

/// The engine's `wrect_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wrect {
    pub left: i32,
    pub right: i32,
    pub top: i32,
    pub bottom: i32,
}

/// One `hud.txt` line: the sheet it comes from (as `SPR_Load` is given it,
/// `sprites/<sheet>.spr`) and its rectangle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HudIcon {
    pub name: String,
    pub sheet: String,
    pub rect: Wrect,
}

/// Parses `hud.txt`: a count line, then `name resolution sheet x y w h` per
/// icon. Lines that don't fit are skipped, so a stray comment can't abort the
/// game under `panic = "abort"`.
pub fn parse_hud_txt(text: &str) -> Vec<HudIcon> {
    let mut icons = Vec::new();
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.len() < 7 || words[0].starts_with("//") {
            continue;
        }
        let nums: Option<Vec<i32>> = words[3..7].iter().map(|w| w.parse().ok()).collect();
        let Some(nums) = nums else { continue };
        let (x, y, w, h) = (nums[0], nums[1], nums[2], nums[3]);
        icons.push(HudIcon {
            name: words[0].to_string(),
            sheet: format!("sprites/{}.spr", words[2]),
            rect: Wrect {
                left: x,
                right: x + w,
                top: y,
                bottom: y + h,
            },
        });
    }
    icons
}

/// Where the game's `hud.txt` is: the `-addons` folder's copy first (it
/// overrides `dod\`'s in the engine's search path), then the game folder's.
fn hud_txt_paths() -> Vec<std::path::PathBuf> {
    let root = crate::game_folder::game_root();
    vec![
        root.join(crate::game_folder::addon_name())
            .join("sprites")
            .join("hud.txt"),
        crate::game_folder::mod_dir()
            .join("sprites")
            .join("hud.txt"),
    ]
}

/// The icons of the game's `hud.txt`, read afresh each time (a console
/// command, not a per-frame path).
pub fn hud_icons() -> Vec<HudIcon> {
    for path in hud_txt_paths() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            return parse_hud_txt(&text);
        }
    }
    Vec::new()
}

/// The `hud.txt` entry called `name`, if there is one. The first entry wins
/// where a name repeats (the HUD's own lookup takes the first too).
pub fn lookup(name: &str) -> Option<HudIcon> {
    hud_icons()
        .into_iter()
        .find(|i| i.name.eq_ignore_ascii_case(name))
}

/// Whether a typed argument names a `hud.txt` icon rather than a file: no
/// folder separator and no extension.
pub fn looks_like_icon_name(arg: &str) -> bool {
    !arg.is_empty() && !arg.contains(['/', '\\', '.'])
}

/// The sheets `SPR_Set` has picked this session, in first-seen order (capped
/// so a stray loop can't grow it without bound), for `list`.
static DRAWN_SHEETS: RwLock<Vec<String>> = RwLock::new(Vec::new());
const DRAWN_CAP: usize = 64;

/// `list`'s report of what the HUD has drawn so far, with the `hud.txt`
/// names each sheet holds, so nobody has to open the file to find a name.
pub fn drawn_report() -> String {
    let sheets = match DRAWN_SHEETS.read() {
        Ok(s) if !s.is_empty() => s.clone(),
        _ => return String::new(),
    };
    let icons = hud_icons();
    let mut out =
        String::from("HUD sheets drawn so far (hide by sheet path, or by a hud.txt name):\n");
    for sheet in sheets {
        let names: Vec<&str> = icons
            .iter()
            .filter(|i| i.sheet.eq_ignore_ascii_case(&sheet))
            .map(|i| i.name.as_str())
            .collect();
        out.push_str(&format!(
            "  {sheet}: {}\n",
            if names.is_empty() {
                "(no hud.txt names)".to_string()
            } else {
                names.join(" ")
            }
        ));
    }
    out
}

fn note_drawn(sheet: &str) {
    if let Ok(list) = DRAWN_SHEETS.read()
        && (list.iter().any(|s| s == sheet) || list.len() >= DRAWN_CAP)
    {
        return;
    }
    if let Ok(mut list) = DRAWN_SHEETS.write()
        && !list.iter().any(|s| s == sheet)
        && list.len() < DRAWN_CAP
    {
        list.push(sheet.to_string());
    }
}

/// Sprite handle -> path, as `SPR_Load` handed them out. Handles are small
/// integers the engine reuses only across level changes (a `VidInit`), when
/// the HUD loads everything again and the map is rewritten.
static HANDLES: RwLock<Vec<(i32, String)>> = RwLock::new(Vec::new());

/// What `SPR_Set` picked last: the index into [`HANDLES`] (`usize::MAX` for
/// unknown), and whether that whole sheet is hidden. Atomics, since the draws
/// run many times a frame.
static CURRENT: AtomicUsize = AtomicUsize::new(usize::MAX);
static CURRENT_HIDDEN: AtomicBool = AtomicBool::new(false);

type SprLoadFn = unsafe extern "C" fn(*const c_char) -> i32;
type SprSetFn = unsafe extern "C" fn(i32, i32, i32, i32);
type SprDrawFn = unsafe extern "C" fn(i32, i32, i32, *const Wrect);

static REAL_SPR_LOAD: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static REAL_SPR_SET: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static REAL_SPR_DRAW: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static REAL_SPR_DRAW_HOLES: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static REAL_SPR_DRAW_ADDITIVE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
/// The `gEngfuncs` copy the hooks went into (0 = not installed).
static INSTALLED_AT: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" fn hook_spr_load(name: *const c_char) -> i32 {
    let real = REAL_SPR_LOAD.load(Ordering::Acquire);
    if real.is_null() {
        return 0;
    }
    let real: SprLoadFn = unsafe { std::mem::transmute(real) };
    let handle = unsafe { real(name) };
    if handle != 0 && !name.is_null() {
        let path = unsafe { CStr::from_ptr(name) }
            .to_string_lossy()
            .into_owned();
        if let Ok(mut map) = HANDLES.write() {
            match map.iter_mut().find(|(h, _)| *h == handle) {
                Some(entry) => entry.1 = path,
                None => map.push((handle, path)),
            }
        }
    }
    handle
}

unsafe extern "C" fn hook_spr_set(handle: i32, r: i32, g: i32, b: i32) {
    let (index, hidden) = match HANDLES.read() {
        Ok(map) => match map.iter().position(|(h, _)| *h == handle) {
            Some(i) => {
                let path = &map[i].1;
                note_drawn(path);
                (i, crate::hide_asset::should_hide(path, DrawnAs::HudSprite))
            }
            None => (usize::MAX, false),
        },
        Err(_) => (usize::MAX, false),
    };
    CURRENT.store(index, Ordering::Relaxed);
    CURRENT_HIDDEN.store(hidden, Ordering::Relaxed);
    let real = REAL_SPR_SET.load(Ordering::Acquire);
    if !real.is_null() {
        let real: SprSetFn = unsafe { std::mem::transmute(real) };
        unsafe { real(handle, r, g, b) };
    }
}

/// Whether the draw about to happen is of something hidden: the whole sheet,
/// or this rectangle of it as a hidden `hud.txt` entry.
fn skip_draw(prc: *const Wrect) -> bool {
    if CURRENT_HIDDEN.load(Ordering::Relaxed) {
        return true;
    }
    if prc.is_null() || !crate::hide_asset::has_hud_icons() {
        return false;
    }
    let index = CURRENT.load(Ordering::Relaxed);
    let rect = unsafe { *prc };
    match HANDLES.read() {
        Ok(map) => map
            .get(index)
            .is_some_and(|(_, path)| crate::hide_asset::should_hide_hud_icon(path, &rect)),
        Err(_) => false,
    }
}

unsafe extern "C" fn hook_spr_draw(frame: i32, x: i32, y: i32, prc: *const Wrect) {
    if skip_draw(prc) {
        return;
    }
    let real = REAL_SPR_DRAW.load(Ordering::Acquire);
    if !real.is_null() {
        let real: SprDrawFn = unsafe { std::mem::transmute(real) };
        unsafe { real(frame, x, y, prc) };
    }
}

unsafe extern "C" fn hook_spr_draw_holes(frame: i32, x: i32, y: i32, prc: *const Wrect) {
    if skip_draw(prc) {
        return;
    }
    let real = REAL_SPR_DRAW_HOLES.load(Ordering::Acquire);
    if !real.is_null() {
        let real: SprDrawFn = unsafe { std::mem::transmute(real) };
        unsafe { real(frame, x, y, prc) };
    }
}

unsafe extern "C" fn hook_spr_draw_additive(frame: i32, x: i32, y: i32, prc: *const Wrect) {
    if skip_draw(prc) {
        return;
    }
    let real = REAL_SPR_DRAW_ADDITIVE.load(Ordering::Acquire);
    if !real.is_null() {
        let real: SprDrawFn = unsafe { std::mem::transmute(real) };
        unsafe { real(frame, x, y, prc) };
    }
}

/// Redirects the five `SPR_*` pointers in client.dll's `gEngfuncs` copy
/// here. Once per copy; a second call is free.
#[cfg(target_arch = "x86")]
pub fn install() {
    let Some(gengfuncs) = crate::engine::client_gengfuncs() else {
        unsafe {
            crate::debug::report(
                "hud_sprites: client.dll's gEngfuncs copy not found yet; HUD sprites not hidden",
            )
        };
        return;
    };
    if INSTALLED_AT.load(Ordering::Acquire) == gengfuncs as usize {
        return;
    }
    let hooks: [(usize, &AtomicPtr<c_void>, *mut c_void); 5] = [
        (
            ENGFUNCS_SLOT_SPR_LOAD,
            &REAL_SPR_LOAD,
            hook_spr_load as *mut c_void,
        ),
        (
            ENGFUNCS_SLOT_SPR_SET,
            &REAL_SPR_SET,
            hook_spr_set as *mut c_void,
        ),
        (
            ENGFUNCS_SLOT_SPR_DRAW,
            &REAL_SPR_DRAW,
            hook_spr_draw as *mut c_void,
        ),
        (
            ENGFUNCS_SLOT_SPR_DRAW_HOLES,
            &REAL_SPR_DRAW_HOLES,
            hook_spr_draw_holes as *mut c_void,
        ),
        (
            ENGFUNCS_SLOT_SPR_DRAW_ADDITIVE,
            &REAL_SPR_DRAW_ADDITIVE,
            hook_spr_draw_additive as *mut c_void,
        ),
    ];
    let mut done = 0;
    for (slot, real, hook) in hooks {
        match unsafe { crate::engine::patch_gengfuncs_slot(gengfuncs, slot, hook) } {
            Ok(was) => {
                real.store(was, Ordering::Release);
                done += 1;
            }
            Err(why) => unsafe {
                crate::debug::report(&format!("hud_sprites: slot {slot} not hooked -- {why}"))
            },
        }
    }
    if done == hooks.len() {
        INSTALLED_AT.store(gengfuncs as usize, Ordering::Release);
    }
    unsafe {
        crate::debug::report(&format!(
            "hud_sprites: {done} of 5 SPR_* pointers in gEngfuncs at {gengfuncs:p} now go through dodstudio_hide_asset (#808)"
        ))
    };
}

#[cfg(not(target_arch = "x86"))]
pub fn install() {}

#[cfg(test)]
mod tests {
    use super::*;

    const HUD: &str = "4\n\
        number_0\t\t\t640\t\thud_icons \t \t0\t0\t20\t25\t\n\
        icon_bandage\t\t\t640\t\thud_icons  \t\t65      0      \t64      64\n\
        clip_garand_full  \t\t640\t\tclip_bullets   \t\t0   \t183   \t57  \t48\n\
        // a comment\n\
        broken line here\n";

    #[test]
    fn hud_txt_lines_become_sheet_paths_and_rectangles() {
        let icons = parse_hud_txt(HUD);
        assert_eq!(icons.len(), 3);
        assert_eq!(icons[1].name, "icon_bandage");
        assert_eq!(icons[1].sheet, "sprites/hud_icons.spr");
        assert_eq!(
            icons[1].rect,
            Wrect {
                left: 65,
                right: 129,
                top: 0,
                bottom: 64
            }
        );
        assert_eq!(icons[2].sheet, "sprites/clip_bullets.spr");
        assert_eq!(icons[2].rect.bottom, 231);
    }

    #[test]
    fn two_icons_at_one_corner_differ_by_their_size() {
        let icons = parse_hud_txt(
            "2\nnumber_0 640 hud_icons 0 0 20 25\nicon_mg_deploy 640 hud_icons 0 0 64 64\n",
        );
        assert_ne!(icons[0].rect, icons[1].rect);
        assert_eq!(
            (icons[0].rect.left, icons[0].rect.top),
            (icons[1].rect.left, icons[1].rect.top)
        );
    }

    #[test]
    fn an_icon_name_has_no_folder_or_extension() {
        assert!(looks_like_icon_name("icon_bandage"));
        assert!(!looks_like_icon_name("sprites/hud_icons.spr"));
        assert!(!looks_like_icon_name("hud_icons.spr"));
        assert!(!looks_like_icon_name(""));
    }
}
