use crate::types::PingS;

use crate::nom_helper::nom_fail;

use super::*;

const SLOT_BITS: usize = 5;
const PING_BITS: usize = 12;
const LOSS_BITS: usize = 7;

impl Doer for SvcPings {
    fn id(&self) -> u8 {
        17
    }

    fn parse(i: &[u8], _: AuxRefCell) -> Result<Self> {
        let mut br = BitReader::new(i);
        let mut pings: Vec<PingS> = vec![];

        // Field widths are `SV_EmitPings`'s: slot 5, ping 12, loss 7 bits. An
        // 8+8+8 read takes the same 24 bits per entry, so it stays in step
        // but splits them at the wrong places (#1031).
        while br.read_1_bit() {
            pings.push(PingS {
                has_ping_data: true,
                player_id: Some(br.read_n_bit(SLOT_BITS).to_u8()),
                ping: Some(br.read_n_bit(PING_BITS).to_u16()),
                loss: Some(br.read_n_bit(LOSS_BITS).to_u8()),
            })
        }

        // If we exit the loop, it means we already read the has_ping_data = false bit.

        // Last element.
        pings.push(PingS {
            has_ping_data: false,
            player_id: None,
            ping: None,
            loss: None,
        });

        // Don't forget
        // A read that ran past the end of this message's bytes means the
        // demo is malformed. Reject it here, where the caller's normal
        // parse-error path can skip the file -- the alternative is an
        // out-of-bounds index, and `panic = "abort"` makes that fatal to the
        // whole process rather than to this one demo. See #225.
        if br.is_bad_read() {
            return nom_fail("SvcPings: read past the end of the message");
        }

        let (i, _) = take(br.get_consumed_bytes())(i)?;

        Ok((i, SvcPings { pings }))
    }

    fn write(&self, _: AuxRefCell) -> ByteVec {
        let mut writer = ByteWriter::new();

        writer.append_u8(self.id());

        let mut bw = BitWriter::new();

        for ping in &self.pings {
            if ping.has_ping_data {
                bw.append_bit(true);
                bw.append_u32_range(ping.player_id.unwrap().into(), SLOT_BITS as u32);
                bw.append_u32_range(ping.ping.unwrap().into(), PING_BITS as u32);
                bw.append_u32_range(ping.loss.unwrap().into(), LOSS_BITS as u32);
            } else {
                bw.append_bit(false);
            }
        }

        writer.append_u8_slice(&bw.get_u8_vec());

        writer.data
    }
}
