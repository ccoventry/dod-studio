// maps/map_check.rs
// Does the map this demo was recorded on exist here, and is it the same build?
//
// A demo names its map in the header and stamps the map's checksum beside it.
// That makes three states worth telling apart, not two:
//
//   * the map is missing — the demo cannot be played, let alone captured
//   * the map is present but a DIFFERENT BUILD — it plays, and everything
//     derived from its geometry is quietly wrong
//   * the map is present and matches
//
// The middle state is the one worth the effort. `_b2` against `_b3e`, or a
// recompile that kept the name, gives a demo that loads and looks approximately
// right while every coordinate taken from the map refers to a different world.
// Nothing in playback announces it.
//
// The checksum itself is `bsp::map_checksum`. Reading it back out of a demo
// costs the 544-byte header, plus 8 KB more for an HLTV demo, so a
// whole folder can be checked at load without parsing a single frame.
//
// A POV demo's header carries the checksum. An HLTV demo's header zeroes it,
// but its first `svc_serverinfo` still carries the same value scrambled
// (`dod::server_info`), so the build check works for both (#1539).

use std::path::{Path, PathBuf};

use super::bsp;

/// Demo header layout, up to the field this module needs. The map name is a
/// NUL-padded 260-byte string and the checksum is the `u32` after the game
/// directory.
const HEADER_READ_LEN: usize = crate::patch::DEMO_HEADER_SIZE;
/// How much of the file's start is read when the header has no checksum: the
/// LOADING segment's first frames, where `svc_serverinfo` sits (it ended by
/// about 1.2 KB in all 670 demos surveyed).
const SERVER_INFO_WINDOW: usize = 8 * 1024;
const MAGIC: &[u8] = b"HLDEMO\0\0";
const MAP_NAME_AT: usize = 16;
const MAP_NAME_LEN: usize = 260;
const MAP_CHECKSUM_AT: usize = 536;

/// What a demo needs from the map library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapReference {
    pub map_name: String,
    /// The build the demo was recorded on: the header's checksum, or when the
    /// header's is zero (an HLTV demo) the one in its `svc_serverinfo`. `None`
    /// when neither is there: a local single-player recording has a zero
    /// header and a server info that unscrambles to 0 too, see
    /// `MapStatus::Unverifiable`.
    pub expected_checksum: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapStatus {
    /// Present, and the same build the demo was recorded on.
    Ok { checksum: u32 },
    /// Present, but a different build. Playback will work and be wrong.
    WrongBuild { expected: u32, found: u32 },
    /// Not in the map library at all.
    Missing,
    /// Present, but the demo does not say which build it wants: no header
    /// checksum and no usable `svc_serverinfo` one (a local single-player
    /// recording, whose is 0; a demo cut short; a server this reader does not
    /// know). The most that can be said is that a map by that name is here.
    Unverifiable,
    /// Present and unreadable — truncated, or not a BSP at all.
    Unreadable { reason: String },
}

impl MapStatus {
    /// Whether the demo can be played at all.
    pub fn is_playable(&self) -> bool {
        !matches!(self, MapStatus::Missing)
    }

    /// One line, for a log or a list row.
    pub fn summary(&self, map_name: &str) -> String {
        match self {
            MapStatus::Ok { .. } => format!("`{}` matches", map_name),
            MapStatus::WrongBuild { expected, found } => format!(
                "`{}` is a different build — the demo wants `{:08x}`, this one is `{:08x}`",
                map_name, expected, found
            ),
            MapStatus::Missing => format!("`{}` is missing", map_name),
            MapStatus::Unverifiable => format!(
                "`{}` is present, but the demo does not say which build it needs",
                map_name
            ),
            MapStatus::Unreadable { reason } => {
                format!("`{}` could not be read: {}", map_name, reason)
            }
        }
    }
}

/// The map a demo was recorded on, from its 544-byte header alone: for a
/// caller that only wants the name. Fails on a file that is not a demo or
/// names no usable map.
pub fn map_name(demo: &Path) -> Result<String, String> {
    read_header(demo).map(|(_, name, _)| name)
}

