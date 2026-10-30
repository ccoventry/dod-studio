//! Demo folder links (#376): `native::demo_links`, for the Demo Folder Links
//! list beside + Add Folder. Each command takes the Half-Life Executable
//! setting, since the links live in that install's `dod\demos\`.

use native::demo_links::{self, DemoLink};
use std::path::Path;

/// The links in the game folder `hl_exe` is in.
#[tauri::command]
pub async fn list_demo_links(hl_exe: String) -> Result<Vec<DemoLink>, String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        // Unwrapped: the page says "Could not read the linked folders" (`listFailed`).
        let game = demo_links::game_dir_of(Path::new(&hl_exe))?;
        Ok(demo_links::list(&game))
    }))
    .await
}

/// Links `folder` into the game folder; the error says why it was refused.
#[tauri::command]
pub async fn add_demo_link(hl_exe: String, folder: String) -> Result<DemoLink, String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let game = demo_links::game_dir_of(Path::new(&hl_exe))
            .map_err(crate::messages::failed_to_add_demo_link)?;
        demo_links::add(&game, Path::new(&folder)).map_err(crate::messages::failed_to_add_demo_link)
    }))
    .await
}

/// Unlinks the link `name`; the folder it points at is not touched.
#[tauri::command]
pub async fn remove_demo_link(hl_exe: String, name: String) -> Result<(), String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let game = demo_links::game_dir_of(Path::new(&hl_exe))
            .map_err(crate::messages::failed_to_remove_demo_link)?;
        demo_links::remove(&game, &name).map_err(crate::messages::failed_to_remove_demo_link)
    }))
    .await
}
