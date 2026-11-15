use crate::bit::BitSlice;
use crate::types::OriginCoord;

use crate::nom_helper::nom_fail;

use super::*;

/// An `svc_sound`'s fields as read, borrowed from the message's bytes. The
/// one place its bit layout is written down: [`SvcSound::parse`] copies the
/// fields out, [`SvcSound::skip`] only needs to know where the message ends.
struct SoundBits<'a> {
    flags: &'a BitSlice,
    volume: Option<&'a BitSlice>,
    attenuation: Option<&'a BitSlice>,
    channel: &'a BitSlice,
    entity_index: &'a BitSlice,
    sound_index_long: Option<&'a BitSlice>,
    sound_index_short: Option<&'a BitSlice>,
    has_x: bool,
    has_y: bool,
    has_z: bool,
    origin_x: Option<OriginBits<'a>>,
    origin_y: Option<OriginBits<'a>>,
    origin_z: Option<OriginBits<'a>>,
    /// `None` when the flags leave it out (the default pitch).
    pitch: Option<&'a BitSlice>,
}

struct OriginBits<'a> {
    int_flag: bool,
    fraction_flag: bool,
    is_negative: Option<bool>,
    int_value: Option<&'a BitSlice>,
    fraction_value: Option<&'a BitSlice>,
}

/// Reads one `svc_sound` from `i` (just past its type byte), returning the
/// bytes after it.
fn read_sound(i: &[u8]) -> Result<SoundBits<'_>> {
    let mut br = BitReader::new(i);

    let flags = br.read_n_bit(9);
    let flag_u = flags.to_u32();
    let volume = (flag_u & 1 != 0).then(|| br.read_n_bit(8));
    let attenuation = (flag_u & 2 != 0).then(|| br.read_n_bit(8));
    let channel = br.read_n_bit(3);
    let entity_index = br.read_n_bit(11);
    let (sound_index_long, sound_index_short) = if flag_u & 4 != 0 {
        (Some(br.read_n_bit(16)), None)
    } else {
        (None, Some(br.read_n_bit(8)))
    };
    let (has_x, has_y, has_z) = (br.read_1_bit(), br.read_1_bit(), br.read_1_bit());
    let origin_x = has_x.then(|| read_origin(&mut br));
    let origin_y = has_y.then(|| read_origin(&mut br));
    let origin_z = has_z.then(|| read_origin(&mut br));
    let pitch = (flag_u & 8 != 0).then(|| br.read_n_bit(8));

    // A read that ran past the end of this message's bytes means the
    // demo is malformed. Reject it here, where the caller's normal
    // parse-error path can skip the file -- the alternative is an
    // out-of-bounds index, and `panic = "abort"` makes that fatal to the
    // whole process rather than to this one demo. See #225.
    if br.is_bad_read() {
        return nom_fail("SvcSound: read past the end of the message");
    }

    let (i, _) = take(br.get_consumed_bytes())(i)?;

    Ok((
        i,
        SoundBits {
            flags,
            volume,
            attenuation,
            channel,
            entity_index,
            sound_index_long,
            sound_index_short,
            has_x,
            has_y,
            has_z,
            origin_x,
            origin_y,
            origin_z,
            pitch,
        },
    ))
}

impl SvcSound {
    /// Steps over one `svc_sound` (`i` starts just past its type byte)
    /// without building it, for a parse that keeps no sounds
    /// ([`crate::types::MessageDataParseMode::ParseWithoutSounds`]). Fails
    /// exactly where [`Doer::parse`] would.
    pub(crate) fn skip(i: &[u8]) -> Result<()> {
        let (i, _) = read_sound(i)?;
        Ok((i, ()))
    }
}

impl Doer for SvcSound {
    fn id(&self) -> u8 {
        6
    }

