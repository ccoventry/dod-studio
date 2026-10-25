//! The main menu's background picture (#1045): a whole image of our own, the
//! one whose shape is closest to the screen's, and the stock tiles kept in
//! shape on the pre-Anniversary build.
//!
//! ## How the game draws it
//!
//! `valve\cl_dlls\GameUI.dll`'s `CBasePanel` reads `resource/BackgroundLayout.txt`
//! (through the file system, so `dod_addon` wins under `-addons`) from its
//! `ApplySchemeSettings` (vftable slot 79): a `resolution <w> <h>` line, then
//! one `<file> <mode> <x> <y>` line per picture. Nothing fixes the names or the
//! grid: DoD's 3x4 grid of `800_<row>_<col>_loading.tga` tiles is just what
//! `valve\resource\BackgroundLayout.txt` lists. The 25th Anniversary GameUI
//! reads `resource/HD_BackgroundLayout.txt` instead when its `hd_background`
//! setting is `1` (Half-Life's 3840x1600 grid of 15x7 `21_9_*` tiles).
//!
//! `CBasePanel::PaintBackground` calls one function, `DrawBackgroundImage`, to
//! paint the tiles, scaling each by the screen size over the `resolution`:
//!
//! - **pre-Anniversary** (`GameUI.dll` +0x16e60): mode `scaled` scales x and y
//!   apart, so the 800x600 picture is stretched to any screen (squashed top to
//!   bottom on 16:9); any other mode draws the tile at its own pixel size.
//! - **25th Anniversary** (+0x8010): adds mode `fit`, one scale for both axes,
//!   the larger of the two, centred: the picture covers the screen and the
//!   overflow is cropped (top and bottom on 16:9). Its layout uses `fit`.
//!
//! ## What this does
//!
//! It points that one `call` (in `PaintBackground`, both builds) at
//! [`draw`], which paints, in order of preference:
//!
//! 1. a whole image from `<game>_addon\resource\background\`, any file named
//!    `dodstudio_menu*.png` or `.tga`: the one whose width over height is
//!    closest to the screen's, scaled to cover the screen without changing
//!    its shape (cropped like the Anniversary build crops). So a 4:3 and a
//!    16:9 picture between them suit every screen, and one picture alone is
//!    used everywhere. The hook reads and uploads it itself
//!    (`DrawSetTextureRGBA`), so no tiles, no layout file and no `-addons`
//!    are needed;
//! 2. with `ds_ui_menu_background_keep_shape 1` on the pre-Anniversary build,
//!    the stock tiles fitted the way the Anniversary build fits them;
//! 3. the game's own `DrawBackgroundImage`.
//!
//! `tools/verify_menu_background_offsets.py` checks the call, the panel's
//! fields and the surface slots against both `GameUI.dll`s.

// Everything here but the tests is reached from the game's 32-bit thread.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

use crate::engine::CvarSPartial;
use crate::hw_call_site::{CALL_LEN, call_bytes};
use crate::names::console_name;

/// `ds_ui_menu_background_keep_shape 1` (default): the pre-Anniversary build
/// fits the stock picture to the screen the way the 25th Anniversary build
/// does, cropping instead of stretching. No effect on the Anniversary build,
/// which already does.
pub const KEEP_SHAPE_NAME: &str = console_name!("ui_menu_background_keep_shape");

/// The setting's flag when its cvar could not be registered.
pub static KEEP_SHAPE: AtomicBool = AtomicBool::new(true);
static KEEP_SHAPE_CVAR: AtomicPtr<CvarSPartial> = AtomicPtr::new(std::ptr::null_mut());

/// Called by `commands.rs` once the cvar is registered.
pub fn set_cvar(cvar: *mut CvarSPartial) {
    KEEP_SHAPE_CVAR.store(cvar, Ordering::Release);
}

fn keep_shape() -> bool {
    let cvar = KEEP_SHAPE_CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        KEEP_SHAPE.load(Ordering::Relaxed)
    } else {
        // Safety: the engine owns the cvar for the session.
        unsafe { (*cvar).value != 0.0 }
    }
}

/// For the fallback toggle's bare-name query.
pub fn status() -> String {
    if keep_shape() {
        "the menu background keeps its shape (cropped to fill the screen)".to_string()
    } else {
        "the menu background is drawn as the game draws it (stretched on the pre-Anniversary build)"
            .to_string()
    }
}

