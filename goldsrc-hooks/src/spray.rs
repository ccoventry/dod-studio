//! Player sprays in HD, and switching your own (#428).
//!
//! ## How a spray reaches the GPU
//!
//! A spray is a one-lump WAD3 (`tempdecal.wad`; other players' arrive as
//! `custom.hpk` entries). `Draw_DecalTexture(~slot)` loads it through
//! `Draw_CacheReload` -> `Draw_CacheLoadFromCustom` -> `Draw_ValidateCustomLogo`
//! (each side 1..256 px, four consistent mips, at most 256 colours) ->
//! `Draw_MiptexTexture` -> `GL_LoadTexture(name, GLT_DECAL, w, h, mip0, 1,
//! iType, palette)`, the same `GL_LoadTexture2` branch `texture_hires` hooks
//! for every other kind. Two things set a spray apart from a `decals.wad`
//! decal there, and neither is its content:
//!
//! - **The name is the slot, not the player.** `Draw_CacheLoadFromCustom`
//!   sets a global `gfCustomBuild` and builds the texture name from the cache
//!   entry's name, `"%03i%02i"` of the player slot and lump index; then
//!   `Draw_MiptexTexture` rewrites the first character to `{` (palette entry
//!   255 is pure blue, a cut-out) or `}` (index-alpha). So player 1's spray
//!   is `{00100` in every demo, whoever he is. A replacement keyed by name
//!   would follow the slot. This module keys by **content** instead.
//! - **`gfCustomBuild` is 1 for exactly that upload** (pre-Anniversary
//!   `hw+0x1ff5718`, 25th Anniversary `hw+0x104b77d8`, read out of the one
//!   `mov [flag], 1` in `Draw_CacheLoadFromCustom` by signature), which is
//!   how [`is_custom_upload`] tells a spray from a wad decal; the name shape
//!   is the fallback when the signature isn't found.
//!
//! The RVAs and the reading behind them are in `local/r-and-d/428/notes.md`
//! (a working file, not in the tree); the summary is in
//! `docs/dodstudio_commands.md`.
//!
//! ## The content key
//!
//! `texture_t` overlaps the lump header the engine loaded (the miptex copy
//! sits 0x20 bytes into the texture record, and the record's own fields are
//! written over it), so the lump's name is gone by upload time and the whole
//! lump cannot be hashed as stored. What survives, and what the engine
//! draws, is hashed instead: **MD5 of `width` (LE32), `height` (LE32), the
//! mip-0 indices, the palette count (LE16) and the palette (3 bytes a
//! colour)**, the values `GL_LoadTexture2` is handed plus the count just
//! before the palette. The same bytes can be read from any spray WAD
//! (`tools/spray_key.py`), so a key written from a file matches the upload.
//! It is not the resource MD5 the demo and `custom.hpk` carry: that one is
//! over the whole WAD file, which the upload never sees.
//!
//! ## Files, all under `<game>\dod\logos\`
//!
//! - `<name>.png`: the HD source, RGBA, any size. Uploaded at the largest
//!   power of two `gl_max_size` allows per side (up to 4096, like every
//!   other HD kind), alpha kept, and drawn at the spray's own size because
//!   the decal is sized from `texture_t`, not the GL texture.
//! - `dodstudio_sprays.txt`, the mapping the hook reads (`//` comments):
//!   `<key>\t<png>` replaces every spray whose content hashes to `<key>`;
//!   `player\t<SteamID or name>\t<png>` is for the HLTV route (below).
//!   PNG names are relative to `logos\`.
//!
//! A spray with no line is listed by `dodstudio_debug_hd_misses` as
//! `decal spray <key>`, which is how a key is found from a demo. A missing
//! or unreadable PNG is logged once and the engine's spray stays.
//!
//! ## `dodstudio_spray <name>`
//!
//! Makes `<name>.png` the user's own spray: writes a `tempdecal.wad` the
//! engine accepts (one `{LOGO` lump, sides in steps of 16 and the file at
//! most 20480 bytes, which makes a square picture 112x112; 8-bit, the palette
//! quantised to 255 colours with entry 255 the transparent blue), keeps `tempdecal.wad.dodstudio-backup` of the file
//! that was there the first time, and writes the lump's key to the mapping,
//! so the spray comes back in HD wherever it is seen. The engine reads
//! `tempdecal.wad` when it connects, so the switch shows from the next
//! connect; a demo draws the sprays it recorded, never this file.
//! `tempdecal.wad` is the game's spray file, and the one file in the game's
//! folder this DLL writes, because that is what the command is for: the
//! user's `.cfg` and `.res` files stay untouched, and `cl_logofile` is left
//! as it is (it only names what Options shows).
//!
//! ## HLTV demos
//!
//! An HLTV demo carries every `TE_PLAYERDECAL` with decal index 0 (the
//! bullet hole) and no `svc_customization`, so `CL_ParseTEnt` finds no
//! `customdata` for the slot and shoots the wad decal. Painting the mapped
//! PNG there needs a detour at that test (pre-Anniversary `hw+0x24624`,
//! 25th Anniversary `hw+0x1ae89e`) that hands `R_CustomDecalShoot` a
//! `texture_t` of ours; that is not built yet. What is built is the mapping
//! line and the slot-to-player lookup behind it: `dodstudio_spray players`
//! lists every slot with its name, SteamID and the PNG it would get.

use std::collections::HashMap;
use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::engine;
use crate::names::console_name;
use crate::texture_hires::{Rgba, game_dir};

/// `dodstudio_spray`: list the PNGs in `logos\`, switch to one, or list the
/// players a mapping line would reach. Registered in `commands.rs`.
pub const NAME: &str = console_name!("spray");
/// The mapping file, in `logos\`.
pub const MAPPING_FILE: &str = "dodstudio_sprays.txt";
const LOGOS: &str = "logos";
const TEMPDECAL: &str = "tempdecal.wad";
const BACKUP: &str = "tempdecal.wad.dodstudio-backup";
/// `Draw_ValidateCustomLogo`'s ceiling on each side.
const MAX_LOGO_SIDE: u32 = 256;
/// The file sizes the server takes a spray at: `COM_CreateCustomization`
/// (pre-Anniversary `hw+0x2911f`) drops a decal resource under 1024 or over
/// 20480 bytes, and the console says "Ignoring invalid custom decal". The
/// player then sprays the default logo, and nobody else sees the custom one.
const MIN_WAD_BYTES: usize = 0x400;
const MAX_WAD_BYTES: usize = 0x5000;
/// Spray sides are multiples of this, so all four mips halve exactly.
const LOGO_STEP: u32 = 16;
/// The transparent entry of a cut-out decal: pure blue at index 255 is what
/// makes `Draw_MiptexTexture` upload it as `TEX_TYPE_ALPHA`.
const CUTOUT_KEY: [u8; 3] = [0, 0, 255];
const TRANSPARENT: u8 = 255;

/// A spray's content key: see the module doc.
pub type Key = [u8; 16];

// ── MD5 ─────────────────────────────────────────────────────────────────────

const MD5_S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];
const MD5_K: [u32; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
    0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
    0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
    0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
    0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
    0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];

