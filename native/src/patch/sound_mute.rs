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
// ── Which names ──────────────────────────────────────────────────────────────
// The map's own wiring picks them, not a list: `dod::map_sounds` follows the
// round-end triggers to the win music and reads the capture entities' sound
// keyvalues. It lives in `dod`, pure, because the hook DLL classifies each map
// the same way as it loads (`goldsrc-hooks/src/map_sounds.rs`); its module doc
// has the measurements that ruled a filename list out.
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

use std::collections::{BTreeSet, HashMap};

use dem::bit::BitSliceCast;
use dem::types::{Demo, EngineMessage, FrameData, MessageData, NetMessage};

pub use dod::map_sounds::{MapSounds, MuteSelection, map_sounds};
use dod::map_sounds::normalise;

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
/// `names` are compared in `normalise`d form; [`map_sounds`] already returns
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
