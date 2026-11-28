use super::*;

use Family::{FlagCapture, VictoryMusic};

/// Whether `name` is muted with every setting on, by the lists alone.
fn mutes(name: &[u8], flags: i32) -> bool {
    muted_by(name, flags, |_| true, None).is_some()
}

/// Who mutes `name` with only `family`'s setting on, by the lists alone.
fn with_only(family: Family, name: &str) -> Option<Family> {
    muted_by(name.as_bytes(), 0, |f| f == family, None).map(|(f, _)| f)
}

/// Anzio's real sound wiring (`test-fixtures/dod_anzio_sound_entities.txt`).
const ANZIO: &str = include_str!("../../test-fixtures/dod_anzio_sound_entities.txt");

/// `text` as the engine keeps it: NUL-terminated, with whatever follows.
fn in_memory(text: &str) -> Vec<u8> {
    let mut bytes = text.as_bytes().to_vec();
    bytes.extend_from_slice(b"\0{ \"classname\" \"after the NUL\" }");
    bytes
}

fn names(list: &[&str]) -> BTreeSet<Vec<u8>> {
    list.iter().map(|n| n.as_bytes().to_vec()).collect()
}

/// The runtime route (bytes in the engine's memory) and the offline one
/// (the example's table, `dod::map_sounds` on the parsed lump) agree.
#[test]
fn the_runtime_classification_matches_the_offline_table() {
    let runtime = classify(&in_memory(ANZIO)).expect("anzio parses");
    assert_eq!(
        runtime.victory,
        names(&["ambience/germanwin.wav", "ambience/uswin.wav"])
    );
    assert_eq!(
        runtime.capture,
        names(&[
            "ambience/gerpointcaptured.wav",
            "ambience/uspointcaptured.wav"
        ])
    );

    let offline = dod::map_sounds::map_sounds(
        &dod::map_entities::parse_entity_text(ANZIO).expect("anzio parses"),
    );
    let as_bytes = |set: &BTreeSet<String>| -> BTreeSet<Vec<u8>> {
        set.iter().map(|n| n.as_bytes().to_vec()).collect()
    };
    assert_eq!(runtime.victory, as_bytes(&offline.round_win));
    assert_eq!(runtime.capture, as_bytes(&offline.capture));
}

/// Every installed map, read both ways: the whole lump as the engine
/// keeps it, and the offline example's `parse_entities` on the `.bsp`.
/// `DOD_MAPS_DIR`, or the pre-Anniversary install's `dod\maps`.
#[test]
#[ignore = "needs a folder of real maps"]
fn every_installed_map_classifies_the_same_both_ways() {
    let dir = std::env::var_os("DOD_MAPS_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(
                r"C:\Program Files (x86)\Steam\steamapps\common\Half-Life - PRE-Anniversary for Movies\dod\maps",
            )
        });
    let mut read = 0;
    for entry in
        std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("no maps at {}: {e}", dir.display()))
    {
        let path = entry.unwrap().path();
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("bsp"))
        {
            continue;
        }
        let bsp = std::fs::read(&path).unwrap();
        let (offset, length) = dod::map_entities::lump_span(&bsp, 0).unwrap();
        let lump = &bsp[offset..offset + length];
        let offline = dod::map_entities::parse_entities(&bsp)
            .map(|entities| dod::map_sounds::map_sounds(&entities));
        match (classify(lump), offline) {
            (Ok(runtime), Ok(offline)) => {
                let as_bytes = |set: &BTreeSet<String>| -> BTreeSet<Vec<u8>> {
                    set.iter().map(|n| n.as_bytes().to_vec()).collect()
                };
                assert_eq!(
                    runtime.victory,
                    as_bytes(&offline.round_win),
                    "{}",
                    path.display()
                );
                assert_eq!(
                    runtime.capture,
                    as_bytes(&offline.capture),
                    "{}",
                    path.display()
                );
            }
            (Err(_), Err(_)) => {}
            (runtime, offline) => panic!(
                "{}: runtime {:?}, offline {:?}",
                path.display(),
                runtime.is_ok(),
                offline.is_ok()
            ),
        }
        read += 1;
    }
    assert!(read > 0, "no .bsp in {}", dir.display());
}

