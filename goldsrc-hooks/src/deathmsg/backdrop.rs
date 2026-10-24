//! `ds_hud_deathmsg outline` and `background`: an outline around the kill
//! feed's names, and translucent boxes behind its lines or its names, so the
//! names read over a busy wall.
//!
//! ## Why drawn by hand
//!
//! Neither engine build's vgui2 font manager has an outline (`font.rs`,
//! "Flags"), and the game's own font goes through `pfnDrawConsoleString`,
//! which takes no style at all. So the outline is the name drawn again in
//! the outline colour at every offset within N px of where it goes (the 8
//! neighbours at 1 px, a filled disc beyond: [`in_outline`]), before the
//! coloured pass. That works the same through `font`'s surface calls and
//! through the engine's call.
//!
//! The boxes are one GL quad each, from `opengl32.dll`'s own exports as in
//! `hud_sprite_edges.rs`: no engine offsets, the same on both builds.
//! `pfnFillRGBA` would have been the engine's way, but it adds its colour
//! (`GL_ONE`), so a dark box comes out as nothing.
//!
//! ## Where
//!
//! Everything happens inside `font`'s redirected text calls and `scale`'s
//! redirected `SPR_DrawHoles`, which see every line in `Draw`'s order: the
//! victim's width (the line starts; [`line_started`] notes where it will
//! end), the killer's name, the icon, the victim's name. The line box goes
//! down at the line's first draw, the killer's name or, with no killer, the
//! icon. The name boxes go down just before their name.
//!
//! All of it is off by default, and costs one atomic load per name while off.
//! The settings are atomics: the draws read them on the engine's thread every
//! frame, and the console writes them on the same thread.

// The GL calls run only in the 32-bit DLL; a host build keeps the parsing and
// the geometry for the unit tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code, unused_imports))]

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};

/// The outline's widest, in pixels. Each name is drawn once per offset in
/// the disc, so 4 px is already 68 extra draws a name.
pub(super) const MAX_OUTLINE: i32 = 4;
/// The boxes' padding when none is typed, and the most a box takes.
pub(super) const DEFAULT_PADDING: i32 = 4;
pub(super) const MAX_PADDING: i32 = 64;
/// The gap left between the boxes of two lines, which `Draw` steps by
/// exactly the line's height.
const LINE_GAP: i32 = 2;

/// An `r g b` colour, each 0..=255.
pub(super) type Rgb = (i32, i32, i32);

const BLACK: Rgb = (0, 0, 0);

/// The name colours DoD's `GetClientColor` (`client+0x2ad90`) hands
/// `pfnDrawSetTextColor`, as the engine stores them (`(int)(f * 255)`):
/// team 1, Allies, `(0.3, 0.4, 0.3)` -- the British too, who are team 1 on a
/// British map -- and team 2, Axis, `(1.0, 0.25, 0.25)`. The same on both
/// builds (`client.dll` is identical).
pub(super) const ALLIES_NAME: Rgb = (76, 102, 76);
pub(super) const AXIS_NAME: Rgb = (255, 63, 63);

/// Which team's box colour `background allies|axis` sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Team {
    Allies,
    Axis,
}

/// What `background` was asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Background {
    /// `background default`.
    Off,
    /// `background <alpha> [r g b] [padding]`: one box per line.
    Line { alpha: i32, rgb: Rgb, padding: i32 },
    /// `background team <alpha> [padding]`: one box per name, in a dark
    /// shade of its team's colour.
    Team { alpha: i32, padding: i32 },
    /// `background allies|axis <r g b>`, or `default` (`None`) for the shade.
    TeamColour(Team, Option<Rgb>),
}

/// `0xAARRGGBB`.
fn pack(rgb: Rgb, alpha: i32) -> u32 {
    let byte = |v: i32| v.clamp(0, 255) as u32;
    (byte(alpha) << 24) | (byte(rgb.0) << 16) | (byte(rgb.1) << 8) | byte(rgb.2)
}

fn unpack(packed: u32) -> (Rgb, i32) {
    let byte = |shift: u32| ((packed >> shift) & 0xff) as i32;
    ((byte(16), byte(8), byte(0)), byte(24))
}

/// A team colour override not set: the shade of the game's colour is used.
/// A set one is stored at alpha 255, so it is never 0.
const UNSET: u32 = 0;

