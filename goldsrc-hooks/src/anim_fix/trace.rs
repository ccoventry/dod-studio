//! Diagnostics: the log lines and status reply that say what `apply()` is
//! doing. Nothing here changes what the fix plays.

use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use super::ANIMATIONS_PLAYED;
use super::sequences::model_sequences;
use crate::engine::{self, ClEntityS, ModelSPartial};

/// The third-person model the spectated player was last seen holding.
static LAST_HELD_MODEL: Mutex<Option<String>> = Mutex::new(None);

/// Logs the held third-person model whenever it changes.
///
/// DoD ships far more `p_` models than weapons because they encode stance as
/// well: `p_mg42bu` / `p_mg42bd` / `p_mg42pr` / `p_mg42sr`, and the 30cal set
/// runs `pr` / `r` / `sr`. Only "bu" and "bd" are known for certain (bipod up
/// and down, and they are the only two carrying a shoot sequence); the rest
/// are inferred -- "pr" looks like prone and "sr" like sprint, but that is a
/// reading of the filenames, not a fact.
///
/// A timestamped trail of the changes can be matched against what the player
/// was visibly doing, which settles it by observation rather than by guessing
/// at abbreviations.
pub static LOG_HELD_MODELS: AtomicBool = AtomicBool::new(false);

pub(super) fn note_held_model(spectated: &ClEntityS) {
    if !LOG_HELD_MODELS.load(Ordering::Relaxed) {
        // Forget what was last seen, so switching this on mid-session reports
        // the current model straight away rather than waiting for the next
        // change -- which might never come if the player just stands there.
        *LAST_HELD_MODEL.lock().unwrap() = None;
        return;
    }
    let Some(studio) = engine::engine_studio() else {
        return;
    };
    let held = unsafe { (studio.get_model_by_index)(spectated.curstate.weaponmodel) };
    if held.is_null() {
        return;
    }
    let name = unsafe { (*held).name_str() }.into_owned();

    let mut last = LAST_HELD_MODEL.lock().unwrap();
    if last.as_deref() == Some(name.as_str()) {
        return;
    }
    let previous = last.replace(name.clone());
    drop(last);

    unsafe {
        crate::debug::report(&format!(
            "anim_fix: held model changed -- \"{name}\" (was {}) -- what was the player doing?",
            previous.as_deref().unwrap_or("<none>")
        ))
    };
}

/// Viewmodel/held pairs that could not be matched, reported once each.
///
/// The failure mode this guards against is silent and total: an unlisted
/// naming mismatch discards every frame for that weapon forever, and the only
/// symptom is an animation that never plays. Logged unconditionally, not behind
/// the verbose switch, because nobody would think to turn it on for a weapon
/// they had no reason to suspect.
static REPORTED_MISMATCHES: Mutex<Option<HashSet<(String, String)>>> = Mutex::new(None);

pub(super) fn note_unmatched_pair(viewmodel_name: &str, held_name: &str) {
    let mut guard = REPORTED_MISMATCHES.lock().unwrap();
    let seen = guard.get_or_insert_with(HashSet::new);
    if !seen.insert((viewmodel_name.to_string(), held_name.to_string())) {
        return;
    }
    drop(guard);
    unsafe {
        crate::debug::report(&format!(
            "anim_fix: \"{viewmodel_name}\" and \"{held_name}\" never match, so every frame holding this weapon is skipped -- if they are the same weapon, it needs a VIEWMODEL_ALIASES entry"
        ))
    };
}

/// How far `apply()` got on the most recent frame. Reported only when it
/// *changes*, never per frame -- this runs 60+ times a second, so a line per
/// call would flood the log and slow a capture. Logged as a trace, it answers
/// the only question a take that looks unchanged actually raises: which of the
/// preconditions is the one not being met.
static STAGE: AtomicI32 = AtomicI32::new(-1);

pub(super) const STAGE_DISABLED: i32 = 0;
pub(super) const STAGE_NO_ENGFUNCS: i32 = 1;
pub(super) const STAGE_NOT_SPECTATING: i32 = 2;
pub(super) const STAGE_NO_VIEWMODEL_ENTITY: i32 = 3;
pub(super) const STAGE_NO_VIEWMODEL_MODEL: i32 = 4;
pub(super) const STAGE_NOT_A_DEPLOYABLE_WEAPON: i32 = 5;
pub(super) const STAGE_NO_SPECTATED_PLAYER: i32 = 6;
pub(super) const STAGE_RUNNING: i32 = 7;
pub(super) const STAGE_VIEWMODEL_MISMATCH: i32 = 8;