/// A custom map's own name, in neither list.
const CUSTOM_WIN: &str = "custom_map/our_team_won.wav";
const CUSTOM_CAP: &str = "custom_map/flag_taken.wav";

fn custom_map() -> MapNames {
    MapNames {
        victory: names(&[CUSTOM_WIN, "ambience/uswin.wav"]),
        capture: names(&[CUSTOM_CAP]),
    }
}

#[test]
fn a_name_only_the_map_has_is_muted_from_the_map() {
    let map = custom_map();
    let only =
        |family: Family, name: &str| muted_by(name.as_bytes(), 0, |f| f == family, Some(&map));
    assert_eq!(
        only(VictoryMusic, CUSTOM_WIN),
        Some((VictoryMusic, Source::Map))
    );
    assert_eq!(only(FlagCapture, CUSTOM_WIN), None);
    assert_eq!(
        only(FlagCapture, CUSTOM_CAP),
        Some((FlagCapture, Source::Map))
    );
    assert_eq!(only(VictoryMusic, CUSTOM_CAP), None);
    // Case, slashes and the stream marker, as for the lists.
    assert_eq!(
        only(VictoryMusic, "*Custom_Map\\Our_Team_Won.WAV"),
        Some((VictoryMusic, Source::Map))
    );
    // Without the map it plays: the lists don't know it.
    assert_eq!(muted_by(CUSTOM_WIN.as_bytes(), 0, |_| true, None), None);
}

/// The union: everything the lists mute today, they still mute with a
/// map read, including a name this map doesn't declare.
#[test]
fn the_lists_still_mute_beside_the_map() {
    let map = custom_map();
    for name in VICTORY_SAMPLES.iter().chain(&CAPTURE_SAMPLES) {
        let lists = muted_by(name.as_bytes(), 0, |_| true, None).map(|(f, _)| f);
        let both = muted_by(name.as_bytes(), 0, |_| true, Some(&map)).map(|(f, _)| f);
        assert!(lists.is_some(), "{name}");
        assert_eq!(lists, both, "{name}");
    }
    assert_eq!(
        muted_by(b"ambience/gerpointcaptured.wav", 0, |_| true, Some(&map)),
        Some((FlagCapture, Source::List))
    );
    // In both: reported as the map's.
    assert_eq!(
        muted_by(b"ambience/uswin.wav", 0, |_| true, Some(&map)),
        Some((VictoryMusic, Source::Map))
    );
}

#[test]
fn the_map_mutes_nothing_with_both_settings_off_or_on_a_stop() {
    let map = custom_map();
    for name in [CUSTOM_WIN, CUSTOM_CAP, "ambience/uswin.wav"] {
        assert_eq!(muted_by(name.as_bytes(), 0, |_| false, Some(&map)), None);
        assert_eq!(
            muted_by(name.as_bytes(), SND_STOP, |_| true, Some(&map)),
            None
        );
    }
    assert_eq!(
        muted_by(b"ambience/windy.wav", 0, |_| true, Some(&map)),
        None
    );
}

/// The hook reads whatever is in the game's memory, and a release build
/// aborts on a panic: malformed text gives an error, never a panic.
#[test]
fn malformed_entity_text_gives_nothing_without_panicking() {
    for text in [
        &b"{ \"classname\" \"dod_control_point\" \"point_allies_capsound"[..],
        b"{ \"key\" }",
        b"\"",
    ] {
        assert!(
            classify(text).is_err(),
            "{:?}",
            String::from_utf8_lossy(text)
        );
    }
    // Not entity text at all: whatever it gives, it names nothing.
    let junk: Vec<u8> = (1..=255u8).cycle().take(8192).collect();
    if let Ok(names) = classify(&junk) {
        assert!(names.victory.is_empty() && names.capture.is_empty());
    }
    assert_eq!(classify(b""), Ok(MapNames::default()));
    assert_eq!(classify(b"\0{ \"x\" \"y\" }"), Ok(MapNames::default()));
    // Every prefix of the real map, as a truncated lump would be.
    for end in (0..ANZIO.len()).step_by(7) {
        let _ = classify(&ANZIO.as_bytes()[..end]);
    }
}

