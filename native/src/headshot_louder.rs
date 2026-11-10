//! The louder headshot sounds `ds_sound_headshot_louder 1-3` plays (#1353):
//! three copies of the install's own `sound\player\helmet_hit.wav`, written
//! into `<game>_addon\sound\dodstudio\` at every launch, so no Valve audio
//! ships with Studio and a mod with its own headshot sound gets that one
//! made louder.
//!
//! The engine cannot play a sound above full volume, so louder means a
//! louder file. The steps, with the change measured on DoD's stock sound
//! (RMS -14.7 dBFS):
//!
//! | Step | File | How | RMS change |
//! |---|---|---|---|
//! | 1 | `helmet_hit_louder1.wav` | 2.13x through a look-ahead limiter | +2.5 dB |
//! | 2 | `helmet_hit_louder2.wav` | 6x through the limiter | +3.8 dB |
//! | 3 | `helmet_hit_louder3.wav` | 3x, hard-clipped | +7.2 dB |
//!
//! The original already peaks at 0 dBFS, so any gain must be limited or
//! clipped. The limiter keeps the hit clean; most of step 2's extra gain
//! lands on the quieter tail, so its second knock comes up close to the
//! first. Step 3 is the 3x hard-clipped copy players already swap into
//! `dod\sound\player\` by hand: the loudest, with the crunch that comes with
//! it. An install whose own file is such a copy gets these made from that,
//! so louder on top of it.
//!
//! The game's own files are never written; these are Studio's names in
//! Studio's own folder, rewritten only when the source changed. The hook
//! (`goldsrc-hooks/src/headshot_sound.rs`) plays them by the same names and
//! falls back to the game's sound when one is missing.

use std::path::{Path, PathBuf};

/// Where the copies go, under `<game>_addon\sound\`, and the name the hook
/// plays them by.
pub const DIR: &str = "dodstudio";
/// Step 1, 2 and 3's file names, in [`DIR`].
pub const FILES: [&str; 3] = [
    "helmet_hit_louder1.wav",
    "helmet_hit_louder2.wav",
    "helmet_hit_louder3.wav",
];

/// Where the limiter holds peaks: -0.3 dBFS.
const CEILING: f64 = 0.966_050_878_007_027;
/// How far ahead the limiter starts turning down, in seconds.
const LOOK_AHEAD: f64 = 0.0015;
/// How fast it lets the gain back up, in seconds.
const RELEASE: f64 = 0.06;

/// One step's processing.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Louder {
    Limited(f64),
    Clipped(f64),
}

const STEPS: [Louder; 3] = [
    Louder::Limited(2.13),
    Louder::Limited(6.0),
    Louder::Clipped(3.0),
];

/// A PCM `.wav`'s format and its samples as -1..1, channels interleaved.
#[derive(Debug, Clone, PartialEq)]
struct Pcm {
    channels: u16,
    rate: u32,
    samples: Vec<f64>,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Reads 8- or 16-bit PCM, the two formats GoldSrc plays.
fn read_wav(bytes: &[u8]) -> Result<Pcm, String> {
    use crate::messages::headshot_louder as msg;
    if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err(msg::NOT_A_WAV.to_string());
    }
    let (mut format, mut data) = (None, None);
    let mut at = 12;
    while let (Some(id), Some(len)) = (bytes.get(at..at + 4), u32_at(bytes, at + 4)) {
        let body = at + 8;
        let end = body.saturating_add(len as usize).min(bytes.len());
        match id {
            b"fmt " => format = Some(&bytes[body..end]),
            b"data" => data = Some(&bytes[body..end]),
            _ => {}
        }
        // Chunks are padded to an even length.
        at = body.saturating_add(len as usize + (len as usize & 1));
    }
    let format = format.ok_or(msg::NO_FORMAT)?;
    let data = data.ok_or(msg::NO_DATA)?;
    let (Some(tag), Some(channels), Some(rate), Some(bits)) = (
        u16_at(format, 0),
        u16_at(format, 2),
        u32_at(format, 4),
        u16_at(format, 14),
    ) else {
        return Err(msg::NO_FORMAT.to_string());
    };
    if tag != 1 || channels == 0 {
        return Err(msg::not_pcm(tag, channels, bits));
    }
    let samples = match bits {
        8 => data
            .iter()
            .map(|&b| (f64::from(b) - 128.0) / 128.0)
            .collect(),
        16 => data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&s| f64::from(i16::from_le_bytes(s)) / 32768.0)
            .collect(),
        _ => return Err(msg::not_pcm(tag, channels, bits)),
    };
    Ok(Pcm {
        channels,
        rate,
        samples,
    })
}

