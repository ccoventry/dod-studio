//! Higher-resolution replacements for map (world) textures, swapped in at the
//! moment the engine uploads them to the GPU -- no BSP or wad file is touched.
//!
//! ## Why this works without a BSP edit
//!
//! Texture-upscale R&D (memory `texture-upscale-rnd.md`, Track D) found that a
//! same-resolution AI upscale of a map's `.wad` (Track A) has a ceiling: a
//! `miptex_t` carries both the texture's dimensions and its pixels, and the
//! BSP's texinfo vectors were computed against those dimensions. Cross-checked
//! against Xash3D's renderer (`gl_rsurf.c`, `GL_BuildPolygonFromSurface`): the
//! UV divide reads `texinfo->texture->width` live from the model's own
//! `texture_t`, which `Mod_LoadTextures` fills from the miptex header *before*
//! and separately from the upload. So uploading different pixels -- at any
//! size -- tiles exactly like the original, because OpenGL samples whatever is
//! bound across UV 0..1.
//!
//! It also reaches textures a wad swap cannot: most competitive custom maps
//! (7 of the 8 in the wsod25 demo set) embed every texture in the BSP itself,
//! and every one of those still goes through the same upload call.
//!
//! ## Where it hooks
//!
//! `GL_LoadTexture2(identifier, textureType, width, height, data, mipmap,
//! iType, pPal, filter)`, right at its final branch:
//!
//! ```text
//! if (textureType == GLT_SPRITE && iType == TEX_TYPE_RGBA)
//!     GL_Upload32(data, width, height, mipmap, iType, filter);          // already RGBA
//! else
//!     GL_Upload8(data, width, height, mipmap, iType, pPal, filter);     // palette -> RGBA
//! ```
//!
//! For a world texture (`GLT_WORLD`) with a replacement on disk, the stub calls
//! `GL_Upload32` directly with the replacement's pixels and dimensions instead.
//! Everything before that point -- the name-keyed `gltexture_t` cache entry,
//! which keeps the *original* dimensions so a later load of the same texture
//! still hits it -- has already happened, unmodified. Everything after it
//! (power-of-two rounding, `gl_max_size`, mipmaps, the GL calls) is the
//! engine's own `GL_Upload32`, also unmodified.
//!
//! An earlier version of this module hooked `Draw_MiptexTexture` instead. That
//! turned out to be the *decal* loader (`GLT_DECAL`, `decals.wad`) -- the live
//! test's `"{shot5" 16x16` was a bullet hole -- not the map-texture path.
//!
//! ## What `GL_Upload8` would have done that the replacement must copy
//!
//! - **Texture gamma.** `GL_Upload8` runs the palette through a 256-entry gamma
//!   table (`texgamma`) in place before expanding it. The replacement's RGB goes
//!   through the same table, read from the running engine, so brightness
//!   matches the original.
//! - **`gl_dither`.** For opaque textures (`iType` 0) with `gl_dither` on, each
//!   channel becomes `c | c >> 6`. Copied.
//! - **Transparency.** For masked textures (`{` names, `iType` 1), palette index
//!   255 becomes RGBA `0,0,0,0`. The replacement's alpha is thresholded to
//!   exactly that.
//!
//! ## The size ceiling, and raising it
//!
//! `GL_Upload32` resizes and builds mipmaps in one fixed 2 MB static buffer,
//! and refuses (`Sys_Error`, fatal) anything whose power-of-two-rounded size is
//! over 512x1024 pixels. Its two resample helpers also index 1024-entry stack
//! arrays by output width. [`install`] therefore:
//!
//! - repoints `GL_Upload32`'s five uses of that buffer at a 4 MB one of ours,
//! - replaces the size check with one that allows up to 1024x1024 **and**
//!   refuses any output wider than 1024 -- a case the stock check lets through
//!   (2048x256 fits under its pixel budget) and then overruns the stack with.
//!
//! Replacements are also capped at 1024 per side by this module itself (512 if
//! the ceiling could not be raised), so the engine's check is never the thing
//! that stops them. `gl_max_size` (default 256) still clamps each side below
//! that; set it to 512 or 1024 to see the extra resolution.
//!
//! Detail textures (`gfx/detail/*.tga`, drawn over walls when
//! `r_detailtextures` is on) have a separate, smaller limit: their loader reads
//! each TGA into a 1 MB buffer, so anything over 512x512 fails to load. With
//! the ceiling raised, [`raise_detail_limit`] makes that 4 MB (1024x1024).
//!
//! ## Replacement files
//!
//! `<game>\dod\dodstudio_hd_textures\<name>_<hash>.tga` (override the folder
//! with `GOLDSRC_HOOKS_TEXTURE_HIRES_DIR`). `<name>` is the texture's name,
//! lowercased, with any character Windows forbids in a filename replaced by
//! `_`. `<hash>` is 8 hex digits of FNV-1a-32 over the original's mip-0 palette
//! indices followed by its 768-byte palette. The hash is what makes this safe
//! across maps: 25 names in the wsod25 set are reused by different maps with
//! different pixels, and a name alone would put one map's texture on another.
//! It also means one file covers every map that carries the identical texture.
//!
//! 24- or 32-bit TGA, uncompressed or RLE, any dimensions (power-of-two is
//! best: the engine rounds anything else, down as often as up). The folder is
//! indexed once, on the first world-texture load of the session.
//!
//! ## Opt-in
//!
//! Installed only with `GOLDSRC_HOOKS_TEXTURE_HIRES=1`. Live-tested 2026-09-23
//! across several wsod25 maps in one session (lennon2, railroad2_test,
//! armory_b6, harrington, anzio) at `gl_max_size 1024`: no crash, about 97% of
//! world texture loads replaced (the rest are sky/tool textures the replacement
//! set skips). The detail-texture limit raise came after that test.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use windows_sys::Win32::System::Memory::{MEM_COMMIT, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc};

use crate::detour;
use crate::engine;
use crate::names::console_name;
use crate::scan;

/// The command name that toggles verbose per-load logging. Registered in
/// `commands.rs`.
pub const NAME: &str = console_name!("log_texture_loads");

/// `GL_LoadTexture2`'s tail, from the optional upload callback through both
/// upload calls. Wildcards: the callback pointer's address and the two call
/// displacements.
const LOAD_TEXTURE2_TAIL: &str = "A1 ?? ?? ?? ?? 85 C0 74 0F 8B 4D 14 8B 55 08 57 51 53 52 FF D0 \
    83 C4 10 83 7D 0C 05 75 20 83 7D 20 04 75 1A 8B 45 28 8B 4D 1C 8B 55 14 50 6A 04 51 52 53 57 \
    E8 ?? ?? ?? ?? 83 C4 18 EB 1E 8B 45 28 8B 4D 24 8B 55 20 50 8B 45 1C 51 8B 4D 14 52 50 51 53 \
    57 E8 ?? ?? ?? ?? 83 C4 1C";

