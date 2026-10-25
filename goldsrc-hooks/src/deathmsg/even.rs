//! `ds_hud_deathmsg layout even`: every kill-feed weapon icon in a box of
//! one height, full magazines and bipods shown, every line one step (#1022).
//!
//! ## Why an atlas of our own
//!
//! [`super::scale`]'s rect copy is what `CHudDeathNotice::Draw` lays each
//! line out by (the anchor, the advance after the icon, the line step), so a
//! 26-row rect for every icon gives every line one step. But the engine
//! samples the texture from that same rect (`Draw_SpriteFrameHoles`:
//! texture coordinates `(rect +- 0.5) / frame size`), and Valve's 256x256
//! `weapons<N>` sheets are packed too tight for that: 27 of the 35 modelled
//! icons can't grow to 26 rows without reading the icon below. So
//! `killfeed_icons.py --even` draws every icon into a box of its own in
//! `sprites/dodstudio_killfeed.spr` and writes a rect table beside it
//! (`dodstudio_killfeed.txt`, `name frame x y w h`). This module loads both,
//! gives each `hud.txt` sprite index its box by name (`gHUD.m_rgszSpriteNames`,
//! [`super::scale::sprite_names`]), and `scale`'s icon draw then draws that
//! box of the atlas where the game drew its own rect (`hud_sprites::
//! draw_holes_instead`), while `scale`'s copy gives the line the box's size.
//!
//! ## When it draws
//!
//! Only after `layout even`, and only while everything it needs is there;
//! otherwise the game's own boxes and spacing, exactly as with `layout
//! default` (the default):
//!
//! - the sprite and the table on disk, both valid: every number in them is
//!   bounds-checked here before the engine is handed the sprite;
//! - HD on (`ds_hd_enabled 1`), and every frame of the atlas uploaded with
//!   its HD file this session (`hud_sprite_edges::atlas_all_soft`). The
//!   sprite's own pixels are a placeholder (grey boxes), so with HD off, or
//!   no atlas built for the style, nothing of it shows;
//! - the atlas loaded since the HUD last loaded its sheets (a level load can
//!   hand the same handle to another sprite);
//! - the same files the engine was first handed this session: it keeps that
//!   upload until it exits, so a rebuilt atlas needs a restart.
//!
//! It is followed every frame from `deathmsg::poll`, on the engine's thread:
//! `ds_hd_enabled 0` gives the game's layout from the next frame. The files
//! are read once per level load, never per frame.

// The loading runs only in the 32-bit DLL; a host build keeps the parsing
// and the maths for the unit tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code, unused_imports))]

use std::sync::RwLock;
use std::sync::atomic::{
    AtomicBool, AtomicI32, AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering,
};

use super::scale::WRect;
use crate::hud_sprite_edges::{ATLAS_MAX_FRAMES, ATLAS_NAME};

/// The sprite as the engine is asked for it.
const SPRITE_PATH: &str = "sprites/dodstudio_killfeed.spr";
/// The table's header word and the one version this reads.
const TABLE_VERSION: u32 = 1;
/// `MAX_SPRITE_NAME_LENGTH`: each `m_rgszSpriteNames` entry is this many
/// bytes, `strncpy`'d, so a 24-character name has no terminator
/// (`CHud::VidInit`, `client+0x21d1b`).
pub(super) const SPRITE_NAME_LEN: usize = 24;
/// A frame side past this is not ours (and `texture_hires` replaces none).
const MAX_SIDE: u32 = 4096;
/// More table lines than this is not a kill-feed table (`hud.txt` has ~200
/// entries, 42 of them `d_*`).
const MAX_BOXES: usize = 1024;
/// Files past these sizes are refused unread: two 256x256 frames are 128 KB.
const MAX_SPRITE_BYTES: u64 = 32 << 20;
const MAX_TABLE_BYTES: u64 = 1 << 20;

/// The layout `layout` sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Layout {
    /// The game's own boxes and spacing, with today's icons.
    Default,
    /// Every icon in its box of the atlas.
    Even,
}

/// One icon's box: a frame of the atlas and a rect in it (stock pixels).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct AtlasBox {
    pub frame: u32,
    pub rect: WRect,
}

/// The rect table: the atlas's frame count and each icon's box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Table {
    pub frames: u32,
    pub boxes: Vec<(String, AtlasBox)>,
}

/// What a `.spr` holds, as far as the atlas needs: each frame's size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SpriteInfo {
    pub frames: Vec<(u32, u32)>,
}

/// The argument after `layout`.
pub(super) fn parse_args(rest: &[String]) -> Result<Layout, String> {
    match rest {
        [arg] if arg.eq_ignore_ascii_case("even") => Ok(Layout::Even),
        [arg] if arg.eq_ignore_ascii_case("default") => Ok(Layout::Default),
        _ => Err("expected even or default".to_string()),
    }
}

