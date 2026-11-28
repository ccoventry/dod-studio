//! Studio's way out with map builds in place (#1547). Its own test binary,
//! so the process-wide "Studio is closing" flag and the list of this
//! Studio's placements reach no other test.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration test: its helpers sit outside a #[test], where clippy.toml's test allowance does not reach (#1179)"
)]

use native::maps::build_library::{LIBRARY_FOLDER, add_build};
use native::maps::build_placement::{
    GameState, MapBuild, place_builds, release_at_exit, release_at_exit_in, slot_path,
};
use std::path::PathBuf;

/// The smallest map the checksum reads, its first lump holding `seed`.
fn map_bytes(seed: u8) -> Vec<u8> {
    let header = 4 + 15 * 8;
    let mut bytes = vec![0u8; header + 4];
    bytes[0] = 30;
    bytes[12..16].copy_from_slice(&(header as u32).to_le_bytes());
    bytes[16..20].copy_from_slice(&4u32.to_le_bytes());
    bytes[header..].copy_from_slice(&[seed; 4]);
    bytes
}

fn addon_with_a_build() -> (PathBuf, MapBuild) {
    let dir =
        std::env::temp_dir().join(format!("dodstudio_map_builds_exit_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("old.bsp");
    std::fs::write(&source, map_bytes(2)).unwrap();
    let addon = dir.join("dod_addon");
    let added = add_build(
        &addon.join(LIBRARY_FOLDER),
        "dod_x",
        &source,
        &mut |_, _| {},
    )
    .unwrap();
    (
        addon,
        MapBuild {
            map_name: "dod_x".into(),
            checksum: added.checksum,
        },
    )
}

#[test]
fn closing_studio_removes_its_builds_when_no_game_runs_and_leaves_them_with_one() {
    // The activity log goes to a scratch folder, not the user's.
    unsafe {
        std::env::set_var(
            "DOD_STUDIO_LOG_DIR",
            std::env::temp_dir().join("dod_studio_test_logs"),
        )
    };
    let (addon, want) = addon_with_a_build();
    let slot = slot_path(&addon, "dod_x");

    let (_kept, _) = place_builds(&addon, std::slice::from_ref(&want));
    assert_eq!(release_at_exit_in(GameState::Running), 0);
    assert!(slot.exists(), "a running game may have loaded it");

    std::fs::remove_file(&slot).unwrap();
    let (_placed, _) = place_builds(&addon, std::slice::from_ref(&want));
    assert_eq!(release_at_exit_in(GameState::NoneRunning), 1);
    assert!(!slot.exists());

    // The real entry point, once nothing is placed: nothing to do.
    assert_eq!(release_at_exit(), 0);
    let _ = std::fs::remove_dir_all(addon.parent().unwrap());
}
