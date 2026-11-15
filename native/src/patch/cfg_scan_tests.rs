use super::*;
use crate::test_support::Scratch;

fn scratch(tag: &str) -> Scratch {
    Scratch::new(format_args!("cfg_scan_{tag}"))
}

/// `ds_player_change_name` (#1021) is an ordinary user command: renaming a
/// player for a batch is what Initial Commands are for, and one scheduled
/// mid-clip renames from that frame on, as asked. So it is in no tier.
#[test]
fn a_rename_is_allowed_in_initial_and_scheduled_commands() {
    let name = "ds_player_change_name";
    for (tier, list) in [
        ("BANNED_COMMANDS", BANNED_COMMANDS),
        ("SCHEDULED_BANNED_COMMANDS", SCHEDULED_BANNED_COMMANDS),
        ("MID_DEMO_HAZARDS", MID_DEMO_HAZARDS),
        ("NOOP_IN_INIT_COMMANDS", NOOP_IN_INIT_COMMANDS),
    ] {
        assert!(!list.contains(&name), "{name} is in {tier}");
    }
}

#[test]
fn a_warning_names_the_file_or_falls_back_to_the_whole_path() {
    assert_eq!(
        display_name(&Path::new("dod").join("movie.cfg")),
        "movie.cfg"
    );
    // `..` has no file name, so the whole path is shown instead.
    assert_eq!(display_name(Path::new("..")), "..");
}

#[test]
fn an_exec_chain_is_followed_and_the_last_value_wins() {
    // The real shape: config.cfg ends with `exec movie.cfg`, and movie.cfg
    // is where the interesting values are.
    let dir = scratch("chain");
    std::fs::write(dir.join("config.cfg"), "r_decals \"300\"\nexec movie.cfg\n").unwrap();
    std::fs::write(dir.join("movie.cfg"), "r_decals\t\"0\"\nmirv_fov \"105\"\n").unwrap();

    let scan = scan(&dir);
    assert_eq!(scan.effective("r_decals").unwrap().value, "0");
    assert_eq!(scan.effective("r_decals").unwrap().file_name(), "movie.cfg");
    assert_eq!(scan.effective("mirv_fov").unwrap().value, "105");
}

#[test]
fn a_bound_key_is_not_a_setting() {
    // `bind "F7" "r_decals 4000"` changes nothing until F7 is pressed.
    // Counting it would warn about a value the engine never applied.
    let dir = scratch("bind");
    std::fs::write(
            dir.join("config.cfg"),
            "bind \"F6\" \"r_decals 0; hud_deathnotice_time 5\"\nbind \"F7\" \"r_decals 4000\"\nalias clean \"r_decals 1\"\n",
        )
        .unwrap();

    let scan = scan(&dir);
    assert!(scan.effective("r_decals").is_none(), "{:?}", scan.settings);
}

/// #555: a config line still using a `dodstudio_` hook name is reported
/// with the name that replaced it, a bound key's included, and the file
/// is left exactly as it was.
#[test]
fn an_old_hook_name_in_a_config_is_reported_with_its_new_name() {
    let dir = scratch("renamed");
    let config = "dodstudio_hide_scoreboard 1\n\
                      exec movie.cfg\n\
                      exec dodstudio_helper.cfg\n";
    let movie = "// dodstudio_spec_lock 1\n\
                     bind F7 \"dodstudio_clear_decals\"\n\
                     ds_hud_hide_crosshair 1; dodstudio_hide_crosshair 1\n";
    std::fs::write(dir.join("config.cfg"), config).unwrap();
    std::fs::write(dir.join("movie.cfg"), movie).unwrap();

    let scan = scan(&dir);
    let found: Vec<(&str, &str, String, usize)> = scan
        .renamed
        .iter()
        .map(|r| (r.old.as_str(), r.new.as_str(), r.file_name(), r.line))
        .collect();
    assert_eq!(
        found,
        vec![
            (
                "dodstudio_hide_scoreboard",
                "ds_hud_hide_scoreboard",
                "config.cfg".to_string(),
                1
            ),
            (
                "dodstudio_clear_decals",
                "ds_map_clear_decals",
                "movie.cfg".to_string(),
                2
            ),
            (
                "dodstudio_hide_crosshair",
                "ds_hud_hide_crosshair",
                "movie.cfg".to_string(),
                3
            ),
        ]
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("config.cfg")).unwrap(),
        config
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("movie.cfg")).unwrap(),
        movie
    );
}

#[test]
fn an_old_hook_name_typed_into_a_command_list_is_reported() {
    let commands = vec![
        " dodstudio_spec_lock 1 ".to_string(),
        "ds_spec_lock 1".to_string(),
        "mirv_fov 90".to_string(),
    ];
    assert_eq!(
        renamed_commands(&commands),
        vec![(
            "dodstudio_spec_lock".to_string(),
            "ds_spec_lock".to_string(),
            "dodstudio_spec_lock 1".to_string()
        )]
    );
}

#[test]
fn a_comment_is_not_a_setting_and_a_semicolon_separates_commands() {
    let dir = scratch("comment");
    std::fs::write(
        dir.join("config.cfg"),
        "// r_decals 999\nmirv_fov 90; r_decals 128\n",
    )
    .unwrap();

    let scan = scan(&dir);
    assert_eq!(scan.effective("mirv_fov").unwrap().value, "90");
    assert_eq!(scan.effective("r_decals").unwrap().value, "128");
}

#[test]
fn a_subcommand_is_not_an_assignment() {
    // `mirv_fov handleZoom enabled 1` appears in real movie configs.
    let dir = scratch("subcommand");
    std::fs::write(
        dir.join("config.cfg"),
        "mirv_fov handleZoom enabled \"1\"\n",
    )
    .unwrap();

    assert!(scan(&dir).effective("mirv_fov").is_none());
}

