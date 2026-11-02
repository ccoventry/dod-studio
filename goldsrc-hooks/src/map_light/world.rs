//! The loaded map's world model, as `map_light` reads and writes it: the
//! engine's own structs (`com_model.h`), mirrored here, and safe access to
//! the memory they point at.
//!
//! Every field read sits at the SDK's offset in both engine builds:
//! `R_BuildLightMap` (pre-Anniversary `hw.dll+0x47390`, 25th Anniversary
//! `+0x247b50`) reads `extents` at +0x18/+0x1a, `samples` at +0x54, `styles`
//! at +0x3c and `cached_light` at +0x40 of an `msurface_t`, and
//! `R_AddDynamicLights` the plane at +0x04 and `texturemins` at +0x14. The
//! struct's size is not the same: `GL_BuildLightmaps` walks
//! `model_t.surfaces` (+0xb4, `numsurfaces` +0xb0) 0x5c bytes at a time on
//! the pre-Anniversary build, the SDK's size, and the Anniversary engine's
//! rebuild of every lightmap (`+0x24b7f0`) 0x6c at a time. So the stride is
//! per build ([`BUILDS`]), told apart by the loaded `hw.dll`'s headers; any
//! other build is left alone. `tools/verify_map_light_offsets.py` checks
//! every offset and stride here against both `hw.dll`s.

#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::mem::{offset_of, size_of};

use super::luxels::{MAX_STYLES, Surface};

/// `model_t.type`; 0 is `mod_brush`.
pub(crate) const MODEL_TYPE_AT: usize = 0x44;
/// `model_t.numsurfaces`, then `model_t.surfaces`.
pub(crate) const MODEL_NUMSURFACES_AT: usize = 0xb0;
pub(crate) const MODEL_SURFACES_AT: usize = 0xb4;
/// How far into `model_t` this module reads.
const MODEL_READ_LEN: usize = MODEL_SURFACES_AT + 4;

/// `SURF_DRAWSKY | SURF_DRAWTURB`: sky and water, which have no lightmap
/// (`R_RenderDynamicLightmaps` skips them, both builds).
const SURF_NO_LIGHTMAP: i32 = 0x04 | 0x10;
/// `SURF_DRAWTILED`, which has no lightmap when its texture is
/// `TEX_SPECIAL` (`GL_CreateSurfaceLightmap`).
const SURF_DRAWTILED: i32 = 0x20;
const TEX_SPECIAL: i32 = 1;

/// A `cached_light` no light style ever has, so `R_RenderDynamicLightmaps`
/// sees the surface's light as changed and rebuilds its lightmap.
const STALE: i32 = i32::MIN;

/// `msurface_t`. Pointers are 32-bit: this DLL only runs in the 32-bit
/// engine.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct MSurface {
    visframe: i32,
    plane: u32,
    flags: i32,
    firstedge: i32,
    numedges: i32,
    texturemins: [i16; 2],
    extents: [i16; 2],
    light_s: i32,
    light_t: i32,
    polys: u32,
    texturechain: u32,
    texinfo: u32,
    dlightframe: i32,
    dlightbits: i32,
    lightmaptexturenum: i32,
    styles: [u8; MAX_STYLES],
    cached_light: [i32; MAX_STYLES],
    cached_dlight: i32,
    samples: u32,
    pdecals: u32,
}

// The SDK's size; the 25th Anniversary engine's struct is longer (`BUILDS`),
// with the same fields at the same offsets as far as this reads.
const _: () = assert!(size_of::<MSurface>() == 0x5c);

/// One `hw.dll` this was checked against.
pub(crate) struct Build {
    pub name: &'static str,
    /// The PE header's `TimeDateStamp` and `SizeOfImage`: which compile.
    time_date_stamp: u32,
    size_of_image: u32,
    /// `sizeof(msurface_t)`: the stride of `model_t.surfaces`.
    surface_size: usize,
}

pub(crate) const BUILDS: [Build; 2] = [
    Build {
        name: "pre-Anniversary",
        time_date_stamp: 0x5f3d_8dcc,
        size_of_image: 0x0125_9000,
        surface_size: 0x5c,
    },
    Build {
        name: "25th Anniversary",
        time_date_stamp: 0x6704_93da,
        size_of_image: 0x014a_a000,
        surface_size: 0x6c,
    },
];

