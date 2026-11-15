//! Smooth edges for the kill feed's HD weapon icons (#841).
//!
//! The kill feed draws its icon with `SPR_Set(icon, 255, 255, 255)` and
//! `SPR_DrawHoles` (`docs/goldsrc_death_notices.md` §6). The engine's
//! `Draw_SpriteFrameHoles` (pre-Anniversary `hw+0x3d430`) turns on the alpha
//! test and, while `gl_spriteblend` is non-zero, `GL_BLEND` with
//! `SRC_ALPHA, ONE_MINUS_SRC_ALPHA`; the alpha function is the engine's
//! default `NOTEQUAL 0`. So the draw itself could blend soft edges. What made
//! them hard was the upload: an alpha-test sheet's HD replacement went up
//! with its alpha cut at 128 and the cut-out texels black
//! (`texture_hires::match_engine_expansion`), so every texel was fully in or
//! out, and a 4x sheet drawn at 3x showed that texel grid as uneven steps.
//!
//! Three changes, for HD-replaced `weapons<N>` HUD sheets only (the sheets
//! `hud.txt`'s `d_*` kill-feed icons come from). The first two also hold
//! for the frames of the even layout's own atlas ([`ATLAS_NAME`], #1022),
//! whose rects share no rows:
//!
//! 1. **Upload** ([`prepare_soft_alpha`]): the file's alpha is kept as it
//!    is, the engine's texture gamma goes on the colour of every texel that
//!    shows, and the colour of the nearest showing texels is bled into the
//!    transparent ones around them (alpha stays 0), so bilinear filtering and
//!    the mip chain pull neither black nor the upscaler's stray colour into
//!    an edge. Texels further than [`BLEED_PASSES`] from any showing texel
//!    stay black, as before, so an additive draw of the same sheet (the
//!    weapon menu's `weapon_*` icons) adds nothing there.
//! 2. **Draw** ([`draw_blended`], from `hud_sprites`' `SPR_DrawHoles` hook):
//!    `GL_BLEND` on with `SRC_ALPHA, ONE_MINUS_SRC_ALPHA` and the alpha test
//!    at `GREATER 0` for that one draw, so the edges blend whatever
//!    `gl_spriteblend` says, and the blend function and alpha function put
//!    back right after. The engine's draw turns `GL_BLEND` off itself when it
//!    returns, as it does for every `SPR_DrawHoles`, and that is left as it
//!    is.
//!
//! The GL calls are `opengl32.dll`'s own exports, as in `hud_sprite_lod.rs`:
//! no engine offsets, the same on both `hw.dll` builds. Every other sheet
//! and every other draw is the engine's as before; a `weapons<N>` sheet with
//! no HD file is not marked and draws as stock.
//!
//! 3. **Shared rows** ([`shared_row_trims`]): some `d_*` rects in `hud.txt`
//!    run one to four rows into the top of the `d_*` icon below them on the
//!    same sheet (`d_mg34` into `d_k43`, `d_mp44` into `d_kar`, `d_enfield`
//!    into `d_scopedenfield`...). Those rows belong to the lower icon: a soft
//!    sheet's icon is drawn without them, so an HD icon that fills them
//!    (#860's K43) does not show under the one above. They are empty in
//!    Valve's sheets, and stock or non-HD sheets are never trimmed. The rect
//!    is swapped in the `SPR_DrawHoles` hook, which `deathmsg::scale` calls
//!    with the unscaled rect under its scaled modelview, so the trim is the
//!    same at every scale, and the rects the line layout reads are not
//!    touched.
//!
//! No switch: the engine keeps an uploaded sprite until the process exits
//! (`GL_LoadTexture` returns it by name), so a cvar could only change the
//! draw half, not bring the hard-cut upload back.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code, unused_imports))]

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::hud_sprites::{HudIcon, Wrect};

/// How many texels out from a showing texel its colour is bled: enough for
/// the 4x sheet's level 2 (the 1:1 HUD's), whose texels average 4x4 of
/// level 0's.
pub const BLEED_PASSES: usize = 4;

/// Bit N set: `weapons<N>`'s HD replacement went up with soft edges this
/// session, so its `SPR_DrawHoles` draws blend.
static SOFT_SHEETS: AtomicU32 = AtomicU32::new(0);

