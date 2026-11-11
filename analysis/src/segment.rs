//! Which maps a demo holds, and where its analysed part ends (#217).

use crate::{AnalyzerEvent, AnalyzerState};
use dem::types::EngineMessage;

/// Records every signon's map in `signon_maps`, before the segment gate, so
/// the ones after the match are listed too.
pub fn use_signon_maps(state: &mut AnalyzerState, event: &AnalyzerEvent) {
    if let AnalyzerEvent::EngineMessage(EngineMessage::SvcServerInfo(msg)) = event {
        let file = String::from_utf8_lossy(&msg.map_file_name);
        let map = file
            .trim_end_matches('\0')
            .trim_start_matches("maps/")
            .trim_end_matches(".bsp")
            .to_string();
        state.signon_maps.push(map);
    }
}

/// Ends the analysed demo at a second signon (#217).
///
/// `SvcServerInfo` starts every signon. A second one means the server changed
/// level and the demo kept recording: to another map, or to the *same* map,
/// as when both halves of a match are played back to back. Everything after
/// it re-sends teams, classes and the clock from scratch, so letting it
/// through resets every player to Unassigned and restarts the demo clock.
///
/// The first map is the one analysed, gameplay or not, and nothing after the
/// boundary is. The game's `viewdemo` stops at that same boundary, so the
/// analysis (and every highlight built from it) covers exactly what the game
/// can play. A demo with more than one map is split instead
/// (`signon_maps`, the Demo Analyzer's and the Master Queue's Split).
///
/// Runs before every other hook, so none of them sees the new signon: the
/// POV hook would otherwise take the new connection's player slot.
pub fn use_segment_boundary(state: &mut AnalyzerState, event: &AnalyzerEvent) {
    let AnalyzerEvent::EngineMessage(EngineMessage::SvcServerInfo(msg)) = event else {
        return;
    };
    if state.initial_map_name.is_some() {
        state.map_changed = true;
        return;
    }
    let map_name = String::from_utf8_lossy(&msg.map_file_name)
        .trim_end_matches('\0')
        .to_string();
    state.initial_map_name = Some(
        map_name
            .trim_start_matches("maps/")
            .trim_end_matches(".bsp")
            .to_string(),
    );
}
