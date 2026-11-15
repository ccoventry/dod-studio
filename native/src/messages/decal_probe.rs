// messages/decal_probe.rs
// Errors of the decal probe's options and demo (`patch/decal_probe.rs`, #237).
// Each function returns the text only.

use std::fmt::Display;

pub fn could_not_parse_demo(err: impl Display) -> String {
    format!("Could not parse demo file: {err}")
}

pub const NO_OFFSETS: &str = "no offsets to probe";

pub const OFFSETS_NOT_ASCENDING: &str = "offsets must be strictly ascending — a row's hole count is only readable as a threshold if they are";

pub const OFFSETS_ARE_MAGNITUDES: &str =
    "offsets are magnitudes; the OUT and IN rows apply the sign";

pub const NO_GRIDS: &str = "no grids to stamp";

pub const ANCHOR_NEEDS_AXIS: &str =
    "--anchor needs --axis: without a normal there is no direction to offset along";

/// The surface search found nothing. The floats print with the precision the
/// message always used (`{:.1}` for the tolerance, `{:.0}` for the others).
pub fn no_usable_surface(
    harvested: usize,
    min_plane_decals: impl Display,
    plane_tolerance: impl Display,
    min_plane_spread: impl Display,
    columns: usize,
    require_approach: impl Display,
) -> String {
    format!(
        "no usable surface: {harvested} harvested decals, none forming a connected patch of \
         {min_plane_decals}+ decals coplanar within {plane_tolerance:.1} units, spanning {min_plane_spread:.0}+ units, wide enough \
         to seat {columns} columns, and within {require_approach:.0} units of somewhere the camera actually \
         went. Try a demo with more decal traffic, lower --min-plane-decals, a \
         smaller --column-spacing, a larger --require-approach, or pass --anchor \
         x,y,z with --axis."
    )
}

pub const NO_TEXTURE_INDEX: &str = "no decal texture index available: the demo registered none and none was supplied. Pass --texture-index.";

pub fn too_few_network_frames(eligible: usize, needed: usize) -> String {
    format!("only {eligible} parsed network frames after the last level load, need {needed}")
}

pub fn too_few_probes_placed(placed: usize, wanted: usize) -> String {
    format!("only {placed} of {wanted} probes could be placed — too few eligible frames")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decal_probe_text_matches_its_original_inline_text() {
        assert_eq!(
            could_not_parse_demo("bad"),
            "Could not parse demo file: bad"
        );
        assert_eq!(NO_OFFSETS, "no offsets to probe");
        assert_eq!(
            OFFSETS_NOT_ASCENDING,
            "offsets must be strictly ascending — a row's hole count is only readable as a threshold if they are"
        );
        assert_eq!(
            OFFSETS_ARE_MAGNITUDES,
            "offsets are magnitudes; the OUT and IN rows apply the sign"
        );
        assert_eq!(NO_GRIDS, "no grids to stamp");
        assert_eq!(
            ANCHOR_NEEDS_AXIS,
            "--anchor needs --axis: without a normal there is no direction to offset along"
        );
        assert_eq!(
            no_usable_surface(7, 3, 2.0f32, 64.0f32, 5, 100.0f32),
            "no usable surface: 7 harvested decals, none forming a connected patch of 3+ decals coplanar within 2.0 units, spanning 64+ units, wide enough to seat 5 columns, and within 100 units of somewhere the camera actually went. Try a demo with more decal traffic, lower --min-plane-decals, a smaller --column-spacing, a larger --require-approach, or pass --anchor x,y,z with --axis."
        );
        assert_eq!(
            NO_TEXTURE_INDEX,
            "no decal texture index available: the demo registered none and none was supplied. Pass --texture-index."
        );
        assert_eq!(
            too_few_network_frames(2, 5),
            "only 2 parsed network frames after the last level load, need 5"
        );
        assert_eq!(
            too_few_probes_placed(2, 5),
            "only 2 of 5 probes could be placed — too few eligible frames"
        );
    }
}