/// Parses the rect table: `//` comments and blank lines skipped, then a
/// header `dodstudio_killfeed <version> <frames>`, then `name frame x y w h`
/// per icon. Anything off is an error naming its line, never a guess: a bad
/// table draws the game's layout, not a wrong one.
pub(super) fn parse_table(text: &str) -> Result<Table, String> {
    let mut lines = text
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l.trim()))
        .filter(|(_, l)| !l.is_empty() && !l.starts_with("//"));
    let (at, header) = lines.next().ok_or("the table is empty")?;
    let words: Vec<&str> = header.split_whitespace().collect();
    let frames = match words.as_slice() {
        [word, version, frames] if *word == ATLAS_NAME => {
            if version.parse::<u32>().ok() != Some(TABLE_VERSION) {
                return Err(format!(
                    "line {at}: table version {version}, this DLL reads {TABLE_VERSION}"
                ));
            }
            frames
                .parse::<u32>()
                .ok()
                .filter(|n| (1..=ATLAS_MAX_FRAMES).contains(n))
                .ok_or(format!(
                    "line {at}: {frames} frames, expected 1..={ATLAS_MAX_FRAMES}"
                ))?
        }
        _ => {
            return Err(format!(
                "line {at}: expected the header `{ATLAS_NAME} {TABLE_VERSION} <frames>`"
            ));
        }
    };
    let mut boxes: Vec<(String, AtlasBox)> = Vec::new();
    for (at, line) in lines {
        let words: Vec<&str> = line.split_whitespace().collect();
        let [name, rest @ ..] = words.as_slice() else {
            continue;
        };
        let nums: Option<Vec<u32>> = rest.iter().map(|w| w.parse().ok()).collect();
        let Some([frame, x, y, w, h]) = nums.and_then(|n| <[u32; 5]>::try_from(n).ok()) else {
            return Err(format!("line {at}: expected `name frame x y w h`"));
        };
        if !is_icon_name(name) {
            return Err(format!("line {at}: {name:?} is not a d_* sprite name"));
        }
        if frame >= frames {
            return Err(format!("line {at}: frame {frame}, the table has {frames}"));
        }
        let past = |at: u32, len: u32| u64::from(at) + u64::from(len) > u64::from(MAX_SIDE);
        if w == 0 || h == 0 || past(x, w) || past(y, h) {
            return Err(format!(
                "line {at}: a {w}x{h} box at {x},{y} is out of bounds"
            ));
        }
        if boxes.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)) {
            return Err(format!("line {at}: {name} is listed twice"));
        }
        if boxes.len() == MAX_BOXES {
            return Err(format!("more than {MAX_BOXES} boxes"));
        }
        boxes.push((
            name.to_string(),
            AtlasBox {
                frame,
                rect: WRect {
                    left: x as i32,
                    right: (x + w) as i32,
                    top: y as i32,
                    bottom: (y + h) as i32,
                },
            },
        ));
    }
    if boxes.is_empty() {
        return Err("the table lists no icon".to_string());
    }
    Ok(Table { frames, boxes })
}

/// A `hud.txt` kill-feed name as the table may hold it: `d_`, then up to
/// what a 24-byte `m_rgszSpriteNames` entry holds, plain ASCII.
fn is_icon_name(name: &str) -> bool {
    name.len() > 2
        && name.len() <= SPRITE_NAME_LEN
        && name.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("d_"))
        && name.bytes().all(|b| b.is_ascii_graphic())
}

