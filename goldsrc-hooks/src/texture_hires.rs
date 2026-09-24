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
//! Everything lives under one folder, `<game>\dod\dodstudio_hd\`, so it can be
//! backed up, copied or deleted as a unit:
//!
//! ```text
//! dodstudio_hd\
//!     world\   models\   sprites\   detail\   sky\      <- one per asset type
//!         ultrasharp\  remacri\  siax\  generalv3\  x4plus\  plain\  blend\
//!         overrides\                           <- single-file picks, win over any style
//! ```
//!
//! `dodstudio_hd_style <name>` (default `ultrasharp`; `GOLDSRC_HOOKS_HD_STYLE`
//! if the cvar can't be registered) picks the style subfolder, once, at the
//! first map load: the engine keeps every texture it has uploaded, so a change
//! mid-session would only reach textures not loaded yet. A style folder that
//! doesn't exist simply means "originals" (plus overrides). Deleting the style
//! folders you don't use is fine.
//!
//! Map textures are `world\<style>\<name>_<hash>.tga`. `<name>` is the texture's name,
//! lowercased, with any character Windows forbids in a filename replaced by
//! `_`. `<hash>` is 8 hex digits of FNV-1a-32 over the original's mip-0 palette
//! indices followed by its 768-byte palette. The hash is what makes this safe
//! across maps: 25 names in the wsod25 set are reused by different maps with
//! different pixels, and a name alone would put one map's texture on another.
//! It also means one file covers every map that carries the identical texture.
//!
//! Model skins (`GLT_STUDIO`, uploaded by `Mod_LoadStudioModel` through the
//! same branch) go in `models\<style>\<texture>_<hash>.tga`.
//! The engine's identifier is the model path glued to the texture name
//! (`models/v_garand.mdlgarand.bmp`); files use just the texture name, with the
//! hash telling apart same-named skins from different models or installs.
//! Player skins recoloured per player (`DM_Base.bmp` remaps) go through a
//! different path with a per-player palette and are not replaced.
//!
//! World sprite frames (`GLT_SPRITE`: muzzle flashes, smoke, explosions,
//! uploaded one frame at a time by `Mod_LoadSpriteFrame`, again through the
//! same branch) go in `sprites\<style>\<sprite>_<frame>_<hash>.tga`: the
//! sprite's file name without its folder or `.spr`, the frame number as the
//! engine counts it (frame `j` of group `i` is `i * 100 + j`), and the same
//! hash over that frame's pixels and the sprite's palette. The sprite's
//! render format picks the `iType`, and so what the replacement must look
//! like: normal and additive sprites are opaque RGB, alpha-test sprites cut
//! out like masked world textures, and index-alpha sprites (smoke) use only
//! the file's alpha -- the colour is always palette entry 255's, as in the
//! engine. HUD sprites are not replaced (see `GLT_SPRITE`).
//!
//! HD **detail** textures go in `detail\<style>\`, named
//! exactly as under `gfx\detail\` (e.g. `detail\ultrasharp\1.tga` replaces
//! `gfx\detail\1.tga`), so the game's own `gfx\detail` never has to change.
//! The detail loader's path is rewritten just before `LoadTGA` reads it; a copy
//! too big for the loader's buffer is skipped in favour of the original.
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
/// The stock detail buffer.
const DETAIL_STOCK_BYTES: usize = 0x10_0000;
/// Where in [`DETAIL_LOADER`] the path-redirect detour goes: `lea eax,
/// [ebp-0x10c]`, loading the path buffer's address for `LoadTGA`.
const DETAIL_PATH_AT: usize = 0x4b;
const DETAIL_PATH_STOLEN: &[u8] = &[0x8D, 0x85, 0xF4, 0xFE, 0xFF, 0xFF];
/// 1024x1024 RGBA.
const DETAIL_MAX_BYTES: usize = 1024 * 1024 * 4;

/// `R_LoadSkys`'s entry through its `malloc` of the 256x256 RGBA face buffer.
/// Wildcards: globals' addresses and call displacements.
const SKY_LOADER: &str = "55 8B EC 83 EC 6C A1 ?? ?? ?? ?? 56 57 33 FF 3B C7 89 7D F4 75 25 BE ?? ?? ?? ?? \
    39 3E 74 0B 56 6A 01 FF 15 ?? ?? ?? ?? 89 3E 83 C6 04 81 FE ?? ?? ?? ?? 7C E6 5F 5E 8B E5 5D C3 39 3D \
    ?? ?? ?? ?? 74 1D D9 05 ?? ?? ?? ?? D8 1D ?? ?? ?? ?? DF E0 F6 C4 44 7B 0A 89 7D F8 E8 ?? ?? ?? ?? EB 07 \
    C7 45 F8 01 00 00 00 68 00 00 04 00 E8 ?? ?? ?? ?? 83 C4 04";
/// `push 0x40000` -- the face buffer's `malloc` size, in [`SKY_LOADER`].
const SKY_MALLOC_AT: usize = 0x67;
const SKY_STOCK_PUSH: &[u8] = &[0x68, 0x00, 0x00, 0x04, 0x00];

/// `R_LoadSkys`'s per-face tail: the end of the TGA gamma loop, the texture
/// bind and the `glTexImage2D` call, whose width and height are hardcoded
/// `push 0x100`s. Wildcards: globals' addresses and call displacements.
const SKY_UPLOAD: &str = "8D 0C 02 81 F9 00 00 04 00 0F 8C 7B FF FF FF 8B 04 9D ?? ?? ?? ?? 85 C0 75 0C \
    E8 ?? ?? ?? ?? 89 04 9D ?? ?? ?? ?? 8B 14 9D ?? ?? ?? ?? 52 E8 ?? ?? ?? ?? 8D 45 E0 8D 4D D8 50 8D 55 D4 \
    51 52 E8 ?? ?? ?? ?? 8B 45 E0 83 C4 10 83 F8 20 56 68 01 14 00 00 68 08 19 00 00 6A 00 68 00 01 00 00 \
    68 00 01 00 00 75 07 68 58 80 00 00 EB 05 68 57 80 00 00 6A 00 68 E1 0D 00 00 FF 15 ?? ?? ?? ??";
/// Offsets into [`SKY_UPLOAD`].
mod sky {
    /// `mov eax, [ebx*4 + skytexturenums]` -- the detoured span. Every path
    /// that has a loaded, gamma-corrected face reaches it (the gamma loop's
    /// end and its `gl_dither` == 0 skip both land here).
    pub const HOOK_AT: usize = 0x0f;
    pub const HOOK_LEN: usize = 7;
    /// `push 0x100`: height, then width, for `glTexImage2D`.
    pub const HEIGHT_PUSH: usize = 0x5a;
    pub const WIDTH_PUSH: usize = 0x5f;
}
/// `push 0x100`.
const SKY_DIM_PUSH: &[u8] = &[0x68, 0x00, 0x01, 0x00, 0x00];
/// The stock face size, and the largest replacement face (1024x1024 RGBA).
const SKY_STOCK_SIDE: u32 = 256;
const SKY_MAX_SIDE: u32 = 1024;
const SKY_MAX_BYTES: usize = (SKY_MAX_SIDE * SKY_MAX_SIDE * 4) as usize;
/// `R_LoadSkys`'s path buffer: `char path[64]` at `ebp-0x6c`, filled with
/// `gfx/env/<skyname><face>.tga` before the face loads.
const SKY_PATH_OFFSET: usize = 0x6c;
const SKY_PATH_CAP: usize = 0x40;
const SKY_PREFIX: &str = "gfx/env/";
/// HD sky faces, beside the other HD folders: `dodstudio_hd\sky\<style>\<skyname><face>.tga`.
const SKY_DIR: &str = "dodstudio_hd/sky";

