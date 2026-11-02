//! `ds_debug_texture`: names the world texture in the middle of the screen,
//! for reviewing a map's detail textures in the game (#1309).
//!
//! It prints:
//!
//! - **the texture**: its name, how far away it is, and whether it is on the
//!   world or on a brush entity (a `func_wall`, a door);
//! - **its HD key**, `<name>_<hash>`, the HD swap's own (`texture_hires`), and
//!   the active style's file for it if there is one;
//! - **its detail line**: the line for it in the map's `_detail.txt`, from
//!   the one file the engine reads, or that it has none;
//! - **the map**.
//!
//! ## Where the view is
//!
//! `GetViewInfo` (the studio API's `r_origin` and `vpn`) is the view the last
//! frame drew. In a POV demo that is the recorded view, which the engine
//! copies over `V_CalcRefdef`'s, and in an HLTV demo the spectated one: what
//! is on screen either way. Between frames, where console commands run, it
//! still holds that frame's.
//!
//! ## The trace
//!
//! The client's event API, the way DoD's own `client.dll` does it at
//! `+0x4e45c`..`+0x4e4a6` (the same file on both engine builds): a point-hull
//! `EV_PlayerTrace` finds which physent the line hits, then
//! `EV_TraceTexture(ent, start, end)` names the texture of the surface it
//! hits on that physent's model. Both are the engine's own functions, so
//! there is no `hw.dll` offset to keep per build. The physents are the ones
//! the engine set up for the frame, which demo playback does too: DoD's
//! weather code makes the same two calls with no prediction set-up first.
//!
//! ## Which detail file
//!
//! The engine opens `maps/<map>_detail.txt` through its search paths, which
//! put `dod_addon` (only with `-addons`) ahead of `dod`, and `dod_downloads`
//! after it, and reads the first one it finds, whole: a line missing from
//! that file is missing, whatever a later folder's copy says.
//!
//! The lines also go to the hook log, so a scripted test can read them back.

#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

use std::ffi::{CStr, c_char, c_void};
use std::path::Path;

use crate::commands::console_print;
use crate::engine;
use crate::names::console_name;
use crate::texture_hires::{self, WorldHd};

pub const NAME: &str = console_name!("debug_texture");

/// `event_api_s` slots, by byte offset, each pinned by DoD's `client.dll`
/// (byte-identical on every install): `version` sits at 0, then one pointer
/// per function in `event_api.h`'s order. `+0x4e472` pushes 2 and calls
/// `+0x38`; `+0x4e48e` calls `+0x3c` with start, end, `PM_WORLD_ONLY`, -1
/// and the trace; `+0x4e4a6` calls `+0x4c` with the trace's `ent` and the
/// line, and compares the result with "sky". `+0x47cf` calls `+0x24` with a
/// trace's `ent` and reads a physent back.
const EV_GET_PHYSENT: usize = 0x24;
const EV_SET_TRACE_HULL: usize = 0x38;
const EV_PLAYER_TRACE: usize = 0x3c;
const EV_TRACE_TEXTURE: usize = 0x4c;

type GetPhysentFn = unsafe extern "C" fn(idx: i32) -> *const u8;
type SetTraceHullFn = unsafe extern "C" fn(hull: i32);
type PlayerTraceFn =
    unsafe extern "C" fn(start: *mut f32, end: *mut f32, flags: i32, ignore: i32, tr: *mut u8);
type TraceTextureFn =
    unsafe extern "C" fn(ground: i32, start: *mut f32, end: *mut f32) -> *const c_char;

/// The point hull, as the client numbers hulls for `EV_SetTraceHull`.
const POINT_HULL: i32 = 2;
/// `PM_STUDIO_IGNORE`: players and other studio models don't stop the line.
const PM_STUDIO_IGNORE: i32 = 1;
/// Far enough to cross any map (GoldSrc's world is +-4096 on each axis).
const REACH: f32 = 16384.0;