/// The loaded `hw.dll`'s entry in [`BUILDS`], or `None` for a build this
/// wasn't checked against (or no `hw.dll` yet).
pub(crate) fn loaded_build() -> Option<&'static Build> {
    let base = crate::engine::engine_module_base()?;
    // Safety: a module handle the loader gave us.
    let identity = unsafe { crate::pe::image_identity(base as *mut u8) }?;
    BUILDS
        .iter()
        .find(|b| identity == (b.time_date_stamp, b.size_of_image))
}

/// `mtexinfo_t`.
#[repr(C)]
#[derive(Clone, Copy)]
struct MTexInfo {
    vecs: [[f32; 4]; 2],
    mipadjust: f32,
    texture: u32,
    flags: i32,
}

const _: () = assert!(size_of::<MTexInfo>() == 0x2c);

/// `mplane_t`.
#[repr(C)]
#[derive(Clone, Copy)]
struct MPlane {
    normal: [f32; 3],
    dist: f32,
    kind: u8,
    signbits: u8,
    pad: [u8; 2],
}

const _: () = assert!(size_of::<MPlane>() == 0x14);

/// One surface with a lightmap, as read: where it is, what its luxels need,
/// and where its samples are.
pub(crate) struct LitSurface {
    pub at: usize,
    pub surface: Surface,
    pub samples: usize,
    pub sample_len: usize,
}

/// Remembers the last memory region `VirtualQuery` described, so walking a
/// map's surfaces (all in one or two hunk regions) asks the OS a handful of
/// times, not once per pointer.
#[derive(Default)]
pub(crate) struct Regions {
    last: Option<Region>,
}

#[derive(Clone, Copy)]
struct Region {
    start: usize,
    end: usize,
    writable: bool,
}

impl Regions {
    /// Whether `len` bytes from `at` are committed and readable.
    pub(crate) fn readable(&mut self, at: usize, len: usize) -> bool {
        self.region_of(at, len).is_some()
    }

    /// Whether `len` bytes from `at` are committed and writable.
    pub(crate) fn writable(&mut self, at: usize, len: usize) -> bool {
        self.region_of(at, len).is_some_and(|r| r.writable)
    }

    fn region_of(&mut self, at: usize, len: usize) -> Option<Region> {
        let end = at.checked_add(len)?;
        if at == 0 {
            return None;
        }
        if let Some(r) = self.last
            && r.start <= at
            && end <= r.end
        {
            return Some(r);
        }
        let r = query(at)?;
        self.last = Some(r);
        (end <= r.end).then_some(r)
    }
}