fn read_i32(bytes: &[u8], at: usize) -> Option<i32> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    Some(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Checks a `.spr` the way the engine will read it, before the engine reads
/// it: `IDSP` version 2, alpha-test (the format the HD path gives soft
/// edges), a 256-colour palette, 1..=[`ATLAS_MAX_FRAMES`] single frames whose
/// pixels lie inside the file.
pub(super) fn parse_sprite(bytes: &[u8]) -> Result<SpriteInfo, String> {
    if bytes.get(..4) != Some(b"IDSP") {
        return Err("not a Half-Life sprite (no IDSP)".to_string());
    }
    let field = |at: usize| read_i32(bytes, at).ok_or("the header is cut short");
    let version = field(4)?;
    if version != 2 {
        return Err(format!("sprite version {version}, expected 2"));
    }
    let format = field(12)?;
    if format != 3 {
        return Err(format!(
            "texture format {format}, expected 3 (alpha test, as the game's weapon sheets)"
        ));
    }
    let count = field(0x1c)?;
    if !(1..=ATLAS_MAX_FRAMES as i32).contains(&count) {
        return Err(format!("{count} frames, expected 1..={ATLAS_MAX_FRAMES}"));
    }
    let colours = bytes
        .get(0x28..0x2a)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .ok_or("the header is cut short")?;
    if colours != 256 {
        return Err(format!("{colours} palette colours, expected 256"));
    }
    let mut at = 0x2a + 256 * 3;
    let mut frames = Vec::new();
    for n in 0..count {
        let kind = field(at)?;
        if kind != 0 {
            return Err(format!("frame {n} is a group, expected single frames"));
        }
        let (w, h) = (field(at + 12)?, field(at + 16)?);
        if !(1..=MAX_SIDE as i32).contains(&w) || !(1..=MAX_SIDE as i32).contains(&h) {
            return Err(format!("frame {n} is {w}x{h}"));
        }
        at += 20;
        let end = at + (w as usize) * (h as usize);
        if end > bytes.len() {
            return Err(format!("frame {n}'s pixels run past the end of the file"));
        }
        frames.push((w as u32, h as u32));
        at = end;
    }
    Ok(SpriteInfo { frames })
}

/// Whether every box of `table` lies inside its frame of `sprite`, and the
/// two agree on the frame count.
pub(super) fn check_fit(table: &Table, sprite: &SpriteInfo) -> Result<(), String> {
    if sprite.frames.len() != table.frames as usize {
        return Err(format!(
            "the table says {} frame(s), the sprite has {}",
            table.frames,
            sprite.frames.len()
        ));
    }
    for (name, b) in &table.boxes {
        let (w, h) = sprite.frames[b.frame as usize];
        if b.rect.right > w as i32 || b.rect.bottom > h as i32 {
            return Err(format!(
                "{name}'s box runs past frame {}'s {w}x{h}",
                b.frame
            ));
        }
    }
    Ok(())
}

/// What a load read, as [`UPLOADED`] keeps it: FNV-1a 64 over the sprite's
/// bytes, then the table's, never 0.
pub(super) fn files_key(sprite: &[u8], table: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in sprite.iter().chain(table) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h.max(1)
}

/// Whether files read now (`now`) may be laid out over the upload the engine
/// kept from the files it was first handed (`first`, 0 for none yet).
pub(super) fn check_unchanged(first: u64, now: u64) -> Result<(), String> {
    if first == 0 || first == now {
        return Ok(());
    }
    Err("the atlas changed on disk since the game first loaded it, and the engine keeps that first one: restart the game".to_string())
}

/// The name in one `m_rgszSpriteNames` entry: up to its first NUL, or all
/// [`SPRITE_NAME_LEN`] bytes when `strncpy` left none.
fn entry_name(entry: &[u8]) -> &[u8] {
    let end = entry.iter().position(|&b| b == 0).unwrap_or(entry.len());
    &entry[..end]
}

/// Each sprite index's box: `names` is `count` [`SPRITE_NAME_LEN`]-byte
/// entries, as `CHud` holds them; an index whose name the table lists gets
/// that box (case aside, as `GetSpriteIndex` compares), every other `None`.
pub(super) fn boxes_by_index(names: &[u8], count: usize, table: &Table) -> Vec<Option<AtlasBox>> {
    (0..count)
        .map(|i| {
            let entry = names.get(i * SPRITE_NAME_LEN..(i + 1) * SPRITE_NAME_LEN)?;
            let name = entry_name(entry);
            table
                .boxes
                .iter()
                .find(|(n, _)| n.as_bytes().eq_ignore_ascii_case(name))
                .map(|(_, b)| *b)
        })
        .collect()
}

/// The rect `scale`'s copy holds for a line drawn from `b` at scale `s`: the
/// box's size scaled, its corner the box's (the line layout reads only the
/// size; the draw reads the box itself from [`box_for`]).
pub(super) fn line_rect(b: AtlasBox, s: f32) -> WRect {
    super::scale::scale_rect(b.rect, s)
}

/// Why the even layout isn't drawing, as `status` says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum Wait {
    /// Drawing (or `layout default`).
    None = 0,
    /// The files are missing or bad: [`LOAD_ERROR`] says which.
    Files = 1,
    /// `ds_hd_enabled 0`.
    HdOff = 2,
    /// A frame went up without its HD file.
    NoHd = 3,
    /// No sprite index matched a table name, or the names aren't readable.
    NoNames = 4,
    /// The kill feed's draw isn't redirected (`scale` not installed) or the
    /// `SPR_*` hooks are missing.
    NoHooks = 5,
}

impl Wait {
    fn from_u8(v: u8) -> Wait {
        match v {
            1 => Wait::Files,
            2 => Wait::HdOff,
            3 => Wait::NoHd,
            4 => Wait::NoNames,
            5 => Wait::NoHooks,
            _ => Wait::None,
        }
    }
}