#[test]
fn a_config_nothing_execs_sets_nothing_but_is_still_reported() {
    // A movie.cfg no one runs is not a warning about the current capture —
    // but it is one `exec` away from being one.
    let dir = scratch("unreferenced");
    std::fs::write(dir.join("config.cfg"), "cl_showfps 1\n").unwrap();
    std::fs::write(dir.join("movie.cfg"), "mirv_fov \"105\"\n").unwrap();

    let scan = scan(&dir);
    assert!(scan.effective("mirv_fov").is_none());
    assert_eq!(scan.unreferenced.len(), 1);
    assert_eq!(scan.unreferenced[0].file_name().unwrap(), "movie.cfg");
}

fn init(commands: &[&str]) -> Vec<String> {
    commands.iter().map(|c| c.to_string()).collect()
}

fn before(command: &str, offset_seconds: f32) -> ScheduledCommand<'_> {
    ScheduledCommand {
        command,
        after: false,
        offset_seconds,
    }
}

fn after(command: &str, offset_seconds: f32) -> ScheduledCommand<'_> {
    ScheduledCommand {
        command,
        after: true,
        offset_seconds,
    }
}

#[test]
fn an_init_command_that_overrides_a_config_value_is_a_conflict() {
    // The case that matters: someone sets mirv_fov in Init Commands with a
    // different value sitting in movie.cfg. The init command wins, which is
    // the point — but they should be told, not left to notice.
    let dir = scratch("override");
    std::fs::write(dir.join("config.cfg"), "exec movie.cfg\n").unwrap();
    std::fs::write(dir.join("movie.cfg"), "mirv_fov \"105\"\nr_decals \"0\"\n").unwrap();

    let w = value_warnings(
        &scan(&dir),
        &init(&["mirv_fov 90", "sys_autodir", "cl_showfps 1"]),
        3,
        &[],
    );

    assert_eq!(w.conflicts.len(), 1, "{:?}", w.conflicts);
    let c = &w.conflicts[0];
    assert_eq!(c.cvar, "mirv_fov");
    assert_eq!(c.effective.value, "90");
    assert_eq!(c.effective.source, ValueSource::Initial);
    assert_eq!(c.values[0].value, "105");
    assert!(
        matches!(&c.values[0].source, ValueSource::Config { file, line: 1 }
                if file.file_name().unwrap() == "movie.cfg")
    );
}

#[test]
fn a_command_beaten_by_a_later_one_in_the_same_list_is_a_conflict() {
    // The real shape: the user types `mirv_movie_fps 500`, and the pipeline
    // appends the Capture FPS setting after it. Both lines are on screen and
    // only the second one happens.
    let w = value_warnings(
        &CfgScan::default(),
        &init(&[
            "mirv_fov 105",
            "mirv_movie_fps 500",
            "sys_autodir",
            "mirv_movie_fps 120",
        ]),
        2,
        &[],
    );

    assert_eq!(w.conflicts.len(), 1, "{:?}", w.conflicts);
    let c = &w.conflicts[0];
    assert_eq!(c.cvar, "mirv_movie_fps");
    assert_eq!(c.values[0].value, "500");
    assert_eq!(c.values[0].source, ValueSource::Initial);
    assert_eq!(c.effective.value, "120");
    assert_eq!(
        c.effective.source,
        ValueSource::App,
        "so a caller can tell who appended it"
    );
}

#[test]
fn repeating_the_same_value_is_silent_everywhere() {
    // Rule 1's "who cares" case: the same value in a config twice, in
    // Initial Commands and before the clip is nothing worth saying.
    let dir = scratch("same_everywhere");
    std::fs::write(
        dir.join("config.cfg"),
        "sensitivity \"2\"\nexec movie.cfg\n",
    )
    .unwrap();
    std::fs::write(dir.join("movie.cfg"), "sensitivity 2.0\n").unwrap();

    let w = value_warnings(
        &scan(&dir),
        &init(&["sensitivity 2", "sensitivity \"2\""]),
        2,
        &[before("sensitivity 2", 3.0)],
    );
    assert_eq!(w, ValueWarnings::default());
}

#[test]
fn setting_a_cvar_to_what_the_config_already_says_is_silent() {
    let dir = scratch("agrees");
    std::fs::write(dir.join("config.cfg"), "mirv_fov \"105\"\n").unwrap();

    let w = value_warnings(&scan(&dir), &init(&["mirv_fov 105"]), 1, &[]);
    assert!(w.conflicts.is_empty(), "{:?}", w.conflicts);
}

#[test]
fn a_conflict_only_between_config_lines_is_not_reported() {
    // Nothing the app sends is involved, and `echo` lines count as
    // assignments to the scanner. See `value_warnings`' doc comment.
    let dir = scratch("config_only");
    std::fs::write(
        dir.join("config.cfg"),
        "gl_max_size 256\necho \"one\"\nexec movie.cfg\n",
    )
    .unwrap();
    std::fs::write(dir.join("movie.cfg"), "gl_max_size 2048\necho \"two\"\n").unwrap();

    let w = value_warnings(&scan(&dir), &[], 0, &[]);
    assert!(w.conflicts.is_empty(), "{:?}", w.conflicts);
}

#[test]
fn a_config_only_conflict_is_reported_once_the_app_names_the_cvar() {
    // Any app-side value, even one agreeing with the effective config line,
    // brings the whole pool into view.
    let dir = scratch("config_then_init");
    std::fs::write(dir.join("config.cfg"), "gl_max_size 256\nexec movie.cfg\n").unwrap();
    std::fs::write(dir.join("movie.cfg"), "gl_max_size 2048\n").unwrap();

    let w = value_warnings(&scan(&dir), &init(&["gl_max_size 2048"]), 1, &[]);
    assert_eq!(w.conflicts.len(), 1, "{:?}", w.conflicts);
    assert_eq!(w.conflicts[0].values.len(), 3);
    assert_eq!(w.conflicts[0].effective.value, "2048");
}

