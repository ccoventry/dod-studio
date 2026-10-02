//! High-quality overview maps (#371): when `overviews/<map>_hd.tga` sits
//! beside the overview the game loads, its tiles replace the game's.
//!
//! ## Why the game alone can't
//!
//! The engine's overview loader (`hw.dll` PRE `+0x45f50`, Anniversary
//! `+0x243870`) reads the image into a fixed 3 MB RGBA buffer -- 1024x768 at
//! most -- and cuts it into 128x128 tiles, one sprite frame each, uploaded as
//! textures named `<image>_<n>`. A bigger image is refused ("Wrong map image
//! dimensions").
//!
//! ## What this does
//!
//! The client asks for the overview through `gEngfuncs.LoadMapSprite`
//! (`cl_enginefunc_t` slot 89; `CHudSpectator::LoadMapSprites`,
//! `client+0x39064`). [`wrap`] puts [`load_map_sprite`] in that slot of the
//! engine's table before the client copies it in `Initialize`. After the
//! engine has built the sprite, each frame's texture is uploaded again from
//! the matching tile of the `_hd` image, which is any multiple of the game's
//! tile grid: 4096x3072 gives 512x512 tiles.
//!
//! The sprite layout is the same in both builds (checked in both loaders):
//! `model_t+0x184` is the sprite, `+0xc` its frame count, frames from `+0x20`
//! eight bytes apart, and each frame's texture number at `+0x18`.
//!
//! The engine keeps its own idea of the bound texture, so the binding is put
//! back after the uploads. Pure green is made transparent, as the game does.
//!
//! `dod_addon/overviews` is looked in first when the game runs with
//! `-addons`, as the game itself does.

use std::ffi::{CStr, c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

/// `cl_enginefunc_t.LoadMapSprite`.
const LOAD_MAP_SPRITE_SLOT: usize = 89;

const MODEL_SPRITE: usize = 0x184;
const SPRITE_FRAMES: usize = 0xc;
const SPRITE_FRAME_LIST: usize = 0x20;
const FRAME_TEXTURE: usize = 0x18;

const GL_TEXTURE_2D: u32 = 0x0de1;
const GL_TEXTURE_BINDING_2D: u32 = 0x8069;
const GL_RGBA: u32 = 0x1908;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
const GL_TEXTURE_MIN_FILTER: u32 = 0x2801;
const GL_TEXTURE_MAG_FILTER: u32 = 0x2800;
const GL_LINEAR: i32 = 0x2601;

type LoadMapSpriteFn = unsafe extern "C" fn(*const c_char) -> *mut c_void;
type BindFn = unsafe extern "system" fn(u32, u32);
type TexImageFn = unsafe extern "system" fn(u32, i32, i32, i32, i32, i32, u32, u32, *const c_void);
type TexParameterFn = unsafe extern "system" fn(u32, u32, i32);
type GetIntegerFn = unsafe extern "system" fn(u32, *mut i32);

static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
/// What the last overview load did, for `dodstudio_debug_status`.
static LAST: Mutex<Option<String>> = Mutex::new(None);

/// Puts [`load_map_sprite`] into the engine's function table. Called from
/// the `Initialize` trampoline before the real one copies the table.
///
/// # Safety
///
/// `engfuncs` is the table the engine passes `Initialize`, at least 90 slots.
pub unsafe fn wrap(engfuncs: *mut c_void) {
    if engfuncs.is_null() {
        return;
    }
    let slot = unsafe { (engfuncs as *mut usize).add(LOAD_MAP_SPRITE_SLOT) };
    let current = unsafe { slot.read() };
    let ours = load_map_sprite as LoadMapSpriteFn as usize;
    if current == 0 || current == ours {
        return;
    }
    ORIGINAL.store(current, Ordering::Release);
    unsafe { slot.write(ours) };
    unsafe { crate::debug::report("overview_hd: LoadMapSprite wrapped") };
}

unsafe extern "C" fn load_map_sprite(name: *const c_char) -> *mut c_void {
    let original = ORIGINAL.load(Ordering::Acquire);
    if original == 0 {
        return std::ptr::null_mut();
    }
    // Safety: the engine's own function, from its own table.
    let original: LoadMapSpriteFn = unsafe { std::mem::transmute(original) };
    let model = unsafe { original(name) };
    if model.is_null() || name.is_null() {
        return model;
    }
    let name = unsafe { CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned();
    let outcome = hd_file(&name).map(|path| match unsafe { upgrade(model, &path) } {
        Ok(tile) => format!("{name}: tiles from {} ({tile}x{tile})", path.display()),
        Err(why) => format!("{name}: {} not used -- {why}", path.display()),
    });
    match &outcome {
        Some(line) => unsafe { crate::debug::report(&format!("overview_hd: {line}")) },
        None => unsafe {
            crate::debug::report(&format!(
                "overview_hd: {name} loaded, no _hd image beside it"
            ))
        },
    }
    if let Ok(mut last) = LAST.lock() {
        *last = outcome;
    }
    model
}

fn game_root() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_default()
}

fn addons_on() -> bool {
    std::env::args().any(|a| a.eq_ignore_ascii_case("-addons"))
}

/// `overviews/x.tga` -> `overviews/x_hd.tga`, in `dod_addon` (with
/// `-addons`) or `dod`, whichever has it.
fn hd_file(image: &str) -> Option<PathBuf> {
    let image = image.replace('\\', "/");
    let stem = image.rsplit_once('.').map(|(s, _)| s).unwrap_or(&image);
    if stem.contains("..") {
        return None;
    }
    let relative = format!("{stem}_hd.tga");
    let root = game_root();
    let mut folders = Vec::new();
    if addons_on() {
        folders.push(root.join("dod_addon"));
    }
    folders.push(root.join("dod"));
    folders
        .into_iter()
        .map(|f| f.join(&relative))
        .find(|p| p.is_file())
}

/// The tile of `hd` for frame `index` of a `columns` x `rows` grid, with pure
/// green made transparent.
fn tile(pixels: &[u8], width: usize, tile: usize, columns: usize, index: usize) -> Vec<u8> {
    let (cx, cy) = (index % columns, index / columns);
    let mut out = Vec::with_capacity(tile * tile * 4);
    for y in 0..tile {
        let start = ((cy * tile + y) * width + cx * tile) * 4;
        for px in pixels[start..start + tile * 4].as_chunks::<4>().0 {
            if px[0] == 0 && px[1] == 255 && px[2] == 0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                out.extend_from_slice(px);
            }
        }
    }
    out
}

