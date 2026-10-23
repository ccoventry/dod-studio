//! Drawing HUD sprites S times bigger: one engine draw call under a modelview
//! zoom about a screen point.
//!
//! Nothing in the engine or DoD's client scales a HUD sprite: `pfnSPR_Draw*`
//! draws exactly its rect's pixels. So the modules that scale one
//! (`deathmsg::scale` for the kill feed, `objicons::scale` for the objective
//! icons and timer) hand the engine its own, unscaled call and wrap it in
//! `translate(c) * scale(S, S) * translate(-c)`. The engine then draws its own
//! quad (its texture coordinates, alpha test, `gl_spriteblend`, scissor and
//! `GL_Bind` cache) S times bigger about `c`. `glTranslatef`/`glScalef`
//! post-multiply the current matrix, so this is the engine's pixel space
//! whatever matrix the HUD pass has current; the push/pop leave nothing
//! behind.
//!
//! The calls come from `opengl32.dll`, the engine's own GL, loaded before
//! anything draws: not the engine's `qgl*` slots (per-build addresses) and
//! not TriAPI (no matrix calls).

// The GL calls run only in the 32-bit DLL; a host build keeps the signatures.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code, unused_imports))]

/// Whether the GL calls can be had, loading them on first use. A failure is
/// not cached, so a later try can succeed.
pub(crate) fn ready() -> Result<(), String> {
    #[cfg(target_arch = "x86")]
    {
        gl::get().map(|_| ())
    }
    #[cfg(not(target_arch = "x86"))]
    {
        Err("only in the 32-bit game".to_string())
    }
}

/// Runs `draw` under a modelview zoom of `s` about `(cx, cy)` in screen
/// pixels. Returns `false`, without calling `draw`, when the GL calls cannot
/// be had; the caller then draws unscaled.
///
/// Safety: on the engine's thread with its GL context current (inside a HUD
/// draw), which is where every caller is.
pub(crate) unsafe fn about(cx: f32, cy: f32, s: f32, draw: impl FnOnce()) -> bool {
    // Safety: the caller's.
    unsafe { mapped((cx, cy), (cx, cy), s, draw) }
}

/// Runs `draw` with the point `from` moved to `to` and everything about it
/// `s` times bigger: `translate(to) * scale(s) * translate(-from)`. [`about`]
/// is the case `from == to`. Returns `false`, without calling `draw`, when
/// the GL calls cannot be had.
///
/// Safety: as [`about`].
pub(crate) unsafe fn mapped(from: (f32, f32), to: (f32, f32), s: f32, draw: impl FnOnce()) -> bool {
    #[cfg(target_arch = "x86")]
    {
        let Ok(gl) = gl::get() else {
            return false;
        };
        // Safety: see the function's own; balanced push/pop, the matrix mode
        // put back if it was changed.
        unsafe { gl.zoomed(from, to, s, draw) };
        true
    }
    #[cfg(not(target_arch = "x86"))]
    {
        let _ = (from, to, s, draw);
        false
    }
}

#[cfg(target_arch = "x86")]
mod gl {
    use std::sync::OnceLock;

    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    const GL_MATRIX_MODE: u32 = 0x0ba0;
    const GL_MODELVIEW: u32 = 0x1700;

    type GetIntegervFn = unsafe extern "system" fn(u32, *mut i32);
    type MatrixModeFn = unsafe extern "system" fn(u32);
    type NoArgFn = unsafe extern "system" fn();
    type ThreeFloatsFn = unsafe extern "system" fn(f32, f32, f32);

    pub(super) struct Gl {
        get_integerv: GetIntegervFn,
        matrix_mode: MatrixModeFn,
        push_matrix: NoArgFn,
        pop_matrix: NoArgFn,
        translatef: ThreeFloatsFn,
        scalef: ThreeFloatsFn,
    }

    impl Gl {
        pub(super) unsafe fn zoomed(
            &self,
            from: (f32, f32),
            to: (f32, f32),
            s: f32,
            draw: impl FnOnce(),
        ) {
            // Safety: the caller's (engine thread, context current).
            unsafe {
                let mut mode: i32 = GL_MODELVIEW as i32;
                (self.get_integerv)(GL_MATRIX_MODE, &mut mode);
                if mode != GL_MODELVIEW as i32 {
                    (self.matrix_mode)(GL_MODELVIEW);
                }
                (self.push_matrix)();
                (self.translatef)(to.0, to.1, 0.0);
                (self.scalef)(s, s, 1.0);
                (self.translatef)(-from.0, -from.1, 0.0);
                draw();
                (self.pop_matrix)();
                if mode != GL_MODELVIEW as i32 {
                    (self.matrix_mode)(mode as u32);
                }
            }
        }
    }

    static GL: OnceLock<Gl> = OnceLock::new();

    fn load() -> Result<Gl, String> {
        let module = unsafe { GetModuleHandleA(c"opengl32.dll".as_ptr() as *const u8) };
        if module.is_null() {
            return Err("opengl32.dll is not loaded".to_string());
        }
        let proc = |name: &std::ffi::CStr| -> Result<usize, String> {
            unsafe { GetProcAddress(module, name.as_ptr() as *const u8) }
                .map(|p| p as usize)
                .ok_or_else(|| format!("opengl32.dll has no {}", name.to_string_lossy()))
        };
        // Safety: the exported functions' own signatures (stdcall).
        unsafe {
            Ok(Gl {
                get_integerv: std::mem::transmute::<usize, GetIntegervFn>(proc(c"glGetIntegerv")?),
                matrix_mode: std::mem::transmute::<usize, MatrixModeFn>(proc(c"glMatrixMode")?),
                push_matrix: std::mem::transmute::<usize, NoArgFn>(proc(c"glPushMatrix")?),
                pop_matrix: std::mem::transmute::<usize, NoArgFn>(proc(c"glPopMatrix")?),
                translatef: std::mem::transmute::<usize, ThreeFloatsFn>(proc(c"glTranslatef")?),
                scalef: std::mem::transmute::<usize, ThreeFloatsFn>(proc(c"glScalef")?),
            })
        }
    }

    pub(super) fn get() -> Result<&'static Gl, String> {
        if let Some(gl) = GL.get() {
            return Ok(gl);
        }
        let loaded = load()?;
        let _ = GL.set(loaded);
        GL.get().ok_or_else(|| "GL functions not kept".to_string())
    }
}
