//! A GoldSrc BSP's entity lump: the text that says what the map places
//! (spawns, control points, sounds, relays), parsed into key/value pairs.
//!
//! Pure parsing, no I/O, so both sides read a map the same way: the demo
//! patcher and the offline tools (`native::patch::bsp_entities`, which adds
//! the lump rewrite) parse a `.bsp` read from disk, and the hook DLL parses
//! the copy the engine keeps in memory (`model_t.entities`,
//! `goldsrc-hooks/src/map_sounds.rs`).
//!
//! Malformed text never panics: anything outside a `{ }` block is skipped, a
//! `//` comment is skipped as the engine skips it, and a string that never
//! closes is an error, so the caller decides what an unreadable map means.

use std::fmt::Write as _;

/// Lumps in a version 30 BSP header.
pub const LUMP_COUNT: usize = 15;
/// The entity lump's index.
pub const LUMP_ENTITIES: usize = 0;
/// The version word, then `(offset, length)` for each lump.
pub const HEADER_SIZE: usize = 4 + LUMP_COUNT * 8;

/// One entity from the lump: an ordered list of key/value pairs.
///
/// Ordered, and not a map, because the lump is the user's file -- a rewrite that
/// reorders or dedupes keys is a bigger edit than the one being asked for, and
/// duplicate keys are legal (`light` writes several `style` keys in some
/// compilers).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MapEntity {
    pub pairs: Vec<(String, String)>,
}

impl MapEntity {
    /// First value for `key`, or `None`.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn classname(&self) -> &str {
        self.get("classname").unwrap_or("")
    }

    /// The submodel this entity draws, for a brush entity (`"model" "*7"`).
    ///
    /// Brush entities are the ones that matter for a packet-entity budget:
    /// each is exactly one submodel, so the number below is a one-to-one key
    /// between the lump and what a demo carries on the wire.
    pub fn brush_submodel(&self) -> Option<u32> {
        self.get("model")?.strip_prefix('*')?.parse().ok()
    }

    /// `renderamt`, defaulting the way the engine does when the key is absent.
    ///
    /// The default matters: a `func_button` with `rendermode 2` and no
    /// `renderamt` key is not faintly visible, it is *invisible*, and that is
    /// what makes it free to remove.
    pub fn render(&self) -> (i32, i32) {
        let mode = self
            .get("rendermode")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0);
        let amt = self
            .get("renderamt")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0);
        (mode, amt)
    }
}