#[test]
fn the_last_before_to_fire_is_the_effective_value() {
    // Scheduled is given in firing order: 10s back runs before 2s back, so
    // the 2s one, nearest the highlight, is what the clip records at.
    let w = value_warnings(
        &CfgScan::default(),
        &[],
        0,
        &[
            before("hud_deathnotice_time 555", 10.0),
            before("hud_deathnotice_time 1", 2.0),
        ],
    );
    assert_eq!(w.conflicts.len(), 1, "{:?}", w.conflicts);
    assert_eq!(w.conflicts[0].effective.value, "1");
    assert_eq!(
        w.conflicts[0].effective.source,
        ValueSource::ScheduledBefore {
            offset_seconds: 2.0
        }
    );
}

#[test]
fn every_combination_of_sources_conflicts() {
    let dir = scratch("combos");
    std::fs::write(dir.join("config.cfg"), "sensitivity 1\n").unwrap();
    let cfg = scan(&dir);
    let none = CfgScan::default();

    // config + Initial, config + Scheduled, Initial + Scheduled, all three.
    let cases: [(&CfgScan, Vec<String>, Vec<ScheduledCommand>, &str, usize); 4] = [
        (&cfg, init(&["sensitivity 2"]), vec![], "2", 2),
        (&cfg, vec![], vec![before("sensitivity 3", 1.0)], "3", 2),
        (
            &none,
            init(&["sensitivity 2"]),
            vec![before("sensitivity 3", 1.0)],
            "3",
            2,
        ),
        (
            &cfg,
            init(&["sensitivity 2"]),
            vec![before("sensitivity 3", 1.0)],
            "3",
            3,
        ),
    ];
    for (scan, init_commands, scheduled, effective, count) in cases {
        let w = value_warnings(scan, &init_commands, init_commands.len(), &scheduled);
        assert_eq!(w.conflicts.len(), 1, "{:?}", w.conflicts);
        assert_eq!(w.conflicts[0].effective.value, effective);
        assert_eq!(w.conflicts[0].values.len(), count);
    }
}

#[test]
fn an_after_value_is_never_part_of_the_conflict_pool() {
    let w = value_warnings(
        &CfgScan::default(),
        &init(&["hud_deathnotice_time 6"]),
        1,
        &[after("hud_deathnotice_time 1", 0.5)],
    );
    assert!(w.conflicts.is_empty(), "{:?}", w.conflicts);
}

#[test]
fn an_unpaired_after_that_differs_from_the_baseline_is_asymmetric() {
    let w = value_warnings(
        &CfgScan::default(),
        &init(&["hud_deathnotice_time 6"]),
        1,
        &[after("hud_deathnotice_time 1", 0.5)],
    );
    assert_eq!(w.asymmetric.len(), 1, "{:?}", w.asymmetric);
    let a = &w.asymmetric[0];
    assert_eq!(a.after.value, "1");
    assert_eq!(a.baseline.value, "6");
    assert_eq!(a.baseline.source, ValueSource::Initial);
}

#[test]
fn the_last_unpaired_after_is_what_later_clips_start_from() {
    // 5 then back to the baseline 6 before the next clip: no asymmetry.
    let restored = value_warnings(
        &CfgScan::default(),
        &init(&["hud_deathnotice_time 6"]),
        1,
        &[
            after("hud_deathnotice_time 5", 0.5),
            after("hud_deathnotice_time 6", 2.0),
        ],
    );
    assert!(restored.asymmetric.is_empty(), "{:?}", restored.asymmetric);

    let not_restored = value_warnings(
        &CfgScan::default(),
        &init(&["hud_deathnotice_time 6"]),
        1,
        &[
            after("hud_deathnotice_time 6", 0.5),
            after("hud_deathnotice_time 5", 2.0),
        ],
    );
    assert_eq!(not_restored.asymmetric.len(), 1);
    assert_eq!(not_restored.asymmetric[0].after.value, "5");
}

#[test]
fn an_unpaired_after_matching_the_baseline_is_silent() {
    let w = value_warnings(
        &CfgScan::default(),
        &init(&["hud_deathnotice_time 6"]),
        1,
        &[after("hud_deathnotice_time 6.0", 0.5)],
    );
    assert_eq!(w, ValueWarnings::default());
}

#[test]
fn an_unpaired_after_with_no_baseline_anywhere_is_silent() {
    let w = value_warnings(
        &CfgScan::default(),
        &[],
        0,
        &[after("hud_deathnotice_time 1", 0.5)],
    );
    assert_eq!(w, ValueWarnings::default());
}

#[test]
fn any_before_pairs_the_after_and_silences_rule_two() {
    // Baseline 6, After 1: asymmetric. Add a Before at 555 and Rule 2 goes
    // quiet at once; Rule 1 now reports 6 against 555 instead.
    let w = value_warnings(
        &CfgScan::default(),
        &init(&["hud_deathnotice_time 6"]),
        1,
        &[
            before("hud_deathnotice_time 555", 3.0),
            after("hud_deathnotice_time 1", 0.5),
        ],
    );
    assert!(w.asymmetric.is_empty(), "{:?}", w.asymmetric);
    assert_eq!(w.conflicts.len(), 1);
    assert_eq!(w.conflicts[0].effective.value, "555");
}

