use super::*;
use crate::test_support::Scratch;

#[test]
fn test_take_key_uses_last_two_components_lowercased() {
    let key = take_key(Path::new(
        r"D:\Captures\Session_20260818_142233\Dodstudio_Chain_01_b0",
    ));
    assert_eq!(
        key,
        Some("session_20260818_142233/dodstudio_chain_01_b0".to_string())
    );
}

#[test]
fn test_take_key_is_stable_across_drives() {
    // The same take copied to a different drive must produce the same key —
    // this is the whole reason the absolute path isn't used.
    let a = take_key(Path::new(r"D:\Captures\session_1\dodstudio_chain_01_b0"));
    let b = take_key(Path::new(
        r"X:\somewhere\else\session_1\dodstudio_chain_01_b0",
    ));
    assert_eq!(a, b);
    assert!(a.is_some());
}

#[test]
fn test_take_key_distinguishes_sessions() {
    let a = take_key(Path::new(r"D:\c\session_1\dodstudio_chain_01_b0"));
    let b = take_key(Path::new(r"D:\c\session_2\dodstudio_chain_01_b0"));
    assert_ne!(a, b);
}

#[test]
fn test_take_key_none_without_a_parent_component() {
    assert_eq!(take_key(Path::new("")), None);
}

#[test]
fn a_dod_copy_without_its_language_file_says_which_to_rename() {
    let root = Scratch::new("language_file");
    let resource = root.path().join("dod_movies").join("resource");
    std::fs::create_dir_all(&resource).unwrap();
    std::fs::write(resource.join("dod_english.txt"), b"x").unwrap();
    let missing = missing_language_file(root.path(), "dod_movies", "english").unwrap();
    assert_eq!(missing.needed, "dod_movies_english.txt");
    assert_eq!(missing.rename_from.as_deref(), Some("dod_english.txt"));
    assert_eq!(
        std::path::PathBuf::from(&missing.show),
        resource.join("dod_english.txt")
    );
    // A French Steam: its own dod_french.txt first, else the English one.
    let french = missing_language_file(root.path(), "dod_movies", "french").unwrap();
    assert_eq!(french.needed, "dod_movies_french.txt");
    assert_eq!(french.rename_from.as_deref(), Some("dod_english.txt"));
    std::fs::write(resource.join("dod_french.txt"), b"x").unwrap();
    let french = missing_language_file(root.path(), "dod_movies", "french").unwrap();
    assert_eq!(french.rename_from.as_deref(), Some("dod_french.txt"));
}

#[test]
fn dod_itself_or_a_copy_with_its_file_needs_nothing() {
    let root = Scratch::new("language_file_ok");
    let resource = root.path().join("dod_movies").join("resource");
    std::fs::create_dir_all(&resource).unwrap();
    assert!(missing_language_file(root.path(), "dod", "english").is_none());
    assert!(missing_language_file(root.path(), "DOD", "english").is_none());
    std::fs::write(resource.join("dod_movies_english.txt"), b"x").unwrap();
    assert!(missing_language_file(root.path(), "dod_movies", "english").is_none());
    // Nothing to rename: the file has to come from dod.
    let other = missing_language_file(root.path(), "dod_other", "english").unwrap();
    assert_eq!(other.rename_from, None);
    assert_eq!(
        std::path::PathBuf::from(&other.show),
        root.path().join("dod_other")
    );
}

#[test]
fn test_take_key_matches_across_the_take_number_nesting() {
    // The capture side (native/src/patch/builder.rs) computes take_key
    // from the block folder it asked HLAE to write to. The render side
    // (render_manager.rs) computes it from ClipData.take_folder, which
    // scan_folder_background sets to wherever it actually found the
    // wav/bmp — one level deeper, inside HLAE's own take0000 auto-numbered
    // subfolder. Both must resolve to the same key or auto-Rendered can
    // never correlate a finished render back to its highlights.
    let capture_side = take_key(Path::new(r"D:\Captures\session_1\dodstudio_chain_01_b0"));
    let render_side = take_key(Path::new(
        r"D:\Captures\session_1\dodstudio_chain_01_b0\take0000",
    ));
    assert_eq!(capture_side, render_side);
    assert_eq!(
        capture_side,
        Some("session_1/dodstudio_chain_01_b0".to_string())
    );
}

#[test]
fn test_take_key_handles_higher_numbered_takes() {
    let key = take_key(Path::new(
        r"D:\Captures\session_1\dodstudio_chain_01_b0\take0003",
    ));
    assert_eq!(key, Some("session_1/dodstudio_chain_01_b0".to_string()));
}

#[test]
fn console_log_is_cleared_beside_hl_exe() {
    let root = Scratch::new("qconsole_root");
    let log = root.join("qconsole.log");
    std::fs::write(&log, b"console spam").unwrap();

    remove_console_log(&root);

    assert!(
        !log.exists(),
        "qconsole.log beside hl.exe should be removed"
    );
}

