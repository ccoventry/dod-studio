// patch/bsp_entities.rs
// Read and rewrite a GoldSrc BSP's entity lump.
//
// ── Why a map file gets edited at all ────────────────────────────────────────
// `dod_lennon2` puts 276 entities on the wire from its opening snapshot, and
// the pre-Anniversary engine's `MAX_PACKET_ENTITIES` is 256, so every HLTV
// recording of that map closes `hl.exe` to desktop the moment playback starts
// (#207). The map's own entities account for essentially all of that count;
// gameplay adds only ~15-25 on top. Trimming the entity lump is therefore the
// one edit that fixes the map rather than working around it.
//
// ── Why only the entity lump ─────────────────────────────────────────────────
// The map checksum the engine stamps into a demo header is a CRC-32 over lumps
// 1..14 with the entity lump *excluded* (`bsp::map_checksum`). Editing lump 0
// alone therefore leaves the checksum untouched, so a trimmed map still loads
// every demo recorded against the original — which is the only reason shipping
// a modified BSP is defensible at all. `rewrite_entity_lump` rebuilds the file
// around a new lump 0 and nothing else, and the caller is expected to assert
// the checksum afterwards; `map_entity_trim` refuses to write if it moved.
//
// This module never writes over its input. It returns bytes; choosing a
// destination is the caller's problem, and the map in a game directory belongs
// to the user.

// The parser itself lives in `dod::map_entities`, pure and without I/O, so the
// hook DLL reads a map's entities with the same code (#284). Re-exported here
// so the patcher's callers keep their paths.
pub use dod::map_entities::{
    MapEntity, entity_text, parse_entities, parse_entity_text, serialize_entities,
};

use dod::map_entities::{HEADER_SIZE, LUMP_COUNT, LUMP_ENTITIES, lump_span};

/// Rebuilds a BSP around a new entity lump, leaving every other lump's bytes
/// exactly as they were.
///
/// Lumps are re-laid-out in ascending original-offset order and 4-byte aligned,
/// which is what the compilers produce and what the engine expects. Only lump 0
/// changes size, so the checksum over lumps 1..14 is untouched by construction
/// — but callers should still assert it, since "by construction" is how the
/// last three engine assumptions in this codebase were wrong.
pub fn rewrite_entity_lump(bytes: &[u8], entity_text: &str) -> Result<Vec<u8>, String> {
    if bytes.len() < HEADER_SIZE {
        return Err("file is shorter than a BSP header".to_string());
    }

    let mut spans = Vec::with_capacity(LUMP_COUNT);
    for index in 0..LUMP_COUNT {
        spans.push(lump_span(bytes, index)?);
    }

    // The new lump 0, NUL-terminated the way the engine's parser expects.
    let mut entities = entity_text.as_bytes().to_vec();
    entities.push(0);

    // Write lumps back in the order they appeared in the source file. Keeping
    // that order means a diff of the two files is confined to the entity lump
    // and the header, rather than being a wholesale reshuffle.
    let mut order: Vec<usize> = (0..LUMP_COUNT).collect();
    order.sort_by_key(|i| spans[*i].0);

    let mut out = vec![0u8; HEADER_SIZE];
    out[0..4].copy_from_slice(&(super::bsp::BSP_VERSION).to_le_bytes());

    let mut new_spans = [(0usize, 0usize); LUMP_COUNT];
    for index in order {
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
        let offset = out.len();
        if index == LUMP_ENTITIES {
            out.extend_from_slice(&entities);
        } else {
            let (o, l) = spans[index];
            let data = bytes
                .get(o..o + l)
                .ok_or_else(|| format!("lump {index} runs past the end of the file"))?;
            out.extend_from_slice(data);
        }
        new_spans[index] = (offset, out.len() - offset);
    }

    for (index, (offset, length)) in new_spans.iter().enumerate() {
        let at = 4 + index * 8;
        out[at..at + 4].copy_from_slice(&(*offset as i32).to_le_bytes());
        out[at + 4..at + 8].copy_from_slice(&(*length as i32).to_le_bytes());
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A BSP with a header, an entity lump and 14 stub lumps carrying
    /// recognisable bytes, so a rewrite that drops or reorders payload shows up.
    fn synthetic_bsp(entity_text: &str) -> Vec<u8> {
        let mut entities = entity_text.as_bytes().to_vec();
        entities.push(0);

        let mut payloads: Vec<Vec<u8>> = Vec::with_capacity(LUMP_COUNT);
        payloads.push(entities);
        for index in 1..LUMP_COUNT {
            payloads.push(vec![index as u8; index * 3]);
        }

        let mut out = vec![0u8; HEADER_SIZE];
        out[0..4].copy_from_slice(&30i32.to_le_bytes());
        let mut spans = Vec::new();
        for payload in &payloads {
            while !out.len().is_multiple_of(4) {
                out.push(0);
            }
            spans.push((out.len(), payload.len()));
            out.extend_from_slice(payload);
        }
        for (index, (offset, length)) in spans.iter().enumerate() {
            let at = 4 + index * 8;
            out[at..at + 4].copy_from_slice(&(*offset as i32).to_le_bytes());
            out[at + 4..at + 8].copy_from_slice(&(*length as i32).to_le_bytes());
        }
        out
    }

    const SAMPLE: &str = "{\n\"classname\" \"worldspawn\"\n\"wad\" \"\\\\half-life\\\\dod.wad\"\n}\n\
                          {\n\"origin\" \"1 2 3\"\n\"model\" \"*7\"\n\"classname\" \"func_illusionary\"\n\"rendermode\" \"4\"\n\"renderamt\" \"255\"\n}\n\
                          {\n\"model\" \"*8\"\n\"classname\" \"func_button\"\n\"rendermode\" \"2\"\n}\n";

    /// The whole premise of editing a shipped map: lump 0 changes, the map
    /// checksum does not, so demos recorded against the original still load.
    #[test]
    fn rewriting_entities_leaves_the_map_checksum_alone() {
        let original = synthetic_bsp(SAMPLE);
        let before = crate::patch::bsp::map_checksum(&original).unwrap();

        let mut ents = parse_entities(&original).unwrap();
        ents.retain(|e| e.classname() != "func_button");
        let trimmed = rewrite_entity_lump(&original, &serialize_entities(&ents)).unwrap();

        assert_eq!(crate::patch::bsp::map_checksum(&trimmed).unwrap(), before);
        assert_eq!(parse_entities(&trimmed).unwrap().len(), 2);
        assert!(trimmed.len() < original.len());
    }

    /// Every other lump's bytes have to survive the rebuild verbatim, or the
    /// checksum test above is passing for the wrong reason.
    #[test]
    fn rewriting_entities_preserves_every_other_lump() {
        let original = synthetic_bsp(SAMPLE);
        let trimmed =
            rewrite_entity_lump(&original, "{\n\"classname\" \"worldspawn\"\n}\n").unwrap();
        for index in 1..LUMP_COUNT {
            let (o1, l1) = lump_span(&original, index).unwrap();
            let (o2, l2) = lump_span(&trimmed, index).unwrap();
            assert_eq!(l1, l2, "lump {index} changed length");
            assert_eq!(
                &original[o1..o1 + l1],
                &trimmed[o2..o2 + l2],
                "lump {index} changed bytes"
            );
        }
    }

    #[test]
    fn rejects_a_truncated_header() {
        assert!(rewrite_entity_lump(&[0u8; 8], "{}").is_err());
    }
}
