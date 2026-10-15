//! Which world textures a map uses, and the HD file each one gets (#426;
//! the Rust half of #372's texture scan).
//!
//! The build names a map texture's HD file `<name>_<hash>.tga`, where the
//! hash is FNV-1a over the texture's pixels: its mip-0 palette indices then
//! its 768-byte palette, exactly the bytes `hw.dll` hands `GL_LoadTexture2`.
//! The hook (`texture_hires.rs`) hashes the same bytes at load time to find
//! the file, so a map's HD coverage is a question of which of those names
//! exist in a style's folder. This module is a port of what the scripts do
//! to get the names (`tools/hd/goldsrc.py`'s `map_textures` and
//! `world_hd.py`'s key loop): read each texture the BSP's texture lump
//! lists, from the BSP when it is embedded or from the wads its worldspawn
//! names, hash it, skip what the build skips. The hashing and naming must
//! stay byte for byte what the scripts and the hook do.
//!
//! The parsing takes byte slices and a lookup closure, so the tests run on
//! made-up maps; [`read_map_keys`] and [`WadCache`] are the file reading.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Serialize;

/// World textures the build never makes (`hdcommon.SKIP`, the hook's
/// `TOOL_TEXTURES`): tool brushes the renderer never draws, and the sky,
/// drawn from `gfx/env`. Lowercase.
pub const SKIP: [&str; 9] = [
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

/// The folders a map can be in, in the order the build looks
/// (`goldsrc.bsp_path`).
const MAP_DIRS: [&str; 2] = ["dod", "dod_downloads"];

/// The folders a wad can be in, in the order the build looks
/// (`goldsrc.map_textures`).
const WAD_DIRS: [&str; 2] = ["dod", "valve"];

/// FNV-1a, 32-bit, over `parts` in order: `hdcommon.fnv1a32`, the hook's
/// `fnv1a32`.
pub fn fnv1a32(parts: &[&[u8]]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for part in parts {
        for &b in *part {
            hash ^= u32::from(b);
            hash = hash.wrapping_mul(0x0100_0193);
        }
    }
    hash
}

/// A texture name as it starts its file name: `hdcommon.file_stem_name`.
/// Lowercased, with the characters Windows refuses in a file name, and
/// control characters, as `_`.
pub fn file_stem_name(name: &str) -> String {
    name.chars()
        .flat_map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => vec!['_'],
            c if (c as u32) < 32 => vec!['_'],
            c => c.to_lowercase().collect(),
        })
        .collect()
}

/// The HD file's stem for a texture: `<name>_<hash>`, the hash in eight
/// lowercase hex digits (`world_hd.py`'s `key`).
pub fn hd_file_stem(name: &str, indices: &[u8], palette: &[u8]) -> String {
    format!(
        "{}_{:08x}",
        file_stem_name(name),
        fnv1a32(&[indices, palette])
    )
}

/// A texture as `Mod_LoadTextures` reads it: the 40-byte miptex header, then
/// the mips, then two bytes and the palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MipTex<'a> {
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// Mip 0; shorter than `width * height` when the data is cut short.
    pub indices: &'a [u8],
    /// 768 bytes, fewer when cut short.
    pub palette: &'a [u8],
}

/// The 16-byte, NUL-padded name at `at`, decoded as latin-1 like the scripts.
fn name_at(buf: &[u8], at: usize) -> Option<String> {
    let raw = buf.get(at..at + 16)?;
    let end = raw.iter().position(|&b| b == 0).unwrap_or(16);
    Some(raw[..end].iter().map(|&b| char::from(b)).collect())
}

fn u32_at(buf: &[u8], at: usize) -> Option<u32> {
    buf.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn i32_at(buf: &[u8], at: usize) -> Option<i32> {
    u32_at(buf, at).map(|v| v as i32)
}

/// `goldsrc.read_miptex`: the texture at `at`, with mip 0 at +40 and the
/// palette at +40 + (pixels / 64) * 85 + 2. Slices run to the end of `buf`
/// when it is short, as the scripts' do; the caller checks the lengths.
pub fn read_miptex(buf: &[u8], at: usize) -> Option<MipTex<'_>> {
    let name = name_at(buf, at)?;
    let width = u32_at(buf, at + 16)?;
    let height = u32_at(buf, at + 20)?;
    let pixels = (width as usize).checked_mul(height as usize)?;
    let data_at = at.checked_add(40)?;
    let indices = clip(buf, data_at, pixels);
    let palette_at = data_at
        .checked_add((pixels >> 6).checked_mul(85)?)?
        .checked_add(2)?;
    let palette = clip(buf, palette_at, 768);
    Some(MipTex {
        name,
        width,
        height,
        indices,
        palette,
    })
}