#[test]
fn a_paired_after_with_a_differing_value_is_the_accepted_gap() {
    // #216's known, accepted gap: a Before equal to the baseline pairs the
    // After, Rule 1's pool is all one value, and the After's own (still
    // different) value is looked at by neither rule. Asserted so a change
    // here is a decision, not an accident.
    let w = value_warnings(
        &CfgScan::default(),
        &init(&["hud_deathnotice_time 6"]),
        1,
        &[
            before("hud_deathnotice_time 6", 3.0),
            after("hud_deathnotice_time 1", 0.5),
        ],
    );
    assert_eq!(w, ValueWarnings::default());
}

#[test]
fn a_scheduled_r_decals_is_flagged_however_it_is_written() {
    // Setting the ring mid-demo strands every decal above the new limit and
    // breaks the flush, while the capture still completes and still looks
    // plausible — so this is the one that has to be caught by name.
    let hits = mid_demo_hazards(&["sensitivity 3".to_string(), "R_Decals 128".to_string()]);

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].0, "r_decals");
    assert_eq!(hits[0].1, "R_Decals 128");
}

#[test]
fn a_scheduled_mirv_fov_is_flagged() {
    // The decal flush sizes its whole sweep's on-screen test against
    // capture_fov_resolved, read once as a pre-pass before the demo
    // plays — a Scheduled Command changing it mid-clip doesn't
    // retroactively resize anything the flush already decided.
    let hits = mid_demo_hazards(&["sensitivity 3".to_string(), "mirv_fov 105".to_string()]);

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].0, "mirv_fov");
    assert_eq!(hits[0].1, "mirv_fov 105");
}

#[test]
fn a_scheduled_gl_widescreenfov_is_flagged() {
    // Widens the effective on-screen FOV the same way mirv_fov/default_fov
    // do, but capture_fov_resolved never reads it — a mid-demo toggle
    // invalidates the flush's sizing with nothing that could have caught it.
    let hits = mid_demo_hazards(&["gl_widescreenfov 1".to_string()]);

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].0, "gl_widescreenfov");
    assert_eq!(hits[0].1, "gl_widescreenfov 1");
}

#[test]
fn every_pipeline_owned_recording_mechanic_is_flagged() {
    // mirv_movie_filename races the block-routing aliases and can
    // misroute frames to the wrong take folder; mirv_recordmovie_start/
    // stop race the pipeline's own record-bounds scheduling;
    // mirv_movie_fps is pinned once at load and everything downstream
    // assumes it never changes; mirv_movie_ffmpeg configures the
    // direct-to-video pipe before anything records into it;
    // host_framerate races sys_fast_forward/sys_normal_speed's own
    // timing. All of them dangerous scheduled mid-demo — whether typing
    // them anywhere at all is banned outright is `banned_commands`'
    // narrower list, tested separately below. mirv_movie_separate_hud is
    // deliberately absent — see `MID_DEMO_HAZARDS`'s own doc comment.
    let hits = mid_demo_hazards(&[
        "mirv_movie_filename foo".to_string(),
        "mirv_recordmovie_start".to_string(),
        "mirv_recordmovie_stop".to_string(),
        "mirv_movie_fps 500".to_string(),
        "mirv_movie_ffmpeg all enabled 1".to_string(),
        "host_framerate 0.05".to_string(),
    ]);

    let flagged: Vec<&str> = hits.iter().map(|(cvar, _)| cvar.as_str()).collect();
    assert_eq!(
        flagged,
        vec![
            "mirv_movie_filename",
            "mirv_recordmovie_start",
            "mirv_recordmovie_stop",
            "mirv_movie_fps",
            "mirv_movie_ffmpeg",
            "host_framerate",
        ]
    );
}

#[test]
fn mirv_movie_separate_hud_is_untracked_everywhere() {
    // No setting exists behind it any more (#214), and
    // nothing in this codebase has verified what a mid-demo toggle does
    // — unlike r_decals/mirv_fov, which are measured. Rather than assert
    // a mechanism nobody has checked, it gets no special treatment at
    // all: not banned, not a mid-demo hazard, not shadowed.
    let commands = vec!["mirv_movie_separate_hud 1".to_string()];
    assert!(banned_commands(&commands).is_empty());
    assert!(mid_demo_hazards(&commands).is_empty());
    assert!(scheduled_banned_commands(&commands).is_empty());
}

#[test]
fn banned_commands_covers_exactly_the_tier_1_set() {
    // No dedicated setting corresponds to any of these, and no scenario
    // has been found where typing one is anything but a misunderstanding
    // — banned outright, unlike mirv_movie_fps (redundant with a real
    // setting, so shadowed-with-a-warning instead) or r_decals/mirv_fov
    // (the user's own stated value wins). Does NOT
    // include mirv_movie_filename any more — see
    // `scheduled_banned_commands_flags_the_decal_flush_cvars_and_mirv_movie_filename`.
    // r_drawentities/cl_lw are here for a different reason (no legitimate
    // non-default value, not "no dedicated setting"), but the ban itself
    // is identical, so one list-membership test covers both reasons.
    let hits = banned_commands(&[
        "mirv_recordmovie_start".to_string(),
        "mirv_recordmovie_stop".to_string(),
        "mirv_movie_ffmpeg all enabled 1".to_string(),
        "host_framerate 0.05".to_string(),
        "r_norefresh 1".to_string(),
        "r_drawentities 0".to_string(),
        "cl_lw 0".to_string(),
    ]);

    let flagged: Vec<&str> = hits.iter().map(|(cvar, _)| cvar.as_str()).collect();
    assert_eq!(
        flagged,
        vec![
            "mirv_recordmovie_start",
            "mirv_recordmovie_stop",
            "mirv_movie_ffmpeg",
            "host_framerate",
            "r_norefresh",
            "r_drawentities",
            "cl_lw",
        ]
    );
}

