//! Which of a map's own sounds are its victory music and which its
//! flag-capture announcements, read from the map's entities (#284).
//!
//! Shared by the demo patcher (`native::patch::sound_mute`), the offline
//! survey (`native/examples/map_sound_table.rs`) and the hook DLL, which
//! classifies each map as it loads (`goldsrc-hooks/src/map_sounds.rs`). Pure,
//! no I/O.
//!
//! ## Why the names come from the map, not from a list
//!
//! A hardcoded filename list looked reasonable until it was measured against
//! the 219 maps installed here. Two things it gets wrong:
//!
//! - It misses sounds nobody thought of. `ambience/gerareasecure.wav` and
//!   `usareasecure.wav` are the second commonest capture pair (193 and 174
//!   maps) and were in neither issue's list -- and they are the ones the demo
//!   library actually hits.
//! - Matching `win` as a substring silences `ambience/windy.wav`, which 28
//!   maps use as placed atmosphere. Muting a map's wind is precisely the
//!   opposite of what was asked for.
//!
//! So the selection is structural instead. Capture sounds are whatever string
//! sits in a capture entity's `*sound*` keyvalue. Win music is found by
//! following the map's own wiring: `dod_control_point_master` names the
//! targetname it fires per team (`allies_capture_target` /
//! `axis_capture_target`, present in 212 maps each) and `dod_round_timer`
//! names the stalemate one, so the sound is the `message` of whatever
//! `ambient_generic` answers to those names. Measured over the 211 maps
//! carrying such a master: 204 resolve to a wav, 201 of them directly and 3
//! through one `trigger_relay` / `multi_manager` hop, and 7 maps simply have
//! no win sound to mute.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::map_entities::MapEntity;

/// Classnames whose `*sound*` keyvalues name a flag-capture sound.
///
/// `dod_object` is deliberately not here. Its `object_takesound` /
/// `object_dropsound` fire when someone picks up or drops an objective, which
/// is a different event from a point changing hands and is not what was asked
/// for.
const CAPTURE_SOUND_CLASSES: [&str; 2] = ["dod_control_point", "dod_capture_area"];

/// Entities a round-win target may pass through on its way to the
/// `ambient_generic`.
const RELAY_CLASSES: [&str; 2] = ["trigger_relay", "multi_manager"];

/// Keys on a `multi_manager` that are its own settings rather than a target.
const MULTI_MANAGER_META: [&str; 6] = [
    "classname",
    "targetname",
    "origin",
    "angles",
    "spawnflags",
    "wait",
];

/// How many hops from the master a win sound may be. Measured: 3 of 204 maps
/// need one; none needs two.
const MAX_RELAY_DEPTH: usize = 1;

/// Which families of the map's own sounds to silence.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MuteSelection {
    /// The sound a control point plays when it changes hands.
    pub capture: bool,
    /// The music the map plays when a round ends.
    pub round_win: bool,
}

impl MuteSelection {
    pub fn any(&self) -> bool {
        self.capture || self.round_win
    }
}

/// The sample names one map plays for each family, [`normalise`]d.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MapSounds {
    pub capture: BTreeSet<String>,
    pub round_win: BTreeSet<String>,
}

impl MapSounds {
    /// The names `selection` asks to silence.
    pub fn selected(&self, selection: MuteSelection) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        if selection.capture {
            out.extend(self.capture.iter().cloned());
        }
        if selection.round_win {
            out.extend(self.round_win.iter().cloned());
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.capture.is_empty() && self.round_win.is_empty()
    }
}

/// Lowercased, forward-slashed, trimmed: the form both a BSP keyvalue and a
/// demo precache entry are compared in. Maps disagree on both counts --
/// `GsG5/uspointcaptured.wav` is shipped with that capitalisation.
pub fn normalise(name: &str) -> String {
    name.trim_matches(|c: char| c == '\0' || c.is_whitespace())
        .replace('\\', "/")
        .to_ascii_lowercase()
}

fn is_wav(value: &str) -> bool {
    normalise(value).ends_with(".wav")
}

/// Every sound name this map plays for a capture or a round win.
pub fn map_sounds(entities: &[MapEntity]) -> MapSounds {
    let mut sounds = MapSounds::default();

    for entity in entities {
        if !CAPTURE_SOUND_CLASSES.contains(&entity.classname()) {
            continue;
        }
        for (key, value) in &entity.pairs {
            // `dod_capture_area` has a `sounds` key that is an index, not a
            // path, on two of the installed maps; the wav test rejects it.
            if key.to_ascii_lowercase().contains("sound") && is_wav(value) {
                sounds.capture.insert(normalise(value));
            }
        }
    }

    sounds.round_win = round_win_sounds(entities);
    sounds
}

