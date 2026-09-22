// patch/map_text.rs
// Hides a map's own on-screen text in a demo, by the string the map declares.
//
// ── Which channel actually carries it ────────────────────────────────────────
// #287 nominated two: `svc_temp_entity`/`TE_TEXTMESSAGE` (what a `game_text`
// entity sends) and `svc_centerprint`. Measured across all 36 demos in the
// local library, both carry **nothing at all**. Every map-authored line on
// screen arrived as the `HudText` user message:
//
//   MAP_ALLIED_VICTORY2                   `dod_score_ent`'s `message`, at the
//                                         exact frames the win music plays
//   MAP_SPAWN_WARNING                     `env_message`'s `message` -- the
//                                         anzio mortar warning, four times in
//                                         one half
//   "Allies take control over the         a custom map skipping the token and
//    village!"                            putting the English in `message`
//
// `HudText` carries the *token*, not the resolved sentence: the client looks
// `MAP_SPAWN_WARNING` up in `titles.txt` and then in the localisation file.
// Matching therefore compares what the map declares against what the demo
// carries, with no localisation involved on either side -- and it works
// identically for maps that skip the token, because then both sides are the
// English.
//
// ── Why not just suppress the channel ────────────────────────────────────────
// #287 asked whether to suppress all of it or only the map's own. The
// measurement answers that: DoD's own clan-match prompts (`#Clan_allies_ready`,
// `#Clan_axis_ready`) share `HudText`, and they appear in nearly every match
// demo. Blanking the channel would take those with it. Selecting by the
// strings the map declares separates the two exactly, and needs no list of
// DoD's own messages to keep in step with the game.
//
// ── What gets nopped ─────────────────────────────────────────────────────────
// A `HudText` is a user message, not an engine one, so the whole `NetMessage`
// is replaced with `SvcNop` rather than the inner payload being edited. Same
// move as `sound_mute` and `decal_strip`: one byte on the wire, and
// `demo_writer` recomputes payload lengths.

use std::collections::BTreeSet;

use dem::types::{Demo, EngineMessage, FrameData, MessageData, NetMessage};
use dod::UserMessage;

use super::bsp_entities::MapEntity;

/// The user message DoD puts map text on. Compared against
/// `dem::types::UserMessage::name`, which is NUL-padded to 16 bytes.
const HUD_TEXT: &str = "HudText";

/// Entities whose `message` is the round result: the line that goes up with the
/// win music.
const RESULT_CLASSES: [&str; 1] = ["dod_score_ent"];

/// Entities whose `message` is a hint or warning the map fires at a trigger --
/// the anzio mortar warning is an `env_message`. `game_text` is here for
/// completeness; no demo in the library carries one, and if that ever changes
/// it arrives on a different message this pass does not touch.
const HINT_CLASSES: [&str; 2] = ["env_message", "game_text"];

/// Which of the map's on-screen lines to hide.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TextSelection {
    /// "The Allies have secured the town!" and friends, at round end.
    pub round_result: bool,
    /// Warnings and hints the map fires at a trigger, such as the anzio mortar
    /// warning near a spawn exit.
    pub hints: bool,
}

impl TextSelection {
    pub fn any(&self) -> bool {
        self.round_result || self.hints
    }
}

/// The strings one map declares, split the way [`TextSelection`] asks about
/// them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MapText {
    pub round_result: BTreeSet<String>,
    pub hints: BTreeSet<String>,
}

impl MapText {
    pub fn selected(&self, selection: TextSelection) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        if selection.round_result {
            out.extend(self.round_result.iter().cloned());
        }
        if selection.hints {
            out.extend(self.hints.iter().cloned());
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.round_result.is_empty() && self.hints.is_empty()
    }
}

/// What a hide pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextStats {
    /// `HudText` messages nopped.
    pub hidden: usize,
    /// Which of the selected strings the demo actually carried.
    pub matched: BTreeSet<String>,
}

/// Trimmed of the NULs and padding both a keyvalue and a wire string may carry.
/// Case is *not* folded: these are looked up verbatim in `titles.txt`, so two
/// strings differing in case are two different lookups.
fn clean(text: &str) -> String {
    text.trim_matches(|c: char| c == '\0' || c.is_whitespace())
        .to_string()
}

/// Every on-screen string this map declares.
pub fn map_text(entities: &[MapEntity]) -> MapText {
    let mut text = MapText::default();
    for entity in entities {
        let Some(message) = entity.get("message").map(clean) else {
            continue;
        };
        if message.is_empty() {
            continue;
        }
        // `env_message`'s `message` is a title token; `ambient_generic` uses the
        // same key for a wav path, which is why the classname decides rather
        // than the key.
        if RESULT_CLASSES.contains(&entity.classname()) {
            text.round_result.insert(message);
        } else if HINT_CLASSES.contains(&entity.classname()) {
            text.hints.insert(message);
        }
    }
    text
}

