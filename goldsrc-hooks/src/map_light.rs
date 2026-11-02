//! `ds_map_apply_lighting`: brighten (or darken) parts of a map's baked
//! lighting during playback, from a per-map file of boxes (#1312).
//!
//! ## The file
//!
//! `dodstudio_maps/<map>_light.txt`, in `dod_addon` (with `-addons`) or
//! `dod`, the folder `ds_map_run_cfgs` reads its cfgs from (#1302); the first
//! one found is read. One box per line, `x1 y1 z1 x2 y2 z2 scale [fade]`,
//! `//` comments: [`boxes`] has the rules. Coordinates are the map's own, as
//! `ds_debug_position` or a BSP viewer gives them.
//!
//! ## What it does
//!
//! Once a map's world model has loaded, the per-frame poll walks its surfaces
//! ([`world`]) and multiplies the baked light of every luxel in a box
//! ([`luxels`]), in the engine's copy of the lighting (`msurface_t.samples`,
//! which `Mod_LoadLighting` keeps for the map's life). The `.bsp` on disk is
//! never touched, so every demo's map check still passes. The engine built
//! the map's lightmap textures from the old samples at load, so each changed
//! surface's `cached_light` is then set to a value no light style has:
//! `R_RenderDynamicLightmaps` sees the style "changed" the next time it draws
//! the surface and rebuilds and re-uploads its lightmap from the new samples,
//! the path a flickering light takes every frame. That needs `r_dynamic 1`
//! (the default), which a line in the log checks. The same scaled samples
//! light the player models standing there (`R_LightPoint` reads them too),
//! and a later rebuild of every lightmap (the 25th Anniversary engine does
//! one on a gamma change) keeps them.
//!
//! ## Once per load
//!
//! A demo of the same map can keep the world model the engine already has,
//! samples and all, so scaling again would double the change. After scaling,
//! a few of the changed bytes are remembered with their new values: while
//! they still hold them, the samples are already scaled. A fresh load puts
//! the file's bytes back, which differ from the scaled ones by construction,
//! and the next frame scales them again.
//!
//! Setting the cvar to 0 puts every changed luxel back (and rebuilds the
//! lightmaps); setting it to 1 again, on a later frame, reads the file again.
//! That is how to try a new scale without reloading the demo.

// Only the 32-bit build has a world model to read; a host build compiles the
// rules for the tests alone.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

mod boxes;
mod luxels;
mod world;

use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::names::console_name;
use crate::setting::Setting;

pub const NAME: &str = console_name!("map_apply_lighting");

/// `ds_map_apply_lighting`, or its default when the cvar could not be
/// registered. On by default: a map with no light file is left alone.
pub static SETTING: Setting = Setting::new(NAME, true);

/// What follows the map name in a light file's name.
const FILE_SUFFIX: &str = "_light.txt";
/// Changed bytes remembered to tell a kept model from a fresh load.
const MARKS: usize = 16;
/// Problems in a light file the log lists line by line; more are counted.
const PROBLEMS_LISTED: usize = 5;

/// One surface whose samples were changed, and what they held before.
struct Changed {
    surface: usize,
    styles: usize,
    samples: usize,
    original: Vec<u8>,
}

/// The current map's state, as far as its lighting goes.
enum State {
    /// Nothing done for the loaded map yet (or the cvar is off).
    Idle,
    /// Looked: the map has no light file, or nothing in it reached a luxel.
    Untouched {
        level: u32,
        model: usize,
    },
    Applied(Applied),
}

struct Applied {
    level: u32,
    model: usize,
    map: String,
    changed: Vec<Changed>,
    /// `(address, value)` of a few bytes the scaling changed.
    marks: Vec<(usize, u8)>,
    luxels: usize,
    boxes: usize,
}

/// Written only by [`poll`] on the engine thread; [`status_line`] reads it.
/// Neither ever waits for the other (`try_write` / `try_read`).
static STATE: RwLock<State> = RwLock::new(State::Idle);