/// The stock `GL_Upload32` pixel budget, and its buffer (4 bytes a pixel).
const STOCK_MAX_PIXELS: usize = 0x80000;
/// What [`install`] raises it to.
const RAISED_MAX_PIXELS: usize = 0x10_0000;
/// The resample helpers' stack arrays: output width may never exceed this.
const MAX_OUTPUT_WIDTH: u32 = 1024;

/// `GLT_STUDIO` / `GLT_WORLD` / `GLT_SPRITE` in GoldSrc's `GL_TEXTURETYPE`:
/// model skins, map textures and world sprite frames, the three this module
/// replaces. HUD sprites (`GLT_HUDSPRITE`, 2) are left alone: the HUD draws
/// them pixel for pixel, without mipmaps, so a bigger copy would only be
/// shrunk back down, badly.
const GLT_STUDIO: u32 = 3;
const GLT_WORLD: u32 = 4;
const GLT_SPRITE: u32 = 5;
/// `TEX_TYPE_NONE` / `TEX_TYPE_ALPHA`: the two `iType`s world textures use.
const TEX_TYPE_NONE: u32 = 0;
const TEX_TYPE_ALPHA: u32 = 1;
/// `TEX_TYPE_ALPHA_GRADIENT`: index-alpha sprites (`SPR_INDEXALPHA`).
/// `GL_Upload8` draws every pixel in palette entry 255's colour, with the
/// pixel's palette index as its alpha.
const TEX_TYPE_ALPHA_GRADIENT: u32 = 3;
/// `TEX_TYPE_RGBA`: already truecolour. As `GLT_SPRITE` this is the overview
/// map's tiles and every detail texture, not sprite frames, and it takes
/// `GL_Upload32` directly -- never replaced, and not counted as a sprite.
const TEX_TYPE_RGBA: u32 = 4;

/// The HD folders, relative to the game directory (`dod/`). Each holds one
/// subfolder per upscale style (`ultrasharp`, `plain`, ...) plus
/// [`OVERRIDES`]. Relative because the detail and sky paths are handed back to
/// the engine's own file loader, which resolves them inside `dod/`.
const WORLD_DIR: &str = "dodstudio_hd/world";
const MODELS_DIR: &str = "dodstudio_hd/models";
const SPRITES_DIR: &str = "dodstudio_hd/sprites";
/// Per-texture picks, in each type's folder, that win over the active style --
/// e.g. one texture a particular model got wrong, taken from another style.
const OVERRIDES: &str = "overrides";

/// `dodstudio_hd_style`: which style subfolder to use. Read once, when the
/// first HD folder is indexed (the first map load), because the engine keeps
/// every texture it has uploaded -- changing it later in a session would only
/// affect textures not loaded yet. Set it in `movie.cfg` or on the launch line.
pub const STYLE_NAME: &str = console_name!("hd_style");
pub const DEFAULT_STYLE: &str = "ultrasharp";
/// Fallback when the cvar could not be registered.
const STYLE_ENV: &str = "GOLDSRC_HOOKS_HD_STYLE";
static STYLE_CVAR: std::sync::atomic::AtomicPtr<crate::engine::CvarSPartial> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
static ACTIVE_STYLE: OnceLock<String> = OnceLock::new();

/// Called by `commands.rs` once `dodstudio_hd_style` is registered.
pub fn set_style_cvar(cvar: *mut crate::engine::CvarSPartial) {
    STYLE_CVAR.store(cvar, Ordering::Release);
}

/// A style name as a folder name: lowercase letters, digits, `-` and `_` only,
/// so a cvar value can never climb out of `dodstudio_hd` (`..`, `/`, `:`).
fn clean_style(raw: &str) -> Option<String> {
    let s = raw.trim().to_ascii_lowercase();
    (!s.is_empty()
        && s.len() <= 32
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
    .then_some(s)
}

/// The style in force this session, fixed the first time it's asked for.
fn active_style() -> &'static str {
    ACTIVE_STYLE.get_or_init(|| {
        let cvar = STYLE_CVAR.load(Ordering::Acquire);
        // Safety: a cvar_t the engine registered for us and keeps for the
        // session; its `string` is always a valid C string.
        let from_cvar = (!cvar.is_null())
            .then(|| unsafe { (*cvar).string })
            .filter(|p| !p.is_null())
            .map(|p| {
                unsafe { std::ffi::CStr::from_ptr(p) }
                    .to_string_lossy()
                    .into_owned()
            });
        let chosen = from_cvar
            .or_else(|| std::env::var(STYLE_ENV).ok())
            .and_then(|s| clean_style(&s))
            .unwrap_or_else(|| DEFAULT_STYLE.to_string());
        unsafe { crate::debug::report(&format!("texture_hires: HD style {chosen:?}")) };
        chosen
    })
}

/// `<type>/<active style>` and `<type>/overrides`, in that order -- later wins.
fn style_dirs(type_dir: &str) -> [String; 2] {
    [
        format!("{type_dir}/{}", active_style()),
        format!("{type_dir}/{OVERRIDES}"),
    ]
}

/// World textures seen, and how many were replaced.
static WORLD_SEEN: AtomicU32 = AtomicU32::new(0);
static REPLACED: AtomicU32 = AtomicU32::new(0);
/// Model skins seen, and how many were replaced.
static MODEL_SEEN: AtomicU32 = AtomicU32::new(0);
static MODEL_REPLACED: AtomicU32 = AtomicU32::new(0);
/// Sprite frames seen, and how many were replaced.
static SPRITE_SEEN: AtomicU32 = AtomicU32::new(0);
static SPRITE_REPLACED: AtomicU32 = AtomicU32::new(0);
/// Textures of any type seen -- what `has_observed` keys on.
static ANY_SEEN: AtomicU32 = AtomicU32::new(0);

/// The most recent replacement, for `status()`: name, original size, new size.
struct LastReplaced {
    name: String,
    from: (u32, u32),
    to: (u32, u32),
}
static LAST_REPLACED: Mutex<Option<LastReplaced>> = Mutex::new(None);

/// `dodstudio_hd_misses`: lists every texture that kept its original this
/// session, and why. Registered in `commands.rs`.
pub const MISSES_NAME: &str = console_name!("hd_misses");
pub const MISSES_COMMAND_NAMES: &[&str] = &[MISSES_NAME];

