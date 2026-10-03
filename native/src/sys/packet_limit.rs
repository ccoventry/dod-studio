//! How many entities one snapshot may carry before the game closes (#207).
//!
//! `CL_ParsePacketEntities` stops at `MAX_PACKET_ENTITIES`: 256 on the
//! pre-Anniversary `hw.dll`, 1024 on the 25th Anniversary one (read from each
//! build's bounds check; the measurements are on #207). A demo whose snapshot
//! goes over the limit closes the game to the desktop the moment it gets
//! there, so Studio warns before a capture instead.

use std::path::Path;

/// The pre-Anniversary engine's `MAX_PACKET_ENTITIES`.
pub const PRE_ANNIVERSARY_PACKET_ENTITIES: u32 = 256;
/// The 25th Anniversary engine's `MAX_PACKET_ENTITIES`.
pub const ANNIVERSARY_PACKET_ENTITIES: u32 = 1024;

/// A cvar only the 25th Anniversary `hw.dll` registers (its world-shader
/// gate, see `goldsrc-hooks/src/world_shaders.rs`).
const ANNIVERSARY_MARKER: &[u8] = b"sv_allow_shaders";
/// The engine's own error string for the limit, in both builds.
const ENGINE_MARKER: &[u8] = b"MAX_PACKET_ENTITIES";

/// The limit of the `hw.dll` beside `game_exe` (`hl.exe`), or `None` when
/// there is no readable `hw.dll` that names the limit.
pub fn packet_entity_limit(game_exe: &Path) -> Option<u32> {
    let bytes = std::fs::read(game_exe.parent()?.join("hw.dll")).ok()?;
    limit_of(&bytes)
}

fn limit_of(hw_dll: &[u8]) -> Option<u32> {
    let has = |needle: &[u8]| hw_dll.windows(needle.len()).any(|w| w == needle);
    if !has(ENGINE_MARKER) {
        None
    } else if has(ANNIVERSARY_MARKER) {
        Some(ANNIVERSARY_PACKET_ENTITIES)
    } else {
        Some(PRE_ANNIVERSARY_PACKET_ENTITIES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    #[test]
    fn tells_the_two_builds_apart() {
        let pre = b"..CL_ParsePacketEntities: newindex == MAX_PACKET_ENTITIES..";
        let anniversary = b"..newindex == MAX_PACKET_ENTITIES..sv_allow_shaders..";
        assert_eq!(limit_of(pre), Some(256));
        assert_eq!(limit_of(anniversary), Some(1024));
        assert_eq!(limit_of(b"not an engine"), None);
    }

    #[test]
    fn reads_the_hw_dll_beside_hl_exe() {
        let dir = Scratch::new("packet_limit");
        std::fs::write(dir.join("hw.dll"), b"xx MAX_PACKET_ENTITIES xx").unwrap();
        assert_eq!(packet_entity_limit(&dir.join("hl.exe")), Some(256));
        assert_eq!(
            packet_entity_limit(&dir.join("missing").join("hl.exe")),
            None
        );
    }

    /// The real files, where this PC has them.
    #[test]
    #[ignore]
    fn the_installs_on_this_pc() {
        let common = Path::new("C:/Program Files (x86)/Steam/steamapps/common");
        for (install, want) in [
            ("Half-Life - PRE-Anniversary for Movies", 256),
            ("Half-Life - POST-Anniversary for Movies", 1024),
        ] {
            let exe = common.join(install).join("hl.exe");
            if exe.exists() {
                assert_eq!(packet_entity_limit(&exe), Some(want), "{install}");
            }
        }
    }
}