/// Parses the entity lump text into entities, preserving key order.
///
/// Tolerant in the same places the engine is: anything outside a `{ }` block is
/// skipped, a `//` comment is skipped to the end of its line, and the lump's
/// trailing NUL terminates it.
pub fn parse_entity_text(text: &str) -> Result<Vec<MapEntity>, String> {
    let mut out = Vec::new();
    let mut current: Option<MapEntity> = None;
    let mut chars = text.char_indices().peekable();

    while let Some((i, c)) = chars.next() {
        match c {
            '\0' => break,
            '{' => current = Some(MapEntity::default()),
            '}' => {
                if let Some(e) = current.take() {
                    out.push(e);
                }
            }
            '/' if starts_comment(text.as_bytes(), i) => {
                skip_to(&mut chars, comment_end(text.as_bytes(), i));
            }
            '"' => {
                let key = take_quoted(text, i)?;
                let after_key = key.1;
                let vq = next_quote(text, after_key)
                    .ok_or_else(|| format!("entity lump: key {:?} has no value", key.0))?;
                let value = take_quoted(text, vq)?;
                skip_to(&mut chars, value.1);
                match current.as_mut() {
                    Some(e) => e.pairs.push((key.0, value.0)),
                    // A key/value pair outside a block is malformed; the engine
                    // ignores it and so does this, rather than failing a file
                    // the game itself loads.
                    None => continue,
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Advances `chars` to the first char at or past byte index `end`.
fn skip_to(chars: &mut std::iter::Peekable<std::str::CharIndices>, end: usize) {
    while chars.next_if(|&(j, _)| j < end).is_some() {}
}

/// Whether a `//` comment starts at byte `at`.
///
/// The engine's tokenizer (`COM_Parse`: pre-Anniversary `hw.dll` `0x1d2b13d`,
/// Anniversary `0x101b8ddd`) skips `//` to the end of the line, but only where
/// a token would start: after whitespace or the end of the previous token. In
/// the middle of a bare word, or inside a quoted string, `//` is text. A
/// commented-out line can then hold a lone `"` the engine never sees:
/// dod_beachassault2_mtek's worldspawn has `//\sierra\...\aircraft.wad;"`, a
/// wad list disabled by hand. Reading that quote shifted every later key and
/// value by one and left the lump's last string unclosed (#988).
fn starts_comment(bytes: &[u8], at: usize) -> bool {
    bytes.get(at..at + 2) == Some(b"//")
        && (at == 0
            || matches!(
                bytes[at - 1],
                0..=b' ' | b'"' | b'{' | b'}' | b'(' | b')' | b'\'' | b','
            ))
}

/// The byte index of the newline ending the comment at `at`, or the end of
/// the text.
fn comment_end(bytes: &[u8], at: usize) -> usize {
    bytes[at..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(bytes.len(), |n| at + n)
}

/// The byte index of the next `"` at or after `at`, skipping comments: the
/// opening quote of a key's value.
fn next_quote(text: &str, mut at: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    while at < bytes.len() {
        match bytes[at] {
            b'"' => return Some(at),
            b'/' if starts_comment(bytes, at) => at = comment_end(bytes, at),
            _ => at += 1,
        }
    }
    None
}

/// Reads a quoted string starting at the opening quote at `at`.
/// Returns the contents and the byte index just past the closing quote.
fn take_quoted(text: &str, at: usize) -> Result<(String, usize), String> {
    // `at` is a `"`, one byte, so `at + 1` is a char boundary.
    let rest = &text[at + 1..];
    let end = rest
        .find('"')
        .ok_or_else(|| format!("entity lump: unterminated string at {at}"))?;
    Ok((rest[..end].to_string(), at + 1 + end + 1))
}

/// The entity lump of a BSP, as text (the trailing NUL is stripped).
pub fn entity_text(bytes: &[u8]) -> Result<String, String> {
    let (offset, length) = lump_span(bytes, LUMP_ENTITIES)?;
    let raw = offset
        .checked_add(length)
        .and_then(|end| bytes.get(offset..end))
        .ok_or_else(|| "entity lump runs past the end of the file".to_string())?;
    Ok(text_up_to_nul(raw))
}

/// `raw` up to its first NUL, as text.
///
/// Entity lumps are plain ASCII in practice but carry the odd high byte in a
/// mapper's name or a message string, so this decodes lossily rather than
/// refusing to read a map the game loads.
pub fn text_up_to_nul(raw: &[u8]) -> String {
    let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).into_owned()
}

pub fn parse_entities(bytes: &[u8]) -> Result<Vec<MapEntity>, String> {
    parse_entity_text(&entity_text(bytes)?)
}

/// Renders entities back to lump text, in the format the compilers emit.
pub fn serialize_entities(entities: &[MapEntity]) -> String {
    let mut out = String::new();
    for e in entities {
        out.push_str("{\n");
        for (k, v) in &e.pairs {
            let _ = writeln!(out, "\"{k}\" \"{v}\"");
        }
        out.push_str("}\n");
    }
    out
}

/// Lump `index`'s `(offset, length)` from a BSP header.
pub fn lump_span(bytes: &[u8], index: usize) -> Result<(usize, usize), String> {
    let at = 4 + index * 8;
    let read = |at: usize| -> Result<i32, String> {
        bytes
            .get(at..at + 4)
            .map(|s| i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
            .ok_or_else(|| format!("BSP header truncated at byte {at}"))
    };
    let offset = read(at)?;
    let length = read(at + 4)?;
    if offset < 0 || length < 0 {
        return Err(format!("lump {index} has a negative offset or length"));
    }
    Ok((offset as usize, length as usize))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "{\n\"classname\" \"worldspawn\"\n\"wad\" \"\\\\half-life\\\\dod.wad\"\n}\n\
                          {\n\"origin\" \"1 2 3\"\n\"model\" \"*7\"\n\"classname\" \"func_illusionary\"\n\"rendermode\" \"4\"\n\"renderamt\" \"255\"\n}\n\
                          {\n\"model\" \"*8\"\n\"classname\" \"func_button\"\n\"rendermode\" \"2\"\n}\n";

    #[test]
    fn parses_keys_in_order_and_finds_brush_models() {
        let ents = parse_entity_text(SAMPLE).unwrap();
        assert_eq!(ents.len(), 3);
        assert_eq!(ents[0].classname(), "worldspawn");
        assert_eq!(ents[0].brush_submodel(), None);
        assert_eq!(ents[1].brush_submodel(), Some(7));
        assert_eq!(ents[1].pairs[0].0, "origin");
        assert_eq!(ents[2].brush_submodel(), Some(8));
    }

    /// The default that decides whether an entity is worth removing: no
    /// `renderamt` key with a non-zero `rendermode` means invisible, not opaque.
    #[test]
    fn absent_renderamt_reads_as_invisible() {
        let ents = parse_entity_text(SAMPLE).unwrap();
        assert_eq!(ents[1].render(), (4, 255));
        assert_eq!(ents[2].render(), (2, 0));
        assert_eq!(ents[0].render(), (0, 0));
    }

    #[test]
    fn serialize_round_trips() {
        let ents = parse_entity_text(SAMPLE).unwrap();
        let again = parse_entity_text(&serialize_entities(&ents)).unwrap();
        assert_eq!(ents, again);
    }

    #[test]
    fn rejects_a_truncated_header() {
        assert!(entity_text(&[0u8; 8]).is_err());
    }

    /// A lump whose offset + length overflows is refused, not sliced.
    #[test]
    fn rejects_a_lump_past_the_end() {
        let mut header = vec![0u8; HEADER_SIZE];
        header[4..8].copy_from_slice(&i32::MAX.to_le_bytes());
        header[8..12].copy_from_slice(&i32::MAX.to_le_bytes());
        assert!(entity_text(&header).is_err());
    }

    #[test]
    fn text_stops_at_the_first_nul() {
        assert_eq!(text_up_to_nul(b"{ }\0garbage"), "{ }");
        assert_eq!(text_up_to_nul(b"no nul"), "no nul");
        assert_eq!(text_up_to_nul(b""), "");
    }

    /// The hook parses whatever is in the game's memory, and a release build
    /// aborts on a panic: every malformed shape is an error or a short list.
    #[test]
    fn malformed_text_never_panics() {
        let cases: &[&str] = &[
            "",
            "{",
            "}",
            "\"",
            "{ \"",
            "{ \"key",
            "{ \"key\"",
            "{ \"key\" ",
            "{ \"key\" \"value",
            "\"key\" \"value\"",
            "}}}{{{",
            "{ \"k\" \"v\" \"k2\" }",
            "{ \"é\" \"ü\" } { \"日本\" \"語\" }",
            "{ \"a\" \"\u{fffd}\" }\u{fffd}\"",
            "\0{ \"classname\" \"worldspawn\" }",
        ];
        for text in cases {
            let _ = parse_entity_text(text);
        }
        // Every prefix of a real-looking lump, cut at every char boundary.
        let lump = "{\n\"classname\" \"dod_control_point\"\n\"point_allies_capsound\" \"ambience/uspointcaptured.wav\"\n}\n{ \"é\" \"x\" }";
        for (i, _) in lump.char_indices() {
            let _ = parse_entity_text(&lump[..i]);
        }
        // Bytes that are not UTF-8 at all, decoded the way the hook does.
        let junk: Vec<u8> = (0..=255u8).cycle().take(4096).collect();
        let _ = parse_entity_text(&text_up_to_nul(&junk[1..]));
    }

    /// dod_beachassault2_mtek's worldspawn, shortened: a commented-out wad
    /// list ending in a lone quote, which the engine skips with the comment
    /// (#988). Then dod_achen_mtek's commented-out spawn door, which the
    /// engine never spawns.
    #[test]
    fn a_comment_hides_its_stray_quote() {
        let lump = r#"{
"wad" "\sierra\half-life\valve\halflife.wad;\sierra\half-life\firearms\firearms.wad"

//\sierra\half-life\valve\ed_terrain.wad;\sierra\half-life\valve\aircraft.wad;"
"mapversion" "220"
"skyname" "boat"
"classname" "worldspawn"
}
//{
//"model" "*180"  // AXIS SPAWN DOOR
//"classname" "func_door_rotating"
//}
{
"classname" "ambient_generic"
"message" "ambience/uswin.wav"
}
"#;
        let ents = parse_entity_text(lump).unwrap();
        assert_eq!(ents.len(), 2);
        let keys: Vec<&str> = ents[0].pairs.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["wad", "mapversion", "skyname", "classname"]);
        assert_eq!(ents[0].get("mapversion"), Some("220"));
        assert_eq!(ents[0].classname(), "worldspawn");
        assert_eq!(ents[1].get("message"), Some("ambience/uswin.wav"));
    }

    /// Inside a quoted string `//` is text, as in `COM_Parse`. A comment
    /// between a key and its value, or after the last brace, is skipped.
    #[test]
    fn comments_are_skipped_outside_quoted_strings() {
        let ents =
            parse_entity_text("{\n\"url\" \"http://x//y\"\n\"key\" // \"not\"\n\"value\"\n}// \"")
                .unwrap();
        assert_eq!(
            ents[0].pairs,
            [
                ("url".to_string(), "http://x//y".to_string()),
                ("key".to_string(), "value".to_string()),
            ]
        );
    }

    #[test]
    fn unterminated_strings_are_errors() {
        assert!(parse_entity_text("{ \"key").is_err());
        assert!(parse_entity_text("{ \"key\" \"value").is_err());
        assert!(parse_entity_text("{ \"key\" }").is_err());
    }
}