#[test]
fn the_fast_forward_cvars_are_a_hazard_only_when_scheduled() {
    // sys_fast_forward sets them between clips and sys_normal_speed puts
    // the Initial Commands value back at every pre-roll (#724): typed in
    // Initial Commands they are honoured, so no tier reports them there;
    // scheduled, they last only until the next clip.
    let commands: Vec<String> = ["fps_override 1", "FPS_MAX 300", "gl_vsync 0"]
        .iter()
        .map(|c| c.to_string())
        .collect();
    let hits = mid_demo_hazards(&commands);
    let hazards: Vec<&str> = hits.iter().map(|(cvar, _)| cvar.as_str()).collect();
    assert_eq!(hazards, FAST_FORWARD_CVARS);
    assert!(banned_commands(&commands).is_empty());
    assert!(scheduled_banned_commands(&commands).is_empty());
    assert!(noop_commands_in_init(&commands).is_empty());
}

#[test]
fn r_norefresh_is_the_fast_forwards_and_refused_like_host_framerate() {
    let commands = vec!["r_norefresh 1".to_string()];
    assert_eq!(banned_commands(&commands).len(), 1);
    assert_eq!(mid_demo_hazards(&commands).len(), 1);
}

#[test]
fn fatal_cvar_hazards_flags_only_a_non_default_value() {
    let dir = scratch("fatal");
    std::fs::write(
        dir.join("config.cfg"),
        "sv_cheats \"1\"\nr_drawentities \"0\"\ncl_lw \"1\"\nsensitivity \"3\"\n",
    )
    .unwrap();

    let scan = scan(&dir);
    let hazards = fatal_cvar_hazards(&scan);

    // cl_lw is explicitly 1 (the required value) and sensitivity is not a
    // FATAL_CVARS entry at all -- neither should be reported. sv_cheats is
    // set only so r_drawentities is reachable at all; see
    // r_drawentities_alone_is_not_fatal_because_the_engine_clamps_it.
    assert_eq!(hazards.len(), 1, "{hazards:?}");
    assert_eq!(hazards[0].cvar, "r_drawentities");
    assert_eq!(hazards[0].value, "0");
    assert_eq!(hazards[0].required, "1");
    assert_eq!(hazards[0].file_name(), "config.cfg");
}

#[test]
fn fatal_cvar_hazards_resolves_last_value_wins_like_everything_else() {
    // A config that sets it wrong and then corrects it is not a hazard --
    // that is genuinely the value the engine ends up running with.
    let dir = scratch("fatal_corrected");
    std::fs::write(
        dir.join("config.cfg"),
        "r_drawentities \"0\"\nr_drawentities \"1\"\n",
    )
    .unwrap();

    let scan = scan(&dir);
    assert!(fatal_cvar_hazards(&scan).is_empty());
}

#[test]
fn fatal_cvar_hazards_ignores_an_unreferenced_config() {
    // A movie.cfg sitting in the folder that nothing execs sets nothing --
    // same rule `CfgScan::effective` already applies to everything else.
    let dir = scratch("fatal_unreferenced");
    std::fs::write(dir.join("movie.cfg"), "r_drawentities \"0\"\n").unwrap();

    let scan = scan(&dir);
    assert!(fatal_cvar_hazards(&scan).is_empty());
}

#[test]
fn fatal_cvars_are_in_banned_commands_too() {
    // The config-file hazard above and the typed-command ban are two
    // halves of the same fact; letting them drift apart would leave one
    // route to the crash unblocked while the other still warns about it.
    for entry in FATAL_CVARS {
        assert!(
            BANNED_COMMANDS.contains(&entry.cvar),
            "{} missing from BANNED_COMMANDS",
            entry.cvar
        );
    }
}

#[test]
fn r_drawentities_alone_is_not_fatal_because_the_engine_clamps_it() {
    // GoldSrc resets r_drawentities to 1.0 itself while sv_cheats is 0
    // (hw.dll 0x1d455c9), so the value never reaches DoD's client and the
    // config line is inert. Reporting it would block a capture over
    // something harmless -- confirmed live, the game does not quit.
    let dir = scratch("clamped");
    std::fs::write(dir.join("config.cfg"), "r_drawentities \"0\"\n").unwrap();

    assert!(fatal_cvar_hazards(&scan(&dir)).is_empty());
}

#[test]
fn r_drawentities_is_fatal_once_a_config_also_enables_cheats() {
    // A non-zero sv_cheats skips the engine's clamp entirely, so the value
    // sticks and DoD's own CHud::Redraw check quits the game.
    let dir = scratch("clamped_cheats");
    std::fs::write(
        dir.join("config.cfg"),
        "sv_cheats \"1\"\nr_drawentities \"0\"\n",
    )
    .unwrap();

    let hazards = fatal_cvar_hazards(&scan(&dir));
    assert_eq!(hazards.len(), 1, "{hazards:?}");
    assert_eq!(hazards[0].cvar, "r_drawentities");
}

#[test]
fn cl_lw_is_fatal_with_or_without_cheats() {
    // No engine clamp on this one -- it takes whatever value it is given,
    // which is why it is the half that reproduced live.
    for (tag, extra) in [("cllw_plain", ""), ("cllw_cheats", "sv_cheats \"1\"\n")] {
        let dir = scratch(tag);
        std::fs::write(dir.join("config.cfg"), format!("{extra}cl_lw \"0\"\n")).unwrap();

        let hazards = fatal_cvar_hazards(&scan(&dir));
        assert_eq!(hazards.len(), 1, "extra={extra:?} -> {hazards:?}");
        assert_eq!(hazards[0].cvar, "cl_lw");
    }
}

