//! The game's damaged-demo notices, handed to the page (#1373): the hook DLL
//! sends one over Studio's game pipe when a demo it plays turns out damaged
//! (`native::sys::game_damage_notices`), and the page shows a toast with a
//! Repair button for it (`damaged_demo_toast.js`).

use tauri::{AppHandle, Emitter};

/// The page's event for one notice: a `game_damage_notices::DamageNotice`.
const DAMAGED_EVENT: &str = "game_demo_damaged";

/// Hands every damaged-demo notice the game sends to the page.
pub fn serve_damage_notices(app: AppHandle) {
    native::sys::game_damage_notices::set_damage_handler(move |notice| {
        app.emit(DAMAGED_EVENT, notice)
            .map_err(crate::messages::damage_notice_not_handed_on)
    });
}
