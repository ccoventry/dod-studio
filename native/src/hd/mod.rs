//! The HD texture files, as the app sees them (#372).
//!
//! `goldsrc-hooks`' `texture_hires` swaps in upscaled textures from
//! `<game>\dod\dodstudio_hd\<type>\<style>\`, and `goldsrc-hooks/tools/hd/`'s
//! scripts build them. This module is the app's side of that: what is built
//! ([`scan`]), and fetching the upscaler the build needs ([`setup`]). Building
//! itself is still the scripts' job; the Rust port is #372's second step.
//! The user's own styles are [`my_styles`], and the misses the game logged
//! [`misses`].
//!
//! The layout and names here mirror the hook's and the scripts', and must stay
//! in step with both:
//!
//! - `texture_hires.rs`: the five type folders, `overrides`, and the default
//!   style;
//! - `tools/hd/styles.py`: the built-in styles and their model files.

pub mod build;
pub mod map_list;
pub mod misses;
pub mod my_styles;
pub mod preview;
pub mod python;
pub mod setup;
pub mod upscaler;

use std::path::{Path, PathBuf};

use serde::Serialize;

/// The asset types, one folder each under `dodstudio_hd`, in the order the
/// hook's own docs list them.
pub const ASSET_TYPES: [&str; 5] = ["world", "models", "sprites", "detail", "sky"];

/// Per-file picks that win over any style (`texture_hires.rs`'s `OVERRIDES`).
pub const OVERRIDES: &str = "overrides";

/// What `dodstudio_hd_style` is when nothing sets it.
pub const DEFAULT_STYLE: &str = "ultrasharp";

/// The console lines that turn HD on and pick a style.
pub const ENABLED_CVAR: &str = "dodstudio_hd_enabled";
pub const STYLE_CVAR: &str = "dodstudio_hd_style";

/// A style the scripts know without `my_styles.txt`.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct BuiltInStyle {
    pub name: &'static str,
    /// The Real-ESRGAN model file stem, for the AI styles.
    pub model: Option<&'static str>,
}

/// `tools/hd/styles.py`'s `BUILT_IN`, in the same order.
pub const BUILT_IN_STYLES: [BuiltInStyle; 7] = [
    BuiltInStyle {
        name: "ultrasharp",
        model: Some("ultrasharp-4x"),
    },
    BuiltInStyle {
        name: "remacri",
        model: Some("remacri-4x"),
    },
    BuiltInStyle {
        name: "siax",
        model: Some("4x_NMKD-Siax_200k"),
    },
    BuiltInStyle {
        name: "generalv3",
        model: Some("RealESRGAN_General_x4_v3"),
    },
    BuiltInStyle {
        name: "x4plus",
        model: Some("realesrgan-x4plus"),
    },
    // A plain enlargement with sharpening: no AI, no model.
    BuiltInStyle {
        name: "plain",
        model: None,
    },
    // Made from x4plus and plain, never upscaled on its own.
    BuiltInStyle {
        name: "blend",
        model: None,
    },
];

/// One folder under a type: a style, or `overrides`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FolderStatus {
    pub name: String,
    pub files: u64,
    pub bytes: u64,
}

/// One asset type's folder.
#[derive(Debug, Clone, Serialize)]
pub struct TypeStatus {
    pub asset_type: &'static str,
    /// Style folders and `overrides`, by name.
    pub folders: Vec<FolderStatus>,
}