/// `buf[at..at + len]`, cut to what the buffer has.
fn clip(buf: &[u8], at: usize, len: usize) -> &[u8] {
    let start = at.min(buf.len());
    let end = at.saturating_add(len).min(buf.len());
    &buf[start..end]
}

/// The BSP header's `(offset, length)` of the entities lump and the
/// textures lump: the only two the scan reads.
pub fn scan_lumps(header: &[u8]) -> Option<[(usize, usize); 2]> {
    let lump = |index: usize| -> Option<(usize, usize)> {
        let at = 4 + index * 8;
        let offset = i32_at(header, at)?;
        let length = i32_at(header, at + 4)?;
        (offset >= 0 && length >= 0).then_some((offset as usize, length as usize))
    };
    Some([lump(0)?, lump(2)?])
}

/// Bytes the header and lump table take: the version and 15 lumps.
pub const HEADER_LEN: usize = 4 + 15 * 8;

/// The wads worldspawn lists (`"wad" "a.wad;b.wad"`), as file names:
/// the scripts drop any folder in front, as the engine does.
pub fn wad_names(entities: &[u8]) -> Vec<String> {
    let text: String = entities.iter().map(|&b| char::from(b)).collect();
    let Some(value) = key_value(&text, "wad") else {
        return Vec::new();
    };
    value
        .split(';')
        .map(|w| w.replace('\\', "/"))
        .map(|w| w.rsplit('/').next().unwrap_or("").to_string())
        .filter(|w| !w.is_empty())
        .collect()
}

/// The first `"key" "value"` in an entity lump's text, whitespace between
/// the quotes as the scripts' regex allows.
fn key_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let quoted = format!("\"{key}\"");
    let mut from = 0;
    while let Some(hit) = text[from..].find(&quoted) {
        let after = from + hit + quoted.len();
        let rest = text[after..].trim_start();
        if let Some(value) = rest.strip_prefix('"') {
            let end = value.find('"')?;
            return Some(&value[..end]);
        }
        from = after;
    }
    None
}

/// A WAD3's directory: lowercased name to `(offset, size on disk)`
/// (`goldsrc.wad_lookup`). `None` for anything that isn't a WAD3.
pub fn wad_directory(wad: &[u8]) -> Option<HashMap<String, (usize, usize)>> {
    if wad.get(..4)? != b"WAD3" {
        return None;
    }
    let count = i32_at(wad, 4)?.max(0) as usize;
    let dir_at = i32_at(wad, 8)?.max(0) as usize;
    Some(wad_entries(clip(wad, dir_at, count * 32), count))
}

/// The directory's `count` 32-byte entries, from the bytes at its offset:
/// `(offset, size on disk, size, type, compression, pad, name[16])`.
fn wad_entries(dir: &[u8], count: usize) -> HashMap<String, (usize, usize)> {
    let mut entries = HashMap::with_capacity(count.min(1 << 16));
    for i in 0..count {
        let at = 32 * i;
        let (Some(offset), Some(size), Some(name)) =
            (i32_at(dir, at), i32_at(dir, at + 4), name_at(dir, at + 16))
        else {
            break;
        };
        if offset < 0 || size < 0 {
            continue;
        }
        entries.insert(name.to_lowercase(), (offset as usize, size as usize));
    }
    entries
}

/// One entry of a BSP's texture lump.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureEntry<'a> {
    pub name: String,
    /// The texture's pixels when the BSP carries them; `None` when a wad
    /// does (mip offset 0).
    pub embedded: Option<MipTex<'a>>,
}

