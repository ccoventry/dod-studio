use super::*;

/// A settings.json saved by a pre-2026-08-18 build still has an
/// `allocation_strategy` key on disk (the field was removed from
/// `AppSettings` when the Chronological drive-allocation strategy was
/// deleted). Since `AppSettings` doesn't set `#[serde(deny_unknown_fields)]`,
/// serde must silently ignore the stale key rather than fail deserialization
/// — otherwise every existing user's settings would revert to defaults on
/// their next launch after upgrading.
#[test]
fn test_deserialize_ignores_legacy_allocation_strategy_field() {
    let legacy_json = r#"{
        "hlae_path": "C:/hlae/hlae.exe",
        "hl_path": "C:/dod/hl.exe",
        "ffmpeg_path": null,
        "pinned_folders": [],
        "language": "en",
        "capture_fps": 300,
        "pre_roll_seconds": 2.0,
        "post_roll_seconds": 0.6,
        "allocation_strategy": "Chronological"
    }"#;

    let settings: AppSettings = serde_json::from_str(legacy_json)
        .expect("legacy allocation_strategy field must not break deserialization");

    assert_eq!(settings.hlae_path, "C:/hlae/hlae.exe");
    assert_eq!(settings.capture_fps, 300);
    // Settings saved before #402 drive the `dod` folder, as they always did.
    assert_eq!(settings.game_folder, "dod");
    // Settings saved before #440 do not start finishing clips on their own.
    assert!(!settings.finish_clips_after_batch);
    assert_eq!(settings.finish_codec_obs, "source_copy");
    assert_eq!(settings.finish_codec_video, "render_tab");
    assert_eq!(settings.finish_codec_frames, "render_tab");
    // Settings saved before #644 keep every take.
    assert_eq!(settings.take_after_finish, "keep");
    assert!(!settings.take_delete_only_when_space_needed);
    // Settings saved before #861 don't render while capturing.
    assert!(!settings.render_while_capturing);
    assert_eq!(settings.renders_while_capturing, 1);
    // Settings saved before #1430 keep 50 GB free on the Windows drive.
    assert_eq!(settings.windows_drive_keep_free_gb, 50);
    // Settings saved before #831 keep the gaps between kills.
    assert_eq!(
        settings.kill_gaps.mode,
        native::patch::types::KillGapMode::Keep
    );
    assert_eq!(settings.kill_gaps.over_seconds, 8.0);
    // No render_codec saved: today's default, ProRes 422.
    assert_eq!(settings.render_codec, "prores_422");
    // Settings saved before #246 get today's worker count.
    assert_eq!(
        settings.scan_workers,
        crate::scan_manager::SCAN_CONCURRENCY as i32
    );
    // Settings saved before #599 still open Setup when a path is missing.
    assert!(!settings.setup_dismissed);
    // Settings saved before #533 get the pinned-folder look once.
    assert!(!settings.pinned_folders_reviewed);
    // Settings saved before #531 keep walking into subfolders.
    assert!(settings.scan_include_subfolders);
}

