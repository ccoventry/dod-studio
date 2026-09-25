//! Which Real-ESRGAN folder the build uses (#372 part 2).
//!
//! Like Python ([`super::python`]), an upscaler the user already has is used
//! rather than downloaded again. The candidates, best first:
//!
//! 1. **The folder the user chose** on the HD page ([`chosen_file`]).
//! 2. **DoD Studio's own**, `%APPDATA%\dod-studio\hd_tools\realesrgan`,
//!    where [`super::setup`] downloads to.
//! 3. **`realesrgan\` beside the scripts**, where the scripts' README and
//!    `setup_tools.py` put it.
//! 4. **`REALESRGAN`**, the variable the scripts read, if it is set for the
//!    app too.
//!
//! A folder counts only if it has the `.exe`. Among those, the one with the
//! most style models wins, and ties go to the earlier candidate, so a
//! complete folder anywhere beats a half-downloaded one.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{BUILT_IN_STYLES, setup};

/// Where a folder came from, for the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpscalerSource {
    Chosen,
    App,
    Scripts,
    Env,
}

/// Where the user's pick is kept: one line, the folder.
pub fn chosen_file(hd_tools: &Path) -> PathBuf {
    hd_tools.join("realesrgan.txt")
}

pub fn chosen(hd_tools: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(chosen_file(hd_tools)).ok()?;
    let line = text.lines().next()?.trim();
    (!line.is_empty()).then(|| PathBuf::from(line))
}

/// Saves the user's pick (a folder with `realesrgan-ncnn-vulkan.exe` in it),
/// or forgets it with `None`.
pub fn set_chosen(hd_tools: &Path, dir: Option<&Path>) -> Result<(), String> {
    let file = chosen_file(hd_tools);
    match dir {
        Some(dir) => {
            if !setup::upscaler_exe(dir).is_file() {
                return Err(crate::messages::hd_not_an_upscaler_folder(dir.display()));
            }
            std::fs::create_dir_all(hd_tools)
                .map_err(|e| crate::messages::labeled(hd_tools.display(), e))?;
            std::fs::write(&file, format!("{}\n", dir.display()))
                .map_err(|e| crate::messages::labeled(file.display(), e))
        }
        None => match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(crate::messages::labeled(file.display(), e))
            }
            _ => Ok(()),
        },
    }
}

/// The AI styles' model files, as `models\<name>.param/.bin`.
fn models() -> impl Iterator<Item = &'static str> {
    BUILT_IN_STYLES.iter().filter_map(|s| s.model)
}

/// How many of the AI styles' models `dir` has.
pub fn model_count(dir: &Path) -> usize {
    models().filter(|m| setup::model_present(dir, m)).count()
}

/// Whether `dir` has the upscaler and every AI style's model.
pub fn complete(dir: &Path) -> bool {
    setup::upscaler_exe(dir).is_file() && model_count(dir) == models().count()
}

/// The candidates in order (see the module doc), existing or not.
fn candidates(hd_tools: &Path, scripts: Option<&Path>) -> Vec<(UpscalerSource, PathBuf)> {
    let mut list = Vec::new();
    if let Some(dir) = chosen(hd_tools) {
        list.push((UpscalerSource::Chosen, dir));
    }
    list.push((UpscalerSource::App, setup::realesrgan_dir(hd_tools)));
    if let Some(scripts) = scripts {
        list.push((UpscalerSource::Scripts, scripts.join("realesrgan")));
    }
    // The scripts' variable names the .exe itself.
    if let Some(exe) = std::env::var_os("REALESRGAN").map(PathBuf::from)
        && let Some(dir) = exe.parent()
    {
        list.push((UpscalerSource::Env, dir.to_path_buf()));
    }
    list
}

/// The folder a build uses and where it came from, or `None` when no
/// candidate has the upscaler (Download then fills DoD Studio's own).
pub fn resolve(hd_tools: &Path, scripts: Option<&Path>) -> Option<(UpscalerSource, PathBuf)> {
    let mut best: Option<(usize, UpscalerSource, PathBuf)> = None;
    for (source, dir) in candidates(hd_tools, scripts) {
        if !setup::upscaler_exe(&dir).is_file() {
            continue;
        }
        let count = model_count(&dir);
        if best.as_ref().is_none_or(|(most, _, _)| count > *most) {
            best = Some((count, source, dir));
        }
    }
    best.map(|(_, source, dir)| (source, dir))
}

