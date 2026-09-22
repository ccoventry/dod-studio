//! `dodstudio_hide_hand_signals`: stop players miming their voice commands.
//!
//! ## Why the voice mute does not cover this
//!
//! `dodstudio_mute_voice_commands` NOPs a client-side `EV_PlaySound` call, which
//! is where the *sound* comes from. The gesture is a different mechanism
//! entirely: `client.dll` contains **no `hs_` string at all**, because the
//! client never picks these animations by name. The server picks a sequence
//! index, and it arrives as replicated entity state -- `curstate.sequence` --
//! which is also why it survives into an HLTV demo.
//!
//! So there is no call to remove. What there is, every frame, is a number in a
//! structure this DLL can already read.
//!
//! ## Detection is by label, not by index
//!
//! #283 measured the sequence indices: 54 `hs_*` sequences per player model,
//! in two contiguous runs at 212-238 and 287-313, identical across all five
//! stock models. It also flagged the risk in using them -- 17 of the user's 41
//! viewmodels are custom, and a custom *player* model could reorder its
//! sequence list.
//!
//! So this does not use the indices. It reads the model's own sequence labels
//! -- the same `mstudioseqdesc_t` walk `anim_fix` already does and caches -- and
//! asks whether the label starts with `hs_`. That is exact for any model,
//! custom or stock, and a model that reorders its sequences is handled rather
//! than mis-suppressed.
//!
//! ## What is substituted
//!
//! A sequence index has to be *something*, so blanking is not an option. Each
//! player's last non-`hs_` sequence is remembered and put back for the duration
//! of the signal, which is their stance/weapon idle or aim in every case that
//! matters. A player first seen mid-signal has no remembered sequence, so they
//! are left alone rather than given a guess -- counted, and reported by
//! `dodstudio_status`.
//!
//! `gaitsequence` is deliberately untouched. It drives the legs independently,
//! and the standing/prone split in the `hs_` names says the signal is an
//! upper-body sequence, so substituting `sequence` alone should be the whole
//! job. If a signal ever turns out to move the legs too, that is where to look.
//!
//! ## Where the write happens, and why it should be early enough
//!
//! #283's remaining open question was whether a write lands before the renderer
//! reads it. This runs from `commands::poll`, which the crate drives from the
//! `HUD_Frame` trampoline -- and `HUD_Frame` is called once per frame *before*
//! the engine renders the view, not from `HUD_Redraw`, which paints the HUD
//! after it. So the write is in place for the same frame's `StudioDrawPlayer`.
//!
//! That is an argument from call order, not a measurement. If a live test shows
//! the gesture surviving, the fallback the issue names is right: the studio
//! renderer's `StudioDrawPlayer`, reachable through the interface already
//! captured in slot 39.
//!
//! ## It applies to every player in view
//!
//! Not only the spectated one, which is what clean footage wants and is a
//! behavioural choice rather than an obvious default -- #283 said so and the
//! status line repeats it.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};

use crate::engine::{self, ClEntityS};
use crate::names::console_name;

/// The cvar name. Registered in `commands.rs`.
pub const NAME: &str = console_name!("hide_hand_signals");

/// Set from the cvar by `commands::poll`.
pub static ENABLED: AtomicBool = AtomicBool::new(false);

/// Every hand-signal sequence in DoD's player models is named `hs_<command>`,
/// with a prone variant of each. 54 per model, 27 of them prone.
const PREFIX: &str = "hs_";

/// GoldSrc's player slots. Entity indices 1..=32 are players; anything above is
/// a map entity or a temporary, and none of them animate from this list.
const MAX_PLAYERS: usize = 32;

/// Per-player last non-signal sequence, or -1 for "not seen yet".
static LAST_NORMAL: [AtomicI32; MAX_PLAYERS + 1] = [const { AtomicI32::new(-1) }; MAX_PLAYERS + 1];

/// How many signals have been replaced this session.
static REPLACED: AtomicUsize = AtomicUsize::new(0);

/// How many were seen with no remembered sequence to put back.
static UNSUBSTITUTED: AtomicUsize = AtomicUsize::new(0);

/// Whether `sequence` is a hand signal in `model`'s own sequence list.
///
/// `None` when the model has no usable sequence list -- a non-studio model, or
/// one whose header did not survive `anim_fix`'s sanity checks. An unknown
/// sequence is never treated as a signal, because suppressing the wrong
/// animation is worse than suppressing none.
fn is_hand_signal(model: *mut engine::ModelSPartial, sequence: i32) -> Option<bool> {
    if sequence < 0 {
        return Some(false);
    }
    let labels = crate::anim_fix::sequence_labels(model);
    labels
        .get(sequence as usize)
        .map(|label| label.starts_with(PREFIX))
}

