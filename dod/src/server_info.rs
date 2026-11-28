//! The map build a demo was recorded on, read from its `svc_serverinfo`.
//!
//! A POV demo's header carries the map's checksum; an HLTV demo's header
//! zeroes it (#1539). Every demo's first `svc_serverinfo` carries it too,
//! scrambled by the engine's `COM_Munge3` keyed on the recorder's slot, and
//! unscrambling it gives exactly `bsp::map_checksum` of the map the server
//! ran. Pure bytes in, value out: no I/O, so the web analyzer can share it.

/// Where a demo's first frame starts: the header is 544 bytes.
pub const DEMO_HEADER_LEN: usize = 544;

/// The first four bytes of the engine's `COM_Munge3` table, the only ones a
/// single 32-bit value uses.
const MUNGE_TABLE: [u8; 4] = [0x20, 0x07, 0x13, 0x61];

/// `svc_serverinfo`'s message id.
const SVC_SERVER_INFO: u8 = 11;

/// The network protocols the finder accepts: the two it was checked against
/// (47 is the 2004-era demos, 48 is 1.3).
const KNOWN_PROTOCOLS: [i32; 2] = [47, 48];

/// Offsets inside a `svc_serverinfo`, from its id byte.
const CHECKSUM_AT: usize = 9;
const CLIENT_DLL_HASH_AT: usize = 13;
const CLIENT_DLL_HASH_LEN: usize = 16;
const MAX_PLAYERS_AT: usize = 29;
const PLAYER_INDEX_AT: usize = 30;
const GAME_DIR_AT: usize = 32;
/// The longest game directory name the finder believes.
const GAME_DIR_MAX: usize = 32;

/// What the first `svc_serverinfo` says about the recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfoStamp {
    /// The map checksum as sent: scrambled, see [`Self::map_checksum`].
    pub scrambled_map_checksum: u32,
    /// The recorder's slot, which keys the scramble.
    pub player_index: u8,
    pub max_players: u8,
    /// The server's `client.dll` MD5: all zeros from a modern 1.3 server.
    pub client_dll_hash: [u8; CLIENT_DLL_HASH_LEN],
}

impl ServerInfoStamp {
    /// The checksum of the map the server ran: `bsp::map_checksum`'s value.
    pub fn map_checksum(&self) -> u32 {
        unscramble_map_checksum(self.scrambled_map_checksum, self.player_index)
    }
}

/// Undoes the engine's `COM_Munge3` on a `svc_serverinfo` map checksum.
///
/// `player_index` is the slot byte from the same message. The key is its
/// complement, so slot 0 is key `0xff`.
pub fn unscramble_map_checksum(value: u32, player_index: u8) -> u32 {
    let key = !player_index;
    let mut bytes = (value ^ u32::from(key)).to_le_bytes();
    for (j, byte) in bytes.iter_mut().enumerate() {
        *byte ^= (0xa5 | (j << j) | j | usize::from(MUNGE_TABLE[j])) as u8;
    }
    u32::from_be_bytes(bytes) ^ !u32::from(key)
}

/// Finds the first `svc_serverinfo` in the start of a demo file.
///
/// `file_start` is the file from byte 0, as many bytes as the caller read:
/// the message sits in the LOADING segment, in the first netmsg frame, so a
/// few tens of KB cover it (53 demos checked, the first 60,000 bytes found
/// it in each). The search is by byte pattern from the end of the header, and
/// a hit is accepted only when the fields around it read as a server info:
/// a known protocol, a player count of 1 to 32, a slot within it and a
/// printable game directory. `None` when there is none, or the file is cut
/// before it.
pub fn find_server_info(file_start: &[u8]) -> Option<ServerInfoStamp> {
    let body = file_start.get(DEMO_HEADER_LEN..)?;
    body.iter()
        .enumerate()
        .filter(|(_, b)| **b == SVC_SERVER_INFO)
        .find_map(|(at, _)| read_server_info(&body[at..]))
}

