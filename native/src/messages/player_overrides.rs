// messages/player_overrides.rs
// Errors of the Players table the hook reads and of the crosshair sprite
// preview (`player_overrides.rs`, #237). Each function returns the text only.

use std::fmt::Display;

/// The table file could not be written.
pub fn could_not_write_players_table(path: impl Display, err: impl Display) -> String {
    format!("could not write the Players table to {path}: {err}")
}

/// A sprite file cut off before a field the decoder reads.
pub const SPRITE_ENDS_EARLY: &str = "the file ends early";

pub const SPRITE_NO_IDSP_HEADER: &str = "not a sprite (no IDSP header)";

pub const SPRITE_NOT_VERSION_2: &str = "not a version 2 (Half-Life) sprite";

pub const SPRITE_NO_FRAMES: &str = "the sprite has no frames";

pub const SPRITE_PALETTE_PAST_END: &str = "the palette runs past the end";

pub const SPRITE_FRAME_PAST_END: &str = "the frame runs past the end";

pub fn sprite_palette_colours(count: impl Display) -> String {
    format!("{count} palette colours")
}

pub fn sprite_frame_group(count: impl Display) -> String {
    format!("a frame group of {count}")
}

pub fn sprite_frame_size(width: impl Display, height: impl Display) -> String {
    format!("a {width}x{height} frame")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_overrides_text_matches_its_original_inline_text() {
        assert_eq!(
            could_not_write_players_table("D:/t.txt", "denied"),
            "could not write the Players table to D:/t.txt: denied"
        );
        assert_eq!(SPRITE_ENDS_EARLY, "the file ends early");
        assert_eq!(SPRITE_NO_IDSP_HEADER, "not a sprite (no IDSP header)");
        assert_eq!(SPRITE_NOT_VERSION_2, "not a version 2 (Half-Life) sprite");
        assert_eq!(SPRITE_NO_FRAMES, "the sprite has no frames");
        assert_eq!(SPRITE_PALETTE_PAST_END, "the palette runs past the end");
        assert_eq!(SPRITE_FRAME_PAST_END, "the frame runs past the end");
        assert_eq!(sprite_palette_colours(300), "300 palette colours");
        assert_eq!(sprite_frame_group(0), "a frame group of 0");
        assert_eq!(sprite_frame_size(2000, 3), "a 2000x3 frame");
    }
}
