//! Map overviews made from the map itself (#371): the image the game draws
//! for its overview map (`dod/overviews/<map>.bmp` or `.tga`) and the `.txt`
//! that places it.
//!
//! - [`level`] reads the BSP;
//! - [`reach`] finds every floor a player can get to from the spawns;
//! - [`scene`] turns that into polygons in image pixels, grouped into areas
//!   with default colours, plus water, capture zones, flags and spawns;
//! - [`slopes`] finds the slopes too steep to stand on that a player still
//!   gets onto, which the page outlines;
//! - [`transform`] is the game's own world-to-image mapping, and writes the
//!   `.txt`;
//! - [`image`] encodes what the Overviews page drew as the `.tga` or `.bmp`
//!   the game's loader takes;
//! - [`spawn_zones`] finds each spawn's protection, which the page outlines;
//! - [`credit`] finds who made the map, for the title card;
//! - [`flag_icons`] reads the icons the game draws on its map for each flag,
//!   for the page to preview;
//! - [`files`] finds installs and maps and writes the result, keeping a
//!   backup of anyone's own overview.
//!
//! The page does the drawing and the editing; this module never rasterises.

pub mod credit;
pub mod files;
pub mod flag_icons;
pub mod image;
pub mod level;
pub mod reach;
pub mod scene;
pub mod slopes;
pub mod spawn_zones;
pub mod transform;

use std::path::Path;

/// The scene for one map of an install: `<install>/dod/maps/<map>.bsp`,
/// flags named from the install's own `dod/resource/dod_english.txt`.
pub fn scene_for(install: &Path, map: &str) -> Result<scene::Scene, String> {
    scene_for_until(install, map, &|_| false)
}

/// [`scene_for`], telling `progress` how far along it is and giving up
/// with [`reach::CANCELLED`] when it answers true.
pub fn scene_for_until(
    install: &Path,
    map: &str,
    progress: &dyn Fn(f32) -> bool,
) -> Result<scene::Scene, String> {
    let bsp = install.join("dod").join("maps").join(format!("{map}.bsp"));
    let level = level::Level::from_file(&bsp)?;
    let strings =
        scene::read_strings(&install.join("dod").join("resource").join("dod_english.txt"));
    let mut scene = scene::build_until(&level, &strings, progress)?;
    scene.checksum = crate::patch::bsp::map_checksum_of_file(&bsp).unwrap_or(0);
    scene.credit = credit::map_credit(install, map, &level.entities);
    Ok(scene)
}