static OUTLINE_PX: AtomicI32 = AtomicI32::new(0);
static OUTLINE_COLOUR: AtomicU32 = AtomicU32::new(0xff00_0000);
/// 0 off, 1 a box per line, 2 a box per name.
static MODE: AtomicU32 = AtomicU32::new(MODE_OFF);
const MODE_OFF: u32 = 0;
const MODE_LINE: u32 = 1;
const MODE_TEAM: u32 = 2;
static BOX_COLOUR: AtomicU32 = AtomicU32::new(0);
static PADDING: AtomicI32 = AtomicI32::new(DEFAULT_PADDING);
static ALLIES_BOX: AtomicU32 = AtomicU32::new(UNSET);
static AXIS_BOX: AtomicU32 = AtomicU32::new(UNSET);

/// Whether the current line's box is still to be drawn, and the x its
/// victim's name ends at (from [`line_started`]).
static LINE_PENDING: AtomicBool = AtomicBool::new(false);
static LINE_RIGHT: AtomicI32 = AtomicI32::new(0);

/// Whether a name at offset `(dx, dy)` belongs to an outline `n` px wide: the
/// 8 neighbours at 1 px, every offset in a disc of radius `n` (rounded out a
/// little, so a 2 px outline has no notches on its diagonals) beyond.
pub(super) fn in_outline(dx: i32, dy: i32, n: i32) -> bool {
    (dx, dy) != (0, 0) && dx.abs() <= n && dy.abs() <= n && dx * dx + dy * dy <= n * n + n
}

/// The box colour behind a name drawn in `name`: the team's own when one was
/// set, else a quarter of the name's colour.
pub(super) fn team_shade(name: Rgb, allies: Option<Rgb>, axis: Option<Rgb>) -> Rgb {
    match (name, allies, axis) {
        (ALLIES_NAME, Some(own), _) => own,
        (AXIS_NAME, _, Some(own)) => own,
        ((r, g, b), _, _) => (r / 4, g / 4, b / 4),
    }
}

/// A line's box: `left..right` padded, the line's own height less a small
/// gap at the bottom, so two lines' translucent boxes never overlap.
pub(super) fn line_box(left: i32, right: i32, y: i32, height: i32, padding: i32) -> [i32; 4] {
    [
        left - padding,
        y,
        right + padding,
        y + (height - LINE_GAP).max(1),
    ]
}

/// A name's box: the text's own extent, padded, and kept inside its line.
pub(super) fn name_box(
    x: i32,
    width: i32,
    text_y: i32,
    text_tall: i32,
    line: (i32, i32),
    padding: i32,
) -> [i32; 4] {
    let (line_y, line_tall) = line;
    let top = (text_y - padding / 2).max(line_y);
    let bottom = (text_y + text_tall + padding / 2).min(line_y + (line_tall - LINE_GAP).max(1));
    [x - padding, top, x + width + padding, bottom.max(top + 1)]
}

/// Whether anything here is on: the layout then keeps clear of the right
/// edge as it does for `scale` and `font`.
pub(super) fn active() -> bool {
    OUTLINE_PX.load(Ordering::Relaxed) > 0 || MODE.load(Ordering::Relaxed) != MODE_OFF
}

/// How much further than the name's own width a line reaches to the right:
/// its outline, or its box's padding.
pub(super) fn edge_extra() -> i32 {
    let outline = OUTLINE_PX.load(Ordering::Relaxed);
    let padding = if MODE.load(Ordering::Relaxed) == MODE_OFF {
        0
    } else {
        PADDING.load(Ordering::Relaxed)
    };
    outline.max(padding)
}

/// The outline in force: its width (0 off), colour and alpha.
pub(super) fn outline() -> (i32, Rgb, i32) {
    let (rgb, alpha) = unpack(OUTLINE_COLOUR.load(Ordering::Relaxed));
    (OUTLINE_PX.load(Ordering::Relaxed), rgb, alpha)
}

/// Notes that a line starts, its victim's name ending at `right`, from the
/// victim's width call.
pub(super) fn line_started(right: i32) {
    if MODE.load(Ordering::Relaxed) == MODE_LINE {
        LINE_RIGHT.store(right, Ordering::Relaxed);
        LINE_PENDING.store(true, Ordering::Relaxed);
    }
}