/// Why a texture kept its original. Declared in the order they're listed,
/// most worth a look first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Miss {
    /// An HD file has this name, but was made from different pixels -- a
    /// swapped or updated original, or a different map's same-named texture.
    WrongVersion,
    /// No HD file has this name.
    NoFile,
    /// An HD file matched but couldn't be used.
    Failed,
    /// Left alone by design: tool textures, blank sprite frames, formats the
    /// hook doesn't replace.
    OnPurpose,
}

impl Miss {
    fn heading(self) -> &'static str {
        match self {
            Miss::WrongVersion => "HD file is for a different version of the texture",
            Miss::NoFile => "no HD file",
            Miss::Failed => "HD file found but not usable",
            Miss::OnPurpose => "left alone on purpose",
        }
    }
}

/// One line of the miss list: what to show after the name, and how many
/// loads hit it (a sprite's frames count separately).
struct MissEntry {
    detail: String,
    loads: u32,
}

/// Keyed by (why, asset type, name), so the list prints grouped and sorted,
/// and a texture loaded again only bumps its count. Written at map load only.
static MISSES: Mutex<std::collections::BTreeMap<(Miss, &'static str, String), MissEntry>> =
    Mutex::new(std::collections::BTreeMap::new());
/// Bounds the list; misses past it are only counted.
const MAX_MISSES: usize = 2000;
static MISSES_DROPPED: AtomicU32 = AtomicU32::new(0);

fn record_miss(why: Miss, kind: &'static str, name: &str, detail: impl FnOnce() -> String) {
    let Ok(mut misses) = MISSES.lock() else {
        return;
    };
    let key = (why, kind, name.to_string());
    if let Some(entry) = misses.get_mut(&key) {
        entry.loads += 1;
    } else if misses.len() < MAX_MISSES {
        misses.insert(
            key,
            MissEntry {
                detail: detail(),
                loads: 1,
            },
        );
    } else {
        MISSES_DROPPED.fetch_add(1, Ordering::Relaxed);
    }
}

/// World texture names the pipeline never builds: tool brushes the map
/// compiler removes or the renderer never draws, and the sky, which is drawn
/// from `gfx/env` instead. Lowercase; matches `SKIP` in `pipeline_hd.py`.
const TOOL_TEXTURES: &[&str] = &[
    "aaatrigger",
    "clip",
    "origin",
    "null",
    "skip",
    "hint",
    "bevel",
    "sky",
    "black",
];

/// How a load is shown in the miss list: a model skin as `<model> <skin>`,
/// a sprite frame as its sprite (its frames are counted, not listed).
fn miss_display_name(texture_type: u32, identifier: &str) -> String {
    let lower = identifier.to_ascii_lowercase();
    match texture_type {
        GLT_STUDIO => match lower.rfind(".mdl") {
            Some(at) if at + 4 < identifier.len() => {
                format!("{} {}", &identifier[..at + 4], &identifier[at + 4..])
            }
            _ => identifier.to_string(),
        },
        GLT_SPRITE => match lower.rfind(".spr_") {
            Some(at) => identifier[..at + 4].to_string(),
            None => identifier.to_string(),
        },
        _ => identifier.to_string(),
    }
}

/// The miss list, as console lines.
fn misses_report() -> Vec<String> {
    let Ok(misses) = MISSES.lock() else {
        return vec![format!("{MISSES_NAME}: the list's lock is poisoned\n")];
    };
    let style = ACTIVE_STYLE
        .get()
        .map(String::as_str)
        .unwrap_or("not chosen yet");
    if misses.is_empty() {
        return vec![format!(
            "{MISSES_NAME}: every HD-eligible texture loaded so far was replaced (style {style:?})\n"
        )];
    }
    let mut lines = vec![format!(
        "{MISSES_NAME}: {} texture(s) kept their original this session (style {style:?})\n",
        misses.len()
    )];
    let mut current = None;
    for ((why, kind, name), entry) in misses.iter() {
        if current != Some(*why) {
            current = Some(*why);
            let n = misses.keys().filter(|(w, _, _)| w == why).count();
            lines.push(format!("-- {} ({n}):\n", why.heading()));
        }
        let loads = match (*kind, entry.loads) {
            (_, 1) => String::new(),
            ("sprite", n) => format!(" ({n} frame loads)"),
            (_, n) => format!(" ({n} loads)"),
        };
        lines.push(format!("  {kind:<6} {name}  {}{loads}\n", entry.detail));
    }
    let dropped = MISSES_DROPPED.load(Ordering::Relaxed);
    if dropped > 0 {
        lines.push(format!("  ...and {dropped} more not listed\n"));
    }
    lines
}

/// `dodstudio_hd_misses` prints the list; `dodstudio_hd_misses clear` empties
/// it, e.g. before loading the next map.
pub unsafe extern "C" fn misses_command() {
    let clear = misses_args()
        .get(1)
        .is_some_and(|a| a.eq_ignore_ascii_case("clear"));
    if clear {
        if let Ok(mut misses) = MISSES.lock() {
            misses.clear();
        }
        MISSES_DROPPED.store(0, Ordering::Relaxed);
        crate::commands::console_print(&format!("{MISSES_NAME}: list cleared\n"));
        return;
    }
    let lines = misses_report();
    // One call per line: the engine's console print has a fixed-size buffer.
    for line in &lines {
        crate::commands::console_print(line);
    }
    unsafe {
        crate::debug::report(&format!(
            "texture_hires: {MISSES_NAME} --\n{}",
            lines.concat()
        ))
    };
}

fn misses_args() -> Vec<String> {
    let Some(engfuncs) = crate::engine::engfuncs() else {
        return Vec::new();
    };
    let argc = unsafe { (engfuncs.cmd_argc)() };
    (0..argc)
        .filter_map(|i| {
            let ptr = unsafe { (engfuncs.cmd_argv)(i) };
            if ptr.is_null() {
                return None;
            }
            Some(
                unsafe { std::ffi::CStr::from_ptr(ptr) }
                    .to_string_lossy()
                    .into_owned(),
            )
        })
        .collect()
}

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
/// `redirect_detail_path`'s address, and where its stub returns to.
static DETAIL_PATH_FN: AtomicUsize = AtomicUsize::new(0);
static DETAIL_RESUME: AtomicUsize = AtomicUsize::new(0);
/// `sky_face`'s address, where its stub returns to, and the two `push 0x100`
/// immediates it rewrites per face.
static SKY_FN: AtomicUsize = AtomicUsize::new(0);
static SKY_RESUME: AtomicUsize = AtomicUsize::new(0);
static SKY_WIDTH_IMM: AtomicUsize = AtomicUsize::new(0);
static SKY_HEIGHT_IMM: AtomicUsize = AtomicUsize::new(0);
/// What those immediates hold right now, so a stock face after an HD one
/// puts them back.
static SKY_CURRENT_DIMS: Mutex<(u32, u32)> = Mutex::new((SKY_STOCK_SIDE, SKY_STOCK_SIDE));
/// Sky faces replaced this session, and the folder's index.
static SKY_REPLACED: AtomicU32 = AtomicU32::new(0);
static SKY_INDEX: OnceLock<HashMap<String, String>> = OnceLock::new();

/// `{ data, width, height }`, read by the swap stub as `[eax]`, `[eax+4]`,
/// `[eax+8]` -- three `usize`s are three contiguous dwords on this 32-bit
/// target.
static PENDING: [AtomicUsize; 3] = [const { AtomicUsize::new(0) }; 3];
/// Owns the pixels `PENDING` points at. Replaced on the next swap, by which
/// time `GL_Upload32` has long since copied them into its own buffer.
static PENDING_PIXELS: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// Installed once per process; see [`detour::Detour`] on why never undone.
static INSTALLED: Mutex<Option<Vec<detour::Detour>>> = Mutex::new(None);

/// The replacement folders' indexes, built on first use: map textures, model
/// skins and sprite frames.
static INDEX: OnceLock<Index> = OnceLock::new();
static MODEL_INDEX: OnceLock<Index> = OnceLock::new();
static SPRITE_INDEX: OnceLock<Index> = OnceLock::new();

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

/// Where HD detail textures live, relative to the game directory: the same
/// relative paths as under `gfx/detail/`, so `gfx/detail/1.tga` is replaced by
/// `dodstudio_hd/detail/<style>/1.tga`. Relative, not absolute, because the rewritten
/// path goes back to the engine's own file loader, which resolves it inside
/// `dod/` exactly as it resolves `gfx/...`.
const DETAIL_DIR: &str = "dodstudio_hd/detail";
/// What the detail loader's path starts with once it has run `gfx/%s.tga`
/// over a `_detail.txt` entry like `detail/1`.
const DETAIL_PREFIX: &str = "gfx/detail/";
/// The detail loader's path buffer (`char path[0x104]` at `ebp-0x10c`).
const DETAIL_PATH_CAP: usize = 0x104;

/// Every HD detail texture for the active style (and overrides), by its
/// lowercased path under `gfx/detail/`, to the path to hand the engine
/// instead. Built on the first detail load of the session.
static DETAIL_INDEX: OnceLock<HashMap<String, String>> = OnceLock::new();
/// Detail textures loaded from [`DETAIL_DIR`] this session.
static DETAIL_REDIRECTED: AtomicU32 = AtomicU32::new(0);

fn game_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_default()
        .join("dod")
}

