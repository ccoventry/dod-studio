//! `g_PlayerExtraInfo`: the per-player record `client.dll` keeps for the
//! scoreboard, the kill feed's colours and the spectator UI (#1130).
//!
//! ## Where it is
//!
//! `extra_player_info_t g_PlayerExtraInfo[65]` at `client+0xe9118`, 48 bytes
//! a player, indexed by entity index (1..=32 are the players). The names and
//! member order are Valve's, from `client.so`'s DWARF (`linux_so.py client
//! struct extra_player_info_t`, `g_PlayerExtraInfo` 3,120 bytes); every
//! offset below is `client.dll`'s, from its own code (82 references, the
//! same `client.dll` in the PRE and POST installs):
//!
//! ```text
//!   +0x00 short frags       +0x0c int   status       +0x24 int  health
//!   +0x02 short objscore    +0x10 char  teamname[16] +0x28 int  teamId
//!   +0x04 short deaths      +0x20 float showhealth   +0x2c bool dead
//!   +0x06 short playerclass (never read or written)
//!   +0x08 short teamnumber
//! ```
//!
//! ## Who fills `health`
//!
//! Two writers, nothing else:
//!
//! - `HUD_ProcessPlayerState` (`client+0x31130`) copies
//!   `entity_state_t::health` for every player in the frame's packet, in
//!   every kind of demo. DoD's `delta.lst` carries it for players (`health,
//!   DT_BYTE, 8`), so it is current for every player the recorder can see.
//! - `CHud::MsgFunc_HLTV` (`client+0x222c9`) writes it from the `HLTV` user
//!   message (`BYTE player, BYTE health | 0x80`; player 0 sets all), which
//!   the server sends only to spectators: HLTV demos have it, POV demos not.
//!
//! `tools/verify_player_info_offsets.py` checks both writers' operands in
//! both installs. What the demos carry is in `docs/goldsrc_player_info.md`.
//!
//! Every read is of a fixed `.data` array in a mapped `client.dll`, bounded
//! to 1..=32 first, so it can't fault or run past the table.

#![cfg_attr(not(target_arch = "x86"), allow(dead_code))]

/// `g_PlayerExtraInfo`, as an RVA in `client.dll`.
const TABLE_RVA: usize = 0xe9_118;
/// `sizeof(extra_player_info_t)`.
const STRIDE: usize = 48;
const TEAM_AT: usize = 0x08;
const HEALTH_AT: usize = 0x24;
const DEAD_AT: usize = 0x2c;

/// Allies (British maps included) and Axis, as `teamnumber` holds them.
pub(crate) const ALLIES: i32 = 1;
pub(crate) const AXIS: i32 = 2;

/// The address of `field` for player `index`, or `None` outside 1..=32.
fn field_address(base: usize, index: i32, field: usize) -> Option<usize> {
    if !(1..=32).contains(&index) {
        return None;
    }
    Some(base + TABLE_RVA + STRIDE * index as usize + field)
}

/// Player `index`'s `teamnumber` (0 none, [`ALLIES`], [`AXIS`], 3
/// spectators), for 1..=32 in the `client.dll` loaded at `base`.
pub(crate) fn team(base: usize, index: i32) -> Option<i32> {
    let at = field_address(base, index, TEAM_AT)?;
    // Safety: inside the 65-entry `.data` array of a mapped client.dll.
    Some(i32::from(unsafe { *(at as *const i16) }))
}

/// Player `index`'s `health` as the client last heard it (see the module
/// doc for from where).
pub(crate) fn health(base: usize, index: i32) -> Option<i32> {
    let at = field_address(base, index, HEALTH_AT)?;
    // Safety: as in `team`.
    Some(unsafe { *(at as *const i32) })
}

/// Whether the client holds player `index` as dead.
pub(crate) fn is_dead(base: usize, index: i32) -> Option<bool> {
    let at = field_address(base, index, DEAD_AT)?;
    // Safety: as in `team`.
    Some(unsafe { *(at as *const u8) } != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_player_slots_have_an_address() {
        assert_eq!(field_address(0, 0, TEAM_AT), None);
        assert_eq!(field_address(0, 33, TEAM_AT), None);
        assert_eq!(field_address(0, -1, HEALTH_AT), None);
        assert_eq!(field_address(0, 1, TEAM_AT), Some(0xe9_118 + 48 + 8));
    }

    #[test]
    fn the_last_slot_ends_inside_the_65_entry_table() {
        let end = field_address(0, 32, DEAD_AT).unwrap();
        assert!(end < TABLE_RVA + 65 * STRIDE);
    }
}