/// Whether the upscaler and each AI style's model are in the folder a build
/// would use ([`upscaler::resolve`]).
#[derive(Debug, Clone, Serialize)]
pub struct ToolsStatus {
    pub dir: String,
    /// Where that folder came from; `None` when no folder has the upscaler
    /// yet, and `dir` is DoD Studio's own, which Download fills.
    pub source: Option<upscaler::UpscalerSource>,
    /// The folder the user chose, whether or not it is the one used.
    pub chosen: Option<String>,
    pub upscaler: String,
    pub upscaler_present: bool,
    pub models: Vec<ModelStatus>,
    /// Every model in the folder (both halves present), by file stem, sorted:
    /// what a custom AI style can use.
    pub available_models: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelStatus {
    pub style: &'static str,
    pub model: &'static str,
    pub present: bool,
}

/// Everything the HD page's status panel shows.
#[derive(Debug, Clone, Serialize)]
pub struct HdStatus {
    pub hd_root: String,
    pub hd_root_exists: bool,
    pub types: Vec<TypeStatus>,
    /// Every style with a folder under at least one type, sorted: what
    /// `dodstudio_hd_style` can usefully be set to.
    pub built_styles: Vec<String>,
    /// Every built-in style, built or not, so the page can offer them all.
    pub known_styles: Vec<&'static str>,
    pub default_style: &'static str,
    /// The cvar names, so the page's `movie.cfg` lines come from here.
    pub enabled_cvar: &'static str,
    pub style_cvar: &'static str,
    pub tools: ToolsStatus,
    /// Which Python a build would use. Filled in by the caller, since
    /// finding out runs a few processes: [`python::resolve`].
    pub python: Option<python::PythonStatus>,
    /// The build scripts' folder, `None` when this copy of the app has none.
    pub scripts: Option<String>,
    /// The install's `my_styles.txt`. Filled in by the caller, which knows
    /// the scripts' folder: [`my_styles::read`].
    pub my_styles: Option<my_styles::MyStyles>,
    /// The maps the style preview can sample ([`preview::map_choices`]),
    /// filled in by the caller.
    pub maps: Vec<String>,
    /// The install's `hd_maps.txt` and every map it can pick from. Filled in
    /// by the caller, which knows the scripts' folder: [`map_list::read`].
    pub map_list: Option<map_list::MapList>,
}

/// `<game>\dod\dodstudio_hd`, from the `hl.exe` path the app launches.
pub fn hd_root(game_exe: &Path) -> Option<PathBuf> {
    Some(game_exe.parent()?.join("dod").join("dodstudio_hd"))
}

/// Reads what is built under `hd_root` and what is set up in `tools_dir`.
///
/// Missing folders are reported as missing, not as errors: before the first
/// build there is nothing to find, and that is the page's starting state.
pub fn scan(hd_root: &Path, tools_dir: &Path) -> HdStatus {
    let types: Vec<TypeStatus> = ASSET_TYPES
        .iter()
        .map(|&asset_type| TypeStatus {
            asset_type,
            folders: folders_in(&hd_root.join(asset_type)),
        })
        .collect();

    let mut built_styles: Vec<String> = types
        .iter()
        .flat_map(|t| t.folders.iter())
        .filter(|f| f.name != OVERRIDES && f.files > 0)
        .map(|f| f.name.clone())
        .collect();
    built_styles.sort();
    built_styles.dedup();

    HdStatus {
        hd_root: hd_root.to_string_lossy().to_string(),
        hd_root_exists: hd_root.is_dir(),
        types,
        built_styles,
        known_styles: BUILT_IN_STYLES.iter().map(|s| s.name).collect(),
        default_style: DEFAULT_STYLE,
        enabled_cvar: ENABLED_CVAR,
        style_cvar: STYLE_CVAR,
        tools: tools_status(tools_dir),
        python: None,
        scripts: None,
        my_styles: None,
        maps: Vec::new(),
        map_list: None,
    }
}

/// Each subfolder of `type_dir` with its file count and size, sorted by name.
fn folders_in(type_dir: &Path) -> Vec<FolderStatus> {
    let Ok(entries) = std::fs::read_dir(type_dir) else {
        return Vec::new();
    };
    let mut folders: Vec<FolderStatus> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| {
            let (files, bytes) = count_files(&e.path());
            FolderStatus {
                name: e.file_name().to_string_lossy().to_string(),
                files,
                bytes,
            }
        })
        .collect();
    folders.sort_by(|a, b| a.name.cmp(&b.name));
    folders
}

/// Files and bytes under `dir`, all the way down.
fn count_files(dir: &Path) -> (u64, u64) {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .fold((0, 0), |(files, bytes), e| {
            (
                files + 1,
                bytes + e.metadata().map(|m| m.len()).unwrap_or(0),
            )
        })
}

fn tools_status(tools_dir: &Path) -> ToolsStatus {
    let upscaler = setup::upscaler_exe(tools_dir);
    let models = BUILT_IN_STYLES
        .iter()
        .filter_map(|s| s.model.map(|model| (s.name, model)))
        .map(|(style, model)| ModelStatus {
            style,
            model,
            present: setup::model_present(tools_dir, model),
        })
        .collect();
    ToolsStatus {
        dir: tools_dir.to_string_lossy().to_string(),
        source: None,
        chosen: None,
        upscaler: upscaler.to_string_lossy().to_string(),
        upscaler_present: upscaler.is_file(),
        models,
        available_models: available_models(tools_dir),
    }
}