/// The loaded level and its world model.
struct Level {
    /// FNV-1a of the level name, to see a new level without keeping it.
    hash: u32,
    name: String,
    model: usize,
}

/// Brings the lighting in line with the cvar and the loaded map. Every frame
/// from `commands::poll`, on the engine thread. A frame where nothing
/// changed costs the level name's hash, a pointer read and a compare of
/// [`MARKS`] bytes.
pub fn poll() {
    let Ok(mut state) = STATE.try_write() else {
        return;
    };
    let level = current_level();
    if !SETTING.on() {
        if let State::Applied(applied) = &*state
            && level.as_ref().is_some_and(|l| still_applied(applied, l))
        {
            restore(applied);
        }
        *state = State::Idle;
        return;
    }
    let Some(level) = level else {
        return;
    };
    let done = match &*state {
        State::Idle => false,
        State::Untouched { level: hash, model } => *hash == level.hash && *model == level.model,
        State::Applied(applied) => still_applied(applied, &level),
    };
    if !done {
        *state = apply(&level);
    }
}

/// Whether `applied` was made on this level's world model and its samples
/// still hold the scaled values.
fn still_applied(applied: &Applied, level: &Level) -> bool {
    applied.level == level.hash
        && applied.model == level.model
        && applied.marks.first().is_some_and(|&(at, _)| crate::crash::readable(at, 1))
        // Safety: readable, checked for the first; every mark lies in the
        // same lighting block, hunk memory that stays committed.
        && applied
            .marks
            .iter()
            .all(|&(at, value)| unsafe { std::ptr::read_volatile(at as *const u8) } == value)
}

/// The loaded level, once its own world model is in place and loaded: until
/// the new map's world replaces the old one, the old one's memory may
/// already be the new map's. `None` changes no state, so a map that is
/// still loading is looked at again once it has.
fn current_level() -> Option<Level> {
    let engfuncs = crate::engine::engfuncs()?;
    // Safety: a pointer into the engine's client state, valid for the
    // session; checked for null before reading.
    let raw = unsafe { (engfuncs.pfn_get_level_name)() };
    if raw.is_null() {
        return None;
    }
    let level = unsafe { CStr::from_ptr(raw) }.to_bytes();
    if level.is_empty() {
        return None;
    }
    // Safety: the engine's own accessor, bounds-checked by the engine.
    let entity = unsafe { (engfuncs.get_entity_by_index)(0) };
    if entity.is_null() {
        return None;
    }
    // Safety: a cl_entity_t the engine owns; `model` is the SDK layout.
    let model = unsafe { (*entity).model } as usize;
    if model == 0 || !world::model_loaded(model) {
        return None;
    }
    // Safety: readable (`model_loaded` checked past it); `name` is a
    // NUL-padded char[64].
    let name = unsafe { &(*(model as *const crate::engine::ModelSPartial)).name };
    let name_len = name.iter().position(|&b| b == 0).unwrap_or(name.len());
    if !name[..name_len].eq_ignore_ascii_case(level) {
        return None;
    }
    let hash = level.iter().fold(0x811c_9dc5_u32, |h, &b| {
        (h ^ b as u32).wrapping_mul(0x0100_0193)
    });
    Some(Level {
        hash,
        name: String::from_utf8_lossy(level).into_owned(),
        model,
    })
}

