//! What an AGR take (#450) holds once its batch has ended: whether its
//! `.agr` recorded anything, and the game's sound beside it.
//!
//! AGR mode records the sound with HLAE's movie recorder, picture discarded
//! (`builder::helper_cfg`'s `AGR_SOUND_LINES`), so HLAE leaves it where any
//! take's goes: `<block>/take0000/sound.wav`, beside an empty stream folder.
//! [`place_take_sound`] moves it beside the `.agr`, under the `.agr`'s name,
//! so an editor or Blender gets two files that start together, and the take
//! folder HLAE made, empty by then, goes.

use std::path::{Path, PathBuf};

/// The bytes `mirv_agr start` writes before any frame: `afxGameRecord\0`
/// and the format version.
pub const AGR_HEADER_BYTES: u64 = 18;

/// Whether the `.agr` at `agr` holds at least one frame. HLAE leaves a
/// header-only file when it records nothing: measured on the 25th
/// Anniversary build with the game kept behind other windows (#450), every
/// `.agr` was 18 bytes while `sound.wav` kept growing. A missing file holds
/// nothing either.
pub fn agr_has_frames(agr: &Path) -> bool {
    std::fs::metadata(agr).is_ok_and(|m| m.len() > AGR_HEADER_BYTES)
}

/// Where the sound for `agr` goes: beside it, with its name and `.wav`.
pub fn sound_path_for(agr: &Path) -> PathBuf {
    agr.with_extension("wav")
}

/// HLAE's `sound.wav` under `block_folder`: in its newest `take*` folder that
/// has one. `None` when no take folder has one.
pub fn take_sound(block_folder: &Path) -> Option<PathBuf> {
    let mut takes: Vec<PathBuf> = super::take_length::take_folders(block_folder)
        .into_iter()
        .filter(|folder| folder != block_folder)
        .collect();
    takes.sort();
    takes
        .iter()
        .rev()
        .map(|take| take.join("sound.wav"))
        .find(|wav| wav.is_file())
}

/// Moves HLAE's `sound.wav` under `block_folder` beside `agr`
/// ([`sound_path_for`]), replacing an older one there, then removes the take
/// folder it came from when nothing but empty folders is left in it.
/// Returns where the sound went, or `None` when there was none
/// ([`take_sound`]). Fails when the move fails; a take folder that won't go
/// is left.
pub fn place_take_sound(block_folder: &Path, agr: &Path) -> std::io::Result<Option<PathBuf>> {
    let Some(sound) = take_sound(block_folder) else {
        return Ok(None);
    };
    let placed = sound_path_for(agr);
    std::fs::rename(&sound, &placed)?;
    if let Some(take) = sound.parent()
        && holds_no_files(take)
    {
        // Only empty folders: removing them loses nothing, and a leftover
        // take folder is harmless.
        let _ = std::fs::remove_dir_all(take);
    }
    Ok(Some(placed))
}

/// What [`place_batch_sounds`] did across an AGR batch's takes.
#[derive(Debug, Default, PartialEq)]
pub struct SoundPlacement {
    /// Takes whose sound now sits beside their `.agr`.
    pub placed: usize,
    /// Takes whose `.agr` holds no frame ([`agr_has_frames`]), by take key.
    pub empty_agr: Vec<String>,
    /// Takes with an `.agr` but no `sound.wav`, by take key.
    pub no_sound: Vec<String>,
    /// Takes whose sound could not be moved: key and error.
    pub failed: Vec<(String, String)>,
}

/// Puts each AGR take's sound beside its `.agr` ([`place_take_sound`]), for
/// every block of a batch whose `.agr` is on disk; a block that recorded
/// nothing is skipped. Logs one line with the counts, plus one per empty
/// `.agr`, missing sound or failed move, and returns the same.
pub fn place_batch_sounds(blocks: &[crate::patch::types::CaptureBlock]) -> SoundPlacement {
    let mut result = SoundPlacement::default();
    let mut agrs = 0;
    for block in blocks {
        let folder = Path::new(&block.take_folder);
        let agr = folder.join(crate::patch::builder::agr_file_name(
            &block.demo_name,
            block.block_index,
        ));
        if !agr.is_file() {
            continue;
        }
        agrs += 1;
        if !agr_has_frames(&agr) {
            crate::log_markdown(&format!(
                "⚠️ **AGR sound** — {}: HLAE recorded nothing into {} (header only). Seen on the \
                 25th Anniversary build with the game kept behind other windows (#450).",
                block.take_key,
                agr.display()
            ));
            result.empty_agr.push(block.take_key.clone());
        }
        match place_take_sound(folder, &agr) {
            Ok(Some(_)) => result.placed += 1,
            Ok(None) => {
                crate::log_markdown(&format!(
                    "⚠️ **AGR sound** — {}: no `sound.wav` under {}, so its `.agr` has no sound \
                     beside it. HLAE's movie recorder needs its FFmpeg for AGR mode's sound.",
                    block.take_key,
                    folder.display()
                ));
                result.no_sound.push(block.take_key.clone());
            }
            Err(e) => {
                crate::log_markdown(&format!(
                    "⚠️ **AGR sound** — {}: could not move the sound beside {}: {e}",
                    block.take_key,
                    agr.display()
                ));
                result.failed.push((block.take_key.clone(), e.to_string()));
            }
        }
    }
    if agrs > 0 {
        crate::log_markdown(&format!(
            "🦴 **AGR sound** — {} of {agrs} `.agr` file(s) have their sound beside them as a \
             `.wav` of the same name.",
            result.placed
        ));
    }
    result
}

