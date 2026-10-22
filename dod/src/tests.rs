//! Unit tests for the user-message parsers.
//!
//! Every payload here is hand-built from the parser's own field order: little-endian integers,
//! NUL-terminated strings where the code calls `null_string`. Nothing is read from a demo file.
//!
//! Tests that pin behaviour which looks wrong carry a `// BUG?` comment: they assert what the
//! code does today, so a fix will turn them red on purpose.

#![allow(deprecated)] // `UserMessage::AmmoPickup` / `UserMessage::WeapPickup`.

use super::*;

/// Parses `data` as the message `name` and unwraps the expected variant.
macro_rules! parse_as {
    ($name:expr, $data:expr, $variant:ident) => {{
        let data: &[u8] = &$data[..];
        match UserMessage::new($name.as_bytes(), data) {
            Ok(UserMessage::$variant(inner)) => inner,
            Ok(other) => panic!("{} parsed as the wrong variant: {other:?}", $name),
            Err(_) => panic!("{} failed to parse {data:?}", $name),
        }
    }};
}

fn parses(name: &str, data: &[u8]) -> bool {
    UserMessage::new(name.as_bytes(), data).is_ok()
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

fn i16le(v: i16) -> [u8; 2] {
    v.to_le_bytes()
}

fn u16le(v: u16) -> [u8; 2] {
    v.to_le_bytes()
}

// ---------------------------------------------------------------------------------------------
// Dispatch table: one valid payload per message name.
// ---------------------------------------------------------------------------------------------

/// `(message name, a valid payload, Debug name of the variant it must produce)`.
///
/// Covers every name `UserMessage::new` dispatches on except `VGUIMenu`, which never parses
/// (see `vgui_menu_always_fails`).
fn valid_cases() -> Vec<(&'static str, Vec<u8>, &'static str)> {
    vec![
        ("AmmoPickup", vec![1, 2], "AmmoPickup"),
        ("AmmoShort", cat(&[&[1], &u16le(300)]), "AmmoShort"),
        ("AmmoX", vec![3, 30], "AmmoX"),
        (
            "BloodPuff",
            cat(&[&i16le(1), &i16le(2), &i16le(3)]),
            "BloodPuff",
        ),
        ("CameraView", b"cam\0".to_vec(), "CameraView"),
        ("CancelProg", vec![2, 0], "CancelProg"),
        ("CapMsg", cat(&[&[5], b"Flag\0", &[1]]), "CapMsg"),
        (
            "ClCorpse",
            cat(&[
                b"m.mdl\0",
                &i16le(1),
                &i16le(2),
                &i16le(3),
                &[4, 5, 6, 7],
                &u16le(8),
                &[2],
            ]),
            "ClCorpse",
        ),
        ("ClanTimer", vec![60], "ClanTimer"),
        ("ClientAreas", cat(&[&[4, 255], b"icon\0"]), "ClientAreas"),
        ("CurMarker", vec![3], "CurMarker"),
        ("CurWeapon", vec![1, 12, 30], "CurWeapon"),
        ("DeathMsg", vec![3, 7, 37], "DeathMsg"),
        ("Frags", cat(&[&[4], &i16le(10)]), "Frags"),
        ("GameRules", vec![1, 0], "GameRules"),
        ("HandSignal", vec![4, 2], "HandSignal"),
        ("Health", vec![100], "Health"),
        ("HideWeapon", vec![9], "HideWeapon"),
        ("HLTV", vec![5, 128], "Hltv"),
        ("HudText", cat(&[b"Hint\0", &[1]]), "HudText"),
        ("InitHUD", vec![], "InitHUD"),
        ("InitObj", objectives(&[(10, 0, 1, (100, -100))]), "InitObj"),
        ("MapMarker", vec![1, 2, 3, 4, 5, 6], "MapMarker"),
        ("MOTD", cat(&[&[1], b"hi"]), "Motd"),
        ("ObjScore", cat(&[&[3], &i16le(15)]), "ObjScore"),
        ("Object", vec![9, 9, 9], "Object"),
        ("PClass", vec![3, 21], "PClass"),
        ("PShoot", vec![1, 1], "PShoot"),
        ("PStatus", vec![3, 1], "PStatus"),
        ("PTeam", vec![3, 2], "PTeam"),
        ("PlayersIn", vec![1, 1, 2, 3], "PlayersIn"),
        ("ReloadDone", vec![], "ReloadDone"),
        ("ReqState", vec![], "ReqState"),
        ("ResetHUD", vec![], "ResetHUD"),
        ("ResetSens", vec![], "ResetSens"),
        ("RoundState", vec![3], "AlliesWin"),
        ("SayText", cat(&[&[3], b"hello\0"]), "SayText"),
        ("Scope", vec![6], "Scope"),
        ("ScoreInfo", vec![3, 1, 2, 3, 1, 1, 0], "ScoreInfo"),
        (
            "ScoreInfoLong",
            cat(&[&[3], &i16le(1), &i16le(2), &i16le(3), &[1, 1, 0]]),
            "ScoreInfoLong",
        ),
        (
            "ScoreShort",
            cat(&[&[3], &i16le(1), &i16le(2), &i16le(3), &[0]]),
            "ScoreShort",
        ),
        (
            "ScreenFade",
            cat(&[&u16le(1), &u16le(2), &u16le(3), &[128, 0, 0, 200]]),
            "ScreenFade",
        ),
        (
            "ScreenShake",
            cat(&[&u16le(4096), &u16le(4096), &u16le(4096)]),
            "ScreenShake",
        ),
        ("ServerName", b"Server".to_vec(), "ServerName"),
        ("SetFOV", vec![90], "SetFOV"),
        ("SetObj", vec![1, 2, 0], "SetObj"),
        ("ShowMenu", vec![1, 2, 3], "ShowMenu"),
        ("Spectator", vec![3, 1], "Spectator"),
        ("StartProg", cat(&[&[1, 1], &u16le(5)]), "StartProg"),
        (
            "StartProgF",
            cat(&[&[1, 2], &1.5f32.to_le_bytes()]),
            "StartProgF",
        ),
        ("StatusValue", vec![50], "StatusValue"),
        ("TeamScore", cat(&[&[1], &u16le(300)]), "TeamScore"),
        ("TextMsg", cat(&[&[4], b"#Msg\0"]), "TextMsg"),
        ("TimeLeft", u16le(600).to_vec(), "TimeLeft"),
        ("TimerStatus", vec![1, 2, 3], "TimerStatus"),
        ("UseSound", vec![1], "UseSound"),
        (
            "VoiceMask",
            cat(&[&(-1i32).to_le_bytes(), &6i32.to_le_bytes()]),
            "VoiceMask",
        ),
        ("WaveStatus", vec![2], "WaveStatus"),
        ("WaveTime", vec![15], "WaveTime"),
        (
            "WeaponList",
            vec![1, 210, 255, 0, 2, 1, 7, 0, 0, 30],
            "WeaponList",
        ),
        ("WeapPickup", vec![1], "WeapPickup"),
        ("Weather", vec![1, 2], "Weather"),
        ("YouDied", vec![0], "YouDied"),
    ]
}

/// Every name `UserMessage::new` dispatches on, `VGUIMenu` included.
const DISPATCHED_NAMES: usize = 64;

#[test]
fn dispatch_table_covers_every_known_name() {
    // 64 match arms in `UserMessage::new`; `VGUIMenu` is tested on its own.
    assert_eq!(valid_cases().len(), DISPATCHED_NAMES - 1);
}

#[test]
fn dispatch_by_name_produces_the_matching_variant() {
    for (name, data, variant) in valid_cases() {
        let message = match UserMessage::new(name.as_bytes(), &data) {
            Ok(message) => message,
            Err(_) => panic!("{name} failed to parse {data:?}"),
        };
        let debug = format!("{message:?}");
        // `RoundState` is a unit enum, so its Debug shows the inner variant.
        let expected = if name == "RoundState" {
            format!("RoundState({variant})")
        } else {
            format!("{variant}(")
        };
        assert!(
            debug.starts_with(&expected),
            "{name} produced {debug}, expected {expected}..."
        );
    }
}

#[test]
fn unknown_name_is_an_error() {
    assert!(!parses("NotAMessage", &[1, 2, 3]));
    assert!(!parses("", &[]));
}

#[test]
fn names_are_case_sensitive() {
    assert!(parses("Health", &[1]));
    assert!(!parses("health", &[1]));
    // HLTV and MOTD are registered upper-case; the CamelCase spellings are unknown.
    assert!(parses("HLTV", &[1, 2]));
    assert!(!parses("Hltv", &[1, 2]));
    assert!(parses("MOTD", &[1]));
    assert!(!parses("Motd", &[1]));
}

#[test]
fn trailing_nuls_on_the_name_are_trimmed() {
    // Names come from fixed-width svc_newusermsg fields, padded with NULs.
    let msg = match UserMessage::new(b"Health\0\0\0\0\0\0", &[42]) {
        Ok(UserMessage::Health(h)) => h,
        _ => panic!("padded name did not dispatch"),
    };
    assert_eq!(msg.0, 42);
}

#[test]
fn non_utf8_name_is_an_error() {
    assert!(UserMessage::new(&[0xff, 0xfe], &[1]).is_err());
}

#[test]
fn every_proper_prefix_of_a_fixed_payload_is_an_error() {
    // These accept shorter payloads by design, and are covered by their own tests.
    const ACCEPTS_PREFIXES: &[&str] = &[
        "AmmoPickup",
        "Object",
        "PShoot",
        "ShowMenu",
        "WeapPickup",
        "ServerName",
        "MOTD",
    ];
    for (name, data, _) in valid_cases() {
        if ACCEPTS_PREFIXES.contains(&name) {
            continue;
        }
        for len in 0..data.len() {
            assert!(
                !parses(name, &data[..len]),
                "{name} accepted truncated payload {:?}",
                &data[..len]
            );
        }
    }
}

#[test]
fn trailing_byte_is_rejected_except_where_the_parser_allows_it() {
    // Parsers that are not `all_consuming`, or whose last field soaks up the rest.
    const ACCEPTS_TRAILING: &[&str] = &[
        "AmmoPickup",  // take(i.len())
        "Object",      // take(i.len())
        "PShoot",      // take(i.len())
        "ShowMenu",    // take(i.len())
        "WeapPickup",  // take(i.len())
        "ServerName",  // whole payload is the string
        "MOTD",        // rest of payload is the string
        "TextMsg",     // a trailing NUL is one more (empty) optional arg
        "ClanTimer",   // not all_consuming
        "ClientAreas", // not all_consuming
    ];
    for (name, mut data, _) in valid_cases() {
        data.push(0);
        assert_eq!(
            parses(name, &data),
            ACCEPTS_TRAILING.contains(&name),
            "{name} with a trailing NUL: {data:?}"
        );
    }
}

/// Deterministic xorshift so the sweep is reproducible.
fn xorshift(state: &mut u32) -> u32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    *state
}