/// 16-bit PCM, whatever the source's depth, with a plain 44-byte header.
fn write_wav(pcm: &Pcm) -> Vec<u8> {
    let data_len = pcm.samples.len() as u32 * 2;
    let block = pcm.channels * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&pcm.channels.to_le_bytes());
    out.extend_from_slice(&pcm.rate.to_le_bytes());
    out.extend_from_slice(&(pcm.rate * u32::from(block)).to_le_bytes());
    out.extend_from_slice(&block.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in &pcm.samples {
        let v = (s * 32767.0).round().clamp(-32768.0, 32767.0) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// `samples` times `gain`, held under [`CEILING`]: each sample's needed gain
/// is taken over the look-ahead window ahead of it, applied at once on the
/// way down and eased back up.
fn limited(samples: &[f64], rate: u32, channels: u16, gain: f64) -> Vec<f64> {
    let per_second = f64::from(rate) * f64::from(channels);
    let look = (LOOK_AHEAD * per_second) as usize;
    let release = (-1.0 / (RELEASE * per_second)).exp();
    let need: Vec<f64> = samples
        .iter()
        .map(|s| (CEILING / (s * gain).abs().max(1e-12)).min(1.0))
        .collect();
    let mut current = 1.0f64;
    samples
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let end = (i + look + 1).min(need.len());
            let target = need[i..end].iter().copied().fold(1.0, f64::min);
            current = if target < current {
                target
            } else {
                target + (current - target) * release
            };
            (s * gain * current).clamp(-CEILING, CEILING)
        })
        .collect()
}

fn louder(pcm: &Pcm, step: Louder) -> Pcm {
    let samples = match step {
        Louder::Limited(gain) => limited(&pcm.samples, pcm.rate, pcm.channels, gain),
        Louder::Clipped(gain) => pcm
            .samples
            .iter()
            .map(|s| (s * gain).clamp(-1.0, 32767.0 / 32768.0))
            .collect(),
    };
    Pcm { samples, ..*pcm }
}

/// The three files' bytes, step 1 first, from the game's sound.
fn build(source: &[u8]) -> Result<[Vec<u8>; 3], String> {
    let pcm = read_wav(source)?;
    Ok(STEPS.map(|step| write_wav(&louder(&pcm, step))))
}

/// `<game_root>\<game_folder>\sound\player\helmet_hit.wav`: the install's own.
pub fn source_in(game_root: &Path, game_folder: &str) -> PathBuf {
    game_root
        .join(game_folder)
        .join("sound")
        .join("player")
        .join("helmet_hit.wav")
}

/// `<game_root>\<game_folder>_addon\sound\dodstudio\`.
pub fn target_dir(game_root: &Path, game_folder: &str) -> PathBuf {
    game_root
        .join(format!("{game_folder}_addon"))
        .join("sound")
        .join(DIR)
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// This many of the three were (re)written.
    Written {
        count: usize,
        dir: PathBuf,
    },
    Unchanged,
}

/// Writes the three steps for the game at `game_root` (`hl.exe`'s folder),
/// leaving any already right alone.
pub fn install(game_root: &Path, game_folder: &str) -> Result<Outcome, String> {
    use crate::messages::headshot_louder as msg;
    let source = source_in(game_root, game_folder);
    let bytes = std::fs::read(&source).map_err(|e| msg::could_not_read(source.display(), e))?;
    let built = build(&bytes).map_err(|why| msg::unusable(source.display(), why))?;
    let dir = target_dir(game_root, game_folder);
    let mut count = 0;
    for (name, body) in FILES.iter().zip(built) {
        let path = dir.join(name);
        if std::fs::read(&path).is_ok_and(|now| now == body) {
            continue;
        }
        std::fs::create_dir_all(&dir).map_err(|e| msg::could_not_create(dir.display(), e))?;
        std::fs::write(&path, body).map_err(|e| msg::could_not_write(path.display(), e))?;
        count += 1;
    }
    Ok(if count == 0 {
        Outcome::Unchanged
    } else {
        Outcome::Written { count, dir }
    })
}

