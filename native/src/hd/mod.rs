//! The HD texture files, as the app sees them (#372).
//!
//! `goldsrc-hooks`' `texture_hires` swaps in upscaled textures from
//! `<game>\dod\dodstudio_hd\<type>\<style>\`, and `goldsrc-hooks/tools/hd/`'s
//! scripts build them. This module is the app's side of that: what is built
//! ([`scan`]), and fetching the upscaler the build needs ([`setup`]). Building
//! itself is still the scripts' job; the Rust port is #372's second step.
//!
//! The layout and names here mirror the hook's and the scripts', and must stay
//! in step with both:
//!
//! - `texture_hires.rs`: the five type folders, `overrides`, and the default
//!   style;
//! - `tools/hd/styles.py`: the built-in styles and their model files.

pub mod build;
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

/// What runs a style's model, if it has one (`styles.py`'s kinds).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    /// Real-ESRGAN ncnn-vulkan (`ai` in `styles.py`): a `.param` + `.bin` pair.
    Ncnn,
    /// `spandrel_run.py` under the spandrel venv: a `.pth`/`.safetensors`.
    Spandrel,
    /// No model: `plain` and `blend`.
    None,
}

/// A style the scripts know without `my_styles.txt`.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct BuiltInStyle {
    pub name: &'static str,
    pub backend: Backend,
    /// The model: a Real-ESRGAN file stem, or a spandrel file name with its
    /// extension.
    pub model: Option<&'static str>,
}

