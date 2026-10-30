//! `ds_player_overrides` (#428): 1 makes sprays and crosshairs in a
//! demo follow Studio's Players table (`player_table.rs`); 0 leaves both to
//! the game. Default 0, so a game nobody started from Studio behaves as
//! stock. Studio sets it from the Players tab's "Use in the game" box when it
//! starts the game, a review or a batch, and again when the box changes.
//!
//! What a flip reaches: the crosshair from the next frame (`player_crosshair`
//! decides every frame); sprays from the next one loaded, since a spray
//! already on a wall keeps the texture it was drawn with (reload the demo to
//! compare).

use crate::names::console_name;
use crate::setting::Cvar;

pub const NAME: &str = console_name!("player_overrides");

pub static CVAR: Cvar = Cvar::new();

/// Whether the table applies now. One atomic load and a float read, so the
/// per-frame and per-spray paths can ask freely.
pub fn on() -> bool {
    CVAR.on()
}

/// The `ds_debug_status` line.
pub fn status() -> String {
    if on() {
        format!("{NAME} 1: sprays and crosshairs in demos follow Studio's Players table")
    } else {
        format!("{NAME} 0: sprays and crosshairs are the game's own")
    }
}