    fn parse(i: &[u8], _: AuxRefCell) -> Result<Self> {
        let (i, bits) = read_sound(i)?;

        Ok((
            i,
            Self {
                flags: bits.flags.to_owned(),
                volume: bits.volume.map(ToOwned::to_owned),
                attenuation: bits.attenuation.map(ToOwned::to_owned),
                channel: bits.channel.to_owned(),
                entity_index: bits.entity_index.to_owned(),
                sound_index_long: bits.sound_index_long.map(ToOwned::to_owned),
                sound_index_short: bits.sound_index_short.map(ToOwned::to_owned),
                has_x: bits.has_x,
                has_y: bits.has_y,
                has_z: bits.has_z,
                origin_x: bits.origin_x.map(OriginBits::into_coord),
                origin_y: bits.origin_y.map(OriginBits::into_coord),
                origin_z: bits.origin_z.map(OriginBits::into_coord),
                pitch: bits
                    .pitch
                    .map_or_else(|| BitVec::from_element(1u8), ToOwned::to_owned),
            },
        ))
    }

    fn write(&self, _: AuxRefCell) -> ByteVec {
        let mut writer = ByteWriter::new();

        writer.append_u8(self.id());

        let mut bw = BitWriter::new();

        let should_write_volume = self.flags.to_u16() & 1 != 0;
        let should_write_attenuation = self.flags.to_u16() & 2 != 0;
        let should_write_sound_index_long = self.flags.to_u16() & 4 != 0;
        let should_write_pitch = self.flags.to_u16() & 8 != 0;

        bw.append_vec(&self.flags);

        if should_write_volume {
            bw.append_vec(self.volume.as_ref().unwrap());
        }

        if should_write_attenuation {
            bw.append_vec(self.attenuation.as_ref().unwrap());
        }

        bw.append_vec(&self.channel);
        bw.append_vec(&self.entity_index);

        if should_write_sound_index_long {
            bw.append_vec(self.sound_index_long.as_ref().unwrap())
        } else {
            bw.append_vec(self.sound_index_short.as_ref().unwrap())
        }

        bw.append_bit(self.has_x);
        bw.append_bit(self.has_y);
        bw.append_bit(self.has_z);

        if self.has_x {
            write_origin(self.origin_x.as_ref().unwrap(), &mut bw);
        }
        if self.has_y {
            write_origin(self.origin_y.as_ref().unwrap(), &mut bw);
        }
        if self.has_z {
            write_origin(self.origin_z.as_ref().unwrap(), &mut bw);
        }

        if should_write_pitch {
            bw.append_vec(&self.pitch);
        }

        writer.append_u8_slice(bw.get_u8_vec().as_slice());

        writer.data
    }
}

fn read_origin<'a>(br: &mut BitReader<'a>) -> OriginBits<'a> {
    let int_flag = br.read_1_bit();
    let fraction_flag = br.read_1_bit();
    let is_negative = (int_flag || fraction_flag).then(|| br.read_1_bit());
    let int_value = int_flag.then(|| br.read_n_bit(12));
    let fraction_value = fraction_flag.then(|| br.read_n_bit(3));

    OriginBits {
        int_flag,
        fraction_flag,
        is_negative,
        int_value,
        fraction_value,
    }
}

impl OriginBits<'_> {
    fn into_coord(self) -> OriginCoord {
        OriginCoord {
            int_flag: self.int_flag,
            fraction_flag: self.fraction_flag,
            is_negative: self.is_negative,
            int_value: self.int_value.map(ToOwned::to_owned),
            fraction_value: self.fraction_value.map(ToOwned::to_owned),
        }
    }
}

fn write_origin(i: &OriginCoord, bw: &mut BitWriter) {
    bw.append_bit(i.int_flag);
    bw.append_bit(i.fraction_flag);

    if let Some(negative) = i.is_negative {
        bw.append_bit(negative);
    }

    if let Some(int) = &i.int_value {
        bw.append_vec(int);
    }

    if let Some(frac) = &i.fraction_value {
        bw.append_vec(frac);
    }

    // bw.append_vec(self.unknown);
}
