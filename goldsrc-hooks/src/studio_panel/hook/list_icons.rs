//! Icons in the Demos tab's list (#779): a leading image column whose cells
//! show an up arrow, a folder, a drive, a POV demo or an HLTV demo.
//!
//! vgui2's `ListPanel` paints an image column's cells from an `ImageList`
//! (`SetImageList`, slot 174), indexing it with the row's `int` under the
//! column's name (`KeyValues::GetInt`; 0 is no image). An `ImageList` is a
//! `CUtlVector<IImage *>` and a delete flag; its `GetImage(i)` is
//! `m_pMemory[i]` and `IsValidIndex(i)` is `0 <= i < m_Size` (GameUI.dll,
//! both builds), so one is laid out here by hand. The images are ours too:
//! objects with `IImage`'s six-slot vftable (Paint, SetPos, GetContentSize,
//! GetSize, SetSize, SetColor, the order of vgui2.dll's `Bitmap`), each
//! painting a 16x16 picture embedded below through the surface
//! (`VGUI_Surface026`): `CreateNewTextureID` (slot 31) and
//! `DrawSetTextureRGBA` (26) once, then `DrawSetColor` (8),
//! `DrawSetTexture` (27) and `DrawTexturedRect` (29) each paint, as vgui2's
//! `MemoryBitmap` does. Nothing is written to disk.
//!
//! `tools/verify_list_icons.py` checks the slots and layouts on both installs.

use std::cell::UnsafeCell;
use std::ffi::{CStr, c_void};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::slot;

/// The image column: the row's `int` under this key picks the icon.
pub(super) const ICON_KEY: &CStr = c"icon";
pub(super) const ICON_COLUMN_WIDE: i32 = 22;
/// `ListPanel::AddColumnHeader` flags.
pub(super) const COLUMN_FIXEDSIZE: i32 = 1;
pub(super) const COLUMN_IMAGE: i32 = 4;
/// `ListPanel::SetImageList(ImageList *, bool deleteImageListWhenDone)`.
pub(super) const LIST_SLOT_SET_IMAGE_LIST: usize = 174;
/// `KeyValues::SetInt(const char *, int)`.
pub(super) const KEYVALUES_SLOT_SET_INT: usize = 18;
/// `ISurface` slots: `DrawSetColor(int r, int g, int b, int a)`,
/// `DrawSetTextureRGBA(int id, const unsigned char *rgba, int wide, int
/// tall, int hardwareFilter, int forceReload)`, `DrawSetTexture(int id)`,
/// `DrawTexturedRect(int x0, int y0, int x1, int y1)`,
/// `CreateNewTextureID(bool procedural)`.
const SURFACE_DRAW_SET_COLOR: usize = 8;
const SURFACE_DRAW_SET_TEXTURE_RGBA: usize = 26;
const SURFACE_DRAW_SET_TEXTURE: usize = 27;
const SURFACE_DRAW_TEXTURED_RECT: usize = 29;
const SURFACE_CREATE_NEW_TEXTURE_ID: usize = 31;

/// A picture's side, in pixels.
pub(super) const SIZE: i32 = 16;
const PIXELS: usize = (SIZE * SIZE * 4) as usize;

/// Which icon a row shows: its value is the row's `int` under [`ICON_KEY`],
/// an index into the image list, where 0 (never set) is no image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Icon {
    Up = 1,
    Folder = 2,
    Drive = 3,
    Pov = 4,
    Hltv = 5,
}

/// The icon for a row: `is_up` for the up row, `is_drive` for a drive's
/// root, any other folder row a folder, and a demo by whether its header
/// said HLTV (`None` when it couldn't be read: a POV demo until known).
pub(super) fn icon_for(is_folder: bool, is_up: bool, is_drive: bool, hltv: Option<bool>) -> Icon {
    if is_up {
        Icon::Up
    } else if is_drive {
        Icon::Drive
    } else if is_folder {
        Icon::Folder
    } else if hltv == Some(true) {
        Icon::Hltv
    } else {
        Icon::Pov
    }
}