/// Reads the level's light file and scales its world model's samples.
/// Logs what it did; a map without a file logs nothing.
fn apply(level: &Level) -> State {
    let untouched = State::Untouched {
        level: level.hash,
        model: level.model,
    };
    let map = crate::map_text::short_name(&level.name).to_string();
    if !crate::map_cfgs::is_plain_map_name(&map) {
        return untouched;
    }
    let Some(path) = light_file(&map) else {
        return untouched;
    };
    let started = std::time::Instant::now();
    let file = match std::fs::read_to_string(&path) {
        Ok(text) => boxes::parse(&text),
        Err(e) => {
            report(&format!(
                "{NAME}: {map}: couldn't read {}: {e}",
                path.display()
            ));
            return untouched;
        }
    };
    report_problems(&map, &path, &file.problems);
    if file.boxes.is_empty() {
        report(&format!("{NAME}: {map}: no boxes in {}", path.display()));
        return untouched;
    }
    let Some(build) = world::loaded_build() else {
        report(&format!(
            "{NAME}: {map}: this hw.dll is a build the world model's layout wasn't checked against, so its lighting is unchanged"
        ));
        return untouched;
    };
    let Some(applied) = scale_world(level, build, &map, &file.boxes) else {
        report(&format!(
            "{NAME}: {map}: the world model's surfaces aren't readable, so its lighting is unchanged"
        ));
        return untouched;
    };
    report(&format!(
        "{NAME}: {map}: {} box(es) from {}: {} luxels on {} surfaces changed in {} ms ({} engine){}",
        applied.boxes,
        path.display(),
        applied.luxels,
        applied.changed.len(),
        started.elapsed().as_millis(),
        build.name,
        r_dynamic_note()
    ));
    if applied.changed.is_empty() {
        return untouched;
    }
    State::Applied(applied)
}

/// The first `dodstudio_maps/<map>_light.txt` that exists, in the folders
/// `ds_map_run_cfgs` reads.
fn light_file(map: &str) -> Option<PathBuf> {
    let name = format!("{map}{FILE_SUFFIX}");
    crate::map_cfgs::search_folders(&crate::texture_hires::game_dir())
        .into_iter()
        .map(|dir| dir.join(crate::map_cfgs::FOLDER).join(&name))
        .find(|path| path.is_file())
}

/// Multiplies the luxels in `light_boxes` across the world model, and marks
/// each changed surface for a lightmap rebuild. `None` when the model's
/// surface array isn't readable.
fn scale_world(
    level: &Level,
    build: &world::Build,
    map: &str,
    light_boxes: &[boxes::LightBox],
) -> Option<Applied> {
    let mut regions = world::Regions::default();
    let surfaces = world::surface_array(level.model, build, &mut regions)?;
    let mut applied = Applied {
        level: level.hash,
        model: level.model,
        map: map.to_string(),
        changed: Vec::new(),
        marks: Vec::new(),
        luxels: 0,
        boxes: light_boxes.len(),
    };
    for index in 0..surfaces.count {
        let Some(lit) = world::lit_surface(&surfaces, index, &mut regions) else {
            continue;
        };
        let Some(grid) = lit.surface.grid() else {
            continue;
        };
        let (min, max) = grid.bounds();
        if !light_boxes.iter().any(|b| b.reaches(min, max)) {
            continue;
        }
        // Safety: `lit_surface` checked the span is writable; only this
        // thread (the engine's, which also draws) touches samples.
        let samples =
            unsafe { std::slice::from_raw_parts_mut(lit.samples as *mut u8, lit.sample_len) };
        let original = samples.to_vec();
        let styles = lit.surface.style_count();
        let luxels = luxels::edit_luxels(&grid, styles, samples, |point, _style, rgb| {
            let gain = boxes::gain(light_boxes, point);
            let new = rgb.map(|c| boxes::scaled(c, gain));
            let changed = new != *rgb;
            *rgb = new;
            changed
        });
        if luxels == 0 {
            continue;
        }
        if applied.marks.len() < MARKS
            && let Some(at) = original
                .iter()
                .zip(samples.iter())
                .position(|(a, b)| a != b)
        {
            applied.marks.push((lit.samples + at, samples[at]));
        }
        // Safety: a surface `lit_surface` returned, this frame.
        unsafe { world::mark_stale(lit.at, styles) };
        applied.luxels += luxels;
        applied.changed.push(Changed {
            surface: lit.at,
            styles,
            samples: lit.samples,
            original,
        });
    }
    Some(applied)
}

