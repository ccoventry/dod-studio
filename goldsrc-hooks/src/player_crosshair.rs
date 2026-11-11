//! Each player's crosshair in a demo, from Studio's Players table (#428).
//!
//! ## Which player
//!
//! The one whose view is on screen: the player the in-eye camera is on
//! (`spectator_target::in_eye_target`) in an HLTV demo, and in a POV demo
//! while the dead recorder watches a teammate in first person (#1321); else,
//! in a POV demo, the recorder (the local player). Free and chase cameras draw no
//! crosshair, so nothing is decided for them. Outside a demo, and while
//! `ds_player_overrides` is 0, the game's own crosshair stays.
//!
//! ## How, without a new patch
//!
//! `CHudDoDCrossHair::Draw` draws its crosshair with the engine's `SPR_Set`
//! and one `SPR_Draw*` call (`docs/goldsrc_crosshair.md`): in POV, a 64x64
//! tile of `customXHair.spr` picked by the viewer's `cl_xhair_style`
//! (`client+0x2ced0`); spectating in-eye, the 24x24 rect of `crosshairs.spr`
//! (`client+0x2d1f0`), or with `ds_spec_match_pov` the same
//! `customXHair.spr` tile as POV (`spectator_crosshair.rs`). `hud_sprites.rs`
//! already sits on those calls in client.dll's own `gEngfuncs` copy (#808),
//! the same pointers on every install since client.dll is byte-identical
//! across them, so nothing in `hw.dll` is touched and nothing needs a
//! per-build table. When a draw is the crosshair's -- any rect of
//! `customXHair.spr`, or exactly the 24x24 spectator rect of
//! `crosshairs.spr` while in-eye -- the hook sets the table's sprite instead,
//! draws its tile ([`spectator_crosshair::tile_rect`], DoD's own 4x4 grid of
//! 64x64), moved so it stays centred where the game centred its own, and
//! sets the game's sprite back for whatever draws next.
//!
//! When the crosshair shows is still the game's alone: `ShouldDrawCrossHair`,
//! `ds_hud_hide_crosshair` (which stubs `Draw`, so no draw arrives here)
//! and the spectator gate of `ds_spec_match_pov` decide whether a
//! draw happens; this only changes what it draws. In POV the swap needs the
//! viewer's `cl_xhair_style` at 1..16: style 0 sends DoD to its classic
//! dynamic crosshair (`client+0x2cda0`), which draws no `customXHair.spr`
//! tile to swap.
//!
//! ## Cost on the draw path
//!
//! [`poll`] works out, once a frame from `commands::poll`, each player's
//! row and the sprite's handle (loaded with the engine's own `SPR_Load` the
//! first time it is needed after each level load) into one slot per player:
//! the player on screen every frame, the others one a frame in turn. The
//! draw hooks ([`swap`]) read the camera's mode and target from client.dll
//! as they draw, and that player's slot: atomics and two reads, no lock.
//!
//! Who is on screen is read at the draw, not taken from [`poll`], because the
//! spectator mode can change after `HUD_Frame` in the same frame: the first
//! frame of an in-eye view then drew the game's spectator "+" (#1321).

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering};

use crate::hud_sprites::Wrect;
use crate::player_table::Crosshair;

/// Entity numbers 1..=32, and 0 unused.
const SLOTS: usize = 33;
/// The sprite handle each player's crosshair is drawn with, 0 for the
/// game's own; by entity number.
static SLOT_HANDLE: [AtomicI32; SLOTS] = [const { AtomicI32::new(0) }; SLOTS];
/// The tile, 1..=16, while the slot's handle is set.
static SLOT_TILE: [AtomicI32; SLOTS] = [const { AtomicI32::new(0) }; SLOTS];
/// The [`GENERATION`] the slot's handle was loaded in: a handle from before
/// a level load may name another sprite now, so it isn't drawn.
static SLOT_GEN: [AtomicU32; SLOTS] = [const { AtomicU32::new(0) }; SLOTS];
/// The slot [`poll`] looks at next, besides the player on screen.
static SWEEP: AtomicUsize = AtomicUsize::new(0);
/// Whether the table gives crosshairs at all: `ds_player_overrides` on, a
/// demo playing, and a crosshair row or fallback in the table.
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// The recorder's entity number in a POV demo; 0 in an HLTV demo.
static RECORDER: AtomicI32 = AtomicI32::new(0);
/// The handle for the player on screen as [`poll`] last saw it, for
/// [`overriding`]; 0 for the game's own.
static HANDLE: AtomicI32 = AtomicI32::new(0);
/// Draws swapped this session, for the status line.
static SWAPPED: AtomicU32 = AtomicU32::new(0);