/// The pictures, one letter per pixel; the palette is in [`rgba_of`].
const PICTURES: [[&str; 16]; 5] = [
    // up
    [
        "................",
        ".......WW.......",
        "......WWWW......",
        ".....WWWWWW.....",
        "....WWWWWWWW....",
        "...WWW.WW.WWW...",
        "..WW...WW...WW..",
        ".......WW.......",
        ".......WW.......",
        ".......WW.......",
        ".......WW.......",
        ".......WW.......",
        ".......WW.......",
        ".......WW.......",
        "................",
        "................",
    ],
    // folder
    [
        "................",
        "................",
        "..yyyyy.........",
        ".yYYYYYy........",
        ".yYYYYYYyyyyyyy.",
        ".yYYYYYYYYYYYYy.",
        ".yyyyyyyyyyyyyy.",
        ".yYYYYYYYYYYYYy.",
        ".yYYYYYYYYYYYYy.",
        ".yYYYYYYYYYYYYy.",
        ".yYYYYYYYYYYYYy.",
        ".yYYYYYYYYYYYYy.",
        ".yYYYYYYYYYYYYy.",
        ".yyyyyyyyyyyyyy.",
        "................",
        "................",
    ],
    // drive
    [
        "................",
        "................",
        "................",
        "................",
        "................",
        ".gggggggggggggg.",
        ".gGGGGGGGGGGGGg.",
        ".gGGGGGGGGGGGGg.",
        ".gGGGGGGGGGGGGg.",
        ".gggggggggggggg.",
        ".gGGGGGGGGGGLGg.",
        ".gggggggggggggg.",
        "................",
        "................",
        "................",
        "................",
    ],
    // POV demo: a film strip
    [
        "................",
        ".KKKKKKKKKKKKKK.",
        ".KWKKKKKKKKKKWK.",
        ".KKKKKKKKKKKKKK.",
        ".KWKKBBBBBBKKWK.",
        ".KKKKBBBBBBKKKK.",
        ".KWKKBBBBBBKKWK.",
        ".KKKKBBBBBBKKKK.",
        ".KWKKBBBBBBKKWK.",
        ".KKKKBBBBBBKKKK.",
        ".KWKKBBBBBBKKWK.",
        ".KKKKKKKKKKKKKK.",
        ".KWKKKKKKKKKKWK.",
        ".KKKKKKKKKKKKKK.",
        "................",
        "................",
    ],
    // HLTV demo: a television
    [
        "................",
        "................",
        "....K......K....",
        ".....K....K.....",
        "..KKKKKKKKKKKK..",
        "..KGGGGGGGGGGK..",
        "..KGBBBBBBBBGK..",
        "..KGBBBBBBBBGK..",
        "..KGBBBBBBBBGK..",
        "..KGBBBBBBBBGK..",
        "..KGGGGGGGGGGK..",
        "..KKKKKKKKKKKK..",
        ".....KK..KK.....",
        "....KKKKKKKK....",
        "................",
        "................",
    ],
];

/// A picture letter's colour, RGBA.
fn rgba_of(letter: u8) -> [u8; 4] {
    match letter {
        b'W' => [235, 235, 235, 255],
        b'Y' => [232, 190, 78, 255],
        b'y' => [170, 120, 30, 255],
        b'G' => [176, 176, 176, 255],
        b'g' => [96, 96, 96, 255],
        b'L' => [90, 220, 90, 255],
        b'K' => [38, 38, 38, 255],
        b'B' => [90, 140, 210, 255],
        _ => [0, 0, 0, 0],
    }
}

/// A picture as the surface takes it: rows top to bottom, RGBA.
fn pixels(picture: &[&str; 16]) -> [u8; PIXELS] {
    let mut out = [0u8; PIXELS];
    for (row, line) in picture.iter().enumerate() {
        for (column, letter) in line.bytes().enumerate().take(SIZE as usize) {
            let at = (row * SIZE as usize + column) * 4;
            out[at..at + 4].copy_from_slice(&rgba_of(letter));
        }
    }
    out
}

/// One of our images: `IImage`'s vftable first, then what its methods keep.
#[repr(C)]
struct Image {
    vftable: *const ImageVftable,
    x: i32,
    y: i32,
    wide: i32,
    tall: i32,
    color: u32,
    /// The surface's texture id, 0 until the first paint uploads the picture.
    texture: i32,
    pixels: [u8; PIXELS],
}

/// `IImage`: Paint, SetPos, GetContentSize, GetSize, SetSize, SetColor.
#[repr(C)]
struct ImageVftable {
    paint: unsafe extern "thiscall" fn(*mut Image),
    set_pos: unsafe extern "thiscall" fn(*mut Image, i32, i32),
    get_content_size: unsafe extern "thiscall" fn(*mut Image, *mut i32, *mut i32),
    get_size: unsafe extern "thiscall" fn(*mut Image, *mut i32, *mut i32),
    set_size: unsafe extern "thiscall" fn(*mut Image, i32, i32),
    set_color: unsafe extern "thiscall" fn(*mut Image, u32),
}

