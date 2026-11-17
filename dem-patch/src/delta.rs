#![warn(clippy::unwrap_used, clippy::expect_used)]

use crate::{
    bit::{BitReader, BitSlice, BitSliceCast, BitWriter},
    types::{Delta, DeltaDecoder, DeltaDecoderS, DeltaType, DeltaValue},
    write_error::DeltaWriteError,
};

pub fn parse_delta(dd: &DeltaDecoder, br: &mut BitReader) -> Delta {
    // At most 7 mask bytes (3 bits of count), read up front onto the stack.
    let mask_byte_count = br.read_n_bit(3).to_u8() as usize;
    let mut mask_bytes = [0u8; 8];
    for mask_byte in mask_bytes.iter_mut().take(mask_byte_count) {
        *mask_byte = br.read_n_bit(8).to_u8();
    }
    let set_fields: u32 = mask_bytes.iter().map(|b| b.count_ones()).sum();
    let mut res = Delta::with_capacity((set_fields as usize).min(dd.len()));

    for (i, mask_byte) in mask_bytes.iter().enumerate().take(mask_byte_count) {
        for j in 0..8 {
            let index = j + i * 8;

            if index == dd.len() {
                return res;
            }

            if (mask_byte & (1 << j)) != 0 {
                let description = &dd[index];
                let value = parse_delta_field(description, br);
                // `insert`, not a plain push: a corrupted description can
                // name two fields alike, and the later one wins, as it did
                // when this was a hash map.
                res.insert(description.key.clone(), value);
            }
        }
    }

    res
}

macro_rules! flag {
    ($lhs:expr, $rhs:expr) => {{ $lhs as u32 & $rhs as u32 != 0 }};
}

/// `bits` and `divisor` both come off disk. `bits` of 0 makes the signed paths
/// below compute `0 - 1`, and `divisor` of 0 makes the integer paths divide by
/// zero -- each an abort under `panic = "abort"`. Neither occurs in a valid
/// delta description. See #225.
fn signed_bits(description: &DeltaDecoderS) -> usize {
    (description.bits as usize).saturating_sub(1)
}

/// Integer division that yields 0 instead of aborting when `divisor` is 0.
macro_rules! div_or_zero {
    ($value:expr, $divisor:expr) => {
        $value.checked_div($divisor).unwrap_or(0)
    };
}

fn parse_delta_field(description: &DeltaDecoderS, br: &mut BitReader) -> DeltaValue {
    let lhs = description.flags;

    let is_signed = flag!(lhs, DeltaType::Signed);
    let is_byte = flag!(lhs, DeltaType::Byte);
    let is_short = flag!(lhs, DeltaType::Short);
    let is_integer = flag!(lhs, DeltaType::Integer);
    let is_some_float = flag!(lhs, DeltaType::Float)
        || flag!(lhs, DeltaType::TimeWindow8)
        || flag!(lhs, DeltaType::TimeWindowBig);
    let is_angle = flag!(lhs, DeltaType::Angle);
    let is_string = flag!(lhs, DeltaType::String);

    if is_byte {
        if is_signed {
            let sign = if br.read_1_bit() { -1 } else { 1 };
            let value = br.read_n_bit(signed_bits(description)).to_u8();
            let res_value =
                div_or_zero!(sign * value as i8, description.divisor as i8).to_le_bytes();
            res_value.into()
        } else {
            let value = (br.read_n_bit(description.bits as usize)).to_u8();
            let res_value = div_or_zero!(value, description.divisor as u8).to_le_bytes();
            res_value.into()
        }
    } else if is_short {
        if is_signed {
            let sign = if br.read_1_bit() { -1 } else { 1 };
            let value = (br.read_n_bit(signed_bits(description))).to_u16();
            let res_value =
                div_or_zero!(sign * value as i16, description.divisor as i16).to_le_bytes();
            res_value.into()
        } else {
            let value = (br.read_n_bit(description.bits as usize)).to_u16();
            let res_value = div_or_zero!(value, description.divisor as u16).to_le_bytes();
            res_value.into()
        }
    } else if is_integer {
        if is_signed {
            let sign = if br.read_1_bit() { -1 } else { 1 };
            let value = (br.read_n_bit(signed_bits(description))).to_u32();
            let res_value =
                div_or_zero!(sign * value as i32, description.divisor as i32).to_le_bytes();
            res_value.into()
        } else {
            let value = (br.read_n_bit(description.bits as usize)).to_u32();
            let res_value = div_or_zero!(value, description.divisor as u32).to_le_bytes();
            res_value.into()
        }
    } else if is_some_float {
        if is_signed {
            let sign = if br.read_1_bit() { -1 } else { 1 };
            let value = (br.read_n_bit(signed_bits(description))).to_u32();
            let res_value = (((sign * value as i32) as f32) / (description.divisor)).to_le_bytes();
            res_value.into()
        } else {
            let value = (br.read_n_bit(description.bits as usize)).to_u32();
            let res_value = ((value as f32) / (description.divisor)).to_le_bytes();
            res_value.into()
        }
    } else if is_angle {
        let value = (br.read_n_bit(description.bits as usize)).to_u32();
        // `1 << bits` overflows for a `bits` of 32 or more, which a corrupted
        // description can carry. #225.
        let multiplier = 360f32 / (1u32.checked_shl(description.bits).unwrap_or(u32::MAX) as f32);
        let res_value = (value as f32 * multiplier).to_le_bytes();
        res_value.into()
    } else if is_string {
        bitslice_to_u8_vec(br.read_string()).into()
    } else {
        // Was `unreachable!`. It is reachable: `flags` is read off disk, and a
        // corrupted description can name no type at all. Flag the read so the
        // caller rejects the message rather than aborting the process. #225.
        br.flag_bad_read();
        DeltaValue::default()
    }
}

