//! Demos recorded on an old server build (`BUILD 2693`, 2004-era), whose
//! `svc_serverinfo` carries a 21-byte block after the VAC byte that today's
//! client does not read (`dem-patch`'s `server_info.rs`, #1456). Ignored
//! because no such demo is in the repo: community demos are not ours to
//! commit. Point `DOD_OLD_BUILD_DEMOS` at a folder of them:
//!
//!     DOD_OLD_BUILD_DEMOS=<folder> cargo test -p analysis -- --ignored old_server_build

use crate::*;
use std::fs;

/// Every `.dem` in the folder `DOD_OLD_BUILD_DEMOS` names. Panics when the
/// variable is unset or the folder holds no demo, so the test can't pass on
/// nothing.
fn old_build_demos() -> Vec<std::path::PathBuf> {
    let folder = std::env::var_os("DOD_OLD_BUILD_DEMOS")
        .map(std::path::PathBuf::from)
        .expect("set DOD_OLD_BUILD_DEMOS to a folder of old-server-build demos");
    let mut demos: Vec<_> = fs::read_dir(&folder)
        .unwrap_or_else(|e| panic!("{}: {e} (set DOD_OLD_BUILD_DEMOS)", folder.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dem")))
        .collect();
    demos.sort();
    assert!(!demos.is_empty(), "no .dem in {}", folder.display());
    demos
}

/// Each demo parses, analyses to a map, players and kills, and its frames
/// write back unchanged (the old-build block is kept, not dropped).
#[test]
#[ignore]
fn old_server_build_demos_parse_analyse_and_round_trip() {
    for path in old_build_demos() {
        let bytes = fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));

        let analysis =
            Analysis::try_from_bytes(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let kills: usize = analysis
            .state
            .players
            .iter()
            .map(|p| p.stats.1 as usize)
            .sum();
        println!(
            "{}: {} {}, {} players, {} kills, {} rounds",
            path.display(),
            analysis.demo_info.demo_type,
            analysis.demo_info.map_name,
            analysis.state.players.len(),
            kills,
            analysis.state.rounds.len(),
        );
        assert!(
            !analysis.demo_info.map_name.is_empty(),
            "{}",
            path.display()
        );
        assert!(!analysis.state.players.is_empty(), "{}", path.display());
        assert!(kills > 0, "{}: no kills", path.display());

        let demo =
            dem::open_demo_from_bytes(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        // The writer rebuilds the directory at the end of the file (its
        // frame counts can differ from the recorder's, ci_fixture.dem's too), so
        // the frames before it are what must come back unchanged.
        let directory_offset = u32::from_le_bytes(bytes[540..544].try_into().unwrap()) as usize;
        let written = demo.write_to_bytes().unwrap();
        let first_difference = written.iter().zip(&bytes).position(|(a, b)| a != b);
        assert!(
            written.len() == bytes.len()
                && first_difference.is_none_or(|at| at >= directory_offset),
            "{}: wrote {} bytes of {}, first difference at byte {first_difference:?}, directory at {directory_offset}",
            path.display(),
            written.len(),
            bytes.len(),
        );
    }
}