/// vgui2's `ImageList`: a `CUtlVector<IImage *>` (memory, allocation
/// count, grow size, size, the elements pointer again) and whether it
/// deletes its images, which ours never does.
#[repr(C)]
struct ImageList {
    memory: *mut *mut Image,
    allocation_count: i32,
    grow_size: i32,
    size: i32,
    elements: *mut *mut Image,
    delete_images: bool,
}

/// How many entries the list holds: index 0 is no image.
const ENTRIES: usize = PICTURES.len() + 1;

struct Shared {
    images: UnsafeCell<[Image; PICTURES.len()]>,
    pointers: UnsafeCell<[*mut Image; ENTRIES]>,
    list: UnsafeCell<ImageList>,
}

// Safety: written once by `install` on the engine thread before the list
// panel is handed the list, read by the panel's paints on that thread.
unsafe impl Sync for Shared {}

static VFTABLE: ImageVftable = ImageVftable {
    paint,
    set_pos,
    get_content_size,
    get_size,
    set_size,
    set_color,
};

const BLANK: Image = Image {
    vftable: &VFTABLE,
    x: 0,
    y: 0,
    wide: 0,
    tall: 0,
    color: 0xffff_ffff,
    texture: 0,
    pixels: [0; PIXELS],
};

static SHARED: Shared = Shared {
    images: UnsafeCell::new([BLANK; PICTURES.len()]),
    pointers: UnsafeCell::new([std::ptr::null_mut(); ENTRIES]),
    list: UnsafeCell::new(ImageList {
        memory: std::ptr::null_mut(),
        allocation_count: ENTRIES as i32,
        grow_size: 0,
        size: ENTRIES as i32,
        elements: std::ptr::null_mut(),
        delete_images: false,
    }),
};
static BUILT: AtomicBool = AtomicBool::new(false);
/// The surface the images paint through, from `install`.
static SURFACE: AtomicUsize = AtomicUsize::new(0);

/// Hands `list` (a `ListPanel`) the image list, building it the first time.
/// The caller adds the image column ([`ICON_KEY`], [`COLUMN_IMAGE`]).
pub(super) unsafe fn install(surface: *mut c_void, list: *mut c_void) {
    SURFACE.store(surface as usize, Ordering::Release);
    if !BUILT.swap(true, Ordering::AcqRel) {
        // Safety: the first and only writer, before the panel reads any of it.
        unsafe {
            let images = &mut *SHARED.images.get();
            let pointers = &mut *SHARED.pointers.get();
            for (image, picture) in images.iter_mut().zip(PICTURES.iter()) {
                image.pixels = pixels(picture);
            }
            for (slot, image) in pointers[1..].iter_mut().zip(images.iter_mut()) {
                *slot = image;
            }
            let list = &mut *SHARED.list.get();
            list.memory = pointers.as_mut_ptr();
            list.elements = pointers.as_mut_ptr();
        }
    }
    type SetImageListFn = unsafe extern "thiscall" fn(*mut c_void, *mut c_void, u32);
    // Safety: slot 174 of a checked build; the list outlives the game.
    unsafe {
        let set_image_list: SetImageListFn = slot(list, LIST_SLOT_SET_IMAGE_LIST);
        set_image_list(list, SHARED.list.get() as *mut c_void, 0);
    }
}