/// The map a demo was recorded on and the build it wants: the header, and
/// when the header has no checksum a further 8 KB for the first
/// `svc_serverinfo`.
pub fn map_reference(demo: &Path) -> Result<MapReference, String> {
    let (mut file, name, header_checksum) = read_header(demo)?;
    let expected_checksum = match header_checksum {
        0 => server_info_checksum_of(&mut file),
        checksum => Some(checksum),
    };
    Ok(MapReference {
        map_name: name,
        expected_checksum,
    })
}

/// The demo's open file, its map name and its header checksum, from the first
/// 544 bytes. The file is left positioned after them.
fn read_header(demo: &Path) -> Result<(std::fs::File, String, u32), String> {
    use std::io::Read;

    let mut file = std::fs::File::open(demo).map_err(|e| format!("{}: {}", demo.display(), e))?;
    let mut head = vec![0u8; HEADER_READ_LEN];
    file.read_exact(&mut head)
        .map_err(|_| format!("{}: shorter than a demo header", demo.display()))?;

    if &head[..MAGIC.len()] != MAGIC {
        return Err(format!("{}: not a HLDEMO file", demo.display()));
    }

    let raw = &head[MAP_NAME_AT..MAP_NAME_AT + MAP_NAME_LEN];
    let name: String = String::from_utf8_lossy(
        &raw.iter()
            .copied()
            .take_while(|b| *b != 0)
            .collect::<Vec<u8>>(),
    )
    .trim()
    .to_lowercase();

    if name.is_empty() || !is_safe_map_name(&name) {
        return Err(format!(
            "{}: header carries no usable map name",
            demo.display()
        ));
    }

    let checksum = u32::from_le_bytes([
        head[MAP_CHECKSUM_AT],
        head[MAP_CHECKSUM_AT + 1],
        head[MAP_CHECKSUM_AT + 2],
        head[MAP_CHECKSUM_AT + 3],
    ]);

    Ok((file, name, checksum))
}

/// The checksum in the first `svc_serverinfo` after the header, from the next
/// [`SERVER_INFO_WINDOW`] bytes of `file` (positioned after the header).
///
/// An HLTV header leaves its checksum zeroed. A zero is never a checksum, so
/// a map is never called the wrong build on the strength of a field that was
/// never filled in: the server info's value stands in for it.
fn server_info_checksum_of(file: &mut std::fs::File) -> Option<u32> {
    use std::io::Read;

    let mut window = vec![0u8; HEADER_READ_LEN];
    (&mut *file)
        .take(SERVER_INFO_WINDOW as u64)
        .read_to_end(&mut window)
        .ok()?;
    server_info_checksum(&window)
}

/// The map checksum in the first `svc_serverinfo` of `file_start` (a demo's
/// start, header included), when there is one and it is not zero. A local
/// single-player recording unscrambles to 0 and has none.
fn server_info_checksum(file_start: &[u8]) -> Option<u32> {
    dod::server_info::find_server_info(file_start)
        .map(|stamp| stamp.map_checksum())
        .filter(|checksum| *checksum != 0)
}

/// A map name is a short identifier. One that is not is not a map name worth
/// joining onto a path.
pub fn is_safe_map_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Where a map by this name would live.
pub fn map_path(maps_dir: &Path, map_name: &str) -> PathBuf {
    maps_dir.join(format!("{}.bsp", map_name))
}

/// The map library belonging to a given `hl.exe`.
///
/// DoD's content sits beside the executable, so maps are
/// `<hl.exe dir>/<game folder>/maps` (`dod` unless the Game folder setting says
/// otherwise, #402) — and note that demos live *inside* the game folder, one
/// level below the exe. Deriving
/// this from a demo's own folder instead gives `dod/dod/maps`, which exists
/// nowhere and fails silently.
///
/// `None` when that does not resolve to a real directory, rather than handing
/// back a path that can only fail later.
pub fn maps_dir_for_exe(exe: &Path) -> Option<PathBuf> {
    let dir = crate::shared::paths::mod_dir_for_exe(exe)?.join("maps");
    dir.is_dir().then_some(dir)
}

