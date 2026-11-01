//! The GL state around x-ray's shell pass (#1297): the depth test turned
//! round (or off, for the outline round players in view), no depth writes and
//! no texture, all put back afterwards. From `opengl32.dll`'s own exports,
//! the engine's GL, on the engine's thread.

/// Runs `draw` with the depth test passing only behind what is already
/// drawn (`everywhere`: passing everywhere), no depth writes and texturing
/// off, then puts all three back. `false` when GL is not available (nothing
/// drawn).
///
/// Safety: on the engine's thread, with its GL context current.
pub(super) unsafe fn shell_pass(everywhere: bool, draw: impl FnOnce()) -> bool {
    match imp::get() {
        Some(gl) => {
            unsafe { gl.shell_pass(everywhere, draw) };
            true
        }
        None => false,
    }
}

#[cfg(target_arch = "x86")]
mod imp {
    use std::sync::OnceLock;

    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    const GL_DEPTH_FUNC: u32 = 0x0b74;
    const GL_DEPTH_WRITEMASK: u32 = 0x0b72;
    const GL_TEXTURE_2D: u32 = 0x0de1;
    const GL_LESS: u32 = 0x0201;
    const GL_GREATER: u32 = 0x0204;
    const GL_GEQUAL: u32 = 0x0206;
    const GL_ALWAYS: u32 = 0x0207;

    type GetIntegervFn = unsafe extern "system" fn(u32, *mut i32);
    type GetBooleanvFn = unsafe extern "system" fn(u32, *mut u8);
    type EnumFn = unsafe extern "system" fn(u32);
    type MaskFn = unsafe extern "system" fn(u8);
    type IsEnabledFn = unsafe extern "system" fn(u32) -> u8;

    pub(super) struct Gl {
        get_integerv: GetIntegervFn,
        get_booleanv: GetBooleanvFn,
        depth_func: EnumFn,
        depth_mask: MaskFn,
        enable: EnumFn,
        disable: EnumFn,
        is_enabled: IsEnabledFn,
    }

    /// The depth test for "behind what is drawn": `GL_GREATER`, or
    /// `GL_LESS` when the depth range is reversed (`GL_GEQUAL` normal).
    fn behind(depth_func: u32) -> u32 {
        if depth_func == GL_GEQUAL || depth_func == GL_GREATER {
            GL_LESS
        } else {
            GL_GREATER
        }
    }

    impl Gl {
        pub(super) unsafe fn shell_pass(&self, everywhere: bool, draw: impl FnOnce()) {
            unsafe {
                let mut func = 0i32;
                (self.get_integerv)(GL_DEPTH_FUNC, &mut func);
                let mut mask = 1u8;
                (self.get_booleanv)(GL_DEPTH_WRITEMASK, &mut mask);
                let textured = (self.is_enabled)(GL_TEXTURE_2D) != 0;
                (self.depth_func)(if everywhere {
                    GL_ALWAYS
                } else {
                    behind(func as u32)
                });
                (self.depth_mask)(0);
                if textured {
                    (self.disable)(GL_TEXTURE_2D);
                }
                draw();
                if textured {
                    (self.enable)(GL_TEXTURE_2D);
                }
                (self.depth_mask)(mask);
                (self.depth_func)(func as u32);
            }
        }
    }

    static GL: OnceLock<Option<Gl>> = OnceLock::new();

    fn load() -> Option<Gl> {
        let module = unsafe { GetModuleHandleA(c"opengl32.dll".as_ptr() as *const u8) };
        if module.is_null() {
            return None;
        }
        let proc = |name: &std::ffi::CStr| -> Option<usize> {
            unsafe { GetProcAddress(module, name.as_ptr() as *const u8) }.map(|p| p as usize)
        };
        // Safety: the exported functions' own signatures (stdcall).
        unsafe {
            use std::mem::transmute as t;
            Some(Gl {
                get_integerv: t::<usize, GetIntegervFn>(proc(c"glGetIntegerv")?),
                get_booleanv: t::<usize, GetBooleanvFn>(proc(c"glGetBooleanv")?),
                depth_func: t::<usize, EnumFn>(proc(c"glDepthFunc")?),
                depth_mask: t::<usize, MaskFn>(proc(c"glDepthMask")?),
                enable: t::<usize, EnumFn>(proc(c"glEnable")?),
                disable: t::<usize, EnumFn>(proc(c"glDisable")?),
                is_enabled: t::<usize, IsEnabledFn>(proc(c"glIsEnabled")?),
            })
        }
    }

    pub(super) fn get() -> Option<&'static Gl> {
        GL.get_or_init(load).as_ref()
    }
}

#[cfg(not(target_arch = "x86"))]
mod imp {
    pub(super) struct Gl;

    impl Gl {
        pub(super) unsafe fn shell_pass(&self, _everywhere: bool, _draw: impl FnOnce()) {}
    }

    pub(super) fn get() -> Option<&'static Gl> {
        None
    }
}