/// The line's box, once, at its first draw at `x` on a line from `y`
/// `height` px tall.
///
/// # Safety
///
/// Inside the kill feed's draw, on the engine's thread.
pub(super) unsafe fn line_box_at(x: i32, y: i32, height: i32) {
    if !LINE_PENDING.swap(false, Ordering::Relaxed) || MODE.load(Ordering::Relaxed) != MODE_LINE {
        return;
    }
    let (rgb, alpha) = unpack(BOX_COLOUR.load(Ordering::Relaxed));
    let rect = line_box(
        x,
        LINE_RIGHT.load(Ordering::Relaxed),
        y,
        height,
        PADDING.load(Ordering::Relaxed),
    );
    unsafe { fill(rect, rgb, alpha) };
}

/// Whether names get a box each, in their team's shade.
pub(super) fn name_boxes() -> bool {
    MODE.load(Ordering::Relaxed) == MODE_TEAM
}

/// A name's box in its team's shade, for a name drawn in `name`.
///
/// # Safety
///
/// As [`line_box_at`].
pub(super) unsafe fn name_box_at(
    name: Rgb,
    x: i32,
    width: i32,
    text_y: i32,
    text_tall: i32,
    line: (i32, i32),
) {
    let own = |slot: &AtomicU32| match slot.load(Ordering::Relaxed) {
        UNSET => None,
        packed => Some(unpack(packed).0),
    };
    let rgb = team_shade(name, own(&ALLIES_BOX), own(&AXIS_BOX));
    let (_, alpha) = unpack(BOX_COLOUR.load(Ordering::Relaxed));
    let rect = name_box(
        x,
        width,
        text_y,
        text_tall,
        line,
        PADDING.load(Ordering::Relaxed),
    );
    unsafe { fill(rect, rgb, alpha) };
}

/// A colour argument: three numbers 0..=255.
fn parse_rgb(words: &[String]) -> Result<Rgb, String> {
    let [r, g, b] = words else {
        return Err("a colour is three numbers, r g b, each 0..255".to_string());
    };
    let one = |w: &String| -> Result<i32, String> {
        match w.parse::<i32>() {
            Ok(v) if (0..=255).contains(&v) => Ok(v),
            _ => Err(format!("colour: expected 0..255, got {w:?}")),
        }
    };
    Ok((one(r)?, one(g)?, one(b)?))
}

fn parse_in(word: &str, what: &str, range: std::ops::RangeInclusive<i32>) -> Result<i32, String> {
    match word.parse::<i32>() {
        Ok(v) if range.contains(&v) => Ok(v),
        _ => Err(format!(
            "{what}: expected {}..{}, got {word:?}",
            range.start(),
            range.end()
        )),
    }
}

/// The arguments after `outline`: `<0..4> [r g b] [alpha]`, or `default`
/// (off). Returns the width, colour and alpha.
pub(super) fn parse_outline(rest: &[String]) -> Result<(i32, Rgb, i32), String> {
    if let [only] = rest
        && only.eq_ignore_ascii_case("default")
    {
        return Ok((0, BLACK, 255));
    }
    let Some((px, more)) = rest.split_first() else {
        return Err(format!(
            "need a width, 0..{MAX_OUTLINE} px, then r g b and alpha if you like, or default"
        ));
    };
    let px = parse_in(px, "width", 0..=MAX_OUTLINE)?;
    match more {
        [] => Ok((px, BLACK, 255)),
        [r, g, b] => Ok((px, parse_rgb(&[r.clone(), g.clone(), b.clone()])?, 255)),
        [r, g, b, a] => Ok((
            px,
            parse_rgb(&[r.clone(), g.clone(), b.clone()])?,
            parse_in(a, "alpha", 0..=255)?,
        )),
        _ => Err("expected outline <width> [r g b] [alpha]".to_string()),
    }
}