/// Follows the map's own round-end wiring to the sounds it fires.
fn round_win_sounds(entities: &[MapEntity]) -> BTreeSet<String> {
    let mut by_targetname: HashMap<&str, Vec<&MapEntity>> = HashMap::new();
    for entity in entities {
        if let Some(name) = entity.get("targetname") {
            by_targetname.entry(name).or_default().push(entity);
        }
    }

    let mut frontier: Vec<&str> = Vec::new();
    for entity in entities {
        match entity.classname() {
            "dod_control_point_master" => {
                for key in ["allies_capture_target", "axis_capture_target", "target"] {
                    frontier.extend(entity.get(key));
                }
            }
            // The stalemate sound hangs off the round timer rather than the
            // master: `no_winner.wav` on 15 maps, a short "okay... let's do
            // this" played when a round ends with no winner. It is round-end
            // music like the rest, so it counts as victory music (#284).
            "dod_round_timer" => frontier.extend(entity.get("target")),
            _ => {}
        }
    }

    let mut seen: HashSet<&str> = HashSet::new();
    let mut found = BTreeSet::new();
    for depth in 0..=MAX_RELAY_DEPTH {
        let mut next: Vec<&str> = Vec::new();
        for target in frontier.drain(..) {
            if !seen.insert(target) {
                continue;
            }
            for entity in by_targetname.get(target).into_iter().flatten() {
                match entity.classname() {
                    "ambient_generic" => {
                        if let Some(message) = entity.get("message")
                            && is_wav(message)
                        {
                            found.insert(normalise(message));
                        }
                    }
                    class if RELAY_CLASSES.contains(&class) && depth < MAX_RELAY_DEPTH => {
                        next.extend(relay_targets(entity));
                    }
                    _ => {}
                }
            }
        }
        frontier = next;
    }
    found
}