fn build_detail_index() -> HashMap<String, String> {
    build_folder_index(DETAIL_DIR, "HD detail texture(s)")
}

fn build_sky_index() -> HashMap<String, String> {
    build_folder_index(SKY_DIR, "HD sky face(s)")
}

/// Every file in `<type_dir>/<style>` and `<type_dir>/overrides` (overrides
/// winning), keyed by its lowercased path relative to that folder, mapped to
/// its path relative to the game directory.
fn build_folder_index(type_dir: &str, what: &str) -> HashMap<String, String> {
    fn walk(dir: &Path, rel: &str, out: &mut std::collections::HashSet<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            let rel = format!("{rel}{name}");
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                walk(&entry.path(), &format!("{rel}/"), out);
            } else {
                out.insert(rel);
            }
        }
    }
    let mut out = HashMap::new();
    for sub in style_dirs(type_dir) {
        let mut found = std::collections::HashSet::new();
        walk(&game_dir().join(&sub), "", &mut found);
        unsafe {
            crate::debug::report(&format!(
                "texture_hires: indexed {} {what} in {sub}",
                found.len()
            ))
        };
        for rel in found {
            let engine_path = format!("{sub}/{rel}");
            out.insert(rel, engine_path);
        }
    }
    out
}

/// What `R_LoadSkys` does to a face after `LoadTGA`, applied to a replacement:
/// with `gl_dither` on, every channel below 0xfc becomes
/// `texgamma[c | c >> 6]`; with it off, nothing. Alpha is forced opaque.
fn match_sky_expansion(pixels: &mut [u8], gamma: &[u8; 256], dither: bool) {
    for px in pixels.chunks_exact_mut(4) {
        if dither {
            for c in &mut px[..3] {
                if *c < 0xfc {
                    *c = gamma[(*c | (*c >> 6)) as usize];
                }
            }
        }
        px[3] = 255;
    }
}

/// Points the `glTexImage2D` that follows at `w` x `h`, if it isn't already.
fn set_sky_dims(w: u32, h: u32) -> bool {
    let Ok(mut current) = SKY_CURRENT_DIMS.lock() else {
        return false;
    };
    if *current == (w, h) {
        return true;
    }
    let (wi, hi) = (
        SKY_WIDTH_IMM.load(Ordering::Acquire),
        SKY_HEIGHT_IMM.load(Ordering::Acquire),
    );
    // Safety: both are the operands of `push 0x100` in R_LoadSkys, verified
    // at install; the function is only ever run on the game thread, which is
    // the thread calling this, before it reaches either instruction.
    let ok = unsafe {
        crate::patch::write_code_bytes(wi, &w.to_le_bytes())
            && crate::patch::write_code_bytes(hi, &h.to_le_bytes())
    };
    if ok {
        *current = (w, h);
    }
    ok
}

/// Called by the sky stub once `R_LoadSkys` has a face loaded and
/// gamma-corrected in `buffer`, just before it binds and uploads it. When
/// `dodstudio_hd/sky` has the same face, overwrites `buffer` with it and
/// points the upload at its size; otherwise makes sure the upload is back at
/// the stock 256x256.
///
/// # Safety
///
/// `frame` is `R_LoadSkys`'s `ebp` and `buffer` its face buffer, which
/// [`install_sky`] enlarged to [`SKY_MAX_BYTES`] before this could run.
unsafe extern "C" fn sky_face(frame: *const u8, buffer: *mut u8) {
    let stock = || {
        set_sky_dims(SKY_STOCK_SIDE, SKY_STOCK_SIDE);
    };
    let gamma_at = GAMMA_TABLE.load(Ordering::Acquire);
    let dither_at = DITHER_VALUE.load(Ordering::Acquire);
    if frame.is_null() || buffer.is_null() || gamma_at == 0 || dither_at == 0 {
        return stock();
    }
    let path = unsafe { std::slice::from_raw_parts(frame.sub(SKY_PATH_OFFSET), SKY_PATH_CAP) };
    let Some(len) = path.iter().position(|&b| b == 0) else {
        return stock();
    };
    let original = String::from_utf8_lossy(&path[..len])
        .replace('\\', "/")
        .to_ascii_lowercase();
    let Some(rest) = original.strip_prefix(SKY_PREFIX) else {
        return stock();
    };
    let Some(engine_path) = SKY_INDEX.get_or_init(build_sky_index).get(rest) else {
        record_miss(Miss::NoFile, "sky", &original, || "no HD face".to_string());
        return stock();
    };
    let file = game_dir().join(engine_path);
    let decoded = std::fs::read(&file)
        .map_err(|e| e.to_string())
        .and_then(|b| decode_tga(&b));
    let mut img = match decoded {
        Ok(img) => img,
        Err(why) => {
            unsafe {
                crate::debug::report(&format!(
                    "texture_hires: could not use {}: {why}",
                    file.display()
                ))
            };
            record_miss(Miss::Failed, "sky", &original, || {
                format!("{}: {why}", file.display())
            });
            return stock();
        }
    };
    while img.width > SKY_MAX_SIDE || img.height > SKY_MAX_SIDE {
        img = halve(&img);
    }
    // Safety: addresses read out of GL_Upload8 at install; R_LoadSkys uses
    // the same texgamma table and gl_dither cvar for its own faces.
    let gamma = unsafe { &*(gamma_at as *const [u8; 256]) };
    let dither = unsafe { *(dither_at as *const f32) } != 0.0;
    match_sky_expansion(&mut img.pixels, gamma, dither);
    if !set_sky_dims(img.width, img.height) {
        return stock();
    }
    // Safety: at most SKY_MAX_BYTES, the size install_sky made the buffer.
    unsafe { std::ptr::copy_nonoverlapping(img.pixels.as_ptr(), buffer, img.pixels.len()) };
    SKY_REPLACED.fetch_add(1, Ordering::Relaxed);
    if LOG_TEXTURE_LOADS.load(Ordering::Relaxed) {
        unsafe {
            crate::debug::report(&format!(
                "texture_hires: sky {original} -> {}x{} from {}",
                img.width,
                img.height,
                file.display()
            ))
        };
    }
}