/// Offsets into [`LOAD_TEXTURE2_TAIL`]'s match.
mod tail {
    /// `cmp dword ptr [ebp+0xc], 5; jne upload8` -- the detoured span.
    pub const BRANCH: usize = 24;
    /// `cmp dword ptr [ebp+0x20], 4` -- where the not-a-sprite test resumes.
    pub const RESUME: usize = 30;
    /// `call GL_Upload32`.
    pub const CALL_UPLOAD32: usize = 52;
    /// The `GL_Upload8` argument setup the `jne` goes to.
    pub const UPLOAD8_ARGS: usize = 62;
    /// `call GL_Upload8`.
    pub const CALL_UPLOAD8: usize = 84;
    /// Just past `add esp, 0x1c`: both paths rejoin here.
    pub const AFTER: usize = 92;
}

/// `cmp dword ptr [ebp+0xc], 5` / `jne +0x20`.
const BRANCH_STOLEN: &[u8] = &[0x83, 0x7D, 0x0C, 0x05, 0x75, 0x20];

/// `GL_Upload32`'s entry through its "too big" check. Used to confirm the
/// `call` in [`LOAD_TEXTURE2_TAIL`] lands where expected, and to locate the
/// check and the buffer references.
const UPLOAD32: &str = "55 8B EC 83 EC 14 53 56 8D 45 FC 57 8D 4D F0 50 8D 55 EC 51 52 E8 ?? ?? ?? ?? \
    8B 5D 0C 8B 4D 10 8B D3 8B 35 ?? ?? ?? ?? 0F AF D1 03 F2 83 C4 0C 89 35 ?? ?? ?? ?? 8B 75 18 \
    83 FE 02 89 55 F8 74 0D A1 ?? ?? ?? ?? 8D 04 50 A3 ?? ?? ?? ?? D9 05 ?? ?? ?? ?? A1 ?? ?? ?? ?? \
    D8 1D ?? ?? ?? ?? 40 A3 ?? ?? ?? ?? DF E0 F6 C4 44 7B 41 83 FE 01 74 0A 83 FE 03 74 05 83 FE 04 \
    75 32 33 F6 85 D2 7E 2C 8B 7D 08 83 3F 00 75 1C 8B C6 99 F7 FB 50 52 51 8B 4D 08 53 51 57 E8 \
    ?? ?? ?? ?? 8B 55 F8 8B 4D 10 83 C4 18 46 83 C7 04 3B F2 7C D7 51 8D 55 F4 53 8D 45 0C 52 50 \
    E8 ?? ?? ?? ?? 8B 75 0C 8B 7D F4 8B C6 83 C4 10 0F AF C7 3D 00 00 08 00 89 45 F4 76 0D";

/// Offsets into `GL_Upload32`.
mod upload32 {
    /// `cmp eax, 0x80000; mov [ebp-0xc], eax; jbe ok`. At this point `esi` is
    /// the rounded width, `edi` the rounded height, `eax` their product.
    pub const SIZE_CHECK: usize = 0xca;
    /// `push "GL_LoadTexture: too big"; call Sys_Error`.
    pub const TOO_BIG: usize = 0xd4;
    /// Past the check: the `iType` switch.
    pub const SIZE_OK: usize = 0xe1;
    /// The five `push <scratch buffer>` instructions.
    pub const BUFFER_PUSHES: [usize; 5] = [0x1eb, 0x1fc, 0x22b, 0x26c, 0x29f];
}

/// `cmp eax, 0x80000` / `mov [ebp-0xc], eax` / `jbe +0xd`.
const SIZE_CHECK_STOLEN: &[u8] = &[0x3D, 0x00, 0x00, 0x08, 0x00, 0x89, 0x45, 0xF4, 0x76, 0x0D];

/// Offsets into `GL_Upload8`, each checked against its opcode bytes before
/// the operand is trusted.
mod upload8 {
    /// `mov dl, byte ptr [ecx + <texgamma table>]`.
    pub const GAMMA_TABLE: (usize, &[u8]) = (0x53, &[0x8A, 0x91]);
    /// `fld dword ptr [<gl_dither.value>]`.
    pub const DITHER: (usize, &[u8]) = (0x94, &[0xD9, 0x05]);
    /// `push <expansion buffer>` -- the static buffer placed right after
    /// `GL_Upload32`'s, which proves how big `GL_Upload32`'s is.
    pub const EXPANSION_BUFFER: (usize, &[u8]) = (0x276, &[0x68]);
}

/// The detail-texture loader, from its entry through its `GL_LoadTexture2`
/// call. Wildcards: the `malloc`, `snprintf`, `LoadTGA` and `GL_LoadTexture2`
/// call displacements and the "gfx/%s.tga" format string's address.
const DETAIL_LOADER: &str = "55 8B EC 81 EC 0C 01 00 00 53 56 57 68 00 00 10 00 E8 ?? ?? ?? ?? 8B 5D 08 \
    8B F0 53 68 ?? ?? ?? ?? 8D 85 F4 FE FF FF 68 04 01 00 00 50 83 CF FF E8 ?? ?? ?? ?? 83 C4 14 85 F6 \
    74 4D 8D 4D F8 6A 00 8D 55 FC 51 52 68 00 00 10 00 8D 85 F4 FE FF FF 56 50 E8 ?? ?? ?? ?? 83 C4 18 \
    85 C0 74 21 8B 4D F8 8B 55 FC 68 03 27 00 00 6A 00 6A 04 6A 01 56 51 52 6A 05 53 E8 ?? ?? ?? ??";

/// Offsets into [`DETAIL_LOADER`]: the two `push 0x100000`s.
mod detail {
    /// `malloc(0x100000)` -- the buffer the TGA is read into.
    pub const ALLOC_SIZE: usize = 0x0c;
    /// `LoadTGA(path, buffer, 0x100000, ...)` -- the size it's told it has.
    pub const LOADTGA_SIZE: usize = 0x46;
}

/// `push 0x100000`.
const DETAIL_STOCK_PUSH: &[u8] = &[0x68, 0x00, 0x00, 0x10, 0x00];
/// 1024x1024 RGBA.
const DETAIL_MAX_BYTES: usize = 1024 * 1024 * 4;