/// Bumped by `hud_sprites` each time the HUD loads its sprites again (a
/// level load), which can hand our handles to other sprites.
static GENERATION: AtomicU32 = AtomicU32::new(0);

/// Sprites loaded for the crosshair, by file name, in the generation they
/// were loaded in; handle 0 for one that can't be used (said once). Only
/// [`poll`] touches it, once a frame on the game's thread.
static LOADED: Mutex<(u32, Vec<(String, i32)>)> = Mutex::new((0, Vec::new()));
/// What the last frame decided, for the status line: changed rarely.
static LAST: Mutex<String> = Mutex::new(String::new());

/// The rect of `crosshairs.spr` the spectator path draws stock
/// (`client+0x2d205`).
const SPECTATOR_RECT: Wrect = Wrect {
    left: 24,
    right: 48,
    top: 0,
    bottom: 24,
};

/// Which crosshair sheet a sprite path is, for `hud_sprites`' `SPR_Set`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Sheet {
    Other = 0,
    /// `sprites/customXHair.spr`, the POV (and matched spectator) crosshair.
    Custom = 1,
    /// `sprites/crosshairs.spr`, the stock spectator crosshair.
    Stock = 2,
    /// `sprites/dodcross.spr`, POV's red crosshair. Never swapped: it has no
    /// tile for the table's crosshair to replace.
    Red = 3,
}

impl Sheet {
    pub fn of(path: &str) -> Sheet {
        let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
        if file.eq_ignore_ascii_case("customXHair.spr") {
            Sheet::Custom
        } else if file.eq_ignore_ascii_case("crosshairs.spr") {
            Sheet::Stock
        } else if file.eq_ignore_ascii_case("dodcross.spr") {
            Sheet::Red
        } else {
            Sheet::Other
        }
    }

    pub fn from_u8(v: u8) -> Sheet {
        match v {
            1 => Sheet::Custom,
            2 => Sheet::Stock,
            3 => Sheet::Red,
            _ => Sheet::Other,
        }
    }
}

