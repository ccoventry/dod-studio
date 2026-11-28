use super::*;

fn ci_fixture() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../test-fixtures/ci_fixture.dem"
    ))
    .expect("test-fixtures/ci_fixture.dem")
}

#[test]
fn a_damaged_demo_split_points_at_the_repair() {
    let dir = std::env::temp_dir().join(format!("dodstudio-split-damaged-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let demo = dir.join("damaged.dem");
    let mut bytes = ci_fixture();
    let at = bytes.len() / 2;
    bytes[at..at + 300_000].fill(0xEE);
    std::fs::write(&demo, &bytes).unwrap();
    let err = demo_segments(&demo).unwrap_err();
    assert!(
        err.contains("damaged") && err.contains("Repair it first"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Damage inside one network frame's messages, its header intact: the
/// repair finds no hole in it, so Studio offers no Repair and leaves
/// Split on. The split's strict parse still stops at the damage; what it
/// says must not send the user to Repair.
#[test]
fn a_demo_damaged_inside_a_frame_split_does_not_point_at_the_repair() {
    let dir = std::env::temp_dir().join(format!(
        "dodstudio-split-frame-damaged-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let demo = dir.join("damaged.dem");
    let mut bytes = ci_fixture();
    let parsed = dem::open_demo_from_bytes(&bytes).unwrap();
    let spans: Vec<_> = parsed
        .directory
        .entries
        .iter()
        .flat_map(|e| &e.frames)
        .filter_map(|f| match &f.frame_data {
            FrameData::NetworkMessage(n) => n.1.source_span.clone(),
            _ => None,
        })
        .filter(|span| span.len() > 64)
        .collect();
    bytes[spans[spans.len() / 2].clone()].fill(0xEE);
    std::fs::write(&demo, &bytes).unwrap();
    assert_eq!(analysis::repair::find_first_hole(&bytes), None);
    let err = split_file_with_progress(&demo, Keep::AtLeast(60.0), &[], &mut |_| {})
        .expect_err("the strict parse stops at the damage");
    assert!(err.starts_with("could not read the demo"), "{err}");
    assert!(!err.contains("Repair"), "{err}");
    // The parser's reason, not its dump of every byte after the damage.
    assert!(err.chars().count() < 300, "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_quick_walk_gives_what_the_full_parse_gives() {
    let bytes = ci_fixture();
    let full = segments(&dem::open_demo_from_bytes(&bytes).unwrap());
    assert_eq!(quick_segments(&bytes), Some(full));
    // A file it can't walk gives no answer rather than a wrong one.
    assert_eq!(quick_segments(&bytes[..bytes.len() / 2]), None);
}

/// Every multi-map demo in `DOD_SPLIT_DEMOS` (a folder), or the PRE
/// install's `dod` folder: the quick walk must match the full parse.
#[test]
#[ignore = "needs real multi-map demos (DOD_SPLIT_DEMOS or the PRE install)"]
fn the_quick_walk_matches_the_full_parse_on_real_multi_map_demos() {
    let folder = std::env::var_os("DOD_SPLIT_DEMOS").map(std::path::PathBuf::from).unwrap_or_else(|| {
        std::path::PathBuf::from(
            r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - PRE-Anniversary for Movies\dod",
        )
    });
    let mut checked = 0;
    for entry in std::fs::read_dir(&folder).unwrap_or_else(|e| panic!("{}: {e}", folder.display()))
    {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "dem") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        if quick_maps(&bytes, header_protocol(&bytes).unwrap_or(48)).len() < 2 {
            continue;
        }
        let Ok(demo) = dem::open_demo_from_bytes(&bytes) else {
            continue;
        };
        assert_eq!(
            quick_segments(&bytes),
            Some(segments(&demo)),
            "{}",
            path.display()
        );
        checked += 1;
    }
    assert!(checked > 0, "no multi-map demo in {}", folder.display());
    eprintln!("{checked} multi-map demos match");
}

/// HLAE's demo tools leave their watermark after the NUL of the header's
/// map name (#1541).
const HLAE_WATERMARK: &[u8] = b"anced Effects is great! I love Half-Life Advanced Effects!";

#[test]
fn the_header_map_ends_at_its_first_nul() {
    let mut field = b"dod_railroad\0".to_vec();
    field.extend(HLAE_WATERMARK);
    field.resize(260, 0);
    assert_eq!(header_map(&field), "dod_railroad");
    assert_eq!(header_map(&[0; 260]), "");
}

#[test]
fn a_demo_with_text_after_its_header_map_reads_the_map_alone() {
    let mut bytes = ci_fixture();
    let name_end = 16 + bytes[16..276].iter().position(|&b| b == 0).unwrap();
    bytes[name_end + 1..name_end + 1 + HLAE_WATERMARK.len()].copy_from_slice(HLAE_WATERMARK);
    let full = segments(&dem::open_demo_from_bytes(&bytes).unwrap());
    assert_eq!(full[0].map, "dod_anzio");
    assert_eq!(quick_segments(&bytes), Some(full));
}

#[test]
fn the_last_map_ends_with_the_last_playback_entry() {
    assert_eq!(last_playback_entry([0, 1].into_iter()), 1);
    assert_eq!(last_playback_entry([0, 1, 2].into_iter()), 1);
    assert_eq!(last_playback_entry([0, 1, 1].into_iter()), 2);
    // No playback entry at all: the last, as before.
    assert_eq!(last_playback_entry([0, 2].into_iter()), 1);
    assert_eq!(last_playback_entry(std::iter::empty()), 0);
}

/// `bytes` with the 9-byte type-2 entry HLAE's demo tools add after the
/// playback one (#1541): a demo-start frame, then a next-section frame
/// outside its length, both at time 0.
fn with_hlae_garbage_entry(mut bytes: Vec<u8>) -> Vec<u8> {
    let directory = i32::from_le_bytes(bytes[540..544].try_into().unwrap()) as usize;
    let old = bytes.split_off(directory);
    let count = i32::from_le_bytes(old[..4].try_into().unwrap());
    bytes.extend([2, 0, 0, 0, 0, 0, 0, 0, 0x4c]);
    bytes.extend([5, 0, 0, 0, 0, 1, 0, 0, 0]);
    let new_directory = bytes.len() as i32;
    bytes[540..544].copy_from_slice(&new_directory.to_le_bytes());
    bytes.extend((count + 1).to_le_bytes());
    bytes.extend(&old[4..4 + count as usize * 92]);
    let mut entry = old[4 + 92..4 + 2 * 92].to_vec();
    entry[..4].copy_from_slice(&2i32.to_le_bytes());
    entry[72..76].copy_from_slice(&255i32.to_le_bytes());
    entry[76..80].copy_from_slice(&0f32.to_le_bytes());
    entry[80..84].copy_from_slice(&2i32.to_le_bytes());
    entry[84..88].copy_from_slice(&(directory as i32).to_le_bytes());
    entry[88..92].copy_from_slice(&9i32.to_le_bytes());
    bytes.extend(entry);
    bytes
}

#[test]
fn a_garbage_entry_after_the_playback_does_not_end_the_map() {
    let bytes = ci_fixture();
    let plain = segments(&dem::open_demo_from_bytes(&bytes).unwrap());
    let hlae = with_hlae_garbage_entry(bytes);
    let full = segments(&dem::open_demo_from_bytes(&hlae).unwrap());
    assert_eq!(full, plain);
    assert!(full[0].seconds() > 60.0, "{full:?}");
    assert_eq!(quick_segments(&hlae), Some(full));
}

/// The directory's track times, summed, read from the file's bytes.
fn directory_seconds(bytes: &[u8]) -> Option<f32> {
    let i32_at = |at: usize| Some(i32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    let directory = usize::try_from(i32_at(540)?).ok()?;
    let count = usize::try_from(i32_at(directory)?).ok()?;
    (0..count.min(1024))
        .map(|e| {
            let at = directory + 4 + e * 92 + 76;
            Some(f32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
        })
        .sum()
}

/// Every demo HLAE's demo tools rewrote (text after the header's map name,
/// every track time 0) in `DOD_HLAE_DEMOS` (a folder), or the PRE install's
/// `dod` folder, of which there must be one: its
/// map reads without the watermark, and its length comes from the frames
/// though the directory says 0 (#1541).
#[test]
#[ignore = "needs demos HLAE's demo tools rewrote (DOD_HLAE_DEMOS or the PRE install)"]
fn demos_hlae_rewrote_read_their_map_and_length() {
    let folder = std::env::var_os("DOD_HLAE_DEMOS").map(std::path::PathBuf::from).unwrap_or_else(|| {
        std::path::PathBuf::from(
            r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - PRE-Anniversary for Movies\dod",
        )
    });
    let mut checked = 0;
    for entry in std::fs::read_dir(&folder).unwrap_or_else(|e| panic!("{}: {e}", folder.display()))
    {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "dem") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let Some(field) = bytes.get(16..276) else {
            continue;
        };
        let name_end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
        // Both marks of the rewrite: text after the map name, and a
        // directory whose track times are all 0, so the frame-length path
        // is what gets tested (a demo with only the first has real times).
        if field[name_end..].iter().all(|&b| b == 0) || directory_seconds(&bytes) != Some(0.0) {
            continue;
        }
        let segs = quick_segments(&bytes).expect("the quick walk reads it");
        assert!(
            segs.iter().all(|s| !s.map.contains('\0')),
            "{}: {segs:?}",
            path.display()
        );
        let demo = dem::open_demo_from_bytes(&bytes).unwrap();
        assert_eq!(segments(&demo), segs, "{}", path.display());
        let info = analysis::DemoInfo::from(&demo);
        assert!(info.playback_time > 0.0, "{}", path.display());
        let last = segs.last().unwrap();
        assert!(
            (last.end_seconds - info.playback_time).abs() < 0.01,
            "{}: {segs:?}",
            path.display()
        );
        eprintln!(
            "{}: {} for {:.2} s",
            path.display(),
            segs[0].map,
            info.playback_time
        );
        checked += 1;
    }
    assert!(checked > 0, "no demo HLAE rewrote in {}", folder.display());
    eprintln!("{checked} demos HLAE rewrote read their map and length");
}

fn server_info_bytes(map: &str) -> Vec<u8> {
    let mut b = vec![SVC_SERVERINFO];
    b.extend_from_slice(&48i32.to_le_bytes());
    b.extend_from_slice(&3i32.to_le_bytes());
    b.extend_from_slice(&0x1234_5678u32.to_le_bytes());
    b.extend_from_slice(&[0xaa; 16]);
    b.extend_from_slice(&[32, 4, 1]);
    b.extend_from_slice(b"dod\0My Server\0");
    b.extend_from_slice(format!("maps/{map}.bsp\0").as_bytes());
    b
}

#[test]
fn finds_each_signon_in_raw_bytes() {
    let mut bytes = vec![0u8; 100];
    bytes.extend(server_info_bytes("dod_anzio"));
    bytes.extend([11, 48, 0, 0, 0, 1, 2, 3]); // a stray match
    bytes.extend(vec![7u8; 500]);
    bytes.extend(server_info_bytes("dod_flash"));
    assert_eq!(quick_maps(&bytes, 48), vec!["dod_anzio", "dod_flash"]);
    assert!(quick_maps(&bytes, 47).is_empty());
}

#[test]
fn map_names_lose_the_folder_and_extension() {
    assert_eq!(map_stem("maps/dod_anzio.bsp"), "dod_anzio");
    assert_eq!(map_stem("dod_anzio"), "dod_anzio");
}

#[test]
fn every_piece_gets_its_place_in_the_name() {
    let seg = |i: usize, map: &str| MapSegment {
        index: i,
        map: map.into(),
        map_checksum: 0,
        start_seconds: 0.0,
        end_seconds: 0.0,
        frames: 0,
    };
    let two = [seg(0, "dod_anzio"), seg(1, "dod_flash")];
    assert_eq!(split_name("m", &two, &two[0]), "m_split1_anzio");
    assert_eq!(split_name("m", &two, &two[1]), "m_split2_flash");
    // A map without the dod_ prefix keeps its name.
    let other = [seg(0, "ktp_lobby")];
    assert_eq!(split_name("m", &other, &other[0]), "m_split1_ktp_lobby");
    assert_eq!(
        old_split_name_at("m", &["dod_anzio", "dod_flash"], 1),
        "m_dod_flash"
    );
    let same = [seg(0, "dod_anzio"), seg(1, "dod_anzio")];
    assert_eq!(split_name("m", &same, &same[1]), "m_split2_anzio");
    assert_eq!(
        old_split_name_at("m", &["dod_anzio", "dod_anzio"], 1),
        "m_dod_anzio_2"
    );
}

#[test]
fn a_split_keeps_the_maps_of_a_minute_or_more() {
    let seg = |i: usize, start: f32, end: f32| MapSegment {
        index: i,
        map: "dod_lennon2".into(),
        map_checksum: 0,
        start_seconds: start,
        end_seconds: end,
        frames: 0,
    };
    // wsod25_grp3_h1_dyelife: 21:28 of lennon2, then 14 s of the next half.
    assert_eq!(
        keep_at_least(&[seg(0, -2.0, 1288.0), seg(1, 1288.0, 1302.0)], 60.0),
        [0]
    );
    assert_eq!(
        keep_at_least(
            &[
                seg(0, 0.0, 1200.0),
                seg(1, 1200.0, 2400.0),
                seg(2, 2400.0, 2410.0)
            ],
            60.0
        ),
        [0, 1]
    );
    // All short: keep them all rather than write nothing.
    assert_eq!(
        keep_at_least(&[seg(0, 0.0, 20.0), seg(1, 20.0, 30.0)], 60.0),
        [0, 1]
    );
}

#[test]
fn the_header_protocol_is_read_without_parsing() {
    let mut h = b"HLDEMO\0\0".to_vec();
    h.extend(5i32.to_le_bytes());
    h.extend(48i32.to_le_bytes());
    assert_eq!(header_protocol(&h), Some(48));
    assert_eq!(header_protocol(b"nope"), None);
}

#[test]
fn a_cancelled_split_writes_nothing_and_says_so() {
    let dir = std::env::temp_dir().join(format!("demo_split_cancel_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let demo = dir.join("m.dem");
    std::fs::write(&demo, ci_fixture()).unwrap();
    let (written, cancelled) =
        split_file_cancellable(&demo, Keep::These(&[0]), &[], &mut |_| {}, &|| true).unwrap();
    assert!(cancelled);
    assert!(written.is_empty());
    let left: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
    assert_eq!(left.len(), 1, "only the original");
    let _ = std::fs::remove_dir_all(&dir);
}