#[test]
fn the_pinned_folder_look_reads_as_run_only_when_the_file_says_so() {
    assert!(pinned_review_done_in(
        r#"{"pinned_folders_reviewed": true, "hl_path": "x"}"#
    ));
    assert!(!pinned_review_done_in(
        r#"{"pinned_folders_reviewed": false}"#
    ));
    // A file from before #533, or one that doesn't parse, hasn't had it.
    assert!(!pinned_review_done_in(r#"{"hl_path": "x"}"#));
    assert!(!pinned_review_done_in("not json"));
}

#[test]
fn test_setup_dismissed_roundtrip() {
    let original = AppSettings {
        setup_dismissed: true,
        ..AppSettings::default()
    };
    let json = serde_json::to_string(&original).expect("settings must serialize");
    let restored: AppSettings = serde_json::from_str(&json).expect("settings must deserialize");
    assert!(restored.setup_dismissed);
}

/// The PC id survives a save, and a file from before it has none (#1085).
#[test]
fn pc_id_roundtrip_and_default() {
    let original = AppSettings {
        pc_id: "3f2a".into(),
        ..AppSettings::default()
    };
    let json = serde_json::to_string(&original).expect("settings must serialize");
    let restored: AppSettings = serde_json::from_str(&json).expect("settings must deserialize");
    assert_eq!(restored.pc_id, "3f2a");
    let mut before = serde_json::to_value(AppSettings::default()).expect("settings must serialize");
    before
        .as_object_mut()
        .expect("settings are an object")
        .remove("pc_id");
    let old: AppSettings =
        serde_json::from_value(before).expect("a file without pc_id must deserialize");
    assert!(old.pc_id.is_empty());
}

/// The New UI switch survives a save, and a file from before it has it
/// off (#1085).
#[test]
fn new_ui_roundtrip_and_default_off() {
    let original = AppSettings {
        new_ui: true,
        ..AppSettings::default()
    };
    let json = serde_json::to_string(&original).expect("settings must serialize");
    let restored: AppSettings = serde_json::from_str(&json).expect("settings must deserialize");
    assert!(restored.new_ui);
    let mut before = serde_json::to_value(AppSettings::default()).expect("settings must serialize");
    before
        .as_object_mut()
        .expect("settings are an object")
        .remove("new_ui");
    let old: AppSettings =
        serde_json::from_value(before).expect("a file without new_ui must deserialize");
    assert!(!old.new_ui);
}

/// Open Recent's list survives a save, and a file from before it has none (#935).
#[test]
fn recent_projects_roundtrip_and_default() {
    let original = AppSettings {
        recent_projects: vec!["D:/projects/a.json".into(), "D:/projects/b.json".into()],
        ..AppSettings::default()
    };
    let json = serde_json::to_string(&original).expect("settings must serialize");
    let restored: AppSettings = serde_json::from_str(&json).expect("settings must deserialize");
    assert_eq!(restored.recent_projects, original.recent_projects);

    let older: AppSettings = serde_json::from_str(
        r#"{"hlae_path": "", "hl_path": "", "ffmpeg_path": null, "pinned_folders": [],
            "language": "en", "pre_roll_seconds": 2.0, "post_roll_seconds": 0.6}"#,
    )
    .expect("a settings file");
    assert!(older.recent_projects.is_empty());
}

/// A `.dem` opened from Explorer goes to the Demo Analyzer unless the
/// user picked the queue, including for a file from before #528.
#[test]
fn dem_open_target_defaults_to_the_analyzer() {
    let older: AppSettings = serde_json::from_str(
        r#"{"hlae_path": "", "hl_path": "", "ffmpeg_path": null, "pinned_folders": [],
            "language": "en", "pre_roll_seconds": 2.0, "post_roll_seconds": 0.6}"#,
    )
    .expect("a settings file");
    assert_eq!(older.dem_open_target, "analyzer");
    assert!(older.kept_json_projects.is_empty());
    let queue = AppSettings {
        dem_open_target: "queue".into(),
        kept_json_projects: vec!["D:/projects/old.json".into()],
        ..AppSettings::default()
    };
    let json = serde_json::to_string(&queue).expect("settings must serialize");
    let restored: AppSettings = serde_json::from_str(&json).expect("settings must deserialize");
    assert_eq!(restored.dem_open_target, "queue");
    assert_eq!(restored.kept_json_projects, ["D:/projects/old.json"]);
}