/// Replaces every visible player's hand-signal animation with the last ordinary
/// one they were seen in, returning how many were replaced this frame.
///
/// Called every frame from `commands::poll`. Cheap: 32 pointer reads and a
/// cached label lookup, and it returns immediately when the setting is off.
pub fn apply() -> usize {
    if !ENABLED.load(Ordering::Relaxed) {
        return 0;
    }
    let Some(engfuncs) = engine::engfuncs() else { return 0 };
    let Some(studio) = engine::engine_studio() else { return 0 };

    let mut replaced = 0;
    // `skip(1)` because entity index 0 is the world, and iterating the slots
    // themselves keeps the engine's 1-based index and the array in step with
    // no arithmetic.
    for (index, remembered) in LAST_NORMAL.iter().enumerate().skip(1) {
        // Safety: the engine's own accessor, which returns null for a slot
        // that holds nothing.
        let entity: *mut ClEntityS = unsafe { (engfuncs.get_entity_by_index)(index as i32) };
        if entity.is_null() {
            continue;
        }
        // Safety: a non-null `cl_entity_t` from the engine lives as long as the
        // frame, and this is the engine's own thread.
        let entity = unsafe { &mut *entity };
        if entity.player == 0 {
            continue;
        }

        let sequence = entity.curstate.sequence;
        // Safety: `get_model_by_index` returns null for an index the engine has
        // not precached, which `sequence_labels` handles.
        let model = unsafe { (studio.get_model_by_index)(entity.curstate.modelindex) };
        match is_hand_signal(model, sequence) {
            Some(true) => {}
            // Not a signal, or a model with no readable sequence list: this is
            // the player's ordinary animation, so remember it.
            _ => {
                remembered.store(sequence, Ordering::Relaxed);
                continue;
            }
        }

        let held = remembered.load(Ordering::Relaxed);
        if held < 0 {
            // First sighting of this player is mid-signal. Nothing to put back,
            // and inventing one would be a guess at their stance.
            UNSUBSTITUTED.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        entity.curstate.sequence = held;
        replaced += 1;
    }

    if replaced > 0 {
        REPLACED.fetch_add(replaced, Ordering::Relaxed);
    }
    replaced
}

/// Forgets every remembered sequence. Called when the setting is turned off, so
/// that turning it back on does not put back a stance from before a map change.
pub fn reset() {
    for slot in &LAST_NORMAL {
        slot.store(-1, Ordering::Relaxed);
    }
}

/// One line for `dodstudio_status`.
pub fn status() -> String {
    if !ENABLED.load(Ordering::Relaxed) {
        return "hand signals play normally".to_string();
    }
    let replaced = REPLACED.load(Ordering::Relaxed);
    let missed = UNSUBSTITUTED.load(Ordering::Relaxed);
    if replaced == 0 && missed == 0 {
        return format!(
            "watching every player for {PREFIX}* sequences; none seen yet (this applies to everyone in view, not just the spectated player)"
        );
    }
    let mut line = format!("replaced {replaced} hand-signal frame(s) across every player in view");
    if missed > 0 {
        line.push_str(&format!(
            ", and left {missed} alone for players first seen mid-signal"
        ));
    }
    line
}

/// Whether anything has been replaced this session.
pub fn has_acted() -> bool {
    REPLACED.load(Ordering::Relaxed) > 0 || UNSUBSTITUTED.load(Ordering::Relaxed) > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One slot per player plus the unused index 0, so `LAST_NORMAL[index]`
    /// can be written with the engine's own 1-based entity index and no
    /// arithmetic. Getting this wrong would index out of bounds on slot 32,
    /// at runtime, inside the game.
    #[test]
    fn there_is_a_slot_for_every_player_index() {
        assert_eq!(LAST_NORMAL.len(), MAX_PLAYERS + 1);
        assert!(LAST_NORMAL.len() > MAX_PLAYERS, "index {MAX_PLAYERS} must be addressable");
    }

    /// The prefix is the whole detection rule, and it has to match DoD's own
    /// naming. Every hand signal in the stock player models is `hs_<command>`.
    #[test]
    fn the_prefix_is_dods_own() {
        assert_eq!(PREFIX, "hs_");
        for label in ["hs_yes_sir", "hs_enemy_ahead", "hs_fall_back", "hs_grenade"] {
            assert!(label.starts_with(PREFIX), "{label}");
        }
        // Sequences that merely start with `h` must not match -- the stock
        // models have plenty.
        for label in ["crouch_aim_garand", "run_mp40", "hs", "shs_yes_sir"] {
            assert!(!label.starts_with(PREFIX), "{label}");
        }
    }

    #[test]
    fn reset_forgets_every_remembered_sequence() {
        LAST_NORMAL[1].store(42, Ordering::Relaxed);
        LAST_NORMAL[MAX_PLAYERS].store(7, Ordering::Relaxed);
        reset();
        for slot in &LAST_NORMAL {
            assert_eq!(slot.load(Ordering::Relaxed), -1);
        }
    }

    #[test]
    fn apply_does_nothing_at_all_when_the_setting_is_off() {
        let saved = ENABLED.load(Ordering::Relaxed);
        ENABLED.store(false, Ordering::Relaxed);
        // No engine, so this would fault if it got past the flag check.
        assert_eq!(apply(), 0);
        ENABLED.store(saved, Ordering::Relaxed);
    }

    #[test]
    fn status_says_who_it_applies_to() {
        let saved = ENABLED.load(Ordering::Relaxed);
        let replaced = REPLACED.swap(0, Ordering::Relaxed);
        let missed = UNSUBSTITUTED.swap(0, Ordering::Relaxed);

        ENABLED.store(false, Ordering::Relaxed);
        assert!(status().contains("normally"), "{}", status());

        ENABLED.store(true, Ordering::Relaxed);
        assert!(status().contains("not just the spectated player"), "{}", status());
        assert!(!has_acted());

        REPLACED.store(5, Ordering::Relaxed);
        assert!(status().contains('5'), "{}", status());
        assert!(has_acted());

        UNSUBSTITUTED.store(2, Ordering::Relaxed);
        assert!(status().contains("mid-signal"), "{}", status());

        REPLACED.store(replaced, Ordering::Relaxed);
        UNSUBSTITUTED.store(missed, Ordering::Relaxed);
        ENABLED.store(saved, Ordering::Relaxed);
    }
}