/// Replaces every `HudText` carrying one of `strings` with `SvcNop`.
pub fn hide_map_text(demo: &mut Demo, strings: &BTreeSet<String>) -> TextStats {
    let mut stats = TextStats::default();
    if strings.is_empty() {
        return stats;
    }

    for entry in &mut demo.directory.entries {
        for frame in &mut entry.frames {
            let FrameData::NetworkMessage(boxed) = &mut frame.frame_data else {
                continue;
            };
            let MessageData::Parsed(messages) = &mut boxed.1.messages else {
                continue;
            };
            for message in messages.iter_mut() {
                let NetMessage::UserMessage(user) = message else {
                    continue;
                };
                if clean(&String::from_utf8_lossy(&user.name)) != HUD_TEXT {
                    continue;
                }
                let Ok(UserMessage::HudText(text)) = UserMessage::new(&user.name, &user.data)
                else {
                    continue;
                };
                let text = clean(&text.text);
                if !strings.contains(&text) {
                    continue;
                }
                stats.hidden += 1;
                stats.matched.insert(text);
                *message = NetMessage::EngineMessage(Box::new(EngineMessage::SvcNop));
            }
        }
    }

    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::bsp_entities::parse_entity_text;

    /// Anzio's text entities, verbatim but trimmed to the keys that matter.
    const ANZIO: &str = r#"
{
"classname" "dod_score_ent"
"targetname" "allies"
"message" "MAP_ALLIED_VICTORY2"
"team" "1"
}
{
"classname" "dod_score_ent"
"targetname" "axis"
"message" "MAP_AXIS_VICTORY2"
"team" "2"
}
{
"classname" "env_message"
"targetname" "msg_leave_axis"
"message" "MAP_SPAWN_WARNING"
"spawnflags" "16"
}
{
"classname" "ambient_generic"
"targetname" "allies"
"message" "ambience/uswin.wav"
}
"#;

    fn entities(text: &str) -> Vec<MapEntity> {
        parse_entity_text(text).expect("entity text parses")
    }

    #[test]
    fn splits_round_results_from_hints() {
        let text = map_text(&entities(ANZIO));
        assert_eq!(
            text.round_result,
            BTreeSet::from([
                "MAP_ALLIED_VICTORY2".to_string(),
                "MAP_AXIS_VICTORY2".to_string(),
            ])
        );
        assert_eq!(
            text.hints,
            BTreeSet::from(["MAP_SPAWN_WARNING".to_string()])
        );
    }

    /// `ambient_generic` uses `message` for a wav path. Keying on the classname
    /// rather than the key is what keeps a sound out of the text list.
    #[test]
    fn an_ambient_generics_message_is_not_on_screen_text() {
        let text = map_text(&entities(ANZIO));
        assert!(!text.round_result.contains("ambience/uswin.wav"));
        assert!(!text.hints.contains("ambience/uswin.wav"));
    }

    #[test]
    fn selection_picks_only_what_was_asked_for() {
        let text = map_text(&entities(ANZIO));
        assert!(text.selected(TextSelection::default()).is_empty());

        let hints = text.selected(TextSelection {
            round_result: false,
            hints: true,
        });
        assert_eq!(hints, BTreeSet::from(["MAP_SPAWN_WARNING".to_string()]));

        let both = text.selected(TextSelection {
            round_result: true,
            hints: true,
        });
        assert_eq!(both.len(), 3);
    }

    /// A custom map that skips the token and writes the sentence straight into
    /// `message` -- `chain_01.dem` carries exactly this.
    #[test]
    fn a_literal_sentence_is_selected_the_same_way() {
        let text = map_text(&entities(
            r#"{ "classname" "dod_score_ent" "message" "Allies take control over the village!" }"#,
        ));
        assert_eq!(
            text.round_result,
            BTreeSet::from(["Allies take control over the village!".to_string()])
        );
    }

    #[test]
    fn an_empty_message_is_not_a_string_to_hide() {
        let text = map_text(&entities(r#"{ "classname" "env_message" "message" "" }"#));
        assert!(text.is_empty());
    }

    /// DoD's own clan-match prompts share the `HudText` channel. Nothing in a
    /// map declares them, so a structural selection cannot pick them up -- this
    /// is the test that says so on purpose.
    #[test]
    fn dods_own_prompts_are_never_selected() {
        let text = map_text(&entities(ANZIO));
        let all = text.selected(TextSelection {
            round_result: true,
            hints: true,
        });
        assert!(!all.contains("#Clan_allies_ready"));
        assert!(!all.contains("#Clan_axis_ready"));
    }
}