/// A file saved while Seek Between Clips was experimental (off by default)
/// gets the new default once; a later choice sticks (#434).
#[test]
fn seek_between_clips_turns_on_once_for_an_older_settings_file() {
    let file = |extra: &str| -> AppSettings {
        serde_json::from_str(&format!(
            r#"{{"hlae_path": "C:/hlae/hlae.exe", "hl_path": "C:/dod/hl.exe", "ffmpeg_path": null,
                "pinned_folders": [], "language": "en", "capture_fps": 300,
                "pre_roll_seconds": 2.0, "post_roll_seconds": 0.6{extra}}}"#
        ))
        .expect("a settings file")
    };
    let mut saved_off = file(r#", "hook_runs_batch": false"#);
    saved_off.migrate();
    assert!(saved_off.hook_runs_batch);
    assert_eq!(saved_off.settings_migrations, MIGRATIONS);

    let mut turned_off_again = file(r#", "hook_runs_batch": false, "settings_migrations": 1"#);
    turned_off_again.migrate();
    assert!(!turned_off_again.hook_runs_batch);

    // A file from before the setting existed has the new default.
    let mut older = file("");
    older.migrate();
    assert!(older.hook_runs_batch);
}

/// Hidden installs (#931) round-trip; a file from before them hides none.
#[test]
fn hidden_installs_roundtrip() {
    let legacy: AppSettings = serde_json::from_str(
        r#"{"hlae_path": "", "hl_path": "", "ffmpeg_path": null, "pinned_folders": [],
            "language": "en", "capture_fps": 300, "pre_roll_seconds": 2.0,
            "post_roll_seconds": 0.6}"#,
    )
    .expect("a settings file");
    assert!(legacy.hidden_installs.is_empty());
    let original = AppSettings {
        hidden_installs: vec!["D:/Steam/steamapps/common/Half-Life".to_string()],
        ..AppSettings::default()
    };
    let json = serde_json::to_string(&original).expect("settings must serialize");
    let restored: AppSettings = serde_json::from_str(&json).expect("settings must deserialize");
    assert_eq!(restored.hidden_installs, original.hidden_installs);
}

#[test]
fn test_scan_workers_roundtrip() {
    let original = AppSettings {
        scan_workers: 5,
        ..AppSettings::default()
    };
    let json = serde_json::to_string(&original).expect("settings must serialize");
    let restored: AppSettings = serde_json::from_str(&json).expect("settings must deserialize");
    assert_eq!(restored.scan_workers, 5);
}

/// Settings saved before #442 load with no profiles; saved ones round-trip.
#[test]
fn test_command_profiles_roundtrip() {
    let legacy_json = r#"{
        "hlae_path": "C:/hlae/hlae.exe",
        "hl_path": "C:/dod/hl.exe",
        "ffmpeg_path": null,
        "pinned_folders": [],
        "language": "en",
        "capture_fps": 300,
        "pre_roll_seconds": 2.0,
        "post_roll_seconds": 0.6
    }"#;
    let legacy: AppSettings =
        serde_json::from_str(legacy_json).expect("settings without profiles must deserialize");
    assert!(legacy.command_profiles.is_empty());
    assert_eq!(legacy.command_profile_active, "");

    let original = AppSettings {
        command_profiles: vec![CommandProfile {
            name: "FOTW".to_string(),
            init_commands: vec!["mirv_fov 90".to_string()],
            custom_commands: vec![CustomCommandPayload {
                command: "host_timescale 0.5".to_string(),
                relation: "Before".to_string(),
                offset_seconds: 1.5,
            }],
        }],
        command_profile_active: "FOTW".to_string(),
        ..AppSettings::default()
    };
    let json = serde_json::to_string(&original).expect("settings must serialize");
    let restored: AppSettings = serde_json::from_str(&json).expect("settings must deserialize");
    assert_eq!(restored.command_profile_active, "FOTW");
    assert_eq!(restored.command_profiles.len(), 1);
    assert_eq!(restored.command_profiles[0].init_commands, ["mirv_fov 90"]);
    assert_eq!(
        restored.command_profiles[0].custom_commands[0].offset_seconds,
        1.5
    );
}

