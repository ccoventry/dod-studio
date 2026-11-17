use super::*;
use dem::types::{
    Aux, Demo, Directory, DirectoryEntry, NetworkMessage, NetworkMessageType, SvcPrint,
    SvcTempEntity, TempEntity,
};

// ── The write copies untouched frames verbatim ──────────────────────────
//
// `clean_demo_decals` writes with `write_to_bytes_reusing_source_cancellable`,
// so a frame it edits without going through `parsed_messages_mut` keeps its
// parse-time span and goes out as its ORIGINAL bytes — the edit silently
// dropped. These pin both edits it makes, the strip and the burst, to the
// bytes it hands back.

/// Ordinals carrying a real decal: everything except the run just before
/// the clip, which is left bare so the burst lands on frames the strip
/// never opened.
fn has_decal(ordinal: i32) -> bool {
    !(31..=40).contains(&ordinal)
}

const CLIP: (i32, i32) = (41, 50);
const LAST_ORDINAL: i32 = 61;

/// A demo whose frame ordinals are: 1 DemoStart, 2..=61 network frames,
/// 62 NextSection. Built in code, then written and parsed back, so what
/// the clean receives is a real parsed file with real spans.
fn demo_bytes() -> Vec<u8> {
    let zeros = [0u8; 1024];
    let (_, info) = dem::demo_parser::parse_network_messages_info(&zeros).unwrap();
    let (_, sequence_info) = dem::demo_parser::parse_sequence_info(&zeros).unwrap();

    let network_frame = |ordinal: i32| {
        let mut messages = vec![NetMessage::EngineMessage(Box::new(
            EngineMessage::SvcPrint(SvcPrint {
                message: ByteString::from(format!("{ordinal}\n\0").as_str()),
            }),
        ))];
        if has_decal(ordinal) {
            messages.push(NetMessage::EngineMessage(Box::new(
                EngineMessage::SvcTempEntity(SvcTempEntity {
                    entity_type: 116,
                    entity: TempEntity::TeWorldDecal(vec![0, 1, 0, 1, 0, 1, 9]),
                }),
            )));
        }
        Frame {
            time: ordinal as f32 * 0.01,
            frame: ordinal,
            frame_data: FrameData::NetworkMessage(Box::new((
                NetworkMessageType::Normal,
                NetworkMessage {
                    info: info.clone(),
                    sequence_info: sequence_info.clone(),
                    message_length: 0,
                    messages: MessageData::Parsed(messages),
                    source_span: None,
                },
            ))),
        }
    };

    let mut frames = vec![Frame {
        time: 0.0,
        frame: 1,
        frame_data: FrameData::DemoStart,
    }];
    frames.extend((2..=LAST_ORDINAL).map(network_frame));
    frames.push(Frame {
        time: 1.0,
        frame: LAST_ORDINAL + 1,
        frame_data: FrameData::NextSection,
    });

    Demo {
        header: dem::types::Header {
            magic: b"HLDEMO\x00\x00".to_vec(),
            demo_protocol: 5,
            network_protocol: 48,
            map_name: ByteString::from("dod_anzio"),
            game_directory: ByteString::from("dod"),
            map_checksum: 0,
            directory_offset: 0,
        },
        directory: Directory {
            entries: vec![DirectoryEntry {
                type_: 1,
                description: ByteString::from("Playback"),
                flags: 0,
                cd_track: -1,
                track_time: 0.0,
                frame_count: 0,
                frame_offset: 0,
                file_length: 0,
                frames,
            }],
        },
        _aux: Some(Aux::new2()),
    }
    .write_to_bytes()
    .unwrap()
}