#[test]
fn the_status_line_says_where_the_names_came_from() {
    let read = MapState::Read {
        map: "dod_anzio".into(),
        names: classify(&in_memory(ANZIO)).unwrap(),
    };
    assert_eq!(
        read.describe(),
        "map sounds: dod_anzio 2 victory, 2 capture (from the map)"
    );
    let unreadable = MapState::Unreadable {
        map: "dod_x".into(),
        why: "the map has no entity lump".into(),
    };
    assert!(unreadable.describe().ends_with("the lists alone"));
    assert_eq!(unreadable.names(), None);
}

/// `model_t` is the SDK's struct, shared with client.dll, so both builds
/// agree; `tools/verify_map_entities_offsets.py` checks each `hw.dll`.
#[test]
fn the_model_offsets_are_the_sdk_layout() {
    for build in &BUILDS {
        assert_eq!(build.model_type_at, 0x44, "{}", build.name);
        assert_eq!(build.model_entities_at, 0x180, "{}", build.name);
    }
}

const VICTORY_SAMPLES: [&str; 6] = [
    "ambience/uswin.wav",
    "ambience/germanwin.wav",
    "ambience/britwin.wav",
    "ambience/alliesscore.wav",
    "ambience/axisscore.wav",
    "ambience/no_winner.wav",
];

const CAPTURE_SAMPLES: [&str; 8] = [
    "ambience/uspointcaptured.wav",
    "ambience/gerpointcaptured.wav",
    "ambience/usareasecure.wav",
    "ambience/gerareasecure.wav",
    "ambience/alliescap.wav",
    "ambience/axiscap.wav",
    // dod_escape also places it as an ambient_generic, for a mortar
    // objective destroyed: still an announcement, so still here.
    "ambience/usobjectivesecure.wav",
    "ambience/gerobjectivesecure.wav",
];

#[test]
fn the_names_are_sorted_lowercase_and_unique() {
    for family in Family::ALL {
        for pair in family.names().windows(2) {
            assert!(pair[0] < pair[1], "{} then {}", pair[0], pair[1]);
        }
        for name in family.names() {
            assert_eq!(*name, name.to_ascii_lowercase());
            assert!(!name.contains('\\') && name.ends_with(".wav"), "{name}");
            assert!(name.len() < MAX_NAME, "{name}");
        }
    }
}

#[test]
fn no_name_is_in_both_lists_and_the_survey_found_98() {
    for name in VICTORY_MUSIC {
        assert!(!FLAG_CAPTURE.contains(name), "{name}");
    }
    assert_eq!(VICTORY_MUSIC.len(), 37);
    assert_eq!(FLAG_CAPTURE.len(), 61);
}

#[test]
fn victory_music_is_muted_only_by_its_own_setting() {
    for name in VICTORY_SAMPLES {
        assert_eq!(with_only(VictoryMusic, name), Some(VictoryMusic), "{name}");
        assert_eq!(with_only(FlagCapture, name), None, "{name}");
    }
    // dod_vicenza fires these through a dod_score_ent.
    assert_eq!(with_only(VictoryMusic, "uswin.wav"), Some(VictoryMusic));
}

#[test]
fn capture_announcements_are_muted_only_by_their_own_setting() {
    for name in CAPTURE_SAMPLES {
        assert_eq!(with_only(FlagCapture, name), Some(FlagCapture), "{name}");
        assert_eq!(with_only(VictoryMusic, name), None, "{name}");
    }
}

