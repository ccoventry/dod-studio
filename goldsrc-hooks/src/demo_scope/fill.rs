//! The black around the sniper scope while a demo plays (#799).
//!
//! `DrawTriApiScope` draws the scope as four quads 5 units in front of the
//! eye, one per sprite frame, from the centre out to the corners. Each is
//! `5.5 * tan(fov / 2)` wide, `fov` being the HUD's, and as tall as
//! `CalcFov` makes that for the screen's aspect (`+0x467a3`..`+0x467f9`):
//! 10% past the edges of the view DoD expects. A wider view sees past them
//! and shows the map around the frame: `gl_widescreen_yfov 1` on the 25th
//! Anniversary engine keeps the 4:3 vertical FOV and widens the horizontal
//! one, and HLAE's `mirv_fov` sets the FOV outright.
//!
//! So for the length of `HUD_DrawTransparentTriangles` in a demo, with the
//! scope setting on, the client's copy of `pTriAPI` (`gEngfuncs+0x148`)
//! points at [`TABLE`], a copy of the engine's table whose `SpriteTexture`,
//! `TexCoord2f`, `Vertex3fv`/`Vertex3f` and `End` note the scope's quads.
//! After the fourth quad's `End`, four more quads go in the same plane, from
//! the frame's edges out past any FOV below 179.9 degrees: drawn with the
//! scope's own sprite, render mode and depth, every vertex on the frame's
//! outer-corner texel (palette index 255 in `SPR_INDEXALPHA`: opaque) and
//! tinted black. They sit where the frame sits, so they move with it and
//! need neither the screen size nor the FOV.
//!
//! Not a 2D fill: `pfnFillRGBA` adds its colour (`GL_ONE` blending), so black
//! draws nothing, and the HUD's 2D pass has no notion of where the 3D frame
//! landed. The live game keeps its own scope: the table is only swapped
//! while a demo plays.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicUsize, Ordering};

use crate::engine::{self, ModelSPartial, ScreenInfo, TriangleApiPartial};

/// The `mov edx, [gEngfuncs.pDemoAPI]` operand in [`super::PATTERN`]: the
/// client's own copy of the engine table, which `DrawTriApiScope` reads
/// TriAPI through.
pub(super) const DEMO_API_OPERAND_AT: usize = 51;
/// `pTriAPI` is three slots before `pDemoAPI` (`pTriAPI`, `pEfxAPI`,
/// `pEventAPI`, `pDemoAPI`).
const TRI_API_BEFORE_DEMO_API: usize = 12;
const EVENT_API_BEFORE_DEMO_API: usize = 4;

/// `triangleapi_s` through `FogParams`. DoD's client calls up to `Fog`
/// (`+0x34`), so a copy this long covers every slot it can reach.
const TABLE_SLOTS: usize = 20;
const SLOT_END: usize = 3;
const SLOT_TEX_COORD2F: usize = 6;
const SLOT_VERTEX3FV: usize = 7;
const SLOT_VERTEX3F: usize = 8;
const SLOT_SPRITE_TEXTURE: usize = 11;

const TRI_QUADS: i32 = 2;
/// `CullFace(TRI_FRONT)`, what the scope sets and so what it leaves.
const TRI_FRONT: i32 = 0;
const TRI_NONE: i32 = 1;

/// The scope's quads per frame: frames 0..3 of its sprite.
const SCOPE_QUADS: u32 = 4;
const MAX_VERTICES: usize = 16;
/// How far the bands reach, in multiples of the frame's depth: past the
/// edge of any FOV below `2 * atan(1000)`, 179.9 degrees.
const REACH: f32 = 1000.0;
/// How far the bands reach back into the frame, as a fraction of its
/// half-size, so no seam shows at its edge (the frame is black there).
const OVERLAP: f32 = 0.02;

