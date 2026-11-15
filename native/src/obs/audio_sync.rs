//! Lining OBS's game sound up with the picture (#1385).
//!
//! `-wavonly` ([`super::GAME_ARGS`]) plays the game's sound late, so the game
//! audio source gets a negative sync offset. OBS applies a change of that
//! offset unevenly, which is what this module works around.

use std::time::Duration;

use super::client::{ObsClient, ObsError};

/// The sync offset the game audio source gets, in ms.
///
/// waveOut plays the game's sound about 0.28 s after the frame it belongs to.
/// Measured against HLAE takes of the same spans (sound.wav is timed from the
/// frames, so it is the reference), 0.8 s pieces of
/// `ktps8w9-stealth_gskill_rr2_h1`, one PC (#1385): with no offset OBS's sound
/// was +254 to +296 ms late on PRE and +258 to +321 on POST; with -270 set
/// before the game's sound started it was +8 to +35 ms on PRE and -12 to +36
/// on POST, within about two frames at 60 fps. Another PC's waveOut latency
/// may differ (#646). The game's `_snd_mixahead` (default 0.1 s) is part of it: at 0.05
/// the lag was about 50 ms shorter.
pub const GAME_AUDIO_SYNC_OFFSET_MS: i64 = -270;

/// How long the in-between offset stays set: long enough for OBS to take in a
/// few of the source's 10 ms packets while the game's sound is playing.
#[cfg(not(test))]
const STEP_HOLD: Duration = Duration::from_millis(250);
/// Tests talk to a fake OBS with no sound to wait for.
#[cfg(test)]
const STEP_HOLD: Duration = Duration::ZERO;

/// The offsets to set, in order, to take `source` from `current` to `target`.
///
/// OBS re-times a source's sound to a new offset only when the old offset was
/// nonzero (`libobs/obs-source.c`, `source_output_audio_data`, OBS 33.0.1: the
/// `if (source->last_sync_offset)` before `push_back = false`). From 0 it keeps
/// adding the sound straight after what it already had, at the old time, until
/// the stream next breaks, and only then does the new offset take effect.
/// Measured on PRE with the game playing (#1385): 0 to -270 set live left the
/// sound +271..+293 ms late (POST +265..+300), the offset doing nothing; the
/// same change made through -1 first landed it +2..+35 (POST -3..+28).
/// Offsets set while no game sound plays
/// take effect when it starts either way; the extra step costs nothing then.
fn steps(current: i64, target: i64) -> Vec<i64> {
    if current == target {
        Vec::new()
    } else if current == 0 {
        // -1 ms is too small to hear while it lasts.
        vec![if target > 0 { 1 } else { -1 }, target]
    } else {
        vec![target]
    }
}

/// Sets `source`'s sync offset to `target` so that OBS applies it even while
/// the game's sound is playing. Returns whether anything changed.
pub fn set_offset(client: &mut ObsClient, source: &str, target: i64) -> Result<bool, ObsError> {
    // An offset OBS won't tell is taken as 0: the extra step is harmless, and
    // the offset still gets set.
    let current = client.input_audio_sync_offset(source).unwrap_or(0);
    let plan = steps(current, target);
    for (i, ms) in plan.iter().enumerate() {
        if i > 0 {
            std::thread::sleep(STEP_HOLD);
        }
        client.set_input_audio_sync_offset(source, *ms)?;
    }
    Ok(!plan.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::obs::fake_obs::{Reply, serve};
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    #[test]
    fn an_offset_already_in_place_is_left_alone() {
        assert!(steps(-270, -270).is_empty());
    }

    /// The change OBS would otherwise ignore while the game plays.
    #[test]
    fn a_change_from_zero_goes_through_a_nonzero_offset_first() {
        assert_eq!(steps(0, -270), vec![-1, -270]);
        assert_eq!(steps(0, 120), vec![1, 120]);
    }

    #[test]
    fn a_change_from_a_nonzero_offset_is_one_step() {
        assert_eq!(steps(-500, -270), vec![-270]);
        assert_eq!(steps(-270, 0), vec![0]);
    }

    /// The offsets OBS is sent, in order, starting from `current`.
    /// `None` is an OBS that refuses to say.
    fn sent_from(current: Option<i64>) -> (bool, Vec<i64>) {
        let set = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&set);
        let obs = serve(move |kind, data| match kind {
            "GetInputAudioSyncOffset" => match current {
                Some(ms) => Reply::ok(json!({ "inputAudioSyncOffset": ms })),
                None => Reply::err("no such input"),
            },
            "SetInputAudioSyncOffset" => {
                log.lock()
                    .unwrap()
                    .push(data["inputAudioSyncOffset"].as_i64().unwrap());
                Reply::ok(json!({}))
            }
            _ => Reply::err("unexpected"),
        });
        let mut client = ObsClient::connect(&obs.url, "").expect("connect to the fake");
        let changed = set_offset(&mut client, "src", -270).expect("sets the offset");
        drop(client);
        let sent = set.lock().unwrap().clone();
        (changed, sent)
    }

    #[test]
    fn sets_a_fresh_source_through_minus_one() {
        assert_eq!(sent_from(Some(0)), (true, vec![-1, -270]));
    }

    #[test]
    fn sends_nothing_when_the_offset_is_already_right() {
        assert_eq!(sent_from(Some(-270)), (false, vec![]));
    }

    #[test]
    fn an_offset_obs_will_not_read_is_still_set() {
        assert_eq!(sent_from(None), (true, vec![-1, -270]));
    }
}
