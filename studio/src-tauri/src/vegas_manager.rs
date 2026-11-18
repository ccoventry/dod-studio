//! The Render tab's "Add Scripts to VEGAS" button (#429, #1514): the Tauri
//! surface over `native::vegas`.

use std::time::Instant;

use native::vegas;
use tauri::{AppHandle, Manager};

/// Copies the bundled VEGAS scripts (nameplates, relink) into the user's
/// `Documents\Vegas Script Menu`. Returns that folder's path.
#[tauri::command(async)]
pub fn vegas_install_script(app: AppHandle) -> Result<String, String> {
    let started = Instant::now();
    native::log_markdown(crate::messages::vegas::INSTALLING_SCRIPT);
    let bundled = app
        .path()
        .resource_dir()
        .ok()
        .map(|dir| dir.join("vegas-scripts"));
    let result = vegas::script_source(bundled.as_deref())
        .ok_or_else(|| native::messages::VEGAS_NO_SCRIPT.to_string())
        .and_then(|source| {
            let documents = vegas::documents_dir()?;
            vegas::install_scripts(&source, &documents)
        });
    match &result {
        Ok(path) => native::log_markdown(&crate::messages::vegas::script_installed(
            vegas::SCRIPT_FILES.len(),
            path.display(),
            started.elapsed().as_millis(),
        )),
        Err(err) => native::log_markdown(&crate::messages::vegas::script_install_failed(err)),
    }
    result.map(|path| path.display().to_string())
}