#[test]
fn banned_commands_no_longer_catches_mirv_movie_filename() {
    let hits = banned_commands(&["mirv_movie_filename foo".to_string()]);
    assert!(hits.is_empty(), "{:?}", hits);
}

#[test]
fn scheduled_banned_commands_flags_the_decal_flush_cvars_and_mirv_movie_filename() {
    let hits = scheduled_banned_commands(&[
        "sensitivity 3".to_string(),
        "r_decals 512".to_string(),
        "mirv_fov 105".to_string(),
        "gl_widescreenfov 1".to_string(),
        "mirv_movie_filename foo".to_string(),
    ]);

    let flagged: Vec<&str> = hits.iter().map(|(cvar, _)| cvar.as_str()).collect();
    assert_eq!(
        flagged,
        vec![
            "r_decals",
            "mirv_fov",
            "gl_widescreenfov",
            "mirv_movie_filename"
        ]
    );
}

#[test]
fn a_scheduled_mirv_agr_is_refused_but_an_initial_one_is_not() {
    // AGR capture mode starts and stops mirv_agr at each block's own
    // bounds (#450); a scheduled one collides with that. At demo load it
    // collides with nothing, so it is not refused there.
    let commands = vec![
        "mirv_agr start \"C:\\agr\\take.agr\"".to_string(),
        "mirv_agr stop".to_string(),
    ];
    let flagged: Vec<String> = scheduled_banned_commands(&commands)
        .into_iter()
        .map(|(cvar, _)| cvar)
        .collect();
    assert_eq!(flagged, vec!["mirv_agr", "mirv_agr"]);
    assert!(banned_commands(&commands).is_empty());
    assert_eq!(mid_demo_hazards(&commands).len(), 2);
}

#[test]
fn scheduled_banned_commands_ignores_everything_else() {
    let hits = scheduled_banned_commands(&[
        "mirv_movie_fps 500".to_string(),
        "host_framerate 0.05".to_string(),
        "exec somefile.cfg".to_string(),
        "quit".to_string(),
    ]);

    assert!(hits.is_empty(), "{:?}", hits);
}

#[test]
fn noop_commands_in_init_covers_only_mirv_movie_filename() {
    // exec and quit are the demo filter's now (demo_filtered_commands).
    let hits = noop_commands_in_init(&[
        "sensitivity 3".to_string(),
        "mirv_movie_filename foo".to_string(),
        "exec somefile.cfg".to_string(),
        "quit".to_string(),
    ]);

    let flagged: Vec<&str> = hits.iter().map(|(cvar, _)| cvar.as_str()).collect();
    assert_eq!(flagged, vec!["mirv_movie_filename"]);
}

#[test]
fn the_demo_filter_drops_a_name_containing_any_of_its_substrings() {
    use DemoFilterRule::NameContains;
    assert_eq!(
        demo_filter_rule("mirv_matte_setcolor 255 0 255"),
        Some(NameContains("_set"))
    );
    assert_eq!(
        demo_filter_rule("mirv_draw_sv_hitboxes_setucolor 255 0 0"),
        Some(NameContains("_set"))
    );
    assert_eq!(demo_filter_rule("kill"), Some(NameContains("kill")));
    assert_eq!(
        demo_filter_rule("cl_killsound 0"),
        Some(NameContains("kill"))
    );
    assert_eq!(
        demo_filter_rule("bind f7 screenshot"),
        Some(NameContains("bind"))
    );
    assert_eq!(demo_filter_rule("quit"), Some(NameContains("quit")));
    assert_eq!(demo_filter_rule("exit"), Some(NameContains("exit")));
    assert_eq!(
        demo_filter_rule("sv_restart 1"),
        Some(NameContains("_restart"))
    );
    assert_eq!(
        demo_filter_rule("writecfg mine"),
        Some(NameContains("writecfg"))
    );
}

#[test]
fn the_demo_filter_name_test_ignores_case() {
    assert_eq!(
        demo_filter_rule("MIRV_Matte_SetColor 255 0 255"),
        Some(DemoFilterRule::NameContains("_set"))
    );
    assert_eq!(
        demo_filter_rule("  Kill  "),
        Some(DemoFilterRule::NameContains("kill"))
    );
}

#[test]
fn the_demo_filter_drops_a_name_starting_with_connect_but_not_reconnect() {
    assert_eq!(
        demo_filter_rule("connect 127.0.0.1"),
        Some(DemoFilterRule::NameStartsWith("connect"))
    );
    assert_eq!(
        demo_filter_rule("connectionless"),
        Some(DemoFilterRule::NameStartsWith("connect"))
    );
    assert_eq!(demo_filter_rule("reconnect"), None);
}

#[test]
fn the_demo_filter_drops_an_alias_line_and_exec_anywhere() {
    assert_eq!(
        demo_filter_rule("alias foo \"echo hi\""),
        Some(DemoFilterRule::LineStartsWith("alias "))
    );
    assert_eq!(
        demo_filter_rule("exec movie.cfg"),
        Some(DemoFilterRule::LineContains("exec"))
    );
    // Anywhere on the line, an argument included.
    assert_eq!(
        demo_filter_rule("echo executed"),
        Some(DemoFilterRule::LineContains("exec"))
    );
    // Only `alias ` with its space: a name that merely starts with it
    // is not an alias line.
    assert_eq!(demo_filter_rule("aliases"), None);
}

#[test]
fn the_demo_filter_drops_a_word_starting_with_a_filtered_prefix_anywhere() {
    use DemoFilterRule::WordStartsWith;
    assert_eq!(
        demo_filter_rule("echo got a kill"),
        Some(WordStartsWith("kill"))
    );
    assert_eq!(
        demo_filter_rule("echo hi;quit"),
        Some(WordStartsWith("quit"))
    );
    assert_eq!(
        demo_filter_rule("echo _settings"),
        Some(WordStartsWith("_set"))
    );
    // Inside a word is fine for these.
    assert_eq!(demo_filter_rule("echo skill"), None);
    assert_eq!(demo_filter_rule("echo reconnect now"), None);
}

