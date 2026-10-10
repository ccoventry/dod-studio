//! A map's overview image, and where it sits in the world (#448).
//!
//! GoldSrc's overview for `dod_anzio` is two files in the game's
//! `dod/overviews/` folder: `dod_anzio.bmp`, and `dod_anzio.txt`, which says
//! how to lay it over the world:
//!
//! ```text
//! global { ZOOM 1.11  ORIGIN 307.06 372.72 -334.00  ROTATED 0 }
//! layer  { IMAGE "overviews/dod_anzio.bmp"  HEIGHT 25 }
//! ```
//!
//! This reads both. Turning a world position into a point on the image is the
//! frontend's job (`studio/src/kill_map.js`), which has the formula and where
//! it comes from.

use std::path::{Path, PathBuf};

/// The overview image's placement, as its `.txt` states it.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct OverviewPlacement {
    pub zoom: f32,
    pub origin: [f32; 3],
    /// The image is turned 90 degrees against the world.
    pub rotated: bool,
    /// The first layer's image, relative to the game folder.
    pub image: Option<String>,
}

impl Default for OverviewPlacement {
    /// The client's own defaults when a key is missing (`ParseOverviewFile`).
    fn default() -> Self {
        Self {
            zoom: 1.0,
            origin: [0.0; 3],
            rotated: false,
            image: None,
        }
    }
}

/// What the Demo Analyzer draws the map with.
#[derive(Clone, Debug, serde::Serialize)]
pub struct MapOverview {
    pub placement: OverviewPlacement,
    /// The image as a `data:` URL, ready for an `<img>`.
    pub image_data_url: String,
}

/// Larger than any overview has reason to be (1024x768 at 24 bits is 2.3 MB);
/// a file past this is not read.
const MAX_IMAGE_BYTES: u64 = 16 * 1024 * 1024;