/// The table the client reads TriAPI through while it is swapped: the
/// engine's own pointers, five of them ours. Plain words in memory, as the
/// client reads them.
static TABLE: [AtomicUsize; TABLE_SLOTS] = [const { AtomicUsize::new(0) }; TABLE_SLOTS];
/// The engine's table [`TABLE`] was copied from, which the wrappers call.
static ENGINE_TRI: AtomicPtr<TriangleApiPartial> = AtomicPtr::new(std::ptr::null_mut());
static COMPLAINED: AtomicBool = AtomicBool::new(false);
/// Whether this playback has logged its first fill yet.
static LOGGED: AtomicBool = AtomicBool::new(false);

/// What the last scope drawn did, for `status_line`.
static LAST: AtomicU8 = AtomicU8::new(Last::None as u8);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
enum Last {
    None = 0,
    Filled = 1,
    Covered = 2,
    Refused = 3,
}

#[derive(Clone, Copy, Default, Debug, PartialEq)]
struct Vertex {
    position: [f32; 3],
    uv: [f32; 2],
    frame: i32,
}

/// The scope's quads as they are drawn this frame.
#[derive(Clone, Copy, Default)]
struct Capture {
    /// The texture last set is a scope sprite.
    scope: bool,
    model: usize,
    frame: i32,
    uv: [f32; 2],
    vertices: [Vertex; MAX_VERTICES],
    count: usize,
    quads: u32,
}

impl Capture {
    fn texture(&mut self, scope: bool, model: usize, frame: i32) {
        if scope && (!self.scope || model != self.model) {
            *self = Capture::default();
        }
        self.scope = scope;
        if scope {
            self.model = model;
            self.frame = frame;
        }
    }

    fn vertex(&mut self, position: [f32; 3]) {
        if !self.scope || self.count >= MAX_VERTICES {
            return;
        }
        self.vertices[self.count] = Vertex {
            position,
            uv: self.uv,
            frame: self.frame,
        };
        self.count += 1;
    }

    /// Counts a finished quad; after the scope's last one, hands its quads
    /// over and starts again.
    fn quad_ended(&mut self) -> Option<Capture> {
        if !self.scope {
            return None;
        }
        self.quads += 1;
        if self.quads < SCOPE_QUADS {
            return None;
        }
        let done = *self;
        *self = Capture::default();
        Some(done)
    }
}

thread_local! {
    /// The engine's render thread only: the wrappers run inside
    /// `HUD_DrawTransparentTriangles`.
    static CAPTURE: RefCell<Capture> = RefCell::new(Capture::default());
}

fn with_capture<T>(f: impl FnOnce(&mut Capture) -> T) -> Option<T> {
    CAPTURE
        .try_with(|cell| cell.try_borrow_mut().ok().map(|mut c| f(&mut c)))
        .ok()
        .flatten()
}

/// `sprites/scope_spring.spr` and the rest of the scope sprites.
fn is_scope_sprite(name: &str) -> bool {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    file.len() > 6
        && file[..6].eq_ignore_ascii_case("scope_")
        && file.to_ascii_lowercase().ends_with(".spr")
}

/// The render view: origin, then `GetViewInfo`'s right, up and forward.
#[derive(Clone, Copy, Debug)]
struct View {
    origin: [f32; 3],
    right: [f32; 3],
    up: [f32; 3],
    forward: [f32; 3],
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

impl View {
    /// A world point as (right, up, forward) from the eye.
    fn plane_point(&self, p: [f32; 3]) -> [f32; 3] {
        let d = [
            p[0] - self.origin[0],
            p[1] - self.origin[1],
            p[2] - self.origin[2],
        ];
        [dot(d, self.right), dot(d, self.up), dot(d, self.forward)]
    }

    fn world_point(&self, x: f32, y: f32, depth: f32) -> [f32; 3] {
        std::array::from_fn(|k| {
            self.origin[k] + self.right[k] * x + self.up[k] * y + self.forward[k] * depth
        })
    }
}

/// The scope's frame in its plane, in the view's right/up units.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PlaneRect {
    left: f32,
    right: f32,
    bottom: f32,
    top: f32,
    depth: f32,
}