#[test]
fn with_both_settings_off_everything_plays() {
    for name in VICTORY_SAMPLES.iter().chain(&CAPTURE_SAMPLES) {
        assert_eq!(
            muted_by(name.as_bytes(), 0, |_| false, None),
            None,
            "{name}"
        );
    }
}

#[test]
fn names_match_whatever_their_case_slashes_or_stream_marker() {
    assert!(mutes(b"Ambience/USwin.wav", 0));
    assert!(mutes(b"ambience\\gerpointcaptured.wav", 0));
    assert!(mutes(b"*ambience/uswin.wav", 0));
}

/// The console's autocomplete swaps a name that is the whole start of
/// another for the longer one on space.
#[test]
fn neither_setting_is_the_start_of_another_name() {
    fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                for part in text.split("console_name!(\"").skip(1) {
                    let name = part.split('"').next().unwrap();
                    // `console_name!("")` is the bare prefix.
                    if !name.is_empty() {
                        out.push(name.to_string());
                    }
                }
            }
        }
    }
    let mut all = Vec::new();
    walk(std::path::Path::new("src"), &mut all);
    assert!(all.len() > 50, "found only {} names", all.len());
    for ours in ["sound_mute_victory_music", "sound_mute_flag_capture"] {
        assert!(all.iter().any(|n| n == ours), "{ours}");
        for other in all.iter().filter(|n| *n != ours) {
            assert!(!other.starts_with(ours), "{ours} is the start of {other}");
            assert!(
                !ours.starts_with(other.as_str()),
                "{other} is the start of {ours}"
            );
        }
    }
}

#[test]
fn everything_else_plays() {
    for name in [
        // A substring match on "win" would have taken these (28 maps).
        "ambience/windy.wav",
        "ambience/erriewind.wav",
        // Map ambience placed at map start.
        "ambience/tree.wav",
        "ambience/dodambience3.wav",
        "weapons/kar_shoot.wav",
        "player/pl_step1.wav",
        "player/usattack.wav",
        // A sentence.
        "!HG_ALERT",
        "",
        "ambience/uswin.wav.extra",
        "ambience/uswin",
    ] {
        assert!(!mutes(name.as_bytes(), 0), "{name}");
    }
}

#[test]
fn a_stop_always_goes_through() {
    assert!(!mutes(b"ambience/uswin.wav", SND_STOP));
    assert!(!mutes(b"ambience/uspointcaptured.wav", SND_STOP | 1));
}

#[test]
fn a_long_name_is_cut_at_the_engines_limit() {
    let long = [b'a'; 100];
    let mut buf = [0u8; MAX_NAME];
    assert_eq!(normalize(&long, &mut buf).len(), MAX_NAME);
    assert!(!mutes(&long, 0));
}

#[test]
fn a_name_ends_at_its_nul() {
    let mut sfx = [0u8; MAX_NAME];
    sfx[..18].copy_from_slice(b"ambience/uswin.wav");
    sfx[19] = b'x';
    let name = unsafe { name_of(sfx.as_ptr()) };
    assert_eq!(name, b"ambience/uswin.wav");
    assert!(unsafe { name_of(std::ptr::null()) }.is_empty());
}

#[test]
fn each_call_offset_lands_on_the_call() {
    for build in &BUILDS {
        for site in &build.sites {
            let bytes: Vec<&str> = site.pattern.split(' ').collect();
            assert_eq!(bytes[site.call_at], "E8", "{} {}", build.name, site.what);
            assert!(
                bytes[site.call_at + 1..site.call_at + 5]
                    .iter()
                    .all(|b| *b == "??"),
                "{} {}",
                build.name,
                site.what
            );
        }
    }
}

#[test]
fn each_build_has_two_static_calls_and_one_dynamic() {
    for build in &BUILDS {
        let kinds: Vec<Kind> = build.sites.iter().map(|s| s.kind).collect();
        assert_eq!(kinds.iter().filter(|k| **k == Kind::Static).count(), 2);
        assert_eq!(kinds.iter().filter(|k| **k == Kind::Dynamic).count(), 1);
    }
}