#[test]
fn no_parser_panics_on_arbitrary_bytes() {
    let mut names: Vec<&str> = valid_cases().into_iter().map(|(n, _, _)| n).collect();
    names.push("VGUIMenu");
    names.push("Unknown");
    let mut state = 0x1234_5678;
    for name in names {
        for _ in 0..2000 {
            let len = (xorshift(&mut state) % 24) as usize;
            let data: Vec<u8> = (0..len).map(|_| xorshift(&mut state) as u8).collect();
            let _ = UserMessage::new(name.as_bytes(), &data);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Shared field parsers.
// ---------------------------------------------------------------------------------------------

#[test]
fn null_string_reads_up_to_and_consumes_the_terminator() {
    let (rest, s) = null_string(b"abc\0rest").unwrap();
    assert_eq!(s, "abc");
    assert_eq!(rest, b"rest");

    let (rest, s) = null_string(b"\0").unwrap();
    assert_eq!(s, "");
    assert!(rest.is_empty());
}

#[test]
fn null_string_errors_without_terminator_or_on_bad_utf8() {
    assert!(null_string(b"").is_err());
    assert!(null_string(b"abc").is_err());
    assert!(null_string(&[0xff, 0x00]).is_err());
}

#[test]
fn team_ids() {
    let cases = [
        (0, Team::Unassigned),
        (1, Team::Allies),
        (2, Team::Axis),
        (3, Team::Spectators),
    ];
    for (id, expected) in cases {
        assert_eq!(team(&[id]).unwrap().1, expected, "team id {id}");
    }
    // British is never produced from the wire: British players are on team 1.
    for id in 4..=u8::MAX {
        assert!(team(&[id]).is_err(), "team id {id}");
    }
    assert!(team(&[]).is_err());
}

#[test]
fn team_from_str() {
    assert_eq!(Team::try_from("allies"), Ok(Team::Allies));
    assert_eq!(Team::try_from("axis"), Ok(Team::Axis));
    assert_eq!(Team::try_from("spectators"), Ok(Team::Spectators));
    assert_eq!(Team::try_from("unassigned"), Ok(Team::Unassigned));
    assert_eq!(Team::try_from("british"), Ok(Team::British));
    assert_eq!(Team::try_from("brit"), Ok(Team::British));
    assert_eq!(Team::try_from("Allies"), Err(()));
    assert_eq!(Team::try_from(""), Err(()));
}

#[test]
fn class_ids() {
    use Class::*;
    let expected = [
        Unassigned,
        Rifleman,
        StaffSergeant,
        MasterSergeant,
        Sergeant,
        Sniper,
        SupportInfantry,
        MachineGunner,
        Bazooka,
        Mortar,
        Grenadier,
        Stosstruppe,
        Unteroffizer,
        Sturmtruppe,
        Scharfschutze,
        Fg42Zweibein,
        Fg42Zielfernrohr,
        MG34Schutze,
        MG42Schutze,
        Panzerschreck,
        AxisMortar,
        BritishRifleman,
        SergeantMajor,
        Marksman,
        Gunner,
        RocketInfantry,
        BritishMortar,
        Random,
    ];
    for (id, class_) in expected.into_iter().enumerate() {
        assert_eq!(class(&[id as u8]).unwrap().1, class_, "class id {id}");
    }
    for id in 28..=u8::MAX {
        assert!(class(&[id]).is_err(), "class id {id}");
    }
}

#[test]
fn class_is_british() {
    let british: Vec<Class> = (0..=27u8)
        .map(|id| class(&[id]).unwrap().1)
        .filter(Class::is_british)
        .collect();
    assert_eq!(
        british,
        vec![
            Class::BritishRifleman,
            Class::SergeantMajor,
            Class::Marksman,
            Class::Gunner,
            Class::RocketInfantry,
            Class::BritishMortar,
        ]
    );
}

#[test]
fn weapon_ids_match_enum_discriminants() {
    let unknown_ids = [0u8, 15, 16, 33, 34, 41];
    for id in 0..=u8::MAX {
        let w = weapon(&[id]).unwrap().1;
        if id > 43 || unknown_ids.contains(&id) {
            assert_eq!(w, Weapon::Unknown, "weapon id {id}");
        } else {
            assert_eq!(w as u8, id, "weapon id {id}");
        }
    }
    assert!(weapon(&[]).is_err());
}

#[test]
fn weapon_is_grenade() {
    let grenades: Vec<Weapon> = (0..=43u8)
        .map(|id| weapon(&[id]).unwrap().1)
        .filter(Weapon::is_grenade)
        .collect();
    assert_eq!(
        grenades,
        vec![Weapon::Mk2Grenade, Weapon::StickGrenade, Weapon::MillsBomb]
    );
}

#[test]
fn ammo_ids() {
    let cases = [
        (0u8, "Unknown"),
        (1, "Smg"),
        (2, "AltRifle"),
        (3, "Rifle"),
        (4, "Pistol"),
        (5, "Springfield"),
        (6, "Heavy"),
        (7, "Mg42"),
        (8, "Browning30Cal"),
        (9, "Rocket"),
        (10, "Unknown"),
        (254, "Unknown"),
        (255, "Infinite"),
    ];
    for (id, expected) in cases {
        assert_eq!(
            format!("{:?}", ammo(&[id]).unwrap().1),
            expected,
            "ammo {id}"
        );
    }
    assert!(ammo(&[]).is_err());
}

#[test]
fn ammo_grenade_is_never_produced() {
    // BUG? `Ammo::Grenade` exists and is documented, but no wire id maps to it: grenade ammo
    // parses as `Unknown`.
    for id in 0..=u8::MAX {
        assert!(
            !matches!(ammo(&[id]).unwrap().1, Ammo::Grenade),
            "ammo {id}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Per-message parsers.
// ---------------------------------------------------------------------------------------------

#[test]
fn ammo_pickup_and_weap_pickup_accept_any_payload() {
    for data in [vec![], vec![0], vec![1, 2, 3, 4, 5]] {
        parse_as!("AmmoPickup", data, AmmoPickup);
        parse_as!("WeapPickup", data, WeapPickup);
    }
}

#[test]
fn ammo_short() {
    let m = parse_as!("AmmoShort", cat(&[&[1], &u16le(0x1234)]), AmmoShort);
    assert!(matches!(m.ammo, Ammo::Smg));
    assert_eq!(m.amount, 0x1234);

    let m = parse_as!("AmmoShort", cat(&[&[255], &u16le(u16::MAX)]), AmmoShort);
    assert!(matches!(m.ammo, Ammo::Infinite));
    assert_eq!(m.amount, u16::MAX);
}

#[test]
fn ammox() {
    let m = parse_as!("AmmoX", [3u8, 30], AmmoX);
    assert!(matches!(m.ammo, Ammo::Rifle));
    assert_eq!(m.amount, 30);
}

#[test]
fn blood_puff() {
    let m = parse_as!(
        "BloodPuff",
        cat(&[&i16le(-1), &i16le(2), &i16le(i16::MIN)]),
        BloodPuff
    );
    assert_eq!(m.0, (-1, 2, i16::MIN));
}

#[test]
fn camera_view() {
    let m = parse_as!("CameraView", b"camera_1\0", CameraView);
    assert_eq!(m.target_name, "camera_1");

    let m = parse_as!("CameraView", b"\0", CameraView);
    assert_eq!(m.target_name, "");

    assert!(!parses("CameraView", b"camera_1"));
    assert!(!parses("CameraView", b""));
}

#[test]
fn cancel_prog() {
    let m = parse_as!("CancelProg", [2u8, 7], CancelProg);
    assert_eq!(m.area_index, 2);
    assert_eq!(m._unk2, 7);
}

#[test]
fn cap_msg() {
    let m = parse_as!("CapMsg", cat(&[&[5], b"the Bridge\0", &[2]]), CapMsg);
    assert_eq!(m.client_index, 5);
    assert_eq!(m.point_name, "the Bridge");
    assert_eq!(m.team, Team::Axis);

    let m = parse_as!("CapMsg", [5u8, 0, 1], CapMsg);
    assert_eq!(m.point_name, "");
    assert_eq!(m.team, Team::Allies);

    // Team 4 is not a wire team.
    assert!(!parses("CapMsg", &cat(&[&[5], b"x\0", &[4]])));
}

#[test]
fn cl_corpse() {
    let data = cat(&[
        b"models/player.mdl\0",
        &i16le(100),
        &i16le(-200),
        &i16le(300),
        &[(-10i8) as u8, 20, (-30i8) as u8],
        &[7],
        &u16le(0x0102),
        &[2],
    ]);
    let m = parse_as!("ClCorpse", data, ClCorpse);
    assert_eq!(m.model_name, "models/player.mdl");
    assert_eq!(m.origin, (100, -200, 300));
    assert_eq!(m.angle, (-10, 20, -30));
    assert_eq!(m.animation_sequence, 7);
    assert_eq!(m.body, 0x0102);
    assert_eq!(m.team, Team::Axis);
}

#[test]
fn clan_timer() {
    let m = parse_as!("ClanTimer", [60u8], ClanTimer);
    assert_eq!(m.0, Duration::from_secs(60));

    let m = parse_as!("ClanTimer", [255u8], ClanTimer);
    assert_eq!(m.0, Duration::from_secs(255));

    assert!(!parses("ClanTimer", &[]));
}

#[test]
fn clan_timer_ignores_trailing_bytes() {
    // Unlike its neighbours, `clan_timer` is not `all_consuming`.
    let m = parse_as!("ClanTimer", [30u8, 0xAA, 0xBB], ClanTimer);
    assert_eq!(m.0, Duration::from_secs(30));
}

#[test]
fn client_areas_without_icon() {
    // Any flag other than 255 (-1 as a byte) means no icon string follows.
    for flag in [0u8, 1, 2, 254] {
        let m = parse_as!("ClientAreas", [4u8, flag], ClientAreas);
        assert_eq!(m.icon_index, 4);
        assert_eq!(m.hud_icon, None);
    }
}

#[test]
fn client_areas_with_icon() {
    let m = parse_as!(
        "ClientAreas",
        cat(&[&[4, 255], b"sprites/obj\0"]),
        ClientAreas
    );
    assert_eq!(m.icon_index, 4);
    assert_eq!(m.hud_icon.as_deref(), Some("sprites/obj"));

    let m = parse_as!("ClientAreas", [4u8, 255, 0], ClientAreas);
    assert_eq!(m.hud_icon.as_deref(), Some(""));

    // Flag 255 promises a string: missing or unterminated is an error.
    assert!(!parses("ClientAreas", &[4, 255]));
    assert!(!parses("ClientAreas", &cat(&[&[4, 255], b"abc"])));
    assert!(!parses("ClientAreas", &[4]));
}

#[test]
fn client_areas_ignores_trailing_bytes() {
    // Not `all_consuming`: whatever follows is dropped silently.
    let m = parse_as!("ClientAreas", [4u8, 0, 1, 2, 3], ClientAreas);
    assert_eq!(m.hud_icon, None);

    let m = parse_as!("ClientAreas", cat(&[&[4, 255], b"a\0junk"]), ClientAreas);
    assert_eq!(m.hud_icon.as_deref(), Some("a"));
}

#[test]
fn cur_marker() {
    let m = parse_as!("CurMarker", [3u8], CurMarker);
    assert_eq!(m.marker_id, 3);
}

#[test]
fn cur_weapon() {
    let m = parse_as!("CurWeapon", [1u8, 12, 30], CurWeapon);
    assert!(m.is_active);
    assert_eq!(m.weapon, Weapon::Mp40);
    assert_eq!(m.clip_ammo, 30);

    let m = parse_as!("CurWeapon", [0u8, 5, 0], CurWeapon);
    assert!(!m.is_active);
    assert_eq!(m.weapon, Weapon::Garand);

    // Any non-zero byte is "active".
    let m = parse_as!("CurWeapon", [2u8, 5, 0], CurWeapon);
    assert!(m.is_active);

    // An unknown weapon id does not fail the message.
    let m = parse_as!("CurWeapon", [1u8, 99, 0], CurWeapon);
    assert_eq!(m.weapon, Weapon::Unknown);
}

#[test]
fn death_msg() {
    let m = parse_as!("DeathMsg", [3u8, 7, 37], DeathMsg);
    assert_eq!(m.killer_client_index, 3);
    assert_eq!(m.victim_client_index, 7);
    assert_eq!(m.weapon, Weapon::K98Bayonet);

    // World/suicide kill: killer 0, weapon 0.
    let m = parse_as!("DeathMsg", [0u8, 7, 0], DeathMsg);
    assert_eq!(m.killer_client_index, 0);
    assert_eq!(m.weapon, Weapon::Unknown);
}

#[test]
fn frags_and_obj_score_are_signed() {
    let m = parse_as!("Frags", cat(&[&[4], &i16le(-3)]), Frags);
    assert_eq!((m.client_index, m.frags), (4, -3));

    let m = parse_as!("ObjScore", cat(&[&[3], &i16le(i16::MAX)]), ObjScore);
    assert_eq!((m.client_index, m.score), (3, i16::MAX));
}

#[test]
fn two_byte_messages() {
    let m = parse_as!("GameRules", [1u8, 0], GameRules);
    assert_eq!((m._unk1, m._unk2), (1, 0));

    let m = parse_as!("HandSignal", [4u8, 2], HandSignal);
    assert_eq!((m.client_index, m.animation_id), (4, 2));

    let m = parse_as!("HLTV", [5u8, 128], Hltv);
    assert_eq!((m.client_id, m.flags), (5, 128));

    let m = parse_as!("PStatus", [3u8, 1], PStatus);
    assert_eq!((m.client_index, m.status), (3, 1));

    let m = parse_as!("Spectator", [3u8, 1], Spectator);
    assert_eq!(m.client_index, 3);
    assert!(m.is_spectator);
    let m = parse_as!("Spectator", [3u8, 0], Spectator);
    assert!(!m.is_spectator);
}

#[test]
fn one_byte_value_messages() {
    assert_eq!(parse_as!("Health", [100u8], Health).0, 100);
    assert_eq!(parse_as!("Health", [255u8], Health).0, 255);
    assert_eq!(parse_as!("HideWeapon", [9u8], HideWeapon).flags, 9);
    assert_eq!(parse_as!("SetFOV", [90u8], SetFOV).0, 90);
    assert_eq!(parse_as!("StatusValue", [50u8], StatusValue).0, 50);
    assert_eq!(parse_as!("WaveStatus", [2u8], WaveStatus).0, 2);
    assert_eq!(
        parse_as!("WaveTime", [15u8], WaveTime).0,
        Duration::from_secs(15)
    );
    assert!(parse_as!("UseSound", [1u8], UseSound).is_entity_in_sphere);
    assert!(!parse_as!("UseSound", [0u8], UseSound).is_entity_in_sphere);
    parse_as!("Scope", [6u8], Scope);
}

#[test]
fn hud_text() {
    let m = parse_as!("HudText", cat(&[b"#Hint_text\0", &[1]]), HudText);
    assert_eq!(m.text, "#Hint_text");
    assert_eq!(m.init_hud_style, 1);

    let m = parse_as!("HudText", [0u8, 0], HudText);
    assert_eq!(m.text, "");
    assert_eq!(m.init_hud_style, 0);

    // The style byte is required.
    assert!(!parses("HudText", b"text\0"));
}

#[test]
fn empty_messages_reject_any_payload() {
    for name in ["InitHUD", "ReloadDone", "ReqState", "ResetHUD", "ResetSens"] {
        assert!(parses(name, &[]), "{name} empty");
        assert!(!parses(name, &[0]), "{name} with one byte");
    }
}

#[test]
fn opaque_messages_accept_any_payload() {
    for data in [vec![], vec![1], vec![0xff; 40]] {
        parse_as!("Object", data, Object);
        parse_as!("PShoot", data, PShoot);
        parse_as!("ShowMenu", data, ShowMenu);
    }
}

#[test]
fn fixed_length_opaque_messages() {
    for (name, len) in [
        ("MapMarker", 6),
        ("TimerStatus", 3),
        ("Weather", 2),
        ("YouDied", 1),
    ] {
        assert!(parses(name, &vec![0xAB; len]), "{name} with {len} bytes");
        assert!(!parses(name, &vec![0xAB; len - 1]), "{name} short");
        assert!(!parses(name, &vec![0xAB; len + 1]), "{name} long");
    }
}

/// Encodes InitObj objectives as `(entity_index, area_index, team, origin)`, with `_unk1`
/// fixed to 1 and icons derived from the area index.
fn objectives(objs: &[(u16, u8, u8, (i16, i16))]) -> Vec<u8> {
    let mut out = vec![objs.len() as u8];
    for &(entity, area, team, (x, y)) in objs {
        out.extend_from_slice(&entity.to_le_bytes());
        out.extend_from_slice(&[area, team, 1, area * 3, area * 3 + 1, area * 3 + 2]);
        out.extend_from_slice(&x.to_le_bytes());
        out.extend_from_slice(&y.to_le_bytes());
    }
    out
}

#[test]
fn init_obj() {
    let data = objectives(&[(300, 0, 1, (-1024, 2048)), (301, 1, 2, (0, -1))]);
    assert_eq!(data.len(), 1 + 2 * 12);
    let m = parse_as!("InitObj", data, InitObj);
    assert_eq!(m.objectives.len(), 2);

    let o = &m.objectives[0];
    assert_eq!(o.entity_index, 300);
    assert_eq!(o.area_index, 0);
    assert_eq!(o.team, Some(Team::Allies));
    assert_eq!(o._unk1, 1);
    assert_eq!(
        (o.neutral_icon_index, o.allies_icon_index, o.axis_icon_index),
        (0, 1, 2)
    );
    assert_eq!(o.origin, (-1024, 2048));

    let o = &m.objectives[1];
    assert_eq!(o.entity_index, 301);
    assert_eq!(o.team, Some(Team::Axis));
    assert_eq!(
        (o.neutral_icon_index, o.allies_icon_index, o.axis_icon_index),
        (3, 4, 5)
    );
    assert_eq!(o.origin, (0, -1));
}

#[test]
fn init_obj_empty_and_max_count() {
    let m = parse_as!("InitObj", [0u8], InitObj);
    assert!(m.objectives.is_empty());

    let objs: Vec<_> = (0..255u16).map(|i| (i, 0, 2, (0, 0))).collect();
    let m = parse_as!("InitObj", objectives(&objs), InitObj);
    assert_eq!(m.objectives.len(), 255);
    assert_eq!(m.objectives[254].entity_index, 254);
}

#[test]
fn init_obj_count_larger_than_payload_is_an_error() {
    let mut data = objectives(&[(1, 0, 1, (0, 0))]);
    data[0] = 255;
    assert!(!parses("InitObj", &data));

    // Count says 1, two objectives follow.
    let mut data = objectives(&[(1, 0, 1, (0, 0)), (2, 1, 1, (0, 0))]);
    data[0] = 1;
    assert!(!parses("InitObj", &data));
}

#[test]
fn init_obj_team_byte() {
    // BUG? The `tag("\x00").map(|_| None)` arm is unreachable: `team` already maps 0 to
    // `Unassigned`, so a neutral objective is `Some(Unassigned)` and `team` is never `None`.
    let m = parse_as!("InitObj", objectives(&[(1, 0, 0, (0, 0))]), InitObj);
    assert_eq!(m.objectives[0].team, Some(Team::Unassigned));

    // Spectators (3) is accepted as an owner; 4+ fails the whole message.
    let m = parse_as!("InitObj", objectives(&[(1, 0, 3, (0, 0))]), InitObj);
    assert_eq!(m.objectives[0].team, Some(Team::Spectators));
    assert!(!parses("InitObj", &objectives(&[(1, 0, 4, (0, 0))])));
}

#[test]
fn motd() {
    let m = parse_as!("MOTD", cat(&[&[1], b"Welcome"]), Motd);
    assert!(m.is_terminal);
    assert_eq!(m.text, "Welcome");

    let m = parse_as!("MOTD", [0u8], Motd);
    assert!(!m.is_terminal);
    assert_eq!(m.text, "");

    assert!(!parses("MOTD", &[]));
    assert!(!parses("MOTD", &[0, 0xff]));
}

#[test]
fn motd_keeps_the_trailing_nul() {
    // BUG? The text is the raw rest of the payload, so a NUL-terminated chunk (as written by
    // WRITE_STRING) keeps its terminator in `text`.
    let m = parse_as!("MOTD", cat(&[&[0], b"Hi\0"]), Motd);
    assert_eq!(m.text, "Hi\0");
}

#[test]
fn p_class() {
    let m = parse_as!("PClass", [3u8, 21], PClass);
    assert_eq!(m.client_index, 3);
    assert_eq!(m.class, Class::BritishRifleman);
    assert!(!parses("PClass", &[3, 28]));
}

#[test]
fn p_team() {
    let m = parse_as!("PTeam", [3u8, 2], PTeam);
    assert_eq!((m.client_index, m.team), (3, Team::Axis));
    let m = parse_as!("PTeam", [3u8, 0], PTeam);
    assert_eq!(m.team, Team::Unassigned);
    assert!(!parses("PTeam", &[3, 4]));
}

#[test]
fn players_in() {
    let m = parse_as!("PlayersIn", [1u8, 2, 3, 4], PlayersIn);
    assert_eq!(m.area_index, 1);
    assert_eq!(m.team, Team::Axis);
    assert_eq!(m.players_inside_area, 3);
    assert_eq!(m.required_players_to_capture, 4);
    assert!(!parses("PlayersIn", &[1, 9, 3, 4]));
}

#[test]
fn round_state() {
    let cases: [(u8, fn(&RoundState) -> bool); 5] = [
        (0, |r| matches!(r, RoundState::Reset)),
        (1, |r| matches!(r, RoundState::Start)),
        (3, |r| matches!(r, RoundState::AlliesWin)),
        (4, |r| matches!(r, RoundState::AxisWin)),
        (5, |r| matches!(r, RoundState::Draw)),
    ];
    for (id, check) in cases {
        let r = parse_as!("RoundState", [id], RoundState);
        assert!(check(&r), "round state {id} gave {r:?}");
    }
    for id in [2u8, 6, 255] {
        assert!(!parses("RoundState", &[id]), "round state {id}");
    }
}

#[test]
fn say_text() {
    let m = parse_as!("SayText", cat(&[&[3], b"\x02Player : gg\n\0"]), SayText);
    assert_eq!(m.client_index, 3);
    assert_eq!(m.text, "\x02Player : gg\n");

    let m = parse_as!("SayText", [0u8, 0], SayText);
    assert_eq!(m.text, "");

    assert!(!parses("SayText", &cat(&[&[3], b"no terminator"])));
    assert!(!parses("SayText", &[3, 0xc3, 0x28, 0])); // invalid UTF-8
}

#[test]
fn score_info() {
    let data = [3u8, (-5i8) as u8, 12, 127, 21, 1, 0xEE];
    let m = parse_as!("ScoreInfo", data, ScoreInfo);
    assert_eq!(m.client_index, 3);
    assert_eq!((m.points, m.kills, m.deaths), (-5, 12, 127));
    assert_eq!(m.class, Class::BritishRifleman);
    assert_eq!(m.team, Team::Allies);
}

#[test]
fn score_info_long() {
    let data = cat(&[
        &[3],
        &i16le(-5),
        &i16le(1000),
        &i16le(i16::MAX),
        &[19, 2, 0xEE],
    ]);
    assert_eq!(data.len(), 10);
    let m = parse_as!("ScoreInfoLong", data, ScoreInfoLong);
    assert_eq!(m.client_index, 3);
    assert_eq!((m.score, m.frags, m.deaths), (-5, 1000, i16::MAX));
    assert_eq!(m.class, Class::Panzerschreck);
    assert_eq!(m.team, Team::Axis);
}

#[test]
fn score_short() {
    let data = cat(&[&[3], &i16le(7), &i16le(-2), &i16le(300), &[0xEE]]);
    assert_eq!(data.len(), 8);
    let m = parse_as!("ScoreShort", data, ScoreShort);
    assert_eq!(m.client_index, 3);
    assert_eq!((m.score, m.kills, m.deaths), (7, -2, 300));
}

#[test]
fn screen_fade() {
    // The head-hit red screen: (128, 0, 0, 200).
    let data = cat(&[&u16le(0x1000), &u16le(0x0800), &u16le(1), &[128, 0, 0, 200]]);
    let m = parse_as!("ScreenFade", data, ScreenFade);
    assert_eq!(m.duration, 0x1000);
    assert_eq!(m.hold_time, 0x0800);
    assert_eq!(m.flags, 1);
    assert_eq!(m.color, (128, 0, 0, 200));
}

#[test]
fn screen_shake() {
    let data = cat(&[&u16le(4096 * 5), &u16le(4096 * 2), &u16le(4096 * 3)]);
    let m = parse_as!("ScreenShake", data, ScreenShake);
    assert_eq!(m.amplitude, 5);
    assert_eq!(m.duration, Duration::from_secs(2));
    assert_eq!(m.frequency, 3);

    let m = parse_as!(
        "ScreenShake",
        cat(&[&u16le(0), &u16le(2048), &u16le(0)]),
        ScreenShake
    );
    assert_eq!(m.duration, Duration::from_millis(500));

    let m = parse_as!(
        "ScreenShake",
        cat(&[&u16le(u16::MAX), &u16le(u16::MAX), &u16le(u16::MAX)]),
        ScreenShake
    );
    assert_eq!((m.amplitude, m.frequency), (15, 15));
}

#[test]
fn screen_shake_truncates_amplitude_and_frequency() {
    // BUG? amplitude and frequency are 4.12 fixed point but integer-divided by 4096, so the
    // fraction is lost: 5.5 reads as 5, and anything under 1.0 reads as 0. `duration` keeps it.
    let half = 2048u16;
    let m = parse_as!(
        "ScreenShake",
        cat(&[&u16le(4096 * 5 + half), &u16le(half), &u16le(half)]),
        ScreenShake
    );
    assert_eq!(m.amplitude, 5);
    assert_eq!(m.frequency, 0);
    assert_eq!(m.duration, Duration::from_millis(500));
}

#[test]
fn server_name() {
    let m = parse_as!("ServerName", b"My Server", ServerName);
    assert_eq!(m.0, "My Server");

    let m = parse_as!("ServerName", b"", ServerName);
    assert_eq!(m.0, "");

    assert!(!parses("ServerName", &[0xff]));
}

#[test]
fn server_name_keeps_the_trailing_nul() {
    // BUG? `wrapped_string` takes the whole payload, so a WRITE_STRING terminator stays in the
    // name.
    let m = parse_as!("ServerName", b"My Server\0", ServerName);
    assert_eq!(m.0, "My Server\0");
}

#[test]
fn set_obj() {
    let m = parse_as!("SetObj", [1u8, 2, 0], SetObj);
    assert_eq!(m.area_index, 1);
    assert_eq!(m.team, Some(Team::Axis));

    let m = parse_as!("SetObj", [1u8, 1, 0xEE], SetObj);
    assert_eq!(m.team, Some(Team::Allies));

    assert!(!parses("SetObj", &[1, 4, 0]));
}

#[test]
fn set_obj_neutral_is_some_unassigned() {
    // BUG? As with InitObj, the `None` arm is unreachable: a flag reset to neutral (team 0)
    // parses as `Some(Unassigned)`.
    let m = parse_as!("SetObj", [2u8, 0, 0], SetObj);
    assert_eq!(m.team, Some(Team::Unassigned));
}

#[test]
fn start_prog() {
    let m = parse_as!("StartProg", cat(&[&[1, 1], &u16le(5)]), StartProg);
    assert_eq!(m.area_index, 1);
    assert_eq!(m.team, Team::Allies);
    assert_eq!(m.cap_duration, Duration::from_secs(5));

    let m = parse_as!("StartProg", cat(&[&[1, 2], &u16le(u16::MAX)]), StartProg);
    assert_eq!(m.cap_duration, Duration::from_secs(u16::MAX as u64));
}

#[test]
fn start_prog_f() {
    // Six bytes: byte, byte, f32.
    let data = cat(&[&[1, 2], &1.5f32.to_le_bytes()]);
    assert_eq!(data.len(), 6);
    let m = parse_as!("StartProgF", data, StartProgF);
    assert_eq!(m.area_index, 1);
    assert_eq!(m.team, Team::Axis);
    assert_eq!(m.cap_duration, Duration::from_millis(1500));

    let m = parse_as!(
        "StartProgF",
        cat(&[&[0, 1], &0f32.to_le_bytes()]),
        StartProgF
    );
    assert_eq!(m.cap_duration, Duration::ZERO);
}

#[test]
fn start_prog_f_rejects_a_duration_that_is_not_one() {
    // `Duration::from_secs_f32` used to panic on these, aborting analysis of a
    // corrupt demo.
    let negative = cat(&[&[1, 2], &(-1.0f32).to_le_bytes()]);
    let nan = [1, 2, 0xff, 0xff, 0xff, 0xff];
    let infinite = cat(&[&[1, 2], &f32::INFINITY.to_le_bytes()]);
    for data in [&negative[..], &nan, &infinite] {
        assert!(UserMessage::new(b"StartProgF", data).is_err(), "{data:?}");
    }
}

#[test]
fn team_score() {
    let m = parse_as!("TeamScore", cat(&[&[1], &u16le(300)]), TeamScore);
    assert_eq!(m.team, Team::Allies);
    assert_eq!(m.score, 300);
    assert!(!parses("TeamScore", &cat(&[&[7], &u16le(1)])));
}

#[test]
fn text_msg_without_args() {
    let m = parse_as!("TextMsg", cat(&[&[4], b"#Game_joined\0"]), TextMsg);
    assert_eq!(m.destination, 4);
    assert_eq!(m.text, "#Game_joined");
    assert_eq!((m.arg1, m.arg2, m.arg3, m.arg4), (None, None, None, None));
}

#[test]
fn text_msg_with_args() {
    let data = cat(&[&[3], b"%s %s %s %s\0", b"a\0", b"b\0", b"c\0", b"d\0"]);
    let m = parse_as!("TextMsg", data, TextMsg);
    assert_eq!(m.text, "%s %s %s %s");
    assert_eq!(m.arg1.as_deref(), Some("a"));
    assert_eq!(m.arg2.as_deref(), Some("b"));
    assert_eq!(m.arg3.as_deref(), Some("c"));
    assert_eq!(m.arg4.as_deref(), Some("d"));

    let m = parse_as!("TextMsg", cat(&[&[3], b"x\0", b"a\0", b"b\0"]), TextMsg);
    assert_eq!(m.arg2.as_deref(), Some("b"));
    assert_eq!((m.arg3, m.arg4), (None, None));
}

#[test]
fn text_msg_edges() {
    // A trailing NUL is an empty arg, not ignored.
    let m = parse_as!("TextMsg", cat(&[&[3], b"x\0\0"]), TextMsg);
    assert_eq!(m.arg1.as_deref(), Some(""));

    // A fifth arg, or unterminated trailing text, fails the whole message.
    let five = cat(&[&[3], b"x\0", b"1\0", b"2\0", b"3\0", b"4\0", b"5\0"]);
    assert!(!parses("TextMsg", &five));
    assert!(!parses("TextMsg", &cat(&[&[3], b"x\0", b"dangling"])));
    assert!(!parses("TextMsg", &[3]));
}

#[test]
fn time_left() {
    let m = parse_as!("TimeLeft", u16le(600), TimeLeft);
    assert_eq!(m.0, Duration::from_secs(600));
}

#[test]
fn vgui_menu_always_fails() {
    // `vgui_menu` is a stub that never parses, whatever the payload.
    for data in [vec![], vec![1], vec![1, 2, 3, 4, 5]] {
        assert!(!parses("VGUIMenu", &data));
    }
}

#[test]
fn voice_mask() {
    let data = cat(&[&(-1i32).to_le_bytes(), &0x0000_0006i32.to_le_bytes()]);
    let m = parse_as!("VoiceMask", data, VoiceMask);
    assert_eq!(m.audible_players, -1);
    assert_eq!(m.banned_players, 6);
}

#[test]
fn weapon_list() {
    let m = parse_as!(
        "WeaponList",
        [1u8, 210, 255, 0, 2, 1, 7, 0xAA, 0xBB, 30],
        WeaponList
    );
    assert!(matches!(m.primary_ammo, Ammo::Smg));
    assert_eq!(m.primary_ammo_max, 210);
    assert!(matches!(m.secondary_ammo, Ammo::Infinite));
    assert_eq!(m.secondary_ammo_max, 0);
    assert_eq!(m.slot, 2);
    assert_eq!(m.position_in_slot, 1);
    assert_eq!(m.weapon, Weapon::Thompson);
    assert_eq!((m._unk1, m._unk2), (0xAA, 0xBB));
    assert_eq!(m.clip_size, 30);
}