/// #555: Initial Commands, Scheduled Commands and every profile move from
/// the old `dodstudio_` hook names to the names that replaced them;
/// anything else is kept as is.
#[test]
fn test_old_console_names_migrate_on_load() {
    let scheduled = |command: &str| CustomCommandPayload {
        command: command.to_string(),
        relation: "Before".to_string(),
        offset_seconds: 1.0,
    };
    let mut settings = AppSettings {
        init_commands: vec![
            "dodstudio_hide_scoreboard 1".to_string(),
            "mirv_fov 90".to_string(),
            "dodstudio_deathmsg block !76561197962734863".to_string(),
        ],
        custom_commands: vec![
            scheduled("dodstudio_spec_lock 1"),
            scheduled("host_timescale 0.5"),
        ],
        command_profiles: vec![CommandProfile {
            name: "Old".to_string(),
            init_commands: vec!["dodstudio_clear_decals".to_string()],
            custom_commands: vec![scheduled("dodstudio_seek_by -10;dodstudio_spec_target 3")],
        }],
        ..AppSettings::default()
    };

    assert!(settings.migrate_console_names());
    assert_eq!(
        settings.init_commands,
        [
            "ds_hud_hide_scoreboard 1",
            "mirv_fov 90",
            "ds_hud_deathmsg block !76561197962734863"
        ]
    );
    assert_eq!(settings.custom_commands[0].command, "ds_spec_lock 1");
    assert_eq!(settings.custom_commands[1].command, "host_timescale 0.5");
    assert_eq!(
        settings.command_profiles[0].init_commands,
        ["ds_map_clear_decals"]
    );
    assert_eq!(
        settings.command_profiles[0].custom_commands[0].command,
        "ds_demo_seek_by -10;ds_spec_target 3"
    );

    // Already moved: nothing to do, so nothing is saved again.
    assert!(!settings.migrate_console_names());
    assert!(!AppSettings::default().migrate_console_names());
}

#[test]
fn test_default_settings_serialize_and_deserialize_roundtrip() {
    let original = AppSettings::default();
    let json = serde_json::to_string(&original).expect("default settings must serialize");
    let restored: AppSettings =
        serde_json::from_str(&json).expect("serialized default settings must deserialize");

    assert_eq!(restored.language, original.language);
    assert_eq!(restored.capture_fps, original.capture_fps);
    assert_eq!(restored.fast_forward_speed, original.fast_forward_speed);
    assert_eq!(restored.render_codec, original.render_codec);
    assert_eq!(
        restored.render_custom_codec_args,
        original.render_custom_codec_args
    );
}

/// The 0.2.0 fresh-install defaults, which a file missing the field gets
/// too; a value someone saved is kept.
#[test]
fn fresh_install_defaults_and_missing_fields_match() {
    let fresh = AppSettings::default();
    let minimal: AppSettings = serde_json::from_str(
        r#"{"hlae_path": "", "hl_path": "", "ffmpeg_path": null, "pinned_folders": [],
            "language": "en", "pre_roll_seconds": 2.0, "post_roll_seconds": 0.6}"#,
    )
    .expect("a settings file missing the changed fields");
    let screen =
        native::sys::screen::default_capture_resolution(native::sys::screen::primary_screen_size());
    for s in [&fresh, &minimal] {
        assert_eq!((s.resolution_width, s.resolution_height), screen);
        assert_eq!(s.capture_fps, 120);
        assert_eq!(s.capture_mode, "frame_sequence");
        assert_eq!(s.record_start_lead, 5.0);
        assert_eq!(s.record_stop_trail, 5.0);
        assert!(s.auto_clear_temp_demos);
        assert!(!s.notify_patching);
        assert!(!s.notify_demo_loading);
        assert!(!s.notify_between_clips);
        assert!(s.notify_captures_done && s.notify_renders_done && s.notify_error);
        assert!(s.notify_updates);
        assert_eq!(s.review_covers, "all");
        assert_eq!(s.review_order, "time");
        assert!(s.review_tick_yes);
        assert!(s.review_fast_forward);
        assert_eq!(s.review_fast_forward_gap, 8);
        assert!(s.pick_folders.is_empty());
    }

    let saved: AppSettings = serde_json::from_str(
        r#"{"hlae_path": "", "hl_path": "", "ffmpeg_path": null, "pinned_folders": [],
            "language": "en", "capture_fps": 300, "capture_mode": "frame_sequence",
            "resolution_width": 2560, "resolution_height": 1440,
            "pre_roll_seconds": 2.0, "post_roll_seconds": 0.6,
            "record_start_lead": 0.0, "record_stop_trail": 0.0,
            "auto_clear_temp_demos": false, "notify_patching": true,
            "notify_demo_loading": true, "notify_between_clips": true,
            "review_tick_yes": false, "review_fast_forward_gap": 20}"#,
    )
    .expect("a settings file with the old values saved");
    assert_eq!(saved.capture_fps, 300);
    assert_eq!(
        (saved.resolution_width, saved.resolution_height),
        (2560, 1440)
    );
    assert_eq!(saved.capture_mode, "frame_sequence");
    assert_eq!(saved.record_start_lead, 0.0);
    assert_eq!(saved.record_stop_trail, 0.0);
    assert!(!saved.auto_clear_temp_demos);
    assert!(saved.notify_patching && saved.notify_demo_loading && saved.notify_between_clips);
    assert!(!saved.review_tick_yes);
    assert_eq!(saved.review_fast_forward_gap, 20);
    // Saved before #937: no read rate measured yet.
    assert_eq!(saved.scan_read_bytes_per_sec, 0.0);
}

