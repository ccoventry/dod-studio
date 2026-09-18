// patch/sound_mute.rs
// Silences a map's own capture and round-win sounds in a demo, by name.
//
// ── Why this is not a goldsrc-hooks setting ──────────────────────────────────
// #284 and #285 both assumed these sounds could be dropped the way `sound_fix`
// retunes a gunshot: hook `pEventAPI->EV_PlaySound` and match the sample name.
// They cannot. Neither sound is client-side at all -- no capture or win sample
// name appears anywhere in `client.dll` or `dod.dll`, because both are map data
// played by the server:
//
//   point captured   a `dod_control_point` keyvalue (`point_allies_capsound`
//                    / `point_axis_capsound`), reaching the client as
//                    `svc_sound` carrying that control point's entity index.
//
//   round-win music  an `ambient_generic` the map triggers when the round ends,
//                    reaching the client as `svc_spawnstaticsound`.
//
// The event hook sees neither, so the interception has to be the demo itself --
// which this pipeline already rewrites. `decal_strip` established the move:
// replace the unwanted message with `SvcNop`, one byte on the wire, and let
// `demo_writer` recompute payload lengths. No offset arithmetic, and nothing
// injected inside an existing packet.
//
// ── Why the names come from the map, not from a list ─────────────────────────
// A hardcoded filename list looked reasonable until it was measured against the
// 219 maps installed here. Two things it gets wrong:
//
//   * It misses sounds nobody thought of. `ambience/gerareasecure.wav` and
//     `usareasecure.wav` are the second commonest capture pair (193 and 174
//     maps) and were in neither issue's list -- and they are the ones the demo
//     library actually hits.
//   * Matching `win` as a substring silences `ambience/windy.wav`, which 28
//     maps use as placed atmosphere. Muting a map's wind is precisely the
//     opposite of what was asked for.
//
// So the selection is structural instead. Capture sounds are whatever string
// sits in a capture entity's `*sound*` keyvalue. Win music is found by
// following the map's own wiring: `dod_control_point_master` names the
// targetname it fires per team (`allies_capture_target` / `axis_capture_target`,
// present in 212 maps each) and `dod_round_timer` names the stalemate one, so
// the sound is the `message` of whatever `ambient_generic` answers to those
// names. Measured over the 211 maps carrying such a master: 204 resolve to a
// wav, 201 of them directly and 3 through one `trigger_relay` /
// `multi_manager` hop, and 7 maps simply have no win sound to mute.
//
// ── Why the signon block is left alone ───────────────────────────────────────
// `svc_spawnstaticsound` does two unrelated jobs. In the signon block it
// *registers* the ambience a mapper placed in the world; mid-stream it is a
// triggered sound being played. Only position distinguishes them -- measured on
// an anzio half, the placed `mill`/`waves`/`dodambience3` sit in directory
// entry 0 (45 frames) while `uswin.wav` appears in entry 1, at frames 206737
// and 771033, and is absent entirely from the sibling demo recorded on the
// losing side. Structural selection should never pick a placed sound in the
// first place; skipping the signon entry means a map that reuses one wav for
// both cannot turn that mistake into silent atmosphere.
//
// A trigger emits a *pair*: `vol=255 flags=0` to start and `vol=0 flags=32`
// (SND_STOP) to stop. Both are nopped. Nopping only the start would leave a
// stop for a sound that never began -- harmless, but it leaves the demo
// describing something that did not happen.

use std::collections::{BTreeSet, HashMap, HashSet};

use dem::bit::BitSliceCast;
use dem::types::{Demo, EngineMessage, FrameData, MessageData, NetMessage};

use super::bsp_entities::MapEntity;

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
const MULTI_MANAGER_META: [&str; 6] =
    ["classname", "targetname", "origin", "angles", "spawnflags", "wait"];

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

/// The sample names one map plays for each family, normalised for comparison
/// against a demo's precache list.
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

/// What a mute pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MuteStats {
    /// `svc_sound` messages nopped.
    pub sounds: usize,
    /// `svc_spawnstaticsound` messages nopped.
    pub static_sounds: usize,
    /// Selected names the demo's precache list actually carries. A name the map
    /// declares but the demo never precached is not an error -- it means the
    /// sound could not have played -- but an empty set when something was
    /// selected is worth reporting.
    pub matched_names: BTreeSet<String>,
    /// Messages left alone because they were in the signon block.
    pub signon_skipped: usize,
}

impl MuteStats {
    pub fn total(&self) -> usize {
        self.sounds + self.static_sounds
    }
}