/// The sky stub, replacing `mov eax, [ebx*4 + skytexturenums]`.
///
/// ```asm
///     push esi                      ; the face buffer
///     push ebp
///     call [SKY_FN]                 ; sky_face(ebp, buffer)
///     add esp, 8
///     mov eax, [ebx*4 + ...]        ; the stolen instruction, copied verbatim
///     jmp [SKY_RESUME]
/// ```
///
/// The stolen instruction is position-independent (an absolute address), so
/// it is copied from the game's own bytes rather than rebuilt. `ecx`/`edx`
/// are reloaded by the code the stub returns to before any use.
fn sky_stub(stolen: &[u8]) -> Vec<u8> {
    let mut code = vec![0x56, 0x55]; // push esi; push ebp
    indirect(&mut code, CALL, &SKY_FN);
    code.extend_from_slice(&[0x83, 0xC4, 0x08]); // add esp, 8
    code.extend_from_slice(stolen);
    indirect(&mut code, JMP, &SKY_RESUME);
    code
}

/// Lets `R_LoadSkys` use HD faces from [`SKY_DIR`]: enlarges its face buffer
/// to 1024x1024, then hooks the moment each face is ready to upload. Faces
/// without an HD copy upload exactly as before.
fn install_sky(base: usize) -> Result<detour::Detour, String> {
    // Safety: `base` is hw.dll's module handle, mapped for the session.
    let loader = unsafe { scan::find_unique(base, SKY_LOADER) }
        .map_err(|why| format!("could not locate R_LoadSkys -- {why}"))?;
    let upload = unsafe { scan::find_unique(base, SKY_UPLOAD) }
        .map_err(|why| format!("could not locate R_LoadSkys's upload -- {why}"))?;
    check_span(loader + SKY_MALLOC_AT, SKY_STOCK_PUSH)?;
    check_span(upload + sky::HEIGHT_PUSH, SKY_DIM_PUSH)?;
    check_span(upload + sky::WIDTH_PUSH, SKY_DIM_PUSH)?;
    let hook_at = upload + sky::HOOK_AT;
    check_span(hook_at, &[0x8B, 0x04, 0x9D])?;
    // Safety: inside the matched span.
    let stolen =
        unsafe { std::slice::from_raw_parts(hook_at as *const u8, sky::HOOK_LEN) }.to_vec();

    // Buffer first: nothing may write a big face before the buffer is big.
    // Safety: verified above to be `push 0x40000` before R_LoadSkys' malloc.
    if !unsafe {
        crate::patch::write_code_bytes(
            loader + SKY_MALLOC_AT + 1,
            &(SKY_MAX_BYTES as u32).to_le_bytes(),
        )
    } {
        return Err("could not enlarge the sky face buffer".into());
    }
    SKY_WIDTH_IMM.store(upload + sky::WIDTH_PUSH + 1, Ordering::Release);
    SKY_HEIGHT_IMM.store(upload + sky::HEIGHT_PUSH + 1, Ordering::Release);
    SKY_FN.store(sky_face as *const () as usize, Ordering::Release);
    SKY_RESUME.store(hook_at + sky::HOOK_LEN, Ordering::Release);
    // Safety: span verified above; the only branch into it (the gl_dither
    // skip) targets its first byte.
    unsafe { detour::install(hook_at, sky::HOOK_LEN, &sky_stub(&stolen)) }
}

/// The path to hand the engine instead of `path`, if the active style (or
/// overrides) has a replacement for it. `None` leaves it alone.
fn detail_override(path: &str, index: &HashMap<String, String>) -> Option<String> {
    let norm = path.replace('\\', "/").to_ascii_lowercase();
    let rest = norm.strip_prefix(DETAIL_PREFIX)?;
    let redirected = index.get(rest)?;
    (redirected.len() < DETAIL_PATH_CAP).then(|| redirected.clone())
}

/// Whether the TGA at `file` fits in `max_bytes` of RGBA. A replacement too
/// big for the loader's buffer would fail to load and take the detail layer
/// with it; the original is better than nothing.
fn detail_fits(file: &Path, max_bytes: usize) -> bool {
    use std::io::Read;
    let mut header = [0u8; 18];
    let read = std::fs::File::open(file).and_then(|mut f| f.read_exact(&mut header));
    if read.is_err() {
        return false;
    }
    let w = u16::from_le_bytes([header[12], header[13]]) as usize;
    let h = u16::from_le_bytes([header[14], header[15]]) as usize;
    w * h * 4 <= max_bytes
}

/// Called by the detail-path stub with the detail loader's path buffer, just
/// before it is handed to `LoadTGA`. Rewrites it in place to the HD copy when
/// there is one.
///
/// # Safety
///
/// `path` is the loader's own `char[0x104]`, NUL-terminated by its `snprintf`.
unsafe extern "C" fn redirect_detail_path(path: *mut u8) {
    if path.is_null() {
        return;
    }
    let buf = unsafe { std::slice::from_raw_parts_mut(path, DETAIL_PATH_CAP) };
    let Some(len) = buf.iter().position(|&b| b == 0) else {
        return;
    };
    let original = String::from_utf8_lossy(&buf[..len]).into_owned();
    let index = DETAIL_INDEX.get_or_init(build_detail_index);
    let max = if DETAIL_RAISED.load(Ordering::Acquire) {
        DETAIL_MAX_BYTES
    } else {
        DETAIL_STOCK_BYTES
    };
    let Some(new) = detail_override(&original, index) else {
        record_miss(Miss::NoFile, "detail", &original, || {
            "no HD copy".to_string()
        });
        return;
    };
    if !detail_fits(&game_dir().join(&new), max) {
        record_miss(Miss::Failed, "detail", &original, || {
            format!("{new} is over the loader's {max}-byte limit")
        });
        if LOG_TEXTURE_LOADS.load(Ordering::Relaxed) {
            unsafe {
                crate::debug::report(&format!(
                    "texture_hires: detail {original}: HD copy is over the {max}-byte limit, using the original"
                ))
            };
        }
        return;
    }
    buf[..new.len()].copy_from_slice(new.as_bytes());
    buf[new.len()] = 0;
    DETAIL_REDIRECTED.fetch_add(1, Ordering::Relaxed);
    if LOG_TEXTURE_LOADS.load(Ordering::Relaxed) {
        unsafe { crate::debug::report(&format!("texture_hires: detail {original} -> {new}")) };
    }
}