unsafe extern "thiscall" fn paint(this: *mut Image) {
    let surface = SURFACE.load(Ordering::Acquire) as *mut c_void;
    if surface.is_null() || this.is_null() {
        return;
    }
    // Safety: our own image, painted on the engine thread.
    let image = unsafe { &mut *this };
    type NewTextureFn = unsafe extern "thiscall" fn(*mut c_void, u32) -> i32;
    type UploadFn = unsafe extern "thiscall" fn(*mut c_void, i32, *const u8, i32, i32, i32, i32);
    type ColorFn = unsafe extern "thiscall" fn(*mut c_void, i32, i32, i32, i32);
    type TextureFn = unsafe extern "thiscall" fn(*mut c_void, i32);
    type RectFn = unsafe extern "thiscall" fn(*mut c_void, i32, i32, i32, i32);
    unsafe {
        if image.texture == 0 {
            let new_texture: NewTextureFn = slot(surface, SURFACE_CREATE_NEW_TEXTURE_ID);
            image.texture = new_texture(surface, 0);
            let upload: UploadFn = slot(surface, SURFACE_DRAW_SET_TEXTURE_RGBA);
            // No hardware filter: the pixels stay crisp; force the reload.
            upload(
                surface,
                image.texture,
                image.pixels.as_ptr(),
                SIZE,
                SIZE,
                0,
                1,
            );
        }
        let [r, g, b, a] = image.color.to_le_bytes();
        let set_color: ColorFn = slot(surface, SURFACE_DRAW_SET_COLOR);
        set_color(surface, r.into(), g.into(), b.into(), a.into());
        let set_texture: TextureFn = slot(surface, SURFACE_DRAW_SET_TEXTURE);
        set_texture(surface, image.texture);
        let (wide, tall) = if image.wide > 0 && image.tall > 0 {
            (image.wide, image.tall)
        } else {
            (SIZE, SIZE)
        };
        let rect: RectFn = slot(surface, SURFACE_DRAW_TEXTURED_RECT);
        rect(surface, image.x, image.y, image.x + wide, image.y + tall);
    }
}

unsafe extern "thiscall" fn set_pos(this: *mut Image, x: i32, y: i32) {
    if let Some(image) = unsafe { this.as_mut() } {
        image.x = x;
        image.y = y;
    }
}

unsafe extern "thiscall" fn get_content_size(_this: *mut Image, wide: *mut i32, tall: *mut i32) {
    unsafe {
        if !wide.is_null() {
            *wide = SIZE;
        }
        if !tall.is_null() {
            *tall = SIZE;
        }
    }
}

unsafe extern "thiscall" fn get_size(this: *mut Image, wide: *mut i32, tall: *mut i32) {
    let (w, t) = match unsafe { this.as_ref() } {
        Some(image) if image.wide > 0 && image.tall > 0 => (image.wide, image.tall),
        _ => (SIZE, SIZE),
    };
    unsafe {
        if !wide.is_null() {
            *wide = w;
        }
        if !tall.is_null() {
            *tall = t;
        }
    }
}

unsafe extern "thiscall" fn set_size(this: *mut Image, wide: i32, tall: i32) {
    if let Some(image) = unsafe { this.as_mut() } {
        image.wide = wide;
        image.tall = tall;
    }
}

unsafe extern "thiscall" fn set_color(this: *mut Image, color: u32) {
    if let Some(image) = unsafe { this.as_mut() } {
        image.color = color;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_picture_is_sixteen_by_sixteen_in_the_palette() {
        for picture in &PICTURES {
            for line in picture {
                assert_eq!(line.len(), SIZE as usize, "{line}");
                for letter in line.bytes() {
                    assert!(
                        letter == b'.' || rgba_of(letter)[3] == 255,
                        "{}",
                        letter as char
                    );
                }
            }
        }
        let folder = pixels(&PICTURES[1]);
        // The folder's top-left corner is clear, its body is yellow.
        assert_eq!(&folder[0..4], &[0, 0, 0, 0]);
        let body = (8 * SIZE as usize + 8) * 4;
        assert_eq!(&folder[body..body + 4], &[232, 190, 78, 255]);
    }

    #[test]
    fn rows_get_their_icon() {
        assert_eq!(icon_for(true, true, false, None), Icon::Up);
        assert_eq!(icon_for(true, false, true, None), Icon::Drive);
        assert_eq!(icon_for(true, false, false, None), Icon::Folder);
        assert_eq!(icon_for(false, false, false, Some(true)), Icon::Hltv);
        assert_eq!(icon_for(false, false, false, Some(false)), Icon::Pov);
        assert_eq!(icon_for(false, false, false, None), Icon::Pov);
        assert_eq!(Icon::Up as i32, 1);
        assert_eq!(Icon::Hltv as i32, PICTURES.len() as i32);
    }

    #[test]
    fn the_image_list_lays_out_like_vgui2s() {
        assert_eq!(std::mem::size_of::<ImageList>(), 0x18);
        assert_eq!(std::mem::offset_of!(ImageList, size), 0xc);
        assert_eq!(std::mem::offset_of!(ImageList, delete_images), 0x14);
        assert_eq!(std::mem::offset_of!(Image, vftable), 0);
    }
}