/// The layout asked for: 1 for `even`.
static WANT_EVEN: AtomicBool = AtomicBool::new(false);
/// Whether the even layout draws this frame. Written by [`poll`] and the
/// command, read per icon; all on the engine's thread.
static READY: AtomicBool = AtomicBool::new(false);
/// Why not, while [`WANT_EVEN`] and not [`READY`].
static WAIT: AtomicU8 = AtomicU8::new(0);
/// Moves whenever the boxes by index change, so `scale` rebuilds its copy;
/// [`key`] is 0 while not ready.
static MAP_GENERATION: AtomicU32 = AtomicU32::new(0);
/// The atlas's handle and the `hud_sprites::hud_generation` it was loaded
/// in (`u32::MAX`: never).
static HANDLE: AtomicI32 = AtomicI32::new(0);
static LOADED_IN: AtomicU32 = AtomicU32::new(u32::MAX);
/// The loaded sprite's frame count.
static FRAMES: AtomicU32 = AtomicU32::new(0);
/// The boxes by sprite index, a leaked `[Option<AtlasBox>]`, and what it was
/// made from: the names array and its count (0 before).
static MAP: AtomicUsize = AtomicUsize::new(0);
static MAP_LEN: AtomicUsize = AtomicUsize::new(0);
static MAP_NAMES: AtomicUsize = AtomicUsize::new(0);
static MAP_TABLE: AtomicU32 = AtomicU32::new(0);
/// How many indices got a box, for `status`.
static MATCHED: AtomicUsize = AtomicUsize::new(0);
/// The table loaded with the atlas, and a count that moves at each load.
/// Read and written at a load or a names change, never per frame.
static TABLE: RwLock<Option<Table>> = RwLock::new(None);
static TABLE_LOADS: AtomicU32 = AtomicU32::new(0);
/// What went wrong at the last load, for `status`.
static LOAD_ERROR: RwLock<String> = RwLock::new(String::new());
/// [`files_key`] of the sprite and table the engine was first handed this
/// session; 0 before. The engine keeps a sprite's first upload, HD frames
/// included, until it exits, so files rebuilt since would lay the old
/// pixels out by the new boxes: a load that finds other files is refused.
static UPLOADED: AtomicU64 = AtomicU64::new(0);

/// Whether the even layout draws this frame.
pub(super) fn ready() -> bool {
    READY.load(Ordering::Relaxed)
}

/// What `scale` keys its copy on: 0 while the even layout isn't drawing,
/// else a number that moves with the boxes.
pub(super) fn key() -> u32 {
    if ready() {
        MAP_GENERATION.load(Ordering::Relaxed) | 1
    } else {
        0
    }
}

/// The box sprite index `index` draws from while the even layout draws.
pub(super) fn box_for(index: usize) -> Option<AtlasBox> {
    if !ready() {
        return None;
    }
    let (ptr, len) = (MAP.load(Ordering::Acquire), MAP_LEN.load(Ordering::Acquire));
    if ptr == 0 || index >= len {
        return None;
    }
    // Safety: a leaked `[Option<AtlasBox>; len]`, never freed.
    unsafe { *(ptr as *const Option<AtlasBox>).add(index) }
}

/// The atlas's handle, while the even layout draws.
pub(super) fn handle() -> i32 {
    HANDLE.load(Ordering::Relaxed)
}

/// The sprite's path as the engine is given it, for `ds_map_hide_asset`.
pub(super) fn sprite_path() -> &'static str {
    SPRITE_PATH
}

/// What `status` says about the layout.
pub(super) fn describe() -> String {
    if !WANT_EVEN.load(Ordering::Relaxed) {
        return "layout = default".to_string();
    }
    if ready() {
        return format!(
            "layout = even ({} icons from {SPRITE_PATH})",
            MATCHED.load(Ordering::Relaxed)
        );
    }
    format!(
        "layout = even, waiting: {} -- the game's layout until then",
        why_waiting()
    )
}

fn why_waiting() -> String {
    match Wait::from_u8(WAIT.load(Ordering::Relaxed)) {
        Wait::None => "not loaded yet".to_string(),
        Wait::Files => LOAD_ERROR
            .read()
            .map(|e| e.clone())
            .unwrap_or_else(|_| "the atlas didn't load".to_string()),
        Wait::HdOff => "HD is off (ds_hd_enabled 0)".to_string(),
        Wait::NoHd => "the atlas went up without its HD files: none for this ds_hd_style in dodstudio_hd\\sprites (run killfeed_icons.py --even), or HD was off when it first loaded (restart the game)".to_string(),
        Wait::NoNames => "no kill-feed icon of the game's hud.txt is in the atlas's table".to_string(),
        Wait::NoHooks => "the kill feed's icon draw isn't hooked".to_string(),
    }
}

