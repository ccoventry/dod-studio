//! Each model's sequence list, read once out of its studio header and cached,
//! and the label lookups that pick a viewmodel animation from it.

use std::collections::HashMap;
use std::sync::Mutex;

use super::classify::{DeployState, swap_family_prefix};
use crate::engine::{self, ModelSPartial, StudioHdrPartial, StudioSeqDescPartial};

/// One model's sequences: the label, and how long it runs in seconds.
type SequenceInfo = Vec<(String, f64)>;

static SEQUENCE_CACHE: Mutex<Option<HashMap<usize, SequenceInfo>>> = Mutex::new(None);

/// Returns every sequence label baked into `model`, cached by the model
/// pointer's address (stable for the life of a precached model).
fn model_sequence_info(model: *mut ModelSPartial) -> Vec<(String, f64)> {
    let key = model as usize;
    let mut cache = SEQUENCE_CACHE.lock().unwrap();
    let cache = cache.get_or_insert_with(HashMap::new);
    if let Some(cached) = cache.get(&key) {
        return cached.clone();
    }

    let Some(studio) = engine::engine_studio() else {
        return Vec::new();
    };
    let extradata = unsafe { (studio.mod_extradata)(model) };
    if extradata.is_null() {
        return Vec::new();
    }

    let header = extradata as *const StudioHdrPartial;
    // Validate before trusting anything in here. `mod_extradata` is happy to
    // hand back a pointer for a model that is not a studio model at all, and
    // the loop below walks `numseq` entries at `seqindex` with no bound of its
    // own -- a garbage header would read arbitrary memory until it faulted.
    const STUDIO_MAGIC: i32 = 0x5453_4449; // "IDST"
    const MAX_SEQUENCES: i32 = 512;
    let (id, numseq, seqindex) = unsafe { ((*header).id, (*header).numseq, (*header).seqindex) };
    if id != STUDIO_MAGIC || !(0..=MAX_SEQUENCES).contains(&numseq) || seqindex <= 0 {
        unsafe {
            crate::debug::report(&format!(
                "anim_fix: refusing to read sequences from {model:p} -- header id {id:#x}, numseq {numseq}, seqindex {seqindex}"
            ))
        };
        cache.insert(key, Vec::new());
        return Vec::new();
    }
    let base = extradata as *const u8;

    let mut labels = Vec::with_capacity(numseq.max(0) as usize);
    for i in 0..numseq {
        let entry =
            unsafe { base.add(seqindex as usize + i as usize * size_of::<StudioSeqDescPartial>()) }
                as *const StudioSeqDescPartial;
        let (fps, frames) = unsafe { ((*entry).fps, (*entry).numframes) };
        // A sequence with a nonsense rate or frame count gets zero rather than
        // an absurd duration -- callers treat zero as "don't know".
        let duration = if fps > 0.0 && (0..=4096).contains(&frames) {
            f64::from(frames) / f64::from(fps)
        } else {
            0.0
        };
        labels.push((unsafe { (*entry).label_str() }.into_owned(), duration));
    }

    cache.insert(key, labels.clone());
    labels
}

/// How long one of a model's sequences runs, in seconds. Zero when the model
/// or index cannot be read, or the header's numbers are not credible.
pub(super) fn model_sequence_duration(model: *mut ModelSPartial, sequence: i32) -> f64 {
    if sequence < 0 {
        return 0.0;
    }
    model_sequence_info(model)
        .get(sequence as usize)
        .map(|(_, d)| *d)
        .unwrap_or(0.0)
}

/// Just the labels, for everything that only needs to name a sequence.
/// The same list, for anything outside this module that needs to know what a
/// sequence index is called. `hand_signals` asks whether a label starts with
/// `hs_`, and a cached walk is what keeps that a per-frame-cheap question.
pub(crate) fn sequence_labels(model: *mut ModelSPartial) -> Vec<String> {
    model_sequence_strings(model)
}

pub(super) fn model_sequence_strings(model: *mut ModelSPartial) -> Vec<String> {
    model_sequence_info(model)
        .into_iter()
        .map(|(label, _)| label)
        .collect()
}

fn apply_deploy_state_to_sequence(
    sequence: i32,
    state: Option<DeployState>,
    viewmodel: *mut ModelSPartial,
) -> i32 {
    let Some(state) = state else { return sequence };
    let labels = model_sequence_strings(viewmodel);
    let Some(current_label) = labels.get(sequence.max(0) as usize) else {
        return sequence;
    };

    let wanted = swap_family_prefix(current_label, state);
    labels
        .iter()
        .position(|l| l.eq_ignore_ascii_case(&wanted))
        .map(|i| i as i32)
        // No matching sequence in the other family (e.g. mg42/mg34's single
        // shared "reload") -- keep the caller's original index.
        .unwrap_or(sequence)
}

pub(super) fn animation_lookup_sequence(
    label: &str,
    state: Option<DeployState>,
    viewmodel: *mut ModelSPartial,
) -> i32 {
    animation_lookup_any(&[label], state, viewmodel)
}

/// Finds the first sequence matching any of `candidates`, in order.
///
/// DoD's models are not consistent about what they call things -- a firing
/// animation is `shoot` on some weapons and `fire` on others -- so the caller
/// gives the names worth trying rather than assuming one.
pub(super) fn animation_lookup_any(
    candidates: &[&str],
    state: Option<DeployState>,
    viewmodel: *mut ModelSPartial,
) -> i32 {
    let labels = model_sequence_strings(viewmodel);

    // Exact match first, substring only as a fallback. Several models list a
    // qualified variant *before* the plain one -- v_luger.mdl is
    // [.., 5:reload_empty, 6:reload, ..] -- so a substring-first search picks
    // the wrong animation, which is exactly what made a luger reload play as
    // reload_empty in testing. The fallback still matters, because the bipod
    // weapons have no bare label at all: v_bar.mdl is up_reload / down_reload,
    // v_mg42.mdl is upshoot / downshoot.
    for candidate in candidates {
        let needle = candidate.to_lowercase();
        if let Some(i) = labels.iter().position(|l| l.to_lowercase() == needle) {
            return apply_deploy_state_to_sequence(i as i32, state, viewmodel);
        }
    }
    for candidate in candidates {
        let needle = candidate.to_lowercase();
        if let Some(i) = labels
            .iter()
            .position(|l| l.to_lowercase().contains(&needle))
        {
            return apply_deploy_state_to_sequence(i as i32, state, viewmodel);
        }
    }
    -1
}