/// One `GameUI.dll` build: where `PaintBackground` calls
/// `DrawBackgroundImage`, and the `CBasePanel` fields that function reads.
pub(crate) struct Build {
    pub name: &'static str,
    /// Unique in that build's code, the call's `rel32` pinned, so a match
    /// also proves where the call goes.
    pub pattern: &'static str,
    /// Where the call's `E8` is in `pattern`.
    pub at: usize,
    /// `CUtlVector<bimage_t>`: the elements pointer, and the count.
    pub images: usize,
    pub count: usize,
    /// The layout's `resolution` width and height.
    pub res_x: usize,
    pub res_y: usize,
    /// Whether the build knows mode `fit` (a tile's byte at
    /// [`TILE_FIT`]); the pre-Anniversary one stretches.
    pub fits: bool,
}

/// Both builds: `mov ecx, esi; call DrawBackgroundImage; pop esi`, between
/// the end of the switch case before it (`call [edx+0x24]; pop esi; ...;
/// ret`) and the epilogue after it.
pub(crate) const BUILDS: [Build; 2] = [
    Build {
        name: "pre-Anniversary",
        // GameUI.dll +0x16b56: call +0x16b60 -> +0x16e60.
        pattern: "FF 52 24 5E 83 C4 08 C3 8B CE E8 FB 02 00 00 5E 83 C4 08 C3",
        at: 10,
        images: 0x74,
        count: 0x80,
        res_x: 0x88,
        res_y: 0x8c,
        fits: false,
    },
    Build {
        name: "25th Anniversary",
        // GameUI.dll +0x875f: call +0x8769 -> +0x8010.
        pattern: "FF 52 24 5E 8B E5 5D C3 8B CE E8 A2 F8 FF FF 5E 8B E5 5D C3",
        at: 10,
        images: 0x78,
        count: 0x84,
        res_x: 0x8c,
        res_y: 0x90,
        fits: true,
    },
];

/// `bimage_t`, 24 bytes: texture id, x, y, width, height, then `scaled`
/// and (Anniversary) `fit` flags.
const TILE_SIZE: usize = 0x18;
const TILE_SCALED: usize = 0x14;
#[cfg(test)]
const TILE_FIT: usize = 0x15;

/// `ISurface` (`VGUI_Surface026`) slots, as GameUI's own paint calls them
/// (`tools/verify_menu_background_offsets.py`): `DrawSetColor(r, g, b, a)`,
/// `DrawSetTextureRGBA(id, rgba, wide, tall, hardwareFilter, forceReload)`,
/// `DrawSetTexture(id)`, `DrawTexturedRect(x0, y0, x1, y1)`,
/// `CreateNewTextureID(procedural)`, `GetScreenSize(&wide, &tall)`.
const SURFACE_DRAW_SET_COLOR: usize = 8;
const SURFACE_DRAW_SET_TEXTURE_RGBA: usize = 26;
const SURFACE_DRAW_SET_TEXTURE: usize = 27;
const SURFACE_DRAW_TEXTURED_RECT: usize = 29;
const SURFACE_CREATE_NEW_TEXTURE_ID: usize = 31;
const SURFACE_GET_SCREEN_SIZE: usize = 32;

/// The images' folder under `<game>_addon`, and what their names start with.
const FOLDER: [&str; 2] = ["resource", "background"];
const PREFIX: &str = "dodstudio_menu";
/// Larger sides are refused: GL drivers of the game's era stop at 8192, and
/// a 32-bit game has little room for the decoded copy.
const MAX_SIDE: u32 = 8192;

static INSTALLED: AtomicBool = AtomicBool::new(false);
static DONE: AtomicBool = AtomicBool::new(false);
/// Index into [`BUILDS`] plus one.
static BUILD: AtomicUsize = AtomicUsize::new(0);
/// The stock `DrawBackgroundImage`.
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static SURFACE: AtomicUsize = AtomicUsize::new(0);
/// Set once the stock layout has been described in the log.
static REPORTED_TILES: AtomicBool = AtomicBool::new(false);
static REPORTED_DRAW: AtomicBool = AtomicBool::new(false);

/// One image found in the folder.
struct Candidate {
    path: PathBuf,
    width: u32,
    height: u32,
    /// The surface's texture id once uploaded; 0 before.
    texture: i32,
    /// Set when it could not be decoded, so it is not tried again.
    failed: bool,
}

struct Images {
    scanned: bool,
    list: Vec<Candidate>,
}