/// What every launch does before the game starts, beside the demo bar's
/// layout: [`install`], with one activity-log line when files were written
/// or could not be. An empty `hl_exe` does nothing.
pub fn prepare(hl_exe: &str, game_folder: &str) {
    let Some(game_root) = Path::new(hl_exe)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    else {
        return;
    };
    let started = std::time::Instant::now();
    match install(game_root, game_folder) {
        Ok(Outcome::Written { count, dir }) => crate::log_markdown(
            &crate::messages::headshot_louder::written(count, dir.display(), started.elapsed()),
        ),
        Ok(Outcome::Unchanged) => {}
        Err(why) => crate::log_markdown(&crate::messages::headshot_louder::skipped(why)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    fn rms(s: &[f64]) -> f64 {
        (s.iter().map(|v| v * v).sum::<f64>() / s.len() as f64).sqrt()
    }

    /// A decaying knock that peaks at full scale, like the game's sound.
    fn knock(rate: u32) -> Pcm {
        let samples = (0..rate / 2)
            .map(|i| {
                let t = f64::from(i) / f64::from(rate);
                (t * 900.0 * std::f64::consts::TAU).sin() * (-t * 12.0).exp() * 32767.0 / 32768.0
            })
            .collect();
        Pcm {
            channels: 1,
            rate,
            samples,
        }
    }

    #[test]
    fn a_wav_round_trips() {
        let pcm = knock(22050);
        let back = read_wav(&write_wav(&pcm)).unwrap();
        assert_eq!((back.channels, back.rate), (1, 22050));
        assert_eq!(back.samples.len(), pcm.samples.len());
        assert!(
            back.samples
                .iter()
                .zip(&pcm.samples)
                .all(|(a, b)| (a - b).abs() < 1.0 / 16384.0)
        );
    }

    #[test]
    fn other_chunks_and_8_bit_are_read() {
        let mut wav = write_wav(&Pcm {
            channels: 1,
            rate: 11025,
            samples: vec![0.5; 4],
        });
        // A LIST chunk with an odd length (padded) before the data, as the
        // game's own file has one.
        let list = b"LIST\x03\x00\x00\x00abc\x00";
        wav.splice(36..36, list.iter().copied());
        assert_eq!(read_wav(&wav).unwrap().samples.len(), 4);

        let mut eight = write_wav(&Pcm {
            channels: 1,
            rate: 11025,
            samples: vec![],
        });
        eight[34] = 8; // bits per sample
        eight.truncate(40);
        eight.extend_from_slice(&2u32.to_le_bytes());
        eight.extend_from_slice(&[128, 255]);
        let pcm = read_wav(&eight).unwrap();
        assert_eq!(pcm.samples, vec![0.0, 127.0 / 128.0]);
    }

    #[test]
    fn not_a_pcm_wav_is_refused() {
        assert!(read_wav(b"not a wav at all").is_err());
        let mut adpcm = write_wav(&knock(22050));
        adpcm[20] = 2;
        assert!(read_wav(&adpcm).is_err());
    }

    #[test]
    fn each_step_is_louder_and_the_limited_ones_never_clip() {
        let pcm = knock(22050);
        let out: Vec<Pcm> = STEPS.iter().map(|&s| louder(&pcm, s)).collect();
        let mut last = rms(&pcm.samples);
        for step in &out {
            assert!(rms(&step.samples) > last * 1.1);
            last = rms(&step.samples);
        }
        for step in &out[..2] {
            assert!(step.samples.iter().all(|s| s.abs() <= CEILING));
        }
        // Step 3 clips: many samples sit at full scale.
        assert!(out[2].samples.iter().filter(|s| s.abs() >= 0.999).count() > 10);
    }

    #[test]
    fn install_writes_the_three_once() {
        let root = Scratch::new("headshot_louder");
        let source = source_in(&root, "dod");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, write_wav(&knock(22050))).unwrap();

        let dir = target_dir(&root, "dod");
        assert_eq!(
            install(&root, "dod").unwrap(),
            Outcome::Written {
                count: 3,
                dir: dir.clone()
            }
        );
        for name in FILES {
            assert!(read_wav(&std::fs::read(dir.join(name)).unwrap()).is_ok());
        }
        assert_eq!(install(&root, "dod").unwrap(), Outcome::Unchanged);
        // The game's own file is untouched.
        assert_eq!(std::fs::read(&source).unwrap(), write_wav(&knock(22050)));
    }

    #[test]
    fn no_game_sound_is_an_error_and_writes_nothing() {
        let root = Scratch::new("headshot_louder_none");
        assert!(install(&root, "dod").is_err());
        assert!(!target_dir(&root, "dod").exists());
    }

    /// The hook plays these by name; a rename on one side only would play
    /// the game's sound with no error.
    #[test]
    fn the_hook_plays_the_same_names() {
        let hook = include_str!("../../goldsrc-hooks/src/headshot_sound.rs");
        for name in FILES {
            assert!(hook.contains(&format!("{DIR}/{name}")), "{name}");
        }
    }
}
