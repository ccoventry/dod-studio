//! Each player's crosshair in a demo, from Studio's Players table (#428).
//!
//! ## Which player
//!
//! The one whose view is on screen: in a POV demo the recorder (the local
//! player), in an HLTV demo the player the in-eye camera is on
//! (`spectator_target::in_eye_target`). Free and chase cameras draw no
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
//! [`poll`] decides once a frame, from `commands::poll`: the player, their
//! row, and the sprite's handle (loaded with the engine's own `SPR_Load` the
//! first time it is needed after each level load). The draw hooks then read
//! three atomics ([`swap`]); no lock is taken there.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};

use crate::hud_sprites::Wrect;
use crate::player_table::Crosshair;

/// The sprite handle to draw the crosshair with, 0 for the game's own.
static HANDLE: AtomicI32 = AtomicI32::new(0);
/// The tile, 1..=16, while [`HANDLE`] is set.
static TILE: AtomicI32 = AtomicI32::new(0);
/// Whether the in-eye spectator camera is up, so `crosshairs.spr`'s 24x24
/// rect is the spectator crosshair.
static IN_EYE: AtomicBool = AtomicBool::new(false);
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
}

impl Sheet {
    pub fn of(path: &str) -> Sheet {
        let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
        if file.eq_ignore_ascii_case("customXHair.spr") {
            Sheet::Custom
        } else if file.eq_ignore_ascii_case("crosshairs.spr") {
            Sheet::Stock
        } else {
            Sheet::Other
        }
    }

    pub fn from_u8(v: u8) -> Sheet {
        match v {
            1 => Sheet::Custom,
            2 => Sheet::Stock,
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
        Sheet::Other => false,
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
    let handle = HANDLE.load(Ordering::Relaxed);
    if handle == 0 || sheet == Sheet::Other || prc.is_null() {
        return None;
    }
    let old = unsafe { *prc };
    if !is_crosshair_draw(sheet, &old, IN_EYE.load(Ordering::Relaxed)) {
        return None;
    }
    let new = tile_wrect(TILE.load(Ordering::Relaxed))?;
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

/// The entity whose view is on screen in the demo playing: the recorder in
/// POV, the in-eye target in HLTV.
fn viewed_player() -> Option<i32> {
    if crate::spray_hltv::hltv() {
        return crate::spectator_target::in_eye_target();
    }
    let engfuncs = crate::engine::engfuncs()?;
    // Safety: the engine's own call; the result is the local player's
    // `cl_entity_t`, `index` first, or null with no level loaded.
    let local = unsafe { (engfuncs.get_local_player)() } as *const i32;
    if local.is_null() || !crate::crash::readable(local as usize, 4) {
        return None;
    }
    Some(unsafe { *local }).filter(|i| (1..=32).contains(i))
}

/// This frame's crosshair: the player on screen and what the table gives
/// them, or `None` for the game's own.
fn decide() -> Option<(String, Crosshair)> {
    if !crate::player_overrides::on() || !crate::spray_hltv::playing_demo() {
        return None;
    }
    let table = crate::spray_hltv::current_table()?;
    if !table.crosshair_active() {
        return None;
    }
    let entity = viewed_player()?;
    let (name, steam_id) = crate::spray_hltv::player(entity as usize - 1)?;
    let choice = table.choose_crosshair(&name, steam_id);
    Some((name, choice))
}

/// Once a frame, from `commands::poll`, on the game's thread.
pub fn poll() {
    IN_EYE.store(
        crate::spectator_target::mode() == Some(4),
        Ordering::Relaxed,
    );
    let (handle, tile, text) = match decide() {
        Some((name, Crosshair::Tile { sprite, tile })) => {
            let handle = handle_for(&sprite);
            let text = if handle == 0 {
                format!("{name}: tile {tile} of {sprite}, which did not load; the game's own")
            } else {
                format!("{name}: tile {tile} of {sprite}")
            };
            (handle, i32::from(tile), text)
        }
        Some((name, Crosshair::Game)) => (0, 0, format!("{name}: the game's own")),
        None => (0, 0, String::new()),
    };
    TILE.store(tile, Ordering::Relaxed);
    HANDLE.store(handle, Ordering::Relaxed);
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
        assert_eq!(Sheet::of("sprites/hud_icons.spr"), Sheet::Other);
        for s in [Sheet::Other, Sheet::Custom, Sheet::Stock] {
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
    }

    #[test]
    fn nothing_is_swapped_without_a_handle() {
        HANDLE.store(0, Ordering::Relaxed);
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
