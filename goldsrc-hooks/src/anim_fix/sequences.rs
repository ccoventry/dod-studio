//! Each model's sequence list, read once out of its studio header and cached,
//! and the label lookups that pick a viewmodel animation from it.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use super::classify::{DeployState, swap_family_prefix};
use crate::engine::{self, ModelSPartial, StudioHdrPartial, StudioSeqDescPartial};

/// One of a model's sequences.
pub(super) struct Sequence {
    /// The label as the model spells it. An `Arc` so `sequence_label` can hand
    /// it out without copying it.
    pub(super) label: Arc<str>,
    /// `label.to_lowercase()`, worked out once here rather than for every
    /// label on every lookup.
    lower: Box<str>,
    /// How long it runs, in seconds. Zero means "don't know".
    duration: f64,
}

/// One model's sequences, in index order.
///
/// Shared, not copied: a cache hit is one refcount bump. It used to clone
/// every label instead -- 345 of them on each DoD player model, for every
/// visible player, every frame that hand signals were being hidden.
pub(super) type SequenceInfo = Arc<[Sequence]>;

static SEQUENCE_CACHE: Mutex<Option<HashMap<usize, SequenceInfo>>> = Mutex::new(None);

/// What a model that cannot be read yet reports. Not cached, so it is tried
/// again next time; shared, so trying again does not allocate.
static NO_SEQUENCES: LazyLock<SequenceInfo> = LazyLock::new(|| Arc::from(Vec::new()));

/// Runs `f` on `model`'s sequence list, cached by the model pointer's address
/// (stable for the life of a precached model). `f` runs under the cache lock,
/// so it must not look anything up here itself.
fn with_sequences<R>(model: *mut ModelSPartial, f: impl FnOnce(&SequenceInfo) -> R) -> R {
    let key = model as usize;
    let mut cache = SEQUENCE_CACHE.lock().unwrap();
    let cache = cache.get_or_insert_with(HashMap::new);
    if let Some(cached) = cache.get(&key) {
        return f(cached);
    }
    match read_sequences(model) {
        Some(read) => f(cache.entry(key).or_insert(read)),
        None => f(&NO_SEQUENCES),
    }
}

/// Walks `model`'s studio header. `None` when the engine cannot hand the
/// model over yet, which is worth asking again about; an empty list when it
/// can but the header is not credible, which is not.
fn read_sequences(model: *mut ModelSPartial) -> Option<SequenceInfo> {
    let studio = engine::engine_studio()?;
    let extradata = unsafe { (studio.mod_extradata)(model) };
    if extradata.is_null() {
        return None;
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
        return Some(Arc::from(Vec::new()));
    }
    let base = extradata as *const u8;

    let mut sequences = Vec::with_capacity(numseq.max(0) as usize);
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
        sequences.push(sequence(&unsafe { (*entry).label_str() }, duration));
    }
    Some(Arc::from(sequences))
}

fn sequence(label: &str, duration: f64) -> Sequence {
    Sequence {
        label: Arc::from(label),
        lower: label.to_lowercase().into_boxed_str(),
        duration,
    }
}

/// Every sequence in `model`, or an empty list when it cannot be read.
pub(super) fn model_sequences(model: *mut ModelSPartial) -> SequenceInfo {
    with_sequences(model, Arc::clone)
}

/// How long one of a model's sequences runs, in seconds. Zero when the model
/// or index cannot be read, or the header's numbers are not credible.
pub(super) fn model_sequence_duration(model: *mut ModelSPartial, sequence: i32) -> f64 {
    if sequence < 0 {
        return 0.0;
    }
    with_sequences(model, |sequences| {
        sequences
            .get(sequence as usize)
            .map(|s| s.duration)
            .unwrap_or(0.0)
    })
}

/// What one of a model's sequences is called, or `None` when the model or
/// index cannot be read. `hand_signals` asks whether a label starts with
/// `hs_`, and `apply()` reads the spectated player's body label, both every
/// frame -- so this hands back the cached label itself, not a copy of it.
pub(crate) fn sequence_label(model: *mut ModelSPartial, index: usize) -> Option<Arc<str>> {
    with_sequences(model, |sequences| {
        sequences.get(index).map(|s| Arc::clone(&s.label))
    })
}