struct Shared(UnsafeCell<Images>);
// Safety: only `draw` touches it, and the engine calls that on its own
// thread, one frame at a time.
unsafe impl Sync for Shared {}

static IMAGES: Shared = Shared(UnsafeCell::new(Images {
    scanned: false,
    list: Vec::new(),
}));

/// Width and height from a PNG's or TGA's header, without decoding it.
pub(crate) fn image_size(head: &[u8]) -> Option<(u32, u32)> {
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
    if head.len() >= 24 && head.starts_with(PNG) && &head[12..16] == b"IHDR" {
        let w = u32::from_be_bytes(head[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(head[20..24].try_into().ok()?);
        return (w > 0 && h > 0).then_some((w, h));
    }
    // TGA: truecolor (2) or RLE truecolor (10), no colour map.
    if head.len() >= 18 && head[1] == 0 && matches!(head[2], 2 | 10) {
        let w = u16::from_le_bytes([head[12], head[13]]) as u32;
        let h = u16::from_le_bytes([head[14], head[15]]) as u32;
        return (w > 0 && h > 0).then_some((w, h));
    }
    None
}

/// Whether a file in the folder is one of ours.
pub(crate) fn is_candidate(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with(PREFIX) && (lower.ends_with(".png") || lower.ends_with(".tga"))
}

/// The image whose shape is closest to the screen's: the smallest
/// difference of log aspect ratios, so 4:3 vs 16:9 is judged by how much
/// would be cropped, not by a raw difference. The first wins a tie.
pub(crate) fn pick(sizes: &[(u32, u32)], screen: (i32, i32)) -> Option<usize> {
    if screen.0 <= 0 || screen.1 <= 0 {
        return None;
    }
    let want = (screen.0 as f64 / screen.1 as f64).ln();
    let mut best: Option<(usize, f64)> = None;
    for (i, &(w, h)) in sizes.iter().enumerate() {
        if w == 0 || h == 0 {
            continue;
        }
        let off = ((w as f64 / h as f64).ln() - want).abs();
        if best.is_none_or(|(_, b)| off < b - 1e-9) {
            best = Some((i, off));
        }
    }
    best.map(|(i, _)| i)
}

/// Where a `w`x`h` picture goes to cover the screen without changing shape:
/// one scale, the larger of the two, centred. Edges past the screen are
/// cropped by the screen itself.
pub(crate) fn cover(w: u32, h: u32, screen: (i32, i32)) -> (i32, i32, i32, i32) {
    let (sw, sh) = (screen.0 as f64, screen.1 as f64);
    let scale = (sw / w as f64).max(sh / h as f64);
    let (dw, dh) = (w as f64 * scale, h as f64 * scale);
    let (x0, y0) = ((sw - dw) / 2.0, (sh - dh) / 2.0);
    (
        x0.round() as i32,
        y0.round() as i32,
        (x0 + dw).round() as i32,
        (y0 + dh).round() as i32,
    )
}

/// One stock tile placed the way [`cover`] places the whole `res` picture.
/// Each edge is rounded from its own layout position, so tiles that touch
/// in the layout touch on screen.
pub(crate) fn fit_tile(
    res: (i32, i32),
    screen: (i32, i32),
    tile: (i32, i32, i32, i32),
) -> (i32, i32, i32, i32) {
    let (rw, rh) = (res.0.max(1) as f64, res.1.max(1) as f64);
    let (sw, sh) = (screen.0 as f64, screen.1 as f64);
    let scale = (sw / rw).max(sh / rh);
    let (ox, oy) = ((sw - rw * scale) / 2.0, (sh - rh * scale) / 2.0);
    let (x, y, w, h) = tile;
    let at = |o: f64, v: i32| (o + v as f64 * scale).round() as i32;
    (at(ox, x), at(oy, y), at(ox, x + w), at(oy, y + h))
}

/// A layout tile on screen: a `scaled` one through [`fit_tile`]; any other
/// at its own pixel size from its fitted position, as the game places one.
pub(crate) fn place_tile(
    res: (i32, i32),
    screen: (i32, i32),
    tile: (i32, i32, i32, i32),
    scaled: bool,
) -> (i32, i32, i32, i32) {
    let fitted = fit_tile(res, screen, tile);
    if scaled {
        fitted
    } else {
        (fitted.0, fitted.1, fitted.0 + tile.2, fitted.1 + tile.3)
    }
}

fn folder() -> PathBuf {
    crate::game_folder::game_root()
        .join(crate::game_folder::addon_name())
        .join(FOLDER[0])
        .join(FOLDER[1])
}

/// The candidates in `dir`, by name so the order is the same every run.
fn scan_folder(dir: &Path) -> Vec<Candidate> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(is_candidate)
        })
        .collect();
    names.sort();
    let mut out = Vec::new();
    for path in names {
        let mut head = [0u8; 32];
        let size = std::fs::File::open(&path).ok().and_then(|mut f| {
            use std::io::Read;
            let n = f.read(&mut head).ok()?;
            image_size(&head[..n])
        });
        let message = match size {
            Some((w, h)) if w <= MAX_SIDE && h <= MAX_SIDE => {
                out.push(Candidate {
                    path: path.clone(),
                    width: w,
                    height: h,
                    texture: 0,
                    failed: false,
                });
                format!("menu_background: found {} ({w}x{h})", path.display())
            }
            Some((w, h)) => format!(
                "menu_background: {} is {w}x{h}, over the {MAX_SIDE} px limit -- skipped",
                path.display()
            ),
            None => format!(
                "menu_background: {} is not a PNG or truecolor TGA -- skipped",
                path.display()
            ),
        };
        unsafe { crate::debug::report(&message) };
    }
    out
}