fn bitslice_to_u8_vec(i: &BitSlice) -> Vec<u8> {
    i.chunks(8).map(|chunk| chunk.to_u8()).collect()
}

/// The most fields a delta's mask can address: `parse_delta` reads at most 7
/// mask bytes (a 3-bit count).
const MASK_FIELDS: usize = 7 * 8;

/// Encodes `delta` with `delta_decoder`, the table it was read with.
///
/// Fields are matched by `DeltaDecoderS::key`, the name `parse_delta` files
/// them under, so a field whose name is not UTF-8 is written back like any
/// other. Upstream matched the raw name and unwrapped, which aborted on such a
/// name, on a field the table lacks and on a field narrower than its value.
/// On an error `bw` holds part of the delta: the message is not written. #1510.
pub fn write_delta(
    delta: &Delta,
    delta_decoder: &DeltaDecoder,
    bw: &mut BitWriter,
) -> Result<(), DeltaWriteError> {
    // Consider this like a modulo.
    // Delta with description of index 13 is byte_mask[13 / 8] at 13 % 8.
    // Byte mask count adds accordingly if we have entry with biggest index number.
    let mut byte_mask = [0u8; 8];
    let mut byte_mask_count = 0u8;
    let mut described = 0usize;

    // This step marks which delta field will be encoded.
    for (index, description) in delta_decoder.iter().enumerate() {
        if !delta.contains_key(&description.key) {
            continue;
        }
        if index >= MASK_FIELDS {
            return Err(DeltaWriteError::FieldOutOfMask {
                field: description.key.to_string(),
                index,
            });
        }
        let quotient = index / 8;
        let remainder = index % 8;

        byte_mask[quotient] |= 1 << remainder;
        byte_mask_count = byte_mask_count.max(quotient as u8 + 1);
        described += 1;
    }

    // Every field once: a field the table lacks would be dropped, and one it
    // describes twice would be written twice under one mask bit.
    if described != delta.len() {
        return Err(undescribed_field(delta, delta_decoder));
    }

    bw.append_u32_range(byte_mask_count as u32, 3);
    for i in 0..byte_mask_count {
        bw.append_u8(byte_mask[i as usize]);
    }

    // We have to write delta by the described order.
    for description in delta_decoder {
        if let Some(value) = delta.get(&description.key) {
            write_delta_field(description, value, bw)?;
        }
    }

    Ok(())
}

/// Which of `delta`'s fields `delta_decoder` does not describe exactly once.
fn undescribed_field(delta: &Delta, delta_decoder: &DeltaDecoder) -> DeltaWriteError {
    for key in delta.keys() {
        match delta_decoder.iter().filter(|d| d.key == *key).count() {
            0 => return DeltaWriteError::UnknownField(key.to_string()),
            1 => {}
            _ => return DeltaWriteError::DuplicateField(key.to_string()),
        }
    }
    // Unreachable while `Delta` keeps its keys unique; say so rather than
    // pretend a field was found.
    DeltaWriteError::UnknownField(String::new())
}

/// The first `N` bytes of `value`, or the error naming the field.
fn leading<const N: usize>(
    description: &DeltaDecoderS,
    value: &[u8],
) -> Result<[u8; N], DeltaWriteError> {
    value
        .get(..N)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| DeltaWriteError::ValueTooShort {
            field: description.key.to_string(),
            have: value.len(),
            need: N,
        })
}