/// The texture lump's entries, in order. A negative offset (a texture the
/// compiler dropped) is skipped, as the scripts skip it; a lump cut short
/// ends the list.
pub fn texture_entries(lump: &[u8]) -> Vec<TextureEntry<'_>> {
    let Some(count) = i32_at(lump, 0) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for i in 0..count.max(0) as usize {
        let Some(offset) = i32_at(lump, 4 + 4 * i) else {
            break;
        };
        if offset < 0 {
            continue;
        }
        let at = offset as usize;
        let Some(name) = name_at(lump, at) else {
            break;
        };
        let embedded = match u32_at(lump, at + 24) {
            Some(0) => None,
            Some(_) => read_miptex(lump, at),
            None => break,
        };
        out.push(TextureEntry { name, embedded });
    }
    out
}

/// What a map's world textures come to: the HD file stems the build makes
/// for it, and what it leaves out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MapKeys {
    /// Every `<name>_<hash>` the map needs, each once (a texture shared by
    /// two of the map's wads is in here once per copy, as the build makes
    /// a file for each copy).
    pub keys: BTreeSet<String>,
    /// Masked textures that are all cut-out (`{blue`): the build makes no
    /// file, and the hook never looks for one.
    pub blank: BTreeSet<String>,
    /// Textures found in none of the map's wads: the build skips them, and
    /// the game shows them stock (or pink).
    pub missing: Vec<String>,
    /// The wad file names the map's worldspawn lists, in its order: the
    /// files the keys depend on besides the `.bsp`.
    pub wads: Vec<String>,
}

/// The build's keys for a map, from its texture lump and the wads its
/// worldspawn lists. `wad_texture(wad file name, lowercased texture name)`
/// gives the raw miptex bytes of that texture in that wad, or `None` when
/// the wad is absent or lacks it; it is asked once per wad, since a name in
/// more than one of the map's wads is built once per wad.
pub fn map_keys(
    textures_lump: &[u8],
    wads: &[String],
    mut wad_texture: impl FnMut(&str, &str) -> Option<Vec<u8>>,
) -> MapKeys {
    let mut out = MapKeys {
        wads: wads.to_vec(),
        ..MapKeys::default()
    };
    for entry in texture_entries(textures_lump) {
        if let Some(tex) = entry.embedded {
            add_key(&mut out, &tex);
            continue;
        }
        let lower = entry.name.to_lowercase();
        let mut found = false;
        for wad in wads {
            let Some(bytes) = wad_texture(wad, &lower) else {
                continue;
            };
            if let Some(tex) = read_miptex(&bytes, 0) {
                add_key(&mut out, &tex);
            }
            found = true;
        }
        if !found {
            out.missing.push(entry.name);
        }
    }
    out
}

/// `world_hd.py`'s key loop for one texture: what the build skips, what it
/// calls blank, and the key for the rest.
fn add_key(out: &mut MapKeys, tex: &MipTex<'_>) {
    let base = tex.name.to_lowercase();
    let pixels = (tex.width as usize).saturating_mul(tex.height as usize);
    if SKIP.contains(&base.as_str())
        || base.starts_with("sky")
        || tex.indices.len() != pixels
        || tex.palette.len() != 768
    {
        return;
    }
    let key = hd_file_stem(&tex.name, tex.indices, tex.palette);
    if tex.name.starts_with('{') && tex.indices.iter().all(|&i| i == 255) {
        out.blank.insert(key);
    } else {
        out.keys.insert(key);
    }
}

/// The install's wads, each one opened once, its directory read once and
/// its textures read on demand: the scan touches a few textures of each of
/// a hundred wads, and reading every wad whole would be hundreds of
/// megabytes.
pub struct WadCache {
    game: PathBuf,
    /// By file name; `None` for a wad in neither folder, or not a WAD3.
    wads: HashMap<String, Option<(std::fs::File, HashMap<String, (usize, usize)>)>>,
}

impl WadCache {
    /// For the install whose Half-Life folder is `game`.
    pub fn new(game: &Path) -> Self {
        Self {
            game: game.to_path_buf(),
            wads: HashMap::new(),
        }
    }

