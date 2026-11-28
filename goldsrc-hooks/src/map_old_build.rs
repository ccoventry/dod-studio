//! `ds_map_fix_old_build` (on by default): an HLTV demo recorded on an older
//! build of its map draws its brush entities with the pieces they were
//! spawned with, so it shows no black boxes or solid water where the map
//! changed (#1540).
//!
//! HLTV demos carry no map checksum the engine checks, so such a demo plays on
//! the installed map, and when the map's piece numbering changed between the
//! builds the demo's `*N` models name the wrong pieces: its triggers' black
//! render settings land on visible walls, its water on sandbag triggers. Each
//! brush entity's spawn baseline still says which piece it means, by its
//! bounds; `dod::brush_pieces` holds the rules and the evidence. A demo on the
//! map's own build is left exactly as it is: every entity fits its own piece.
//!
//! ## Where
//!
//! `client.dll`'s `HUD_AddEntity` (`engine/client_exports.rs`), which the
//! engine calls for each entity it is about to draw. For a brush entity whose
//! piece is wrong, [`before_add_entity`] puts the right piece's `model_t` in
//! `cl_entity_t.model`, so the renderer draws that piece with the demo's own
//! render mode, amount and skin; for one that fits no piece of today's map it
//! returns "don't draw". The engine sets `model` from the model index again
//! as each packet arrives (pre-Anniversary `hw.dll+0x11fb0`, called for every
//! entity in the packet), so the swap is made on every frame, which also
//! means turning the setting off shows the demo's own pieces from the next
//! packet. No code is patched.
//!
//! ## When
//!
//! The decisions are made once per demo, on the first brush entity drawn: the
//! map's pieces from the world model the engine loaded (`world.rs`), the
//! demo's model list, and every entity's spawn baseline. A new demo is noticed
//! by its name (each frame, in [`poll`]) or by any entity's baseline no longer
//! being the one decided on. Per entity per frame after that: one world-model
//! read, one table look-up, and for a remapped entity one pointer write.
//!
//! The hook log says per demo how many brush entities fit their own piece,
//! were drawn with another, were hidden, or were left as they are;
//! `ds_debug_status` adds how many hidden ones the engine tried to draw.

mod world;

use std::cell::RefCell;
use std::ffi::c_char;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU32, Ordering};

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
#[derive(Clone, Copy)]
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

thread_local! {
    /// `HUD_AddEntity`, `poll` and the console all run on the engine's
    /// thread, so the decisions need no lock.
    static STATE: RefCell<State> = const { RefCell::new(State::Stale) };
    /// The demo name the decisions were last made for, from `demo_reload`.
    static DEMO: RefCell<Option<String>> = const { RefCell::new(None) };
}

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
/// `entity` is null or the engine's own `cl_entity_t` for this call, on the
/// engine's thread.
pub unsafe fn before_add_entity(entity: *mut ClEntityS) -> Redirect {
    if entity.is_null() || !SETTING.on() {
        return Redirect::Keep;
    }
    // Safety: the caller's contract.
    let entity = unsafe { &mut *entity };
    let Ok(index) = usize::try_from(entity.index) else {
        return Redirect::Keep;
    };
    STATE.with(|cell| {
        let Ok(mut state) = cell.try_borrow_mut() else {
            return Redirect::Keep;
        };
        let world = world::world_model();
        let fingerprint = world::fingerprint(&entity.baseline);
        if needs_deciding(&state, world, index, &fingerprint) {
            *state = decide(world);
        }
        match &mut *state {
            State::Ready(plan) => plan.apply(index, entity),
            _ => Redirect::Keep,
        }
    })
}

/// Whether `state` no longer holds for the world and this entity's baseline.
fn needs_deciding(state: &State, world: usize, index: usize, fingerprint: &Fingerprint) -> bool {
    match state {
        State::Stale => true,
        State::NotYet { at } => *at != crate::engine::client_time().to_bits(),
        State::Unusable { world: was } => *was != world,
        State::Ready(plan) => {
            plan.world != world
                || plan
                    .entries
                    .get(index)
                    .is_some_and(|entry| entry.fingerprint != *fingerprint)
        }
    }
}

/// Reads the map and the demo and decides every brush entity, saying so in
/// the hook log.
fn decide(world: usize) -> State {
    let started = std::time::Instant::now();
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

    let mut actions = plan.actions.iter();
    let mut not_in_list = 0;
    let entries = baselines
        .iter()
        .map(|baseline| {
            let action = match baseline.spawned {
                Some(_) => actions.next().copied().unwrap_or(Action::Keep),
                None => Action::Keep,
            };
            let applied = match action {
                Action::Keep => Applied::Keep,
                Action::Draw(n) => match models.model_of(n) {
                    Some(model) => Applied::Draw(model),
                    None => {
                        not_in_list += 1;
                        Applied::Hide { seen: false }
                    }
                },
                Action::Hide => Applied::Hide { seen: false },
            };
            Entry {
                fingerprint: baseline.fingerprint,
                applied,
            }
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
    State::Ready(Plan { world, entries })
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

/// Notices a new demo by its name, so the next brush entity drawn decides
/// again and the log has a line for every demo. Every frame, on the engine
/// thread.
pub fn poll() {
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

/// For `ds_debug_status`: the current demo's line, and how many hidden
/// entities the engine has tried to draw.
pub fn status() -> String {
    let line = read_or_recover(&SUMMARY).clone();
    if line.is_empty() {
        return "no brush entity drawn yet".into();
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

    #[test]
    fn a_stale_state_is_decided_and_a_ready_one_only_for_a_new_baseline() {
        let fp = Fingerprint::default();
        assert!(needs_deciding(&State::Stale, 1, 0, &fp));
        let ready = State::Ready(Plan {
            world: 0x1000,
            entries: vec![Entry {
                fingerprint: fp,
                applied: Applied::Keep,
            }],
        });
        assert!(!needs_deciding(&ready, 0x1000, 0, &fp));
        // Past the entities read: nothing to compare, nothing to redo.
        assert!(!needs_deciding(&ready, 0x1000, 5, &fp));
        assert!(needs_deciding(&ready, 0x2000, 0, &fp), "another world");
        let other = Fingerprint {
            modelindex: 7,
            ..Fingerprint::default()
        };
        assert!(needs_deciding(&ready, 0x1000, 0, &other), "a new baseline");
        assert!(!needs_deciding(
            &State::Unusable { world: 0x1000 },
            0x1000,
            0,
            &fp
        ));
        assert!(needs_deciding(
            &State::Unusable { world: 0x1000 },
            0x2000,
            0,
            &fp
        ));
    }
}