/// The rectangle the scope's vertices span, or `None` when there is none
/// or it is not in front of the eye.
fn plane_rect(points: &[[f32; 3]]) -> Option<PlaneRect> {
    let first = points.first()?;
    let mut rect = PlaneRect {
        left: first[0],
        right: first[0],
        bottom: first[1],
        top: first[1],
        depth: 0.0,
    };
    for p in points {
        if !p.iter().all(|v| v.is_finite()) {
            return None;
        }
        rect.left = rect.left.min(p[0]);
        rect.right = rect.right.max(p[0]);
        rect.bottom = rect.bottom.min(p[1]);
        rect.top = rect.top.max(p[1]);
        rect.depth += p[2];
    }
    rect.depth /= points.len() as f32;
    let reach = rect.depth * REACH;
    let fits = [rect.left, rect.right, rect.bottom, rect.top]
        .iter()
        .all(|v| v.abs() < reach);
    (rect.depth > 0.0 && rect.right > rect.left && rect.top > rect.bottom && fits).then_some(rect)
}

/// The four bands around `rect` (left, right, bottom, top), each a quad of
/// (right, up) corners, together covering the plane out to `REACH` times
/// its depth, with `OVERLAP` into the frame.
fn bands(rect: &PlaneRect) -> [[[f32; 2]; 4]; 4] {
    let reach = rect.depth * REACH;
    let in_x = (rect.right - rect.left) * 0.5 * OVERLAP;
    let in_y = (rect.top - rect.bottom) * 0.5 * OVERLAP;
    let (l, r) = (rect.left + in_x, rect.right - in_x);
    let (b, t) = (rect.bottom + in_y, rect.top - in_y);
    [
        [[-reach, -reach], [l, -reach], [l, reach], [-reach, reach]],
        [[r, -reach], [reach, -reach], [reach, reach], [r, reach]],
        [[l, -reach], [r, -reach], [r, b], [l, b]],
        [[l, t], [r, t], [r, reach], [l, reach]],
    ]
}

/// The texture coordinate of the frame's outer corner in sprite frame
/// `frame`: that frame's vertex farthest from the frame's centre.
fn outer_corner_uv(vertices: &[Vertex], planar: &[[f32; 3]], frame: i32) -> Option<[f32; 2]> {
    let rect = plane_rect(planar)?;
    let (cx, cy) = (
        (rect.left + rect.right) * 0.5,
        (rect.bottom + rect.top) * 0.5,
    );
    vertices
        .iter()
        .zip(planar)
        .filter(|(v, _)| v.frame == frame)
        .max_by(|(_, a), (_, b)| {
            let da = (a[0] - cx).abs() + (a[1] - cy).abs();
            let db = (b[0] - cx).abs() + (b[1] - cy).abs();
            da.total_cmp(&db)
        })
        .map(|(v, _)| v.uv)
}

/// Normalised screen bounds (min x, min y, max x, max y; -1..1, y up).
type ScreenRect = [f32; 4];

fn screen_bounds(corners: &[[f32; 2]; 4]) -> ScreenRect {
    corners.iter().fold(
        [f32::MAX, f32::MAX, f32::MIN, f32::MIN],
        |[x0, y0, x1, y1], [x, y]| [x0.min(*x), y0.min(*y), x1.max(*x), y1.max(*y)],
    )
}

/// Whether the frame already covers the whole screen, so no band shows.
fn covers_screen(bounds: &ScreenRect) -> bool {
    bounds[0] <= -1.0 && bounds[1] <= -1.0 && bounds[2] >= 1.0 && bounds[3] >= 1.0
}

/// `bounds` in pixels on a `width` x `height` screen: left, top, right,
/// bottom, clamped to the screen.
fn to_pixels(bounds: &ScreenRect, width: i32, height: i32) -> [i32; 4] {
    let (w, h) = (width as f32, height as f32);
    let x = |v: f32| ((1.0 + v) * 0.5 * w).round().clamp(0.0, w) as i32;
    let y = |v: f32| ((1.0 - v) * 0.5 * h).round().clamp(0.0, h) as i32;
    [x(bounds[0]), y(bounds[3]), x(bounds[2]), y(bounds[1])]
}