/// The folder to use, falling back to DoD Studio's own (empty until
/// Download) so the page always has a path to show.
pub fn resolve_or_app(
    hd_tools: &Path,
    scripts: Option<&Path>,
) -> (Option<UpscalerSource>, PathBuf) {
    match resolve(hd_tools, scripts) {
        Some((source, dir)) => (Some(source), dir),
        None => (None, setup::realesrgan_dir(hd_tools)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    fn fake_upscaler(dir: &Path, models: &[&str]) {
        std::fs::create_dir_all(dir.join("models")).unwrap();
        std::fs::write(setup::upscaler_exe(dir), b"").unwrap();
        for model in models {
            for ext in ["param", "bin"] {
                std::fs::write(dir.join(format!("models/{model}.{ext}")), b"").unwrap();
            }
        }
    }

    fn all_models() -> Vec<&'static str> {
        models().collect()
    }

    #[test]
    fn nothing_anywhere_is_none_and_falls_back_to_the_app_folder() {
        let tools = Scratch::new("hd_upscaler_none");
        assert!(resolve(&tools, None).is_none());
        assert_eq!(
            resolve_or_app(&tools, None),
            (None, setup::realesrgan_dir(&tools))
        );
    }

    #[test]
    fn a_complete_folder_beside_the_scripts_is_found() {
        let tools = Scratch::new("hd_upscaler_scripts_tools");
        let scripts = Scratch::new("hd_upscaler_scripts");
        fake_upscaler(&scripts.join("realesrgan"), &all_models());
        let (source, dir) = resolve(&tools, Some(&scripts)).unwrap();
        assert_eq!(source, UpscalerSource::Scripts);
        assert_eq!(dir, scripts.join("realesrgan"));
        assert!(complete(&dir));
    }

    #[test]
    fn the_most_complete_folder_wins_and_ties_go_to_the_earlier_one() {
        let tools = Scratch::new("hd_upscaler_rank");
        let scripts = Scratch::new("hd_upscaler_rank_scripts");
        // The app's own has only one model; beside the scripts has them all.
        fake_upscaler(&setup::realesrgan_dir(&tools), &["realesrgan-x4plus"]);
        fake_upscaler(&scripts.join("realesrgan"), &all_models());
        assert_eq!(
            resolve(&tools, Some(&scripts)).unwrap().0,
            UpscalerSource::Scripts
        );
        // Complete in both: the earlier candidate (the app's own) wins.
        fake_upscaler(&setup::realesrgan_dir(&tools), &all_models());
        assert_eq!(
            resolve(&tools, Some(&scripts)).unwrap().0,
            UpscalerSource::App
        );
    }

    #[test]
    fn a_chosen_folder_is_used_saved_and_forgotten() {
        let tools = Scratch::new("hd_upscaler_chosen");
        let mine = Scratch::new("hd_upscaler_mine");
        // Not an upscaler folder: refused, nothing saved.
        assert!(set_chosen(&tools, Some(&mine)).is_err());
        assert_eq!(chosen(&tools), None);

        fake_upscaler(&mine, &all_models());
        set_chosen(&tools, Some(&mine)).unwrap();
        assert_eq!(chosen(&tools), Some(mine.to_path_buf()));
        assert_eq!(
            resolve(&tools, None),
            Some((UpscalerSource::Chosen, mine.to_path_buf()))
        );

        set_chosen(&tools, None).unwrap();
        assert_eq!(chosen(&tools), None);
        assert!(resolve(&tools, None).is_none());
    }

    /// The scripts' README and setup_tools.py put the upscaler here too.
    #[test]
    fn the_scripts_default_folder_is_the_one_checked() {
        let styles = include_str!("../../../goldsrc-hooks/tools/hd/styles.py");
        assert!(
            styles.contains(r#"os.path.join(HERE, "realesrgan", "realesrgan-ncnn-vulkan.exe")"#)
        );
        assert!(styles.contains(r#"os.environ.get("REALESRGAN")"#));
    }
}