/// MD5 (RFC 1321) of `parts` in order. The key is small (a spray is at most
/// 64 KB of indices), so a dependency isn't worth the lock-file churn.
pub fn md5(parts: &[&[u8]]) -> Key {
    let len: usize = parts.iter().map(|p| p.len()).sum();
    let mut msg = Vec::with_capacity(len + 72);
    for p in parts {
        msg.extend_from_slice(p);
    }
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((len as u64).wrapping_mul(8)).to_le_bytes());

    let (mut a0, mut b0, mut c0, mut d0) =
        (0x67452301u32, 0xefcdab89u32, 0x98badcfeu32, 0x10325476u32);
    for block in msg.as_chunks::<64>().0 {
        let m: [u32; 16] = std::array::from_fn(|i| {
            u32::from_le_bytes([
                block[4 * i],
                block[4 * i + 1],
                block[4 * i + 2],
                block[4 * i + 3],
            ])
        });
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(MD5_K[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(MD5_S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut out = [0u8; 16];
    for (i, v) in [a0, b0, c0, d0].iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&v.to_le_bytes());
    }
    out
}

pub fn hex(key: &Key) -> String {
    key.iter().map(|b| format!("{b:02x}")).collect()
}

fn parse_key(text: &str) -> Option<Key> {
    let text = text.trim();
    if text.len() != 32 {
        return None;
    }
    let mut key = [0u8; 16];
    for (i, pair) in text.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        key[i] = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(key)
}

// ── The content key ─────────────────────────────────────────────────────────

/// The key of a spray from the bytes the engine draws: `width`, `height`,
/// mip 0 and the palette, whose length is 3 times the count stored before it.
pub fn spray_key(width: u32, height: u32, indices: &[u8], palette: &[u8]) -> Key {
    let count = (palette.len() / 3) as u16;
    md5(&[
        &width.to_le_bytes(),
        &height.to_le_bytes(),
        indices,
        &count.to_le_bytes(),
        &palette[..count as usize * 3],
    ])
}

/// The key of the spray `GL_LoadTexture2` is uploading, from its own
/// arguments. `None` when they don't describe a lump this module knows
/// (a palette count of 0 or over 256).
///
/// # Safety
///
/// `data` and `palette` are `GL_LoadTexture2`'s arguments for a `GLT_DECAL`
/// upload: `width * height` indices, and a palette preceded by its 2-byte
/// count, all inside the cache buffer `Draw_ValidateCustomLogo` checked.
pub unsafe fn upload_key(
    data: *const u8,
    palette: *const u8,
    width: u32,
    height: u32,
) -> Option<Key> {
    if data.is_null()
        || palette.is_null()
        || !(1..=MAX_LOGO_SIDE).contains(&width)
        || !(1..=MAX_LOGO_SIDE).contains(&height)
    {
        return None;
    }
    // A count of 0 passes `Draw_ValidateCustomLogo` and `GL_Upload8` then
    // reads a full palette anyway (6 of the 216 sprays in one `custom.hpk`
    // are written that way), so it means 256 here too.
    let count = match unsafe { (palette.sub(2) as *const u16).read_unaligned() } {
        0 => 256,
        n if n > 256 => return None,
        n => n,
    };
    let (indices, pal) = unsafe {
        (
            std::slice::from_raw_parts(data, (width * height) as usize),
            std::slice::from_raw_parts(palette, count as usize * 3),
        )
    };
    Some(spray_key(width, height, indices, pal))
}

// ── Telling a spray from a wad decal ────────────────────────────────────────

/// `Draw_CacheLoadFromCustom`, from `push <"T">; push <szCustName>;
/// mov [gfCustomBuild], 1; call strcpy` through the `push 5` of the
/// `memcpy(szCustName + 1, clean, 5)` that follows -- the two builds order
/// the argument loads differently, hence two patterns. Wildcards: the two
/// addresses, the flag's address and the call displacement.
const LOAD_FROM_CUSTOM_PRE: &str = "68 ?? ?? ?? ?? 68 ?? ?? ?? ?? C7 05 ?? ?? ?? ?? 01 00 00 00 \
    E8 ?? ?? ?? ?? 8B 4D 08 6A 05 51 68";
const LOAD_FROM_CUSTOM_POST: &str = "68 ?? ?? ?? ?? 68 ?? ?? ?? ?? C7 05 ?? ?? ?? ?? 01 00 00 00 \
    E8 ?? ?? ?? ?? 6A 05 8D 45 EC 50 68";
/// `gfCustomBuild`'s address: 0 until looked for, `usize::MAX` when not found.
static CUSTOM_BUILD_FLAG: AtomicUsize = AtomicUsize::new(0);

/// Finds `gfCustomBuild` once. The match is only trusted when the string
/// the first `push` names is `"T"`, the prefix every custom name gets.
fn find_custom_build_flag() -> Option<usize> {
    let base = engine::engine_module_base()?;
    // Safety: `base` is hw.dll's module handle, mapped for the session.
    let at = unsafe { crate::scan::find_unique(base, LOAD_FROM_CUSTOM_PRE) }
        .or_else(|_| unsafe { crate::scan::find_unique(base, LOAD_FROM_CUSTOM_POST) })
        .ok()?;
    // Safety: the pattern matched these 20 bytes; the pushed address is in
    // hw.dll's own read-only data.
    let (prefix, flag) = unsafe {
        (
            ((at + 1) as *const u32).read_unaligned() as usize,
            ((at + 12) as *const u32).read_unaligned() as usize,
        )
    };
    if prefix == 0 || flag == 0 {
        return None;
    }
    let is_t = unsafe { std::slice::from_raw_parts(prefix as *const u8, 2) } == b"T\0";
    is_t.then_some(flag)
}

fn custom_build_flag() -> Option<usize> {
    match CUSTOM_BUILD_FLAG.load(Ordering::Acquire) {
        0 => {
            let found = find_custom_build_flag();
            CUSTOM_BUILD_FLAG.store(found.unwrap_or(usize::MAX), Ordering::Release);
            unsafe {
                crate::debug::report(&match found {
                    Some(at) => format!("spray: gfCustomBuild at {at:#x}"),
                    None => {
                        "spray: gfCustomBuild not found; sprays are told apart from wad decals by \
                             their name shape instead"
                            .to_string()
                    }
                })
            };
            found
        }
        usize::MAX => None,
        at => Some(at),
    }
}

/// The name `Draw_MiptexTexture` gives a custom spray: `{` or `}` and the
/// five characters of `%03i%02i` (the slot may be `-2` while a spray is
/// validated, hence the `-`).
pub fn looks_like_custom_name(identifier: &str) -> bool {
    let b = identifier.as_bytes();
    b.len() == 6
        && (b[0] == b'{' || b[0] == b'}')
        && b[1..].iter().all(|c| c.is_ascii_digit() || *c == b'-')
}

/// Whether the `GLT_DECAL` upload in progress is a player's spray rather
/// than a `decals.wad` decal.
pub fn is_custom_upload(identifier: &str) -> bool {
    match custom_build_flag() {
        // Safety: a dword in hw.dll's data, found as described above.
        Some(flag) => (unsafe { (flag as *const u32).read_volatile() }) != 0,
        None => looks_like_custom_name(identifier),
    }
}

// ── The mapping file ────────────────────────────────────────────────────────

/// Who a `player` line names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerId {
    /// SteamID64, written as the 17 digits, `STEAM_0:X:Y` or `[U:1:N]`.
    SteamId(u64),
    /// A player name, matched case-insensitively.
    Name(String),
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Mapping {
    /// Content key -> PNG name.
    pub by_key: HashMap<Key, String>,
    /// `player` lines, in file order; the first match wins.
    pub players: Vec<(PlayerId, String)>,
    /// Lines that weren't a comment, blank, or one of the two shapes.
    pub bad: Vec<(usize, String)>,
}

/// `STEAM_0:X:Y`, `[U:1:N]` or the 17-digit SteamID64.
pub fn parse_steam_id(text: &str) -> Option<u64> {
    const BASE: u64 = 76561197960265728;
    let text = text.trim();
    if let Some(rest) = text.strip_prefix("STEAM_") {
        let mut it = rest.split(':');
        let _universe = it.next()?;
        let x: u64 = it.next()?.parse().ok()?;
        let y: u64 = it.next()?.parse().ok()?;
        return (it.next().is_none() && x <= 1).then(|| BASE + y * 2 + x);
    }
    if let Some(inner) = text.strip_prefix("[U:1:").and_then(|t| t.strip_suffix(']')) {
        let n: u64 = inner.parse().ok()?;
        return Some(BASE + n);
    }
    let id: u64 = text.parse().ok()?;
    (id > BASE).then_some(id)
}

/// A PNG name from a mapping line: a file name inside `logos\`, never a path.
fn clean_png_name(raw: &str) -> Option<String> {
    let name = raw.trim();
    (!name.is_empty()
        && !name.contains(['/', '\\', ':'])
        && name != ".."
        && name.to_ascii_lowercase().ends_with(".png"))
    .then(|| name.to_string())
}

/// Splits a line into its fields: at tabs when it has any, else at runs of
/// spaces, keeping the last field whole so a PNG name may contain spaces.
fn fields(line: &str, n: usize) -> Vec<&str> {
    if line.contains('\t') {
        let mut out: Vec<&str> = line.splitn(n, '\t').map(str::trim).collect();
        out.retain(|f| !f.is_empty());
        return out;
    }
    let mut out = Vec::new();
    let mut rest = line.trim();
    while out.len() + 1 < n {
        match rest.split_once(char::is_whitespace) {
            Some((head, tail)) => {
                out.push(head);
                rest = tail.trim_start();
            }
            None => break,
        }
    }
    if !rest.is_empty() {
        out.push(rest);
    }
    out
}

pub fn parse_mapping(text: &str) -> Mapping {
    let mut map = Mapping::default();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
            continue;
        }
        let bad = |map: &mut Mapping| map.bad.push((i + 1, raw.to_string()));
        if line.to_ascii_lowercase().starts_with("player") {
            let f = fields(line, 3);
            match (f.get(1), f.get(2).and_then(|p| clean_png_name(p))) {
                (Some(id), Some(png)) if f.len() == 3 => {
                    let id = parse_steam_id(id)
                        .map(PlayerId::SteamId)
                        .unwrap_or_else(|| PlayerId::Name(id.to_string()));
                    map.players.push((id, png));
                }
                _ => bad(&mut map),
            }
            continue;
        }
        let f = fields(line, 2);
        match (
            f.first().and_then(|k| parse_key(k)),
            f.get(1).and_then(|p| clean_png_name(p)),
        ) {
            (Some(key), Some(png)) if f.len() == 2 => {
                map.by_key.insert(key, png);
            }
            _ => bad(&mut map),
        }
    }
    map
}

