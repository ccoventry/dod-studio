//! Map overviews made from the map itself (#371): the image the game draws
//! for its overview map (`dod/overviews/<map>.bmp` or `.tga`) and the `.txt`
//! that places it.
//!
//! - [`level`] reads the BSP;
//! - [`reach`] finds every floor a player can get to from the spawns;
//! - [`scene`] turns that into polygons in image pixels, grouped into areas
//!   with default colours, plus water, capture zones, flags and spawns;
//! - [`transform`] is the game's own world-to-image mapping, and writes the
//!   `.txt`;
//! - [`image`] encodes what the Overviews page drew as the `.tga` or `.bmp`
//!   the game's loader takes;
//! - [`files`] finds installs and maps and writes the result, keeping a
//!   backup of anyone's own overview.
//!
//! The page does the drawing and the editing; this module never rasterises.

pub mod files;
pub mod image;
pub mod level;
pub mod reach;
pub mod scene;
pub mod transform;

use std::path::Path;

/// The scene for one map of an install: `<install>/dod/maps/<map>.bsp`,
/// flags named from the install's own `dod/resource/dod_english.txt`.
pub fn scene_for(install: &Path, map: &str) -> Result<scene::Scene, String> {
    let bsp = install.join("dod").join("maps").join(format!("{map}.bsp"));
    let level = level::Level::from_file(&bsp)?;
    let strings =
        scene::read_strings(&install.join("dod").join("resource").join("dod_english.txt"));
    scene::build(&level, &strings)
}