/// Every world-decal payload in the demo, by frame ordinal.
fn world_decals_by_ordinal(bytes: &[u8]) -> std::collections::BTreeMap<i32, Vec<Vec<u8>>> {
    let demo = open_demo_from_bytes(bytes).unwrap();
    let mut out = std::collections::BTreeMap::new();
    for (entry_idx, frame_idx, ordinal) in frame_ordinals(&demo) {
        let frame = &demo.directory.entries[entry_idx].frames[frame_idx];
        for msg in parsed_messages(frame).into_iter().flatten() {
            if let NetMessage::EngineMessage(eng) = msg
                && let EngineMessage::SvcTempEntity(te) = eng.as_ref()
                && let TempEntity::TeWorldDecal(payload) = &te.entity
            {
                out.entry(ordinal)
                    .or_insert_with(Vec::new)
                    .push(payload.clone());
            }
        }
    }
    out
}

#[test]
fn both_edits_the_clean_makes_reach_the_bytes_it_writes() {
    let bytes = demo_bytes();
    let opts = DecalCleanOptions {
        ring_limit: 8,
        burst_margin: 0,
        max_per_frame: 4,
        lead_seconds: 0.0,
        inject_r_decals_command: false,
        flush_coord: Some([1.0, 2.0, 3.0]),
        flush_texture_index: Some(5),
        ..Default::default()
    };

    let (cleaned, stats) =
        clean_demo_decals(&bytes, &[CLIP], &opts, crate::decals::Cancel::never()).unwrap();

    let original = vec![0, 1, 0, 1, 0, 1, 9];
    let injected = vec![8, 0, 16, 0, 24, 0, 5];
    let decals = world_decals_by_ordinal(&cleaned);
    for ordinal in 2..=LAST_ORDINAL {
        let found = decals.get(&ordinal).cloned().unwrap_or_default();
        let expected = if (CLIP.0..=CLIP.1).contains(&ordinal) {
            vec![original.clone()]
        } else if ordinal == 39 || ordinal == 40 {
            // The burst walks back from the frame before the clip.
            vec![injected.clone(); 4]
        } else {
            // Stripped, or never had one.
            Vec::new()
        };
        assert_eq!(found, expected, "frame ordinal {ordinal}");
    }
    assert_eq!(stats.temp_entity_stripped, 40);
    assert_eq!(stats.flush_decals_injected, 8);
}

#[test]
fn a_clean_that_changes_nothing_hands_back_the_file_it_was_given() {
    let bytes = demo_bytes();
    let opts = DecalCleanOptions {
        strip_outside_windows: false,
        flush_burst: false,
        inject_r_decals_command: false,
        ..Default::default()
    };

    let (cleaned, _) =
        clean_demo_decals(&bytes, &[CLIP], &opts, crate::decals::Cancel::never()).unwrap();
    assert_eq!(cleaned, bytes);
}

/// The same guard on a real recording, which exercises every message type
/// the game actually sends rather than the three built above. The default
/// is the checked-in `test-fixtures/ci_fixture.dem`, so CI runs it;
/// `DOD_ROUNDTRIP_DEMO` points it at any other.
#[test]
fn a_real_demo_survives_the_verbatim_write() {
    let path = std::env::var_os("DOD_ROUNDTRIP_DEMO")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../test-fixtures/ci_fixture.dem"
            ))
        });
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut demo = open_demo_from_bytes(&bytes).unwrap();

    // Untouched, the output is the input, directory and all: the writer
    // writes the engine's own frame counts.
    let reused = demo
        .write_to_bytes_reusing_source_cancellable(&bytes, &|| false)
        .unwrap()
        .unwrap();
    assert!(reused == bytes, "an untouched real demo did not round-trip");

    // Stripped everywhere, every frame the strip opened must be re-encoded
    // and every other copied, which together must equal re-encoding the
    // lot. A frame edited without its span being cleared would differ.
    let (wall, spray) = strip_decal_messages(&mut demo, &[]);
    assert!(wall + spray > 0, "the demo has no decals to strip");
    let reused = demo
        .write_to_bytes_reusing_source_cancellable(&bytes, &|| false)
        .unwrap()
        .unwrap();
    assert!(
        reused == demo.write_to_bytes().unwrap(),
        "the verbatim write disagrees with a full re-encode after stripping"
    );
}