    /// Opens the wad and reads its directory: the 12-byte header, then the
    /// 32-byte entries where it says they are.
    fn open(path: &Path) -> Option<(std::fs::File, HashMap<String, (usize, usize)>)> {
        let mut file = std::fs::File::open(path).ok()?;
        let mut header = [0u8; 12];
        file.read_exact(&mut header).ok()?;
        if &header[..4] != b"WAD3" {
            return None;
        }
        let count = i32_at(&header, 4)?.max(0) as usize;
        let dir_at = i32_at(&header, 8)?.max(0) as u64;
        file.seek(SeekFrom::Start(dir_at)).ok()?;
        let mut dir = vec![0u8; count.min(1 << 20) * 32];
        file.read_exact(&mut dir).ok()?;
        Some((file, wad_entries(&dir, count)))
    }

    fn directory(
        &mut self,
        wad: &str,
    ) -> &mut Option<(std::fs::File, HashMap<String, (usize, usize)>)> {
        if !self.wads.contains_key(wad) {
            let found = wad_path(&self.game, wad).and_then(|path| Self::open(&path));
            self.wads.insert(wad.to_string(), found);
        }
        self.wads.get_mut(wad).expect("inserted above")
    }

    /// The raw miptex bytes of `name` (lowercased) in `wad`.
    pub fn texture(&mut self, wad: &str, name: &str) -> Option<Vec<u8>> {
        let (file, dir) = self.directory(wad).as_mut()?;
        let &(offset, size) = dir.get(name)?;
        file.seek(SeekFrom::Start(offset as u64)).ok()?;
        let mut bytes = vec![0u8; size.min(64 << 20)];
        file.read_exact(&mut bytes).ok()?;
        Some(bytes)
    }
}

/// Where the wad file `wad` is, looking where the build looks
/// ([`WAD_DIRS`]).
pub fn wad_path(game: &Path, wad: &str) -> Option<PathBuf> {
    WAD_DIRS
        .iter()
        .map(|dir| game.join(dir).join(wad))
        .find(|p| p.is_file())
}

/// Where `map`'s `.bsp` is, looking where the build looks ([`MAP_DIRS`]).
pub fn bsp_path(game: &Path, map: &str) -> Option<PathBuf> {
    MAP_DIRS
        .iter()
        .map(|dir| game.join(dir).join("maps").join(format!("{map}.bsp")))
        .find(|p| p.is_file())
}

/// The build's keys for the map at `bsp`. Reads the header and the two
/// lumps it needs, not the whole file: the maps' other lumps are most of
/// their size.
pub fn read_map_keys(bsp: &Path, wads: &mut WadCache) -> Result<MapKeys, String> {
    let mut file =
        std::fs::File::open(bsp).map_err(|e| crate::messages::labeled(bsp.display(), e))?;
    let mut header = [0u8; HEADER_LEN];
    file.read_exact(&mut header)
        .map_err(|e| crate::messages::labeled(bsp.display(), e))?;
    let [entities, textures] =
        scan_lumps(&header).ok_or_else(|| format!("{}: not a BSP", bsp.display()))?;
    let read_lump = |file: &mut std::fs::File, (offset, length): (usize, usize)| {
        let mut bytes = vec![0u8; length];
        file.seek(SeekFrom::Start(offset as u64))
            .and_then(|_| file.read_exact(&mut bytes))
            .map_err(|e| crate::messages::labeled(bsp.display(), e))?;
        Ok::<_, String>(bytes)
    };
    let entities = read_lump(&mut file, entities)?;
    let textures = read_lump(&mut file, textures)?;
    let wad_list = wad_names(&entities);
    Ok(map_keys(&textures, &wad_list, |wad, name| {
        wads.texture(wad, name)
    }))
}

/// How much of one map's world textures a style has built.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MapCoverage {
    pub map: String,
    /// How many HD files the map needs (its unique textures, less the ones
    /// the build skips).
    pub total: usize,
    /// How many of them each style folder has, by style name.
    pub built: BTreeMap<String, usize>,
}

/// Every map's world coverage in every style (#426): what the HD page's
/// "fully built for N maps" lines and the `hd_maps.txt` warning come from.
#[derive(Debug, Clone, Serialize, Default)]
pub struct WorldCoverage {
    /// The style folders under `world`, sorted.
    pub styles: Vec<String>,
    /// One per map, in the order given.
    pub maps: Vec<MapCoverage>,
    /// Maps whose `.bsp` could not be read, with why.
    pub unreadable: Vec<String>,
    /// How long the scan took, so the page can say when it is slow.
    pub millis: u64,
}