/// The arithmetic below wraps where it overflows, as it always has in a
/// release build; `wrapping_` only spells that out, so a debug build (the
/// tests) writes the same bytes instead of panicking on a value off disk.
fn write_delta_field(
    description: &DeltaDecoderS,
    value: &[u8],
    bw: &mut BitWriter,
) -> Result<(), DeltaWriteError> {
    let lhs = description.flags;

    let is_signed = flag!(lhs, DeltaType::Signed);
    let is_byte = flag!(lhs, DeltaType::Byte);
    let is_short = flag!(lhs, DeltaType::Short);
    let is_integer = flag!(lhs, DeltaType::Integer);
    let is_angle = flag!(lhs, DeltaType::Angle);
    let is_some_float = flag!(lhs, DeltaType::Float)
        || flag!(lhs, DeltaType::TimeWindow8)
        || flag!(lhs, DeltaType::TimeWindowBig);
    let is_string = flag!(lhs, DeltaType::String);

    // `append_u32_range` slices a 32-bit value, and a signed field spends one
    // of its bits on the sign. `bits` is read off disk. #1510.
    let is_number = is_byte || is_short || is_integer || is_some_float || is_angle;
    if is_number && (description.bits > 32 || (is_signed && description.bits == 0)) {
        return Err(DeltaWriteError::BadWidth {
            field: description.key.to_string(),
            bits: description.bits,
        });
    }

    if is_byte {
        let bytes: [u8; 1] = leading(description, value)?;
        if is_signed {
            let res_value = i8::from_le_bytes(bytes);
            let signed_value = res_value.wrapping_mul(description.divisor as i8);
            let is_negative = signed_value < 0;

            let value = if is_negative {
                bw.append_bit(true);
                signed_value.wrapping_neg()
            } else {
                bw.append_bit(false);
                signed_value
            };

            // value is positive so cast unsigned without side effects.
            bw.append_u32_range(value as u32, description.bits - 1);
        } else {
            let res_value = u8::from_le_bytes(bytes);
            let value = res_value.wrapping_mul(description.divisor as u8);

            bw.append_u32_range(value as u32, description.bits);
        }
    } else if is_short {
        let bytes: [u8; 2] = leading(description, value)?;
        if is_signed {
            let res_value = i16::from_le_bytes(bytes);
            let signed_value = res_value.wrapping_mul(description.divisor as i16);
            let is_negative = signed_value < 0;

            let value = if is_negative {
                bw.append_bit(true);
                signed_value.wrapping_neg()
            } else {
                bw.append_bit(false);
                signed_value
            };

            bw.append_u32_range(value as u32, description.bits - 1);
        } else {
            let res_value = u16::from_le_bytes(bytes);
            let value = res_value.wrapping_mul(description.divisor as u16);

            bw.append_u32_range(value as u32, description.bits);
        }
    } else if is_integer {
        let bytes: [u8; 4] = leading(description, value)?;
        if is_signed {
            let res_value = i32::from_le_bytes(bytes);
            let signed_value = res_value.wrapping_mul(description.divisor as i32);
            let is_negative = signed_value < 0;

            let value = if is_negative {
                bw.append_bit(true);
                signed_value.wrapping_neg()
            } else {
                bw.append_bit(false);
                signed_value
            };

            bw.append_u32_range(value as u32, description.bits - 1);
        } else {
            let res_value = u32::from_le_bytes(bytes);
            let value = res_value.wrapping_mul(description.divisor as u32);

            bw.append_u32_range(value, description.bits);
        }
    } else if is_some_float {
        let bytes: [u8; 4] = leading(description, value)?;
        if is_signed {
            let res_value = f32::from_le_bytes(bytes);
            let signed_value = res_value * description.divisor;

            let value = if signed_value.is_sign_negative() {
                bw.append_bit(true);
                -signed_value
            } else {
                bw.append_bit(false);
                signed_value
            };

            bw.append_u32_range(value.round() as u32, description.bits - 1);
        } else {
            let res_value = f32::from_le_bytes(bytes);
            let value = res_value * description.divisor;

            bw.append_u32_range(value.round() as u32, description.bits);
        }
    } else if is_angle {
        // Quick hack. Angle is i16 so here it is.
        let bytes: [u8; 4] = leading(description, value)?;
        let res_value = f32::from_le_bytes(bytes);
        let multiplier = 360f32 / 1i32.wrapping_shl(description.bits) as f32;
        let value = (res_value / multiplier).round() as u32;
        bw.append_u32_range(value, description.bits);
    } else if is_string {
        for c in value {
            bw.append_u8(*c);
        }
    } else {
        // Was `unreachable!`: `flags` is read off disk, and `parse_delta`
        // only flags such a field as a bad read. #1510.
        return Err(DeltaWriteError::NoType(description.key.to_string()));
    }

    Ok(())
}