/// Cleanup runs whether or not the engine was launched with `-condebug`,
/// so an absent log is ordinary rather than an error worth surfacing.
#[test]
fn a_missing_console_log_is_not_an_error() {
    let root = Scratch::new("qconsole_none");
    remove_console_log(&root);
}

#[test]
fn is_chain_demo_filename_matches_real_output_names() {
    assert!(is_chain_demo_filename("dodstudio_chain_01.dem"));
    assert!(is_chain_demo_filename("dodstudio_chain_9999.dem"));
}

/// No digit-count cap: a batch large enough to need `dodstudio_chain_100500.dem`
/// must still be cleaned up correctly, not silently left behind.
#[test]
fn is_chain_demo_filename_has_no_upper_bound_on_digit_count() {
    assert!(is_chain_demo_filename("dodstudio_chain_100500.dem"));
}

/// A source demo that happens to share the "chain_" prefix must never be
/// mistaken for a patched output file and deleted.
#[test]
fn is_chain_demo_filename_rejects_lookalike_source_demos() {
    assert!(!is_chain_demo_filename("chain_harrington_round1.dem"));
    assert!(!is_chain_demo_filename("chain_.dem"));
    assert!(!is_chain_demo_filename("dodstudio_chain_01.dem.bak"));
    assert!(!is_chain_demo_filename("prefix_chain_01.dem"));
    assert!(!is_chain_demo_filename("dodstudio_chain_01.cfg"));
}

/// Nothing but that one file may be touched — the game folder holds the
/// user's own configs, and anything wider here would be unrecoverable.
#[test]
fn nothing_but_the_console_log_is_touched() {
    let root = Scratch::new("qconsole_keep");
    let dod = root.join("dod");
    std::fs::create_dir_all(&dod).unwrap();
    let keep = [
        dod.join("config.cfg"),
        dod.join("movie.cfg"),
        root.join("debug.log"),
        root.join("condump_001.txt"),
    ];
    for f in &keep {
        std::fs::write(f, b"mine").unwrap();
    }
    std::fs::write(root.join("qconsole.log"), b"spam").unwrap();

    remove_console_log(&root);

    for f in &keep {
        assert!(f.exists(), "{:?} is the user's file and must survive", f);
    }
}

#[test]
fn plain_folder_names_are_game_folders() {
    for name in ["dod", "dod_movies", "dod_3_1_beta", "dod_1.2", "valve"] {
        assert_eq!(validate_game_folder(name), Ok(()), "{name}");
    }
}

/// Each of these would build a path outside the install, or a launch
/// line the engine reads differently from the folder Studio writes to.
#[test]
fn paths_and_launch_options_are_not_game_folders() {
    for name in [
        "",
        "   ",
        " dod",
        "dod ",
        "dod movies",
        "..",
        ".",
        "../dod",
        "dod/maps",
        r"dod\maps",
        r"C:\Games\dod",
        "C:dod",
        "dod?",
        "-insecure",
        "+map",
        "dod\u{0}",
    ] {
        assert!(
            validate_game_folder(name).is_err(),
            "{name:?} should be refused"
        );
    }
}

#[test]
fn a_refused_game_folder_leaves_the_setting_alone() {
    // Tests never set a valid folder (the setting is process-wide and
    // tests run in parallel), so the default is what every one sees.
    assert!(set_game_folder("../elsewhere").is_err());
    assert_eq!(game_folder(), DEFAULT_GAME_FOLDER);
    let root = Path::new(r"C:\Games\Half-Life");
    assert_eq!(mod_dir(root), root.join("dod"));
    assert_eq!(addon_dir(root), root.join("dod_addon"));
    assert_eq!(downloads_dir(root), root.join("dod_downloads"));
    assert_eq!(
        mod_dir_for_exe(&root.join("hl.exe")),
        Some(root.join("dod"))
    );
}

#[test]
fn only_folders_with_a_liblist_and_a_client_dll_are_listed() {
    let root = Scratch::new("game_folders");
    for (folder, liblist, client) in [
        ("dod", true, true),
        ("dod_movies", true, true),
        ("Dod_1_2", true, true),
        ("dod_addon", false, false),
        ("dod_downloads", false, false),
        // The movie installs' valve: a liblist.gam, no client.dll (#768).
        ("valve", true, false),
        ("half_a_mod", false, true),
    ] {
        let dir = root.join(folder);
        std::fs::create_dir_all(dir.join("cl_dlls")).unwrap();
        if liblist {
            std::fs::write(dir.join("liblist.gam"), b"game \"Day of Defeat\"").unwrap();
        }
        if client {
            std::fs::write(dir.join("cl_dlls").join("client.dll"), b"MZ").unwrap();
        }
    }
    // A file named like a folder is not one.
    std::fs::write(root.join("liblist.gam"), b"").unwrap();
    assert_eq!(list_game_folders(&root), ["dod", "Dod_1_2", "dod_movies"]);
    assert!(list_game_folders(&root.join("missing")).is_empty());
}