/// `s.to_lowercase()`, borrowed when that would change nothing -- as it does
/// for every candidate label this module is actually given.
fn lowercase(s: &str) -> Cow<'_, str> {
    if s.bytes().any(|b| !b.is_ascii() || b.is_ascii_uppercase()) {
        Cow::Owned(s.to_lowercase())
    } else {
        Cow::Borrowed(s)
    }
}

fn apply_deploy_state_to_sequence(
    sequence: i32,
    state: Option<DeployState>,
    sequences: &[Sequence],
) -> i32 {
    let Some(state) = state else { return sequence };
    let Some(current) = sequences.get(sequence.max(0) as usize) else {
        return sequence;
    };

    let wanted = swap_family_prefix(&current.label, state);
    sequences
        .iter()
        .position(|s| s.label.eq_ignore_ascii_case(&wanted))
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
    let sequences = model_sequences(viewmodel);

    // Exact match first, substring only as a fallback. Several models list a
    // qualified variant *before* the plain one -- v_luger.mdl is
    // [.., 5:reload_empty, 6:reload, ..] -- so a substring-first search picks
    // the wrong animation, which is exactly what made a luger reload play as
    // reload_empty in testing. The fallback still matters, because the bipod
    // weapons have no bare label at all: v_bar.mdl is up_reload / down_reload,
    // v_mg42.mdl is upshoot / downshoot.
    for candidate in candidates {
        let needle = lowercase(candidate);
        if let Some(i) = sequences.iter().position(|s| *s.lower == *needle) {
            return apply_deploy_state_to_sequence(i as i32, state, &sequences);
        }
    }
    for candidate in candidates {
        let needle = lowercase(candidate);
        if let Some(i) = sequences.iter().position(|s| s.lower.contains(&*needle)) {
            return apply_deploy_state_to_sequence(i as i32, state, &sequences);
        }
    }
    -1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim_fix::classify::ATTACK_SEQUENCES;

    /// Puts `labels` in the cache as `model`'s sequences, as if read from its
    /// header, and returns the model. Each test uses its own address, since
    /// the cache is shared by every test in the crate.
    fn seed(address: usize, labels: &[&str]) -> *mut ModelSPartial {
        let model = std::ptr::without_provenance_mut::<ModelSPartial>(address);
        let sequences: Vec<Sequence> = labels
            .iter()
            .enumerate()
            .map(|(i, label)| sequence(label, i as f64 * 0.5))
            .collect();
        SEQUENCE_CACHE
            .lock()
            .unwrap()
            .get_or_insert_with(HashMap::new)
            .insert(address, Arc::from(sequences));
        model
    }

    // Real sequence lists, read out of the `.mdl` files shipped with DoD 1.3.
    const V_LUGER: &[&str] = &[
        "idle",
        "idle",
        "idle",
        "shoot",
        "shoot_empty",
        "reload_empty",
        "reload",
        "draw",
        "empty_idle",
    ];
    const V_BAR: &[&str] = &[
        "up_idle",
        "up_reload",
        "up_draw",
        "up_shoot",
        "up_to_down",
        "down_idle",
        "down_reload",
        "down_shoot",
        "down_to_up",
    ];
    const V_STICK: &[&str] = &[
        "idle",
        "draw",
        "pinpull",
        "holster",
        "throw",
        "exploding_idle",
        "exploding_draw",
        "exploding_pinpull",
        "exploding_throw",
    ];

    /// The point of the shared list: a second lookup is the same allocation
    /// as the first, and a label handed out is the cached one, not a copy.
    #[test]
    fn a_cache_hit_shares_the_list_instead_of_copying_it() {
        let model = seed(0x5e9_0001, V_LUGER);
        let first = model_sequences(model);
        let second = model_sequences(model);
        assert!(Arc::ptr_eq(&first, &second));

        let label = sequence_label(model, 6).unwrap();
        assert_eq!(&*label, "reload");
        assert!(Arc::ptr_eq(&label, &first[6].label));
    }

    #[test]
    fn labels_and_durations_are_read_by_index() {
        let model = seed(0x5e9_0002, V_STICK);
        assert_eq!(sequence_label(model, 4).as_deref(), Some("throw"));
        assert_eq!(sequence_label(model, V_STICK.len()), None);
        assert_eq!(model_sequence_duration(model, 4), 2.0);
        assert_eq!(model_sequence_duration(model, V_STICK.len() as i32), 0.0);
        assert_eq!(model_sequence_duration(model, -1), 0.0);
    }

    /// A model the engine cannot hand over yet reads as empty and is not
    /// cached, so it is asked about again once it can be.
    #[test]
    fn an_unreadable_model_is_empty_and_not_cached() {
        let address = 0x5e9_0003;
        let model = std::ptr::without_provenance_mut::<ModelSPartial>(address);
        assert!(model_sequences(model).is_empty());
        assert_eq!(sequence_label(model, 0), None);
        assert_eq!(animation_lookup_sequence("draw", None, model), -1);
        let cache = SEQUENCE_CACHE.lock().unwrap();
        assert!(cache.as_ref().is_none_or(|c| !c.contains_key(&address)));
    }

    /// The v_luger case the exact-first order exists for.
    #[test]
    fn an_exact_label_beats_an_earlier_substring_match() {
        let model = seed(0x5e9_0004, V_LUGER);
        assert_eq!(animation_lookup_sequence("reload", None, model), 6);
        assert_eq!(animation_lookup_sequence("draw", None, model), 7);
        assert_eq!(animation_lookup_any(ATTACK_SEQUENCES, None, model), 3);
        assert_eq!(animation_lookup_sequence("holster", None, model), -1);
    }

    /// The bipod weapons have no bare labels, so the substring fallback finds
    /// them, and the deploy state then picks the family.
    #[test]
    fn prefixed_labels_are_found_and_moved_to_the_deploy_family() {
        let model = seed(0x5e9_0005, V_BAR);
        assert_eq!(animation_lookup_sequence("reload", None, model), 1);
        assert_eq!(
            animation_lookup_sequence("reload", Some(DeployState::Up), model),
            1
        );
        assert_eq!(
            animation_lookup_sequence("reload", Some(DeployState::Down), model),
            6
        );
        assert_eq!(
            animation_lookup_any(ATTACK_SEQUENCES, Some(DeployState::Down), model),
            7
        );
        assert_eq!(
            animation_lookup_sequence("idle", Some(DeployState::Down), model),
            5
        );
    }

    /// v_mg42's labels have no underscore after the family, and a single
    /// shared "reload" with no family at all, which stays where it is.
    #[test]
    fn mg42_families_and_its_shared_reload() {
        let model = seed(
            0x5e9_0006,
            &["upidle", "downidle", "upshoot", "downshoot", "reload"],
        );
        assert_eq!(
            animation_lookup_any(ATTACK_SEQUENCES, Some(DeployState::Down), model),
            3
        );
        assert_eq!(
            animation_lookup_sequence("reload", Some(DeployState::Down), model),
            4
        );
    }

    #[test]
    fn lookups_ignore_case_on_both_sides() {
        let model = seed(0x5e9_0007, &["IDLE", "Up_Shoot", "Down_Shoot"]);
        assert_eq!(animation_lookup_sequence("idle", None, model), 0);
        assert_eq!(animation_lookup_sequence("Idle", None, model), 0);
        assert_eq!(animation_lookup_sequence("SHOOT", None, model), 1);
        assert_eq!(
            animation_lookup_sequence("shoot", Some(DeployState::Down), model),
            2
        );
    }

    #[test]
    fn lowercase_only_allocates_when_it_changes_something() {
        assert!(matches!(lowercase("shoot"), Cow::Borrowed("shoot")));
        assert!(matches!(lowercase("slash1"), Cow::Borrowed(_)));
        assert_eq!(lowercase("Up_Shoot"), "up_shoot");
        assert_eq!(lowercase("ÜBER"), "über");
    }
}
