//! Who is in each demo (#1174): the player index's two commands, shared by
//! the Master Queue, the Demo Analyzer and Configuration > Players.

use crate::messages;
use std::sync::atomic::Ordering;

/// Which request `index_demo_players` is serving, per lane; a newer one stops
/// an older one between demos (the Demo Analyzer moved to another folder).
/// One lane each, so the Master Queue's and Configuration > Players' lookups
/// never cancel the Analyzer's or each other's.
static PLAYER_INDEX_ANALYZER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PLAYER_INDEX_QUEUE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PLAYER_INDEX_PLAYERS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Who is in each demo (#437, #174), one `demo_players` event per demo as it
/// is read: from the small player index when it is current (milliseconds), or
/// from the analyzer cache or a full parse (about a second) when it is not.
/// Resolves with how many demos were read.
#[tauri::command]
pub async fn index_demo_players(
    app_handle: tauri::AppHandle,
    paths: Vec<String>,
    request_id: u64,
    lane: String,
) -> Result<usize, String> {
    let current: &'static std::sync::atomic::AtomicU64 = match lane.as_str() {
        "queue" => &PLAYER_INDEX_QUEUE,
        "players" => &PLAYER_INDEX_PLAYERS,
        _ => &PLAYER_INDEX_ANALYZER,
    };
    current.store(request_id, Ordering::SeqCst);
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        use tauri::Emitter;
        let mut done = 0;
        let mut unreadable = 0;
        let total = paths.len();
        for path in paths {
            if current.load(Ordering::SeqCst) != request_id {
                break;
            }
            let result = native::player_index::demo_players(std::path::Path::new(&path));
            unreadable += usize::from(result.is_err());
            let payload = match result {
                Ok((demo, _)) => serde_json::json!({
                    "lane": lane,
                    "requestId": request_id,
                    "path": path,
                    "demoType": demo.demo_type,
                    "players": demo.players,
                }),
                Err(e) => serde_json::json!({
                    "lane": lane,
                    "requestId": request_id,
                    "path": path,
                    "error": e,
                }),
            };
            let _ = app_handle.emit("demo_players", payload);
            done += 1;
        }
        native::log_markdown(&format!(
            "[players] read the players of {done} of {total} demo(s) ({lane} lane), {unreadable} unreadable"
        ));
        Ok(done)
    }))
    .await
}

/// Which of `paths` already have their players (answered here, all at once),
/// which still need a read, and which sit on a cloud drive with no cache
/// entry (#992). Opens no demo, so it never downloads one.
#[tauri::command]
pub async fn lookup_demo_players(
    paths: Vec<String>,
) -> Result<native::player_index::PlayerLookup, String> {
    messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        Ok(native::player_index::lookup_players(&paths))
    }))
    .await
}