pub fn logos_dir() -> PathBuf {
    game_dir().join(LOGOS)
}

pub fn mapping_path() -> PathBuf {
    logos_dir().join(MAPPING_FILE)
}

/// Reads the mapping afresh: a spray loads once per connect or demo, so the
/// file is read that rarely, and an edit between demos is seen.
pub fn load_mapping() -> Mapping {
    match std::fs::read_to_string(mapping_path()) {
        Ok(text) => parse_mapping(&text),
        Err(_) => Mapping::default(),
    }
}

/// The PNG mapped to `key`, as a path, or `None`.
pub fn png_for_key(key: &Key) -> Option<PathBuf> {
    load_mapping()
        .by_key
        .get(key)
        .map(|png| logos_dir().join(png))
}

/// Which `player` line a slot's name and SteamID match, if any.
pub fn png_for_player<'a>(mapping: &'a Mapping, name: &str, steam_id: u64) -> Option<&'a str> {
    mapping
        .players
        .iter()
        .find(|(id, _)| match id {
            PlayerId::SteamId(s) => steam_id != 0 && *s == steam_id,
            PlayerId::Name(n) => n.eq_ignore_ascii_case(name),
        })
        .map(|(_, png)| png.as_str())
}

// ── PNG in, fitted RGBA out ─────────────────────────────────────────────────

/// Decodes a PNG of any colour type or depth to RGBA8, top row first.
pub fn decode_png(bytes: &[u8]) -> Result<Rgba, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let size = reader.output_buffer_size().ok_or("too big to decode")?;
    // A 32-bit game: run out of memory with an error, not an abort.
    let mut buf = Vec::new();
    buf.try_reserve_exact(size)
        .map_err(|_| format!("not enough memory for {size} bytes of pixels"))?;
    buf.resize(size, 0);
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (width, height) = (info.width, info.height);
    if width == 0 || height == 0 {
        return Err("empty image".into());
    }
    let src = &buf[..info.buffer_size()];
    let count = (width as usize) * (height as usize);
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(count * 4)
        .map_err(|_| "not enough memory for the RGBA copy".to_string())?;
    match info.color_type {
        png::ColorType::Rgba => pixels.extend_from_slice(&src[..count * 4]),
        png::ColorType::Rgb => {
            for px in src[..count * 3].as_chunks::<3>().0 {
                pixels.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for px in src[..count * 2].as_chunks::<2>().0 {
                pixels.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
            }
        }
        png::ColorType::Grayscale => {
            for &g in &src[..count] {
                pixels.extend_from_slice(&[g, g, g, 255]);
            }
        }
        other => return Err(format!("colour type {other:?} after expansion")),
    }
    Ok(Rgba {
        width,
        height,
        pixels,
    })
}

/// The largest power of two at most `side` and at most `cap`; never 0.
pub fn pot_floor(side: u32, cap: u32) -> u32 {
    let v = side.min(cap).max(1);
    1 << (31 - v.leading_zeros())
}