/// `pmtrace_t`'s `fraction`, `endpos` and `ent` (`pm_defs.h`; `ent` at
/// `+0x30` is what `client.dll` reads at `+0x4e491`). The struct is 68 bytes;
/// the buffer is bigger, to spare a byte count that matters.
const TR_FRACTION: usize = 0x10;
const TR_ENDPOS: usize = 0x14;
const TR_ENT: usize = 0x30;
const TR_SIZE: usize = 128;
/// `physent_t`'s `name[32]` (the model's name, `*12` for a brush entity) and
/// `info` (its entity index).
const PE_NAME_LEN: usize = 32;
const PE_INFO: usize = 0x50;

/// What the line from the eye hit.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub texture: Option<String>,
    pub distance: f32,
    pub point: [f32; 3],
    /// `None` for the world, else the brush entity's model and entity index.
    pub entity: Option<(String, i32)>,
}

/// Where the map's detail line came from.
#[derive(Debug, Clone, PartialEq)]
pub enum Detail {
    /// The map has no `_detail.txt` in any folder the engine reads.
    NoFile,
    /// The file the engine reads, and the texture's line in it, if any.
    File { path: String, line: Option<String> },
}

/// The folders the engine reads `maps/<map>_detail.txt` from, first first:
/// `<game>_addon` only with `-addons`, then `<game>`, then `<game>_downloads`.
pub fn detail_folders(game: &str, addons: bool) -> Vec<String> {
    let mut folders = Vec::new();
    if addons {
        folders.push(format!("{game}_addon"));
    }
    folders.push(game.to_string());
    folders.push(format!("{game}_downloads"));
    folders
}

/// The line for `texture` in a `_detail.txt`'s text: the first one whose
/// first word is the texture's name, in any case (texture names are
/// case-blind in the engine), trimmed. Comment lines are skipped; a comment
/// after the numbers is kept, as the file has it.
pub fn detail_line<'a>(text: &'a str, texture: &str) -> Option<&'a str> {
    text.lines().map(str::trim).find(|line| {
        !line.starts_with("//")
            && line
                .split_whitespace()
                .next()
                .is_some_and(|word| word.eq_ignore_ascii_case(texture))
    })
}

/// The command's lines, first the summary one.
pub fn describe(
    map: Option<&str>,
    hit: Option<&Hit>,
    hd: Option<&WorldHd>,
    detail: Option<&Detail>,
) -> Vec<String> {
    let map_part = map.map_or("no map loaded".to_string(), |m| format!("map {m}"));
    let Some(hit) = hit else {
        return vec![format!(
            "{NAME}: {map_part}; nothing in view (no frame drawn yet?)"
        )];
    };
    let on = match &hit.entity {
        None => "the world".to_string(),
        Some((model, index)) => format!("brush entity {index} ({model})"),
    };
    let Some(texture) = &hit.texture else {
        return vec![format!(
            "{NAME}: {map_part}; the view hits {on} {:.0} units away at {:.0} {:.0} {:.0}, \
             but the engine names no texture there",
            hit.distance, hit.point[0], hit.point[1], hit.point[2]
        )];
    };
    let mut lines = vec![format!(
        "{NAME}: {map_part}; texture {texture} on {on}, {:.0} units away at {:.0} {:.0} {:.0}",
        hit.distance, hit.point[0], hit.point[1], hit.point[2]
    )];
    lines.push(match hd {
        None | Some(WorldHd::NoHook) => {
            "  HD key: unknown, the HD texture hook isn't installed (no dodstudio_hd folder at start-up)"
                .to_string()
        }
        Some(WorldHd::NotUploaded) => {
            format!("  HD key: unknown, no world texture named {texture} was loaded since the game started")
        }
        Some(WorldHd::Known { key, file: Some(file) }) => {
            format!("  HD key: {key} (HD file {})", file.display())
        }
        Some(WorldHd::Known { key, file: None }) => {
            format!("  HD key: {key} (no HD file for it in the active style)")
        }
    });
    lines.push(match detail {
        None | Some(Detail::NoFile) => {
            "  detail: none, the map has no _detail.txt the engine reads".to_string()
        }
        Some(Detail::File {
            path,
            line: Some(line),
        }) => format!("  detail: {line}   (from {path})"),
        Some(Detail::File { path, line: None }) => {
            format!("  detail: none for {texture} in {path}")
        }
    });
    lines
}