/// The detail-path stub, replacing `lea eax, [ebp-0x10c]` right before
/// `LoadTGA`'s last two arguments are pushed.
///
/// ```asm
///     lea eax, [ebp-0x10c]
///     push eax
///     call [DETAIL_PATH_FN]         ; redirect_detail_path(path)
///     add esp, 4
///     lea eax, [ebp-0x10c]          ; the stolen instruction
///     jmp [DETAIL_RESUME]
/// ```
///
/// `ecx`/`edx` are dead here (their values were already pushed), and `eax` is
/// reloaded by the stolen `lea` last.
fn detail_path_stub() -> Vec<u8> {
    let mut code = DETAIL_PATH_STOLEN.to_vec();
    code.push(0x50); // push eax
    indirect(&mut code, CALL, &DETAIL_PATH_FN);
    code.extend_from_slice(&[0x83, 0xC4, 0x04]); // add esp, 4
    code.extend_from_slice(DETAIL_PATH_STOLEN);
    indirect(&mut code, JMP, &DETAIL_RESUME);
    code
}

/// The part of a model skin's identifier that names the texture itself.
/// `Mod_LoadStudioModel` builds the identifier as `"%s%s"` of the model's path
/// and the texture's name (`models/v_garand.mdlgarand.bmp`); keying files on
/// the texture name alone keeps them readable, and the content hash already
/// keeps same-named skins from different models apart.
fn studio_texture_name(identifier: &str) -> &str {
    let lower = identifier.to_ascii_lowercase();
    match lower.rfind(".mdl") {
        Some(at) if at + 4 < identifier.len() => &identifier[at + 4..],
        _ => identifier,
    }
}

/// A sprite frame's identifier as it appears in a replacement's filename.
/// `Mod_LoadSpriteFrame` names each frame `"%s_%i"` of the sprite's model
/// path and the frame number (`sprites/muzzleflash1.spr_0`; frame `j` of
/// group `i` is `i * 100 + j`); files use the sprite's file name without the
/// folder or `.spr` (`muzzleflash1_0`), the hash keeping same-named sprites
/// from different folders apart.
fn sprite_frame_name(identifier: &str) -> String {
    let base = identifier.rsplit(['/', '\\']).next().unwrap_or(identifier);
    let lower = base.to_ascii_lowercase();
    match lower.rfind(".spr_") {
        Some(at) => format!("{}_{}", &lower[..at], &lower[at + 5..]),
        None => lower,
    }
}

fn build_index() -> Index {
    build_index_in(WORLD_DIR)
}

fn build_model_index() -> Index {
    build_index_in(MODELS_DIR)
}

fn build_sprite_index() -> Index {
    build_index_in(SPRITES_DIR)
}