fn decode(path: &Path) -> Result<crate::texture_hires::Rgba, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("could not read it: {e}"))?;
    let lower = path.to_string_lossy().to_ascii_lowercase();
    if lower.ends_with(".png") {
        crate::spray::decode_png(&bytes)
    } else {
        crate::texture_hires::decode_tga(&bytes)
    }
}

/// Reads `*mut c_void`'s vftable slot `index`.
///
/// Safety: `object` must be a live object with at least `index + 1` slots.
unsafe fn slot<T: Copy>(object: *mut c_void, index: usize) -> T {
    unsafe {
        let vftable = *(object as *const *const usize);
        std::mem::transmute_copy(&*vftable.add(index))
    }
}

type ColorFn = unsafe extern "thiscall" fn(*mut c_void, i32, i32, i32, i32);
type TextureFn = unsafe extern "thiscall" fn(*mut c_void, i32);
type RectFn = unsafe extern "thiscall" fn(*mut c_void, i32, i32, i32, i32);
type NewTextureFn = unsafe extern "thiscall" fn(*mut c_void, u32) -> i32;
type UploadFn = unsafe extern "thiscall" fn(*mut c_void, i32, *const u8, i32, i32, i32, i32);
type ScreenSizeFn = unsafe extern "thiscall" fn(*mut c_void, *mut i32, *mut i32);
type DrawFn = unsafe extern "thiscall" fn(*mut c_void);

/// Paints `texture` over `rect`, untinted.
unsafe fn paint(surface: *mut c_void, texture: i32, rect: (i32, i32, i32, i32)) {
    unsafe {
        let set_color: ColorFn = slot(surface, SURFACE_DRAW_SET_COLOR);
        set_color(surface, 255, 255, 255, 255);
        let set_texture: TextureFn = slot(surface, SURFACE_DRAW_SET_TEXTURE);
        set_texture(surface, texture);
        let draw_rect: RectFn = slot(surface, SURFACE_DRAW_TEXTURED_RECT);
        draw_rect(surface, rect.0, rect.1, rect.2, rect.3);
    }
}

