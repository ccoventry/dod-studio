// messages/bsp.rs
// Why a map file is refused (`patch/bsp.rs`, #237). Each function returns the
// text only.

use std::fmt::Display;

pub fn truncated_at(at: impl Display) -> String {
    format!("truncated at byte {at}")
}

pub fn lump_past_end(index: impl Display) -> String {
    format!("lump {index} runs past the end of the file")
}

pub const SHORTER_THAN_HEADER: &str = "file is shorter than a BSP header";

pub fn not_goldsrc_version(version: impl Display, wanted: impl Display) -> String {
    format!("BSP version {version} is not the GoldSrc version {wanted}")
}

pub fn could_not_read(path: impl Display, err: impl Display) -> String {
    format!("could not read {path}: {err}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bsp_text_matches_its_original_inline_text() {
        assert_eq!(truncated_at(12), "truncated at byte 12");
        assert_eq!(lump_past_end(3), "lump 3 runs past the end of the file");
        assert_eq!(SHORTER_THAN_HEADER, "file is shorter than a BSP header");
        assert_eq!(
            not_goldsrc_version(29, 30),
            "BSP version 29 is not the GoldSrc version 30"
        );
        assert_eq!(could_not_read("m.bsp", "e"), "could not read m.bsp: e");
    }
}