/// The stamp at the start of `bytes`, when it reads as a `svc_serverinfo`.
fn read_server_info(bytes: &[u8]) -> Option<ServerInfoStamp> {
    let protocol = i32::from_le_bytes(bytes.get(1..5)?.try_into().ok()?);
    if !KNOWN_PROTOCOLS.contains(&protocol) {
        return None;
    }
    let max_players = *bytes.get(MAX_PLAYERS_AT)?;
    let player_index = *bytes.get(PLAYER_INDEX_AT)?;
    if !(1..=32).contains(&max_players) || player_index > max_players {
        return None;
    }
    let game_dir = bytes.get(GAME_DIR_AT..)?;
    let game_dir = &game_dir[..game_dir.len().min(GAME_DIR_MAX + 1)];
    let len = game_dir.iter().position(|b| *b == 0)?;
    if len == 0 || !game_dir[..len].iter().all(|b| b.is_ascii_graphic()) {
        return None;
    }
    Some(ServerInfoStamp {
        scrambled_map_checksum: u32::from_le_bytes(
            bytes.get(CHECKSUM_AT..CHECKSUM_AT + 4)?.try_into().ok()?,
        ),
        player_index,
        max_players,
        client_dll_hash: bytes
            .get(CLIENT_DLL_HASH_AT..CLIENT_DLL_HASH_AT + CLIENT_DLL_HASH_LEN)?
            .try_into()
            .ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A demo start: a zeroed header, then `padding` bytes, then a
    /// `svc_serverinfo` with the given fields.
    fn demo_start(
        padding: usize,
        protocol: i32,
        scrambled: u32,
        slot: u8,
        max_players: u8,
    ) -> Vec<u8> {
        let mut bytes = vec![0u8; DEMO_HEADER_LEN + padding];
        bytes.push(SVC_SERVER_INFO);
        bytes.extend_from_slice(&protocol.to_le_bytes());
        bytes.extend_from_slice(&1i32.to_le_bytes());
        bytes.extend_from_slice(&scrambled.to_le_bytes());
        bytes.extend_from_slice(&[0x2f; 16]);
        bytes.extend_from_slice(&[max_players, slot, 1]);
        bytes.extend_from_slice(b"dod\0server\0maps/dod_anzio.bsp\0\0\0");
        bytes
    }

    #[test]
    fn an_hltv_demos_scrambled_checksum_unscrambles_to_the_maps_build() {
        // dod_anzio's own build 801b771f, as a real HLTV demo (slot 0) sent it.
        assert_eq!(unscramble_map_checksum(0xe037_4325, 0), 0x801b_771f);
    }

    #[test]
    fn a_pov_demos_scrambled_checksum_unscrambles_to_its_header_value() {
        // test-fixtures/ci_fixture.dem: slot 2, header checksum 801b771f.
        assert_eq!(unscramble_map_checksum(0xe237_4327, 2), 0x801b_771f);
    }

    #[test]
    fn a_different_slot_unscrambles_a_different_value() {
        // chemille-gamewyze-fx-1.dem (2004, recorded on build bb690c32), slot 12.
        assert_eq!(
            unscramble_map_checksum(CHEMILLE_FX_1_SCRAMBLED, 12),
            0xbb69_0c32
        );
    }

    /// What `chemille-gamewyze-fx-1.dem` carries at slot 12.
    const CHEMILLE_FX_1_SCRAMBLED: u32 = 0xc14c_3112;

    #[test]
    fn the_server_info_is_found_after_padding_and_read_with_its_slot() {
        let bytes = demo_start(300, 48, 0xe037_4325, 0, 32);

        let stamp = find_server_info(&bytes).unwrap();

        assert_eq!(stamp.map_checksum(), 0x801b_771f);
        assert_eq!(stamp.player_index, 0);
        assert_eq!(stamp.max_players, 32);
        assert_eq!(stamp.client_dll_hash, [0x2f; 16]);
    }

    #[test]
    fn a_stray_id_byte_before_the_server_info_is_skipped() {
        let mut bytes = vec![0u8; DEMO_HEADER_LEN];
        bytes.extend_from_slice(&[SVC_SERVER_INFO, 1, 2, 3]);
        bytes.extend_from_slice(&demo_start(0, 47, 0xe237_4327, 2, 16)[DEMO_HEADER_LEN..]);

        let stamp = find_server_info(&bytes).unwrap();

        assert_eq!(stamp.map_checksum(), 0x801b_771f);
        assert_eq!(stamp.max_players, 16);
    }

    #[test]
    fn something_that_does_not_read_as_a_server_info_is_refused() {
        // Unknown protocol, no players, slot past the player count, no game dir.
        for bytes in [
            demo_start(10, 99, 1, 0, 32),
            demo_start(10, 48, 1, 0, 0),
            demo_start(10, 48, 1, 40, 32),
            demo_start(10, 48, 1, 0, 33),
        ] {
            assert_eq!(find_server_info(&bytes), None);
        }
        let mut no_dir = demo_start(10, 48, 1, 0, 32);
        no_dir.truncate(DEMO_HEADER_LEN + 10 + 32);
        assert_eq!(find_server_info(&no_dir), None);
    }

    #[test]
    fn a_file_shorter_than_its_header_has_no_server_info() {
        assert_eq!(find_server_info(&[0u8; 100]), None);
        assert_eq!(find_server_info(&[]), None);
    }
}