/// Puts every changed surface's samples back and rebuilds its lightmap.
/// Last changed first: where two surfaces' samples overlap, the later copy
/// was taken after the earlier surface's scaling, so only this order ends
/// on the bytes the map loaded with.
fn restore(applied: &Applied) {
    for changed in applied.changed.iter().rev() {
        // Safety: `still_applied` just confirmed the model and samples are
        // the ones scaled, so these spans are the writable ones they were.
        unsafe {
            std::ptr::copy_nonoverlapping(
                changed.original.as_ptr(),
                changed.samples as *mut u8,
                changed.original.len(),
            );
            world::mark_stale(changed.surface, changed.styles);
        }
    }
    report(&format!(
        "{NAME}: {}: off, {} surfaces' light put back",
        applied.map,
        applied.changed.len()
    ));
}

fn report_problems(map: &str, path: &Path, problems: &[boxes::Problem]) {
    if problems.is_empty() {
        return;
    }
    let listed: Vec<String> = problems
        .iter()
        .take(PROBLEMS_LISTED)
        .map(|p| format!("line {}: {}", p.line, p.why))
        .collect();
    let more = problems.len().saturating_sub(PROBLEMS_LISTED);
    let tail = if more > 0 {
        format!("; {more} more")
    } else {
        String::new()
    };
    report(&format!(
        "{NAME}: {map}: skipped {} line(s) of {}: {}{tail}",
        problems.len(),
        path.display(),
        listed.join("; ")
    ));
}

/// `", but r_dynamic is 0 ..."` when the rebuild that shows the change is
/// off, else nothing.
fn r_dynamic_note() -> &'static str {
    let off = crate::engine::engfuncs().is_some_and(|e| {
        // Safety: the engine's own lookup; a registered cvar lives for the
        // session.
        let cvar = unsafe { (e.pfn_get_cvar_pointer)(c"r_dynamic".as_ptr()) };
        !cvar.is_null() && unsafe { (*cvar).value } == 0.0
    });
    if off {
        ", but r_dynamic is 0, so it shows only from the next map load"
    } else {
        ""
    }
}

/// The `ds_debug_status` line.
pub fn status_line() -> String {
    let what = match STATE.try_read().as_deref() {
        Ok(State::Applied(applied)) => format!(
            "{}: {} box(es), {} luxels on {} surfaces changed",
            applied.map,
            applied.boxes,
            applied.luxels,
            applied.changed.len()
        ),
        Ok(State::Untouched { .. }) => "this map has no light file, or it changed nothing".into(),
        Ok(State::Idle) => "nothing applied".into(),
        Err(_) => "busy".into(),
    };
    format!("{NAME} {}: {what}", u8::from(SETTING.on()))
}

fn report(message: &str) {
    // Safety: `debug::report` only appends to the hook log.
    unsafe { crate::debug::report(message) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_file_is_named_after_the_map() {
        assert_eq!(
            format!("dod_railroad2_test{FILE_SUFFIX}"),
            "dod_railroad2_test_light.txt"
        );
    }

    #[test]
    fn scaling_a_floor_in_a_box_brightens_only_the_luxels_inside() {
        let surface = luxels::tests::floor();
        let grid = surface.grid().expect("grid");
        let light = boxes::parse("1600 100 -300 1632 132 -280 2");
        let mut samples = vec![40u8; surface.sample_len().expect("len")];
        let changed = luxels::edit_luxels(&grid, 1, &mut samples, |p, _, rgb| {
            let new = rgb.map(|c| boxes::scaled(c, boxes::gain(&light.boxes, p)));
            let changed = new != *rgb;
            *rgb = new;
            changed
        });
        // Columns 0..=2 (x 1600, 1616, 1632) of 5, all 3 rows.
        assert_eq!(changed, 9);
        assert_eq!(samples.iter().filter(|&&b| b == 80).count(), 9 * 3);
        assert_eq!(samples.iter().filter(|&&b| b == 40).count(), 6 * 3);
    }

    #[test]
    fn the_name_is_the_start_of_no_other() {
        for other in [
            crate::map_cfgs::NAME,
            crate::map_sounds::VICTORY_MUSIC_NAME,
            crate::map_weather::NAME,
        ] {
            assert!(
                !other.starts_with(NAME) && !NAME.starts_with(other),
                "{other}"
            );
        }
    }
}