/// The file stems in each style folder under `world`, by style.
/// `overrides` counts for every style, since its files win over any style.
fn style_stems(world_dir: &Path) -> BTreeMap<String, HashSet<String>> {
    let Ok(entries) = std::fs::read_dir(world_dir) else {
        return BTreeMap::new();
    };
    let mut folders: BTreeMap<String, HashSet<String>> = BTreeMap::new();
    for entry in entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
    {
        let name = entry.file_name().to_string_lossy().to_string();
        folders.insert(name, tga_stems(&entry.path()));
    }
    let overrides = folders.remove(super::OVERRIDES).unwrap_or_default();
    for stems in folders.values_mut() {
        stems.extend(overrides.iter().cloned());
    }
    folders
}

/// The `.tga` files in `dir`, as lowercased stems.
fn tga_stems(dir: &Path) -> HashSet<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return HashSet::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            name.strip_suffix(".tga").map(str::to_string)
        })
        .collect()
}

/// The coverage of `maps` (names without `.bsp`, in `game`'s map folders)
/// by every style under `hd_root\world`.
pub fn world_coverage(game: &Path, hd_root: &Path, maps: &[String]) -> WorldCoverage {
    world_coverage_with(
        game,
        hd_root,
        maps,
        &mut super::map_keys_cache::KeysCache::default(),
        &mut |_, _| {},
    )
}