#[test]
fn the_scan_read_rate_roundtrips() {
    let original = AppSettings {
        scan_read_bytes_per_sec: 140e6,
        ..AppSettings::default()
    };
    let json = serde_json::to_string(&original).unwrap();
    let restored: AppSettings = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.scan_read_bytes_per_sec, 140e6);
}

/// A fresh install renders ProRes 422; a saved codec, HQ's `"prores"`
/// included, is kept as it is.
#[test]
fn a_fresh_install_renders_prores_422_and_a_saved_codec_stays() {
    assert_eq!(AppSettings::default().render_codec, "prores_422");
    for saved in ["prores", "prores_lt", "dnxhr"] {
        let original = AppSettings {
            render_codec: saved.to_string(),
            ..AppSettings::default()
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.render_codec, saved);
    }
}

#[test]
fn the_rescan_offer_stamp_roundtrips_and_an_older_file_has_none() {
    let original = AppSettings {
        rescan_offered_for: "0.2.1+h1".into(),
        ..AppSettings::default()
    };
    let json = serde_json::to_string(&original).unwrap();
    let restored: AppSettings = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.rescan_offered_for, "0.2.1+h1");
    let older: AppSettings = serde_json::from_str(
        r#"{"hlae_path": "", "hl_path": "", "ffmpeg_path": null, "pinned_folders": [],
            "language": "en", "pre_roll_seconds": 2.0, "post_roll_seconds": 0.6}"#,
    )
    .expect("a settings file");
    assert_eq!(older.rescan_offered_for, "");
}

/// A settings file from before migration 3, with this Players table.
fn file_with_players(players: &str) -> AppSettings {
    serde_json::from_str(&format!(
        r#"{{"hlae_path": "", "hl_path": "", "ffmpeg_path": null, "pinned_folders": [],
            "language": "en", "pre_roll_seconds": 2.0, "post_roll_seconds": 0.6,
            "settings_migrations": 2, "players": {players}}}"#
    ))
    .expect("a settings file")
}

#[test]
fn migration_3_turns_a_used_tables_own_fallback_into_no_spray_with_a_notice() {
    let mut settings = file_with_players(
        r#"{"pov_fallback": "own", "players": [{"name": "m00cat", "png": "a.png"}]}"#,
    );
    settings.migrate();
    assert_eq!(
        settings.players.pov_fallback,
        native::player_overrides::SprayFallback::None
    );
    assert!(settings.pov_own_spray_notice);
    assert_eq!(settings.settings_migrations, MIGRATIONS);
}