/// The sheet number of a `texture_hires` sprite frame name (`weapons2_0`),
/// for the frames that get soft edges: `weapons<N>_<frame>` with N in 1..=31.
pub fn sheet_of_frame(name: &str) -> Option<u32> {
    let rest = strip_prefix_ignore_case(name, "weapons")?;
    let (n, frame) = rest.split_once('_')?;
    if frame.is_empty() || !frame.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    sheet_number(n)
}

/// The sheet number of a sprite path as `SPR_Load` is given it
/// (`sprites/weapons2.spr`).
pub fn sheet_of_path(path: &str) -> Option<u32> {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let stem = file.len().checked_sub(4).and_then(|cut| {
        file.get(cut..)
            .filter(|ext| ext.eq_ignore_ascii_case(".spr"))
            .map(|_| &file[..cut])
    })?;
    sheet_number(strip_prefix_ignore_case(stem, "weapons")?)
}

fn strip_prefix_ignore_case<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

fn sheet_number(digits: &str) -> Option<u32> {
    if digits.is_empty() || digits.len() > 2 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().filter(|n| (1..32).contains(n))
}

/// Notes that `weapons<sheet>` went up with soft edges.
pub fn mark_soft(sheet: u32) {
    if (1..32).contains(&sheet) {
        SOFT_SHEETS.fetch_or(1 << sheet, Ordering::Relaxed);
    }
}

/// `sprites/<ATLAS_NAME>.spr`: the kill feed's even-layout atlas, written by
/// `killfeed_icons.py --even` (#1022, `deathmsg::even`). Its frames go up
/// with soft edges as the `weapons<N>` sheets do, and draw blended.
pub const ATLAS_NAME: &str = "dodstudio_killfeed";

/// The most frames the atlas may have: one bit each in [`ATLAS_SOFT`].
pub const ATLAS_MAX_FRAMES: u32 = 32;

/// Bit N set: frame N of the atlas went up with its HD file (soft edges)
/// this session. The engine keeps an uploaded sprite until the process
/// exits, so a frame that went up stock stays stock.
static ATLAS_SOFT: AtomicU32 = AtomicU32::new(0);

/// The atlas frame a `texture_hires` sprite frame name (`dodstudio_killfeed_1`)
/// is, for frames below [`ATLAS_MAX_FRAMES`].
pub fn atlas_frame_of(name: &str) -> Option<u32> {
    let rest = strip_prefix_ignore_case(name, ATLAS_NAME)?.strip_prefix('_')?;
    if rest.is_empty() || rest.len() > 2 || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    rest.parse().ok().filter(|n| *n < ATLAS_MAX_FRAMES)
}

/// Notes that atlas frame `frame` went up with its HD file.
pub fn mark_atlas_soft(frame: u32) {
    if frame < ATLAS_MAX_FRAMES {
        ATLAS_SOFT.fetch_or(1 << frame, Ordering::Relaxed);
    }
}

/// Whether frames `0..frames` of the atlas all went up with their HD files.
pub fn atlas_all_soft(frames: u32) -> bool {
    if frames == 0 || frames > ATLAS_MAX_FRAMES {
        return false;
    }
    let want = if frames == 32 {
        u32::MAX
    } else {
        (1u32 << frames) - 1
    };
    ATLAS_SOFT.load(Ordering::Relaxed) & want == want
}

/// The sheet number of the sheet at `path` when it went up with soft edges,
/// else 0.
pub fn soft_sheet(path: &str) -> u32 {
    let soft = SOFT_SHEETS.load(Ordering::Relaxed);
    if soft == 0 {
        return 0;
    }
    sheet_of_path(path)
        .filter(|n| soft & (1 << n) != 0)
        .unwrap_or(0)
}

/// A kill-feed icon whose bottom rows are also the top rows of another
/// `d_*` icon's rect below it on the same sheet: drawn down to `bottom`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trim {
    pub sheet: u32,
    pub rect: Wrect,
    pub bottom: i32,
}

