//! R&D for #37 Tier 4 (throwaway): a parse that also leaves out the entity
//! snapshots (`svc_packetentities`, `svc_deltapacketentities`) and
//! `svc_clientdata`, which the capture-queue scan would not need if headshots
//! and kill positions came from elsewhere. Neither message has a length
//! prefix, so both flavours still read every bit to find the next message.

use crate::delta::skip_delta;
use crate::nom_helper::nom_fail;

use super::*;

/// What to do with the three message types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier4Flavour {
    /// Decode as usual, then drop the result at once.
    DecodeAndDrop,
    /// Walk the bits without building anything.
    BitWalk,
}

const SVC_CLIENT_DATA: u8 = 15;
const SVC_PACKET_ENTITIES: u8 = 40;
const SVC_DELTA_PACKET_ENTITIES: u8 = 41;

fn decoder<'a>(
    aux: &'a crate::types::Aux,
    name: &str,
) -> std::result::Result<&'a crate::types::DeltaDecoder, ()> {
    aux.delta_decoders.get(name).ok_or(())
}

fn walk_entities(i: &[u8], aux: AuxRefCell, delta_packet: bool) -> Result<()> {
    let aux = aux.borrow();
    let (Ok(player), Ok(custom), Ok(plain)) = (
        decoder(&aux, "entity_state_player_t\0"),
        decoder(&aux, "custom_entity_state_t\0"),
        decoder(&aux, "entity_state_t\0"),
    ) else {
        return nom_fail("missing entity decoder");
    };
    let mut br = BitReader::new(i);
    br.read_n_bit(16);
    if delta_packet {
        br.read_n_bit(8);
    }
    let mut entity_index: u16 = 0;
    loop {
        if br.peek_n_bits(16).to_u16() == 0 {
            br.read_n_bit(16);
            break;
        }
        let has_custom_delta;
        if delta_packet {
            let remove_entity = br.read_1_bit();
            if br.read_1_bit() {
                entity_index = br.read_n_bit(11).to_u16();
            } else {
                entity_index += br.read_n_bit(6).to_u16();
            }
            if remove_entity {
                continue;
            }
            has_custom_delta = br.read_1_bit();
        } else {
            if br.read_1_bit() {
                entity_index += 1;
            } else if br.read_1_bit() {
                entity_index = br.read_n_bit(11).to_u16();
            } else {
                entity_index += br.read_n_bit(6).to_u16();
            }
            has_custom_delta = br.read_1_bit();
            if br.read_1_bit() {
                br.read_n_bit(6);
            }
        }
        let dd = if entity_index > 0 && entity_index <= aux.max_client as u16 {
            player
        } else if has_custom_delta {
            custom
        } else {
            plain
        };
        skip_delta(dd, &mut br);
        if br.is_bad_read() {
            break;
        }
    }
    if br.is_bad_read() {
        return nom_fail("entities: read past the end of the message");
    }
    let (i, _) = take(br.get_consumed_bytes())(i)?;
    Ok((i, ()))
}

fn walk_client_data(i: &[u8], aux: AuxRefCell) -> Result<()> {
    let aux = aux.borrow();
    if aux.is_hltv {
        return Ok((i, ()));
    }
    let (Ok(client), Ok(weapon)) = (
        decoder(&aux, "clientdata_t\0"),
        decoder(&aux, "weapon_data_t\0"),
    ) else {
        return nom_fail("missing clientdata decoder");
    };
    let mut br = BitReader::new(i);
    if br.read_1_bit() {
        br.read_n_bit(8);
    }
    skip_delta(client, &mut br);
    while br.read_1_bit() {
        br.read_n_bit(6);
        skip_delta(weapon, &mut br);
    }
    if br.is_bad_read() {
        return nom_fail("SvcClientData: read past the end of the message");
    }
    let (i, _) = take(br.get_consumed_bytes())(i)?;
    Ok((i, ()))
}

impl NetMessage {
    /// [`NetMessage::parse_unless_sound`] that also drops the three entity
    /// and client-data messages.
    pub fn parse_tier4(
        i: &[u8],
        aux: AuxRefCell,
        flavour: Tier4Flavour,
    ) -> Result<Option<NetMessage>> {
        let Some((&type_, rest)) = i.split_first() else {
            return NetMessage::parse_unless_sound(i, aux);
        };
        match (type_, flavour) {
            (SVC_CLIENT_DATA, Tier4Flavour::DecodeAndDrop) => {
                let (i, m) = SvcClientData::parse(rest, aux)?;
                drop(m);
                Ok((i, None))
            }
            (SVC_PACKET_ENTITIES, Tier4Flavour::DecodeAndDrop) => {
                let (i, m) = SvcPacketEntities::parse(rest, aux)?;
                drop(m);
                Ok((i, None))
            }
            (SVC_DELTA_PACKET_ENTITIES, Tier4Flavour::DecodeAndDrop) => {
                let (i, m) = SvcDeltaPacketEntities::parse(rest, aux)?;
                drop(m);
                Ok((i, None))
            }
            (SVC_CLIENT_DATA, Tier4Flavour::BitWalk) => {
                let (i, ()) = walk_client_data(rest, aux)?;
                Ok((i, None))
            }
            (SVC_PACKET_ENTITIES, Tier4Flavour::BitWalk) => {
                let (i, ()) = walk_entities(rest, aux, false)?;
                Ok((i, None))
            }
            (SVC_DELTA_PACKET_ENTITIES, Tier4Flavour::BitWalk) => {
                let (i, ()) = walk_entities(rest, aux, true)?;
                Ok((i, None))
            }
            _ => NetMessage::parse_unless_sound(i, aux),
        }
    }
}