#[test]
fn migration_3_says_nothing_to_someone_who_never_used_the_table() {
    let mut settings = file_with_players(r#"{"pov_fallback": "own"}"#);
    settings.migrate();
    assert_eq!(
        settings.players.pov_fallback,
        native::player_overrides::SprayFallback::Png
    );
    assert!(!settings.pov_own_spray_notice);
}

#[test]
fn a_fresh_install_saves_no_profiles() {
    // Clean clips is built in (#911): the Commands tab lists it from
    // `builtin_profiles`, never from the settings file.
    let fresh = AppSettings::default();
    assert!(fresh.command_profiles.is_empty());
    let mut again = fresh.clone();
    again.migrate();
    assert!(again.command_profiles.is_empty());
    assert!(again.builtin_profile_notice.is_empty());
}

/// #911: settings migration 2 copied Clean clips into the file as an
/// editable profile. An unchanged copy goes (the built-in one replaces
/// it); a changed one stays as "Clean clips (mine)", with the notice.
#[test]
fn a_clean_clips_copy_from_migration_2_is_dropped_or_kept_as_mine() {
    let file = |extra: &str| -> AppSettings {
        serde_json::from_str(&format!(
            r#"{{"hlae_path": "", "hl_path": "", "ffmpeg_path": null, "pinned_folders": [],
                "language": "en", "pre_roll_seconds": 2.0, "post_roll_seconds": 0.6{extra}}}"#
        ))
        .expect("a settings file")
    };
    // Clean clips as #898 first added it (dbfcaa64), before 0.2.0 changed it.
    let shipped = r#"["r_decals 256", "mirv_fov 90", "ds_spec_match_pov 1",
        "ds_spec_hide_hltv_messages 1", "ds_spec_hide_bars 1", "ds_spec_deathcam 0",
        "ds_hud_hide_scoreboard 1", "ds_hud_hide_voice_menus 1", "ds_hud_hide_crosshair 0",
        "ds_hud_hide_map_text 0", "ds_hud_hide_clan_text 1", "ds_hud_show_scope 1",
        "ds_hud_overview_players 0", "ds_hud_overview_team_marker 0", "ds_view_use_cl_bob 0",
        "ds_view_hide_headshot_red_screen 0", "ds_view_hide_screen_shake 0",
        "ds_player_hide_hand_signals 0", "ds_sound_mute_voice_commands 0",
        "ds_map_allow_shaders 0"]"#;

    // Unchanged: dropped, silently, and the active name stays on it.
    let mut unchanged = file(&format!(
        r#", "settings_migrations": 2, "command_profile_active": "Clean clips",
           "command_profiles": [{{"name": "FOTW", "init_commands": ["mirv_fov 100"]}},
                                {{"name": "Clean clips", "init_commands": {shipped}}}]"#
    ));
    unchanged.migrate();
    let names: Vec<_> = unchanged
        .command_profiles
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names, ["FOTW"]);
    assert_eq!(unchanged.command_profile_active, "Clean clips");
    assert!(unchanged.builtin_profile_notice.is_empty());

    // Changed (one line added): kept under its new name, which the
    // active name follows, and the notice is due.
    let changed_lines = shipped.replacen('[', r#"["host_framerate 60", "#, 1);
    let mut changed = file(&format!(
        r#", "settings_migrations": 2, "command_profile_active": "Clean clips",
           "command_profiles": [{{"name": "Clean clips", "init_commands": {changed_lines}}}]"#
    ));
    changed.migrate();
    assert_eq!(changed.command_profiles.len(), 1);
    assert_eq!(changed.command_profiles[0].name, "Clean clips (mine)");
    assert_eq!(
        changed.command_profiles[0].init_commands[0],
        "host_framerate 60"
    );
    assert_eq!(changed.command_profile_active, "Clean clips (mine)");
    assert_eq!(changed.builtin_profile_notice, ["Clean clips (mine)"]);

    // The notice is not added twice, and survives a save until the
    // frontend empties it.
    changed.migrate();
    assert_eq!(changed.builtin_profile_notice, ["Clean clips (mine)"]);
    let json = serde_json::to_string(&changed).unwrap();
    let restored: AppSettings = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.builtin_profile_notice, ["Clean clips (mine)"]);

    // From before migration 2: nothing is added any more.
    let mut older = file(
        r#", "settings_migrations": 1,
           "command_profiles": [{"name": "FOTW", "init_commands": ["mirv_fov 100"]}]"#,
    );
    older.migrate();
    assert_eq!(older.command_profiles.len(), 1);
    assert_eq!(older.settings_migrations, MIGRATIONS);
}