fn engine_tri() -> Option<&'static TriangleApiPartial> {
    let tri = ENGINE_TRI.load(Ordering::Acquire);
    // Safety: the engine's own table, for the session.
    (!tri.is_null()).then(|| unsafe { &*tri })
}

unsafe extern "C" fn sprite_texture(model: *mut ModelSPartial, frame: i32) -> i32 {
    // Safety: a non-null model the client got from the engine.
    let scope = !model.is_null() && is_scope_sprite(&unsafe { (*model).name_str() });
    with_capture(|c| c.texture(scope, model as usize, frame));
    match engine_tri() {
        Some(tri) => unsafe { (tri.sprite_texture)(model, frame) },
        None => 0,
    }
}

unsafe extern "C" fn tex_coord2f(u: f32, v: f32) {
    with_capture(|c| c.uv = [u, v]);
    if let Some(tri) = engine_tri() {
        unsafe { (tri.tex_coord2f)(u, v) };
    }
}

unsafe extern "C" fn vertex3fv(point: *const f32) {
    if !point.is_null() {
        // Safety: TriAPI's contract, three floats.
        let p = unsafe { std::ptr::read_unaligned(point as *const [f32; 3]) };
        with_capture(|c| c.vertex(p));
    }
    if let Some(tri) = engine_tri() {
        unsafe { (tri.vertex3fv)(point) };
    }
}

unsafe extern "C" fn vertex3f(x: f32, y: f32, z: f32) {
    with_capture(|c| c.vertex([x, y, z]));
    if let Some(tri) = engine_tri() {
        unsafe { (tri.vertex3f)(x, y, z) };
    }
}

unsafe extern "C" fn end() {
    let Some(tri) = engine_tri() else {
        return;
    };
    unsafe { (tri.end)() };
    if let Some(done) = with_capture(Capture::quad_ended).flatten() {
        fill_around(tri, &done);
    }
}

fn render_view() -> Option<View> {
    let studio = engine::engine_studio()?;
    let (mut origin, mut up, mut right, mut forward) = ([0f32; 3], [0f32; 3], [0f32; 3], [0f32; 3]);
    // Safety: the engine's own accessor, writing three floats into each.
    unsafe {
        (studio.get_view_info)(
            origin.as_mut_ptr(),
            up.as_mut_ptr(),
            right.as_mut_ptr(),
            forward.as_mut_ptr(),
        )
    };
    Some(View {
        origin,
        right,
        up,
        forward,
    })
}

/// Draws the bands around the scope `done` just drew, right after its last
/// quad, leaving the state as the scope left it.
fn fill_around(tri: &TriangleApiPartial, done: &Capture) {
    let Some(view) = render_view() else {
        return;
    };
    let vertices = &done.vertices[..done.count.min(MAX_VERTICES)];
    let mut planar = [[0f32; 3]; MAX_VERTICES];
    for (slot, v) in planar.iter_mut().zip(vertices) {
        *slot = view.plane_point(v.position);
    }
    let planar = &planar[..vertices.len()];
    let (Some(rect), Some([u, v])) = (
        plane_rect(planar),
        outer_corner_uv(vertices, planar, done.frame),
    ) else {
        return;
    };

    let corners = [
        [rect.left, rect.bottom],
        [rect.right, rect.bottom],
        [rect.right, rect.top],
        [rect.left, rect.top],
    ];
    let mut screen = [[0f32; 2]; 4];
    let mut on_screen = true;
    for (out, [x, y]) in screen.iter_mut().zip(corners) {
        let world = view.world_point(x, y, rect.depth);
        let mut s = [0f32; 3];
        // Safety: three floats in, three out.
        on_screen &= unsafe { (tri.world_to_screen)(world.as_ptr(), s.as_mut_ptr()) } == 0;
        *out = [s[0], s[1]];
    }
    let bounds = screen_bounds(&screen);
    if on_screen && covers_screen(&bounds) {
        LAST.store(Last::Covered as u8, Ordering::Relaxed);
        return;
    }

    // Safety: the engine's TriAPI, on its render thread, inside the
    // transparent pass the scope itself draws in.
    unsafe {
        (tri.sprite_texture)(done.model as *mut ModelSPartial, done.frame);
        (tri.cull_face)(TRI_NONE);
        (tri.color4f)(0.0, 0.0, 0.0, 1.0);
        (tri.begin)(TRI_QUADS);
        for band in bands(&rect) {
            for [x, y] in band {
                (tri.tex_coord2f)(u, v);
                (tri.vertex3fv)(view.world_point(x, y, rect.depth).as_ptr());
            }
        }
        (tri.end)();
        (tri.color4f)(1.0, 1.0, 1.0, 1.0);
        (tri.cull_face)(TRI_FRONT);
    }
    LAST.store(Last::Filled as u8, Ordering::Relaxed);
    if !LOGGED.swap(true, Ordering::Relaxed) {
        log_first_fill(&bounds, on_screen);
    }
}