/// The arguments after `background`.
pub(super) fn parse_background(rest: &[String]) -> Result<Background, String> {
    let Some((first, more)) = rest.split_first() else {
        return Err("need an alpha 0..255, or team, allies, axis or default".to_string());
    };
    match first.to_ascii_lowercase().as_str() {
        "default" if more.is_empty() => Ok(Background::Off),
        "team" => {
            let (alpha, padding) = match more {
                [a] => (a, None),
                [a, p] => (a, Some(p)),
                _ => return Err("expected background team <alpha> [padding]".to_string()),
            };
            Ok(Background::Team {
                alpha: parse_in(alpha, "alpha", 0..=255)?,
                padding: match padding {
                    Some(p) => parse_in(p, "padding", 0..=MAX_PADDING)?,
                    None => DEFAULT_PADDING,
                },
            })
        }
        side @ ("allies" | "axis") => {
            let team = if side == "allies" {
                Team::Allies
            } else {
                Team::Axis
            };
            match more {
                [d] if d.eq_ignore_ascii_case("default") => Ok(Background::TeamColour(team, None)),
                _ => Ok(Background::TeamColour(team, Some(parse_rgb(more)?))),
            }
        }
        _ => {
            let alpha = parse_in(first, "alpha", 0..=255)?;
            let (rgb, padding) = match more {
                [] => (BLACK, DEFAULT_PADDING),
                [p] => (BLACK, parse_in(p, "padding", 0..=MAX_PADDING)?),
                [_, _, _] => (parse_rgb(more)?, DEFAULT_PADDING),
                [r, g, b, p] => (
                    parse_rgb(&[r.clone(), g.clone(), b.clone()])?,
                    parse_in(p, "padding", 0..=MAX_PADDING)?,
                ),
                _ => {
                    return Err("expected background <alpha> [r g b] [padding]".to_string());
                }
            };
            Ok(Background::Line {
                alpha,
                rgb,
                padding,
            })
        }
    }
}

fn describe_rgb((r, g, b): Rgb) -> String {
    format!("{r} {g} {b}")
}

/// Makes the outline `px` wide in `rgb` at `alpha`; the reply is what the
/// console prints.
pub(super) fn set_outline(px: i32, rgb: Rgb, alpha: i32) -> String {
    OUTLINE_COLOUR.store(pack(rgb, alpha), Ordering::Relaxed);
    OUTLINE_PX.store(px.clamp(0, MAX_OUTLINE), Ordering::Relaxed);
    if px == 0 {
        "outline off".to_string()
    } else {
        format!(
            "outline = {px} px, colour {}, alpha {alpha} (with the game's font the outline is always opaque), from the next frame",
            describe_rgb(rgb)
        )
    }
}

/// Applies a parsed `background`; the reply is what the console prints.
pub(super) fn set_background(asked: Background) -> String {
    match asked {
        Background::Off => {
            MODE.store(MODE_OFF, Ordering::Relaxed);
            LINE_PENDING.store(false, Ordering::Relaxed);
            "background off".to_string()
        }
        Background::Line {
            alpha,
            rgb,
            padding,
        } => {
            BOX_COLOUR.store(pack(rgb, alpha), Ordering::Relaxed);
            PADDING.store(padding, Ordering::Relaxed);
            MODE.store(MODE_LINE, Ordering::Relaxed);
            format!(
                "background = a box behind each line, colour {}, alpha {alpha}, padding {padding} px, from the next frame",
                describe_rgb(rgb)
            )
        }
        Background::Team { alpha, padding } => {
            BOX_COLOUR.store(pack(BLACK, alpha), Ordering::Relaxed);
            PADDING.store(padding, Ordering::Relaxed);
            MODE.store(MODE_TEAM, Ordering::Relaxed);
            format!(
                "background = a box behind each name in a dark shade of its team's colour (or `background allies|axis <r g b>`), alpha {alpha}, padding {padding} px, from the next frame"
            )
        }
        Background::TeamColour(team, rgb) => {
            let (slot, name) = match team {
                Team::Allies => (&ALLIES_BOX, "allies (and British)"),
                Team::Axis => (&AXIS_BOX, "axis"),
            };
            slot.store(rgb.map_or(UNSET, |c| pack(c, 255)), Ordering::Relaxed);
            let colour = match rgb {
                Some(c) => describe_rgb(c),
                None => "a quarter of the name's colour".to_string(),
            };
            let note = if MODE.load(Ordering::Relaxed) == MODE_TEAM {
                ""
            } else {
                " (shown with `background team <alpha>`)"
            };
            format!("background {name} boxes = {colour}{note}")
        }
    }
}