fn stage_name(stage: i32) -> &'static str {
    match stage {
        STAGE_DISABLED => "disabled (dodstudio_hltv_show_viewmodel_animations is 0)",
        STAGE_NO_ENGFUNCS => "waiting for engfuncs",
        STAGE_NOT_SPECTATING => {
            "not spectating (IsSpectateOnly() is false) -- the fix only acts in a spectated view"
        }
        STAGE_NO_VIEWMODEL_ENTITY => "no viewmodel entity",
        STAGE_NO_VIEWMODEL_MODEL => "viewmodel entity has no model",
        STAGE_NOT_A_DEPLOYABLE_WEAPON => {
            "viewmodel is not one of the deployable weapons (MG42/MG34/BAR/Bren)"
        }
        STAGE_NO_SPECTATED_PLAYER => "spectated entity is missing or is not a player",
        STAGE_RUNNING => "running -- all preconditions met",
        STAGE_VIEWMODEL_MISMATCH => {
            "viewmodel is not the weapon the spectated player is holding (ignored this frame)"
        }
        _ => "unknown",
    }
}

/// The stage plus the viewmodel pointers behind it, as of the last frame that
/// changed either. Logging keys off all three, so an alternation between two
/// different viewmodels and one viewmodel flickering to null look different in
/// the log instead of both reading as "stage changed".
static LAST_TRACE: Mutex<Option<(i32, usize, usize, i32)>> = Mutex::new(None);

pub(super) fn stage(stage: i32) {
    stage_with(
        stage,
        std::ptr::null_mut::<u8>(),
        std::ptr::null_mut::<u8>(),
        -1,
    );
}

/// Records how far this frame got, logging only when the stage, either
/// pointer, or the entity index changes.
///
/// `index` is the viewmodel entity's own `index` field, which `apply()` uses
/// as the spectated player. Logging it answers the question the model pointer
/// alone leaves open: whether the weapon changing means the *spectated player*
/// changed (the director moving on) or the same player swapped weapons.
pub(super) fn stage_with<A, B>(stage: i32, entity: *mut A, model: *mut B, index: i32) {
    STAGE.store(stage, Ordering::Relaxed);

    let key = (stage, entity as usize, model as usize, index);
    let mut last = LAST_TRACE.lock().unwrap();
    if *last == Some(key) {
        return;
    }
    *last = Some(key);
    drop(last);

    unsafe {
        crate::debug::report(&format!(
            "anim_fix: {} | entity {entity:p} idx {index}, model {model:p}",
            stage_name(stage)
        ))
    };
}

/// One-line summary for the `dodstudio_hltv_show_viewmodel_animations` status reply.
pub fn status() -> String {
    let seen = SEEN_VIEWMODELS.lock().unwrap();
    let count = seen.as_ref().map(|s| s.len()).unwrap_or(0);
    format!(
        "{} -- {count} viewmodels, {} played",
        stage_name(STAGE.load(Ordering::Relaxed)),
        ANIMATIONS_PLAYED.load(Ordering::Relaxed),
    )
}

/// Every distinct viewmodel this session, logged once each.
///
/// `apply()` keys entirely off the viewmodel's model name, so when it reports
/// "not one of the deployable weapons" the only useful follow-up is *which*
/// model it actually saw. Capped, and one line per distinct name rather than
/// per frame.
static SEEN_VIEWMODELS: Mutex<Option<HashSet<String>>> = Mutex::new(None);

pub(super) fn note_viewmodel(name: &str, deployable: bool, model: *mut ModelSPartial) {
    const LIMIT: usize = 24;
    let mut guard = SEEN_VIEWMODELS.lock().unwrap();
    let seen = guard.get_or_insert_with(HashSet::new);
    if seen.len() >= LIMIT || !seen.insert(name.to_string()) {
        return;
    }
    drop(guard);

    // Dump the model's whole sequence list the first time it is seen. Every
    // animation this fix plays is found by matching a label ("shoot", "draw",
    // "reload"), so when a lookup comes back empty the only thing worth
    // knowing is what the model actually calls its animations -- and DoD is
    // not consistent about it. One line per weapon, not per frame.
    let sequences = model_sequences(model);
    unsafe {
        crate::debug::report(&format!(
            "anim_fix: viewmodel seen -- \"{name}\" (deployable weapon: {}), {} sequences: [{}]",
            if deployable { "yes" } else { "no" },
            sequences.len(),
            sequences
                .iter()
                .enumerate()
                .map(|(i, s)| format!("{i}:{}", s.label))
                .collect::<Vec<_>>()
                .join(", ")
        ))
    };
}

/// `"idx 6"`. Names would be nicer, but reaching them needs an engine slot
/// that cannot be verified against any call site in client.dll -- see the note
/// on `ClEngineFuncsPartial`. The index is stable within a match and can be
/// matched against the scoreboard.
pub(super) fn describe_player(index: i32) -> String {
    format!("idx {index}")
}