/// The stock `GL_Upload32` pixel budget, and its buffer (4 bytes a pixel).
const STOCK_MAX_PIXELS: usize = 0x80000;
/// What [`install`] raises it to.
const RAISED_MAX_PIXELS: usize = 0x10_0000;
/// The resample helpers' stack arrays: output width may never exceed this.
const MAX_OUTPUT_WIDTH: u32 = 1024;

/// `GLT_WORLD` in GoldSrc's `GL_TEXTURETYPE`.
const GLT_WORLD: u32 = 4;
/// `TEX_TYPE_NONE` / `TEX_TYPE_ALPHA`: the two `iType`s world textures use.
const TEX_TYPE_NONE: u32 = 0;
const TEX_TYPE_ALPHA: u32 = 1;

/// Where replacements live, relative to `hl.exe`, unless overridden.
const DEFAULT_DIR: &str = r"dod\dodstudio_hd_textures";
const DIR_ENV: &str = "GOLDSRC_HOOKS_TEXTURE_HIRES_DIR";

/// World textures seen, and how many were replaced.
static WORLD_SEEN: AtomicU32 = AtomicU32::new(0);
static REPLACED: AtomicU32 = AtomicU32::new(0);
/// Textures of any type seen -- what `has_observed` keys on.
static ANY_SEEN: AtomicU32 = AtomicU32::new(0);

/// The most recent replacement, for `status()`: name, original size, new size.
struct LastReplaced {
    name: String,
    from: (u32, u32),
    to: (u32, u32),
}
static LAST_REPLACED: Mutex<Option<LastReplaced>> = Mutex::new(None);

/// Whether to write a debug-log line for every load. Off by default -- a map
/// loads well over a hundred textures.
pub static LOG_TEXTURE_LOADS: AtomicBool = AtomicBool::new(false);

/// Engine addresses read by the stubs (`call`/`jmp dword ptr [slot]`) or by
/// [`decide`]. Set once by [`install`] before either stub can run.
static DECIDE_FN: AtomicUsize = AtomicUsize::new(0);
static UPLOAD32_FN: AtomicUsize = AtomicUsize::new(0);
static RESUME: AtomicUsize = AtomicUsize::new(0);
static UPLOAD8_ARGS: AtomicUsize = AtomicUsize::new(0);
static AFTER: AtomicUsize = AtomicUsize::new(0);
static SIZE_OK: AtomicUsize = AtomicUsize::new(0);
static TOO_BIG: AtomicUsize = AtomicUsize::new(0);
static GAMMA_TABLE: AtomicUsize = AtomicUsize::new(0);
static DITHER_VALUE: AtomicUsize = AtomicUsize::new(0);

/// Whether the 1024x1024 ceiling is in effect; replacements are capped to 512
/// per side otherwise.
static CEILING_RAISED: AtomicBool = AtomicBool::new(false);
/// Whether detail textures may be up to 1024x1024 (512x512 otherwise).
static DETAIL_RAISED: AtomicBool = AtomicBool::new(false);

/// `{ data, width, height }`, read by the swap stub as `[eax]`, `[eax+4]`,
/// `[eax+8]` -- three `usize`s are three contiguous dwords on this 32-bit
/// target.
static PENDING: [AtomicUsize; 3] = [const { AtomicUsize::new(0) }; 3];
/// Owns the pixels `PENDING` points at. Replaced on the next swap, by which
/// time `GL_Upload32` has long since copied them into its own buffer.
static PENDING_PIXELS: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// Installed once per process; see [`detour::Detour`] on why never undone.
static INSTALLED: Mutex<Option<Vec<detour::Detour>>> = Mutex::new(None);

/// The replacement folder's index, built on first use.
static INDEX: OnceLock<Index> = OnceLock::new();

struct Index {
    dir: PathBuf,
    files: HashMap<(String, u32), PathBuf>,
}

/// FNV-1a, 32-bit, over `parts` in order. Must match the texture pipeline's
/// Python implementation byte for byte.
fn fnv1a32(parts: &[&[u8]]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for part in parts {
        for &b in *part {
            hash ^= b as u32;
            hash = hash.wrapping_mul(0x0100_0193);
        }
    }
    hash
}

/// A texture name as it appears in a replacement's filename: lowercased, with
/// the characters Windows refuses in a filename replaced by `_`.
fn file_stem_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c.to_ascii_lowercase(),
        })
        .collect()
}

/// `name_1a2b3c4d.tga` -> `("name", 0x1a2b3c4d)`.
fn parse_file_name(file: &str) -> Option<(String, u32)> {
    let stem = file
        .strip_suffix(".tga")
        .or_else(|| file.strip_suffix(".TGA"))?;
    let (name, hash) = stem.rsplit_once('_')?;
    if name.is_empty() || hash.len() != 8 {
        return None;
    }
    let hash = u32::from_str_radix(hash, 16).ok()?;
    Some((name.to_ascii_lowercase(), hash))
}

fn replacement_dir() -> PathBuf {
    if let Ok(dir) = std::env::var(DIR_ENV)
        && !dir.trim().is_empty()
    {
        return PathBuf::from(dir.trim());
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_default()
        .join(DEFAULT_DIR)
}

fn build_index() -> Index {
    let dir = replacement_dir();
    let mut files = HashMap::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(key) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(parse_file_name)
            {
                files.insert(key, path);
            }
        }
    }
    unsafe {
        crate::debug::report(&format!(
            "texture_hires: indexed {} replacement(s) in {}",
            files.len(),
            dir.display()
        ))
    };
    Index { dir, files }
}

/// A decoded TGA: RGBA8, top row first.
struct Rgba {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

/// Decodes a 24- or 32-bit truecolor TGA, uncompressed (type 2) or RLE (type
/// 10). Anything else is refused rather than guessed at.
fn decode_tga(bytes: &[u8]) -> Result<Rgba, String> {
    if bytes.len() < 18 {
        return Err("shorter than a TGA header".into());
    }
    let id_len = bytes[0] as usize;
    let colormap_type = bytes[1];
    let image_type = bytes[2];
    let width = u16::from_le_bytes([bytes[12], bytes[13]]) as u32;
    let height = u16::from_le_bytes([bytes[14], bytes[15]]) as u32;
    let bpp = bytes[16];
    let top_first = bytes[17] & 0x20 != 0;
    if colormap_type != 0 || !matches!(image_type, 2 | 10) {
        return Err(format!(
            "unsupported TGA (colormap {colormap_type}, type {image_type}); save as truecolor"
        ));
    }
    if !matches!(bpp, 24 | 32) {
        return Err(format!("{bpp} bits per pixel; expected 24 or 32"));
    }
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err(format!("implausible size {width}x{height}"));
    }
    let bytes_pp = bpp as usize / 8;
    let count = (width * height) as usize;
    let mut src = bytes.get(18 + id_len..).ok_or("truncated after header")?;