/// Shrinks an image to `nw` x `nh` (each at most the current side) with a
/// box filter: every destination pixel averages the source pixels it covers,
/// colour weighted by alpha so transparent pixels don't darken edges.
pub fn resample(img: &Rgba, nw: u32, nh: u32) -> Rgba {
    let (w, h) = (img.width as usize, img.height as usize);
    let (nw, nh) = (nw.max(1) as usize, nh.max(1) as usize);
    if nw == w && nh == h {
        return Rgba {
            width: img.width,
            height: img.height,
            pixels: img.pixels.clone(),
        };
    }
    let mut out = Vec::with_capacity(nw * nh * 4);
    for y in 0..nh {
        let (y0, y1) = (y * h / nh, ((y + 1) * h / nh).max(y * h / nh + 1).min(h));
        for x in 0..nw {
            let (x0, x1) = (x * w / nw, ((x + 1) * w / nw).max(x * w / nw + 1).min(w));
            let (mut r, mut g, mut b, mut a, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let p = &img.pixels[(sy * w + sx) * 4..][..4];
                    let alpha = p[3] as u64;
                    r += p[0] as u64 * alpha;
                    g += p[1] as u64 * alpha;
                    b += p[2] as u64 * alpha;
                    a += alpha;
                    n += 1;
                }
            }
            // A fully transparent box has no colour to average: black.
            let per_alpha = |sum: u64| (sum + a / 2).checked_div(a).unwrap_or(0) as u8;
            out.extend_from_slice(&[
                per_alpha(r),
                per_alpha(g),
                per_alpha(b),
                ((a + n / 2) / n) as u8,
            ]);
        }
    }
    Rgba {
        width: nw as u32,
        height: nh as u32,
        pixels: out,
    }
}

/// Each side to the largest power of two at most itself and `cap`.
pub fn fit_pot(img: &Rgba, cap: u32) -> Rgba {
    resample(img, pot_floor(img.width, cap), pot_floor(img.height, cap))
}

/// PNGs already reported unreadable this session, so each is said once.
static WARNED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Says why `path` couldn't be used, the first time only.
pub fn warn_once(path: &Path, why: &str) {
    let Ok(mut warned) = WARNED.lock() else {
        return;
    };
    if warned.iter().any(|p| p == path) {
        return;
    }
    warned.push(path.to_path_buf());
    let line = format!("{NAME}: {} not used -- {why}\n", path.display());
    unsafe { crate::debug::report(line.trim_end()) };
    crate::commands::console_print(&line);
}

/// The replacement for a mapped spray: the PNG decoded and fitted to `cap`.
pub fn load_png_fit(path: &Path, cap: u32) -> Result<Rgba, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let img = decode_png(&bytes)?;
    Ok(fit_pot(&img, cap))
}

// ── tempdecal.wad ───────────────────────────────────────────────────────────

/// An 8-bit spray as the engine wants it: indices into a 256-entry palette
/// whose last entry is the transparent key.
pub struct Logo {
    pub width: u32,
    pub height: u32,
    pub indices: Vec<u8>,
    pub palette: [u8; 768],
}

/// Median-cut quantisation of the opaque pixels to at most `n` colours.
fn median_cut(colors: &HashMap<[u8; 3], u32>, n: usize) -> Vec<[u8; 3]> {
    // Sorted, so the same picture always gives the same palette and key: a
    // HashMap's order changes every run, and ties in the splits with it.
    let mut all: Vec<([u8; 3], u32)> = colors.iter().map(|(c, k)| (*c, *k)).collect();
    all.sort_unstable();
    let mut boxes = vec![all];
    if boxes[0].is_empty() {
        return Vec::new();
    }
    while boxes.len() < n {
        // The box with the widest channel range; stop when none can split.
        let mut best: Option<(usize, usize, u32)> = None;
        for (i, b) in boxes.iter().enumerate() {
            if b.len() < 2 {
                continue;
            }
            for ch in 0..3 {
                let (lo, hi) = b.iter().fold((255u8, 0u8), |(lo, hi), (c, _)| {
                    (lo.min(c[ch]), hi.max(c[ch]))
                });
                let range = (hi - lo) as u32;
                if best.is_none_or(|(_, _, r)| range > r) {
                    best = Some((i, ch, range));
                }
            }
        }
        let Some((i, ch, range)) = best else {
            break;
        };
        if range == 0 {
            break;
        }
        let mut b = std::mem::take(&mut boxes[i]);
        b.sort_by_key(|(c, _)| c[ch]);
        let total: u64 = b.iter().map(|(_, k)| *k as u64).sum();
        let mut acc = 0u64;
        let mut split = 1;
        for (j, (_, k)) in b.iter().enumerate() {
            acc += *k as u64;
            if acc * 2 >= total {
                split = (j + 1).clamp(1, b.len() - 1);
                break;
            }
        }
        let tail = b.split_off(split);
        boxes[i] = b;
        boxes.push(tail);
    }
    boxes
        .iter()
        .filter(|b| !b.is_empty())
        .map(|b| {
            let total: u64 = b.iter().map(|(_, k)| *k as u64).sum::<u64>().max(1);
            let mut sum = [0u64; 3];
            for (c, k) in b {
                for ch in 0..3 {
                    sum[ch] += c[ch] as u64 * *k as u64;
                }
            }
            [
                ((sum[0] + total / 2) / total) as u8,
                ((sum[1] + total / 2) / total) as u8,
                ((sum[2] + total / 2) / total) as u8,
            ]
        })
        .collect()
}

fn nearest(palette: &[[u8; 3]], c: [u8; 3]) -> u8 {
    let mut best = (u32::MAX, 0u8);
    for (i, p) in palette.iter().enumerate() {
        let d = (0..3)
            .map(|ch| {
                let diff = p[ch] as i32 - c[ch] as i32;
                (diff * diff) as u32
            })
            .sum::<u32>();
        if d < best.0 {
            best = (d, i as u8);
        }
    }
    best.1
}

/// Maps RGBA pixels onto `palette`: alpha under 128 is the transparent key,
/// the rest the nearest colour. Each distinct colour is looked up once.
fn index_pixels(pixels: &[u8], palette: &[[u8; 3]]) -> Vec<u8> {
    let mut seen: HashMap<[u8; 3], u8> = HashMap::new();
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            if p[3] < 128 {
                return TRANSPARENT;
            }
            let c = [p[0], p[1], p[2]];
            *seen.entry(c).or_insert_with(|| nearest(palette, c))
        })
        .collect()
}

/// Quantises an RGBA image (already at most 256 a side) to 255 colours plus
/// the transparent key at 255.
pub fn quantise(img: &Rgba) -> Logo {
    let mut histogram: HashMap<[u8; 3], u32> = HashMap::new();
    for p in img.pixels.as_chunks::<4>().0 {
        if p[3] >= 128 {
            *histogram.entry([p[0], p[1], p[2]]).or_default() += 1;
        }
    }
    let colors = median_cut(&histogram, 255);
    let mut palette = [0u8; 768];
    for (i, c) in colors.iter().enumerate() {
        palette[3 * i..3 * i + 3].copy_from_slice(c);
    }
    palette[765..768].copy_from_slice(&CUTOUT_KEY);
    Logo {
        width: img.width,
        height: img.height,
        indices: index_pixels(&img.pixels, &colors),
        palette,
    }
}

/// The four mip levels' indices, each level the image halved and mapped
/// onto the same palette.
fn mips(img: &Rgba, logo: &Logo) -> [Vec<u8>; 4] {
    let colors: Vec<[u8; 3]> = (0..255)
        .map(|i| {
            [
                logo.palette[3 * i],
                logo.palette[3 * i + 1],
                logo.palette[3 * i + 2],
            ]
        })
        .collect();
    std::array::from_fn(|level| {
        if level == 0 {
            return logo.indices.clone();
        }
        let small = resample(
            img,
            (img.width >> level).max(1),
            (img.height >> level).max(1),
        );
        index_pixels(&small.pixels, &colors)
    })
}

