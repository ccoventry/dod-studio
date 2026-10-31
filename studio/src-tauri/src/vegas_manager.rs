//! The Render tab's "Add nameplate script to VEGAS" button (#429): the Tauri
//! surface over `native::vegas`.

use std::time::Instant;

use native::vegas;
use tauri::{AppHandle, Manager};

/// Copies the bundled nameplate script into the user's `Documents\Vegas
/// Script Menu`. Returns the installed file's path.
#[tauri::command(async)]
pub fn vegas_install_script(app: AppHandle) -> Result<String, String> {
    let started = Instant::now();
    native::log_markdown("[vegas] installing the nameplate script into the script menu");
    let bundled = app
        .path()
        .resource_dir()
        .ok()
        .map(|dir| dir.join("vegas-scripts"));
    let result = vegas::script_source(bundled.as_deref())
        .ok_or_else(|| native::messages::VEGAS_NO_SCRIPT.to_string())
        .and_then(|source| {
            let documents = vegas::documents_dir()?;
            vegas::install_script(&source, &documents)
        });
    match &result {
        Ok(path) => native::log_markdown(&format!(
            "[vegas] script installed at {} in {} ms",
            path.display(),
            started.elapsed().as_millis()
        )),
        Err(err) => native::log_markdown(&format!("[vegas] script install failed: {err}")),
    }
    result.map(|path| path.display().to_string())
}