/// The map's detail file as the engine picks it, and `texture`'s line in it.
fn detail_for(map: &str, texture: &str) -> Detail {
    let root = crate::game_folder::game_root();
    let folders = detail_folders(
        crate::game_folder::name(),
        crate::texture_hires::addons_on(),
    );
    let file = format!("{map}_detail.txt");
    let found = folders.iter().find_map(|folder| {
        let path = root.join(folder).join("maps").join(&file);
        std::fs::read(&path).ok().map(|bytes| (folder, bytes))
    });
    match found {
        None => Detail::NoFile,
        Some((folder, bytes)) => {
            let text = String::from_utf8_lossy(&bytes);
            Detail::File {
                path: shown_path(folder, &file),
                line: detail_line(&text, texture).map(str::to_string),
            }
        }
    }
}

/// `dod_addon\maps\dod_lennon2_detail.txt`: the path from the game's folder,
/// which says which folder without the user's own path.
fn shown_path(folder: &str, file: &str) -> String {
    Path::new(folder)
        .join("maps")
        .join(file)
        .display()
        .to_string()
}

/// The last drawn frame's eye and forward vector.
fn view() -> Option<([f32; 3], [f32; 3])> {
    let studio = engine::engine_studio()?;
    let (mut origin, mut up, mut right, mut forward) = ([0f32; 3], [0f32; 3], [0f32; 3], [0f32; 3]);
    // Safety: the engine fills four vec3s from its render globals.
    unsafe {
        (studio.get_view_info)(
            origin.as_mut_ptr(),
            up.as_mut_ptr(),
            right.as_mut_ptr(),
            forward.as_mut_ptr(),
        )
    };
    let length = forward.iter().map(|c| c * c).sum::<f32>().sqrt();
    (length > 0.5).then_some((origin, forward))
}

/// One `event_api_s` slot as a function, or `None` while it is null.
///
/// # Safety
///
/// `api` is the engine's live `event_api_s`, and `F` the slot's signature.
unsafe fn slot<F: Copy>(api: *const u8, offset: usize) -> Option<F> {
    let raw = unsafe { (api.add(offset) as *const *const c_void).read() };
    // Safety: a non-null pointer in this slot is the function `F` describes.
    (!raw.is_null()).then(|| unsafe { std::mem::transmute_copy::<*const c_void, F>(&raw) })
}

fn read_f32(buf: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(buf[at..at + 4].try_into().unwrap_or_default())
}