/// The size of the WAD [`build_wad`] writes for a `w`x`h` spray: header,
/// miptex header, four mips, palette count and palette, padded to 4, then
/// the one directory entry.
fn wad_bytes(w: u32, h: u32) -> usize {
    let pix = (w * h) as usize;
    let lump = 40 + pix + (pix >> 2) + (pix >> 4) + (pix >> 6) + 2 + 768;
    (12 + lump).next_multiple_of(4) + 32
}

/// The spray's size: sides in steps of 16 (so every mip halves exactly),
/// the aspect kept, never larger than the picture, and the biggest that
/// still fits [`MAX_WAD_BYTES`]. A square picture comes out 112x112.
fn logo_size(width: u32, height: u32) -> (u32, u32) {
    let long = width.max(height).max(1);
    let short = width.min(height);
    let step_floor = |side: u32| (side / LOGO_STEP * LOGO_STEP).clamp(LOGO_STEP, MAX_LOGO_SIDE);
    let mut target = step_floor(long);
    loop {
        let scaled = (short as f64 * target as f64 / long as f64 / LOGO_STEP as f64).round() as u32;
        let other = (scaled * LOGO_STEP).clamp(LOGO_STEP, target);
        if wad_bytes(target, other) <= MAX_WAD_BYTES || target == LOGO_STEP {
            return if width >= height {
                (target, other)
            } else {
                (other, target)
            };
        }
        target -= LOGO_STEP;
    }
}

fn fit_logo(img: &Rgba) -> Rgba {
    let (w, h) = logo_size(img.width, img.height);
    resample(img, w, h)
}

/// A one-lump WAD3 of `img` as a `{LOGO` spray, and the lump's key. The
/// image is first fitted by [`fit_logo`].
pub fn build_wad(img: &Rgba) -> Result<(Vec<u8>, Key), String> {
    let fitted = fit_logo(img);
    let logo = quantise(&fitted);
    let (w, h) = (logo.width, logo.height);
    if !(1..=MAX_LOGO_SIDE).contains(&w) || !(1..=MAX_LOGO_SIDE).contains(&h) {
        return Err(format!("{w}x{h} is outside 1..{MAX_LOGO_SIDE}"));
    }
    let levels = mips(&fitted, &logo);
    let key = spray_key(w, h, &logo.indices, &logo.palette);

    // The lump: miptex header, four mips, the palette count and the palette.
    let mut lump = Vec::new();
    let mut name = [0u8; 16];
    name[..5].copy_from_slice(b"{LOGO");
    lump.extend_from_slice(&name);
    lump.extend_from_slice(&w.to_le_bytes());
    lump.extend_from_slice(&h.to_le_bytes());
    let mut at = 40u32;
    for level in &levels {
        lump.extend_from_slice(&at.to_le_bytes());
        at += level.len() as u32;
    }
    for level in &levels {
        lump.extend_from_slice(level);
    }
    lump.extend_from_slice(&256u16.to_le_bytes());
    lump.extend_from_slice(&logo.palette);

    // The file: header, lump at 12, the directory last (the engine's
    // container check wants `infotableofs + 32 * numlumps == file size`).
    let mut wad = Vec::with_capacity(lump.len() + 64);
    wad.extend_from_slice(b"WAD3");
    wad.extend_from_slice(&1i32.to_le_bytes());
    let lump_at = 12u32;
    let dir_at = (lump_at as usize + lump.len()).next_multiple_of(4) as u32;
    wad.extend_from_slice(&dir_at.to_le_bytes());
    wad.extend_from_slice(&lump);
    wad.resize(dir_at as usize, 0);
    wad.extend_from_slice(&lump_at.to_le_bytes());
    wad.extend_from_slice(&(lump.len() as u32).to_le_bytes());
    wad.extend_from_slice(&(lump.len() as u32).to_le_bytes());
    wad.push(0x43); // type: miptex
    wad.push(0); // compression
    wad.extend_from_slice(&[0, 0]); // pad
    wad.extend_from_slice(&name);
    if !(MIN_WAD_BYTES..=MAX_WAD_BYTES).contains(&wad.len()) {
        return Err(format!(
            "{w}x{h} makes a {} byte file; the game takes {MIN_WAD_BYTES}..{MAX_WAD_BYTES}",
            wad.len()
        ));
    }
    Ok((wad, key))
}

/// A one-lump spray WAD read back: the key, width and height of its lump,
/// checked the way `Draw_ValidateCustomLogo` and the container check do.
pub fn wad_key(wad: &[u8]) -> Result<(Key, u32, u32), String> {
    let u32_at = |at: usize| -> Result<u32, String> {
        wad.get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| "truncated".to_string())
    };
    if wad.get(..4) != Some(b"WAD3") {
        return Err("not a WAD3 file".into());
    }
    let (count, dir_at) = (u32_at(4)?, u32_at(8)? as usize);
    if count != 1 {
        return Err(format!("{count} lumps; a spray has exactly one"));
    }
    if dir_at + 32 != wad.len() {
        return Err("the directory isn't at the end of the file".into());
    }
    let (at, size) = (u32_at(dir_at)? as usize, u32_at(dir_at + 8)? as usize);
    if at < 12 || at + size > dir_at {
        return Err("the lump runs outside the file".into());
    }
    let lump = &wad[at..at + size];
    let (w, h) = (u32_at(at + 16)?, u32_at(at + 20)?);
    if !(1..=MAX_LOGO_SIDE).contains(&w) || !(1..=MAX_LOGO_SIDE).contains(&h) {
        return Err(format!("{w}x{h} is outside 1..{MAX_LOGO_SIDE}"));
    }
    let mip0 = u32_at(at + 24)? as usize;
    let pix = (w * h) as usize;
    let pal_count_at = mip0 + pix + (pix >> 2) + (pix >> 4) + (pix >> 6);
    let count = lump
        .get(pal_count_at..pal_count_at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]) as usize)
        .ok_or("truncated before the palette")?;
    // 0 reads as a full palette, as in `upload_key`.
    let count = match count {
        0 => 256,
        n if n > 256 => return Err(format!("{n} palette entries")),
        n => n,
    };
    let indices = lump.get(mip0..mip0 + pix).ok_or("truncated mip 0")?;
    let palette = lump
        .get(pal_count_at + 2..pal_count_at + 2 + 3 * count)
        .ok_or("truncated palette")?;
    Ok((spray_key(w, h, indices, palette), w, h))
}

// ── The command ─────────────────────────────────────────────────────────────

fn args() -> Vec<String> {
    let Some(engfuncs) = engine::engfuncs() else {
        return Vec::new();
    };
    let argc = unsafe { (engfuncs.cmd_argc)() };
    (0..argc)
        .filter_map(|i| {
            let ptr = unsafe { (engfuncs.cmd_argv)(i) };
            (!ptr.is_null()).then(|| {
                unsafe { CStr::from_ptr(ptr) }
                    .to_string_lossy()
                    .into_owned()
            })
        })
        .collect()
}

