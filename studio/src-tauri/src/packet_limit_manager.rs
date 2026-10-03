//! The engine's packet-entity limit for the Capture page's warning (#207):
//! `native::sys::packet_limit`, for the `hl.exe` Studio launches.

/// The `MAX_PACKET_ENTITIES` of the engine beside `game_path`, or `None` when
/// its `hw.dll` can't be read.
#[tauri::command]
pub async fn engine_packet_entity_limit(game_path: String) -> Option<u32> {
    tokio::task::spawn_blocking(move || {
        native::sys::packet_limit::packet_entity_limit(std::path::Path::new(&game_path))
    })
    .await
    .ok()
    .flatten()
}