/// The line from the eye along the view, and what it hits.
fn trace() -> Option<Hit> {
    let (eye, forward) = view()?;
    let api = engine::engfuncs()?.p_event_api as *const u8;
    if api.is_null() {
        return None;
    }
    // Safety: the engine's live event API, slots pinned above.
    let (set_hull, player_trace, trace_texture, get_physent) = unsafe {
        (
            slot::<SetTraceHullFn>(api, EV_SET_TRACE_HULL)?,
            slot::<PlayerTraceFn>(api, EV_PLAYER_TRACE)?,
            slot::<TraceTextureFn>(api, EV_TRACE_TEXTURE)?,
            slot::<GetPhysentFn>(api, EV_GET_PHYSENT),
        )
    };
    let mut start = eye;
    let mut end = [0f32; 3];
    for i in 0..3 {
        end[i] = eye[i] + forward[i] * REACH;
    }
    let mut tr = [0u8; TR_SIZE];
    // Safety: the client's own call sequence (see the module doc); `tr` is
    // bigger than a `pmtrace_t`.
    unsafe {
        set_hull(POINT_HULL);
        player_trace(
            start.as_mut_ptr(),
            end.as_mut_ptr(),
            PM_STUDIO_IGNORE,
            -1,
            tr.as_mut_ptr(),
        );
    }
    let fraction = read_f32(&tr, TR_FRACTION);
    if !(0.0..1.0).contains(&fraction) {
        return None;
    }
    let point = [
        read_f32(&tr, TR_ENDPOS),
        read_f32(&tr, TR_ENDPOS + 4),
        read_f32(&tr, TR_ENDPOS + 8),
    ];
    let ent = i32::from_le_bytes(tr[TR_ENT..TR_ENT + 4].try_into().unwrap_or_default());
    let ent = ent.max(0);
    // Safety: the engine returns a texture_t name or null.
    let texture = unsafe {
        let raw = trace_texture(ent, start.as_mut_ptr(), end.as_mut_ptr());
        (!raw.is_null()).then(|| CStr::from_ptr(raw).to_string_lossy().into_owned())
    };
    let entity =
        (ent != 0).then(|| physent(get_physent, ent).unwrap_or_else(|| ("?".to_string(), -1)));
    Some(Hit {
        texture: texture.filter(|t| !t.is_empty()),
        distance: fraction * REACH,
        point,
        entity,
    })
}

/// A physent's model name and entity index.
fn physent(get_physent: Option<GetPhysentFn>, ent: i32) -> Option<(String, i32)> {
    // Safety: the engine's physent array entry, or null.
    unsafe {
        let pe = get_physent?(ent);
        if pe.is_null() {
            return None;
        }
        let name = std::slice::from_raw_parts(pe, PE_NAME_LEN);
        let end = name.iter().position(|&b| b == 0).unwrap_or(PE_NAME_LEN);
        let info = (pe.add(PE_INFO) as *const i32).read_unaligned();
        Some((String::from_utf8_lossy(&name[..end]).into_owned(), info))
    }
}

fn map() -> Option<String> {
    let engfuncs = engine::engfuncs()?;
    // Safety: a pointer into the engine's client state; null-checked.
    let raw = unsafe { (engfuncs.pfn_get_level_name)() };
    if raw.is_null() {
        return None;
    }
    let name = unsafe { CStr::from_ptr(raw) }.to_string_lossy();
    let name = name.trim_start_matches("maps/").trim_end_matches(".bsp");
    (!name.is_empty()).then(|| name.to_string())
}

