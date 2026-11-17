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
