// messages/headshot_louder.rs
// The louder headshot sounds written at every launch (`headshot_louder.rs`,
// #1353): errors and activity-log lines. Each function returns the text only.

use std::fmt::Display;
use std::time::Duration;

pub const NOT_A_WAV: &str = "not a RIFF WAVE file";
pub const NO_FORMAT: &str = "the file has no usable fmt chunk";
pub const NO_DATA: &str = "the file has no data chunk";

pub fn not_pcm(tag: u16, channels: u16, bits: u16) -> String {
    format!(
        "format {tag}, {channels} channel(s), {bits}-bit: only 8- or 16-bit PCM can be made louder"
    )
}

pub fn could_not_read(path: impl Display, err: impl Display) -> String {
    format!("could not read {path}: {err}")
}

pub fn unusable(path: impl Display, why: impl Display) -> String {
    format!("{path}: {why}")
}

pub fn could_not_create(path: impl Display, err: impl Display) -> String {
    format!("could not create {path}: {err}")
}

pub fn could_not_write(path: impl Display, err: impl Display) -> String {
    format!("could not write {path}: {err}")
}

pub fn written(count: usize, dir: impl Display, took: Duration) -> String {
    format!(
        "- Louder headshot sounds: wrote {count} of 3 into {dir} from the game's helmet_hit.wav in {} ms",
        took.as_millis()
    )
}

pub fn skipped(why: impl Display) -> String {
    format!(
        "- Louder headshot sounds: skipped, ds_sound_headshot_louder will play the game's own sound ({why})"
    )
}