/// The shared rows of `hud.txt`'s kill-feed icons, which belong to the lower
/// icon (#841): for every `d_*` entry on a `weapons<N>` sheet whose rect
/// runs into the top of another `d_*` entry's rect below it (same sheet,
/// overlapping columns), the trim to that entry's top. Rows shared with
/// other sprites (`weapon_luger`...) are left alone, and entries with the
/// same top (`d_mortar` and `d_germortar`) are not "below" each other.
pub fn shared_row_trims(icons: &[HudIcon]) -> Vec<Trim> {
    let feed: Vec<(u32, Wrect)> = icons
        .iter()
        .filter(|i| {
            i.name
                .get(..2)
                .is_some_and(|p| p.eq_ignore_ascii_case("d_"))
        })
        .filter_map(|i| sheet_of_path(&i.sheet).map(|s| (s, i.rect)))
        .collect();
    let mut trims: Vec<Trim> = Vec::new();
    for &(sheet, a) in &feed {
        let bottom = feed
            .iter()
            .filter(|&&(s, b)| {
                s == sheet
                    && b.top > a.top
                    && b.top < a.bottom
                    && b.left < a.right
                    && a.left < b.right
            })
            .map(|&(_, b)| b.top)
            .min();
        if let Some(bottom) = bottom
            && !trims.iter().any(|t| t.sheet == sheet && t.rect == a)
        {
            trims.push(Trim {
                sheet,
                rect: a,
                bottom,
            });
        }
    }
    trims
}

/// [`shared_row_trims`] of the game's `hud.txt`, worked out once, on the
/// first `weapons<N>` sheet load (`hud_sprites`' `SPR_Load` hook).
static TRIMS: OnceLock<Vec<Trim>> = OnceLock::new();

/// Works [`TRIMS`] out from `icons` unless already done.
pub fn load_trims(icons: impl FnOnce() -> Vec<HudIcon>) {
    TRIMS.get_or_init(|| shared_row_trims(&icons()));
}

/// The rect to draw for `rect` of soft sheet `sheet`: `rect` without the
/// rows it shares with the kill-feed icon below it, or `None` to draw it as
/// given. The draw starts at the same `y` either way, so only the rows drawn
/// change, never the line's layout.
pub fn trimmed(sheet: u32, rect: &Wrect) -> Option<Wrect> {
    trim_in(TRIMS.get()?, sheet, rect)
}

fn trim_in(trims: &[Trim], sheet: u32, rect: &Wrect) -> Option<Wrect> {
    trims
        .iter()
        .find(|t| t.sheet == sheet && t.rect == *rect)
        .map(|t| Wrect {
            bottom: t.bottom,
            ..*rect
        })
}

/// How many kill-feed icons lose shared rows, for `ds_debug_status`.
pub fn trim_count() -> usize {
    TRIMS.get().map_or(0, Vec::len)
}