/// Makes `layout` the layout. The reply is what the console prints.
pub(super) fn apply(layout: Layout) -> Result<String, String> {
    match layout {
        Layout::Default => {
            WANT_EVEN.store(false, Ordering::Relaxed);
            READY.store(false, Ordering::Relaxed);
            WAIT.store(Wait::None as u8, Ordering::Relaxed);
            super::scale::refresh();
            Ok("layout = default, the game's own boxes and spacing".to_string())
        }
        Layout::Even => even_on(),
    }
}

/// `layout even`: the kill feed's operands and text calls ours (so a line
/// steps by the box and the names centre on it at scale 1 with no font
/// too), then the atlas loaded and followed.
#[cfg(target_arch = "x86")]
fn even_on() -> Result<String, String> {
    super::scale::ensure_installed().map_err(|why| format!("cannot lay the feed out: {why}"))?;
    super::font::ensure_installed().map_err(|why| format!("cannot centre the names: {why}"))?;
    WANT_EVEN.store(true, Ordering::Relaxed);
    // Typed again: the files are read again too, so a fixed table or a
    // first-time build counts. A rebuild of an atlas the engine already
    // uploaded is refused there (`UPLOADED`).
    LOADED_IN.store(u32::MAX, Ordering::Relaxed);
    poll();
    super::scale::refresh();
    Ok(if ready() {
        format!(
            "layout = even: every kill-feed icon in a box of one height ({} icons), from the next frame. `layout default` puts the game's back.",
            MATCHED.load(Ordering::Relaxed)
        )
    } else {
        format!(
            "layout = even, but the game's layout shows until this is fixed: {}",
            why_waiting()
        )
    })
}

#[cfg(not(target_arch = "x86"))]
fn even_on() -> Result<String, String> {
    Err("only in the 32-bit game".to_string())
}

/// Follows what the even layout needs, every frame from `deathmsg::poll`,
/// before `scale` syncs its copy. One load per level load at most; a frame
/// with nothing changed costs a few atomics.
pub(super) fn poll() {
    if !WANT_EVEN.load(Ordering::Relaxed) {
        READY.store(false, Ordering::Relaxed);
        return;
    }
    #[cfg(target_arch = "x86")]
    {
        let generation = crate::hud_sprites::hud_generation();
        if LOADED_IN.load(Ordering::Relaxed) != generation {
            LOADED_IN.store(generation, Ordering::Relaxed);
            load();
        }
        map_indices();
    }
    let wait = waiting();
    WAIT.store(wait as u8, Ordering::Relaxed);
    READY.store(wait == Wait::None, Ordering::Relaxed);
}

/// What still keeps the even layout from drawing, cheapest checks first.
fn waiting() -> Wait {
    if HANDLE.load(Ordering::Relaxed) == 0 {
        return Wait::Files;
    }
    if !crate::texture_hires::enabled() {
        return Wait::HdOff;
    }
    if !crate::hud_sprite_edges::atlas_all_soft(FRAMES.load(Ordering::Relaxed)) {
        return Wait::NoHd;
    }
    if !crate::hud_sprites::installed() || !super::scale::installed() {
        return Wait::NoHooks;
    }
    if MATCHED.load(Ordering::Relaxed) == 0 {
        return Wait::NoNames;
    }
    Wait::None
}

#[cfg(target_arch = "x86")]
fn report(message: &str) {
    unsafe { crate::debug::report(&format!("deathmsg layout: {message}")) };
}

/// Where the engine finds the sprite: the `-addons` folder first when the
/// game runs with it, then the game folder. The table is read from beside
/// the sprite the engine loads.
#[cfg(target_arch = "x86")]
fn locate() -> Result<std::path::PathBuf, String> {
    let file = format!("{ATLAS_NAME}.spr");
    let mut dirs = Vec::new();
    if crate::texture_hires::addons_on() {
        dirs.push(
            crate::game_folder::game_root()
                .join(crate::game_folder::addon_name())
                .join("sprites"),
        );
    }
    dirs.push(crate::game_folder::mod_dir().join("sprites"));
    dirs.iter()
        .map(|d| d.join(&file))
        .find(|p| p.is_file())
        .ok_or_else(|| {
            format!(
                "no sprites\\{file} in {} (build it with killfeed_icons.py --even)",
                dirs.iter()
                    .map(|d| d.display().to_string())
                    .collect::<Vec<_>>()
                    .join(" or ")
            )
        })
}