/// What `status` says.
pub(super) fn describe() -> String {
    let (px, rgb, alpha) = outline();
    let outline = if px == 0 {
        "outline = off".to_string()
    } else {
        format!("outline = {px} px {} alpha {alpha}", describe_rgb(rgb))
    };
    let (box_rgb, box_alpha) = unpack(BOX_COLOUR.load(Ordering::Relaxed));
    let padding = PADDING.load(Ordering::Relaxed);
    let team = |slot: &AtomicU32| match slot.load(Ordering::Relaxed) {
        UNSET => "shade".to_string(),
        packed => describe_rgb(unpack(packed).0),
    };
    let background = match MODE.load(Ordering::Relaxed) {
        MODE_LINE => format!(
            "background = lines {} alpha {box_alpha} padding {padding}",
            describe_rgb(box_rgb)
        ),
        MODE_TEAM => format!(
            "background = team alpha {box_alpha} padding {padding} (allies {}, axis {})",
            team(&ALLIES_BOX),
            team(&AXIS_BOX)
        ),
        _ => "background = off".to_string(),
    };
    format!("{outline}, {background}")
}

/// One translucent box, `[x0, y0, x1, y1]` in screen pixels.
///
/// # Safety
///
/// On the engine's thread with its GL context current, inside a HUD draw.
unsafe fn fill(rect: [i32; 4], rgb: Rgb, alpha: i32) {
    #[cfg(target_arch = "x86")]
    {
        if alpha <= 0 {
            return;
        }
        if let Some(gl) = gl::get() {
            // Safety: the caller's.
            unsafe { gl.fill(rect, rgb, alpha) };
        }
    }
    #[cfg(not(target_arch = "x86"))]
    {
        let _ = (rect, rgb, alpha);
    }
}

#[cfg(target_arch = "x86")]
mod gl {
    use std::sync::OnceLock;

    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    use super::Rgb;

    const GL_TEXTURE_2D: u32 = 0x0de1;
    const GL_BLEND: u32 = 0x0be2;
    const GL_ALPHA_TEST: u32 = 0x0bc0;
    const GL_BLEND_SRC: u32 = 0x0be1;
    const GL_BLEND_DST: u32 = 0x0be0;
    const GL_CURRENT_COLOR: u32 = 0x0b00;
    const GL_SRC_ALPHA: u32 = 0x0302;
    const GL_ONE_MINUS_SRC_ALPHA: u32 = 0x0303;
    const GL_QUADS: u32 = 0x0007;

    type CapFn = unsafe extern "system" fn(u32);
    type IsEnabledFn = unsafe extern "system" fn(u32) -> u8;
    type GetIntegervFn = unsafe extern "system" fn(u32, *mut i32);
    type GetFloatvFn = unsafe extern "system" fn(u32, *mut f32);
    type BlendFuncFn = unsafe extern "system" fn(u32, u32);
    type Color4ubFn = unsafe extern "system" fn(u8, u8, u8, u8);
    type Color4fFn = unsafe extern "system" fn(f32, f32, f32, f32);
    type BeginFn = unsafe extern "system" fn(u32);
    type EndFn = unsafe extern "system" fn();
    type Vertex2iFn = unsafe extern "system" fn(i32, i32);

    pub(super) struct Gl {
        enable: CapFn,
        disable: CapFn,
        is_enabled: IsEnabledFn,
        get_integerv: GetIntegervFn,
        get_floatv: GetFloatvFn,
        blend_func: BlendFuncFn,
        color4ub: Color4ubFn,
        color4f: Color4fFn,
        begin: BeginFn,
        end: EndFn,
        vertex2i: Vertex2iFn,
    }

    impl Gl {
        /// The quad, with the texture and alpha test off and straight-alpha
        /// blending for it alone; every state it touches is put back.
        pub(super) unsafe fn fill(&self, [x0, y0, x1, y1]: [i32; 4], rgb: Rgb, alpha: i32) {
            let byte = |v: i32| v.clamp(0, 255) as u8;
            // Safety: GL calls on the engine's thread with its context current.
            unsafe {
                let texture = (self.is_enabled)(GL_TEXTURE_2D) != 0;
                let blend = (self.is_enabled)(GL_BLEND) != 0;
                let alpha_test = (self.is_enabled)(GL_ALPHA_TEST) != 0;
                let (mut src, mut dst) = (GL_SRC_ALPHA as i32, GL_ONE_MINUS_SRC_ALPHA as i32);
                (self.get_integerv)(GL_BLEND_SRC, &mut src);
                (self.get_integerv)(GL_BLEND_DST, &mut dst);
                let mut colour = [1.0f32; 4];
                (self.get_floatv)(GL_CURRENT_COLOR, colour.as_mut_ptr());

                (self.disable)(GL_TEXTURE_2D);
                (self.disable)(GL_ALPHA_TEST);
                (self.enable)(GL_BLEND);
                (self.blend_func)(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA);
                (self.color4ub)(byte(rgb.0), byte(rgb.1), byte(rgb.2), byte(alpha));
                (self.begin)(GL_QUADS);
                (self.vertex2i)(x0, y0);
                (self.vertex2i)(x1, y0);
                (self.vertex2i)(x1, y1);
                (self.vertex2i)(x0, y1);
                (self.end)();

                (self.color4f)(colour[0], colour[1], colour[2], colour[3]);
                (self.blend_func)(src as u32, dst as u32);
                if !blend {
                    (self.disable)(GL_BLEND);
                }
                if alpha_test {
                    (self.enable)(GL_ALPHA_TEST);
                }
                if texture {
                    (self.enable)(GL_TEXTURE_2D);
                }
            }
        }
    }