#[cfg(windows)]
fn query(at: usize) -> Option<Region> {
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
        PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE,
        PAGE_WRITECOPY, VirtualQuery,
    };
    // Safety: VirtualQuery writes a struct we own and never faults.
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    let written = unsafe {
        VirtualQuery(
            at as *const std::ffi::c_void,
            &mut info,
            size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    let readable = PAGE_READONLY
        | PAGE_READWRITE
        | PAGE_WRITECOPY
        | PAGE_EXECUTE_READ
        | PAGE_EXECUTE_READWRITE
        | PAGE_EXECUTE_WRITECOPY;
    let writable =
        PAGE_READWRITE | PAGE_WRITECOPY | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY;
    if written == 0
        || info.State != MEM_COMMIT
        || info.Protect & readable == 0
        || info.Protect & (PAGE_GUARD | PAGE_NOACCESS) != 0
    {
        return None;
    }
    let start = info.BaseAddress as usize;
    Some(Region {
        start,
        end: start.saturating_add(info.RegionSize),
        writable: info.Protect & writable != 0,
    })
}

#[cfg(not(windows))]
fn query(_at: usize) -> Option<Region> {
    None
}

/// The world model's surface array, laid out as `build` has it, or `None`
/// when `model` isn't a readable brush model with surfaces.
pub(crate) fn surface_array(
    model: usize,
    build: &Build,
    regions: &mut Regions,
) -> Option<SurfaceArray> {
    if !regions.readable(model, MODEL_READ_LEN) {
        return None;
    }
    // Safety: readable, checked above.
    let (kind, count, surfaces) = unsafe {
        (
            std::ptr::read_unaligned((model + MODEL_TYPE_AT) as *const i32),
            std::ptr::read_unaligned((model + MODEL_NUMSURFACES_AT) as *const i32),
            std::ptr::read_unaligned((model + MODEL_SURFACES_AT) as *const u32) as usize,
        )
    };
    let count = usize::try_from(count).ok().filter(|&n| n > 0)?;
    let stride = build.surface_size;
    (kind == 0 && regions.readable(surfaces, count.checked_mul(stride)?)).then_some(SurfaceArray {
        count,
        at: surfaces,
        stride,
    })
}

/// A world model's `msurface_t` array.
pub(crate) struct SurfaceArray {
    pub count: usize,
    at: usize,
    stride: usize,
}

/// Surface `index` of `surfaces`, if it has a lightmap and everything it
/// points at is readable (its samples writable too).
pub(crate) fn lit_surface(
    surfaces: &SurfaceArray,
    index: usize,
    regions: &mut Regions,
) -> Option<LitSurface> {
    if index >= surfaces.count {
        return None;
    }
    let at = surfaces.at + index * surfaces.stride;
    // Safety: inside the array `surface_array` checked readable, and the
    // build's stride is at least the mirror's size.
    let raw: MSurface = unsafe { std::ptr::read_unaligned(at as *const MSurface) };
    if raw.flags & SURF_NO_LIGHTMAP != 0 || raw.samples == 0 || raw.styles[0] == 255 {
        return None;
    }
    let (texinfo, plane) = (raw.texinfo as usize, raw.plane as usize);
    if !regions.readable(texinfo, size_of::<MTexInfo>())
        || !regions.readable(plane, size_of::<MPlane>())
    {
        return None;
    }
    // Safety: both readable, checked just above.
    let tex: MTexInfo = unsafe { std::ptr::read_unaligned(texinfo as *const MTexInfo) };
    let plane: MPlane = unsafe { std::ptr::read_unaligned(plane as *const MPlane) };
    if raw.flags & SURF_DRAWTILED != 0 && tex.flags & TEX_SPECIAL != 0 {
        return None;
    }
    let surface = Surface {
        texturemins: raw.texturemins,
        extents: raw.extents,
        styles: raw.styles,
        vecs: tex.vecs,
        normal: plane.normal,
        dist: plane.dist,
    };
    let sample_len = surface.sample_len()?;
    let samples = raw.samples as usize;
    regions.writable(samples, sample_len).then_some(LitSurface {
        at,
        surface,
        samples,
        sample_len,
    })
}

/// Marks the surface at `at` for a lightmap rebuild the next time it is
/// drawn (`R_RenderDynamicLightmaps`, with `r_dynamic` on). Only the style
/// slots in use are compared by the engine, so only those are written.
///
/// # Safety
/// `at` is a surface [`lit_surface`] returned, on the engine thread, while
/// its map is still loaded.
pub(crate) unsafe fn mark_stale(at: usize, styles: usize) {
    let cached = at + offset_of!(MSurface, cached_light);
    for slot in 0..styles.min(MAX_STYLES) {
        // Safety: inside the surface, which is hunk memory (writable: its
        // samples were, and the engine writes this field every rebuild).
        unsafe { std::ptr::write_unaligned((cached + slot * 4) as *mut i32, STALE) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The offsets `world.rs`'s doc and `verify_map_light_offsets.py` name.
    #[test]
    fn the_surface_mirror_is_the_sdk_layout() {
        assert_eq!(offset_of!(MSurface, plane), 0x04);
        assert_eq!(offset_of!(MSurface, flags), 0x08);
        assert_eq!(offset_of!(MSurface, texturemins), 0x14);
        assert_eq!(offset_of!(MSurface, extents), 0x18);
        assert_eq!(offset_of!(MSurface, texinfo), 0x2c);
        assert_eq!(offset_of!(MSurface, styles), 0x3c);
        assert_eq!(offset_of!(MSurface, cached_light), 0x40);
        assert_eq!(offset_of!(MSurface, samples), 0x54);
        assert_eq!(offset_of!(MTexInfo, flags), 0x28);
        assert_eq!(offset_of!(MPlane, dist), 0x0c);
    }

    #[test]
    fn every_builds_surface_holds_the_fields_read() {
        for build in &BUILDS {
            assert!(
                build.surface_size >= size_of::<MSurface>(),
                "{}",
                build.name
            );
        }
    }

    #[test]
    fn a_null_or_wrapping_span_is_never_readable() {
        let mut regions = Regions::default();
        assert!(!regions.readable(0, 4));
        assert!(!regions.readable(usize::MAX - 1, 4));
    }

    #[cfg(windows)]
    #[test]
    fn our_own_memory_is_readable_and_writable() {
        let buffer = vec![0u8; 4096];
        let mut regions = Regions::default();
        assert!(regions.readable(buffer.as_ptr() as usize, buffer.len()));
        assert!(regions.writable(buffer.as_ptr() as usize, buffer.len()));
    }
}