/// Every `<name>_<hash>.tga` in `<type_dir>/<style>` and then
/// `<type_dir>/overrides`, so an override replaces the style's file.
fn build_index_in(type_dir: &str) -> Index {
    let mut files = HashMap::new();
    let dirs = style_dirs(type_dir);
    for sub in &dirs {
        let dir = game_dir().join(sub);
        let mut found = 0;
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(key) = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .and_then(parse_file_name)
                {
                    files.insert(key, path);
                    found += 1;
                }
            }
        }
        unsafe {
            crate::debug::report(&format!(
                "texture_hires: indexed {found} replacement(s) in {sub}"
            ))
        };
    }
    Index {
        dir: game_dir().join(&dirs[0]),
        files,
    }
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
/// all-zero transparent pixel for masked ones. For index-alpha sprites every
/// pixel becomes `tint` (the original's palette entry 255) through the gamma
/// table, keeping the replacement's alpha.
fn match_engine_expansion(
    pixels: &mut [u8],
    gamma: &[u8; 256],
    i_type: u32,
    dither: bool,
    tint: [u8; 3],
) {
    if i_type == TEX_TYPE_ALPHA_GRADIENT {
        let rgb = tint.map(|c| gamma[c as usize]);
        for px in pixels.chunks_exact_mut(4) {
            px[..3].copy_from_slice(&rgb);
        }
        return;
    }
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
fn load_replacement(path: &Path, i_type: u32, tint: [u8; 3]) -> Result<Rgba, String> {
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
    match_engine_expansion(&mut img.pixels, gamma, i_type, dither, tint);
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
    let (seen, replaced, kind, index, build): (_, _, _, _, fn() -> Index) = match texture_type {
        GLT_WORLD => (&WORLD_SEEN, &REPLACED, "world", &INDEX, build_index),
        GLT_STUDIO => (
            &MODEL_SEEN,
            &MODEL_REPLACED,
            "model",
            &MODEL_INDEX,
            build_model_index,
        ),
        GLT_SPRITE if i_type != TEX_TYPE_RGBA => (
            &SPRITE_SEEN,
            &SPRITE_REPLACED,
            "sprite",
            &SPRITE_INDEX,
            build_sprite_index,
        ),
        _ => return std::ptr::null(),
    };
    if name_ptr.is_null() {
        return std::ptr::null();
    }
    seen.fetch_add(1, Ordering::Relaxed);

    // Safety: the identifier every caller passes is a NUL-terminated name.
    let identifier = unsafe { std::ffi::CStr::from_ptr(name_ptr) }.to_string_lossy();
    let name = match texture_type {
        GLT_STUDIO => studio_texture_name(&identifier).to_string(),
        GLT_SPRITE => sprite_frame_name(&identifier),
        _ => identifier.to_string(),
    };
    let log = LOG_TEXTURE_LOADS.load(Ordering::Relaxed);
    let skip = |miss: Miss, why: &str| {
        if log {
            unsafe {
                crate::debug::report(&format!(
                    "texture_hires: {kind} {identifier:?} {width}x{height} iType {i_type}: {why}"
                ))
            };
        }
        record_miss(
            miss,
            kind,
            &miss_display_name(texture_type, &identifier),
            || format!("{width}x{height}, {why}"),
        );
        std::ptr::null()
    };

    // Sprites already in RGBA (iType 4) take GL_Upload32 directly and are
    // never replaced; only sprites use the alpha gradient.
    let known_type = matches!(i_type, TEX_TYPE_NONE | TEX_TYPE_ALPHA)
        || (texture_type == GLT_SPRITE && i_type == TEX_TYPE_ALPHA_GRADIENT);
    if !known_type {
        return skip(
            Miss::OnPurpose,
            &format!("iType {i_type}, a format this hook doesn't replace"),
        );
    }
    if data.is_null()
        || palette.is_null()
        || width == 0
        || height == 0
        || width > 4096
        || height > 4096
    {
        return skip(Miss::OnPurpose, "no palette data");
    }

    // Safety: GL_Upload8 is about to read exactly these spans itself.
    let (indices, pal) = unsafe {
        (
            std::slice::from_raw_parts(data, (width * height) as usize),
            std::slice::from_raw_parts(palette, 768),
        )
    };
    // Before the file lookup: a blanked-out copy of a sprite whose real
    // frames have HD files would otherwise show up as the wrong version.
    if texture_type == GLT_SPRITE && i_type == TEX_TYPE_ALPHA && indices.iter().all(|&i| i == 255) {
        return skip(
            Miss::OnPurpose,
            "blank (fully transparent), nothing to upscale",
        );
    }

    let index = index.get_or_init(build);
    let stem = file_stem_name(&name);
    // The name check first: the hash is only worth computing for a texture
    // that has any replacement at all.
    if !index.files.keys().any(|(n, _)| *n == stem) {
        if texture_type == GLT_WORLD && TOOL_TEXTURES.contains(&stem.as_str()) {
            return skip(Miss::OnPurpose, "tool texture, never built");
        }
        let hash = fnv1a32(&[indices, pal]);
        return skip(
            Miss::NoFile,
            &format!("no replacement (would be {stem}_{hash:08x}.tga)"),
        );
    }

    let hash = fnv1a32(&[indices, pal]);
    let Some(path) = index.files.get(&(stem.clone(), hash)) else {
        return skip(
            Miss::WrongVersion,
            &format!(
                "replacement(s) named {stem:?} exist, none for this content (needs {stem}_{hash:08x}.tga)"
            ),
        );
    };

    let tint = [pal[765], pal[766], pal[767]];
    let img = match load_replacement(path, i_type, tint) {
        Ok(img) => img,
        Err(why) => {
            unsafe {
                crate::debug::report(&format!(
                    "texture_hires: could not use {}: {why}",
                    path.display()
                ))
            };
            record_miss(
                Miss::Failed,
                kind,
                &miss_display_name(texture_type, &identifier),
                || format!("{width}x{height}, {}: {why}", path.display()),
            );
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

    replaced.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut last) = LAST_REPLACED.lock() {
        *last = Some(LastReplaced {
            name,
            from: (width, height),
            to: (img.width, img.height),
        });
    }
    if log {
        unsafe {
            crate::debug::report(&format!(
                "texture_hires: {kind} {identifier:?} {width}x{height} -> {}x{} from {}",
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
fn raise_detail_limit(base: usize, loader: usize) -> Result<(), String> {
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

/// Points the detail loader at [`DETAIL_DIR`] for any detail texture that has
/// an HD copy there, so `gfx/detail/` itself never has to be edited.
fn install_detail_redirect(loader: usize) -> Result<detour::Detour, String> {
    let at = loader + DETAIL_PATH_AT;
    check_span(at, DETAIL_PATH_STOLEN)?;
    DETAIL_PATH_FN.store(
        redirect_detail_path as *const () as usize,
        Ordering::Release,
    );
    DETAIL_RESUME.store(at + DETAIL_PATH_STOLEN.len(), Ordering::Release);
    // Safety: span verified above; nothing in the loader branches into it.
    unsafe { detour::install(at, DETAIL_PATH_STOLEN.len(), &detail_path_stub()) }
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
    // Safety: `base` is hw.dll's module handle, mapped for the session.
    match unsafe { scan::find_unique(base, DETAIL_LOADER) } {
        Ok(loader) => {
            // Only with the upload ceiling raised: a 1024x1024 detail texture
            // would otherwise get past LoadTGA and hit GL_Upload32's stock
            // Sys_Error.
            if CEILING_RAISED.load(Ordering::Acquire)
                && let Err(why) = raise_detail_limit(base, loader)
            {
                unsafe {
                    crate::debug::report(&format!(
                        "texture_hires: detail textures left at 512x512 -- {why}"
                    ))
                };
            }
            match install_detail_redirect(loader) {
                Ok(d) => detours.push(d),
                Err(why) => unsafe {
                    crate::debug::report(&format!("texture_hires: {DETAIL_DIR} not used -- {why}"))
                },
            }
        }
        Err(why) => unsafe {
            crate::debug::report(&format!(
                "texture_hires: could not locate the detail texture loader, detail textures left alone -- {why}"
            ))
        },
    }

    GAMMA_TABLE.store(gamma, Ordering::Release);
    DITHER_VALUE.store(dither, Ordering::Release);
    match install_sky(base) {
        Ok(d) => detours.push(d),
        Err(why) => unsafe {
            crate::debug::report(&format!("texture_hires: {SKY_DIR} not used -- {why}"))
        },
    }
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
            WORLD_DIR
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
    let models = format!(
        "{} of {} model skin load(s) replaced ({})",
        MODEL_REPLACED.load(Ordering::Relaxed),
        MODEL_SEEN.load(Ordering::Relaxed),
        MODEL_INDEX
            .get()
            .map(|i| format!("{} file(s)", i.files.len()))
            .unwrap_or_else(|| "folder not read yet".to_string())
    );
    let sprites = format!(
        "{} of {} sprite frame load(s) replaced ({})",
        SPRITE_REPLACED.load(Ordering::Relaxed),
        SPRITE_SEEN.load(Ordering::Relaxed),
        SPRITE_INDEX
            .get()
            .map(|i| format!("{} file(s)", i.files.len()))
            .unwrap_or_else(|| "folder not read yet".to_string())
    );
    let misses = MISSES
        .lock()
        .map(|m| {
            let on_purpose = m.keys().filter(|(w, _, _)| *w == Miss::OnPurpose).count();
            format!(
                "; {} texture(s) kept their original ({} of them on purpose) -- {MISSES_NAME} lists them",
                m.len(),
                on_purpose
            )
        })
        .unwrap_or_default();
    format!(
        "{NAME}: style {:?}; {replaced} of {world} world texture load(s) replaced{last}; {files}; {models}; {sprites}; {} sky face(s) from {SKY_DIR}; ceiling {}; detail textures {}, {} loaded from {DETAIL_DIR} (logging {}){misses}",
        ACTIVE_STYLE
            .get()
            .map(String::as_str)
            .unwrap_or("not chosen yet"),
        SKY_REPLACED.load(Ordering::Relaxed),
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
        DETAIL_REDIRECTED.load(Ordering::Relaxed),
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
    fn sky_offsets_land_on_what_they_name() {
        scan::Pattern::parse(SKY_LOADER).unwrap();
        scan::Pattern::parse(SKY_UPLOAD).unwrap();
        fixed(SKY_LOADER, SKY_MALLOC_AT, SKY_STOCK_PUSH);
        assert_eq!(tokens(SKY_LOADER).len(), SKY_MALLOC_AT + 5 + 8);
        fixed(SKY_UPLOAD, sky::HOOK_AT, &[0x8B, 0x04, 0x9D]);
        fixed(SKY_UPLOAD, sky::HEIGHT_PUSH, SKY_DIM_PUSH);
        fixed(SKY_UPLOAD, sky::WIDTH_PUSH, SKY_DIM_PUSH);
        assert_eq!(SKY_MAX_BYTES, 1024 * 1024 * 4);
    }

    #[test]
    fn sky_stub_reproduces_the_stolen_instruction() {
        let stolen = [0x8B, 0x04, 0x9D, 0x60, 0x34, 0x34, 0x02];
        let code = sky_stub(&stolen);
        assert_eq!(&code[..2], &[0x56, 0x55]);
        assert_eq!(&code[8..11], &[0x83, 0xC4, 0x08]);
        assert_eq!(&code[11..18], &stolen);
        assert_eq!(&code[18..20], &[0xFF, 0x25]);
        assert_eq!(code.len(), 24);
    }

    #[test]
    fn sky_expansion_matches_the_engine() {
        let mut gamma = [0u8; 256];
        for (i, g) in gamma.iter_mut().enumerate() {
            *g = (255 - i) as u8;
        }
        let mut px = vec![3, 0xfc, 0xff, 7];
        match_sky_expansion(&mut px, &gamma, true);
        // 3 -> gamma[3 | 0] = 252; 0xfc and up untouched; alpha opaque.
        assert_eq!(px, vec![252, 0xfc, 0xff, 255]);
        let mut px = vec![3, 4, 5, 7];
        match_sky_expansion(&mut px, &gamma, false);
        assert_eq!(px, vec![3, 4, 5, 255]);
    }

    #[test]
    fn model_skins_are_keyed_by_texture_name() {
        assert_eq!(
            studio_texture_name("models/v_garand.mdlgarand.bmp"),
            "garand.bmp"
        );
        assert_eq!(
            studio_texture_name("models/player/us-inf/us-inf.MDLHead1.bmp"),
            "Head1.bmp"
        );
        assert_eq!(studio_texture_name("no_model_here"), "no_model_here");
        assert_eq!(studio_texture_name("models/x.mdl"), "models/x.mdl");
    }

    #[test]
    fn style_names_cannot_leave_the_hd_folder() {
        assert_eq!(clean_style(" UltraSharp "), Some("ultrasharp".to_string()));
        assert_eq!(
            clean_style("general-v3_2"),
            Some("general-v3_2".to_string())
        );
        assert_eq!(clean_style(".."), None);
        assert_eq!(clean_style("a/b"), None);
        assert_eq!(clean_style("c:"), None);
        assert_eq!(clean_style(""), None);
    }

    #[test]
    fn detail_paths_redirect_only_when_an_hd_copy_exists() {
        let index: HashMap<String, String> = [
            ("1.tga", "dodstudio_hd/detail/ultrasharp/1.tga"),
            (
                "sub/dt_grass1.tga",
                "dodstudio_hd/detail/overrides/sub/dt_grass1.tga",
            ),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        assert_eq!(
            detail_override("gfx/detail/1.tga", &index),
            Some("dodstudio_hd/detail/ultrasharp/1.tga".to_string())
        );
        assert_eq!(
            detail_override("GFX\\Detail\\Sub/DT_Grass1.TGA", &index),
            Some("dodstudio_hd/detail/overrides/sub/dt_grass1.tga".to_string())
        );
        assert_eq!(detail_override("gfx/detail/2.tga", &index), None);
        assert_eq!(detail_override("gfx/env/sky.tga", &index), None);
    }

    #[test]
    fn detail_path_stub_reproduces_the_stolen_lea() {
        fixed(DETAIL_LOADER, DETAIL_PATH_AT, DETAIL_PATH_STOLEN);
        let code = detail_path_stub();
        assert_eq!(&code[..6], DETAIL_PATH_STOLEN);
        assert_eq!(code[6], 0x50);
        assert_eq!(&code[16..22], DETAIL_PATH_STOLEN);
        assert_eq!(&code[22..24], &[0xFF, 0x25]);
        assert_eq!(code.len(), 28);
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
        match_engine_expansion(&mut px, &gamma, TEX_TYPE_NONE, true, [0; 3]);
        assert_eq!(px, vec![255, 252 | 3, 0, 255]);
        // Masked: below half alpha becomes all-zero, above becomes opaque.
        let mut px = vec![10, 10, 10, 100, 10, 10, 10, 200];
        match_engine_expansion(&mut px, &gamma, TEX_TYPE_ALPHA, true, [0; 3]);
        assert_eq!(px, vec![0, 0, 0, 0, 245, 245, 245, 255]);
        // Index alpha: the palette-255 tint through gamma, alpha kept as is,
        // whatever RGB the file had; no dither.
        let mut px = vec![9, 9, 9, 0, 1, 2, 3, 77];
        match_engine_expansion(&mut px, &gamma, TEX_TYPE_ALPHA_GRADIENT, true, [0, 3, 255]);
        assert_eq!(px, vec![255, 252, 0, 0, 255, 252, 0, 77]);
    }

    #[test]
    fn misses_show_readable_names() {
        assert_eq!(
            miss_display_name(GLT_STUDIO, "models/v_garand.mdlgarand.bmp"),
            "models/v_garand.mdl garand.bmp"
        );
        assert_eq!(
            miss_display_name(GLT_SPRITE, "sprites/shot_smoke1.spr_203"),
            "sprites/shot_smoke1.spr"
        );
        assert_eq!(miss_display_name(GLT_WORLD, "{Fence7"), "{Fence7");
    }

    #[test]
    fn misses_list_groups_worst_first_and_counts_repeats() {
        // The list is process-wide; this test is the only one that writes it.
        record_miss(Miss::OnPurpose, "world", "CLIP", || "tool".into());
        record_miss(Miss::WrongVersion, "model", "m garand.bmp", || "x".into());
        record_miss(Miss::NoFile, "sprite", "sprites/a.spr", || "y".into());
        record_miss(Miss::NoFile, "sprite", "sprites/a.spr", || {
            unreachable!("a repeat keeps the first detail")
        });
        let report = misses_report().concat();
        let at = |s: &str| {
            report
                .find(s)
                .unwrap_or_else(|| panic!("{s:?} in {report}"))
        };
        assert!(at(Miss::WrongVersion.heading()) < at(Miss::NoFile.heading()));
        assert!(at(Miss::NoFile.heading()) < at(Miss::OnPurpose.heading()));
        assert!(report.contains("sprites/a.spr  y (2 frame loads)"));
        assert!(report.contains("3 texture(s) kept their original"));
    }

    #[test]
    fn sprite_frames_are_keyed_by_file_name_and_frame() {
        assert_eq!(
            sprite_frame_name("sprites/muzzleflash1.spr_0"),
            "muzzleflash1_0"
        );
        assert_eq!(
            sprite_frame_name("sprites/effects/Debris_Dirt3.SPR_203"),
            "debris_dirt3_203"
        );
        assert_eq!(sprite_frame_name("sprites\\x.spr_1"), "x_1");
        assert_eq!(sprite_frame_name("odd_name"), "odd_name");
        // What the Python pipeline writes for the same frame parses back to it.
        assert_eq!(
            parse_file_name("muzzleflash1_0_1a2b3c4d.tga"),
            Some(("muzzleflash1_0".to_string(), 0x1a2b_3c4d))
        );
    }

    #[test]
    fn status_reports_before_any_load() {
        if !has_observed() {
            assert!(status().contains("0 of 0"));
        }
    }
}