/// The models in `tools_dir\models` with both a `.param` and a `.bin`.
fn available_models(tools_dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(tools_dir.join("models")) else {
        return Vec::new();
    };
    let mut models: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.strip_suffix(".param").map(str::to_string)
        })
        .filter(|model| setup::model_present(tools_dir, model))
        .collect();
    models.sort();
    models
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    #[test]
    fn the_hd_folder_sits_in_dod_beside_hl_exe() {
        let root = hd_root(Path::new("C:/Games/Half-Life/hl.exe")).unwrap();
        assert_eq!(root, Path::new("C:/Games/Half-Life/dod/dodstudio_hd"));
    }

    #[test]
    fn nothing_built_is_a_status_not_an_error() {
        let dir = Scratch::new("hd_empty");
        let status = scan(&dir.join("dodstudio_hd"), &dir.join("tools"));
        assert!(!status.hd_root_exists);
        assert_eq!(status.types.len(), ASSET_TYPES.len());
        assert!(status.types.iter().all(|t| t.folders.is_empty()));
        assert!(status.built_styles.is_empty());
        assert!(!status.tools.upscaler_present);
        assert!(status.tools.models.iter().all(|m| !m.present));
        assert!(status.tools.available_models.is_empty());
    }

    #[test]
    fn every_whole_model_in_the_folder_is_available() {
        let dir = Scratch::new("hd_available_models");
        let models = dir.join("tools").join("models");
        std::fs::create_dir_all(&models).unwrap();
        for file in [
            "realesrgan-x4plus-anime.param",
            "realesrgan-x4plus-anime.bin",
            "ultrasharp-4x.bin",
            "ultrasharp-4x.param",
            "half.param",
            "notes.txt",
        ] {
            std::fs::write(models.join(file), b"").unwrap();
        }
        let status = scan(&dir.join("dodstudio_hd"), &dir.join("tools"));
        assert_eq!(
            status.tools.available_models,
            ["realesrgan-x4plus-anime", "ultrasharp-4x"]
        );
    }

    #[test]
    fn styles_are_counted_per_type_and_overrides_is_not_a_style() {
        let dir = Scratch::new("hd_styles");
        let root = dir.join("dodstudio_hd");
        for (path, size) in [
            ("world/ultrasharp/wall_0badf00d.tga", 10),
            ("world/ultrasharp/floor_12345678.tga", 20),
            ("world/overrides/wall_0badf00d.tga", 5),
            ("sky/plain/sky/dod_anziobk.tga", 7),
        ] {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, vec![0u8; size]).unwrap();
        }
        // An empty style folder is not a built style.
        std::fs::create_dir_all(root.join("models/remacri")).unwrap();

        let status = scan(&root, &dir.join("tools"));
        let world = &status.types[0];
        assert_eq!(world.asset_type, "world");
        assert_eq!(
            world.folders,
            vec![
                FolderStatus {
                    name: "overrides".into(),
                    files: 1,
                    bytes: 5
                },
                FolderStatus {
                    name: "ultrasharp".into(),
                    files: 2,
                    bytes: 30
                },
            ]
        );
        // Files in a style's subfolders count too (sky faces live in sky/).
        let sky = status.types.iter().find(|t| t.asset_type == "sky").unwrap();
        assert_eq!(sky.folders[0].files, 1);
        assert_eq!(status.built_styles, vec!["plain", "ultrasharp"]);
    }

    /// The names here are copies of the hook's and the scripts'. Neither can be
    /// imported (a cdylib, and Python), so the sources are read instead: a
    /// rename on either side fails here rather than in a user's movie.cfg.
    #[test]
    fn the_names_match_the_hook_and_the_scripts() {
        let hook = include_str!("../../../goldsrc-hooks/src/texture_hires.rs");
        let prefix = "dodstudio_";
        for (cvar, suffix) in [(ENABLED_CVAR, "hd_enabled"), (STYLE_CVAR, "hd_style")] {
            assert_eq!(cvar, format!("{prefix}{suffix}"));
            assert!(
                hook.contains(&format!("console_name!(\"{suffix}\")")),
                "{cvar}"
            );
        }
        assert!(hook.contains(&format!("DEFAULT_STYLE: &str = \"{DEFAULT_STYLE}\"")));
        assert!(hook.contains(&format!("OVERRIDES: &str = \"{OVERRIDES}\"")));

        let styles = include_str!("../../../goldsrc-hooks/tools/hd/styles.py");
        assert!(styles.contains(&format!("DEFAULT = \"{DEFAULT_STYLE}\"")));
        for style in BUILT_IN_STYLES {
            let line = match style.model {
                Some(model) => format!("\"{}\": (\"ai\", \"{model}\")", style.name),
                None => format!("\"{}\": (\"", style.name),
            };
            assert!(styles.contains(&line), "{line}");
        }
    }

    #[test]
    fn every_ai_style_has_a_model_and_the_rest_do_not() {
        for style in BUILT_IN_STYLES {
            let ai = !matches!(style.name, "plain" | "blend");
            assert_eq!(style.model.is_some(), ai, "{}", style.name);
        }
        assert!(BUILT_IN_STYLES.iter().any(|s| s.name == DEFAULT_STYLE));
    }
}
