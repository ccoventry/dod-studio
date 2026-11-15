use super::*;

/// How many bytes an old server build (seen: `BUILD 2693`, 2004-era) sends
/// after the VAC byte when that byte is set. Today's client reads only the VAC
/// byte (Xash3D's GoldSrc client the same), and no server build in a library
/// of 1,105 demos (builds 3647 to 10210) sends anything after it. The bytes
/// look like an opaque block (the same 21 in two recordings of different
/// sessions), so they are kept as-is and written back unchanged.
const LEGACY_TRAILER_LEN: usize = 21;

/// `svc_sendextrainfo`: the message every server sends straight after
/// `svc_serverinfo` at sign-on, in modern and old demos alike.
const SVC_SEND_EXTRA_INFO: u8 = 54;

/// Splits the old-build block off the bytes after `svc_serverinfo`'s VAC
/// byte, or returns `None` when there is none.
///
/// Recognised only by its shape: VAC byte set, the next byte not
/// `svc_sendextrainfo`, and `svc_sendextrainfo` exactly
/// [`LEGACY_TRAILER_LEN`] bytes later. Without it the block's first byte reads
/// as `svc_version` and every message after it is misaligned; the parse then
/// failed on a garbage `svc_deltadescription` (#1456).
fn split_legacy_trailer(vac: u8, rest: &[u8]) -> Option<(&[u8], &[u8])> {
    if vac == 0 || rest.first() == Some(&SVC_SEND_EXTRA_INFO) {
        return None;
    }
    if rest.get(LEGACY_TRAILER_LEN) != Some(&SVC_SEND_EXTRA_INFO) {
        return None;
    }
    Some(rest.split_at(LEGACY_TRAILER_LEN))
}

impl Doer for SvcServerInfo {
    fn id(&self) -> u8 {
        11
    }

    fn parse(i: &[u8], aux: AuxRefCell) -> Result<Self> {
        map(
            (
                le_i32,
                le_i32,
                le_i32,
                take(16usize),
                le_u8,
                le_u8,
                le_u8,
                null_string,
                null_string,
                null_string,
                null_string,
                le_u8,
            ),
            |(
                protocol,
                spawn_count,
                map_checksum,
                client_dll_hash,
                max_players,
                player_index,
                is_deathmatch,
                game_dir,
                hostname,
                map_file_name,
                map_cycle,
                unknown,
            )| {
                let mut aux = aux.borrow_mut();

                // mutate max_client
                aux.max_client = max_players;

                Self {
                    protocol,
                    spawn_count,
                    map_checksum,
                    client_dll_hash: client_dll_hash.into(),
                    max_players,
                    player_index,
                    is_deathmatch,
                    game_dir: game_dir.to_vec(),
                    hostname: hostname.to_vec(),
                    map_file_name: map_file_name.to_vec(),
                    map_cycle: map_cycle.to_vec(),
                    unknown,
                    legacy_trailer: None,
                }
            },
        )
        .parse(i)
        .map(
            |(i, mut info)| match split_legacy_trailer(info.unknown, i) {
                Some((trailer, rest)) => {
                    info.legacy_trailer = Some(trailer.to_vec());
                    (rest, info)
                }
                None => (i, info),
            },
        )
    }

    fn write(&self, _: AuxRefCell) -> ByteVec {
        let mut writer = ByteWriter::new();

        writer.append_u8(self.id());

        writer.append_i32(self.protocol);
        writer.append_i32(self.spawn_count);
        writer.append_i32(self.map_checksum);
        writer.append_u8_slice(self.client_dll_hash.padded(16).as_slice());
        writer.append_u8(self.max_players);
        writer.append_u8(self.player_index);
        writer.append_u8(self.is_deathmatch);
        writer.append_u8_slice(&self.game_dir);
        writer.append_u8_slice(&self.hostname);
        writer.append_u8_slice(&self.map_file_name);
        writer.append_u8_slice(&self.map_cycle);
        writer.append_u8(self.unknown);
        if let Some(trailer) = &self.legacy_trailer {
            writer.append_u8_slice(trailer);
        }

        writer.data
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;
    use crate::types::Aux;

    /// The 21 bytes the two `BUILD 2693` recordings carry after the VAC byte.
    const OLD_BUILD_TRAILER: [u8; LEGACY_TRAILER_LEN] = [
        4, 69, 56, 175, 166, 100, 177, 13, 71, 217, 253, 112, 178, 132, 157, 145, 6, 108, 139, 112,
        191,
    ];

    /// An `svc_serverinfo` body (no id byte) ending in `vac`, followed by
    /// `after`.
    fn server_info_body(vac: u8, after: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&48i32.to_le_bytes());
        body.extend_from_slice(&234i32.to_le_bytes());
        body.extend_from_slice(&0x1c6b_b4dfi32.to_le_bytes());
        body.extend_from_slice(&[0x11; 16]);
        body.extend_from_slice(&[14, 5, 1]);
        body.extend_from_slice(b"dod\0server\0maps/dod_avalanche.bsp\0dod_anzio\r\n\0");
        body.push(vac);
        body.extend_from_slice(after);
        body
    }

    fn aux() -> AuxRefCell {
        Rc::new(RefCell::new(Aux::new_raw()))
    }

    /// `svc_sendextrainfo` with an empty fallback dir and cheats off.
    const SEND_EXTRA_INFO: [u8; 3] = [SVC_SEND_EXTRA_INFO, 0, 0];

    #[test]
    fn an_old_build_trailer_is_kept_and_the_next_message_starts_after_it() {
        let after = [OLD_BUILD_TRAILER.as_slice(), &SEND_EXTRA_INFO].concat();
        let body = server_info_body(1, &after);

        let (rest, info) = SvcServerInfo::parse(&body, aux()).unwrap();

        assert_eq!(rest, SEND_EXTRA_INFO);
        assert_eq!(
            info.legacy_trailer.as_deref(),
            Some(OLD_BUILD_TRAILER.as_slice())
        );
    }

    #[test]
    fn an_old_build_trailer_is_written_back_byte_for_byte() {
        let body = server_info_body(1, &OLD_BUILD_TRAILER);
        let with_next = [body.as_slice(), &SEND_EXTRA_INFO].concat();

        let (_, info) = SvcServerInfo::parse(&with_next, aux()).unwrap();

        assert_eq!(info.write(aux()), [&[11u8][..], &body].concat());
    }

    #[test]
    fn a_modern_server_info_reads_only_the_vac_byte() {
        for vac in [0, 1] {
            let body = server_info_body(vac, &SEND_EXTRA_INFO);

            let (rest, info) = SvcServerInfo::parse(&body, aux()).unwrap();

            assert_eq!(rest, SEND_EXTRA_INFO, "vac {vac}");
            assert_eq!(info.legacy_trailer, None, "vac {vac}");
        }
    }

    #[test]
    fn a_set_vac_byte_without_the_trailer_shape_is_left_alone() {
        // svc_sendextrainfo 20 bytes on instead of 21: not the old-build block.
        let after = [&OLD_BUILD_TRAILER[..20], &SEND_EXTRA_INFO].concat();
        let body = server_info_body(1, &after);

        let (rest, info) = SvcServerInfo::parse(&body, aux()).unwrap();

        assert_eq!(rest, after.as_slice());
        assert_eq!(info.legacy_trailer, None);
    }

    #[test]
    fn a_server_info_at_the_end_of_its_message_has_no_trailer() {
        let body = server_info_body(1, &[]);

        let (rest, info) = SvcServerInfo::parse(&body, aux()).unwrap();

        assert!(rest.is_empty());
        assert_eq!(info.legacy_trailer, None);
    }
}
