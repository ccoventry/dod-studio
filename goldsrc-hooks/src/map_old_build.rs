//! `ds_map_fix_old_build` (on by default): a demo recorded on an older build
//! of its map draws its brush entities with the pieces they were spawned
//! with, so it shows no black boxes or solid water where the map changed
//! (#1540).
//!
//! HLTV demos carry no map checksum the engine checks, so such a demo plays on
//! the installed map, and when the map's piece numbering changed between the
//! builds the demo's `*N` models name the wrong pieces: its triggers' black
//! render settings land on visible walls, its water on sandbag triggers. Each
//! brush entity's spawn baseline still says which piece it means, by its
//! bounds; `dod::brush_pieces` holds the rules and the evidence. A demo on the
//! map's own build is left exactly as it is: every entity fits its own piece.
//! POV and HLTV demos are treated alike; live play is never touched, and only
//! two or more entities fitting a piece at another number count as another
//! build (`brush_pieces::MIN_RENUMBERED`).
//!
//! Only drawing changes. The chase camera's and the client's own traces still
//! collide with the pieces the demo names, and changed world geometry (`*0`)
//! is today's map's: players may clip into or float over it.
//!
//! ## Where
//!
//! `client.dll`'s `HUD_AddEntity` (`engine/client_exports.rs`), which the
//! engine calls for each entity it is about to draw. For a brush entity whose
//! piece is wrong, [`before_add_entity`] puts the right piece's `model_t` in
//! `cl_entity_t.model`, so the renderer draws that piece with the demo's own
//! render mode, amount and skin; for one that fits no piece of today's map it
//! returns "don't draw". The engine sets `model` from the model index again
//! as each packet arrives (`world.rs` says where), so the swap is made on
//! every frame, and turning the setting off shows the demo's own pieces from
//! the next packet. Only the engine's own entity slots are changed, never a
//! copy the client made. No code is patched.
//!
//! ## When
//!
//! The decisions are made once per demo, on the first brush entity drawn: the
//! map's pieces from the world model the engine loaded (`world.rs`), the
//! demo's model list, and every entity's spawn baseline. A new demo is noticed
//! by its name ([`poll`], twice a second while a demo plays), by a new world
//! model, or by an entity's baseline no longer being the one decided on; that
//! last one decides again at most once per demo and world, so a baseline that
//! keeps changing can't make every frame decide. Per entity per frame after
//! that: two `GetEntityByIndex` calls, a 40-byte compare and a table look-up,
//! and for a remapped entity one pointer write.
//!
//! The hook log says per demo how many brush entities fit their own piece,
//! were drawn with another, were hidden, or were left as they are;
//! `ds_debug_status` adds how many hidden ones the engine tried to draw.

mod world;

use std::cell::{Cell, RefCell};
use std::ffi::c_char;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use dod::brush_pieces::{self, Action, Counts};
use lock_recover::{read_or_recover, write_or_recover};

use crate::engine::{ClEntityS, ModelSPartial};
use crate::names::console_name;
use crate::setting::Setting;
use world::{Fingerprint, NotRead};

pub const NAME: &str = console_name!("map_fix_old_build");

/// The switch, or its default (on) when the cvar could not be registered: a
/// demo on the map's own build is never changed, so it is safe for everyone.
pub static SETTING: Setting = Setting::new(NAME, true);

/// How often [`poll`] reads the playing demo's name: a new demo's line comes
/// at most this late, and its decisions are already right when its world or
/// baselines differ.
const DEMO_CHECK_INTERVAL: Duration = Duration::from_millis(500);

/// What `HUD_AddEntity`'s detour does with one entity.
pub enum Redirect {
    /// Draw it as the engine set it up.
    Keep,
    /// Its model is now this piece: pass the piece's name to the client.
    Draw(*const c_char),
    /// Don't draw it.
    Hide,
}

/// What one entity slot gets when it is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Applied {
    Keep,
    /// Draw with this `model_t`.
    Draw(usize),
    /// Hide; `seen` once the engine has tried to draw it this demo.
    Hide {
        seen: bool,
    },
}

struct Entry {
    fingerprint: Fingerprint,
    applied: Applied,
}