/// `tools/hd/styles.py`'s `BUILT_IN`, in the same order.
pub const BUILT_IN_STYLES: [BuiltInStyle; 10] = [
    BuiltInStyle {
        name: "ultrasharp",
        backend: Backend::Ncnn,
        model: Some("ultrasharp-4x"),
    },
    BuiltInStyle {
        name: "remacri",
        backend: Backend::Ncnn,
        model: Some("remacri-4x"),
    },
    BuiltInStyle {
        name: "siax",
        backend: Backend::Ncnn,
        model: Some("4x_NMKD-Siax_200k"),
    },
    BuiltInStyle {
        name: "generalv3",
        backend: Backend::Ncnn,
        model: Some("RealESRGAN_General_x4_v3"),
    },
    BuiltInStyle {
        name: "x4plus",
        backend: Backend::Ncnn,
        model: Some("realesrgan-x4plus"),
    },
    // A plain enlargement with sharpening: no AI, no model.
    BuiltInStyle {
        name: "plain",
        backend: Backend::None,
        model: None,
    },
    // Made from x4plus and plain, never upscaled on its own.
    BuiltInStyle {
        name: "blend",
        backend: Backend::None,
        model: None,
    },
    // The second backend's styles (`setup_tools.py --spandrel`).
    BuiltInStyle {
        name: "ultrasharpv2",
        backend: Backend::Spandrel,
        model: Some("4x-UltraSharpV2.safetensors"),
    },
    BuiltInStyle {
        name: "pbrify",
        backend: Backend::Spandrel,
        model: Some("4x-PBRify_UpscalerV4.pth"),
    },
    BuiltInStyle {
        name: "webphoto",
        backend: Backend::Spandrel,
        model: Some("4xNomosWebPhoto_RealPLKSR.pth"),
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
    /// The Real-ESRGAN styles' models.
    pub models: Vec<ModelStatus>,
    /// The second backend.
    pub spandrel: SpandrelStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelStatus {
    pub style: &'static str,
    pub model: &'static str,
    pub present: bool,
}

impl ToolsStatus {
    /// Whether a built-in style has what it runs on: for a Real-ESRGAN
    /// style the upscaler and its model, for a spandrel style the venv and
    /// its model, and nothing for the rest.
    pub fn tools_present_for(&self, style: &str) -> bool {
        let Some(built_in) = BUILT_IN_STYLES.iter().find(|s| s.name == style) else {
            return true;
        };
        let model_present =
            |models: &[ModelStatus]| models.iter().any(|m| m.style == style && m.present);
        match built_in.backend {
            Backend::Ncnn => self.upscaler_present && model_present(&self.models),
            Backend::Spandrel => {
                self.spandrel.python_present && model_present(&self.spandrel.models)
            }
            Backend::None => true,
        }
    }
}

/// The spandrel backend: `setup_tools.py --spandrel`'s folder, whether its
/// venv is there, and each spandrel style's model file.
#[derive(Debug, Clone, Serialize)]
pub struct SpandrelStatus {
    pub dir: String,
    pub python_present: bool,
    pub models: Vec<ModelStatus>,
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
    let with_backend = move |backend: Backend| {
        BUILT_IN_STYLES
            .iter()
            .filter(move |s| s.backend == backend)
            .filter_map(|s| s.model.map(|model| (s.name, model)))
    };
    let models = with_backend(Backend::Ncnn)
        .map(|(style, model)| ModelStatus {
            style,
            model,
            present: setup::model_present(tools_dir, model),
        })
        .collect();
    let spandrel = setup::spandrel_dir(tools_dir);
    let spandrel = SpandrelStatus {
        dir: spandrel.to_string_lossy().to_string(),
        python_present: setup::spandrel_python(&spandrel).is_file(),
        models: with_backend(Backend::Spandrel)
            .map(|(style, model)| ModelStatus {
                style,
                model,
                present: setup::spandrel_model_present(&spandrel, model),
            })
            .collect(),
    };
    ToolsStatus {
        dir: tools_dir.to_string_lossy().to_string(),
        source: None,
        chosen: None,
        upscaler: upscaler.to_string_lossy().to_string(),
        upscaler_present: upscaler.is_file(),
        models,
        spandrel,
    }
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
        assert!(!status.tools.spandrel.python_present);
        assert_eq!(status.tools.spandrel.models.len(), 3);
        assert!(status.tools.spandrel.models.iter().all(|m| !m.present));
    }

    #[test]
    fn the_spandrel_backend_sits_beside_the_upscaler_and_is_seen_when_set_up() {
        let dir = Scratch::new("hd_spandrel");
        let realesrgan = dir.join("hd_tools").join("realesrgan");
        let spandrel = setup::spandrel_dir(&realesrgan);
        assert_eq!(spandrel, dir.join("hd_tools").join("spandrel"));
        std::fs::create_dir_all(spandrel.join("venv").join("Scripts")).unwrap();
        std::fs::write(setup::spandrel_python(&spandrel), b"").unwrap();
        std::fs::create_dir_all(spandrel.join("models")).unwrap();
        std::fs::write(
            spandrel.join("models").join("4x-PBRify_UpscalerV4.pth"),
            b"",
        )
        .unwrap();
        let status = tools_status(&realesrgan);
        assert!(status.tools_present_for("pbrify"));
        assert!(!status.tools_present_for("ultrasharpv2"));
        assert!(status.spandrel.python_present);
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
            let line = match (style.backend, style.model) {
                (Backend::Ncnn, Some(model)) => {
                    format!("\"{}\": (\"ai\", \"{model}\")", style.name)
                }
                (Backend::Spandrel, Some(model)) => {
                    format!("\"{}\": (\"spandrel\", \"{model}\")", style.name)
                }
                _ => format!("\"{}\": (\"", style.name),
            };
            assert!(styles.contains(&line), "{line}");
        }
        // setup_tools.py fetches the same spandrel model files.
        let setup_py = include_str!("../../../goldsrc-hooks/tools/hd/setup_tools.py");
        for style in BUILT_IN_STYLES
            .iter()
            .filter(|s| s.backend == Backend::Spandrel)
        {
            let line = format!("\"{}\": (\"{}\",", style.name, style.model.unwrap());
            assert!(setup_py.contains(&line), "{line}");
        }
    }

    #[test]
    fn every_ai_style_has_a_model_and_the_rest_do_not() {
        for style in BUILT_IN_STYLES {
            let ai = !matches!(style.name, "plain" | "blend");
            assert_eq!(style.model.is_some(), ai, "{}", style.name);
            assert_eq!(style.backend != Backend::None, ai, "{}", style.name);
        }
        assert!(BUILT_IN_STYLES.iter().any(|s| s.name == DEFAULT_STYLE));
    }
}