/// Called by `hud_sprites` when the HUD loads `customXHair.spr`, which it
/// does in every `VidInit`.
pub fn hud_reloaded() {
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// Whether the Players table gives the player on screen a crosshair this
/// frame, so the game's own crosshair draws are swapped for it.
pub fn overriding() -> bool {
    HANDLE.load(Ordering::Relaxed) != 0
}

/// Whether a draw of `sheet` with `rect` is the crosshair's.
pub fn is_crosshair_draw(sheet: Sheet, rect: &Wrect, in_eye: bool) -> bool {
    match sheet {
        Sheet::Custom => true,
        Sheet::Stock => in_eye && *rect == SPECTATOR_RECT,
        Sheet::Red | Sheet::Other => false,
    }
}

/// A tile as the engine's `wrect_t`.
pub fn tile_wrect(tile: i32) -> Option<Wrect> {
    if !(1..=crate::spectator_crosshair::MAX_STYLE).contains(&tile) {
        return None;
    }
    let [left, top, right, bottom] = crate::spectator_crosshair::tile_rect(tile)?;
    Some(Wrect {
        left,
        right,
        top,
        bottom,
    })
}

/// Where to draw `new` so its centre lands on the centre the game gave
/// `old` at `x`, `y` (both crosshair paths centre their rect on the screen).
pub fn recentre(old: &Wrect, new: &Wrect, x: i32, y: i32) -> (i32, i32) {
    let w = |r: &Wrect| r.right - r.left;
    let h = |r: &Wrect| r.bottom - r.top;
    (x + (w(old) - w(new)) / 2, y + (h(old) - h(new)) / 2)
}

/// From a draw hook: the handle, rect and position to draw the crosshair
/// with instead, or `None` to draw what the game asked for. Atomics only.
///
/// # Safety
///
/// `prc` is the rect pointer the game passed to `SPR_Draw*`, null or valid.
pub unsafe fn swap(
    sheet: Sheet,
    prc: *const Wrect,
    x: i32,
    y: i32,
) -> Option<(i32, Wrect, i32, i32)> {
    if !ACTIVE.load(Ordering::Relaxed) || sheet == Sheet::Other || prc.is_null() {
        return None;
    }
    let mode = crate::spectator_target::mode();
    let target = crate::spectator_target::in_eye_target();
    let slot = on_screen(mode, target, RECORDER.load(Ordering::Relaxed))?;
    let handle = SLOT_HANDLE[slot].load(Ordering::Relaxed);
    if handle == 0 || SLOT_GEN[slot].load(Ordering::Relaxed) != GENERATION.load(Ordering::Relaxed) {
        return None;
    }
    let old = unsafe { *prc };
    if !is_crosshair_draw(sheet, &old, mode == Some(4)) {
        return None;
    }
    let new = tile_wrect(SLOT_TILE[slot].load(Ordering::Relaxed))?;
    let (x, y) = recentre(&old, &new, x, y);
    SWAPPED.fetch_add(1, Ordering::Relaxed);
    Some((handle, new, x, y))
}

/// Whether `bytes` start like a Half-Life sprite (`IDSP`, version 2): one
/// the engine's loader takes without an error dialog.
fn looks_like_sprite(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && &bytes[..4] == b"IDSP" && bytes[4..8] == 2i32.to_le_bytes()
}

/// The sprite's file on disk, the `-addons` folder's copy first (it wins in
/// the engine's search path), checked before the engine is asked to load it:
/// a missing or broken file is said once and the game's crosshair stays.
fn sprite_on_disk(file: &str) -> Result<(), String> {
    let root = crate::game_folder::game_root();
    let candidates = [
        root.join(crate::game_folder::addon_name())
            .join("sprites")
            .join(file),
        crate::game_folder::mod_dir().join("sprites").join(file),
    ];
    for path in &candidates {
        let Ok(mut f) = std::fs::File::open(path) else {
            continue;
        };
        let mut head = [0u8; 8];
        use std::io::Read;
        return if f.read_exact(&mut head).is_ok() && looks_like_sprite(&head) {
            Ok(())
        } else {
            Err(format!("{} is not a Half-Life sprite", path.display()))
        };
    }
    Err(format!("no sprites\\{file} in the game folder"))
}

/// The handle for `file`, loading it once per level.
fn handle_for(file: &str) -> i32 {
    let Ok(mut loaded) = LOADED.lock() else {
        return 0;
    };
    let generation = GENERATION.load(Ordering::Relaxed);
    if loaded.0 != generation {
        *loaded = (generation, Vec::new());
    }
    if let Some((_, handle)) = loaded.1.iter().find(|(f, _)| f.eq_ignore_ascii_case(file)) {
        return *handle;
    }
    let handle = match sprite_on_disk(file) {
        Ok(()) => crate::hud_sprites::load_sprite(&format!("sprites/{file}")),
        Err(why) => {
            unsafe {
                crate::debug::report(&format!(
                    "player_crosshair: {why}; the game's crosshair stays"
                ))
            };
            0
        }
    };
    if handle == 0 {
        unsafe {
            crate::debug::report(&format!(
                "player_crosshair: sprites/{file} did not load; the game's crosshair stays"
            ))
        };
    }
    loaded.1.push((file.to_string(), handle));
    handle
}

/// The entity whose view is on screen: the in-eye target whenever the
/// in-eye camera is up (`mode` 4), else the recorder (0 in an HLTV demo,
/// where no other camera draws a crosshair).
///
/// The in-eye camera comes first in POV too: a dead recorder watching a
/// teammate in first person has the teammate's view on screen, and the
/// recorder's own crosshair over it was the wrong one (#1321).
/// `g_iUser1`/`g_iUser2` follow the recorder's observer mode and target in
/// POV playback, as the server sent them.
fn on_screen(mode: Option<i32>, in_eye_target: Option<i32>, recorder: i32) -> Option<usize> {
    let entity = match in_eye_target {
        Some(target) if mode == Some(4) => target,
        _ => recorder,
    };
    usize::try_from(entity)
        .ok()
        .filter(|e| (1..SLOTS).contains(e))
}

/// The local player's entity number: the recorder in a POV demo.
fn local_player() -> Option<i32> {
    let engfuncs = crate::engine::engfuncs()?;
    // Safety: the engine's own call; the result is the local player's
    // `cl_entity_t`, `index` first, or null with no level loaded.
    let local = unsafe { (engfuncs.get_local_player)() } as *const i32;
    if local.is_null() || !crate::crash::readable(local as usize, 4) {
        return None;
    }
    Some(unsafe { *local }).filter(|i| (1..SLOTS as i32).contains(i))
}

/// Works out one player's crosshair into their slot; returns the status
/// text for them (empty with nobody in the slot).
fn refresh(table: &crate::player_table::Table, slot: usize) -> String {
    let generation = GENERATION.load(Ordering::Relaxed);
    let (handle, tile, text) = match crate::spray_hltv::player(slot - 1) {
        None => (0, 0, String::new()),
        Some((name, steam_id)) => match table.choose_crosshair(&name, steam_id) {
            Crosshair::Tile { sprite, tile } => {
                let handle = handle_for(&sprite);
                let text = if handle == 0 {
                    format!("{name}: tile {tile} of {sprite}, which did not load; the game's own")
                } else {
                    format!("{name}: tile {tile} of {sprite}")
                };
                (handle, i32::from(tile), text)
            }
            Crosshair::Game => (0, 0, format!("{name}: the game's own")),
        },
    };
    SLOT_TILE[slot].store(tile, Ordering::Relaxed);
    SLOT_GEN[slot].store(generation, Ordering::Relaxed);
    SLOT_HANDLE[slot].store(handle, Ordering::Relaxed);
    text
}

/// Once a frame, from `commands::poll`, on the game's thread: the player on
/// screen, and one other slot in turn, so a camera switch finds the next
/// player's crosshair ready (within 32 frames of a table change or a player
/// joining).
pub fn poll() {
    let table = crate::spray_hltv::current_table().filter(|t| t.crosshair_active());
    let table = match table {
        Some(t) if crate::player_overrides::on() && crate::spray_hltv::playing_demo() => t,
        _ => {
            ACTIVE.store(false, Ordering::Relaxed);
            HANDLE.store(0, Ordering::Relaxed);
            set_last(String::new());
            return;
        }
    };
    let recorder = if crate::spray_hltv::hltv() {
        0
    } else {
        local_player().unwrap_or(0)
    };
    RECORDER.store(recorder, Ordering::Relaxed);
    let next = SWEEP.fetch_add(1, Ordering::Relaxed) % (SLOTS - 1) + 1;
    let shown = on_screen(
        crate::spectator_target::mode(),
        crate::spectator_target::in_eye_target(),
        recorder,
    );
    if shown != Some(next) {
        refresh(&table, next);
    }
    let text = shown.map_or_else(String::new, |slot| refresh(&table, slot));
    HANDLE.store(
        shown.map_or(0, |slot| SLOT_HANDLE[slot].load(Ordering::Relaxed)),
        Ordering::Relaxed,
    );
    ACTIVE.store(true, Ordering::Relaxed);
    set_last(text);
}

fn set_last(text: String) {
    if let Ok(mut last) = LAST.lock()
        && *last != text
    {
        *last = text;
    }
}

/// The `ds_debug_status` line, while the table gives a crosshair.
pub fn status_line() -> Option<String> {
    let last = LAST.lock().ok()?.clone();
    (!last.is_empty()).then(|| {
        format!(
            "player crosshair: {last} ({} draw(s) swapped this session)",
            SWAPPED.load(Ordering::Relaxed)
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Wrect {
        Wrect {
            left,
            right,
            top,
            bottom,
        }
    }

    #[test]
    fn tiles_are_dods_grid_in_wrect_order() {
        assert_eq!(tile_wrect(1), Some(rect(0, 0, 64, 64)));
        assert_eq!(tile_wrect(2), Some(rect(64, 0, 128, 64)));
        assert_eq!(tile_wrect(5), Some(rect(0, 64, 64, 128)));
        assert_eq!(tile_wrect(16), Some(rect(192, 192, 256, 256)));
        for tile in 1..=16 {
            let r = tile_wrect(tile).unwrap();
            assert_eq!((r.right - r.left, r.bottom - r.top), (64, 64), "{tile}");
        }
        assert_eq!(tile_wrect(0), None);
        assert_eq!(tile_wrect(17), None);
        assert_eq!(tile_wrect(-1), None);
    }

    #[test]
    fn the_new_tile_stays_centred_where_the_game_centred_its_own() {
        // The stock spectator 24x24 at the screen centre of 1920x1080.
        let (x, y) = recentre(&SPECTATOR_RECT, &rect(0, 0, 64, 64), 960 - 12, 540 - 12);
        assert_eq!((x, y), (960 - 32, 540 - 32));
        // A 64x64 tile for another: the same place.
        assert_eq!(
            recentre(&rect(64, 0, 128, 64), &rect(0, 64, 64, 128), 928, 508),
            (928, 508)
        );
    }

    #[test]
    fn only_the_crosshairs_own_draws_are_swapped() {
        assert_eq!(Sheet::of("sprites/customXHair.spr"), Sheet::Custom);
        assert_eq!(Sheet::of("sprites/CUSTOMXHAIR.SPR"), Sheet::Custom);
        assert_eq!(Sheet::of("sprites/crosshairs.spr"), Sheet::Stock);
        assert_eq!(Sheet::of("sprites/dodcross.spr"), Sheet::Red);
        assert_eq!(Sheet::of("sprites/hud_icons.spr"), Sheet::Other);
        for s in [Sheet::Other, Sheet::Custom, Sheet::Stock, Sheet::Red] {
            assert_eq!(Sheet::from_u8(s as u8), s);
        }
        let any = rect(64, 0, 128, 64);
        assert!(is_crosshair_draw(Sheet::Custom, &any, false));
        assert!(is_crosshair_draw(Sheet::Stock, &SPECTATOR_RECT, true));
        // crosshairs.spr outside the in-eye camera, or another rect of it,
        // is not the spectator crosshair.
        assert!(!is_crosshair_draw(Sheet::Stock, &SPECTATOR_RECT, false));
        assert!(!is_crosshair_draw(Sheet::Stock, &any, true));
        assert!(!is_crosshair_draw(Sheet::Other, &any, true));
        // The red crosshair is never swapped.
        assert!(!is_crosshair_draw(Sheet::Red, &any, true));
    }

    #[test]
    fn the_player_on_screen_is_the_in_eye_target_first() {
        // A POV recorder (13) alive, or in a free or chase camera: theirs.
        assert_eq!(on_screen(Some(0), None, 13), Some(13));
        assert_eq!(on_screen(Some(3), None, 13), Some(13));
        assert_eq!(on_screen(Some(2), None, 13), Some(13));
        // Dead and watching teammate 6 in first person: the teammate's (#1321).
        assert_eq!(on_screen(Some(4), Some(6), 13), Some(6));
        // HLTV: the in-eye target, and nobody in any other camera.
        assert_eq!(on_screen(Some(4), Some(6), 0), Some(6));
        assert_eq!(on_screen(Some(3), None, 0), None);
        assert_eq!(on_screen(None, None, 0), None);
        // A target out of range is nobody known: the game's own, never the
        // recorder's over someone else's view.
        assert_eq!(on_screen(Some(4), Some(40), 13), None);
    }

    #[test]
    fn nothing_is_swapped_while_inactive() {
        ACTIVE.store(false, Ordering::Relaxed);
        let r = rect(0, 0, 64, 64);
        assert_eq!(unsafe { swap(Sheet::Custom, &r, 0, 0) }, None);
        assert_eq!(unsafe { swap(Sheet::Custom, std::ptr::null(), 0, 0) }, None);
    }

    #[test]
    fn a_sprite_file_is_checked_before_the_engine_loads_it() {
        let mut good = b"IDSP".to_vec();
        good.extend_from_slice(&2i32.to_le_bytes());
        assert!(looks_like_sprite(&good));
        assert!(!looks_like_sprite(b"IDSP\x01\0\0\0"));
        assert!(!looks_like_sprite(b"WAD3"));
    }
}