/// Lowercased, forward-slashed, trimmed: the form both a BSP keyvalue and a
/// demo precache entry are compared in. Maps disagree on both counts --
/// `GsG5/uspointcaptured.wav` is shipped with that capitalisation.
fn normalise(name: &str) -> String {
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
            // The stalemate sound (`no_winner.wav` on 10 maps) hangs off the
            // round timer rather than the master.
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

/// Precache index -> normalised sound name, from the demo's resource lists.
fn precached_sounds(demo: &Demo) -> HashMap<u32, String> {
    /// `t_sound` in the engine's resource type enum.
    const RESOURCE_TYPE_SOUND: u8 = 0;

    let mut names = HashMap::new();
    for entry in &demo.directory.entries {
        for frame in &entry.frames {
            let FrameData::NetworkMessage(boxed) = &frame.frame_data else {
                continue;
            };
            let MessageData::Parsed(messages) = &boxed.1.messages else {
                continue;
            };
            for message in messages {
                let NetMessage::EngineMessage(engine) = message else {
                    continue;
                };
                if let EngineMessage::SvcResourceList(list) = &**engine {
                    for resource in &list.resources {
                        if resource.type_.to_u8() == RESOURCE_TYPE_SOUND {
                            names.insert(
                                resource.index.to_u32(),
                                normalise(&resource.name.get_string()),
                            );
                        }
                    }
                }
            }
        }
    }
    names
}

/// The precache index a `svc_sound` refers to, if it names one at all.
fn svc_sound_index(sound: &dem::types::SvcSound) -> Option<u32> {
    sound
        .sound_index_long
        .as_ref()
        .map(|bits| bits.to_u32())
        .or_else(|| sound.sound_index_short.as_ref().map(|bits| bits.to_u32()))
}

/// Replaces every `svc_sound` and triggered `svc_spawnstaticsound` naming one of
/// `names` with `SvcNop`.
///
/// `names` are compared in [`normalise`]d form; [`map_sounds`] already returns
/// them that way.
pub fn mute_sounds(demo: &mut Demo, names: &BTreeSet<String>) -> MuteStats {
    let mut stats = MuteStats::default();
    if names.is_empty() {
        return stats;
    }

    let precached = precached_sounds(demo);
    let muted: HashMap<u32, String> = precached
        .into_iter()
        .filter(|(_, name)| names.contains(name))
        .collect();
    if muted.is_empty() {
        return stats;
    }
    stats.matched_names = muted.values().cloned().collect();

    // Entry 0 of a multi-entry demo is the signon block, where
    // `svc_spawnstaticsound` registers placed ambience rather than playing
    // anything. A single-entry demo has no such block, so nothing is skipped.
    let signon_entries = usize::from(demo.directory.entries.len() > 1);

    for (entry_index, entry) in demo.directory.entries.iter_mut().enumerate() {
        let is_signon = entry_index < signon_entries;
        for frame in &mut entry.frames {
            let FrameData::NetworkMessage(boxed) = &mut frame.frame_data else {
                continue;
            };
            let MessageData::Parsed(messages) = &mut boxed.1.messages else {
                continue;
            };
            for message in messages.iter_mut() {
                let NetMessage::EngineMessage(engine) = message else {
                    continue;
                };
                let is_static = match engine.as_ref() {
                    EngineMessage::SvcSound(sound) => match svc_sound_index(sound) {
                        Some(index) if muted.contains_key(&index) => false,
                        _ => continue,
                    },
                    EngineMessage::SvcSpawnStaticSound(sound) => {
                        if !muted.contains_key(&u32::from(sound.sound_index)) {
                            continue;
                        }
                        true
                    }
                    _ => continue,
                };
                if is_signon {
                    stats.signon_skipped += 1;
                    continue;
                }
                if is_static {
                    stats.static_sounds += 1;
                } else {
                    stats.sounds += 1;
                }
                **engine = EngineMessage::SvcNop;
            }
        }
    }

    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::bsp_entities::parse_entity_text;

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

        let captures = sounds.selected(MuteSelection { capture: true, round_win: false });
        assert_eq!(captures.len(), 2);
        assert!(captures.contains("ambience/uspointcaptured.wav"));

        let both = sounds.selected(MuteSelection { capture: true, round_win: true });
        assert_eq!(both.len(), 4);
    }

    #[test]
    fn a_map_with_no_wiring_yields_nothing_to_mute() {
        let sounds = map_sounds(&entities(r#"{ "classname" "worldspawn" }"#));
        assert!(sounds.is_empty());
        assert!(
            sounds
                .selected(MuteSelection { capture: true, round_win: true })
                .is_empty()
        );
    }
}