    let mut pixels = Vec::with_capacity(count * 4);
    let mut push = |px: &[u8]| {
        let a = if bytes_pp == 4 { px[3] } else { 255 };
        pixels.extend_from_slice(&[px[2], px[1], px[0], a]);
    };
    if image_type == 2 {
        let need = count * bytes_pp;
        if src.len() < need {
            return Err("truncated pixel data".into());
        }
        for px in src[..need].chunks_exact(bytes_pp) {
            push(px);
        }
    } else {
        let mut done = 0;
        while done < count {
            let (&header, rest) = src.split_first().ok_or("truncated RLE data")?;
            let run = (header & 0x7f) as usize + 1;
            if done + run > count {
                return Err("RLE run overruns the image".into());
            }
            if header & 0x80 != 0 {
                let px = rest.get(..bytes_pp).ok_or("truncated RLE data")?;
                for _ in 0..run {
                    push(px);
                }
                src = &rest[bytes_pp..];
            } else {
                let raw = rest.get(..run * bytes_pp).ok_or("truncated RLE data")?;
                for px in raw.chunks_exact(bytes_pp) {
                    push(px);
                }
                src = &rest[run * bytes_pp..];
            }
            done += run;
        }
    }

    if !top_first {
        let row = width as usize * 4;
        let mut flipped = Vec::with_capacity(pixels.len());
        for y in (0..height as usize).rev() {
            flipped.extend_from_slice(&pixels[y * row..(y + 1) * row]);
        }
        pixels = flipped;
    }
    Ok(Rgba {
        width,
        height,
        pixels,
    })
}

/// Halves an image with a 2x2 box filter (odd edges clamp).
fn halve(img: &Rgba) -> Rgba {
    let (w, h) = (img.width as usize, img.height as usize);
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut out = Vec::with_capacity(nw * nh * 4);
    for y in 0..nh {
        for x in 0..nw {
            for c in 0..4 {
                let mut sum = 0u32;
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let sx = (x * 2 + dx).min(w - 1);
                    let sy = (y * 2 + dy).min(h - 1);
                    sum += img.pixels[(sy * w + sx) * 4 + c] as u32;
                }
                out.push(((sum + 2) / 4) as u8);
            }
        }
    }
    Rgba {
        width: nw as u32,
        height: nh as u32,
        pixels: out,
    }
}

/// Applies what `GL_Upload8` would have done to the original's palette:
/// texture gamma, `gl_dither`'s `c | c >> 6` for opaque textures, and the
/// all-zero transparent pixel for masked ones.
fn match_engine_expansion(pixels: &mut [u8], gamma: &[u8; 256], i_type: u32, dither: bool) {
    for px in pixels.chunks_exact_mut(4) {
        if i_type == TEX_TYPE_ALPHA && px[3] < 128 {
            px.copy_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        for c in &mut px[..3] {
            let mut v = gamma[*c as usize];
            if i_type == TEX_TYPE_NONE && dither {
                v |= v >> 6;
            }
            *c = v;
        }
        px[3] = 255;
    }
}

/// Loads, fits and prepares the replacement at `path`.
fn load_replacement(path: &Path, i_type: u32) -> Result<Rgba, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut img = decode_tga(&bytes)?;
    let cap = if CEILING_RAISED.load(Ordering::Acquire) {
        MAX_OUTPUT_WIDTH
    } else {
        MAX_OUTPUT_WIDTH / 2
    };
    while img.width > cap || img.height > cap {
        img = halve(&img);
    }

    // Safety: both addresses were read out of GL_Upload8's own instructions
    // and checked at install; the table is 256 bytes of hw.dll's .data.
    let gamma = unsafe { &*(GAMMA_TABLE.load(Ordering::Acquire) as *const [u8; 256]) };
    let dither = unsafe { *(DITHER_VALUE.load(Ordering::Acquire) as *const f32) } != 0.0;
    match_engine_expansion(&mut img.pixels, gamma, i_type, dither);
    Ok(img)
}

/// Called by the swap stub with `GL_LoadTexture2`'s frame pointer. Returns
/// `PENDING`'s address to upload a replacement, or null to carry on unchanged.
///
/// # Safety
///
/// Only called from the installed stub, while `GL_LoadTexture2`'s frame is
/// live: `[ebp+8]` .. `[ebp+0x28]` are its nine arguments.
unsafe extern "C" fn decide(frame: *const u8) -> *const AtomicUsize {
    let arg = |offset: usize| unsafe { (frame.add(offset) as *const u32).read_unaligned() };
    let name_ptr = arg(0x08) as *const std::ffi::c_char;
    let texture_type = arg(0x0c);
    let (width, height) = (arg(0x10), arg(0x14));
    let data = arg(0x18) as *const u8;
    let i_type = arg(0x20);
    let palette = arg(0x24) as *const u8;

    ANY_SEEN.fetch_add(1, Ordering::Relaxed);
    if texture_type != GLT_WORLD || name_ptr.is_null() {
        return std::ptr::null();
    }
    WORLD_SEEN.fetch_add(1, Ordering::Relaxed);

    // Safety: the identifier every caller passes is a NUL-terminated name.
    let name = unsafe { std::ffi::CStr::from_ptr(name_ptr) }.to_string_lossy();
    let log = LOG_TEXTURE_LOADS.load(Ordering::Relaxed);
    let skip = |why: &str| {
        if log {
            unsafe {
                crate::debug::report(&format!(
                    "texture_hires: world {name:?} {width}x{height} iType {i_type}: {why}"
                ))
            };
        }
        std::ptr::null()
    };

    if !matches!(i_type, TEX_TYPE_NONE | TEX_TYPE_ALPHA) {
        return skip("unexpected iType, left alone");
    }
    if data.is_null()
        || palette.is_null()
        || width == 0
        || height == 0
        || width > 4096
        || height > 4096
    {
        return skip("no palette data, left alone");
    }

    let index = INDEX.get_or_init(build_index);
    let stem = file_stem_name(&name);
    // The name check first: the hash is only worth computing for a texture
    // that has any replacement at all.
    if !index.files.keys().any(|(n, _)| *n == stem) {
        return skip("no replacement");
    }

    // Safety: GL_Upload8 is about to read exactly these spans itself.
    let (indices, pal) = unsafe {
        (
            std::slice::from_raw_parts(data, (width * height) as usize),
            std::slice::from_raw_parts(palette, 768),
        )
    };
    let hash = fnv1a32(&[indices, pal]);
    let Some(path) = index.files.get(&(stem.clone(), hash)) else {
        return skip(&format!(
            "replacement(s) named {stem:?} exist, none for this content ({hash:08x})"
        ));
    };

    let img = match load_replacement(path, i_type) {
        Ok(img) => img,
        Err(why) => {
            unsafe {
                crate::debug::report(&format!(
                    "texture_hires: could not use {}: {why}",
                    path.display()
                ))
            };
            return std::ptr::null();
        }
    };

    let Ok(mut owned) = PENDING_PIXELS.lock() else {
        return std::ptr::null();
    };
    *owned = img.pixels;
    PENDING[0].store(owned.as_mut_ptr() as usize, Ordering::Release);
    PENDING[1].store(img.width as usize, Ordering::Release);
    PENDING[2].store(img.height as usize, Ordering::Release);
    drop(owned);

    REPLACED.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut last) = LAST_REPLACED.lock() {
        *last = Some(LastReplaced {
            name: name.to_string(),
            from: (width, height),
            to: (img.width, img.height),
        });
    }
    if log {
        unsafe {
            crate::debug::report(&format!(
                "texture_hires: world {name:?} {width}x{height} -> {}x{} from {}",
                img.width,
                img.height,
                path.display()
            ))
        };
    }
    PENDING.as_ptr()
}