/// Each delta a table cannot encode is an error naming the field, where it
/// was an `unwrap` or a slice out of bounds. #1510.
#[cfg(test)]
mod write_tests {
    use super::*;
    use crate::types::{Delta, DeltaDecoderS, DeltaType};

    const BYTE: u32 = DeltaType::Byte as u32;
    const INTEGER: u32 = DeltaType::Integer as u32;
    const SIGNED: u32 = DeltaType::Signed as u32;

    fn field(name: &[u8], bits: u32, flags: u32) -> DeltaDecoderS {
        DeltaDecoderS::new(name.to_vec(), bits, 1.0, flags)
    }

    fn write(delta: &Delta, table: &DeltaDecoder) -> Result<BitWriter, DeltaWriteError> {
        let mut bw = BitWriter::new();
        write_delta(delta, table, &mut bw).map(|()| bw)
    }

    /// Reads back what `write` wrote.
    fn read_back(bw: &mut BitWriter, table: &DeltaDecoder) -> Delta {
        let bytes = bw.get_u8_vec();
        parse_delta(table, &mut BitReader::new(&bytes))
    }

    #[test]
    fn a_field_whose_name_is_not_utf8_is_written_back() {
        let table = vec![
            field(b"or\xffigin\0", 8, BYTE),
            field(b"angles\0", 16, INTEGER),
        ];
        let mut delta = Delta::new();
        delta.insert(table[0].key.clone(), [7u8].as_slice());
        delta.insert("angles\0", 300u32.to_le_bytes().as_slice());

        let mut bw = write(&delta, &table).expect("both fields are described");
        assert_eq!(read_back(&mut bw, &table), delta);
    }

    #[test]
    fn a_field_the_table_lacks_is_an_error() {
        let table = vec![field(b"angles\0", 16, INTEGER)];
        let mut delta = Delta::new();
        delta.insert("origin[0]\0", 1u32.to_le_bytes().as_slice());
        assert_eq!(
            write(&delta, &table).err(),
            Some(DeltaWriteError::UnknownField("origin[0]\0".into()))
        );
    }

    /// The parse keeps the later of two alike-named fields; here that is the
    /// byte, so the integer before it would have sliced 4 bytes out of 1.
    #[test]
    fn a_field_described_twice_is_an_error() {
        let table = vec![field(b"frame\0", 32, INTEGER), field(b"frame\0", 8, BYTE)];
        let mut delta = Delta::new();
        delta.insert("frame\0", [3u8].as_slice());
        assert_eq!(
            write(&delta, &table).err(),
            Some(DeltaWriteError::DuplicateField("frame\0".into()))
        );
    }

    /// A table changed under a delta: the field is an integer now.
    #[test]
    fn a_value_narrower_than_its_type_is_an_error() {
        let table = vec![field(b"frame\0", 32, INTEGER)];
        let mut delta = Delta::new();
        delta.insert("frame\0", [3u8].as_slice());
        assert_eq!(
            write(&delta, &table).err(),
            Some(DeltaWriteError::ValueTooShort {
                field: "frame\0".into(),
                have: 1,
                need: 4
            })
        );
    }

    #[test]
    fn a_width_that_cannot_be_written_is_an_error() {
        let mut delta = Delta::new();
        delta.insert("frame\0", 3u32.to_le_bytes().as_slice());
        for (bits, flags) in [(40, INTEGER), (0, INTEGER | SIGNED)] {
            let table = vec![field(b"frame\0", bits, flags)];
            assert_eq!(
                write(&delta, &table).err(),
                Some(DeltaWriteError::BadWidth {
                    field: "frame\0".into(),
                    bits
                }),
                "{bits} bits"
            );
        }
    }

    #[test]
    fn a_field_with_no_type_is_an_error() {
        let table = vec![field(b"frame\0", 8, 0)];
        let mut delta = Delta::new();
        delta.insert("frame\0", [3u8].as_slice());
        assert_eq!(
            write(&delta, &table).err(),
            Some(DeltaWriteError::NoType("frame\0".into()))
        );
    }

    /// Upstream indexed an 8-byte mask with it: field 64 was out of bounds.
    #[test]
    fn a_field_past_the_mask_is_an_error() {
        let table: DeltaDecoder = (0..65)
            .map(|i| field(format!("f{i}\0").as_bytes(), 8, BYTE))
            .collect();
        let mut delta = Delta::new();
        delta.insert("f64\0", [1u8].as_slice());
        assert_eq!(
            write(&delta, &table).err(),
            Some(DeltaWriteError::FieldOutOfMask {
                field: "f64\0".into(),
                index: 64
            })
        );
    }
}
