//! Configuration › Paths' install list (#931): `native::sys::installs`, for
//! the Half-Life Executable dropdown and Setup's one-install fill.

/// Every Half-Life install on this PC with its engine build. `game_path` is
/// the hl.exe set now, so the library it is in is searched even when
/// Steam's own list misses it. Reads each `hw.dll`, so off the main thread.
#[tauri::command]
pub async fn find_installs(
    game_path: String,
) -> Result<Vec<native::sys::installs::FoundInstall>, String> {
    let hint = (!game_path.trim().is_empty()).then(|| std::path::PathBuf::from(game_path.trim()));
    crate::messages::spawn_blocking_result(tokio::task::spawn_blocking(move || {
        native::sys::installs::find(hint.as_deref())
    }))
    .await
}