fn log_first_fill(bounds: &ScreenRect, on_screen: bool) {
    let size = engine::engfuncs().and_then(|e| {
        let mut info = ScreenInfo {
            size: size_of::<ScreenInfo>() as i32,
            width: 0,
            height: 0,
            flags: 0,
            char_height: 0,
            char_widths: [0; 256],
        };
        // Safety: the engine fills the struct whose size it is told.
        unsafe { (e.pfn_get_screen_info)(&mut info) };
        (info.width > 0 && info.height > 0).then_some((info.width, info.height))
    });
    let frame = match (size, on_screen) {
        (Some((w, h)), true) => {
            let [x0, y0, x1, y1] = to_pixels(bounds, w, h);
            format!("{w}x{h} screen, the scope's frame spans x {x0}..{x1}, y {y0}..{y1}")
        }
        (Some((w, h)), false) => {
            format!("{w}x{h} screen, the scope's frame reaches behind the eye")
        }
        (None, _) => format!(
            "the scope's frame spans {:.2}..{:.2} x {:.2}..{:.2} of the screen (-1..1)",
            bounds[0], bounds[2], bounds[1], bounds[3]
        ),
    };
    // Safety: the hook log, from the engine's thread.
    unsafe {
        crate::debug::report(&format!(
            "demo_scope: black fill around the scope -- {frame}"
        ))
    };
}

/// The client's `gEngfuncs.pTriAPI` and the engine's table, once both agree
/// with the engine's own `cl_enginefuncs_s`.
fn tri_slot() -> Result<(usize, *mut TriangleApiPartial), String> {
    let engfuncs = engine::engfuncs().ok_or("the engine's function table is not captured yet")?;
    let base = engine::client_module_base().ok_or("client.dll is not loaded yet")?;
    let site = super::site()?;
    // Safety: inside the pattern `site` matched, so mapped code.
    let demo_slot =
        unsafe { std::ptr::read_unaligned((site + DEMO_API_OPERAND_AT) as *const u32) } as usize;
    if demo_slot < base + TRI_API_BEFORE_DEMO_API {
        return Err(format!(
            "the scope's pDemoAPI operand {demo_slot:#x} is not in client.dll"
        ));
    }
    let tri_slot = demo_slot - TRI_API_BEFORE_DEMO_API;
    // Safety: the client's copy of the engine table, in its own data.
    let (demo, event, tri) = unsafe {
        (
            *(demo_slot as *const usize),
            *((demo_slot - EVENT_API_BEFORE_DEMO_API) as *const usize),
            *(tri_slot as *const usize),
        )
    };
    let engine_tri = engfuncs.p_tri_api;
    let ours = TABLE.as_ptr() as usize;
    if demo != engfuncs.p_demo_api as usize
        || event != engfuncs.p_event_api as usize
        || (tri != engine_tri as usize && tri != ours)
        || engine_tri.is_null()
    {
        return Err(format!(
            "client.dll's TriAPI pointer at {tri_slot:#x} does not match the engine's table"
        ));
    }
    Ok((tri_slot, engine_tri))
}