fn usage() -> String {
    format!(
        "{NAME}: list the PNGs in dod\\{LOGOS}\\ and say which is your spray\n\
         {NAME} <name>: make dod\\{LOGOS}\\<name>.png your spray (writes dod\\{TEMPDECAL}, the game's spray file; \
         shows from the next connect) and map it for HD playback\n\
         {NAME} players: each slot in the demo and the PNG a player line in {MAPPING_FILE} gives it\n"
    )
}

/// The PNGs in `logos\`, sorted, with their sizes.
fn logo_pngs() -> Vec<(String, Option<(u32, u32)>)> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(logos_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !name.to_ascii_lowercase().ends_with(".png") {
                continue;
            }
            out.push((name.to_string(), png_size(&path)));
        }
    }
    out.sort_by_key(|(n, _)| n.to_ascii_lowercase());
    out
}

/// Width and height from a PNG's IHDR, without decoding it.
fn png_size(path: &Path) -> Option<(u32, u32)> {
    use std::io::Read;
    let mut head = [0u8; 24];
    std::fs::File::open(path).ok()?.read_exact(&mut head).ok()?;
    (&head[..8] == b"\x89PNG\r\n\x1a\n" && &head[12..16] == b"IHDR").then(|| {
        (
            u32::from_be_bytes([head[16], head[17], head[18], head[19]]),
            u32::from_be_bytes([head[20], head[21], head[22], head[23]]),
        )
    })
}

fn list() -> String {
    let mapping = load_mapping();
    let current = std::fs::read(game_dir().join(TEMPDECAL))
        .ok()
        .and_then(|wad| wad_key(&wad).ok());
    let current_png = current
        .as_ref()
        .and_then(|(key, ..)| mapping.by_key.get(key).cloned());
    let mut out = format!("{NAME}: PNGs in dod\\{LOGOS}\\ --\n");
    let pngs = logo_pngs();
    if pngs.is_empty() {
        out.push_str("  none\n");
    }
    for (name, size) in &pngs {
        let size = size.map_or("not a PNG".to_string(), |(w, h)| format!("{w}x{h}"));
        let keys = mapping
            .by_key
            .iter()
            .filter(|(_, png)| png.eq_ignore_ascii_case(name))
            .count();
        let mapped = match keys {
            0 => String::new(),
            1 => ", mapped to one spray".to_string(),
            n => format!(", mapped to {n} sprays"),
        };
        let own = if current_png
            .as_deref()
            .is_some_and(|c| c.eq_ignore_ascii_case(name))
        {
            "  <- your spray"
        } else {
            ""
        };
        out.push_str(&format!("  {name}  {size}{mapped}{own}\n"));
    }
    match (&current, &current_png) {
        (Some((key, w, h)), None) => out.push_str(&format!(
            "your spray (dod\\{TEMPDECAL}): {w}x{h}, key {}, not made by this command\n",
            hex(key)
        )),
        (None, _) => out.push_str(&format!("your spray: no readable dod\\{TEMPDECAL}\n")),
        _ => {}
    }
    out.push_str(&format!(
        "mapping dod\\{LOGOS}\\{MAPPING_FILE}: {} spray line(s), {} player line(s){}\n",
        mapping.by_key.len(),
        mapping.players.len(),
        if mapping.bad.is_empty() {
            String::new()
        } else {
            format!(
                ", {} line(s) not understood (first: line {})",
                mapping.bad.len(),
                mapping.bad[0].0
            )
        }
    ));
    out
}

/// Rewrites the mapping with `key -> png`, replacing any line for that key
/// and creating the file (with its header) when there is none.
fn write_mapping_line(key: &Key, png: &str) -> Result<(), String> {
    let path = mapping_path();
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut text = String::new();
    if existing.trim().is_empty() {
        text.push_str(&format!(
            "// DoD Studio spray mapping (#428). One spray a line: the key, a tab, a PNG in this\n\
             // folder. The key is the MD5 {NAME} prints (width, height, mip 0, palette count and\n\
             // palette of the spray as the game loads it). player, a tab, a SteamID or name, a tab,\n\
             // a PNG is a line for HLTV demos. Written by {NAME} <name>; edit freely.\n"
        ));
    } else {
        let hexed = hex(key);
        for line in existing.lines() {
            let keep = fields(line.trim(), 2)
                .first()
                .and_then(|k| parse_key(k))
                .is_none_or(|k| hex(&k) != hexed);
            if keep {
                text.push_str(line);
                text.push('\n');
            }
        }
    }
    text.push_str(&format!("{}\t{png}\n", hex(key)));
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

fn switch(name: &str) -> Result<String, String> {
    let file = if name.to_ascii_lowercase().ends_with(".png") {
        name.to_string()
    } else {
        format!("{name}.png")
    };
    let file = clean_png_name(&file).ok_or_else(|| format!("{name:?} is not a PNG name"))?;
    let path = logos_dir().join(&file);
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let img = decode_png(&bytes).map_err(|why| format!("{}: {why}", path.display()))?;
    let (wad, key) = build_wad(&img)?;
    let (_, w, h) = wad_key(&wad)?;

    let dod = game_dir();
    let target = dod.join(TEMPDECAL);
    let backup = dod.join(BACKUP);
    let mut kept = String::new();
    if target.is_file() && !backup.exists() {
        std::fs::copy(&target, &backup)
            .map_err(|e| format!("backing up {}: {e}", target.display()))?;
        kept = format!("; the one that was there is kept as dod\\{BACKUP}");
    }
    let tmp = dod.join("tempdecal.wad.dodstudio-tmp");
    std::fs::write(&tmp, &wad).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &target).map_err(|e| format!("{}: {e}", target.display()))?;
    write_mapping_line(&key, &file)?;
    Ok(format!(
        "{NAME}: wrote dod\\{TEMPDECAL} (the game's spray file) from {file}: {w}x{h}, 8-bit, key {}{kept}. \
         Mapped the key to {file} in dod\\{LOGOS}\\{MAPPING_FILE}, so it plays back in HD. \
         The game reads the file when it connects: the new spray shows from the next connect, not in a demo \
         already playing.\n",
        hex(&key)
    ))
}

/// `players`: every slot the engine has a player in, with what a `player`
/// line gives him. The SteamID comes from the slot's userinfo, as
/// `dodstudio_deathmsg players` reads it.
fn players() -> String {
    let Some(engfuncs) = engine::engfuncs() else {
        return format!("{NAME} players: the game's functions aren't captured yet\n");
    };
    let mapping = load_mapping();
    let hltv = unsafe { (engfuncs.is_spectate_only)() } != 0;
    let mut out = format!(
        "{NAME} players ({} demo; {} player line(s) in {MAPPING_FILE}):\n",
        if hltv { "HLTV" } else { "POV" },
        mapping.players.len()
    );
    let mut any = false;
    for slot in 1..=32 {
        let mut info = engine::HudPlayerInfo::empty();
        unsafe { (engfuncs.pfn_get_player_info)(slot, &mut info) };
        if info.name.is_null() {
            continue;
        }
        any = true;
        let name = unsafe { CStr::from_ptr(info.name) }
            .to_string_lossy()
            .into_owned();
        let steam_id = if info.steam_id != 0 {
            info.steam_id
        } else {
            unsafe { crate::deathmsg::steam_id_from_record(info.name) }
        };
        let id = if steam_id == 0 {
            "SteamID unknown".to_string()
        } else {
            steam_id.to_string()
        };
        let png = png_for_player(&mapping, &name, steam_id)
            .map_or("no player line".to_string(), |p| format!("-> {p}"));
        out.push_str(&format!("  {slot:>2}  {name}  {id}  {png}\n"));
    }
    if !any {
        out.push_str("  nobody: no demo or game loaded\n");
    }
    if hltv {
        out.push_str(
            "  (an HLTV demo draws the bullet hole for every spray; painting the mapped PNG there is not built yet, see the module doc)\n",
        );
    }
    out
}