/// The whole image for this screen, uploaded the first time it is picked.
/// `None` when the folder holds none that loads.
unsafe fn whole_image(surface: *mut c_void, screen: (i32, i32)) -> Option<(i32, u32, u32)> {
    // Safety: the engine thread only (see `Shared`).
    let images = unsafe { &mut *IMAGES.0.get() };
    if !images.scanned {
        images.scanned = true;
        images.list = scan_folder(&folder());
    }
    loop {
        let sizes: Vec<(u32, u32)> = images
            .list
            .iter()
            .map(|c| {
                if c.failed {
                    (0, 0)
                } else {
                    (c.width, c.height)
                }
            })
            .collect();
        let chosen = &mut images.list[pick(&sizes, screen)?];
        if chosen.texture != 0 {
            return Some((chosen.texture, chosen.width, chosen.height));
        }
        match decode(&chosen.path) {
            Ok(rgba) if rgba.width == chosen.width && rgba.height == chosen.height => unsafe {
                let new_texture: NewTextureFn = slot(surface, SURFACE_CREATE_NEW_TEXTURE_ID);
                let id = new_texture(surface, 0);
                let upload: UploadFn = slot(surface, SURFACE_DRAW_SET_TEXTURE_RGBA);
                // Filtered: the picture is scaled to the screen.
                upload(
                    surface,
                    id,
                    rgba.pixels.as_ptr(),
                    rgba.width as i32,
                    rgba.height as i32,
                    1,
                    1,
                );
                chosen.texture = id;
                crate::debug::report(&format!(
                    "menu_background: drawing {} ({}x{}) for a {}x{} screen",
                    chosen.path.display(),
                    chosen.width,
                    chosen.height,
                    screen.0,
                    screen.1
                ));
                return Some((id, chosen.width, chosen.height));
            },
            Ok(_) => {
                chosen.failed = true;
                unsafe {
                    crate::debug::report(&format!(
                        "menu_background: {} decoded to another size than its header says -- skipped",
                        chosen.path.display()
                    ))
                };
            }
            Err(why) => {
                chosen.failed = true;
                unsafe {
                    crate::debug::report(&format!(
                        "menu_background: could not load {}: {why}",
                        chosen.path.display()
                    ))
                };
            }
        }
    }
}

/// The stock tiles, fitted. `false` when there is no layout to draw.
unsafe fn fitted_tiles(
    panel: *mut c_void,
    build: &Build,
    surface: *mut c_void,
    screen: (i32, i32),
) -> bool {
    let base = panel as usize;
    // Safety: `panel` is the `CBasePanel` the game passed, on a checked
    // build, so these are its fields.
    unsafe {
        let tiles = *((base + build.images) as *const usize);
        let count = *((base + build.count) as *const i32);
        let res = (
            *((base + build.res_x) as *const i32),
            *((base + build.res_y) as *const i32),
        );
        if tiles == 0 || count <= 0 || res.0 <= 0 || res.1 <= 0 {
            return false;
        }
        let report = !REPORTED_TILES.swap(true, Ordering::Relaxed);
        let mut unscaled = Vec::new();
        for i in 0..count as usize {
            let t = (tiles + i * TILE_SIZE) as *const i32;
            let tile = (*t.add(1), *t.add(2), *t.add(3), *t.add(4));
            let scaled = *((tiles + i * TILE_SIZE + TILE_SCALED) as *const u8) != 0;
            if report && !scaled {
                unscaled.push(tile);
            }
            paint(surface, *t, place_tile(res, screen, tile, scaled));
        }
        if report {
            crate::debug::report(&format!(
                "menu_background: fitting the stock layout ({}x{}, {count} tile(s), unscaled (x, y, w, h): {unscaled:?}) to a {}x{} screen",
                res.0, res.1, screen.0, screen.1
            ));
        }
    }
    true
}

/// Stands in for `DrawBackgroundImage(this)`.
unsafe extern "thiscall" fn draw(panel: *mut c_void) {
    let original = ORIGINAL.load(Ordering::Acquire);
    let surface = SURFACE.load(Ordering::Acquire) as *mut c_void;
    let build = BUILD
        .load(Ordering::Acquire)
        .checked_sub(1)
        .and_then(|i| BUILDS.get(i));
    if let (false, Some(build)) = (surface.is_null(), build) {
        let mut screen = (0i32, 0i32);
        // Safety: the engine's surface, on the engine thread.
        unsafe {
            let size: ScreenSizeFn = slot(surface, SURFACE_GET_SCREEN_SIZE);
            size(surface, &mut screen.0, &mut screen.1);
            if !REPORTED_DRAW.swap(true, Ordering::Relaxed) {
                crate::debug::report(&format!(
                    "menu_background: first draw, {}x{} screen, keep shape {}",
                    screen.0,
                    screen.1,
                    keep_shape()
                ));
            }
            if let Some((texture, w, h)) = whole_image(surface, screen) {
                paint(surface, texture, cover(w, h, screen));
                return;
            }
            if !build.fits && keep_shape() && fitted_tiles(panel, build, surface, screen) {
                return;
            }
        }
    }
    if original != 0 {
        // Safety: the function the stock call went to.
        unsafe { std::mem::transmute::<usize, DrawFn>(original)(panel) };
    }
}