fn fill_table(engine_tri: *mut TriangleApiPartial) {
    if ENGINE_TRI.load(Ordering::Acquire) == engine_tri {
        return;
    }
    // Safety: the engine's TriAPI table, `TABLE_SLOTS` words long on both
    // engine builds.
    let words = unsafe { std::ptr::read_unaligned(engine_tri as *const [usize; TABLE_SLOTS]) };
    for (cell, word) in TABLE.iter().zip(words) {
        cell.store(word, Ordering::Relaxed);
    }
    let ours: [(usize, usize); 5] = [
        (SLOT_END, end as unsafe extern "C" fn() as usize),
        (
            SLOT_TEX_COORD2F,
            tex_coord2f as unsafe extern "C" fn(f32, f32) as usize,
        ),
        (
            SLOT_VERTEX3FV,
            vertex3fv as unsafe extern "C" fn(*const f32) as usize,
        ),
        (
            SLOT_VERTEX3F,
            vertex3f as unsafe extern "C" fn(f32, f32, f32) as usize,
        ),
        (
            SLOT_SPRITE_TEXTURE,
            sprite_texture as unsafe extern "C" fn(*mut ModelSPartial, i32) -> i32 as usize,
        ),
    ];
    for (slot, f) in ours {
        TABLE[slot].store(f, Ordering::Relaxed);
    }
    ENGINE_TRI.store(engine_tri, Ordering::Release);
}

/// Runs `draw` (the client's `HUD_DrawTransparentTriangles`) with the black
/// fill around the scope when a demo plays with the setting on, else as is.
pub fn with_black_fill(draw: impl FnOnce()) {
    if !super::ON.load(Ordering::Relaxed) || !super::playing_demo() {
        draw();
        return;
    }
    let (slot, engine_tri) = match tri_slot() {
        Ok(found) => found,
        Err(why) => {
            LAST.store(Last::Refused as u8, Ordering::Relaxed);
            if !COMPLAINED.swap(true, Ordering::Relaxed) {
                unsafe {
                    crate::debug::report(&format!(
                        "demo_scope: no black fill around the scope -- {why}"
                    ))
                };
            }
            draw();
            return;
        }
    };
    fill_table(engine_tri);
    with_capture(|c| *c = Capture::default());
    let slot = slot as *mut usize;
    // Safety: the client's own data word, checked by `tri_slot`; the engine
    // thread is the only one that reads it.
    unsafe {
        slot.write(TABLE.as_ptr() as usize);
        draw();
        slot.write(engine_tri as usize);
    }
}

/// Called every frame: a new playback logs its first fill again.
pub fn poll(in_demo: bool) {
    if !in_demo {
        LOGGED.store(false, Ordering::Relaxed);
    }
}

