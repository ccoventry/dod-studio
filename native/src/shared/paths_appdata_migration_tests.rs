use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dod_migrate_{}_{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

#[test]
fn moves_settings_and_logs_then_removes_the_empty_legacy_dir() {
    let root = scratch("basic");
    let legacy = root.join(LEGACY_APPDATA_DIR_NAME);
    let current = root.join(APPDATA_DIR_NAME);
    write(&legacy.join("settings.json"), r#"{"hlae_path":"x"}"#);
    write(&legacy.join("logs").join("activity_20260917.md"), "# log");

    let report = migrate_appdata_dir_between(&legacy, &current);

    assert_eq!(
        std::fs::read_to_string(current.join("settings.json")).unwrap(),
        r#"{"hlae_path":"x"}"#
    );
    assert!(current.join("logs").join("activity_20260917.md").is_file());
    assert!(report.moved.contains(&"settings.json".to_string()));
    assert!(
        report.legacy_removed,
        "an emptied legacy dir should be removed"
    );
    assert!(!legacy.exists());
    let _ = std::fs::remove_dir_all(&root);
}

/// The case a whole-directory rename gets wrong: something that logs ran
/// first, so the destination already exists. `settings.json` must still
/// come across.
#[test]
fn migrates_even_when_the_new_dir_already_exists() {
    let root = scratch("exists");
    let legacy = root.join(LEGACY_APPDATA_DIR_NAME);
    let current = root.join(APPDATA_DIR_NAME);
    write(&legacy.join("settings.json"), "old-settings");
    write(
        &current.join("logs").join("activity_20260917.md"),
        "# new log",
    );

    let report = migrate_appdata_dir_between(&legacy, &current);

    assert_eq!(
        std::fs::read_to_string(current.join("settings.json")).unwrap(),
        "old-settings"
    );
    assert!(report.moved.contains(&"settings.json".to_string()));
    let _ = std::fs::remove_dir_all(&root);
}

/// The exact bug this recursion exists for: the companion DLL creates the
/// *new* `logs` folder on its own before this migration ever runs, so a
/// plain top-level skip would strand months of history in the old
/// location. Both legacy's and the DLL's own log files must survive.
#[test]
fn merges_a_colliding_logs_folder_instead_of_stranding_it() {
    let root = scratch("logs_merge");
    let legacy = root.join(LEGACY_APPDATA_DIR_NAME);
    let current = root.join(APPDATA_DIR_NAME);
    write(
        &legacy.join("logs").join("activity_20260822.md"),
        "# a month of history",
    );
    write(&legacy.join("logs").join("crash_log.md"), "# crash");
    write(
        &current.join("logs").join("dodstudio_goldsrc_hooks.log"),
        "the DLL's own log, written first",
    );

    let report = migrate_appdata_dir_between(&legacy, &current);

    assert_eq!(
        std::fs::read_to_string(current.join("logs").join("activity_20260822.md")).unwrap(),
        "# a month of history",
        "legacy history must not be stranded just because the new logs folder already existed"
    );
    assert_eq!(
        std::fs::read_to_string(current.join("logs").join("crash_log.md")).unwrap(),
        "# crash"
    );
    assert_eq!(
        std::fs::read_to_string(current.join("logs").join("dodstudio_goldsrc_hooks.log")).unwrap(),
        "the DLL's own log, written first",
        "the file already at the destination must not be overwritten"
    );
    assert!(
        report
            .moved
            .contains(&"logs/activity_20260822.md".to_string())
    );
    assert!(report.moved.contains(&"logs/crash_log.md".to_string()));
    assert!(
        report.legacy_removed,
        "the legacy dir should be fully drained and removed"
    );
    assert!(!legacy.exists());
    let _ = std::fs::remove_dir_all(&root);
}

/// A genuine file-vs-file collision inside a merged directory is still
/// left alone, not overwritten -- the recursion must not weaken that
/// guarantee just because it now looks one level deeper.
#[test]
fn a_colliding_file_inside_a_merged_directory_is_still_skipped_not_overwritten() {
    let root = scratch("logs_merge_collision");
    let legacy = root.join(LEGACY_APPDATA_DIR_NAME);
    let current = root.join(APPDATA_DIR_NAME);
    write(&legacy.join("logs").join("crash_log.md"), "stale");
    write(&current.join("logs").join("crash_log.md"), "live");

    let report = migrate_appdata_dir_between(&legacy, &current);

    assert_eq!(
        std::fs::read_to_string(current.join("logs").join("crash_log.md")).unwrap(),
        "live"
    );
    assert_eq!(report.skipped, vec!["logs/crash_log.md".to_string()]);
    assert!(
        !report.legacy_removed,
        "the legacy dir still holds the skipped file"
    );
    assert!(legacy.join("logs").join("crash_log.md").is_file());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn never_overwrites_what_the_new_dir_already_has() {
    let root = scratch("skip");
    let legacy = root.join(LEGACY_APPDATA_DIR_NAME);
    let current = root.join(APPDATA_DIR_NAME);
    write(&legacy.join("settings.json"), "stale");
    write(&current.join("settings.json"), "live");

    let report = migrate_appdata_dir_between(&legacy, &current);

    assert_eq!(
        std::fs::read_to_string(current.join("settings.json")).unwrap(),
        "live"
    );
    assert_eq!(report.skipped, vec!["settings.json".to_string()]);
    assert!(
        !report.legacy_removed,
        "the legacy dir still holds the skipped file"
    );
    assert!(legacy.join("settings.json").is_file());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn is_a_no_op_when_there_is_nothing_to_migrate() {
    let root = scratch("noop");
    let report = migrate_appdata_dir_between(
        &root.join(LEGACY_APPDATA_DIR_NAME),
        &root.join(APPDATA_DIR_NAME),
    );
    assert!(!report.did_something());
    assert!(
        !root.join(APPDATA_DIR_NAME).exists(),
        "must not create the new dir for nothing"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Calling it twice must be harmless -- the app calls it every launch.
#[test]
fn running_twice_changes_nothing_the_second_time() {
    let root = scratch("twice");
    let legacy = root.join(LEGACY_APPDATA_DIR_NAME);
    let current = root.join(APPDATA_DIR_NAME);
    write(&legacy.join("settings.json"), "s");

    let first = migrate_appdata_dir_between(&legacy, &current);
    let second = migrate_appdata_dir_between(&legacy, &current);

    assert!(first.did_something());
    assert!(!second.did_something());
    assert_eq!(
        std::fs::read_to_string(current.join("settings.json")).unwrap(),
        "s"
    );
    let _ = std::fs::remove_dir_all(&root);
}