#[cfg(target_arch = "x86")]
fn install() -> Result<String, String> {
    use std::ffi::CStr;
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    let module = |name: &CStr| {
        let handle = unsafe { GetModuleHandleA(name.as_ptr() as *const u8) };
        (!handle.is_null()).then_some(handle as usize)
    };
    let gameui = module(c"GameUI.dll").ok_or("GameUI.dll is not loaded")?;
    let hw = module(c"hw.dll").ok_or("hw.dll is not loaded")?;
    type CreateInterfaceFn = unsafe extern "C" fn(*const std::ffi::c_char, *mut i32) -> *mut c_void;
    let create = unsafe { GetProcAddress(hw as _, c"CreateInterface".as_ptr() as *const u8) }
        .ok_or("hw.dll has no CreateInterface")?;
    // Safety: the interface factory's signature.
    let create: CreateInterfaceFn = unsafe { std::mem::transmute(create) };
    let surface = unsafe { create(c"VGUI_Surface026".as_ptr(), std::ptr::null_mut()) };
    if surface.is_null() {
        return Err("hw.dll has no VGUI_Surface026".into());
    }
    let mut misses = Vec::new();
    for (index, build) in BUILDS.iter().enumerate() {
        // Safety: a module handle the loader gave us; GameUI never unloads.
        let found = match unsafe { crate::scan::find_unique(gameui, build.pattern) } {
            Ok(found) => found,
            Err(why) => {
                misses.push(format!("{}: {why}", build.name));
                continue;
            }
        };
        let call = found + build.at;
        let mut stock = [0u8; CALL_LEN];
        // Safety: inside the matched pattern.
        stock.copy_from_slice(unsafe { std::slice::from_raw_parts(call as *const u8, CALL_LEN) });
        let original = crate::hw_call_site::call_target(call, &stock);
        ORIGINAL.store(original, Ordering::Release);
        SURFACE.store(surface as usize, Ordering::Release);
        BUILD.store(index + 1, Ordering::Release);
        let ours = call_bytes(call, draw as *const () as usize);
        // Safety: one whole `call rel32`, written from the engine's thread
        // between frames (`commands::poll`).
        if !unsafe { crate::patch::write_code_bytes(call, &ours) } {
            return Err("could not make PaintBackground's call writable".into());
        }
        return Ok(format!(
            "menu_background: {} GameUI, PaintBackground +{:#x} now calls ours (stock +{:#x}); images from {}",
            build.name,
            call - gameui,
            original - gameui,
            folder().display()
        ));
    }
    Err(format!(
        "PaintBackground's call not found -- {}",
        misses.join("; ")
    ))
}

#[cfg(not(target_arch = "x86"))]
fn install() -> Result<String, String> {
    Err("only the 32-bit game has a GameUI.dll".into())
}

/// Installs once `GameUI.dll` is loaded. Runs every frame from
/// `commands::poll`; one atomic load once the attempt is over.
pub fn poll() {
    if DONE.load(Ordering::Relaxed) {
        return;
    }
    #[cfg(target_arch = "x86")]
    {
        use windows_sys::Win32::System::LibraryLoader::GetModuleHandleA;
        // Safety: a plain module lookup.
        if unsafe { GetModuleHandleA(c"GameUI.dll".as_ptr() as *const u8) }.is_null() {
            return; // not loaded yet: try again next frame
        }
    }
    DONE.store(true, Ordering::Relaxed);
    let message = match install() {
        Ok(message) => {
            INSTALLED.store(true, Ordering::Relaxed);
            message
        }
        Err(why) => format!("menu_background: not installed -- {why}"),
    };
    unsafe { crate::debug::report(&message) };
}