/// Check one reference against a map library.
pub fn status_of(reference: &MapReference, maps_dir: &Path) -> MapStatus {
    let path = map_path(maps_dir, &reference.map_name);
    if !path.is_file() {
        return MapStatus::Missing;
    }
    let Some(expected) = reference.expected_checksum else {
        return MapStatus::Unverifiable;
    };
    match bsp::map_checksum_of_file(&path) {
        Ok(found) if found == expected => MapStatus::Ok { checksum: found },
        Ok(found) => MapStatus::WrongBuild { expected, found },
        Err(reason) => MapStatus::Unreadable {
            reason: reason.to_string(),
        },
    }
}

/// Everything about one demo's map, in a single call.
pub fn check_demo(demo: &Path, maps_dir: &Path) -> Result<(MapReference, MapStatus), String> {
    let reference = map_reference(demo)?;
    let status = status_of(&reference, maps_dir);
    Ok((reference, status))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    fn header(map: &str, checksum: u32) -> Vec<u8> {
        let mut head = vec![0u8; HEADER_READ_LEN];
        head[..MAGIC.len()].copy_from_slice(MAGIC);
        head[MAP_NAME_AT..MAP_NAME_AT + map.len()].copy_from_slice(map.as_bytes());
        head[MAP_CHECKSUM_AT..MAP_CHECKSUM_AT + 4].copy_from_slice(&checksum.to_le_bytes());
        head
    }

    fn scratch(tag: &str) -> Scratch {
        Scratch::new(format_args!("map_check_{tag}"))
    }

    /// A demo start as an HLTV proxy writes it: a header with no checksum, then
    /// a `svc_serverinfo` carrying `checksum` scrambled the way the engine does
    /// for `slot` (the inverse of `dod::server_info::unscramble_map_checksum`).
    fn hltv_demo(map: &str, checksum: u32, slot: u8) -> Vec<u8> {
        demo_with_server_info(map, checksum, slot, 32)
    }

    /// `hltv_demo` with a chosen player count.
    fn demo_with_server_info(map: &str, checksum: u32, slot: u8, max_players: u8) -> Vec<u8> {
        let key = !slot;
        let mut bytes = (checksum ^ !u32::from(key)).to_be_bytes();
        for (j, byte) in bytes.iter_mut().enumerate() {
            let table = [0x20u8, 0x07, 0x13, 0x61][j];
            *byte ^= (0xa5 | (j << j) | j | usize::from(table)) as u8;
        }
        let scrambled = u32::from_le_bytes(bytes) ^ u32::from(key);

        let mut demo = header(map, 0);
        demo.extend_from_slice(&[0u8; 200]);
        demo.push(11);
        demo.extend_from_slice(&48i32.to_le_bytes());
        demo.extend_from_slice(&1i32.to_le_bytes());
        demo.extend_from_slice(&scrambled.to_le_bytes());
        demo.extend_from_slice(&[0u8; 16]);
        demo.extend_from_slice(&[max_players, slot, 1]);
        demo.extend_from_slice(b"dod\0server\0maps/x.bsp\0\0\0");
        demo
    }

    /// The smallest map `bsp::map_checksum` reads: a v30 header, fifteen empty
    /// lumps. Its checksum is `0xffff_ffff`.
    fn empty_map(maps: &Path, name: &str) -> u32 {
        let mut bytes = vec![0u8; 4 + 15 * 8];
        bytes[0] = 30;
        std::fs::create_dir_all(maps).unwrap();
        std::fs::write(map_path(maps, name), &bytes).unwrap();
        bsp::map_checksum(&bytes).unwrap()
    }

    #[test]
    fn a_header_yields_the_map_and_the_build_it_wants() {
        let dir = scratch("ref");
        let demo = dir.join("a.dem");
        std::fs::write(&demo, header("dod_anzio", 0x801b_771f)).unwrap();

        let r = map_reference(&demo).unwrap();
        assert_eq!(r.map_name, "dod_anzio");
        assert_eq!(r.expected_checksum, Some(0x801b_771f));
    }

    #[test]
    fn an_hltv_demo_states_no_build_rather_than_a_zero_one() {
        // HLTV leaves the field zeroed. Read as a checksum it would call every
        // map on disk the wrong build; read as "not stated" it says only what
        // the demo actually says.
        let dir = scratch("hltv");
        let demo = dir.join("a.dem");
        std::fs::write(&demo, header("dod_harrington", 0)).unwrap();

        assert_eq!(map_reference(&demo).unwrap().expected_checksum, None);
    }

    #[test]
    fn an_hltv_demo_states_the_build_its_server_info_carries() {
        // #1539: the header is zeroed, the server info is not.
        let dir = scratch("hltv_serverinfo");
        let demo = dir.join("a.dem");
        std::fs::write(&demo, hltv_demo("dod_anzio", 0x801b_771f, 5)).unwrap();

        assert_eq!(
            map_reference(&demo).unwrap().expected_checksum,
            Some(0x801b_771f)
        );
    }

    #[test]
    fn a_local_single_player_recording_states_no_build() {
        // Seen in 4 real demos: header checksum 0, one player, and a server
        // info whose checksum unscrambles to 0 as well. Not a build to check.
        let dir = scratch("single_player");
        let demo = dir.join("a.dem");
        std::fs::write(&demo, demo_with_server_info("dod_anzio", 0, 0, 1)).unwrap();

        let reference = map_reference(&demo).unwrap();
        assert_eq!(reference.expected_checksum, None);
        assert_eq!(
            status_of(&reference, &dir),
            MapStatus::Missing,
            "no map here, and never a wrong build"
        );
    }

    #[test]
    fn the_map_name_alone_needs_only_the_header() {
        let dir = scratch("name_only");
        let demo = dir.join("a.dem");
        std::fs::write(&demo, header("dod_anzio", 0)).unwrap();

        assert_eq!(map_name(&demo).unwrap(), "dod_anzio");
        assert_eq!(map_reference(&demo).unwrap().expected_checksum, None);
    }

    #[test]
    fn the_header_size_is_the_one_the_demo_patcher_and_the_byte_finder_use() {
        assert_eq!(HEADER_READ_LEN, dod::server_info::DEMO_HEADER_LEN);
    }

    #[test]
    fn a_header_checksum_wins_over_the_server_info() {
        let dir = scratch("header_wins");
        let demo = dir.join("a.dem");
        let mut bytes = hltv_demo("dod_anzio", 0x801b_771f, 5);
        bytes[MAP_CHECKSUM_AT..MAP_CHECKSUM_AT + 4].copy_from_slice(&7u32.to_le_bytes());
        std::fs::write(&demo, bytes).unwrap();

        assert_eq!(map_reference(&demo).unwrap().expected_checksum, Some(7));
    }

    #[test]
    fn an_hltv_demo_on_the_installed_build_is_ok_and_on_another_is_a_wrong_build() {
        let dir = scratch("hltv_build");
        let maps = dir.join("maps");
        let installed = empty_map(&maps, "dod_chemille");

        let same = dir.join("same.dem");
        std::fs::write(&same, hltv_demo("dod_chemille", installed, 0)).unwrap();
        let older = dir.join("older.dem");
        std::fs::write(&older, hltv_demo("dod_chemille", 0xbb69_0c32, 12)).unwrap();

        assert_eq!(
            check_demo(&same, &maps).unwrap().1,
            MapStatus::Ok {
                checksum: installed
            }
        );
        assert_eq!(
            check_demo(&older, &maps).unwrap().1,
            MapStatus::WrongBuild {
                expected: 0xbb69_0c32,
                found: installed
            }
        );
    }

    #[test]
    fn a_pov_fixtures_server_info_agrees_with_its_header() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../test-fixtures/ci_fixture.dem");
        let mut head = vec![0u8; HEADER_READ_LEN + SERVER_INFO_WINDOW];
        let read =
            std::io::Read::read(&mut std::fs::File::open(&fixture).unwrap(), &mut head).unwrap();
        head.truncate(read);

        let header_checksum = u32::from_le_bytes(
            head[MAP_CHECKSUM_AT..MAP_CHECKSUM_AT + 4]
                .try_into()
                .unwrap(),
        );
        assert_ne!(header_checksum, 0);
        assert_eq!(server_info_checksum(&head), Some(header_checksum));
    }

    /// The two real HLTV demos of #1539 against the installed maps. Local
    /// only: `DOD_HLTV_MAPBUILD_DEMOS` is a folder holding
    /// `chemille-gamewyze-fx-1.dem` (recorded on build `bb690c32`, the
    /// installed 1.3 map is another) and the anzio scrim demo
    /// `scrim_1790730448-DEN1_h1-2609292052-dod_anzio.dem` (same build);
    /// `DOD_MAPS_DIR` is the maps folder, else the PRE install's.
    #[test]
    #[ignore = "needs the real HLTV demos: set DOD_HLTV_MAPBUILD_DEMOS"]
    fn real_hltv_demos_are_checked_against_the_installed_maps() {
        let demos = std::env::var_os("DOD_HLTV_MAPBUILD_DEMOS")
            .map(PathBuf::from)
            .expect("set DOD_HLTV_MAPBUILD_DEMOS to the folder holding the two demos");
        let maps = std::env::var_os("DOD_MAPS_DIR").map(PathBuf::from).unwrap_or_else(|| {
            PathBuf::from(
                "C:/Program Files (x86)/Steam/steamapps/common/Half-Life - PRE-Anniversary for Movies/dod/maps",
            )
        });

        let (reference, status) =
            check_demo(&demos.join("chemille-gamewyze-fx-1.dem"), &maps).unwrap();
        assert_eq!(reference.expected_checksum, Some(0xbb69_0c32));
        assert!(matches!(status, MapStatus::WrongBuild { .. }), "{status:?}");

        let (reference, status) = check_demo(
            &demos.join("scrim_1790730448-DEN1_h1-2609292052-dod_anzio.dem"),
            &maps,
        )
        .unwrap();
        assert_eq!(reference.expected_checksum, Some(0x801b_771f));
        assert!(matches!(status, MapStatus::Ok { .. }), "{status:?}");
    }

    #[test]
    fn a_missing_map_is_missing_and_a_present_one_without_a_stated_build_is_unverifiable() {
        let dir = scratch("missing");
        let maps = dir.join("maps");
        std::fs::create_dir_all(&maps).unwrap();

        let wants = MapReference {
            map_name: "dod_anzio".to_string(),
            expected_checksum: Some(1),
        };
        assert_eq!(status_of(&wants, &maps), MapStatus::Missing);
        assert!(!status_of(&wants, &maps).is_playable());

        std::fs::write(map_path(&maps, "dod_anzio"), b"not a bsp").unwrap();
        let unstated = MapReference {
            map_name: "dod_anzio".to_string(),
            expected_checksum: None,
        };
        assert_eq!(status_of(&unstated, &maps), MapStatus::Unverifiable);
    }

    #[test]
    fn a_map_that_is_not_a_bsp_is_unreadable_not_a_wrong_build() {
        let dir = scratch("garbage");
        let maps = dir.join("maps");
        std::fs::create_dir_all(&maps).unwrap();
        std::fs::write(map_path(&maps, "dod_anzio"), b"short").unwrap();

        let wants = MapReference {
            map_name: "dod_anzio".to_string(),
            expected_checksum: Some(1),
        };
        assert!(matches!(
            status_of(&wants, &maps),
            MapStatus::Unreadable { .. }
        ));
    }

    #[test]
    fn a_map_name_that_could_escape_the_maps_folder_is_refused() {
        let dir = scratch("escape");
        let demo = dir.join("a.dem");
        std::fs::write(&demo, header("../../windows/system32/x", 1)).unwrap();

        assert!(map_reference(&demo).is_err());
    }
}