/// The current demo's decisions.
struct Plan {
    /// The world model they were made on.
    world: usize,
    /// By entity index.
    entries: Vec<Entry>,
    /// Whether these were made again because a baseline changed: then a
    /// further change leaves that entity as the demo says, rather than
    /// deciding again.
    after_baseline_change: bool,
}

impl Plan {
    /// Applies entity `index`'s decision to `entity`, unless its model has
    /// changed since its baseline.
    fn apply(&mut self, index: usize, entity: &mut ClEntityS) -> Redirect {
        let Some(entry) = self.entries.get_mut(index) else {
            return Redirect::Keep;
        };
        if entity.curstate.modelindex != entry.fingerprint.modelindex {
            return Redirect::Keep;
        }
        match &mut entry.applied {
            Applied::Keep => Redirect::Keep,
            Applied::Draw(model) => {
                entity.model = *model as *mut ModelSPartial;
                // `name` is `model_t`'s first field.
                Redirect::Draw(*model as *const c_char)
            }
            Applied::Hide { seen } => {
                if !*seen {
                    *seen = true;
                    HIDDEN_DRAWN.fetch_add(1, Ordering::Relaxed);
                }
                Redirect::Hide
            }
        }
    }
}

enum State {
    /// Nothing decided since the last demo change.
    Stale,
    /// The map was still loading at this playback time (`client_time`'s
    /// bits): tried again on a later frame.
    NotYet {
        at: u64,
    },
    /// This world can't be read: left alone until another loads.
    Unusable {
        world: usize,
    },
    Ready(Plan),
}

/// What [`before_add_entity`] does before applying the plan.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    /// The plan holds: apply it.
    Apply,
    /// Decide the demo's pieces.
    Decide,
    /// Decide again because this entity's baseline changed.
    DecideAfterBaselineChange,
    /// The baseline changed again since the last re-decision: leave the
    /// entity as the demo says.
    Skip,
}