/// The `ds_debug_status` line, or `None` when it isn't installed.
pub fn status_line() -> Option<String> {
    INSTALLED.load(Ordering::Relaxed).then(|| {
        let images = unsafe { &*IMAGES.0.get() };
        let drawn = images.list.iter().filter(|c| c.texture != 0).count();
        format!(
            "menu background: {} image(s) found, {drawn} loaded; keep shape {}",
            images.list.len(),
            if keep_shape() { "on" } else { "off" }
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_builds_pin_the_call_and_differ() {
        assert_ne!(BUILDS[0].pattern, BUILDS[1].pattern);
        for build in &BUILDS {
            assert!(
                crate::scan::Pattern::parse(build.pattern).is_ok(),
                "{}",
                build.name
            );
            let tokens: Vec<&str> = build.pattern.split_whitespace().collect();
            assert_eq!(tokens[build.at], "E8", "{}: a call rel32", build.name);
            assert!(
                tokens[build.at + 1..build.at + CALL_LEN]
                    .iter()
                    .all(|t| *t != "??"),
                "{}: the call's rel32 is pinned",
                build.name
            );
            // `mov ecx, esi` hands it `this`; `pop esi` follows.
            assert_eq!(
                &tokens[build.at - 2..build.at],
                ["8B", "CE"],
                "{}",
                build.name
            );
            assert_eq!(tokens[build.at + CALL_LEN], "5E", "{}", build.name);
            assert_eq!(
                build.count,
                build.images + 0xc,
                "CUtlVector's count follows its memory"
            );
            assert_eq!(build.res_y, build.res_x + 4);
        }
        assert!(!BUILDS[0].fits && BUILDS[1].fits);
        assert_eq!(TILE_FIT, TILE_SCALED + 1);
    }

    #[test]
    fn headers_give_the_size() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend_from_slice(&3840u32.to_be_bytes());
        png.extend_from_slice(&2160u32.to_be_bytes());
        assert_eq!(image_size(&png), Some((3840, 2160)));
        let mut tga = vec![0u8; 18];
        tga[2] = 2;
        tga[12..14].copy_from_slice(&1600u16.to_le_bytes());
        tga[14..16].copy_from_slice(&1200u16.to_le_bytes());
        tga[16] = 24;
        assert_eq!(image_size(&tga), Some((1600, 1200)));
        tga[2] = 1; // colour-mapped: not ours
        assert_eq!(image_size(&tga), None);
        assert_eq!(image_size(b"short"), None);
    }

    #[test]
    fn only_our_pngs_and_tgas_count() {
        assert!(is_candidate("dodstudio_menu_16x9.png"));
        assert!(is_candidate("DodStudio_Menu.TGA"));
        assert!(!is_candidate("800_1_a_loading.tga"));
        assert!(!is_candidate("dodstudio_menu_16x9.jpg"));
    }

    #[test]
    fn the_closest_shape_is_picked() {
        let both = [(1600, 1200), (3840, 2160)];
        assert_eq!(pick(&both, (1024, 768)), Some(0), "4:3");
        assert_eq!(pick(&both, (1280, 1024)), Some(0), "5:4");
        assert_eq!(pick(&both, (2560, 1440)), Some(1), "16:9");
        assert_eq!(pick(&both, (1920, 1200)), Some(1), "16:10 is nearer 16:9");
        assert_eq!(pick(&both, (3440, 1440)), Some(1), "21:9");
        assert_eq!(
            pick(&[(3840, 2160)], (1024, 768)),
            Some(0),
            "one image suits all"
        );
        assert_eq!(
            pick(&[(0, 0), (1600, 1200)], (2560, 1440)),
            Some(1),
            "failed skipped"
        );
        assert_eq!(pick(&[], (2560, 1440)), None);
        assert_eq!(pick(&both, (0, 0)), None);
    }

    #[test]
    fn cover_keeps_the_shape_and_fills_the_screen() {
        // Same shape: exactly the screen.
        assert_eq!(cover(3840, 2160, (2560, 1440)), (0, 0, 2560, 1440));
        // 4:3 on 16:9: full width, top and bottom cropped equally.
        assert_eq!(cover(800, 600, (2560, 1440)), (0, -240, 2560, 1680));
        // 16:9 on 4:3: full height, the sides cropped.
        assert_eq!(cover(1920, 1080, (1024, 768)), (-171, 0, 1195, 768));
    }

    #[test]
    fn fitted_tiles_touch_and_match_cover() {
        let (res, screen) = ((800, 600), (2560, 1440));
        let left = fit_tile(res, screen, (0, 0, 256, 256));
        let right = fit_tile(res, screen, (256, 0, 256, 256));
        let below = fit_tile(res, screen, (0, 256, 256, 256));
        assert_eq!(left.2, right.0);
        assert_eq!(left.3, below.1);
        let last = fit_tile(res, screen, (768, 512, 32, 88));
        assert_eq!((left.0, left.1, last.2, last.3), cover(800, 600, screen));
        // An unscaled tile keeps its pixel size from its fitted corner.
        assert_eq!(
            place_tile(res, screen, (256, 0, 256, 256), false),
            (right.0, right.1, right.0 + 256, right.1 + 256)
        );
        assert_eq!(place_tile(res, screen, (256, 0, 256, 256), true), right);
    }
}
