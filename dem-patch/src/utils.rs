use bitvec::field::BitField;
use types::BitSlice;

use self::types::{DeltaDecoderS, DeltaType};

use super::*;

pub fn get_initial_delta() -> DeltaDecoderTable {
    let mut res: DeltaDecoderTable = DeltaDecoderTable::new();

    let e1 = DeltaDecoderS::new("flags".into(), 32, 1., DeltaType::Integer as u32);
    let e2 = DeltaDecoderS::new("name".into(), 8, 1., DeltaType::String as u32);
    let e3 = DeltaDecoderS::new("offset".into(), 16, 1., DeltaType::Integer as u32);
    let e4 = DeltaDecoderS::new("size".into(), 8, 1., DeltaType::Integer as u32);
    let e5 = DeltaDecoderS::new("bits".into(), 8, 1., DeltaType::Integer as u32);
    let e6 = DeltaDecoderS::new("divisor".into(), 32, 4000., DeltaType::Float as u32);
    let e7 = DeltaDecoderS::new("preMultiplier".into(), 32, 4000., DeltaType::Float as u32);

    let default_decoder = vec![e1, e2, e3, e4, e5, e6, e7];

    res.insert("delta_description_t\0".to_string(), default_decoder);

    res
}

pub fn bitslice_to_string(bitslice: &BitSlice) -> String {
    assert_eq!(bitslice.len() % 8, 0);

    let byte_count = bitslice.len() / 8;
    let mut res_string = String::new();

    for i in 0..byte_count {
        res_string += (bitslice[(i * 8)..((i + 1) * 8)].load::<u8>() as char)
            .to_string()
            .as_str()
    }

    res_string
}

#[cfg(test)]
mod test {
    use bitvec::{bitarr, order::Lsb0};

    use crate::utils::bitslice_to_string;

    #[test]
    fn bit_slice_to_string() {
        let v = bitarr![u8, Lsb0; 0, 1, 0, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let res = bitslice_to_string(v.as_bitslice());

        assert_eq!(res, "*38\0")
    }
}