/// Reads an overview `.txt`. Tolerant the way the client is: unknown keys and
/// `//` comments are skipped, and a missing key keeps its default. Only the
/// first layer's image is kept -- DoD's overviews have one.
pub fn parse_overview_txt(text: &str) -> OverviewPlacement {
    let mut out = OverviewPlacement::default();
    let tokens: Vec<&str> = text
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .flat_map(str::split_whitespace)
        .collect();
    let num = |i: usize| tokens.get(i).and_then(|t| t.parse::<f32>().ok());

    let mut i = 0;
    while i < tokens.len() {
        match tokens[i].to_ascii_lowercase().as_str() {
            "zoom" => {
                if let Some(z) = num(i + 1).filter(|z| *z > 0.0) {
                    out.zoom = z;
                }
            }
            "origin" => {
                for (axis, value) in out.origin.iter_mut().enumerate() {
                    if let Some(v) = num(i + 1 + axis) {
                        *value = v;
                    }
                }
            }
            "rotated" => out.rotated = num(i + 1).is_some_and(|r| r != 0.0),
            "image" if out.image.is_none() => {
                out.image = tokens.get(i + 1).map(|t| t.trim_matches('"').to_string());
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// Where to look for `overviews/`: the game's own `dod` folder first, then the
/// demo's folder and the two above it, for a demo kept inside a game folder
/// the settings do not point at (the other install, say).
pub fn overview_search_dirs(game_exe: Option<&Path>, demo_path: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dod) = game_exe.and_then(Path::parent).map(|p| p.join("dod")) {
        dirs.push(dod);
    }
    for dir in demo_path.ancestors().skip(1).take(3) {
        if !dirs.iter().any(|d| d == dir) {
            dirs.push(dir.to_path_buf());
        }
    }
    dirs
}

fn image_mime(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "bmp" => Some("image/bmp"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        // `.tga` is a valid overview for the game, but no browser can show
        // one; a handful of custom maps use it.
        _ => None,
    }
}

/// The first of `dirs` holding an overview for `map_name`, with its image
/// read. `None` when no folder has both files, or the image is a format the
/// app cannot show.
pub fn find_map_overview(dirs: &[PathBuf], map_name: &str) -> Option<MapOverview> {
    use base64::Engine as _;

    let map = map_name.trim().to_ascii_lowercase();
    if map.is_empty() || map.contains(['/', '\\', '.']) {
        return None;
    }
    for dir in dirs {
        let Ok(text) = std::fs::read(dir.join("overviews").join(format!("{map}.txt"))) else {
            continue;
        };
        let placement = parse_overview_txt(&String::from_utf8_lossy(&text));
        let named = placement
            .image
            .as_deref()
            .map(|rel| dir.join(rel.replace('\\', "/")));
        let fallback = dir.join("overviews").join(format!("{map}.bmp"));
        for image in named.into_iter().chain([fallback]) {
            let Some(mime) = image_mime(&image) else {
                continue;
            };
            let Ok(meta) = std::fs::metadata(&image) else {
                continue;
            };
            if !meta.is_file() || meta.len() > MAX_IMAGE_BYTES {
                continue;
            }
            let Ok(bytes) = std::fs::read(&image) else {
                continue;
            };
            let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
            return Some(MapOverview {
                placement,
                image_data_url: format!("data:{mime};base64,{b64}"),
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    #[test]
    fn parses_a_stock_overview() {
        // dod_anzio.txt as shipped, commented-out ORIGIN lines included.
        let text = "// overview description file for dod_anzio.bsp\n\nglobal \n{\n\tZOOM\t1.11\n//\tORIGIN\t338 \t-286\t-358\n\tORIGIN \t307.06\t372.72\t-334.00\n\tROTATED\t0\n}\n\nlayer \n{\n\tIMAGE\t\"overviews/dod_anzio.bmp\"\n\tHEIGHT\t25\n}\n";
        let p = parse_overview_txt(text);
        assert_eq!(p.zoom, 1.11);
        assert_eq!(p.origin, [307.06, 372.72, -334.0]);
        assert!(!p.rotated);
        assert_eq!(p.image.as_deref(), Some("overviews/dod_anzio.bmp"));
    }

    #[test]
    fn reads_rotated_and_keeps_defaults_for_missing_keys() {
        let p = parse_overview_txt("global { rotated 1 }");
        assert!(p.rotated);
        assert_eq!(p.zoom, 1.0);
        assert_eq!(p.origin, [0.0; 3]);
        assert_eq!(p.image, None);
    }

    #[test]
    fn ignores_a_zero_zoom() {
        // Every placement divides by zoom; 0 would put the image at infinity.
        assert_eq!(parse_overview_txt("ZOOM 0").zoom, 1.0);
    }

    #[test]
    fn searches_the_game_folder_then_the_demos_ancestors() {
        let dirs = overview_search_dirs(
            Some(Path::new("C:/HL/hl.exe")),
            Path::new("C:/HL/dod/demos/a.dem"),
        );
        assert_eq!(
            dirs,
            vec![
                PathBuf::from("C:/HL/dod"),
                PathBuf::from("C:/HL/dod/demos"),
                PathBuf::from("C:/HL"),
            ]
        );
    }

    #[test]
    fn finds_an_overview_and_encodes_its_image() {
        let scratch = Scratch::new("map_overview_find");
        let dod = scratch.path().join("dod");
        std::fs::create_dir_all(dod.join("overviews")).unwrap();
        std::fs::write(
            dod.join("overviews/dod_test.txt"),
            "global { ZOOM 2 ORIGIN 1 2 3 ROTATED 1 }\nlayer { IMAGE \"overviews/dod_test.bmp\" }",
        )
        .unwrap();
        std::fs::write(dod.join("overviews/dod_test.bmp"), b"BM").unwrap();

        let found = find_map_overview(std::slice::from_ref(&dod), "DOD_Test").expect("found");
        assert_eq!(found.placement.zoom, 2.0);
        assert!(found.placement.rotated);
        assert_eq!(found.image_data_url, "data:image/bmp;base64,Qk0=");

        assert!(find_map_overview(std::slice::from_ref(&dod), "dod_other").is_none());
        // A map name is a file stem, never a path.
        assert!(find_map_overview(&[dod], "../dod_test").is_none());
    }

    #[test]
    fn skips_an_image_no_browser_can_show() {
        let scratch = Scratch::new("map_overview_tga");
        let dod = scratch.path().join("dod");
        std::fs::create_dir_all(dod.join("overviews")).unwrap();
        std::fs::write(
            dod.join("overviews/cs_x.txt"),
            "layer { IMAGE \"overviews/cs_x.tga\" }",
        )
        .unwrap();
        std::fs::write(dod.join("overviews/cs_x.tga"), b"x").unwrap();
        assert!(find_map_overview(&[dod], "cs_x").is_none());
    }
}