    static GL: OnceLock<Option<Gl>> = OnceLock::new();

    fn load() -> Option<Gl> {
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
                enable: std::mem::transmute::<usize, CapFn>(proc(c"glEnable")?),
                disable: std::mem::transmute::<usize, CapFn>(proc(c"glDisable")?),
                is_enabled: std::mem::transmute::<usize, IsEnabledFn>(proc(c"glIsEnabled")?),
                get_integerv: std::mem::transmute::<usize, GetIntegervFn>(proc(c"glGetIntegerv")?),
                get_floatv: std::mem::transmute::<usize, GetFloatvFn>(proc(c"glGetFloatv")?),
                blend_func: std::mem::transmute::<usize, BlendFuncFn>(proc(c"glBlendFunc")?),
                color4ub: std::mem::transmute::<usize, Color4ubFn>(proc(c"glColor4ub")?),
                color4f: std::mem::transmute::<usize, Color4fFn>(proc(c"glColor4f")?),
                begin: std::mem::transmute::<usize, BeginFn>(proc(c"glBegin")?),
                end: std::mem::transmute::<usize, EndFn>(proc(c"glEnd")?),
                vertex2i: std::mem::transmute::<usize, Vertex2iFn>(proc(c"glVertex2i")?),
            })
        }
    }

    /// The GL calls, looked up once (a missing one is kept as `None`).
    pub(super) fn get() -> Option<&'static Gl> {
        GL.get_or_init(load).as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn a_1px_outline_is_the_8_neighbours_and_wider_ones_are_discs() {
        let count = |n: i32| {
            (-n..=n)
                .flat_map(|dx| (-n..=n).map(move |dy| (dx, dy)))
                .filter(|&(dx, dy)| in_outline(dx, dy, n))
                .count()
        };
        assert_eq!(count(0), 0);
        assert_eq!(count(1), 8);
        // 2 px: the 5x5 square less its four corners and the centre.
        assert_eq!(count(2), 20);
        assert!(!in_outline(2, 2, 2));
        assert!(in_outline(2, 1, 2), "no notch beside the diagonal");
        assert!(
            !in_outline(0, 0, 3),
            "the name's own spot is the coloured pass"
        );
        assert_eq!(count(MAX_OUTLINE), 68, "extra draws a name at the widest");
    }

    #[test]
    fn team_boxes_are_a_quarter_of_the_name_unless_the_team_has_its_own() {
        assert_eq!(team_shade(AXIS_NAME, None, None), (63, 15, 15));
        assert_eq!(team_shade(ALLIES_NAME, None, None), (19, 25, 19));
        let own = (0, 0, 90);
        assert_eq!(team_shade(ALLIES_NAME, Some(own), None), own);
        assert_eq!(team_shade(AXIS_NAME, Some(own), None), (63, 15, 15));
        assert_eq!(team_shade(AXIS_NAME, None, Some(own)), own);
        // Spectators and anything else: always the shade.
        assert_eq!(team_shade((255, 178, 0), Some(own), Some(own)), (63, 44, 0));
    }

    #[test]
    fn the_name_colours_are_what_get_client_color_hands_over() {
        let stored = |f: f32| (f * 255.0) as i32;
        assert_eq!(ALLIES_NAME, (stored(0.3), stored(0.4), stored(0.3)));
        assert_eq!(AXIS_NAME, (stored(1.0), stored(0.25), stored(0.25)));
    }

    #[test]
    fn boxes_are_padded_sideways_and_stay_inside_their_line() {
        // A line from y 20, 85 px tall, killer at 1700, victim ending at 2544.
        assert_eq!(line_box(1700, 2544, 20, 85, 4), [1696, 20, 2548, 103]);
        // The next line starts at 105: two px clear of this box.
        assert!(line_box(1700, 2544, 20, 85, 4)[3] < 20 + 85);
        // A 32 px name centred on that line, padded 4: two px above and below.
        assert_eq!(
            name_box(1700, 200, 46, 32, (20, 85), 4),
            [1696, 44, 1904, 80]
        );
        // Text as tall as the line: the box is the line's.
        assert_eq!(name_box(10, 50, 20, 40, (20, 40), 8), [2, 20, 68, 58]);
        // Never upside down.
        let b = name_box(0, 0, 0, 0, (0, 0), 0);
        assert!(b[3] > b[1]);
    }

    #[test]
    fn outline_arguments() {
        assert_eq!(parse_outline(&argv(&["2"])), Ok((2, BLACK, 255)));
        assert_eq!(parse_outline(&argv(&["default"])), Ok((0, BLACK, 255)));
        assert_eq!(parse_outline(&argv(&["0"])), Ok((0, BLACK, 255)));
        assert_eq!(
            parse_outline(&argv(&["1", "255", "255", "255"])),
            Ok((1, (255, 255, 255), 255))
        );
        assert_eq!(
            parse_outline(&argv(&["3", "10", "20", "30", "128"])),
            Ok((3, (10, 20, 30), 128))
        );
        assert!(parse_outline(&argv(&["5"])).is_err(), "too wide");
        assert!(parse_outline(&argv(&["-1"])).is_err());
        assert!(
            parse_outline(&argv(&["1", "255"])).is_err(),
            "half a colour"
        );
        assert!(parse_outline(&argv(&["1", "0", "0", "256"])).is_err());
        assert!(parse_outline(&argv(&["1", "0", "0", "0", "300"])).is_err());
        assert!(parse_outline(&argv(&[])).is_err());
    }

    #[test]
    fn background_arguments() {
        let line = |alpha, rgb, padding| {
            Ok(Background::Line {
                alpha,
                rgb,
                padding,
            })
        };
        assert_eq!(parse_background(&argv(&["160"])), line(160, BLACK, 4));
        assert_eq!(parse_background(&argv(&["160", "8"])), line(160, BLACK, 8));
        assert_eq!(
            parse_background(&argv(&["160", "20", "20", "40"])),
            line(160, (20, 20, 40), 4)
        );
        assert_eq!(
            parse_background(&argv(&["160", "20", "20", "40", "6"])),
            line(160, (20, 20, 40), 6)
        );
        assert_eq!(parse_background(&argv(&["default"])), Ok(Background::Off));
        assert_eq!(
            parse_background(&argv(&["Team", "200"])),
            Ok(Background::Team {
                alpha: 200,
                padding: 4
            })
        );
        assert_eq!(
            parse_background(&argv(&["team", "200", "2"])),
            Ok(Background::Team {
                alpha: 200,
                padding: 2
            })
        );
        assert_eq!(
            parse_background(&argv(&["allies", "0", "40", "0"])),
            Ok(Background::TeamColour(Team::Allies, Some((0, 40, 0))))
        );
        assert_eq!(
            parse_background(&argv(&["axis", "default"])),
            Ok(Background::TeamColour(Team::Axis, None))
        );
        assert!(parse_background(&argv(&["256"])).is_err());
        assert!(parse_background(&argv(&["team"])).is_err());
        assert!(parse_background(&argv(&["axis", "1", "2"])).is_err());
        assert!(parse_background(&argv(&["160", "1", "2"])).is_err());
        assert!(parse_background(&argv(&["160", "65"])).is_err(), "padding");
        assert!(parse_background(&argv(&[])).is_err());
    }

    #[test]
    fn colours_pack_and_unpack() {
        assert_eq!(unpack(pack((1, 2, 3), 4)), ((1, 2, 3), 4));
        assert_eq!(unpack(pack((300, -1, 255), 255)), ((255, 0, 255), 255));
        assert_ne!(pack((0, 0, 0), 255), UNSET, "black at 255 is a colour");
    }
}