/// Whether `folder` holds no file at any depth (empty stream folders only).
fn holds_no_files(folder: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return false;
    };
    entries.flatten().all(|entry| {
        entry
            .file_type()
            .is_ok_and(|t| t.is_dir() && holds_no_files(&entry.path()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    fn block(tag: &str) -> (Scratch, PathBuf) {
        let root = Scratch::new(format_args!("agr_take_{tag}"));
        let block = root.join("session_1").join("dodstudio_chain_01_b0");
        std::fs::create_dir_all(&block).unwrap();
        (root, block)
    }

    /// The take folder AGR mode's movie leaves: `sound.wav` and an empty
    /// `all` stream folder.
    fn hlae_take(block: &Path, take: &str, sound: &[u8]) {
        std::fs::create_dir_all(block.join(take).join("all")).unwrap();
        std::fs::write(block.join(take).join("sound.wav"), sound).unwrap();
    }

    #[test]
    fn a_header_only_agr_recorded_nothing() {
        let (_root, block) = block("header_only");
        let agr = block.join("dodstudio_chain_01_b0.agr");
        assert!(!agr_has_frames(&agr), "missing");
        std::fs::write(&agr, b"afxGameRecord\0\x06\0\0\0").unwrap();
        assert!(!agr_has_frames(&agr), "header only");
        std::fs::write(&agr, b"afxGameRecord\0\x06\0\0\0\xff\xff\xff\xffafxFrame\0").unwrap();
        assert!(agr_has_frames(&agr));
    }

    #[test]
    fn the_sound_goes_beside_the_agr_under_its_name() {
        assert_eq!(
            sound_path_for(Path::new(
                "D:/s/dodstudio_chain_01_b0/dodstudio_chain_01_b0.agr"
            )),
            PathBuf::from("D:/s/dodstudio_chain_01_b0/dodstudio_chain_01_b0.wav")
        );
    }

    #[test]
    fn the_sound_moves_beside_the_agr_and_the_empty_take_folder_goes() {
        let (_root, block) = block("moved");
        hlae_take(&block, "take0000", b"RIFF sound");
        let agr = block.join("dodstudio_chain_01_b0.agr");

        let placed = place_take_sound(&block, &agr).unwrap();

        assert_eq!(placed, Some(block.join("dodstudio_chain_01_b0.wav")));
        assert_eq!(std::fs::read(placed.unwrap()).unwrap(), b"RIFF sound");
        assert!(!block.join("take0000").exists());
    }

    #[test]
    fn a_take_folder_with_anything_else_in_it_stays() {
        let (_root, block) = block("kept");
        hlae_take(&block, "take0000", b"RIFF");
        std::fs::write(block.join("take0000").join("all").join("00000.bmp"), b"bmp").unwrap();
        let agr = block.join("dodstudio_chain_01_b0.agr");

        place_take_sound(&block, &agr).unwrap();

        assert!(
            block
                .join("take0000")
                .join("all")
                .join("00000.bmp")
                .is_file()
        );
    }

    fn capture_block(folder: &Path, index: usize) -> crate::patch::types::CaptureBlock {
        crate::patch::types::CaptureBlock {
            demo_name: "dodstudio_chain_01".to_string(),
            block_index: index,
            drive_index: 0,
            take_key: format!("session_1/dodstudio_chain_01_b{index}"),
            take_folder: folder.to_path_buf(),
            source_streak_indices: vec![index],
            start_tick: 0,
            end_tick: 0,
            record_start_tick: 0,
            record_stop_tick: 0,
            ramped_gaps: 0,
            plan: None,
        }
    }

    #[test]
    fn a_batch_places_every_recorded_takes_sound_and_counts_the_rest() {
        let root = Scratch::new("agr_take_batch");
        let folder = |i: usize| {
            root.join("session_1")
                .join(format!("dodstudio_chain_01_b{i}"))
        };
        // b0: a whole take. b1: an empty .agr with sound. b2: no .agr at all.
        for i in 0..3 {
            std::fs::create_dir_all(folder(i)).unwrap();
        }
        let agr = |i: usize| folder(i).join(format!("dodstudio_chain_01_b{i}.agr"));
        std::fs::write(agr(0), [1u8; 64]).unwrap();
        hlae_take(&folder(0), "take0000", b"RIFF0");
        std::fs::write(agr(1), [0u8; 18]).unwrap();
        hlae_take(&folder(1), "take0000", b"RIFF1");
        let blocks: Vec<_> = (0..3).map(|i| capture_block(&folder(i), i)).collect();

        let result = place_batch_sounds(&blocks);

        assert_eq!(result.placed, 2);
        assert_eq!(result.empty_agr, vec!["session_1/dodstudio_chain_01_b1"]);
        assert!(result.no_sound.is_empty() && result.failed.is_empty());
        assert!(folder(0).join("dodstudio_chain_01_b0.wav").is_file());
    }

    #[test]
    fn the_newest_take_gives_the_sound() {
        let (_root, block) = block("newest");
        hlae_take(&block, "take0000", b"old");
        hlae_take(&block, "take0001", b"new");
        assert_eq!(
            take_sound(&block),
            Some(block.join("take0001").join("sound.wav"))
        );
    }

    #[test]
    fn no_sound_places_nothing() {
        let (_root, block) = block("none");
        let agr = block.join("dodstudio_chain_01_b0.agr");
        assert_eq!(place_take_sound(&block, &agr).unwrap(), None);
        assert!(!sound_path_for(&agr).exists());
    }

    #[test]
    fn a_wav_in_the_block_folder_itself_is_not_a_takes_sound() {
        // The one placed last time is not HLAE's.
        let (_root, block) = block("own");
        std::fs::write(block.join("dodstudio_chain_01_b0.wav"), b"placed").unwrap();
        assert_eq!(take_sound(&block), None);
    }
}