/// For `status_line`.
pub fn describe_last() -> &'static str {
    match LAST.load(Ordering::Relaxed) {
        1 => "black fill outside the frame drawn",
        2 => "the frame covers the screen, no fill needed",
        3 => "black fill refused, see the hook log",
        _ => "no scope drawn yet",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> PlaneRect {
        PlaneRect {
            left: -5.5,
            right: 5.5,
            bottom: -4.1,
            top: 4.1,
            depth: 5.0,
        }
    }

    #[test]
    fn the_pdemoapi_operand_follows_its_load() {
        let t: Vec<&str> = super::super::PATTERN.split_whitespace().collect();
        assert_eq!(
            &t[DEMO_API_OPERAND_AT - 2..DEMO_API_OPERAND_AT],
            &["8B", "15"]
        );
        assert!(
            t[DEMO_API_OPERAND_AT..DEMO_API_OPERAND_AT + 4]
                .iter()
                .all(|b| *b == "??")
        );
        assert_eq!(
            &t[DEMO_API_OPERAND_AT + 4..DEMO_API_OPERAND_AT + 7],
            &["FF", "52", "04"],
            "call [edx+4], IsPlayingback"
        );
    }

    #[test]
    fn the_table_slots_match_the_struct() {
        let word = size_of::<usize>();
        assert_eq!(
            std::mem::offset_of!(TriangleApiPartial, end),
            SLOT_END * word
        );
        assert_eq!(
            std::mem::offset_of!(TriangleApiPartial, tex_coord2f),
            SLOT_TEX_COORD2F * word
        );
        assert_eq!(
            std::mem::offset_of!(TriangleApiPartial, vertex3fv),
            SLOT_VERTEX3FV * word
        );
        assert_eq!(
            std::mem::offset_of!(TriangleApiPartial, vertex3f),
            SLOT_VERTEX3F * word
        );
        assert_eq!(
            std::mem::offset_of!(TriangleApiPartial, sprite_texture),
            SLOT_SPRITE_TEXTURE * word
        );
        assert!(size_of::<TriangleApiPartial>() <= TABLE_SLOTS * word);
    }

    #[test]
    fn scope_sprites_are_told_apart() {
        for name in [
            "sprites/scope_spring.spr",
            "sprites/scope_k43.spr",
            "sprites/scope_enfield.spr",
            "sprites/scope_binoc.spr",
            "SPRITES\\SCOPE_K43.SPR",
        ] {
            assert!(is_scope_sprite(name), "{name}");
        }
        for name in [
            "sprites/scope.spr",
            "sprites/scopeX.spr",
            "sprites/scope_k43.mdl",
            "sprites/hud_scope_k43.spr",
            "",
        ] {
            assert!(!is_scope_sprite(name), "{name}");
        }
    }

    #[test]
    fn the_capture_hands_over_after_the_fourth_quad() {
        let mut c = Capture::default();
        for frame in 0..4 {
            c.texture(true, 0x1234, frame);
            for k in 0..4 {
                c.uv = [k as f32, frame as f32];
                c.vertex([frame as f32, k as f32, 5.0]);
            }
            let done = c.quad_ended();
            assert_eq!(done.is_some(), frame == 3, "frame {frame}");
            if let Some(done) = done {
                assert_eq!(done.count, 16);
                assert_eq!(done.frame, 3);
                assert_eq!(done.vertices[15].uv, [3.0, 3.0]);
            }
        }
        assert_eq!(c.count, 0, "starts again");
    }

    #[test]
    fn other_textures_are_not_captured() {
        let mut c = Capture::default();
        c.texture(false, 0x99, 0);
        c.vertex([1.0, 2.0, 3.0]);
        assert_eq!(c.count, 0);
        assert!(c.quad_ended().is_none());
        // A scope quad, then something else, then the scope again from a
        // fresh start.
        c.texture(true, 0x1234, 0);
        c.vertex([1.0, 2.0, 3.0]);
        c.quad_ended();
        c.texture(false, 0x99, 0);
        c.texture(true, 0x1234, 0);
        assert_eq!((c.count, c.quads), (0, 0));
    }

    #[test]
    fn the_rect_spans_the_points_at_their_depth() {
        let points = [
            [-5.5, -4.1, 5.0],
            [5.5, -4.1, 5.0],
            [5.5, 4.1, 5.0],
            [-5.5, 4.1, 5.0],
            [0.0, 0.0, 5.0],
        ];
        assert_eq!(plane_rect(&points), Some(rect()));
    }

    #[test]
    fn no_rect_behind_the_eye_or_flat_or_broken() {
        assert_eq!(plane_rect(&[]), None);
        assert_eq!(plane_rect(&[[-1.0, -1.0, -5.0], [1.0, 1.0, -5.0]]), None);
        assert_eq!(plane_rect(&[[-1.0, 0.0, 5.0], [1.0, 0.0, 5.0]]), None);
        assert_eq!(plane_rect(&[[f32::NAN, 0.0, 5.0], [1.0, 1.0, 5.0]]), None);
        assert_eq!(plane_rect(&[[-1e5, -1.0, 5.0], [1.0, 1.0, 5.0]]), None);
    }

    fn inside(quad: &[[f32; 2]; 4], x: f32, y: f32) -> bool {
        let r = screen_bounds(quad);
        x >= r[0] && x <= r[2] && y >= r[1] && y <= r[3]
    }

    #[test]
    fn the_bands_cover_everything_outside_the_frame() {
        let r = rect();
        let b = bands(&r);
        let reach = r.depth * REACH;
        // A grid over the plane out to just inside the reach.
        let steps = 200;
        for i in 0..=steps {
            for j in 0..=steps {
                let x = -reach * 0.999 + 2.0 * reach * 0.999 * i as f32 / steps as f32;
                let y = -reach * 0.999 + 2.0 * reach * 0.999 * j as f32 / steps as f32;
                let in_frame = x > r.left && x < r.right && y > r.bottom && y < r.top;
                let covered = b.iter().any(|q| inside(q, x, y));
                assert!(in_frame || covered, "({x}, {y}) uncovered");
            }
        }
        // Near the frame's edges too, where the grid is coarse.
        for (x, y) in [(-5.6, 0.0), (5.6, 0.0), (0.0, 4.2), (0.0, -4.2), (5.6, 4.2)] {
            assert!(b.iter().any(|q| inside(q, x, y)), "({x}, {y})");
        }
    }

    #[test]
    fn the_bands_leave_the_scope_itself_alone() {
        let r = rect();
        let b = bands(&r);
        // Everything more than the overlap inside the frame stays clear.
        for (x, y) in [
            (0.0, 0.0),
            (-5.3, 0.0),
            (5.3, 0.0),
            (0.0, 3.9),
            (0.0, -3.9),
            (5.3, 3.9),
        ] {
            assert!(!b.iter().any(|q| inside(q, x, y)), "({x}, {y})");
        }
        // And they overlap the frame's own black edge a little.
        assert!(b.iter().any(|q| inside(q, -5.45, 0.0)));
    }

    #[test]
    fn the_outer_corner_is_the_frames_far_vertex() {
        let vertices: Vec<Vertex> = [
            ([0.0, 0.0, 5.0], [0.99, 0.99], 3),
            ([5.5, 0.0, 5.0], [0.01, 0.99], 3),
            ([5.5, -4.1, 5.0], [0.01, 0.01], 3),
            ([0.0, -4.1, 5.0], [0.99, 0.01], 3),
            ([-5.5, 4.1, 5.0], [0.5, 0.5], 0),
        ]
        .into_iter()
        .map(|(position, uv, frame)| Vertex {
            position,
            uv,
            frame,
        })
        .collect();
        let planar: Vec<[f32; 3]> = vertices.iter().map(|v| v.position).collect();
        assert_eq!(outer_corner_uv(&vertices, &planar, 3), Some([0.01, 0.01]));
        assert_eq!(outer_corner_uv(&vertices, &planar, 0), Some([0.5, 0.5]));
        assert_eq!(outer_corner_uv(&vertices, &planar, 2), None);
    }

    #[test]
    fn the_view_round_trips_a_point() {
        let view = View {
            origin: [100.0, -20.0, 64.0],
            right: [0.0, -1.0, 0.0],
            up: [0.0, 0.0, 1.0],
            forward: [1.0, 0.0, 0.0],
        };
        let world = view.world_point(2.0, 3.0, 5.0);
        assert_eq!(world, [105.0, -22.0, 67.0]);
        assert_eq!(view.plane_point(world), [2.0, 3.0, 5.0]);
    }

    #[test]
    fn a_frame_past_every_edge_needs_no_fill() {
        assert!(covers_screen(&[-1.1, -1.05, 1.1, 1.05]));
        assert!(covers_screen(&[-1.0, -1.0, 1.0, 1.0]));
        // gl_widescreen_yfov 1 at 16:9: short on every side.
        assert!(!covers_screen(&[-0.83, -0.9, 0.83, 0.9]));
        assert!(!covers_screen(&[-1.1, -0.9, 1.1, 1.1]));
    }

    #[test]
    fn pixels_flip_y_and_clamp() {
        assert_eq!(
            to_pixels(&[-0.5, -0.5, 0.5, 0.5], 2560, 1440),
            [640, 360, 1920, 1080]
        );
        assert_eq!(
            to_pixels(&[-2.0, -2.0, 2.0, 2.0], 1280, 720),
            [0, 0, 1280, 720]
        );
        assert_eq!(to_pixels(&[0.0, 0.5, 1.0, 1.0], 100, 100), [50, 0, 100, 25]);
    }
}