thread_local! {
    /// `HUD_AddEntity`, `poll` and the console all run on the engine's
    /// thread, so the decisions need no lock.
    static STATE: RefCell<State> = const { RefCell::new(State::Stale) };
    /// The demo name the decisions were last made for, from `demo_reload`.
    static DEMO: RefCell<Option<String>> = const { RefCell::new(None) };
    /// When [`poll`] last read the demo name.
    static DEMO_CHECKED: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// Whether a demo is playing, as [`poll`] read it this frame: live play is
/// never changed.
static PLAYING: AtomicBool = AtomicBool::new(false);
/// Hidden entities the engine has tried to draw this demo, each once.
static HIDDEN_DRAWN: AtomicU32 = AtomicU32::new(0);
/// The current demo's line, for `ds_debug_status`.
static SUMMARY: RwLock<String> = RwLock::new(String::new());

/// Called from `HUD_AddEntity`'s detour for an entity whose model is a brush
/// piece (`*N`), before the client sees it. Decides the demo's pieces on
/// first use (see the module doc) and says what to do with this one.
///
/// # Safety
///
/// `entity` is null or a `cl_entity_t` the engine passed for this call, on
/// the engine's thread.
pub unsafe fn before_add_entity(entity: *mut ClEntityS) -> Redirect {
    if entity.is_null() || !is_active() {
        return Redirect::Keep;
    }
    // Safety: the caller's contract.
    let Ok(index) = usize::try_from(unsafe { (*entity).index }) else {
        return Redirect::Keep;
    };
    if !world::is_engine_entity(index, entity) {
        return Redirect::Keep;
    }
    // Safety: the engine's own slot `index`, checked just above.
    let entity = unsafe { &mut *entity };
    STATE.with(|cell| {
        let Ok(mut state) = cell.try_borrow_mut() else {
            return Redirect::Keep;
        };
        let world = world::world_model();
        let fingerprint = world::fingerprint(&entity.baseline);
        match next_step(&state, world, index, &fingerprint) {
            Step::Apply => {}
            Step::Decide => *state = decide(world),
            Step::DecideAfterBaselineChange => {
                *state = decide(world);
                if let State::Ready(plan) = &mut *state {
                    plan.after_baseline_change = true;
                }
            }
            Step::Skip => return Redirect::Keep,
        }
        match &mut *state {
            State::Ready(plan) => plan.apply(index, entity),
            _ => Redirect::Keep,
        }
    })
}

/// Whether the fix may act at all: the setting is on and a demo is playing,
/// so live play is never changed.
fn is_active() -> bool {
    SETTING.on() && PLAYING.load(Ordering::Relaxed)
}

/// Whether `state` still holds for the world and this entity's baseline.
fn next_step(state: &State, world: usize, index: usize, fingerprint: &Fingerprint) -> Step {
    match state {
        State::Stale => Step::Decide,
        State::NotYet { at } if *at != crate::engine::client_time().to_bits() => Step::Decide,
        State::NotYet { .. } => Step::Apply,
        State::Unusable { world: was } if *was != world => Step::Decide,
        State::Unusable { .. } => Step::Apply,
        State::Ready(plan) if plan.world != world => Step::Decide,
        State::Ready(plan) => {
            let changed = plan
                .entries
                .get(index)
                .is_some_and(|entry| entry.fingerprint != *fingerprint);
            match (changed, plan.after_baseline_change) {
                (false, _) => Step::Apply,
                (true, false) => Step::DecideAfterBaselineChange,
                (true, true) => Step::Skip,
            }
        }
    }
}

/// Reads the map and the demo and decides every brush entity, saying so in
/// the hook log.
fn decide(world: usize) -> State {
    let started = Instant::now();
    let Some(level) = crate::map_text::level_name() else {
        return not_yet();
    };
    let map = crate::map_text::short_name(&level).to_string();
    let pieces = match world::read_pieces(world, &level) {
        Ok(pieces) => pieces,
        Err(NotRead::NotYet) => return not_yet(),
        Err(NotRead::Unusable(why)) => return unusable(world, &map, &why),
    };
    let models = match world::read_models(&pieces.pieces) {
        Ok(models) => models,
        Err(why) => return unusable(world, &map, &why),
    };
    let baselines = world::read_baselines(&models);
    let spawned: Vec<_> = baselines.iter().filter_map(|b| b.spawned).collect();
    let plan = brush_pieces::plan(&spawned, &pieces.pieces);
    let is_brush: Vec<bool> = baselines.iter().map(|b| b.spawned.is_some()).collect();
    let (applied, not_in_list) = applied_per_slot(&is_brush, &plan.actions, |n| models.model_of(n));
    let entries = baselines
        .iter()
        .zip(applied)
        .map(|(baseline, applied)| Entry {
            fingerprint: baseline.fingerprint,
            applied,
        })
        .collect();

    HIDDEN_DRAWN.store(0, Ordering::Relaxed);
    let line = summary(
        &map,
        pieces.pieces.len(),
        &plan.counts,
        not_in_list,
        pieces.no_origins.as_deref(),
    );
    report(&format!(
        "{line}, decided in {} ms",
        started.elapsed().as_millis()
    ));
    State::Ready(Plan {
        world,
        entries,
        after_baseline_change: false,
    })
}

/// Each entity slot's decision. `is_brush` says, per slot, whether it spawned
/// a brush entity; `actions` holds one action per such slot, in slot order
/// (`brush_pieces::plan` over them); `model_of` gives the engine's model for
/// a piece, or `None` when the demo's model list doesn't name it, which hides
/// the entity instead. Also returns how many were hidden that way.
fn applied_per_slot(
    is_brush: &[bool],
    actions: &[Action],
    model_of: impl Fn(usize) -> Option<usize>,
) -> (Vec<Applied>, usize) {
    let mut actions = actions.iter();
    let mut not_in_list = 0;
    let applied = is_brush
        .iter()
        .map(|&brush| {
            let action = if brush {
                actions.next().copied().unwrap_or(Action::Keep)
            } else {
                Action::Keep
            };
            match action {
                Action::Keep => Applied::Keep,
                Action::Draw(n) => match model_of(n) {
                    Some(model) => Applied::Draw(model),
                    None => {
                        not_in_list += 1;
                        Applied::Hide { seen: false }
                    }
                },
                Action::Hide => Applied::Hide { seen: false },
            }
        })
        .collect();
    (applied, not_in_list)
}

fn not_yet() -> State {
    State::NotYet {
        at: crate::engine::client_time().to_bits(),
    }
}

fn unusable(world: usize, map: &str, why: &str) -> State {
    report(&format!(
        "{NAME}: {map} can't be read ({why}); its brush entities are drawn as the demo says"
    ));
    State::Unusable { world }
}

fn report(line: &str) {
    *write_or_recover(&SUMMARY) = line.to_string();
    // Safety: plain file append.
    unsafe { crate::debug::report(line) };
}

/// One demo's line for the hook log: counts, never a list.
fn summary(
    map: &str,
    pieces: usize,
    counts: &Counts,
    not_in_list: usize,
    no_origins: Option<&str>,
) -> String {
    let total = counts.named + counts.other + counts.several + counts.nothing + counts.no_bounds;
    // The world is piece 0, not one of the map's brush pieces.
    let pieces = pieces.saturating_sub(1);
    let mut line = if counts.is_other_build() {
        format!(
            "{NAME}: {map} is another build than the demo's: of {total} brush entities, {} fit their own piece, {} are drawn with the piece they fit, {} hidden (no piece of today's map fits them)",
            counts.named,
            counts.other.saturating_sub(not_in_list),
            counts.nothing + not_in_list,
        )
    } else {
        format!(
            "{NAME}: {map} is the demo's build: {} of {total} brush entities fit their own piece, nothing changed",
            counts.named
        )
    };
    line.push_str(&format!(" ({pieces} pieces in the map"));
    if counts.is_other_build() && counts.several > 0 {
        line.push_str(&format!(
            "; {} fit several and are left as they are",
            counts.several
        ));
    }
    if !counts.is_other_build() && counts.other > 0 {
        line.push_str(&format!(
            "; {} fit another piece, too few to be another build",
            counts.other
        ));
    }
    if !counts.is_other_build() && counts.nothing > 0 {
        line.push_str(&format!("; {} fit no piece", counts.nothing));
    }
    if counts.no_bounds > 0 {
        line.push_str(&format!("; {} carry no bounds", counts.no_bounds));
    }
    if not_in_list > 0 {
        line.push_str(&format!(
            "; {not_in_list} of the hidden fit a piece the demo's model list doesn't name"
        ));
    }
    if let Some(why) = no_origins {
        line.push_str(&format!("; no origins: {why}"));
    }
    line.push(')');
    line
}

/// Notes whether a demo is playing, every frame on the engine thread, and
/// while one is, notices a new one by its name, so the next brush entity
/// drawn decides again and the log has a line for every demo. The name is
/// read only every [`DEMO_CHECK_INTERVAL`]: reading it copies it, and before
/// any `playdemo` parses the command line.
pub fn poll() {
    let playing = crate::spray_hltv::playing_demo();
    PLAYING.store(playing, Ordering::Relaxed);
    if !playing || !demo_check_due(Instant::now()) {
        return;
    }
    let demo = crate::demo_reload::current_demo();
    let changed = DEMO.with(|last| {
        let Ok(mut last) = last.try_borrow_mut() else {
            return false;
        };
        if *last == demo {
            return false;
        }
        *last = demo;
        true
    });
    if changed {
        STATE.with(|cell| {
            if let Ok(mut state) = cell.try_borrow_mut() {
                *state = State::Stale;
            }
        });
    }
}

/// Whether the demo name is due a read at `now`, noting it when it is.
fn demo_check_due(now: Instant) -> bool {
    DEMO_CHECKED.with(|checked| {
        let due = checked
            .get()
            .is_none_or(|at| now.saturating_duration_since(at) >= DEMO_CHECK_INTERVAL);
        if due {
            checked.set(Some(now));
        }
        due
    })
}

/// For `ds_debug_status`: the current demo's line, and how many hidden
/// entities the engine has tried to draw.
pub fn status() -> String {
    let line = read_or_recover(&SUMMARY).clone();
    if line.is_empty() {
        return "no brush entity drawn in a demo yet".into();
    }
    format!(
        "{line}; {} hidden reached the renderer",
        HIDDEN_DRAWN.load(Ordering::Relaxed)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(named: usize, other: usize, nothing: usize) -> Counts {
        Counts {
            named,
            other,
            nothing,
            ..Counts::default()
        }
    }

    #[test]
    fn the_log_line_for_another_build_counts_each_outcome() {
        let line = summary("dod_chemille", 186, &counts(112, 58, 14), 0, None);
        assert!(line.contains("another build"), "{line}");
        assert!(line.contains("of 184 brush entities"), "{line}");
        assert!(line.contains("112 fit their own piece"), "{line}");
        assert!(
            line.contains("58 are drawn with the piece they fit"),
            "{line}"
        );
        assert!(line.contains("14 hidden"), "{line}");
        assert!(line.contains("185 pieces in the map"), "{line}");
    }

    #[test]
    fn the_log_line_for_the_maps_own_build_says_nothing_changed() {
        let line = summary("dod_railroad", 115, &counts(114, 0, 0), 0, None);
        assert!(line.contains("is the demo's build"), "{line}");
        assert!(line.contains("114 of 114"), "{line}");
        assert!(line.contains("nothing changed"), "{line}");
    }

    #[test]
    fn the_log_line_names_a_lone_coincidence() {
        let line = summary("dod_x", 10, &counts(8, 1, 0), 0, None);
        assert!(line.contains("nothing changed"), "{line}");
        assert!(line.contains("1 fit another piece, too few"), "{line}");
    }

    #[test]
    fn a_piece_missing_from_the_demos_list_counts_as_hidden() {
        let line = summary("dod_x", 20, &counts(5, 3, 1), 2, None);
        assert!(
            line.contains("1 are drawn with the piece they fit"),
            "{line}"
        );
        assert!(line.contains("3 hidden"), "{line}");
        assert!(line.contains("2 of the hidden"), "{line}");
    }

    #[test]
    fn unreadable_origins_are_named_in_the_line() {
        let line = summary(
            "dod_x",
            3,
            &counts(2, 0, 0),
            0,
            Some("its entity text isn't readable"),
        );
        assert!(line.contains("no origins: its entity text"), "{line}");
    }

    #[test]
    fn on_by_default() {
        assert!(SETTING.on());
    }

    /// A zeroed `cl_entity_t` whose current state names model index `index`.
    fn entity(modelindex: i32) -> Box<ClEntityS> {
        // Safety: plain data and a raw pointer; all zeros is a valid value.
        let mut entity: Box<ClEntityS> = Box::new(unsafe { std::mem::zeroed() });
        entity.curstate.modelindex = modelindex;
        entity
    }

    fn plan_of(applied: Applied) -> Plan {
        Plan {
            world: 0x1000,
            entries: vec![Entry {
                fingerprint: Fingerprint {
                    modelindex: 5,
                    ..Fingerprint::default()
                },
                applied,
            }],
            after_baseline_change: false,
        }
    }

    /// The only test that writes `PLAYING`, so no other test sees it change.
    #[test]
    fn live_play_is_never_changed() {
        // No demo playing (what `poll` reads in live play): inactive, the
        // entity is drawn as the engine set it up, and nothing is decided.
        PLAYING.store(false, Ordering::Relaxed);
        assert!(!is_active());
        let mut entity = entity(5);
        entity.index = 1;
        let redirect = unsafe { before_add_entity(&mut *entity) };
        assert!(matches!(redirect, Redirect::Keep));
        assert!(entity.model.is_null());
        STATE.with(|state| assert!(matches!(*state.borrow(), State::Stale)));
        // A demo playing, with the setting at its default: active.
        PLAYING.store(true, Ordering::Relaxed);
        assert!(is_active());
        PLAYING.store(false, Ordering::Relaxed);
    }

    #[test]
    fn an_entity_whose_model_changed_since_its_baseline_is_kept() {
        let mut plan = plan_of(Applied::Draw(0x1234));
        let mut entity = entity(6);
        assert!(matches!(plan.apply(0, &mut entity), Redirect::Keep));
        assert!(entity.model.is_null());
    }

    #[test]
    fn a_drawn_entity_gets_the_pieces_model() {
        let mut plan = plan_of(Applied::Draw(0x1234));
        let mut entity = entity(5);
        let Redirect::Draw(name) = plan.apply(0, &mut entity) else {
            panic!("not drawn with the piece");
        };
        assert_eq!(name as usize, 0x1234);
        assert_eq!(entity.model as usize, 0x1234);
    }

    #[test]
    fn a_hidden_entity_is_counted_once() {
        let mut plan = plan_of(Applied::Hide { seen: false });
        let mut entity = entity(5);
        let before = HIDDEN_DRAWN.load(Ordering::Relaxed);
        assert!(matches!(plan.apply(0, &mut entity), Redirect::Hide));
        assert!(matches!(plan.apply(0, &mut entity), Redirect::Hide));
        assert_eq!(HIDDEN_DRAWN.load(Ordering::Relaxed), before + 1);
    }

    #[test]
    fn a_slot_past_the_plan_is_kept() {
        let mut plan = plan_of(Applied::Hide { seen: false });
        let mut entity = entity(5);
        assert!(matches!(plan.apply(3, &mut entity), Redirect::Keep));
    }

    #[test]
    fn each_brush_slot_gets_its_own_action_between_other_entities() {
        let is_brush = [false, true, false, false, true, true, false];
        let actions = [Action::Draw(3), Action::Hide, Action::Draw(9)];
        let (applied, not_in_list) =
            applied_per_slot(&is_brush, &actions, |n| (n == 3).then_some(0x30));
        assert_eq!(
            applied,
            vec![
                Applied::Keep,
                Applied::Draw(0x30),
                Applied::Keep,
                Applied::Keep,
                Applied::Hide { seen: false },
                // `*9` isn't in the demo's model list: hidden, and counted.
                Applied::Hide { seen: false },
                Applied::Keep,
            ]
        );
        assert_eq!(not_in_list, 1);
    }

    #[test]
    fn a_stale_state_is_decided_and_a_ready_one_only_for_a_new_world_or_baseline() {
        let fp = Fingerprint::default();
        assert_eq!(next_step(&State::Stale, 1, 0, &fp), Step::Decide);
        let mut plan = plan_of(Applied::Keep);
        plan.entries[0].fingerprint = fp;
        let ready = State::Ready(plan);
        assert_eq!(next_step(&ready, 0x1000, 0, &fp), Step::Apply);
        // Past the entities read: nothing to compare, nothing to redo.
        assert_eq!(next_step(&ready, 0x1000, 5, &fp), Step::Apply);
        assert_eq!(next_step(&ready, 0x2000, 0, &fp), Step::Decide);
        let other = Fingerprint {
            modelindex: 7,
            ..Fingerprint::default()
        };
        assert_eq!(
            next_step(&ready, 0x1000, 0, &other),
            Step::DecideAfterBaselineChange
        );
        let unusable = State::Unusable { world: 0x1000 };
        assert_eq!(next_step(&unusable, 0x1000, 0, &fp), Step::Apply);
        assert_eq!(next_step(&unusable, 0x2000, 0, &fp), Step::Decide);
    }

    #[test]
    fn a_baseline_change_decides_again_only_once_per_demo_and_world() {
        let mut plan = plan_of(Applied::Keep);
        plan.after_baseline_change = true;
        let ready = State::Ready(plan);
        let other = Fingerprint {
            modelindex: 7,
            ..Fingerprint::default()
        };
        assert_eq!(next_step(&ready, 0x1000, 0, &other), Step::Skip);
        // A new world still decides.
        assert_eq!(next_step(&ready, 0x2000, 0, &other), Step::Decide);
    }

    #[test]
    fn the_demo_name_is_read_twice_a_second_at_most() {
        let start = Instant::now();
        DEMO_CHECKED.with(|checked| checked.set(None));
        assert!(demo_check_due(start));
        assert!(!demo_check_due(start + Duration::from_millis(100)));
        assert!(demo_check_due(start + DEMO_CHECK_INTERVAL));
    }
}