/// The soft sheets for `ds_debug_status`: `weapons1 weapons2`, or
/// empty.
pub fn describe_soft() -> String {
    let soft = SOFT_SHEETS.load(Ordering::Relaxed);
    (1..32)
        .filter(|n| soft & (1 << n) != 0)
        .map(|n| format!("weapons{n}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// What the upload of a soft sheet gets instead of
/// `match_engine_expansion`'s cut: texture gamma on every texel that shows,
/// alpha as the file has it, and the transparent texels' colour bled from
/// their showing neighbours ([`BLEED_PASSES`] texels out, black beyond).
pub fn prepare_soft_alpha(pixels: &mut [u8], width: usize, height: usize, gamma: &[u8; 256]) {
    let count = width * height;
    if pixels.len() < count * 4 || count == 0 {
        return;
    }
    // Whether a texel has a colour of its own (showing, or bled already).
    let mut filled = vec![false; count];
    for (i, px) in pixels
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .take(count)
        .enumerate()
    {
        if px[3] == 0 {
            px[..3].fill(0);
        } else {
            for c in &mut px[..3] {
                *c = gamma[*c as usize];
            }
            filled[i] = true;
        }
    }
    let mut next: Vec<(usize, [u8; 3])> = Vec::new();
    for _ in 0..BLEED_PASSES {
        next.clear();
        for y in 0..height {
            for x in 0..width {
                let i = y * width + x;
                if filled[i] {
                    continue;
                }
                let mut sum = [0u32; 3];
                let mut n = 0u32;
                for (dx, dy) in [
                    (-1i32, -1i32),
                    (0, -1),
                    (1, -1),
                    (-1, 0),
                    (1, 0),
                    (-1, 1),
                    (0, 1),
                    (1, 1),
                ] {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                        continue;
                    }
                    let j = ny as usize * width + nx as usize;
                    if filled[j] {
                        for c in 0..3 {
                            sum[c] += pixels[j * 4 + c] as u32;
                        }
                        n += 1;
                    }
                }
                if n > 0 {
                    next.push((i, sum.map(|s| ((s + n / 2) / n) as u8)));
                }
            }
        }
        if next.is_empty() {
            break;
        }
        for &(i, rgb) in &next {
            pixels[i * 4..i * 4 + 3].copy_from_slice(&rgb);
            filled[i] = true;
        }
    }
}

pub const GL_BLEND: u32 = 0x0BE2;
pub const GL_BLEND_SRC: u32 = 0x0BE1;
pub const GL_BLEND_DST: u32 = 0x0BE0;
pub const GL_ALPHA_TEST_FUNC: u32 = 0x0BC1;
pub const GL_ALPHA_TEST_REF: u32 = 0x0BC2;
pub const GL_GREATER: u32 = 0x0204;
pub const GL_SRC_ALPHA: u32 = 0x0302;
pub const GL_ONE_MINUS_SRC_ALPHA: u32 = 0x0303;

type GetIntegervFn = unsafe extern "system" fn(u32, *mut i32);
type GetFloatvFn = unsafe extern "system" fn(u32, *mut f32);
type EnableFn = unsafe extern "system" fn(u32);
type BlendFuncFn = unsafe extern "system" fn(u32, u32);
type AlphaFuncFn = unsafe extern "system" fn(u32, f32);

struct Gl {
    get_integerv: GetIntegervFn,
    get_floatv: GetFloatvFn,
    enable: EnableFn,
    blend_func: BlendFuncFn,
    alpha_func: AlphaFuncFn,
}

/// The alpha function the last blended draw found (and put back), and how
/// many there were, for `ds_debug_status`.
static SEEN_ALPHA_FUNC: AtomicU32 = AtomicU32::new(0);
static SEEN_ALPHA_REF: AtomicU32 = AtomicU32::new(0);
static SOFT_DRAWS: AtomicU32 = AtomicU32::new(0);

/// What the blended draws found, or empty before the first.
pub fn describe_draws() -> String {
    let draws = SOFT_DRAWS.load(Ordering::Relaxed);
    if draws == 0 {
        return String::new();
    }
    format!(
        "{draws} blended draw(s), the HUD's alpha test before them {:#06x} {}",
        SEEN_ALPHA_FUNC.load(Ordering::Relaxed),
        f32::from_bits(SEEN_ALPHA_REF.load(Ordering::Relaxed))
    )
}

/// The GL calls, looked up once; `None` kept when one is missing.
static GL: OnceLock<Option<Gl>> = OnceLock::new();

#[cfg(target_arch = "x86")]
fn load_gl() -> Option<Gl> {
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
    let module = unsafe { GetModuleHandleA(c"opengl32.dll".as_ptr() as *const u8) };
    if module.is_null() {
        return None;
    }
    let proc = |name: &std::ffi::CStr| unsafe {
        GetProcAddress(module, name.as_ptr() as *const u8).map(|p| p as usize)
    };
    // Safety: opengl32.dll's exports, with these (stdcall) signatures.
    unsafe {
        Some(Gl {
            get_integerv: std::mem::transmute::<usize, GetIntegervFn>(proc(c"glGetIntegerv")?),
            get_floatv: std::mem::transmute::<usize, GetFloatvFn>(proc(c"glGetFloatv")?),
            enable: std::mem::transmute::<usize, EnableFn>(proc(c"glEnable")?),
            blend_func: std::mem::transmute::<usize, BlendFuncFn>(proc(c"glBlendFunc")?),
            alpha_func: std::mem::transmute::<usize, AlphaFuncFn>(proc(c"glAlphaFunc")?),
        })
    }
}

#[cfg(not(target_arch = "x86"))]
fn load_gl() -> Option<Gl> {
    None
}

/// Runs `draw` (the engine's `SPR_DrawHoles`) with straight-alpha blending
/// and the alpha test at `GREATER 0`, then puts the blend function and the
/// alpha function back. Without the GL calls it just draws.
///
/// # Safety
///
/// Only on the engine's GL thread with its context current: from a HUD
/// sprite draw.
pub unsafe fn draw_blended(draw: impl FnOnce()) {
    let Some(gl) = GL.get_or_init(load_gl) else {
        draw();
        return;
    };
    let (mut src, mut dst, mut func) = (GL_SRC_ALPHA as i32, GL_ONE_MINUS_SRC_ALPHA as i32, 0);
    let mut reference = 0.0f32;
    // Safety: GL calls on the engine's thread with its context current.
    unsafe {
        (gl.get_integerv)(GL_BLEND_SRC, &mut src);
        (gl.get_integerv)(GL_BLEND_DST, &mut dst);
        (gl.get_integerv)(GL_ALPHA_TEST_FUNC, &mut func);
        (gl.get_floatv)(GL_ALPHA_TEST_REF, &mut reference);
        (gl.enable)(GL_BLEND);
        (gl.blend_func)(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA);
        (gl.alpha_func)(GL_GREATER, 0.0);
    }
    SEEN_ALPHA_FUNC.store(func as u32, Ordering::Relaxed);
    SEEN_ALPHA_REF.store(reference.to_bits(), Ordering::Relaxed);
    SOFT_DRAWS.fetch_add(1, Ordering::Relaxed);
    draw();
    unsafe {
        (gl.blend_func)(src as u32, dst as u32);
        if func != 0 {
            (gl.alpha_func)(func as u32, reference);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_weapons_sheets_are_soft() {
        assert_eq!(sheet_of_frame("weapons2_0"), Some(2));
        assert_eq!(sheet_of_frame("WEAPONS5_0"), Some(5));
        assert_eq!(sheet_of_frame("weapons12_3"), Some(12));
        for name in [
            "weapons_0",
            "weapons2",
            "weapons2_",
            "weapons2_x",
            "hud_icons_0",
            "weapon2_0",
        ] {
            assert_eq!(sheet_of_frame(name), None, "{name}");
        }
        assert_eq!(sheet_of_path("sprites/weapons1.spr"), Some(1));
        assert_eq!(sheet_of_path("sprites\\Weapons3.SPR"), Some(3));
        for path in [
            "sprites/weapons.spr",
            "sprites/hud_icons.spr",
            "sprites/weapons1.tga",
            "spr",
        ] {
            assert_eq!(sheet_of_path(path), None, "{path}");
        }
        assert_eq!(sheet_of_path("sprites/weapons32.spr"), None);
    }

    #[test]
    fn the_atlas_frames_are_known_by_name() {
        assert_eq!(atlas_frame_of("dodstudio_killfeed_0"), Some(0));
        assert_eq!(atlas_frame_of("DODSTUDIO_KILLFEED_1"), Some(1));
        assert_eq!(atlas_frame_of("dodstudio_killfeed_31"), Some(31));
        for name in [
            "dodstudio_killfeed_32",
            "dodstudio_killfeed_",
            "dodstudio_killfeed",
            "dodstudio_killfeed_x",
            "dodstudio_killfeed_100",
            "dodstudio_killfeedx_0",
            "weapons2_0",
        ] {
            assert_eq!(atlas_frame_of(name), None, "{name}");
        }
        // Not a weapons sheet, so no trims and no sheet bit.
        assert_eq!(sheet_of_frame("dodstudio_killfeed_0"), None);
        assert_eq!(sheet_of_path("sprites/dodstudio_killfeed.spr"), None);
    }

    #[test]
    fn the_atlas_counts_as_soft_only_with_every_frame() {
        assert!(!atlas_all_soft(0));
        assert!(!atlas_all_soft(33));
        mark_atlas_soft(0);
        mark_atlas_soft(40);
        assert!(atlas_all_soft(1));
        // Frame 1 never went up with its HD file, so a two-frame atlas isn't.
        assert!(!atlas_all_soft(2));
        mark_atlas_soft(1);
        assert!(atlas_all_soft(2));
    }

    #[test]
    fn marks_follow_the_sheet() {
        mark_soft(4);
        assert_eq!(soft_sheet("sprites/weapons4.spr"), 4);
        assert_eq!(soft_sheet("sprites/hud_icons.spr"), 0);
        assert!(describe_soft().contains("weapons4"));
        mark_soft(0);
        mark_soft(40);
    }

    /// `hud.txt`'s own lines for weapons3 around the MG34 and K43, plus a
    /// non-kill-feed sprite sharing rows and the two mortars' identical
    /// rects.
    const HUD: &str = "9\n\
        d_mg34\t640 weapons3 167 173 89 23\n\
        d_k43\t640 weapons3 167 192 89 14\n\
        d_gerknife\t640 weapons3 167 223 45 13\n\
        d_luger\t640 weapons3 167 235 45 20\n\
        weapon_luger\t640 weapons3 0 0 170 50\n\
        d_mp40\t640 weapons3 0 40 160 20\n\
        d_mortar\t640 weapons4 0 0 50 20\n\
        d_germortar\t640 weapons4 0 0 50 20\n\
        d_piat\t640 hud_icons 0 10 50 20\n";

    #[test]
    fn shared_rows_go_to_the_lower_kill_feed_icon() {
        let icons = crate::hud_sprites::parse_hud_txt(HUD);
        let trims = shared_row_trims(&icons);
        let mg34 = icons[0].rect;
        let gerknife = icons[2].rect;
        assert_eq!(
            trims,
            vec![
                Trim {
                    sheet: 3,
                    rect: mg34,
                    bottom: 192
                },
                Trim {
                    sheet: 3,
                    rect: gerknife,
                    bottom: 235
                },
            ],
            "only d_* over d_* on one weapons sheet; weapon_luger over d_mp40 is left"
        );
        // The trimmed rect keeps its top, so the draw's y and the layout stay.
        assert_eq!(
            trim_in(&trims, 3, &mg34),
            Some(Wrect {
                bottom: 192,
                ..mg34
            })
        );
        // The lower icon draws whole, and other sheets never match.
        assert_eq!(trim_in(&trims, 3, &icons[1].rect), None);
        assert_eq!(trim_in(&trims, 4, &mg34), None);
    }

    #[test]
    fn icons_side_by_side_share_nothing() {
        let icons = crate::hud_sprites::parse_hud_txt(
            "2\nd_a 640 weapons1 0 0 50 30\nd_b 640 weapons1 50 20 50 30\n",
        );
        assert!(shared_row_trims(&icons).is_empty());
    }

    #[test]
    fn soft_alpha_is_kept_and_colour_bled_outwards() {
        // An 8x1 row: one opaque texel, one half, six transparent with
        // stray colour from the upscaler.
        let mut gamma = [0u8; 256];
        for (i, g) in gamma.iter_mut().enumerate() {
            *g = (i as u8).saturating_add(1);
        }
        let mut px = vec![
            100, 50, 20, 255, //
            100, 50, 20, 128, //
            9, 9, 9, 0, //
            9, 9, 9, 0, //
            9, 9, 9, 0, //
            9, 9, 9, 0, //
            9, 9, 9, 0, //
            9, 9, 9, 0, //
        ];
        prepare_soft_alpha(&mut px, 8, 1, &gamma);
        // Showing texels: gamma, alpha kept.
        assert_eq!(&px[0..4], &[101, 51, 21, 255]);
        assert_eq!(&px[4..8], &[101, 51, 21, 128]);
        // The next BLEED_PASSES texels carry the edge colour at alpha 0.
        for t in 2..2 + BLEED_PASSES {
            assert_eq!(&px[t * 4..t * 4 + 4], &[101, 51, 21, 0], "texel {t}");
        }
        // Beyond that, black as before.
        for t in 2 + BLEED_PASSES..8 {
            assert_eq!(&px[t * 4..t * 4 + 4], &[0, 0, 0, 0], "texel {t}");
        }
    }

    #[test]
    fn bleed_averages_the_showing_neighbours() {
        let gamma: [u8; 256] = std::array::from_fn(|i| i as u8);
        // 3x1: red | transparent | blue.
        let mut px = vec![200, 0, 0, 255, 0, 0, 0, 0, 0, 0, 100, 255];
        prepare_soft_alpha(&mut px, 3, 1, &gamma);
        assert_eq!(&px[4..8], &[100, 0, 50, 0]);
    }

    #[test]
    fn a_short_buffer_is_left_alone() {
        let gamma: [u8; 256] = std::array::from_fn(|i| i as u8);
        let mut px = vec![1, 2, 3, 0];
        prepare_soft_alpha(&mut px, 2, 2, &gamma);
        assert_eq!(px, [1, 2, 3, 0]);
    }

    #[test]
    fn gl_enums() {
        assert_eq!(
            (GL_BLEND, GL_BLEND_SRC, GL_BLEND_DST),
            (0x0BE2, 0x0BE1, 0x0BE0)
        );
        assert_eq!((GL_ALPHA_TEST_FUNC, GL_ALPHA_TEST_REF), (0x0BC1, 0x0BC2));
        assert_eq!(
            (GL_GREATER, GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA),
            (0x0204, 0x0302, 0x0303)
        );
    }
}
