//! Draws the throws into the 3D world, from `HUD_DrawTransparentTriangles`,
//! where the engine has the world's view and projection set up and is done
//! with its own translucent pass.
//!
//! Plain OpenGL 1.1 out of `opengl32.dll`, the same immediate mode the engine
//! itself draws with, inside `glPushAttrib`/`glPopAttrib` so nothing it set
//! leaks out. Each throw is drawn twice when see-through is on: faded with
//! the depth test off, so a path behind a wall still shows, then solid with
//! it on, so the visible part reads clearly.

#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::sync::OnceLock;

use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

use super::track::Throw;

const GL_POINTS: u32 = 0x0000;
const GL_LINES: u32 = 0x0001;
const GL_LINE_LOOP: u32 = 0x0002;
const GL_LINE_STRIP: u32 = 0x0003;
const GL_TEXTURE_2D: u32 = 0x0DE1;
const GL_DEPTH_TEST: u32 = 0x0B71;
const GL_BLEND: u32 = 0x0BE2;
const GL_ALPHA_TEST: u32 = 0x0BC0;
const GL_CULL_FACE: u32 = 0x0B44;
const GL_FOG: u32 = 0x0B60;
const GL_SRC_ALPHA: u32 = 0x0302;
const GL_ONE_MINUS_SRC_ALPHA: u32 = 0x0303;
const GL_ALL_ATTRIB_BITS: u32 = 0x000F_FFFF;

/// One colour per throw, in turn.
const PALETTE: [[f32; 3]; 6] = [
    [1.0, 0.85, 0.1],
    [0.2, 0.9, 1.0],
    [1.0, 0.3, 0.9],
    [0.5, 1.0, 0.2],
    [1.0, 0.55, 0.1],
    [0.7, 0.6, 1.0],
];

/// The explosion marker: a ring on the ground and a post above it.
const RING_RADIUS: f32 = 24.0;
const RING_SEGMENTS: usize = 24;
const POST_HEIGHT: f32 = 40.0;
const BOUNCE_SIZE: f32 = 4.0;

struct Gl {
    push_attrib: unsafe extern "system" fn(u32),
    pop_attrib: unsafe extern "system" fn(),
    enable: unsafe extern "system" fn(u32),
    disable: unsafe extern "system" fn(u32),
    blend_func: unsafe extern "system" fn(u32, u32),
    depth_mask: unsafe extern "system" fn(u8),
    line_width: unsafe extern "system" fn(f32),
    point_size: unsafe extern "system" fn(f32),
    begin: unsafe extern "system" fn(u32),
    end: unsafe extern "system" fn(),
    color4f: unsafe extern "system" fn(f32, f32, f32, f32),
    vertex3f: unsafe extern "system" fn(f32, f32, f32),
}

fn gl() -> Option<&'static Gl> {
    static GL: OnceLock<Option<Gl>> = OnceLock::new();
    GL.get_or_init(|| {
        let module = unsafe { GetModuleHandleA(c"opengl32.dll".as_ptr() as *const u8) };
        if module.is_null() {
            unsafe { crate::debug::report("nade: opengl32.dll isn't loaded -- no trails") };
            return None;
        }
        macro_rules! f {
            ($name:literal, $($ty:tt)*) => {{
                let p = unsafe { GetProcAddress(module, $name.as_ptr() as *const u8) }?;
                unsafe {
                    std::mem::transmute::<unsafe extern "system" fn() -> isize, unsafe extern "system" fn $($ty)*>(p)
                }
            }};
        }
        Some(Gl {
            push_attrib: f!(c"glPushAttrib", (u32)),
            pop_attrib: f!(c"glPopAttrib", ()),
            enable: f!(c"glEnable", (u32)),
            disable: f!(c"glDisable", (u32)),
            blend_func: f!(c"glBlendFunc", (u32, u32)),
            depth_mask: f!(c"glDepthMask", (u8)),
            line_width: f!(c"glLineWidth", (f32)),
            point_size: f!(c"glPointSize", (f32)),
            begin: f!(c"glBegin", (u32)),
            end: f!(c"glEnd", ()),
            color4f: f!(c"glColor4f", (f32, f32, f32, f32)),
            vertex3f: f!(c"glVertex3f", (f32, f32, f32)),
        })
    })
    .as_ref()
}

pub fn colour(number: u32) -> [f32; 3] {
    PALETTE[number as usize % PALETTE.len()]
}

/// Draws every throw. Main thread, inside the engine's render pass.
pub fn throws(throws: &[Throw], see_through: bool) {
    if throws.is_empty() {
        return;
    }
    let Some(gl) = gl() else { return };
    unsafe {
        (gl.push_attrib)(GL_ALL_ATTRIB_BITS);
        (gl.disable)(GL_TEXTURE_2D);
        (gl.disable)(GL_ALPHA_TEST);
        (gl.disable)(GL_CULL_FACE);
        (gl.disable)(GL_FOG);
        (gl.enable)(GL_BLEND);
        (gl.blend_func)(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA);
        (gl.depth_mask)(0);

        if see_through {
            (gl.disable)(GL_DEPTH_TEST);
            for t in throws {
                one(gl, t, 0.3, 2.0);
            }
        }
        (gl.enable)(GL_DEPTH_TEST);
        for t in throws {
            one(gl, t, 1.0, 3.0);
        }

        (gl.pop_attrib)();
    }
}

unsafe fn one(gl: &Gl, t: &Throw, alpha: f32, width: f32) {
    let [r, g, b] = colour(t.number);
    unsafe {
        (gl.color4f)(r, g, b, alpha);
        (gl.line_width)(width);
        (gl.begin)(GL_LINE_STRIP);
        for p in &t.points {
            (gl.vertex3f)(p[0], p[1], p[2]);
        }
        (gl.end)();

        // Bounces: a small white cross where it hit something.
        (gl.color4f)(1.0, 1.0, 1.0, alpha);
        (gl.begin)(GL_LINES);
        for p in &t.bounces {
            for axis in 0..3 {
                let mut a = *p;
                let mut z = *p;
                a[axis] -= BOUNCE_SIZE;
                z[axis] += BOUNCE_SIZE;
                (gl.vertex3f)(a[0], a[1], a[2]);
                (gl.vertex3f)(z[0], z[1], z[2]);
            }
        }
        (gl.end)();
        (gl.point_size)(width * 2.5);
        (gl.begin)(GL_POINTS);
        for p in &t.bounces {
            (gl.vertex3f)(p[0], p[1], p[2]);
        }
        (gl.end)();

        if let Some(e) = t.end {
            (gl.color4f)(r, g, b, alpha);
            (gl.begin)(GL_LINE_LOOP);
            for k in 0..RING_SEGMENTS {
                let a = k as f32 / RING_SEGMENTS as f32 * std::f32::consts::TAU;
                (gl.vertex3f)(
                    e[0] + RING_RADIUS * a.cos(),
                    e[1] + RING_RADIUS * a.sin(),
                    e[2],
                );
            }
            (gl.end)();
            (gl.begin)(GL_LINES);
            (gl.vertex3f)(e[0], e[1], e[2]);
            (gl.vertex3f)(e[0], e[1], e[2] + POST_HEIGHT);
            (gl.end)();
        }
    }
}