#[test]
fn the_demo_filter_lets_ordinary_commands_through() {
    for command in [
        "echo ok",
        "sensitivity 3",
        "mirv_fov 90",
        "spec_autodirector 1",
        "mirv_movie_fps 60",
        "sys_autodir",
        "sys_record_start",
        "dodstudio_chain_01_route_0",
        "dodstudio_chain_01_next",
        "echo \"[dod-studio] BREADCRUMB - Tick 500\"",
        "",
        "   ",
    ] {
        assert_eq!(demo_filter_rule(command), None, "{command:?}");
    }
}

#[test]
fn demo_filtered_commands_reports_each_dropped_command_in_order() {
    let hits = demo_filtered_commands(&[
        "sensitivity 3".to_string(),
        "  exec somefile.cfg ".to_string(),
        "mirv_matte_setcolor 255 0 255".to_string(),
        "quit".to_string(),
    ]);
    let got: Vec<(&str, &str, &str)> = hits
        .iter()
        .map(|h| (h.command.as_str(), h.rule.kind(), h.rule.pattern()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("exec somefile.cfg", "lineContains", "exec"),
            ("mirv_matte_setcolor 255 0 255", "nameContains", "_set"),
            ("quit", "nameContains", "quit"),
        ]
    );
}

#[test]
fn no_command_the_pipeline_owns_is_dropped_by_the_demo_filter() {
    // A tiered command the filter also dropped would be reported twice
    // and refused for the wrong reason.
    for name in BANNED_COMMANDS
        .iter()
        .chain(SCHEDULED_BANNED_COMMANDS)
        .chain(MID_DEMO_HAZARDS)
        .chain(NOOP_IN_INIT_COMMANDS)
        .chain(FAST_FORWARD_CVARS)
    {
        assert_eq!(demo_filter_rule(&format!("{name} 1")), None, "{name}");
    }
}

#[test]
fn banned_commands_does_not_catch_tier_2_or_tier_3_cvars() {
    // mirv_movie_fps is redundant-with-a-setting (shadowed, not banned);
    // mirv_movie_separate_hud is untracked entirely (see
    // `mirv_movie_separate_hud_is_untracked_everywhere`); r_decals/
    // mirv_fov/gl_widescreenfov are either respected (Tier 3) or only a
    // Scheduled-Commands hazard, not an everywhere-ban.
    let hits = banned_commands(&[
        "mirv_movie_fps 500".to_string(),
        "mirv_movie_separate_hud 1".to_string(),
        "r_decals 256".to_string(),
        "mirv_fov 90".to_string(),
        "gl_widescreenfov 1".to_string(),
    ]);

    assert!(hits.is_empty(), "{:?}", hits);
}

#[test]
fn the_last_assignment_in_a_list_is_the_one_that_holds() {
    let commands = vec![
        "mirv_movie_fps 500".to_string(),
        "sys_autodir".to_string(),
        "mirv_movie_fps 120".to_string(),
    ];

    assert_eq!(
        effective_in(&commands, "mirv_movie_fps").as_deref(),
        Some("120")
    );
    assert_eq!(effective_in(&commands, "mirv_fov"), None);
    assert_eq!(
        assigned_cvar("mirv_movie_fps 500"),
        Some(("mirv_movie_fps".to_string(), "500".to_string()))
    );
    assert_eq!(assigned_cvar("sys_autodir"), None);
}

#[test]
fn every_assignment_is_recorded_not_just_the_ones_the_pipeline_reads() {
    // Most configs never mention r_decals. The collisions that surprise
    // people are the ones nobody thought to watch for.
    let dir = scratch("all_cvars");
    std::fs::write(
        dir.join("config.cfg"),
        "volume \"0.5\"\nzoom_sensitivity_ratio \"1.2\"\n+mlook\nstopsound\n",
    )
    .unwrap();

    let scan = scan(&dir);
    assert_eq!(scan.effective("volume").unwrap().value, "0.5");
    assert!(
        scan.effective("+mlook").is_none(),
        "a verb is not an assignment"
    );
    assert!(
        scan.effective("stopsound").is_none(),
        "nor is a bare command"
    );
    assert_eq!(
        value_warnings(&scan, &["volume 1".to_string()], 1, &[])
            .conflicts
            .len(),
        1,
        "and a collision on any of them is worth reporting"
    );
}

#[test]
fn a_config_that_execs_itself_terminates() {
    let dir = scratch("cycle");
    std::fs::write(dir.join("config.cfg"), "exec loop.cfg\n").unwrap();
    std::fs::write(dir.join("loop.cfg"), "exec config.cfg\nr_decals 8\n").unwrap();

    assert_eq!(scan(&dir).effective("r_decals").unwrap().value, "8");
}

#[test]
fn an_exec_cannot_climb_out_of_the_mod_folder() {
    let dir = scratch("escape");
    std::fs::write(dir.join("config.cfg"), "exec ../../../windows/win.ini\n").unwrap();

    // Nothing to assert about the outcome beyond it not being read: the
    // point is that the traversal is refused rather than attempted.
    assert_eq!(scan(&dir).files_read, 1);
}

#[test]
fn exec_is_looked_for_in_the_game_folder_then_addon_then_downloads() {
    let root = Path::new("install");
    assert_eq!(
        exec_search_dirs(&root.join("dod")),
        vec![
            root.join("dod"),
            root.join("dod_addon"),
            root.join("dod_downloads")
        ]
    );
}