/// What a relay passes the trigger on to. A `trigger_relay` has one `target`;
/// a `multi_manager` names each of its targets as a *key*, with the delay as
/// the value.
fn relay_targets(entity: &MapEntity) -> Vec<&str> {
    if entity.classname() == "trigger_relay" {
        return entity.get("target").into_iter().collect();
    }
    entity
        .pairs
        .iter()
        .filter(|(key, _)| !MULTI_MANAGER_META.contains(&key.as_str()))
        .map(|(key, _)| key.as_str())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map_entities::parse_entity_text;

    /// Anzio's own wiring, trimmed to the keys that matter.
    const ANZIO: &str = r#"
{
"classname" "dod_control_point"
"point_allies_capsound" "ambience/uspointcaptured.wav"
"point_axis_capsound" "ambience/gerpointcaptured.wav"
"point_name" "POINT_ANZIO_PLAZA"
}
{
"classname" "dod_control_point_master"
"allies_capture_target" "allies"
"axis_capture_target" "axis"
}
{
"classname" "ambient_generic"
"targetname" "allies"
"message" "ambience/uswin.wav"
"spawnflags" "49"
}
{
"classname" "ambient_generic"
"targetname" "axis"
"message" "ambience/germanwin.wav"
"spawnflags" "49"
}
{
"classname" "ambient_generic"
"message" "ambience/windy.wav"
"spawnflags" "2"
}
"#;

    fn entities(text: &str) -> Vec<MapEntity> {
        parse_entity_text(text).expect("entity text parses")
    }

    #[test]
    fn reads_capture_sounds_off_the_control_point() {
        let sounds = map_sounds(&entities(ANZIO));
        assert_eq!(
            sounds.capture,
            BTreeSet::from([
                "ambience/gerpointcaptured.wav".to_string(),
                "ambience/uspointcaptured.wav".to_string(),
            ])
        );
    }

    #[test]
    fn follows_the_master_to_the_win_music() {
        let sounds = map_sounds(&entities(ANZIO));
        assert_eq!(
            sounds.round_win,
            BTreeSet::from([
                "ambience/germanwin.wav".to_string(),
                "ambience/uswin.wav".to_string(),
            ])
        );
    }

    /// The real map's wiring (`test-fixtures/dod_anzio_sound_entities.txt`):
    /// the bell, the tree and the scripted gunshots are targeted
    /// `ambient_generic`s too, and none of them is picked.
    #[test]
    fn the_real_anzio_wiring_picks_its_four_sounds() {
        let text = include_str!("../../test-fixtures/dod_anzio_sound_entities.txt");
        let sounds = map_sounds(&entities(text));
        assert_eq!(
            sounds.round_win,
            BTreeSet::from([
                "ambience/germanwin.wav".to_string(),
                "ambience/uswin.wav".to_string(),
            ])
        );
        assert_eq!(
            sounds.capture,
            BTreeSet::from([
                "ambience/gerpointcaptured.wav".to_string(),
                "ambience/uspointcaptured.wav".to_string(),
            ])
        );
    }

    /// The whole reason selection is structural rather than a name match: 28 of
    /// the installed maps play `windy.wav` as placed atmosphere, and it has
    /// nothing to do with winning.
    #[test]
    fn untargeted_ambience_is_not_win_music() {
        let sounds = map_sounds(&entities(ANZIO));
        assert!(!sounds.round_win.contains("ambience/windy.wav"));
        assert!(!sounds.capture.contains("ambience/windy.wav"));
    }

    #[test]
    fn follows_one_relay_hop() {
        let text = r#"
{
"classname" "dod_control_point_master"
"allies_capture_target" "allies_win"
}
{
"classname" "trigger_relay"
"targetname" "allies_win"
"target" "win_sound"
}
{
"classname" "ambient_generic"
"targetname" "win_sound"
"message" "ambience/uswin.wav"
}
"#;
        assert_eq!(
            map_sounds(&entities(text)).round_win,
            BTreeSet::from(["ambience/uswin.wav".to_string()])
        );
    }

    /// One hop is all the installed maps need; a second is not followed.
    #[test]
    fn does_not_follow_a_second_relay_hop() {
        let text = r#"
{
"classname" "dod_control_point_master"
"allies_capture_target" "hop1"
}
{
"classname" "trigger_relay"
"targetname" "hop1"
"target" "hop2"
}
{
"classname" "trigger_relay"
"targetname" "hop2"
"target" "win_sound"
}
{
"classname" "ambient_generic"
"targetname" "win_sound"
"message" "ambience/uswin.wav"
}
"#;
        assert!(map_sounds(&entities(text)).round_win.is_empty());
    }

    /// A relay that targets itself, or two that target each other, ends.
    #[test]
    fn a_relay_loop_ends() {
        let text = r#"
{
"classname" "dod_control_point_master"
"allies_capture_target" "a"
"axis_capture_target" "b"
}
{
"classname" "trigger_relay"
"targetname" "a"
"target" "b"
}
{
"classname" "trigger_relay"
"targetname" "b"
"target" "a"
}
"#;
        assert!(map_sounds(&entities(text)).is_empty());
    }

    #[test]
    fn follows_a_multi_manager_by_its_keys() {
        let text = r#"
{
"classname" "dod_control_point_master"
"axis_capture_target" "axis_win"
}
{
"classname" "multi_manager"
"targetname" "axis_win"
"spawnflags" "0"
"axis_win_sound" "0"
"axis_win_text" "0.5"
}
{
"classname" "ambient_generic"
"targetname" "axis_win_sound"
"message" "ambience/germanwin.wav"
}
"#;
        assert_eq!(
            map_sounds(&entities(text)).round_win,
            BTreeSet::from(["ambience/germanwin.wav".to_string()])
        );
    }

    #[test]
    fn the_round_timer_supplies_the_stalemate_sound() {
        let text = r#"
{
"classname" "dod_round_timer"
"target" "no_winner"
"round_timer_length" "600"
}
{
"classname" "ambient_generic"
"targetname" "no_winner"
"message" "ambience/no_winner.wav"
}
"#;
        assert_eq!(
            map_sounds(&entities(text)).round_win,
            BTreeSet::from(["ambience/no_winner.wav".to_string()])
        );
    }

    /// `dod_capture_area` on two installed maps has `"sounds" "0"`, an index
    /// into a hardcoded table rather than a path.
    #[test]
    fn a_sound_keyvalue_that_is_not_a_path_is_ignored() {
        let text = r#"
{
"classname" "dod_capture_area"
"sounds" "0"
}
"#;
        assert!(map_sounds(&entities(text)).capture.is_empty());
    }

    #[test]
    fn names_are_compared_case_and_slash_insensitively() {
        let text = r#"
{
"classname" "dod_control_point"
"point_allies_capsound" "GsG5\uspointcaptured.WAV"
}
"#;
        assert_eq!(
            map_sounds(&entities(text)).capture,
            BTreeSet::from(["gsg5/uspointcaptured.wav".to_string()])
        );
    }

    #[test]
    fn selection_picks_only_what_was_asked_for() {
        let sounds = map_sounds(&entities(ANZIO));

        assert!(sounds.selected(MuteSelection::default()).is_empty());

        let captures = sounds.selected(MuteSelection {
            capture: true,
            round_win: false,
        });
        assert_eq!(captures.len(), 2);
        assert!(captures.contains("ambience/uspointcaptured.wav"));

        let both = sounds.selected(MuteSelection {
            capture: true,
            round_win: true,
        });
        assert_eq!(both.len(), 4);
    }

    #[test]
    fn a_map_with_no_wiring_yields_nothing_to_mute() {
        let sounds = map_sounds(&entities(r#"{ "classname" "worldspawn" }"#));
        assert!(sounds.is_empty());
        assert!(
            sounds
                .selected(MuteSelection {
                    capture: true,
                    round_win: true
                })
                .is_empty()
        );
    }
}