/// [`world_coverage`], taking each map's keys from `cache` when its files
/// are unchanged (a cold scan of every map takes seconds, a warm one
/// milliseconds), and calling `progress(maps done, maps)` after each map.
pub fn world_coverage_with(
    game: &Path,
    hd_root: &Path,
    maps: &[String],
    cache: &mut super::map_keys_cache::KeysCache,
    progress: &mut dyn FnMut(usize, usize),
) -> WorldCoverage {
    let started = std::time::Instant::now();
    let styles = style_stems(&hd_root.join("world"));
    let mut wads = WadCache::new(game);
    let mut out = WorldCoverage {
        styles: styles.keys().cloned().collect(),
        ..WorldCoverage::default()
    };
    for (done, map) in maps.iter().enumerate() {
        progress(done, maps.len());
        let keys = match bsp_path(game, map) {
            Some(path) => cache.keys(game, &path, || read_map_keys(&path, &mut wads)),
            None => Err(format!("{map}.bsp: not found")),
        };
        let keys = match keys {
            Ok(keys) => keys,
            Err(why) => {
                out.unreadable.push(why);
                continue;
            }
        };
        out.maps.push(MapCoverage {
            map: map.clone(),
            total: keys.len(),
            built: styles
                .iter()
                .map(|(style, stems)| {
                    let built = keys.iter().filter(|k| stems.contains(*k)).count();
                    (style.clone(), built)
                })
                .collect(),
        });
    }
    progress(maps.len(), maps.len());
    out.millis = started.elapsed().as_millis() as u64;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    /// A miptex with `pixels` as its mip 0 and `palette` after the three
    /// smaller mips' worth of room, as `read_miptex` expects.
    fn miptex(name: &str, width: u32, height: u32, pixels: &[u8], palette: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; 16];
        out[..name.len()].copy_from_slice(name.as_bytes());
        out.extend(width.to_le_bytes());
        out.extend(height.to_le_bytes());
        // Mip offsets: 0 means "in a wad"; anything else is embedded.
        let embedded = if pixels.is_empty() { 0u32 } else { 40 };
        for _ in 0..4 {
            out.extend(embedded.to_le_bytes());
        }
        if !pixels.is_empty() {
            let count = (width * height) as usize;
            out.extend(pixels);
            out.resize(40 + (count >> 6) * 85 + 2, 0);
            out.extend(palette);
        }
        out
    }

    /// A texture lump holding `textures`.
    fn texture_lump(textures: &[Vec<u8>]) -> Vec<u8> {
        let mut lump = (textures.len() as u32).to_le_bytes().to_vec();
        let mut at = 4 + 4 * textures.len();
        for t in textures {
            lump.extend((at as u32).to_le_bytes());
            at += t.len();
        }
        for t in textures {
            lump.extend(t);
        }
        lump
    }

    /// A BSP with just the two lumps the scan reads.
    fn bsp(entities: &str, textures: &[u8]) -> Vec<u8> {
        let mut file = vec![0u8; HEADER_LEN];
        file[..4].copy_from_slice(&30i32.to_le_bytes());
        let ents_at = HEADER_LEN;
        let tex_at = ents_at + entities.len();
        for (index, (offset, length)) in [
            (0, (ents_at, entities.len())),
            (2, (tex_at, textures.len())),
        ] {
            let at = 4 + index * 8;
            file[at..at + 4].copy_from_slice(&(offset as i32).to_le_bytes());
            file[at + 4..at + 8].copy_from_slice(&(length as i32).to_le_bytes());
        }
        file.extend(entities.as_bytes());
        file.extend(textures);
        file
    }

    /// A WAD3 holding `textures` as miptexs.
    fn wad(textures: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut file = b"WAD3".to_vec();
        let dir_at = 12 + textures.iter().map(|(_, t)| t.len()).sum::<usize>();
        file.extend((textures.len() as i32).to_le_bytes());
        file.extend((dir_at as i32).to_le_bytes());
        let mut offsets = Vec::new();
        for (_, t) in textures {
            offsets.push(file.len());
            file.extend(t);
        }
        for ((name, t), offset) in textures.iter().zip(offsets) {
            file.extend((offset as i32).to_le_bytes());
            file.extend((t.len() as i32).to_le_bytes());
            file.extend((t.len() as i32).to_le_bytes());
            file.extend([0x43, 0, 0, 0]);
            let mut n = [0u8; 16];
            n[..name.len()].copy_from_slice(name.as_bytes());
            file.extend(n);
        }
        file
    }

    /// An 8x8 texture's pixels, each one different from `seed` on.
    fn px(seed: u8) -> [u8; 64] {
        std::array::from_fn(|i| seed.wrapping_add(i as u8))
    }

    fn palette() -> Vec<u8> {
        (0..768).map(|i| (i % 256) as u8).collect()
    }

    #[test]
    fn the_hash_and_the_file_name_are_the_scripts() {
        // hdcommon.fnv1a32 (and the hook's own test values).
        assert_eq!(fnv1a32(&[b""]), 0x811c_9dc5);
        assert_eq!(fnv1a32(&[b"a"]), 0xe40c_292c);
        assert_eq!(fnv1a32(&[b"foo", b"bar"]), fnv1a32(&[b"foobar"]));
        // hdcommon.file_stem_name on the characters the maps actually use.
        assert_eq!(file_stem_name("{BLUE"), "{blue");
        assert_eq!(file_stem_name("!C2A53"), "!c2a53");
        assert_eq!(file_stem_name("+0~TNNL_LGT4"), "+0~tnnl_lgt4");
        assert_eq!(file_stem_name("a/b:c*\u{1}"), "a_b_c__");
        // The key: stem, underscore, eight hex digits.
        assert_eq!(hd_file_stem("Wall", b"", b""), "wall_811c9dc5");
        // Pinned from `python -I ref_keys.py` over these same bytes, so the
        // port can't drift from the scripts.
        assert_eq!(hd_file_stem("Wall", &px(1), &palette()), "wall_b5d43885");
    }

    #[test]
    fn the_scripts_still_hash_and_name_the_same_way() {
        let common = include_str!("../../../goldsrc-hooks/tools/hd/hdcommon.py");
        assert!(common.contains("h = ((h ^ b) * 0x01000193) & 0xFFFFFFFF"));
        assert!(common.contains(r#"'<>:"/\\|?*' or ord(c) < 32 else c.lower()"#));
        for name in SKIP {
            assert!(common.contains(&format!("\"{name}\"")), "{name}");
        }
        let world = include_str!("../../../goldsrc-hooks/tools/hd/world_hd.py");
        assert!(world.contains("key = f\"{C.file_stem_name(name)}_{C.fnv1a32(idx, pal):08x}\""));
        assert!(world.contains("base.startswith(\"sky\")"));
        assert!(world.contains("idx.count(255) == len(idx)"));
    }

    #[test]
    fn embedded_and_wad_textures_are_keyed_as_the_build_keys_them() {
        let pal = palette();
        let wall = miptex("WALL", 8, 8, &px(1), &pal);
        let in_wad = miptex("Floor", 8, 8, &[], &[]);
        let sky = miptex("sky_day", 8, 8, &px(9), &pal);
        let clip = miptex("CLIP", 8, 8, &px(9), &pal);
        let blank = miptex("{blue", 8, 8, &[255; 64], &pal);
        let lost = miptex("nowhere", 8, 8, &[], &[]);
        let lump = texture_lump(&[wall, in_wad, sky, clip, blank, lost]);
        let wads = vec!["dod_a.wad".to_string(), "dod_b.wad".to_string()];
        // The floor is in both wads with different pixels: both copies count.
        let mut asked = Vec::new();
        let keys = map_keys(&lump, &wads, |wad, name| {
            asked.push(format!("{wad}:{name}"));
            match (wad, name) {
                ("dod_a.wad", "floor") => Some(miptex("Floor", 8, 8, &px(5), &pal)),
                ("dod_b.wad", "floor") => Some(miptex("floor", 8, 8, &px(8), &pal)),
                _ => None,
            }
        });
        let expect = |name: &str, px: &[u8]| hd_file_stem(name, px, &pal);
        let mut wanted = [
            expect("floor", &px(5)),
            expect("floor", &px(8)),
            expect("wall", &px(1)),
        ];
        wanted.sort();
        assert_eq!(keys.keys.iter().cloned().collect::<Vec<_>>(), wanted);
        assert_eq!(
            keys.blank.iter().cloned().collect::<Vec<_>>(),
            [expect("{blue", &[255; 64])]
        );
        assert_eq!(keys.missing, ["nowhere"]);
        assert_eq!(
            asked,
            [
                "dod_a.wad:floor",
                "dod_b.wad:floor",
                "dod_a.wad:nowhere",
                "dod_b.wad:nowhere"
            ]
        );
    }

    #[test]
    fn a_cut_short_texture_or_lump_is_skipped_not_a_crash() {
        let pal = palette();
        let mut short = miptex("WALL", 8, 8, &px(1), &pal);
        short.truncate(short.len() - 10);
        let keys = map_keys(&texture_lump(&[short]), &[], |_, _| None);
        assert!(keys.keys.is_empty());
        // A lump that claims more entries than it has.
        let mut lump = texture_lump(&[miptex("ok", 1, 1, &[1], &pal)]);
        lump[..4].copy_from_slice(&1000u32.to_le_bytes());
        assert_eq!(texture_entries(&lump).len(), 1);
        // A texture whose offset points past the lump.
        let mut lump = texture_lump(&[miptex("ok", 1, 1, &[1], &pal)]);
        lump[4..8].copy_from_slice(&(1u32 << 30).to_le_bytes());
        assert!(texture_entries(&lump).is_empty());
        // Garbage: no entries, no panic.
        assert!(texture_entries(&[1, 2, 3]).is_empty());
        assert!(texture_entries(&[]).is_empty());
        assert_eq!(wad_directory(b"WAD2...."), None);
        assert_eq!(scan_lumps(&[0; 10]), None);
    }

    #[test]
    fn worldspawn_wads_are_file_names_from_either_folder() {
        let ents = br#"{ "classname" "worldspawn" "wad" "\half-life\valve\halflife.wad;C:/dod/dod_anzio.wad;;" }"#;
        assert_eq!(wad_names(ents), ["halflife.wad", "dod_anzio.wad"]);
        assert!(wad_names(b"{ \"skyname\" \"x\" }").is_empty());
        assert_eq!(wad_names(br#""wad"   "a.wad""#), ["a.wad"]);
    }

    #[test]
    fn a_map_is_read_from_its_bsp_and_wads_and_counted_against_each_style() {
        let dir = Scratch::new("hd_map_textures");
        let pal = palette();
        std::fs::create_dir_all(dir.join("dod").join("maps")).unwrap();
        std::fs::create_dir_all(dir.join("valve")).unwrap();
        std::fs::write(
            dir.join("valve").join("halflife.wad"),
            wad(&[("FLOOR", miptex("FLOOR", 8, 8, &px(5), &pal))]),
        )
        .unwrap();
        let lump = texture_lump(&[
            miptex("WALL", 8, 8, &px(1), &pal),
            miptex("floor", 8, 8, &[], &[]),
            miptex("{blue", 8, 8, &[255; 64], &pal),
        ]);
        std::fs::write(
            dir.join("dod").join("maps").join("dod_test.bsp"),
            bsp(r#"{ "wad" "halflife.wad" }"#, &lump),
        )
        .unwrap();
        std::fs::write(dir.join("dod").join("maps").join("dod_bad.bsp"), b"junk").unwrap();

        let wall = hd_file_stem("wall", &px(1), &pal);
        let floor = hd_file_stem("floor", &px(5), &pal);
        let root = dir.join("dod_addon").join("dodstudio_hd");
        for (folder, stem) in [
            ("ultrasharp", wall.as_str()),
            ("ultrasharp", floor.as_str()),
            ("plain", "other_00000000"),
            ("overrides", wall.as_str()),
        ] {
            let folder = root.join("world").join(folder);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join(format!("{stem}.TGA")), b"").unwrap();
        }
        std::fs::create_dir_all(root.join("world").join("remacri")).unwrap();

        let maps = [
            "dod_test".to_string(),
            "dod_bad".to_string(),
            "dod_gone".to_string(),
        ];
        let coverage = world_coverage(&dir, &root, &maps);
        assert_eq!(coverage.styles, ["plain", "remacri", "ultrasharp"]);
        assert_eq!(
            coverage.maps,
            [MapCoverage {
                map: "dod_test".into(),
                total: 2,
                built: [("ultrasharp", 2), ("plain", 1), ("remacri", 1)]
                    .into_iter()
                    .map(|(s, n)| (s.to_string(), n))
                    .collect(),
            }]
        );
        assert_eq!(coverage.unreadable.len(), 2, "{:?}", coverage.unreadable);
        assert!(coverage.unreadable[0].contains("dod_bad.bsp"));
        assert!(coverage.unreadable[1].contains("dod_gone.bsp"));
    }

    /// The port against the scripts' output on a real install: every key of
    /// a few maps has its file in the full style, and the count is the
    /// scripts' (`python -I ref_keys.py`, 2026-10-13: anzio 124, kalt 71,
    /// avalanche 116, chemille 90). `cargo test -p native -- --ignored
    /// real_maps --nocapture`; `DOD_HD_GAME` points at another install.
    #[test]
    #[ignore]
    fn real_maps_match_the_scripts_and_the_built_files() {
        let game = std::env::var_os("DOD_HD_GAME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(
                    r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - PRE-Anniversary for Movies",
                )
            });
        assert!(
            game.join("hl.exe").is_file(),
            "no install at {}",
            game.display()
        );
        let root = game.join("dod_addon").join("dodstudio_hd");
        let maps: Vec<String> = ["dod_anzio", "dod_kalt", "dod_avalanche", "dod_chemille"]
            .iter()
            .map(|m| m.to_string())
            .collect();
        let coverage = world_coverage(&game, &root, &maps);
        assert!(coverage.unreadable.is_empty(), "{:?}", coverage.unreadable);
        let totals: Vec<usize> = coverage.maps.iter().map(|m| m.total).collect();
        assert_eq!(totals, [124, 71, 116, 90]);
        for map in &coverage.maps {
            assert_eq!(map.built["ultrasharp"], map.total, "{}", map.map);
        }
        assert_eq!(coverage.maps[0].built["ultrasharpv2"], 124);
        assert_eq!(coverage.maps[1].built["ultrasharpv2"], 0);

        // Every map in the install, timed cold and from the cache: the page
        // runs this on each refresh.
        let all = super::super::map_list::available_maps(&game.join("hl.exe"));
        let names: Vec<String> = all.into_iter().map(|m| m.name).collect();
        let mut cache = super::super::map_keys_cache::KeysCache::default();
        let cold = world_coverage_with(&game, &root, &names, &mut cache, &mut |_, _| {});
        let warm = world_coverage_with(&game, &root, &names, &mut cache, &mut |_, _| {});
        assert_eq!(cold.maps, warm.maps);
        println!(
            "{} maps in {} ms cold, {} ms from the cache, {} unreadable",
            cold.maps.len(),
            cold.millis,
            warm.millis,
            cold.unreadable.len()
        );
    }
}