/// Reads a file of at most `max` bytes.
#[cfg(target_arch = "x86")]
fn read_capped(path: &std::path::Path, max: u64) -> Result<Vec<u8>, String> {
    let len = std::fs::metadata(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .len();
    if len > max {
        return Err(format!(
            "{} is {len} bytes, more than {max}",
            path.display()
        ));
    }
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Loads the sprite and its table, after checking both. Clears the handle
/// on any failure, which keeps the game's layout.
#[cfg(target_arch = "x86")]
fn load() {
    HANDLE.store(0, Ordering::Relaxed);
    let result = (|| -> Result<(i32, Table, u32), String> {
        if !crate::hud_sprites::installed() {
            return Err("the HUD's sprite hooks aren't in".to_string());
        }
        let spr = locate()?;
        let txt = spr.with_extension("txt");
        let spr_bytes = read_capped(&spr, MAX_SPRITE_BYTES)?;
        let sprite = parse_sprite(&spr_bytes).map_err(|why| format!("{}: {why}", spr.display()))?;
        let bytes = read_capped(&txt, MAX_TABLE_BYTES)?;
        let text =
            std::str::from_utf8(&bytes).map_err(|_| format!("{} is not text", txt.display()))?;
        let table = parse_table(text).map_err(|why| format!("{}: {why}", txt.display()))?;
        check_fit(&table, &sprite).map_err(|why| format!("{}: {why}", txt.display()))?;
        let key = files_key(&spr_bytes, &bytes);
        check_unchanged(UPLOADED.load(Ordering::Relaxed), key)?;
        let handle = crate::hud_sprites::load_sprite(SPRITE_PATH);
        if handle == 0 {
            return Err(format!("the engine did not load {SPRITE_PATH}"));
        }
        if UPLOADED.load(Ordering::Relaxed) == 0 {
            UPLOADED.store(key, Ordering::Relaxed);
        }
        Ok((handle, table, sprite.frames.len() as u32))
    })();
    match result {
        Ok((handle, table, frames)) => {
            report(&format!(
                "{SPRITE_PATH} loaded as handle {handle}: {} box(es) in {frames} frame(s)",
                table.boxes.len()
            ));
            if let Ok(mut t) = TABLE.write() {
                *t = Some(table);
            }
            TABLE_LOADS.fetch_add(1, Ordering::Relaxed);
            FRAMES.store(frames, Ordering::Relaxed);
            HANDLE.store(handle, Ordering::Relaxed);
            if let Ok(mut e) = LOAD_ERROR.write() {
                e.clear();
            }
        }
        Err(why) => {
            report(&format!("{why}; the game's layout stays"));
            if let Ok(mut e) = LOAD_ERROR.write() {
                *e = why;
            }
        }
    }
}

/// Rebuilds the boxes by index when the names array, its count or the table
/// changed since the last build.
#[cfg(target_arch = "x86")]
fn map_indices() {
    let Some((names, count)) = super::scale::sprite_names() else {
        return;
    };
    let table_loads = TABLE_LOADS.load(Ordering::Relaxed);
    if MAP_NAMES.load(Ordering::Relaxed) == names
        && MAP_LEN.load(Ordering::Relaxed) == count
        && MAP_TABLE.load(Ordering::Relaxed) == table_loads
        && MAP.load(Ordering::Relaxed) != 0
    {
        return;
    }
    let Ok(guard) = TABLE.read() else {
        return;
    };
    let Some(table) = guard.as_ref() else {
        return;
    };
    let span = count * SPRITE_NAME_LEN;
    if names == 0 || count == 0 || !crate::crash::readable(names, span) {
        return;
    }
    // Safety: `CHud`'s `new[24 * m_iSpriteCount]` names array, checked
    // readable over exactly that span.
    let bytes = unsafe { std::slice::from_raw_parts(names as *const u8, span) };
    let map = boxes_by_index(bytes, count, table);
    let matched = map.iter().flatten().count();
    let ptr = Box::leak(map.into_boxed_slice()).as_ptr() as usize;
    // The old map is leaked rather than freed: one per level load at most,
    // and nothing then reads freed memory.
    MAP.store(ptr, Ordering::Release);
    MAP_LEN.store(count, Ordering::Release);
    MAP_NAMES.store(names, Ordering::Relaxed);
    MAP_TABLE.store(table_loads, Ordering::Relaxed);
    MATCHED.store(matched, Ordering::Relaxed);
    MAP_GENERATION.fetch_add(2, Ordering::Relaxed);
    report(&format!(
        "{matched} of the HUD's {count} sprites draw from the atlas"
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    /// The table `killfeed_icons.py --even` wrote from the POST install,
    /// cut to three icons.
    const TABLE_TEXT: &str = "// DoD Studio's kill-feed atlas (#1022), written by killfeed_icons.py --even.\n\
        // ds_hud_deathmsg layout even draws each icon from sprites/dodstudio_killfeed.spr: name frame x y w h.\n\
        dodstudio_killfeed 1 2\n\
        d_30cal 0 156 226 71 26\n\
        d_mp44 1 2 2 59 26\n\
        d_world 1 42 58 37 26\n";

    fn sprite(frames: &[(i32, i32)]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"IDSP");
        for v in [2i32, 2, 3] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&181.0f32.to_le_bytes());
        for v in [256i32, 256, frames.len() as i32] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&0.0f32.to_le_bytes());
        b.extend_from_slice(&1i32.to_le_bytes());
        b.extend_from_slice(&256i16.to_le_bytes());
        b.extend_from_slice(&[0u8; 768]);
        for &(w, h) in frames {
            for v in [0, -w / 2, h / 2, w, h] {
                b.extend_from_slice(&v.to_le_bytes());
            }
            b.extend(std::iter::repeat_n(255u8, (w * h) as usize));
        }
        b
    }

    #[test]
    fn layout_arguments_parse_as_typed() {
        assert_eq!(parse_args(&argv(&["even"])), Ok(Layout::Even));
        assert_eq!(parse_args(&argv(&["EVEN"])), Ok(Layout::Even));
        assert_eq!(parse_args(&argv(&["default"])), Ok(Layout::Default));
        assert!(parse_args(&argv(&[])).is_err());
        assert!(parse_args(&argv(&["uniform"])).is_err());
        assert!(parse_args(&argv(&["even", "now"])).is_err());
    }

    #[test]
    fn the_table_reads_as_the_tool_writes_it() {
        let t = parse_table(TABLE_TEXT).unwrap();
        assert_eq!(t.frames, 2);
        assert_eq!(t.boxes.len(), 3);
        assert_eq!(
            t.boxes[1],
            (
                "d_mp44".to_string(),
                AtlasBox {
                    frame: 1,
                    rect: WRect {
                        left: 2,
                        right: 61,
                        top: 2,
                        bottom: 28
                    }
                }
            )
        );
    }

    #[test]
    fn a_bad_table_is_refused_with_its_line() {
        let bad = |body: &str| parse_table(&format!("dodstudio_killfeed 1 2\n{body}"));
        assert!(parse_table("").is_err());
        assert!(parse_table("// only a comment\n").is_err());
        assert!(
            parse_table("dodstudio_killfeed 2 1\nd_kar 0 0 0 1 1\n").is_err(),
            "a version this DLL doesn't read"
        );
        assert!(
            parse_table("dodstudio_killfeed 1 0\nd_kar 0 0 0 1 1\n").is_err(),
            "no frames"
        );
        assert!(
            parse_table("dodstudio_killfeed 1 33\nd_kar 0 0 0 1 1\n").is_err(),
            "more frames than the bits"
        );
        assert!(parse_table("hud 1 1\nd_kar 0 0 0 1 1\n").is_err());
        assert!(bad("").is_err(), "no boxes");
        assert!(bad("d_kar 2 0 0 10 10").is_err(), "frame past the count");
        assert!(bad("d_kar 0 0 0 0 10").is_err(), "empty box");
        assert!(bad("d_kar 0 4090 0 10 10").is_err(), "past any frame");
        assert!(bad("d_kar 0 -1 0 10 10").is_err(), "negative");
        assert!(bad("d_kar 0 0 0 10").is_err(), "a number short");
        assert!(bad("d_kar 0 0 0 10 10 3").is_err(), "a number over");
        assert!(bad("d_kar 0 0 0 10 x").is_err());
        assert!(bad("weapon_kar 0 0 0 10 10").is_err(), "not a d_ name");
        assert!(
            bad("d_aaaaaaaaaaaaaaaaaaaaaaaa 0 0 0 10 10").is_err(),
            "longer than 24 bytes"
        );
        assert!(
            bad("d_kar 0 0 0 10 10\nD_KAR 1 0 0 10 10").is_err(),
            "twice"
        );
        let err = bad("d_kar 0 0 0 10 10\nd_mp40 9 0 0 10 10").unwrap_err();
        assert!(err.starts_with("line 3:"), "{err}");
    }

    #[test]
    fn the_sprite_is_checked_before_the_engine_reads_it() {
        let ok = sprite(&[(256, 256), (256, 256)]);
        assert_eq!(
            parse_sprite(&ok),
            Ok(SpriteInfo {
                frames: vec![(256, 256), (256, 256)]
            })
        );
        assert!(parse_sprite(b"IDSQ").is_err());
        assert!(parse_sprite(&ok[..0x20]).is_err(), "header cut short");
        assert!(
            parse_sprite(&ok[..ok.len() - 1]).is_err(),
            "pixels cut short"
        );
        let mut additive = ok.clone();
        additive[12] = 2;
        assert!(parse_sprite(&additive).is_err(), "not alpha test");
        let mut grouped = ok.clone();
        grouped[0x2a + 768] = 1;
        assert!(parse_sprite(&grouped).is_err(), "a frame group");
        let mut huge = ok.clone();
        huge[0x2a + 768 + 12..0x2a + 768 + 16].copy_from_slice(&i32::MAX.to_le_bytes());
        assert!(parse_sprite(&huge).is_err(), "a frame too wide");
        let mut none = ok.clone();
        none[0x1c..0x20].copy_from_slice(&0i32.to_le_bytes());
        assert!(parse_sprite(&none).is_err(), "no frames");
        assert!(
            parse_sprite(&sprite(&[(1, 1); 33])).is_err(),
            "more frames than the bits"
        );
    }

    #[test]
    fn the_table_must_fit_the_sprite() {
        let t = parse_table(TABLE_TEXT).unwrap();
        let two = parse_sprite(&sprite(&[(256, 256), (256, 256)])).unwrap();
        assert_eq!(check_fit(&t, &two), Ok(()));
        let one = parse_sprite(&sprite(&[(256, 256)])).unwrap();
        assert!(check_fit(&t, &one).is_err(), "frame count differs");
        let small = parse_sprite(&sprite(&[(256, 256), (60, 60)])).unwrap();
        assert!(
            check_fit(&t, &small).is_err(),
            "d_world's box runs off frame 1"
        );
    }

    #[test]
    fn each_sprite_index_gets_its_box_by_name() {
        let t = parse_table(TABLE_TEXT).unwrap();
        let mut names = vec![0u8; 5 * SPRITE_NAME_LEN];
        for (i, n) in ["number_0", "d_mp44", "D_30CAL", "d_world", "d_skull"]
            .iter()
            .enumerate()
        {
            names[i * SPRITE_NAME_LEN..i * SPRITE_NAME_LEN + n.len()].copy_from_slice(n.as_bytes());
        }
        let map = boxes_by_index(&names, 5, &t);
        assert_eq!(map.len(), 5);
        assert_eq!(map[0], None, "not in the table");
        assert_eq!(map[1], Some(t.boxes[1].1));
        assert_eq!(map[2], Some(t.boxes[0].1), "case aside");
        assert_eq!(map[3], Some(t.boxes[2].1));
        assert_eq!(map[4], None, "d_skull isn't in this table");
        // A count past the array gives no box rather than a read past it.
        assert_eq!(boxes_by_index(&names, 7, &t)[6], None);
    }

    #[test]
    fn an_atlas_rebuilt_after_the_engine_uploaded_it_is_refused() {
        let spr = sprite(&[(256, 256)]);
        let first = files_key(&spr, TABLE_TEXT.as_bytes());
        assert_ne!(first, 0, "0 means nothing uploaded yet");
        assert_eq!(check_unchanged(0, first), Ok(()), "the first load");
        assert_eq!(
            check_unchanged(first, first),
            Ok(()),
            "the same files again"
        );
        // Two boxes swapped in the table, the sprite's bytes the same: the
        // kept pixels would draw the wrong icons.
        let swapped = TABLE_TEXT
            .replace("d_mp44", "d_tmp")
            .replace("d_30cal", "d_mp44");
        let later = files_key(&spr, swapped.as_bytes());
        assert!(check_unchanged(first, later).is_err());
        let mut repacked = spr.clone();
        *repacked.last_mut().unwrap() = 0;
        assert!(check_unchanged(first, files_key(&repacked, TABLE_TEXT.as_bytes())).is_err());
    }

    #[test]
    fn a_24_byte_name_has_no_terminator() {
        let full = [b'd'; SPRITE_NAME_LEN];
        assert_eq!(entry_name(&full).len(), SPRITE_NAME_LEN);
        assert_eq!(entry_name(b"d_kar\0junk"), b"d_kar");
    }

    #[test]
    fn every_line_steps_the_same_whatever_the_icon() {
        // The line step is the copy's height (font.rs's stub), so 26 rows for
        // every box: 65 px at scale 2.5, 26 at 1.
        let t = parse_table(TABLE_TEXT).unwrap();
        for s in [1.0f32, 2.5, 3.0] {
            let steps: Vec<i32> = t
                .boxes
                .iter()
                .map(|(_, b)| {
                    let r = line_rect(*b, s);
                    r.bottom - r.top
                })
                .collect();
            assert!(steps.iter().all(|&h| h == steps[0]), "scale {s}: {steps:?}");
        }
        let mp44 = line_rect(t.boxes[1].1, 2.5);
        assert_eq!((mp44.right - mp44.left, mp44.bottom - mp44.top), (148, 65));
        assert_eq!(super::super::scale::line_step(65, 33), 65);
    }

    #[test]
    fn the_default_layout_draws_nothing_of_the_atlas() {
        assert_eq!(describe(), "layout = default");
        assert!(!ready());
        assert_eq!(key(), 0);
        assert_eq!(box_for(0), None);
        poll();
        assert!(!ready(), "nothing to follow without `layout even`");
    }
}