/// The console command.
pub unsafe extern "C" fn command() {
    let argv = args();
    let text = match argv.get(1).map(String::as_str) {
        None => list(),
        Some(a) if a.eq_ignore_ascii_case("players") => players(),
        Some(a) if a.eq_ignore_ascii_case("help") || a == "?" => usage(),
        Some(name) => match switch(name) {
            Ok(done) => done,
            Err(why) => format!("{NAME}: {why}\n{}", usage()),
        },
    };
    for line in text.lines() {
        crate::commands::console_print(&format!("{line}\n"));
    }
    unsafe { crate::debug::report(&format!("spray: {}", text.trim_end())) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_matches_rfc_1321() {
        assert_eq!(hex(&md5(&[b""])), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex(&md5(&[b"abc"])), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            hex(&md5(&[b"The quick brown fox jumps ", b"over the lazy dog"])),
            "9e107d9d372bb6826bd81d3542a419d6"
        );
        // 80 bytes: the padding needs a second block.
        assert_eq!(
            hex(&md5(&[
                b"12345678901234567890123456789012345678901234567890123456789012345678901234567890"
            ])),
            "57edf4a22be3c955ac49da2e2107b67a"
        );
    }

    fn sample() -> Rgba {
        // 16x16: a red square on a transparent field, a blue dot, a soft edge.
        let (w, h) = (16u32, 16u32);
        let mut pixels = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let inside = (4..12).contains(&x) && (4..12).contains(&y);
                let px = if x == 0 && y == 0 {
                    [0, 0, 255, 255]
                } else if inside {
                    [200, 30 + x as u8, 30, 255]
                } else if x == 12 && y == 8 {
                    [200, 40, 30, 100]
                } else {
                    [0, 0, 0, 0]
                };
                pixels.extend_from_slice(&px);
            }
        }
        Rgba {
            width: w,
            height: h,
            pixels,
        }
    }

    /// A WAD the writer makes passes the engine's checks as `wad_key` applies
    /// them, and the key read back from the file is the key of the upload the
    /// engine would make from it: the same bytes `spray_key` hashes.
    #[test]
    fn a_written_wad_round_trips_through_the_engines_layout() {
        let img = sample();
        let (wad, key) = build_wad(&img).unwrap();
        let (read, w, h) = wad_key(&wad).unwrap();
        assert_eq!((w, h), (16, 16));
        assert_eq!(read, key);

        // The container as the engine's check reads it.
        assert_eq!(&wad[..4], b"WAD3");
        let dir_at = u32::from_le_bytes(wad[8..12].try_into().unwrap()) as usize;
        assert_eq!(dir_at + 32, wad.len());
        assert_eq!(dir_at % 4, 0);
        let (filepos, disksize, size) = (
            u32::from_le_bytes(wad[dir_at..dir_at + 4].try_into().unwrap()) as usize,
            u32::from_le_bytes(wad[dir_at + 4..dir_at + 8].try_into().unwrap()) as usize,
            u32::from_le_bytes(wad[dir_at + 8..dir_at + 12].try_into().unwrap()) as usize,
        );
        assert_eq!(filepos, 12);
        assert_eq!(disksize, size);
        assert!(filepos + disksize <= dir_at);
        assert_eq!(wad[dir_at + 12], 0x43);
        assert_eq!(&wad[dir_at + 16..dir_at + 21], b"{LOGO");

        // The lump as Draw_ValidateCustomLogo reads it.
        let lump = &wad[filepos..filepos + size];
        assert_eq!(&lump[..5], b"{LOGO");
        let pix = 16 * 16;
        let offsets: Vec<usize> = (0..4)
            .map(|i| u32::from_le_bytes(lump[24 + 4 * i..28 + 4 * i].try_into().unwrap()) as usize)
            .collect();
        assert_eq!(
            offsets,
            vec![
                40,
                40 + pix,
                40 + pix + pix / 4,
                40 + pix + pix / 4 + pix / 16
            ]
        );
        let pal_at = offsets[3] + pix / 64;
        assert_eq!(u16::from_le_bytes([lump[pal_at], lump[pal_at + 1]]), 256);
        assert_eq!(lump.len(), pal_at + 2 + 768);
        assert_eq!(&lump[lump.len() - 3..], &CUTOUT_KEY);

        // Transparent where the PNG was, red where it was red, index 255 for
        // the key only.
        let indices = &lump[40..40 + pix];
        assert_eq!(indices[1], TRANSPARENT);
        assert_ne!(indices[8 * 16 + 8], TRANSPARENT);
        assert_eq!(indices[8 * 16 + 12], TRANSPARENT, "alpha 100 is cut out");
        let pal = &lump[pal_at + 2..];
        let red = indices[8 * 16 + 8] as usize;
        assert!(pal[3 * red] > 150 && pal[3 * red + 2] < 80);
    }

    #[test]
    fn the_wad_fits_the_size_a_server_takes() {
        let solid = |width: u32, height: u32| Rgba {
            width,
            height,
            pixels: vec![255; (width * height * 4) as usize],
        };
        // The team sprays' size: 112x112 is about 17.5 KB, 128x128 would be
        // over the 20480 a server takes.
        let (wad, _) = build_wad(&solid(3840, 3840)).unwrap();
        let (_, w, h) = wad_key(&wad).unwrap();
        assert_eq!((w, h), (112, 112));
        assert_eq!(wad.len(), 17516);
        assert!(wad_bytes(128, 128) > MAX_WAD_BYTES);
        // Tall: the long side as far as the file allows, the short one
        // scaled with it in steps of 16.
        let (wad, _) = build_wad(&solid(300, 1000)).unwrap();
        let (_, w, h) = wad_key(&wad).unwrap();
        assert_eq!((w, h), (64, 224));
        assert!(wad.len() <= MAX_WAD_BYTES);
        // Small pictures aren't scaled up past themselves.
        let (wad, _) = build_wad(&solid(50, 40)).unwrap();
        let (_, w, h) = wad_key(&wad).unwrap();
        assert_eq!((w, h), (48, 32));
        assert!(wad.len() >= MIN_WAD_BYTES);
        let (wad, _) = build_wad(&solid(4, 4)).unwrap();
        assert_eq!(wad_key(&wad).unwrap().1, 16);
        // Every size it picks stays inside the server's range.
        for (w, h) in [(1, 4000), (4000, 1), (256, 255), (17, 33), (1000, 999)] {
            let (lw, lh) = logo_size(w, h);
            assert!(lw % 16 == 0 && lh % 16 == 0, "{w}x{h} -> {lw}x{lh}");
            assert!(
                (MIN_WAD_BYTES..=MAX_WAD_BYTES).contains(&wad_bytes(lw, lh)),
                "{w}x{h}"
            );
        }
        assert_eq!(pot_floor(3840, 1024), 1024);
        assert_eq!(pot_floor(3840, 4096), 2048);
        assert_eq!(pot_floor(112, 256), 64);
        assert_eq!(pot_floor(0, 256), 1);
    }

    #[test]
    fn quantise_keeps_within_255_colours_and_the_key_entry() {
        let mut img = sample();
        // Every pixel a different colour: more than 255 to pick from.
        for (i, px) in img.pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            *px = [(i * 7) as u8, (i * 13) as u8, (i * 29) as u8, 255];
        }
        let logo = quantise(&img);
        assert!(logo.indices.iter().all(|&i| i != TRANSPARENT));
        assert_eq!(&logo.palette[765..], &CUTOUT_KEY);
        let used: std::collections::HashSet<u8> = logo.indices.iter().copied().collect();
        assert!(used.len() <= 255 && used.len() > 100);
    }

    #[test]
    fn the_same_picture_always_gives_the_same_key() {
        // Each run of the command maps its key, so a key that changed from
        // run to run left one stale mapping line per run.
        let mut img = Rgba {
            width: 64,
            height: 64,
            pixels: vec![0; 64 * 64 * 4],
        };
        for (i, px) in img.pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            *px = [(i * 7) as u8, (i * 13) as u8, (i * 29) as u8, 255];
        }
        let first = build_wad(&img).unwrap().1;
        for _ in 0..5 {
            assert_eq!(build_wad(&img).unwrap().1, first);
        }
    }

    #[test]
    fn resample_weights_colour_by_alpha() {
        let img = Rgba {
            width: 2,
            height: 1,
            pixels: vec![200, 0, 0, 255, 0, 0, 0, 0],
        };
        let half = resample(&img, 1, 1);
        // The transparent black doesn't darken the red; alpha averages.
        assert_eq!(half.pixels, vec![200, 0, 0, 128]);
    }

    #[test]
    fn the_mapping_file_has_two_line_shapes_and_comments() {
        let text = "// a comment\n\
                    8e506e748e506e748e506e748e506e74\tchris_teamwork.png\n\
                    FFE83715FFE83715FFE83715FFE83715 two words.png\n\
                    player\tSTEAM_0:1:6155141\tchris_teamwork.png\n\
                    player\t[U:1:12310283]\tsame.png\n\
                    player\tSome Name\tname.png\n\
                    player 76561197972310283 plain.png\n\
                    not a line\n\
                    8e506e748e506e748e506e748e506e74\t../escape.png\n";
        let m = parse_mapping(text);
        assert_eq!(m.by_key.len(), 2);
        assert_eq!(
            m.by_key[&parse_key("8e506e748e506e748e506e748e506e74").unwrap()],
            "chris_teamwork.png"
        );
        assert_eq!(
            m.by_key[&parse_key("ffe83715ffe83715ffe83715ffe83715").unwrap()],
            "two words.png"
        );
        assert_eq!(m.players.len(), 4);
        assert_eq!(m.players[0].0, PlayerId::SteamId(76561197972576011));
        assert_eq!(m.players[1].0, PlayerId::SteamId(76561197972576011));
        assert_eq!(m.players[2].0, PlayerId::Name("Some Name".into()));
        assert_eq!(m.players[3].0, PlayerId::SteamId(76561197972310283));
        assert_eq!(
            m.bad.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            vec![8, 9]
        );
        assert_eq!(png_for_player(&m, "some name", 0), Some("name.png"));
        assert_eq!(
            png_for_player(&m, "x", 76561197972576011),
            Some("chris_teamwork.png")
        );
        assert_eq!(png_for_player(&m, "x", 1), None);
    }

    #[test]
    fn steam_ids_in_every_spelling() {
        assert_eq!(parse_steam_id("STEAM_0:1:6155141"), Some(76561197972576011));
        assert_eq!(parse_steam_id("[U:1:12310283]"), Some(76561197972576011));
        assert_eq!(parse_steam_id("76561197972576011"), Some(76561197972576011));
        assert_eq!(parse_steam_id("12"), None);
        assert_eq!(parse_steam_id("m00cat"), None);
    }

    #[test]
    fn custom_names_are_the_slot_and_lump() {
        assert!(looks_like_custom_name("{00100"));
        assert!(looks_like_custom_name("}03101"));
        assert!(looks_like_custom_name("{-0200"));
        assert!(!looks_like_custom_name("{shot5"));
        assert!(!looks_like_custom_name("}scorch1"));
        assert!(!looks_like_custom_name("{0010"));
    }

    #[test]
    fn the_key_is_over_what_the_engine_draws() {
        let indices = [1u8, 2, 3, 4];
        let mut pal = vec![0u8; 768];
        pal[765..].copy_from_slice(&CUTOUT_KEY);
        let full = spray_key(2, 2, &indices, &pal);
        assert_eq!(
            full,
            md5(&[
                &2u32.to_le_bytes(),
                &2u32.to_le_bytes(),
                &indices,
                &256u16.to_le_bytes(),
                &pal
            ])
        );
        // A shorter palette hashes its own count and bytes.
        assert_ne!(spray_key(2, 2, &indices, &pal[..3 * 16]), full);
        // Height and width are not interchangeable.
        assert_ne!(
            spray_key(4, 1, &indices, &pal),
            spray_key(1, 4, &indices, &pal)
        );
    }

    #[test]
    fn decode_png_handles_rgb_and_rgba() {
        for (color, bytes_pp) in [(png::ColorType::Rgb, 3), (png::ColorType::Rgba, 4)] {
            let mut out = Vec::new();
            {
                let mut enc = png::Encoder::new(&mut out, 2, 1);
                enc.set_color(color);
                enc.set_depth(png::BitDepth::Eight);
                let mut w = enc.write_header().unwrap();
                let data: Vec<u8> = if bytes_pp == 3 {
                    vec![10, 20, 30, 40, 50, 60]
                } else {
                    vec![10, 20, 30, 128, 40, 50, 60, 255]
                };
                w.write_image_data(&data).unwrap();
            }
            let img = decode_png(&out).unwrap();
            assert_eq!((img.width, img.height), (2, 1));
            assert_eq!(&img.pixels[..3], &[10, 20, 30]);
            assert_eq!(img.pixels[3], if bytes_pp == 3 { 255 } else { 128 });
            assert_eq!(&img.pixels[4..7], &[40, 50, 60]);
        }
        assert!(decode_png(b"not a png").is_err());
    }

    #[test]
    fn patterns_parse_and_name_the_same_instructions() {
        for p in [LOAD_FROM_CUSTOM_PRE, LOAD_FROM_CUSTOM_POST] {
            assert!(crate::scan::Pattern::parse(p).is_ok());
            let toks: Vec<&str> = p.split_whitespace().collect();
            assert_eq!(toks[0], "68", "push <prefix>");
            assert_eq!(toks[5], "68", "push <szCustName>");
            assert_eq!(&toks[10..12], &["C7", "05"], "mov [flag], imm32");
            assert_eq!(&toks[16..20], &["01", "00", "00", "00"]);
            assert_eq!(toks[20], "E8");
        }
    }
}