/// Appends `FF 15`/`FF 25 <abs32>`: an indirect call/jump through `slot`.
fn indirect(code: &mut Vec<u8>, modrm: u8, slot: &AtomicUsize) {
    code.extend_from_slice(&[0xFF, modrm]);
    code.extend_from_slice(&(slot.as_ptr() as usize as u32).to_le_bytes());
}
const CALL: u8 = 0x15;
const JMP: u8 = 0x25;

/// The swap stub, replacing `cmp [ebp+0xc], 5; jne upload8`.
///
/// ```asm
///     push ebp
///     call [DECIDE_FN]              ; decide(ebp)
///     add esp, 4
///     test eax, eax
///     jnz swap
///     cmp dword ptr [ebp+0xc], 5    ; the stolen bytes, verbatim in effect
///     jne +6
///     jmp [RESUME]
///     jmp [UPLOAD8_ARGS]
/// swap:
///     push [ebp+0x28]               ; filter
///     push [ebp+0x20]               ; iType
///     push [ebp+0x1c]               ; mipmap
///     push [eax+8]                  ; replacement height
///     push [eax+4]                  ; replacement width
///     push [eax]                    ; replacement pixels
///     call [UPLOAD32_FN]
///     add esp, 0x18
///     jmp [AFTER]
/// ```
///
/// `eax`/`ecx`/`edx` and the flags are dead here: both original branch targets
/// reload what they use from the frame. `ebx`/`esi`/`edi`/`ebp` survive both
/// calls (cdecl callee-saved), and the stack is balanced on every path.
fn swap_stub() -> Vec<u8> {
    let mut code = vec![0x55]; // push ebp
    indirect(&mut code, CALL, &DECIDE_FN);
    code.extend_from_slice(&[0x83, 0xC4, 0x04]); // add esp, 4
    code.extend_from_slice(&[0x85, 0xC0]); // test eax, eax
    code.extend_from_slice(&[0x75, 0x12]); // jnz swap (+18)
    code.extend_from_slice(&[0x83, 0x7D, 0x0C, 0x05]); // cmp dword ptr [ebp+0xc], 5
    code.extend_from_slice(&[0x75, 0x06]); // jne +6
    indirect(&mut code, JMP, &RESUME);
    indirect(&mut code, JMP, &UPLOAD8_ARGS);
    // swap:
    code.extend_from_slice(&[0xFF, 0x75, 0x28]); // push [ebp+0x28]
    code.extend_from_slice(&[0xFF, 0x75, 0x20]); // push [ebp+0x20]
    code.extend_from_slice(&[0xFF, 0x75, 0x1C]); // push [ebp+0x1c]
    code.extend_from_slice(&[0xFF, 0x70, 0x08]); // push [eax+8]
    code.extend_from_slice(&[0xFF, 0x70, 0x04]); // push [eax+4]
    code.extend_from_slice(&[0xFF, 0x30]); // push [eax]
    indirect(&mut code, CALL, &UPLOAD32_FN);
    code.extend_from_slice(&[0x83, 0xC4, 0x18]); // add esp, 0x18
    indirect(&mut code, JMP, &AFTER);
    code
}

/// The size-check stub, replacing `cmp eax, 0x80000; mov [ebp-0xc], eax;
/// jbe ok`.
///
/// ```asm
///     mov [ebp-0xc], eax            ; stolen; flags untouched
///     cmp esi, 1024                 ; rounded width: the resample arrays' bound
///     ja  too_big
///     cmp eax, 0x100000             ; rounded width * height: our buffer's size
///     ja  too_big
///     jmp [SIZE_OK]
/// too_big:
///     jmp [TOO_BIG]                 ; the engine's own Sys_Error
/// ```
fn size_check_stub() -> Vec<u8> {
    let mut code = vec![0x89, 0x45, 0xF4]; // mov [ebp-0xc], eax
    code.extend_from_slice(&[0x81, 0xFE]); // cmp esi, imm32
    code.extend_from_slice(&MAX_OUTPUT_WIDTH.to_le_bytes());
    code.extend_from_slice(&[0x77, 0x0D]); // ja too_big (+13)
    code.push(0x3D); // cmp eax, imm32
    code.extend_from_slice(&(RAISED_MAX_PIXELS as u32).to_le_bytes());
    code.extend_from_slice(&[0x77, 0x06]); // ja too_big (+6)
    indirect(&mut code, JMP, &SIZE_OK);
    indirect(&mut code, JMP, &TOO_BIG);
    code
}

/// Reads the operand after `opcode` at `at`, refusing if the opcode bytes are
/// not what the disassembly showed.
///
/// Safety: `at .. at + opcode.len() + 4` must be mapped.
unsafe fn operand_after(at: usize, opcode: &[u8]) -> Result<usize, String> {
    let present = unsafe { std::slice::from_raw_parts(at as *const u8, opcode.len()) };
    if present != opcode {
        return Err(format!(
            "expected {opcode:02x?} at {at:#x}, found {present:02x?}"
        ));
    }
    Ok(unsafe { ((at + opcode.len()) as *const u32).read_unaligned() } as usize)
}