#[test]
fn an_exec_the_game_refuses_resolves_to_nothing() {
    let root = install(
        "exec_refused",
        &[
            ("dod/movie.cfg", "fps_max 100\n"),
            ("dod/movie", "fps_max 1\n"),
            ("dod/my.movie.cfg", "fps_max 2\n"),
            ("dod/cfg/sub.cfg", "fps_max 3\n"),
            ("dod/valve.rc", "fps_max 4\n"),
        ],
    );
    let dirs = exec_search_dirs(&root.join("dod"));
    // No extension is added, and only .cfg and .rc run.
    assert_eq!(resolve_exec(&dirs, "movie"), None);
    assert_eq!(resolve_exec(&dirs, "movie.txt"), None);
    // More than one dot: "invalid filename".
    assert_eq!(resolve_exec(&dirs, "my.movie.cfg"), None);
    // Backslash, colon, tilde: "invalid path".
    assert_eq!(resolve_exec(&dirs, "cfg\\sub.cfg"), None);
    assert_eq!(resolve_exec(&dirs, "c:movie.cfg"), None);
    assert_eq!(resolve_exec(&dirs, "~movie.cfg"), None);
    assert_eq!(
        resolve_exec(&dirs, "cfg/sub.cfg"),
        Some(root.join("dod").join("cfg/sub.cfg"))
    );
    assert_eq!(
        resolve_exec(&dirs, "\"movie.cfg\""),
        Some(root.join("dod").join("movie.cfg"))
    );
    assert_eq!(
        resolve_exec(&dirs, "valve.rc"),
        Some(root.join("dod").join("valve.rc"))
    );
}

#[test]
fn only_an_exec_line_has_an_exec_target() {
    assert_eq!(exec_target("exec movie.cfg"), Some("movie.cfg".into()));
    assert_eq!(
        exec_target("  EXEC \"my movie.cfg\""),
        Some("my movie.cfg".into())
    );
    assert_eq!(exec_target("exec"), None);
    assert_eq!(exec_target("execute 1"), None);
    assert_eq!(exec_target("sensitivity 2"), None);
}

/// An install root with a `dod` folder, and `files` written under it.
fn install(tag: &str, files: &[(&str, &str)]) -> Scratch {
    let root = scratch(tag);
    std::fs::create_dir_all(root.join("dod")).unwrap();
    for (path, text) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    root
}

#[test]
fn an_initial_exec_follows_the_files_it_execs_in_turn_from_any_folder() {
    let root = install(
        "initial_exec_chain",
        &[
            ("dod_addon/movie.cfg", "fps_max 100\nexec more.cfg\n"),
            (
                "dod/more.cfg",
                "hud_deathnotice_time 12\nbind f7 \"fps_max 1\"\n",
            ),
        ],
    );
    let followed = follow_initial_exec(&root.join("dod"), "exec movie.cfg").unwrap();

    assert_eq!(
        followed.file,
        Some(root.join("dod_addon").join("movie.cfg"))
    );
    let set: Vec<(&str, &str)> = followed
        .settings
        .iter()
        .map(|s| (s.cvar.as_str(), s.value.as_str()))
        .collect();
    assert_eq!(set, [("fps_max", "100"), ("hud_deathnotice_time", "12")]);
}

#[test]
fn an_initial_exec_loop_ends() {
    let root = install(
        "initial_exec_loop",
        &[
            ("dod/a.cfg", "exec b.cfg\nfps_max 60\n"),
            (
                "dod/b.cfg",
                "exec a.cfg\nexec b.cfg\nhud_deathnotice_time 3\n",
            ),
        ],
    );
    let followed = follow_initial_exec(&root.join("dod"), "exec a.cfg").unwrap();
    let cvars: Vec<&str> = followed.settings.iter().map(|s| s.cvar.as_str()).collect();
    assert_eq!(cvars, ["hud_deathnotice_time", "fps_max"]);
}

#[test]
fn an_initial_exec_of_a_missing_file_has_no_file_and_no_settings() {
    let root = install("initial_exec_missing", &[]);
    let followed = follow_initial_exec(&root.join("dod"), "exec nope.cfg").unwrap();
    assert_eq!(followed.file, None);
    assert!(followed.settings.is_empty());
    assert!(follow_initial_exec(&root.join("dod"), "sensitivity 2").is_none());
    assert!(
        follow_initial_exec(&root.join("dod"), "exec ../secret.cfg")
            .unwrap()
            .file
            .is_none()
    );
}

#[test]
fn an_exec_d_file_s_values_join_the_value_rules_where_the_exec_stands() {
    let root = install(
        "initial_exec_values",
        &[("dod_addon/wsod.cfg", "hud_deathnotice_time 12\n")],
    );
    let dod = root.join("dod");
    let commands = init(&["hud_deathnotice_time 6", "exec wsod.cfg"]);
    let execs: Vec<InitialExec> = commands
        .iter()
        .filter_map(|c| follow_initial_exec(&dod, c))
        .collect();
    let w = value_warnings_with_execs(&CfgScan::default(), &commands, 2, &[], &execs);

    assert_eq!(w.conflicts.len(), 1, "{:?}", w.conflicts);
    let c = &w.conflicts[0];
    assert_eq!(c.effective.value, "12");
    assert!(matches!(
        c.effective.source,
        ValueSource::Exec { line: 1, .. }
    ));
    // Rule 2 sees it as the baseline too.
    let w = value_warnings_with_execs(
        &CfgScan::default(),
        &init(&["exec wsod.cfg"]),
        1,
        &[after("hud_deathnotice_time 1", 0.5)],
        &execs,
    );
    assert_eq!(w.asymmetric.len(), 1, "{:?}", w.asymmetric);
    assert_eq!(w.asymmetric[0].baseline.value, "12");
}