/// `ds_debug_texture`.
pub unsafe extern "C" fn command() {
    let map = map();
    let hit = map.as_ref().and_then(|_| trace());
    let texture = hit.as_ref().and_then(|h| h.texture.clone());
    let hd = texture.as_deref().map(texture_hires::world_hd);
    let detail = match (&map, &texture) {
        (Some(map), Some(texture)) => Some(detail_for(map, texture)),
        _ => None,
    };
    for line in describe(map.as_deref(), hit.as_ref(), hd.as_ref(), detail.as_ref()) {
        console_print(&format!("{line}\n"));
        unsafe { crate::debug::report(&line) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const LENNON2: &str = "//\r\n// Detail texture map file for dod_lennon_test - by steve hisey\r\n//\r\n\
        in4\t\t\tdetail/fabric2\t \t\t8.0\t8.0\t// blue wallpaper\r\n\
        IN4_old detail/x 1 1\r\n\
        // xconcrete_c54_y detail/commented 1 1\r\n\
        XCONCRETE_C54_Y\t\tdetail/rock\t\t\t10      10\r\n";

    #[test]
    fn the_engine_reads_the_addon_folder_first_and_only_with_addons() {
        assert_eq!(
            detail_folders("dod", true),
            ["dod_addon", "dod", "dod_downloads"]
        );
        assert_eq!(detail_folders("dod", false), ["dod", "dod_downloads"]);
        assert_eq!(detail_folders("dod_movies", true)[0], "dod_movies_addon");
    }

    #[test]
    fn a_texture_s_line_is_found_by_its_first_word_in_any_case() {
        assert_eq!(
            detail_line(LENNON2, "in4"),
            Some("in4\t\t\tdetail/fabric2\t \t\t8.0\t8.0\t// blue wallpaper")
        );
        assert_eq!(
            detail_line(LENNON2, "xconcrete_c54_y"),
            Some("XCONCRETE_C54_Y\t\tdetail/rock\t\t\t10      10")
        );
        assert_eq!(detail_line(LENNON2, "avainwall1"), None);
        // A name that only starts another line's first word is not it.
        assert_eq!(detail_line(LENNON2, "in"), None);
    }

    #[test]
    fn commented_out_lines_are_not_the_texture_s() {
        let text = "// avainwall1 detail/dt_fabric1 12 12\n";
        assert_eq!(detail_line(text, "avainwall1"), None);
    }

    fn hit(entity: Option<(String, i32)>) -> Hit {
        Hit {
            texture: Some("xconcrete_c54_y".to_string()),
            distance: 312.4,
            point: [120.0, -40.2, 16.0],
            entity,
        }
    }

    #[test]
    fn says_the_texture_its_key_and_its_detail_line() {
        let hd = WorldHd::Known {
            key: "xconcrete_c54_y_1a2b3c4d".to_string(),
            file: Some(PathBuf::from(
                "dodstudio_hd/world/ultrasharp/xconcrete_c54_y_1a2b3c4d.tga",
            )),
        };
        let detail = Detail::File {
            path: "dod_addon/maps/dod_lennon2_detail.txt".to_string(),
            line: Some("xconcrete_c54_y detail/dt_conc 10 10".to_string()),
        };
        let lines = describe(
            Some("dod_lennon2"),
            Some(&hit(None)),
            Some(&hd),
            Some(&detail),
        );
        assert_eq!(
            lines[0],
            "ds_debug_texture: map dod_lennon2; texture xconcrete_c54_y on the world, 312 units away at 120 -40 16"
        );
        assert!(lines[1].starts_with("  HD key: xconcrete_c54_y_1a2b3c4d (HD file dodstudio_hd"));
        assert_eq!(
            lines[2],
            "  detail: xconcrete_c54_y detail/dt_conc 10 10   (from dod_addon/maps/dod_lennon2_detail.txt)"
        );
    }

    #[test]
    fn says_when_there_is_no_line_no_file_or_no_key() {
        let missing = Detail::File {
            path: "dod/maps/dod_lennon2_detail.txt".to_string(),
            line: None,
        };
        let lines = describe(
            Some("dod_lennon2"),
            Some(&hit(Some(("*12".to_string(), 40)))),
            Some(&WorldHd::NotUploaded),
            Some(&missing),
        );
        assert!(lines[0].contains("on brush entity 40 (*12)"));
        assert!(lines[1].contains("unknown, no world texture named xconcrete_c54_y"));
        assert_eq!(
            lines[2],
            "  detail: none for xconcrete_c54_y in dod/maps/dod_lennon2_detail.txt"
        );
        let lines = describe(
            Some("dod_anzio"),
            Some(&hit(None)),
            None,
            Some(&Detail::NoFile),
        );
        assert!(lines[1].contains("hook isn't installed"));
        assert!(lines[2].contains("no _detail.txt"));
    }

    #[test]
    fn says_when_nothing_is_in_view() {
        assert_eq!(
            describe(None, None, None, None),
            ["ds_debug_texture: no map loaded; nothing in view (no frame drawn yet?)"]
        );
        let mut sky = hit(None);
        sky.texture = None;
        let lines = describe(Some("dod_anzio"), Some(&sky), None, None);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].ends_with("but the engine names no texture there"));
    }
}