/// Target of the `E8 rel32` at `at`.
///
/// Safety: as [`operand_after`].
unsafe fn call_target(at: usize) -> Result<usize, String> {
    let rel = unsafe { operand_after(at, &[0xE8]) }? as u32;
    Ok((at as u32).wrapping_add(5).wrapping_add(rel) as usize)
}

fn check_span(at: usize, expected: &[u8]) -> Result<(), String> {
    // Safety: callers only pass spans inside a matched signature.
    let present = unsafe { std::slice::from_raw_parts(at as *const u8, expected.len()) };
    if present == expected {
        Ok(())
    } else {
        Err(format!(
            "expected {expected:02x?} at {at:#x}, found {present:02x?}"
        ))
    }
}

/// Raises `GL_Upload32`'s ceiling to 1024x1024: a bigger scratch buffer, then
/// a size check that matches it. Every byte to be changed is verified first;
/// nothing is written unless all of it matches.
fn raise_ceiling(upload32: usize, upload8: usize) -> Result<detour::Detour, String> {
    let check_at = upload32 + upload32::SIZE_CHECK;
    check_span(check_at, SIZE_CHECK_STOLEN)?;

    let mut stock_buffer = None;
    for off in upload32::BUFFER_PUSHES {
        let buffer = unsafe { operand_after(upload32 + off, &[0x68]) }?;
        match stock_buffer {
            None => stock_buffer = Some(buffer),
            Some(b) if b != buffer => {
                return Err(format!(
                    "buffer pushes disagree: {b:#x} vs {buffer:#x} at +{off:#x}"
                ));
            }
            Some(_) => {}
        }
    }
    let stock_buffer = stock_buffer.ok_or("no buffer pushes")?;
    // The next static buffer starts exactly 2 MB later: that is the proof
    // the stock one holds STOCK_MAX_PIXELS pixels and no more.
    let (off, op) = upload8::EXPANSION_BUFFER;
    let next = unsafe { operand_after(upload8 + off, op) }?;
    if next != stock_buffer + STOCK_MAX_PIXELS * 4 {
        return Err(format!(
            "GL_Upload32's buffer {stock_buffer:#x} is not {} bytes before {next:#x}; its size is unknown",
            STOCK_MAX_PIXELS * 4
        ));
    }

    // Safety: a fresh allocation, never freed -- the engine keeps using it.
    let buffer = unsafe {
        VirtualAlloc(
            std::ptr::null(),
            RAISED_MAX_PIXELS * 4,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    } as usize;
    if buffer == 0 {
        return Err("could not allocate the larger texture buffer".into());
    }

    // Buffer first, check second: a larger texture must never be let through
    // while the small buffer is still the one in use.
    for off in upload32::BUFFER_PUSHES {
        // Safety: verified above to be `push imm32` in GL_Upload32.
        if !unsafe {
            crate::patch::write_code_bytes(upload32 + off + 1, &(buffer as u32).to_le_bytes())
        } {
            return Err(format!(
                "could not repoint the buffer push at +{off:#x} (earlier ones already point at the new, larger buffer -- harmless)"
            ));
        }
    }
    SIZE_OK.store(upload32 + upload32::SIZE_OK, Ordering::Release);
    TOO_BIG.store(upload32 + upload32::TOO_BIG, Ordering::Release);
    // Safety: span verified above; the only branch to anywhere near it is the
    // fallthrough into its first byte.
    let detour = unsafe { detour::install(check_at, SIZE_CHECK_STOLEN.len(), &size_check_stub()) }?;
    CEILING_RAISED.store(true, Ordering::Release);
    Ok(detour)
}

/// Lets detail textures (`gfx/detail/*.tga`) be up to 1024x1024: the detail
/// loader `malloc`s a 1 MB buffer and tells `LoadTGA` it holds 1 MB, so any
/// TGA over 512x512 pixels' worth fails with "LoadTGA: texture too large
/// (WxH>256x256)" (the "256x256" is hardcoded text, not the real limit) and
/// the detail layer silently goes missing. Both immediates become 4 MB -- the
/// allocation first, so the limit never exceeds what was allocated. What it
/// loads goes to `GL_Upload32` as RGBA, which [`raise_ceiling`] already opened
/// to 1024x1024.
fn raise_detail_limit(base: usize) -> Result<(), String> {
    // Safety: `base` is hw.dll's module handle, mapped for the session.
    let loader = unsafe { scan::find_unique(base, DETAIL_LOADER) }
        .map_err(|why| format!("could not locate the detail texture loader -- {why}"))?;
    for off in [detail::ALLOC_SIZE, detail::LOADTGA_SIZE] {
        check_span(loader + off, DETAIL_STOCK_PUSH)?;
    }
    for off in [detail::ALLOC_SIZE, detail::LOADTGA_SIZE] {
        // Safety: verified above to be `push 0x100000` in the detail loader.
        if !unsafe {
            crate::patch::write_code_bytes(
                loader + off + 1,
                &(DETAIL_MAX_BYTES as u32).to_le_bytes(),
            )
        } {
            return Err(format!("could not rewrite the size at +{off:#x}"));
        }
    }
    DETAIL_RAISED.store(true, Ordering::Release);
    unsafe {
        crate::debug::report(&format!(
            "texture_hires: detail texture limit raised to 1024x1024 at +{:#x}",
            loader - base
        ))
    };
    Ok(())
}

/// Installs the world-texture swap, and raises the upload ceiling, once.
///
/// Every signature and every byte about to be overwritten is checked first;
/// a mismatch installs nothing and says why.
pub fn install() -> Result<(), String> {
    let mut slot = INSTALLED
        .lock()
        .map_err(|_| "the texture_hires detour lock is poisoned".to_string())?;
    if slot.is_some() {
        return Ok(());
    }

    let base = engine::engine_module_base().ok_or("hw.dll is not loaded yet")?;
    let wrong_build = |what: &str, why: String| {
        format!(
            "could not locate {what} -- {why}. These signatures are the pre-Anniversary hw.dll's; \
             DoD Studio only launches that build"
        )
    };
    // Safety: `base` is a module handle the loader gave us, mapped for the session.
    let tail = unsafe { scan::find_unique(base, LOAD_TEXTURE2_TAIL) }
        .map_err(|why| wrong_build("GL_LoadTexture2's upload branch", why))?;
    let upload32 = unsafe { scan::find_unique(base, UPLOAD32) }
        .map_err(|why| wrong_build("GL_Upload32", why))?;

    let called32 = unsafe { call_target(tail + tail::CALL_UPLOAD32) }?;
    if called32 != upload32 {
        return Err(format!(
            "GL_LoadTexture2 calls {called32:#x}, but GL_Upload32 matched at {upload32:#x}"
        ));
    }
    let upload8 = unsafe { call_target(tail + tail::CALL_UPLOAD8) }?;
    let (off, op) = upload8::GAMMA_TABLE;
    let gamma = unsafe { operand_after(upload8 + off, op) }?;
    let (off, op) = upload8::DITHER;
    let dither = unsafe { operand_after(upload8 + off, op) }?;
    check_span(tail + tail::BRANCH, BRANCH_STOLEN)?;

    let mut detours = Vec::new();
    match raise_ceiling(upload32, upload8) {
        Ok(d) => detours.push(d),
        Err(why) => unsafe {
            crate::debug::report(&format!(
                "texture_hires: upload ceiling left at 512x1024, replacements capped at 512 -- {why}"
            ))
        },
    }
    // Only with the upload ceiling raised: a 1024x1024 detail texture would
    // otherwise get past LoadTGA and hit GL_Upload32's stock Sys_Error.
    if CEILING_RAISED.load(Ordering::Acquire)
        && let Err(why) = raise_detail_limit(base)
    {
        unsafe {
            crate::debug::report(&format!(
                "texture_hires: detail textures left at 512x512 -- {why}"
            ))
        };
    }

    GAMMA_TABLE.store(gamma, Ordering::Release);
    DITHER_VALUE.store(dither, Ordering::Release);
    UPLOAD32_FN.store(upload32, Ordering::Release);
    RESUME.store(tail + tail::RESUME, Ordering::Release);
    UPLOAD8_ARGS.store(tail + tail::UPLOAD8_ARGS, Ordering::Release);
    AFTER.store(tail + tail::AFTER, Ordering::Release);
    DECIDE_FN.store(decide as *const () as usize, Ordering::Release);

    let branch = tail + tail::BRANCH;
    // Safety: span verified above; the one branch into it (the callback
    // test's `je`) targets its first byte, where the new jump starts.
    let swap = unsafe { detour::install(branch, BRANCH_STOLEN.len(), &swap_stub()) }?;
    unsafe {
        crate::debug::report(&format!(
            "texture_hires: swap hook at +{:#x} (stub {:#x}), GL_Upload32 +{:#x}, ceiling {}; replacements from {}",
            branch - base,
            swap.stub_address(),
            upload32 - base,
            if CEILING_RAISED.load(Ordering::Acquire) {
                "1024x1024"
            } else {
                "stock"
            },
            replacement_dir().display()
        ))
    };
    detours.push(swap);
    *slot = Some(detours);
    Ok(())
}

/// One line for `dodstudio_debug_status`.
pub fn status() -> String {
    let world = WORLD_SEEN.load(Ordering::Relaxed);
    let replaced = REPLACED.load(Ordering::Relaxed);
    let last = LAST_REPLACED
        .lock()
        .ok()
        .and_then(|l| {
            l.as_ref().map(|r| {
                format!(
                    ", last: {:?} {}x{} -> {}x{}",
                    r.name, r.from.0, r.from.1, r.to.0, r.to.1
                )
            })
        })
        .unwrap_or_default();
    let files = INDEX
        .get()
        .map(|i| format!("{} file(s) in {}", i.files.len(), i.dir.display()))
        .unwrap_or_else(|| "folder not read yet".to_string());
    format!(
        "{NAME}: {replaced} of {world} world texture load(s) replaced{last}; {files}; ceiling {}, detail textures {} (logging {})",
        if CEILING_RAISED.load(Ordering::Relaxed) {
            "1024x1024"
        } else {
            "stock"
        },
        if DETAIL_RAISED.load(Ordering::Relaxed) {
            "up to 1024x1024"
        } else {
            "up to 512x512"
        },
        if LOG_TEXTURE_LOADS.load(Ordering::Relaxed) {
            "on"
        } else {
            "off"
        }
    )
}

/// Whether the hook has seen any texture load this session -- gates whether
/// `status_text()` includes this module's line at all.
pub fn has_observed() -> bool {
    ANY_SEEN.load(Ordering::Relaxed) > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(pattern: &str) -> Vec<String> {
        pattern.split_whitespace().map(str::to_string).collect()
    }

    fn fixed(pattern: &str, at: usize, expected: &[u8]) {
        let toks = tokens(pattern);
        for (i, &b) in expected.iter().enumerate() {
            assert_eq!(
                u8::from_str_radix(&toks[at + i], 16).ok(),
                Some(b),
                "pattern byte {} should be {b:02X}",
                at + i
            );
        }
    }

    #[test]
    fn patterns_parse() {
        scan::Pattern::parse(LOAD_TEXTURE2_TAIL).unwrap();
        scan::Pattern::parse(UPLOAD32).unwrap();
    }

    #[test]
    fn tail_offsets_land_on_the_instructions_they_name() {
        assert_eq!(tokens(LOAD_TEXTURE2_TAIL).len(), tail::AFTER);
        fixed(LOAD_TEXTURE2_TAIL, tail::BRANCH, BRANCH_STOLEN);
        fixed(LOAD_TEXTURE2_TAIL, tail::RESUME, &[0x83, 0x7D, 0x20, 0x04]);
        fixed(LOAD_TEXTURE2_TAIL, tail::CALL_UPLOAD32, &[0xE8]);
        fixed(LOAD_TEXTURE2_TAIL, tail::UPLOAD8_ARGS, &[0x8B, 0x45, 0x28]);
        fixed(LOAD_TEXTURE2_TAIL, tail::CALL_UPLOAD8, &[0xE8]);
        fixed(LOAD_TEXTURE2_TAIL, tail::AFTER - 3, &[0x83, 0xC4, 0x1C]);
    }

    #[test]
    fn detail_offsets_land_on_the_size_pushes() {
        scan::Pattern::parse(DETAIL_LOADER).unwrap();
        fixed(DETAIL_LOADER, detail::ALLOC_SIZE, DETAIL_STOCK_PUSH);
        fixed(DETAIL_LOADER, detail::LOADTGA_SIZE, DETAIL_STOCK_PUSH);
        // The new size must hold exactly what GL_Upload32's raised ceiling allows.
        assert_eq!(DETAIL_MAX_BYTES, RAISED_MAX_PIXELS * 4);
    }

    #[test]
    fn the_size_check_is_the_end_of_the_upload32_pattern() {
        let toks = tokens(UPLOAD32);
        assert_eq!(toks.len(), upload32::SIZE_CHECK + SIZE_CHECK_STOLEN.len());
        fixed(UPLOAD32, upload32::SIZE_CHECK, SIZE_CHECK_STOLEN);
        // `jbe +0xd` from just past the span lands on SIZE_OK; the span is
        // followed directly by the Sys_Error push.
        let past = upload32::SIZE_CHECK + SIZE_CHECK_STOLEN.len();
        assert_eq!(past, upload32::TOO_BIG);
        assert_eq!(past + 0x0d, upload32::SIZE_OK);
    }

    #[test]
    fn swap_stub_branches_land_where_the_comments_say() {
        let code = swap_stub();
        // jnz at [12]: target = 14 + 0x12 = the first push.
        assert_eq!(&code[12..14], &[0x75, 0x12]);
        assert_eq!(&code[14 + 0x12..14 + 0x12 + 3], &[0xFF, 0x75, 0x28]);
        // The stolen compare, then a jne +6 that skips exactly the RESUME jump.
        assert_eq!(&code[14..18], &BRANCH_STOLEN[..4]);
        assert_eq!(&code[18..20], &[0x75, 0x06]);
        assert_eq!(&code[20..22], &[0xFF, 0x25]);
        assert_eq!(&code[26..28], &[0xFF, 0x25]);
        assert_eq!(code.len(), 32 + 17 + 6 + 3 + 6);
    }

    #[test]
    fn size_check_stub_branches_land_where_the_comments_say() {
        let code = size_check_stub();
        // ja at [9] (+13) and ja at [16] (+6) both land on the TOO_BIG jump.
        assert_eq!(code[9], 0x77);
        assert_eq!(11 + code[10] as usize, 24);
        assert_eq!(code[16], 0x77);
        assert_eq!(18 + code[17] as usize, 24);
        assert_eq!(&code[18..20], &[0xFF, 0x25]);
        assert_eq!(&code[24..26], &[0xFF, 0x25]);
        assert_eq!(code.len(), 30);
    }

    #[test]
    fn fnv_matches_reference_vectors() {
        assert_eq!(fnv1a32(&[b""]), 0x811c_9dc5);
        assert_eq!(fnv1a32(&[b"a"]), 0xe40c_292c);
        assert_eq!(fnv1a32(&[b"foo", b"bar"]), fnv1a32(&[b"foobar"]));
    }

    #[test]
    fn file_names_round_trip() {
        assert_eq!(file_stem_name("{Cancello"), "{cancello");
        assert_eq!(file_stem_name("a*b?c"), "a_b_c");
        assert_eq!(
            parse_file_name("+0lambda_1a2b3c4d.tga"),
            Some(("+0lambda".to_string(), 0x1a2b_3c4d))
        );
        assert_eq!(
            parse_file_name("under_score_DEADBEEF.TGA"),
            Some(("under_score".to_string(), 0xdead_beef))
        );
        assert_eq!(parse_file_name("nohash.tga"), None);
        assert_eq!(parse_file_name("x_123.tga"), None);
        assert_eq!(parse_file_name("x_1a2b3c4d.png"), None);
    }

    fn tga(image_type: u8, bpp: u8, top_first: bool, w: u16, h: u16, body: &[u8]) -> Vec<u8> {
        let mut v = vec![0, 0, image_type, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        v.extend_from_slice(&w.to_le_bytes());
        v.extend_from_slice(&h.to_le_bytes());
        v.push(bpp);
        v.push(if top_first { 0x20 } else { 0 });
        v.extend_from_slice(body);
        v
    }

    #[test]
    fn decodes_uncompressed_bottom_up_24bit() {
        // 1x2, bottom row first: bottom = blue, top = red (stored BGR).
        let img = decode_tga(&tga(2, 24, false, 1, 2, &[255, 0, 0, 0, 0, 255])).unwrap();
        assert_eq!((img.width, img.height), (1, 2));
        assert_eq!(img.pixels, vec![255, 0, 0, 255, 0, 0, 255, 255]);
    }

    #[test]
    fn decodes_rle_32bit() {
        // One run packet of 3 green, then a raw packet of 1 half-alpha red.
        let body = [0x82, 0, 255, 0, 255, 0x00, 0, 0, 255, 128];
        let img = decode_tga(&tga(10, 32, true, 4, 1, &body)).unwrap();
        assert_eq!(
            img.pixels,
            vec![
                0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 255, 0, 0, 128
            ]
        );
    }

    #[test]
    fn refuses_what_it_does_not_understand() {
        assert!(decode_tga(&tga(1, 8, true, 1, 1, &[0])).is_err());
        assert!(decode_tga(&tga(2, 16, true, 1, 1, &[0, 0])).is_err());
        assert!(decode_tga(&tga(2, 24, true, 2, 2, &[0; 5])).is_err());
        assert!(decode_tga(&tga(10, 24, true, 1, 1, &[0x85, 1, 2, 3])).is_err());
    }

    #[test]
    fn halving_averages_and_clamps() {
        let img = Rgba {
            width: 3,
            height: 1,
            pixels: vec![0, 0, 0, 0, 200, 200, 200, 200, 9, 9, 9, 9],
        };
        let half = halve(&img);
        assert_eq!((half.width, half.height), (1, 1));
        assert_eq!(half.pixels, vec![100; 4]);
    }

    #[test]
    fn expansion_matches_the_engine() {
        let mut gamma = [0u8; 256];
        for (i, g) in gamma.iter_mut().enumerate() {
            *g = (255 - i) as u8;
        }
        // Opaque with dither: gamma then c | c >> 6; alpha forced to 255.
        let mut px = vec![0, 3, 255, 7];
        match_engine_expansion(&mut px, &gamma, TEX_TYPE_NONE, true);
        assert_eq!(px, vec![255, 252 | 3, 0, 255]);
        // Masked: below half alpha becomes all-zero, above becomes opaque.
        let mut px = vec![10, 10, 10, 100, 10, 10, 10, 200];
        match_engine_expansion(&mut px, &gamma, TEX_TYPE_ALPHA, true);
        assert_eq!(px, vec![0, 0, 0, 0, 245, 245, 245, 255]);
    }

    #[test]
    fn status_reports_before_any_load() {
        if !has_observed() {
            assert!(status().contains("0 of 0"));
        }
    }
}