/// The tile grid the client draws for `frames` frames: 4:3, `frames / 12`
/// a square (`CHudSpectator::DrawOverviewLayer`).
fn grid(frames: usize) -> Option<(usize, usize)> {
    let k = ((frames / 12) as f64).sqrt().round() as usize;
    (k > 0 && 12 * k * k == frames).then_some((4 * k, 3 * k))
}

/// # Safety
///
/// `model` is the `model_t` the engine's overview loader just returned.
unsafe fn upgrade(model: *mut c_void, path: &Path) -> Result<usize, String> {
    let sprite = unsafe { *((model as *const u8).add(MODEL_SPRITE) as *const usize) };
    if sprite == 0 {
        return Err("the overview has no sprite".into());
    }
    let frames = unsafe { *((sprite + SPRITE_FRAMES) as *const i32) }.max(0) as usize;
    let (columns, rows) = grid(frames).ok_or(format!("{frames} tiles is not a 4:3 grid"))?;
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let image = crate::texture_hires::decode_tga(&bytes)?;
    let (w, h) = (image.width as usize, image.height as usize);
    if w % columns != 0 || h % rows != 0 || w / columns != h / rows {
        return Err(format!(
            "{w}x{h} doesn't divide into {columns}x{rows} square tiles"
        ));
    }
    let size = w / columns;
    if !(128..=2048).contains(&size) {
        return Err(format!("{size}-pixel tiles; 128 to 2048 are used"));
    }

    let gl = unsafe { GetModuleHandleA(c"opengl32.dll".as_ptr() as *const u8) };
    if gl.is_null() {
        return Err("opengl32.dll is not loaded".into());
    }
    let proc = |name: &CStr| unsafe { GetProcAddress(gl, name.as_ptr() as *const u8) };
    let (Some(bind), Some(tex_image), Some(tex_param), Some(get_int)) = (
        proc(c"glBindTexture"),
        proc(c"glTexImage2D"),
        proc(c"glTexParameteri"),
        proc(c"glGetIntegerv"),
    ) else {
        return Err("OpenGL functions not found".into());
    };
    let (bind, tex_image, tex_param, get_int): (BindFn, TexImageFn, TexParameterFn, GetIntegerFn) = unsafe {
        (
            std::mem::transmute::<unsafe extern "system" fn() -> isize, BindFn>(bind),
            std::mem::transmute::<unsafe extern "system" fn() -> isize, TexImageFn>(tex_image),
            std::mem::transmute::<unsafe extern "system" fn() -> isize, TexParameterFn>(tex_param),
            std::mem::transmute::<unsafe extern "system" fn() -> isize, GetIntegerFn>(get_int),
        )
    };

    let mut bound = 0i32;
    unsafe { get_int(GL_TEXTURE_BINDING_2D, &mut bound) };
    for index in 0..frames {
        let frame = unsafe { *((sprite + SPRITE_FRAME_LIST + index * 8) as *const usize) };
        if frame == 0 {
            continue;
        }
        let texture = unsafe { *((frame + FRAME_TEXTURE) as *const u32) };
        let pixels = tile(&image.pixels, w, size, columns, index);
        unsafe {
            bind(GL_TEXTURE_2D, texture);
            tex_image(
                GL_TEXTURE_2D,
                0,
                GL_RGBA as i32,
                size as i32,
                size as i32,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                pixels.as_ptr() as *const c_void,
            );
            tex_param(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
            tex_param(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
        }
    }
    unsafe { bind(GL_TEXTURE_2D, bound as u32) };
    Ok(size)
}

/// One line for `dodstudio_debug_status`, once an overview with an `_hd`
/// image has loaded.
pub fn status_line() -> Option<String> {
    LAST.lock()
        .ok()
        .and_then(|l| l.clone())
        .map(|l| format!("HD overview: {l}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_is_the_clients() {
        assert_eq!(grid(48), Some((8, 6)), "1024x768");
        assert_eq!(grid(12), Some((4, 3)));
        assert_eq!(grid(192), Some((16, 12)));
        assert_eq!(grid(50), None);
    }

    #[test]
    fn a_tile_is_cut_row_major_and_green_goes_clear() {
        // 4x2 image of 2x2 tiles... as 2 columns, 1 row of 2-pixel tiles.
        let mut px = Vec::new();
        for y in 0..2u8 {
            for x in 0..4u8 {
                if x == 3 && y == 1 {
                    px.extend_from_slice(&[0, 255, 0, 255]);
                } else {
                    px.extend_from_slice(&[x, y, 7, 255]);
                }
            }
        }
        let second = tile(&px, 4, 2, 2, 1);
        assert_eq!(&second[0..4], &[2, 0, 7, 255]);
        assert_eq!(
            &second[12..16],
            &[0, 0, 0, 0],
            "the key colour is transparent"
        );
    }

    #[test]
    fn the_hd_name_sits_beside_the_image() {
        assert!(hd_file("overviews/../../x.tga").is_none());
    }
}
