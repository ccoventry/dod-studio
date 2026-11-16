use super::*;
use crate::test_support::Scratch;

// ── Config warning report ────────────────────────────────────────────────
//
// These drive the whole report rather than one helper, so the wiring is
// covered too: the scan, `final_init_commands`, and the assembly. Two
// display bugs in this report were found by screenshot, which is the wrong
// way to find them.

/// A game folder as the engine expects it: `hl.exe` with `dod/` beside it,
/// a `config.cfg` that execs `movie.cfg`, and the values that caused all
/// this in `movie.cfg`.
fn fake_game(tag: &str) -> (Scratch, String) {
    let root = Scratch::new(format_args!("cfgrep_{tag}"));
    let dod = root.join("dod");
    std::fs::create_dir_all(&dod).unwrap();
    std::fs::write(
        dod.join("config.cfg"),
        "bind \"F7\" \"r_decals 4000\"\nexec movie.cfg\n",
    )
    .unwrap();
    std::fs::write(
        dod.join("movie.cfg"),
        // hud_deathnotice_time is here because it is the cvar people
        // genuinely pair around a clip — raised before, restored after.
        "r_decals \"0\"\nmirv_movie_fps \"300\"\nhud_deathnotice_time \"10\"\nmirv_fov \"105\"\n",
    )
    .unwrap();
    let exe = root.join("hl.exe");
    std::fs::write(&exe, b"").unwrap();
    (root, exe.to_string_lossy().to_string())
}

/// Same shape as `fake_game`, but movie.cfg never touches `r_decals` at
/// all — every other `fake_game`-based test relies on it being there
/// (0, specifically, to exercise the flush's own zero-ring case), so the
/// one test that needs the genuinely-nothing-anywhere case gets its own
/// fixture rather than changing that shared one out from under them.
fn fake_game_without_r_decals(tag: &str) -> (Scratch, String) {
    let root = Scratch::new(format_args!("cfgrep_nodecals_{tag}"));
    let dod = root.join("dod");
    std::fs::create_dir_all(&dod).unwrap();
    std::fs::write(dod.join("config.cfg"), "exec movie.cfg\n").unwrap();
    std::fs::write(dod.join("movie.cfg"), "mirv_movie_fps \"300\"\n").unwrap();
    let exe = root.join("hl.exe");
    std::fs::write(&exe, b"").unwrap();
    (root, exe.to_string_lossy().to_string())
}

/// Same shape again, but movie.cfg states `r_decals <value>` and nothing
/// else — for the tests that need a config-stated value other than the
/// 0 the shared `fake_game` fixture always carries.
fn fake_game_with_r_decals(tag: &str, value: &str) -> (Scratch, String) {
    let root = Scratch::new(format_args!("cfgrep_decals_{tag}"));
    let dod = root.join("dod");
    std::fs::create_dir_all(&dod).unwrap();
    std::fs::write(dod.join("config.cfg"), "exec movie.cfg\n").unwrap();
    std::fs::write(dod.join("movie.cfg"), format!("r_decals \"{}\"\n", value)).unwrap();
    let exe = root.join("hl.exe");
    std::fs::write(&exe, b"").unwrap();
    (root, exe.to_string_lossy().to_string())
}

/// Same shape again, movie.cfg assigning `mirv_movie_filename` — the
/// shared `fake_game` fixture's config.cfg only `bind`s it, which is not
/// an assignment the scanner records at all.
fn fake_game_with_mirv_movie_filename(tag: &str) -> (Scratch, String) {
    let root = Scratch::new(format_args!("cfgrep_moviefn_{tag}"));
    let dod = root.join("dod");
    std::fs::create_dir_all(&dod).unwrap();
    std::fs::write(dod.join("config.cfg"), "exec movie.cfg\n").unwrap();
    std::fs::write(dod.join("movie.cfg"), "mirv_movie_filename \"clip\"\n").unwrap();
    let exe = root.join("hl.exe");
    std::fs::write(&exe, b"").unwrap();
    (root, exe.to_string_lossy().to_string())
}

/// Same shape again, movie.cfg assigning `r_drawentities` to a value other
/// than 1. Only fatal when the config also turns cheats on: while
/// `sv_cheats` is 0 GoldSrc clamps the cvar back itself and DoD's client
/// never sees the value (`cfg_scan::FatalCvar::needs_sv_cheats`).
fn fake_game_with_r_drawentities(tag: &str, value: &str, cheats: bool) -> (Scratch, String) {
    let root = Scratch::new(format_args!("cfgrep_fatal_{tag}"));
    let dod = root.join("dod");
    std::fs::create_dir_all(&dod).unwrap();
    std::fs::write(dod.join("config.cfg"), "exec movie.cfg\n").unwrap();
    let cheat_line = if cheats { "sv_cheats \"1\"\n" } else { "" };
    std::fs::write(
        dod.join("movie.cfg"),
        format!("{}r_drawentities \"{}\"\n", cheat_line, value),
    )
    .unwrap();
    let exe = root.join("hl.exe");
    std::fs::write(&exe, b"").unwrap();
    (root, exe.to_string_lossy().to_string())
}

fn scheduled(command: &str, relation: &str, offset: f32) -> CustomCommand {
    CustomCommand {
        command: command.to_string(),
        offset,
        relation: if relation == "After" {
            CommandRelation::After
        } else {
            CommandRelation::Before
        },
    }
}

/// The report for `scan_game_configs`' arguments, in its order.
fn report_for(
    game_path: String,
    init_commands: Vec<String>,
    scheduled_commands: Vec<CustomCommand>,
    capture_fps: Option<i32>,
    decal_flush: Option<bool>,
    hook_runs_batch: Option<bool>,
) -> CfgReport {
    cfg_report(CfgReportRequest {
        game_path,
        init_commands,
        scheduled_commands,
        capture_fps,
        decal_flush,
        hook_runs_batch,
    })
}

fn report_scheduled(tag: &str, custom: Vec<CustomCommand>) -> CfgReport {
    let (_dir, game) = fake_game(tag);
    report_for(game, Vec::new(), custom, Some(120), Some(true), None)
}

/// The Rule 1 row for `cvar`, if any.
fn conflict<'a>(r: &'a CfgReport, cvar: &str) -> Option<&'a ValueConflictRow> {
    r.conflicts
        .iter()
        .find(|c| c.cvar.eq_ignore_ascii_case(cvar))
}

fn values_of(row: &ValueConflictRow) -> Vec<(&str, &str)> {
    row.values
        .iter()
        .map(|v| (v.value.as_str(), v.kind.as_str()))
        .collect()
}

#[test]
fn a_set_and_restore_pair_is_one_conflict_and_no_asymmetry() {
    // The real shape: raise a cvar before the clip, put it back after.
    // The Before conflicts with movie.cfg's 10 (Rule 1); the After is
    // paired, so Rule 2 says nothing, and it is never in Rule 1's pool.
    let r = report_scheduled(
        "pair",
        vec![
            scheduled("hud_deathnotice_time 555", "Before", 10.0),
            scheduled("hud_deathnotice_time 1", "After", 5.0),
        ],
    );

    let row = conflict(&r, "hud_deathnotice_time").expect("Rule 1");
    assert_eq!(values_of(row), [("10", "config"), ("555", "before")]);
    assert_eq!(row.effective.value, "555");
    assert!(row.scheduled, "shown under Scheduled Commands");
    assert!(r.asymmetric.is_empty(), "{:?}", r.asymmetric);
}

#[test]
fn the_latest_before_command_is_the_one_in_effect() {
    // Larger "Before" offsets are further back, so they run first -- and
    // the one nearest the highlight is what the clip records at. The old
    // test credited the 10s one, which is the one that gets overwritten.
    let r = report_scheduled(
        "order",
        vec![
            scheduled("hud_deathnotice_time 1", "Before", 2.0),
            scheduled("hud_deathnotice_time 555", "Before", 10.0),
        ],
    );

    let row = conflict(&r, "hud_deathnotice_time").expect("Rule 1");
    assert_eq!(
        values_of(row),
        [("10", "config"), ("555", "before"), ("1", "before")]
    );
    assert_eq!(row.effective.value, "1");
    assert_eq!(row.effective.offset_seconds, Some(2.0));
}

#[test]
fn an_unpaired_after_that_differs_is_reported_against_the_config_baseline() {
    let r = report_scheduled(
        "unpaired",
        vec![scheduled("hud_deathnotice_time 1", "After", 0.5)],
    );

    assert!(
        conflict(&r, "hud_deathnotice_time").is_none(),
        "{:?}",
        r.conflicts
    );
    assert_eq!(r.asymmetric.len(), 1, "{:?}", r.asymmetric);
    let a = &r.asymmetric[0];
    assert_eq!(a.after.value, "1");
    assert_eq!(a.after.kind, "after");
    assert_eq!(a.baseline.value, "10");
    assert_eq!(a.baseline.file.as_deref(), Some("movie.cfg"));
    assert_eq!(a.baseline.line, Some(3));
}

#[test]
fn an_unpaired_after_matching_the_baseline_is_silent() {
    let r = report_scheduled(
        "unpaired_same",
        vec![scheduled("hud_deathnotice_time 10", "After", 0.5)],
    );
    assert!(r.asymmetric.is_empty(), "{:?}", r.asymmetric);
    assert!(conflict(&r, "hud_deathnotice_time").is_none());
}

#[test]
fn pairing_an_after_moves_the_warning_from_rule_two_to_rule_one() {
    // Any Before pairs the After; this one differs from the baseline, so
    // Rule 1 takes over.
    let r = report_scheduled(
        "paired_differs",
        vec![
            scheduled("hud_deathnotice_time 20", "Before", 1.0),
            scheduled("hud_deathnotice_time 1", "After", 0.5),
        ],
    );
    assert!(r.asymmetric.is_empty(), "{:?}", r.asymmetric);
    assert_eq!(
        conflict(&r, "hud_deathnotice_time")
            .expect("Rule 1")
            .effective
            .value,
        "20"
    );
}

#[test]
fn a_paired_after_with_a_before_at_the_baseline_is_silent_the_accepted_gap() {
    // #216's known, accepted gap: the After still leaves 1 behind, but a
    // Before at the baseline pairs it and neither rule looks again.
    let r = report_scheduled(
        "accepted_gap",
        vec![
            scheduled("hud_deathnotice_time 10", "Before", 1.0),
            scheduled("hud_deathnotice_time 1", "After", 0.5),
        ],
    );
    assert!(r.asymmetric.is_empty(), "{:?}", r.asymmetric);
    assert!(
        conflict(&r, "hud_deathnotice_time").is_none(),
        "{:?}",
        r.conflicts
    );
}

#[test]
fn the_same_value_everywhere_is_silent() {
    // movie.cfg says 10; Initial and Scheduled say 10 too. And
    // mirv_movie_fps 300 in movie.cfg matches the Capture FPS the app
    // appends, so nothing at all is reported.
    let r = report_full(
        "same_everywhere",
        &["hud_deathnotice_time 10", "hud_deathnotice_time \"10\""],
        vec![scheduled("hud_deathnotice_time 10.0", "Before", 2.0)],
        300,
    );
    assert!(r.conflicts.is_empty(), "{:?}", r.conflicts);
    assert!(r.asymmetric.is_empty(), "{:?}", r.asymmetric);
}

#[test]
fn every_pair_of_sources_and_all_three_conflict() {
    // config (10) + Initial
    let r = report_full("combo_ci", &["hud_deathnotice_time 3"], vec![], 300);
    let row = conflict(&r, "hud_deathnotice_time").expect("config + Initial");
    assert_eq!(values_of(row), [("10", "config"), ("3", "initial")]);
    assert!(!row.scheduled, "shown under Initial Commands");

    // config + Scheduled Before
    let r = report_full(
        "combo_cs",
        &[],
        vec![scheduled("hud_deathnotice_time 4", "Before", 2.0)],
        300,
    );
    let row = conflict(&r, "hud_deathnotice_time").expect("config + Scheduled");
    assert_eq!(values_of(row), [("10", "config"), ("4", "before")]);

    // Initial + Scheduled Before, the config agreeing with Initial
    let r = report_full(
        "combo_is",
        &["hud_deathnotice_time 10"],
        vec![scheduled("hud_deathnotice_time 4", "Before", 2.0)],
        300,
    );
    let row = conflict(&r, "hud_deathnotice_time").expect("Initial + Scheduled");
    assert_eq!(
        values_of(row),
        [("10", "config"), ("10", "initial"), ("4", "before")]
    );

    // all three different
    let r = report_full(
        "combo_all",
        &["hud_deathnotice_time 3"],
        vec![scheduled("hud_deathnotice_time 4", "Before", 2.0)],
        300,
    );
    let row = conflict(&r, "hud_deathnotice_time").expect("all three");
    assert_eq!(
        values_of(row),
        [("10", "config"), ("3", "initial"), ("4", "before")]
    );
    assert_eq!(row.effective.value, "4");
}

#[test]
fn every_scheduled_r_decals_is_still_flagged_however_many_there_are() {
    // Deduplication is about which command displaces a config value. Each
    // scheduled r_decals breaks the flush on its own, so they all count.
    let r = report_scheduled(
        "hazards",
        vec![
            scheduled("r_decals 128", "Before", 5.0),
            scheduled("r_decals 4096", "After", 1.0),
        ],
    );

    assert_eq!(
        r.custom.iter().filter(|c| c.kind == "hazard").count(),
        2,
        "{:?}",
        r.custom
    );
}

fn report_full(tag: &str, init: &[&str], custom: Vec<CustomCommand>, fps: i32) -> CfgReport {
    let (_dir, game) = fake_game(tag);
    report_for(
        game,
        init.iter().map(|s| s.to_string()).collect(),
        custom,
        Some(fps),
        Some(true),
        None,
    )
}

fn report(tag: &str, init: &[&str], custom: &[&str], fps: i32) -> CfgReport {
    report_mode(tag, init, custom, fps, None)
}

/// `report` with Seek Between Clips off: a patched batch.
fn report_patched(tag: &str, init: &[&str], custom: &[&str], fps: i32) -> CfgReport {
    report_mode(tag, init, custom, fps, Some(false))
}

fn report_mode(
    tag: &str,
    init: &[&str],
    custom: &[&str],
    fps: i32,
    hook_runs_batch: Option<bool>,
) -> CfgReport {
    let (_dir, game) = fake_game(tag);
    report_for(
        game,
        init.iter().map(|s| s.to_string()).collect(),
        custom.iter().map(|s| scheduled(s, "Before", 2.0)).collect(),
        Some(fps),
        Some(true),
        hook_runs_batch,
    )
}

#[test]
fn a_cvar_only_a_config_sets_is_reported_as_unseen() {
    // r_decals is only ever named in effective_commands when Flush
    // Decals is on — with it off, nothing pins the cvar at all, so a
    // config setting it really is invisible to the pipeline. That is
    // the genuinely silent case this category exists for.
    let (_dir, game) = fake_game("unseen");
    let r = report_for(game, Vec::new(), Vec::new(), Some(120), Some(false), None);

    assert!(
        r.unseen
            .iter()
            .any(|u| u.cvar == "r_decals" && u.value == "0"),
        "{:?}",
        r.unseen
    );
}

#[test]
fn a_config_only_fov_is_not_reported_as_unseen() {
    // Regression: decal_strip::capture_fov_resolved already adopts a
    // config's mirv_fov/default_fov whenever Initial Commands state
    // neither — reporting it as unseen here would tell the user to do
    // something the pipeline is already doing (the same reasoning
    // r_decals already gets when Flush Decals is on).
    let r = report("fov_seen_via_resolve", &[], &[], 120);

    assert!(
        !r.unseen.iter().any(|u| u.cvar == "mirv_fov"),
        "capture_fov_resolved already reads this: {:?}",
        r.unseen
    );
    assert!(
        !r.unseen.iter().any(|u| u.cvar == "r_decals"),
        "the decal pin names r_decals when flush is on, so it is not unseen: {:?}",
        r.unseen
    );
}

#[test]
fn naming_a_cvar_at_the_config_s_own_value_still_counts_as_seeing_it() {
    // Regression: matching on differing values put a cvar the user HAD
    // typed into the "the app cannot see these" list.
    let r = report("agrees", &["mirv_fov 105"], &[], 120);

    assert!(
        !r.unseen.iter().any(|u| u.cvar == "mirv_fov"),
        "it is in Init Commands — it is seen: {:?}",
        r.unseen
    );
}

#[test]
fn a_banned_command_in_initial_commands_is_reported() {
    let r = report("banned_init", &["mirv_recordmovie_start"], &[], 120);

    assert_eq!(r.banned_init.len(), 1, "{:?}", r.banned_init);
    assert_eq!(r.banned_init[0].cvar, "mirv_recordmovie_start");
    assert!(r.banned_scheduled.is_empty());
}

#[test]
fn a_command_too_long_for_a_demo_frame_is_reported_in_its_own_list() {
    let long = format!("echo {}", "x".repeat(59)); // 64 bytes
    let r = report("too_long", &[long.as_str()], &[long.as_str()], 120);

    assert_eq!(r.too_long_init, vec![long.clone()]);
    assert_eq!(r.too_long_scheduled, vec![long]);
}

#[test]
fn r_drawentities_and_cl_lw_in_initial_commands_are_reported_as_banned() {
    let r = report(
        "banned_fatal_init",
        &["r_drawentities 0", "cl_lw 0"],
        &[],
        120,
    );

    let cvars: Vec<&str> = r.banned_init.iter().map(|b| b.cvar.as_str()).collect();
    assert_eq!(
        cvars,
        vec!["r_drawentities", "cl_lw"],
        "{:?}",
        r.banned_init
    );
}

#[test]
fn r_drawentities_and_cl_lw_in_scheduled_commands_are_reported_as_banned() {
    let r = report(
        "banned_fatal_scheduled",
        &[],
        &["r_drawentities 0", "cl_lw 0"],
        120,
    );

    let cvars: Vec<&str> = r.banned_scheduled.iter().map(|b| b.cvar.as_str()).collect();
    assert_eq!(
        cvars,
        vec!["r_drawentities", "cl_lw"],
        "{:?}",
        r.banned_scheduled
    );
}

#[test]
fn a_config_setting_r_drawentities_to_zero_is_reported_as_fatal() {
    let (_dir, game) = fake_game_with_r_drawentities("fatal_config", "0", true);
    let r = report_for(game, vec![], vec![], Some(120), Some(true), None);

    assert_eq!(r.fatal_cvars.len(), 1, "{:?}", r.fatal_cvars);
    assert_eq!(r.fatal_cvars[0].cvar, "r_drawentities");
    assert_eq!(r.fatal_cvars[0].value, "0");
    assert_eq!(r.fatal_cvars[0].required, "1");
    assert_eq!(r.fatal_cvars[0].file, "movie.cfg");
}

#[test]
fn r_drawentities_without_sv_cheats_is_not_reported_as_fatal() {
    // The engine clamps it back to 1.0 on its own, so the line is inert
    // and flagging it would block a capture over nothing. Confirmed live:
    // setting r_drawentities with cheats off does not close the game.
    let (_dir, game) = fake_game_with_r_drawentities("fatal_no_cheats", "0", false);
    let r = report_for(game, vec![], vec![], Some(120), Some(true), None);

    assert!(r.fatal_cvars.is_empty(), "{:?}", r.fatal_cvars);
}

#[test]
fn a_config_setting_r_drawentities_to_one_is_not_reported_as_fatal() {
    let (_dir, game) = fake_game_with_r_drawentities("fatal_config_ok", "1", true);
    let r = report_for(game, vec![], vec![], Some(120), Some(true), None);

    assert!(r.fatal_cvars.is_empty(), "{:?}", r.fatal_cvars);
}

#[test]
fn mirv_movie_filename_in_initial_commands_is_reported_as_a_noop_not_banned() {
    let r = report("noop_init_typed", &["mirv_movie_filename foo"], &[], 120);

    assert!(r.banned_init.is_empty(), "{:?}", r.banned_init);
    assert_eq!(r.noop_init.len(), 1, "{:?}", r.noop_init);
    assert_eq!(r.noop_init[0].cvar, "mirv_movie_filename");
    assert_eq!(r.noop_init[0].source, "Initial Commands");
}

#[test]
fn mirv_movie_filename_a_config_states_is_reported_as_a_noop() {
    // fake_game()'s config.cfg has a bind, not an assignment — this needs
    // a fixture that actually assigns the cvar.
    let (_dir, game) = fake_game_with_mirv_movie_filename("noop_config");
    let r = report_for(game, Vec::new(), Vec::new(), Some(120), Some(false), None);

    assert_eq!(r.noop_init.len(), 1, "{:?}", r.noop_init);
    assert_eq!(r.noop_init[0].cvar, "mirv_movie_filename");
    assert!(
        r.noop_init[0].source.contains("movie.cfg"),
        "{:?}",
        r.noop_init[0]
    );
}

fn filtered(rows: &[FilteredCommandRow]) -> Vec<(&str, &str, &str)> {
    rows.iter()
        .map(|f| (f.command.as_str(), f.rule.as_str(), f.pattern.as_str()))
        .collect()
}

#[test]
fn initial_commands_the_demo_filter_drops_are_reported() {
    let r = report_patched(
        "filtered_init",
        &["exec somefile.cfg", "quit", "sensitivity 3"],
        &[],
        120,
    );

    assert_eq!(
        filtered(&r.filtered_init),
        vec![
            ("exec somefile.cfg", "lineContains", "exec"),
            ("quit", "nameContains", "quit"),
        ]
    );
    assert!(r.noop_init.is_empty(), "{:?}", r.noop_init);
    assert!(r.filtered_scheduled.is_empty());
    assert!(r.filter_applies);
}

#[test]
fn scheduled_commands_the_demo_filter_drops_are_reported() {
    let r = report_patched(
        "filtered_scheduled",
        &[],
        &["mirv_matte_setcolor 255 0 255", "echo ok", "alias a b"],
        120,
    );

    assert_eq!(
        filtered(&r.filtered_scheduled),
        vec![
            ("mirv_matte_setcolor 255 0 255", "nameContains", "_set"),
            ("alias a b", "lineStartsWith", "alias"),
        ]
    );
    assert!(r.filtered_init.is_empty());
}

#[test]
fn a_scheduled_command_the_demo_filter_drops_sets_no_value() {
    // A dropped command never runs, so it cannot conflict with anything.
    let r = report_patched(
        "filtered_no_conflict",
        &["cl_killsound 0", "sensitivity 2"],
        &["cl_killsound 1", "sensitivity 3"],
        120,
    );

    assert_eq!(r.filtered_init.len(), 1, "{:?}", r.filtered_init);
    assert_eq!(r.filtered_scheduled.len(), 1, "{:?}", r.filtered_scheduled);
    // The commands that do run are still compared.
    assert!(
        r.conflicts.iter().any(|c| c.cvar == "sensitivity"),
        "{:?}",
        r.conflicts
    );
    assert!(
        r.conflicts.iter().all(|c| c.cvar != "cl_killsound"),
        "{:?}",
        r.conflicts
    );
}

#[test]
fn a_dropped_scheduled_command_never_conflicts_with_a_config() {
    // The test above drops both sides; here a config states the value,
    // and configs are never filtered, so only the scheduled side's
    // filter keeps this from being a conflict.
    let config = ("dod/config.cfg", "exec movie.cfg\ncl_killsound \"0\"\n");
    let custom = || vec![scheduled("cl_killsound 1", "Before", 2.0)];
    let patched = report_with_files("dropped_vs_config", &[config], &[], custom(), Some(false));
    assert_eq!(
        patched.filtered_scheduled.len(),
        1,
        "{:?}",
        patched.filtered_scheduled
    );
    assert!(
        conflict(&patched, "cl_killsound").is_none(),
        "{:?}",
        patched.conflicts
    );

    // Seek Between Clips: nothing is filtered, so the two do conflict.
    let seek = report_with_files("kept_vs_config", &[config], &[], custom(), Some(true));
    assert!(
        conflict(&seek, "cl_killsound").is_some(),
        "{:?}",
        seek.conflicts
    );
}

#[test]
fn with_seek_between_clips_on_nothing_is_reported_as_dropped() {
    // The hook DLL runs both lists through ClientCmd, past the filter.
    let r = report(
        "filtered_seek",
        &["exec somefile.cfg", "cl_killsound 0"],
        &["cl_killsound 1", "alias a b"],
        120,
    );

    assert!(r.filtered_init.is_empty(), "{:?}", r.filtered_init);
    assert!(
        r.filtered_scheduled.is_empty(),
        "{:?}",
        r.filtered_scheduled
    );
    assert!(!r.filter_applies);
    // So the commands it would have dropped do set values.
    assert!(
        r.conflicts.iter().any(|c| c.cvar == "cl_killsound"),
        "{:?}",
        r.conflicts
    );
}

/// `fake_game`, plus `files` written under the install root (e.g.
/// `dod_addon/wsod.cfg`), with Seek Between Clips as given.
fn report_with_files(
    tag: &str,
    files: &[(&str, &str)],
    init: &[&str],
    custom: Vec<CustomCommand>,
    hook_runs_batch: Option<bool>,
) -> CfgReport {
    let (root, game) = fake_game(tag);
    for (path, text) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    report_for(
        game,
        init.iter().map(|s| s.to_string()).collect(),
        custom,
        Some(300),
        Some(true),
        hook_runs_batch,
    )
}

#[test]
fn an_initial_exec_is_followed_into_its_file_game_folder_first() {
    let r = report_with_files(
        "exec_followed",
        &[
            ("dod/wsod.cfg", "hud_deathnotice_time \"12\"\n"),
            // `exec` opens under GAMECONFIG (dod alone) before GAME, so
            // dod's copy wins and this one never runs.
            ("dod_addon/wsod.cfg", "hud_deathnotice_time \"99\"\n"),
        ],
        &["exec wsod.cfg"],
        vec![scheduled("hud_deathnotice_time 4", "Before", 2.0)],
        None,
    );

    let row = conflict(&r, "hud_deathnotice_time").expect("exec'd file + Scheduled");
    assert_eq!(
        values_of(row),
        [("10", "config"), ("12", "exec"), ("4", "before")]
    );
    assert_eq!(row.values[1].file.as_deref(), Some("wsod.cfg"));
    assert!(r.missing_exec.is_empty(), "{:?}", r.missing_exec);
}

#[test]
fn an_exec_d_file_against_only_configs_is_not_a_conflict() {
    // Both are the user's own files, like a config-only conflict.
    let r = report_with_files(
        "exec_config_only",
        &[("dod_addon/wsod.cfg", "hud_deathnotice_time 12\n")],
        &["exec wsod.cfg"],
        vec![],
        None,
    );
    assert!(
        conflict(&r, "hud_deathnotice_time").is_none(),
        "{:?}",
        r.conflicts
    );
}

#[test]
fn an_initial_exec_of_a_missing_file_is_reported() {
    let r = report_with_files("exec_missing", &[], &["exec nope.cfg"], vec![], None);
    assert_eq!(r.missing_exec, vec!["exec nope.cfg".to_string()]);
}

#[test]
fn a_patched_batch_drops_the_exec_so_its_file_is_not_followed() {
    let r = report_with_files(
        "exec_patched",
        &[("dod_addon/wsod.cfg", "hud_deathnotice_time 12\n")],
        &["exec wsod.cfg"],
        vec![scheduled("hud_deathnotice_time 4", "Before", 2.0)],
        Some(false),
    );
    let row = conflict(&r, "hud_deathnotice_time").expect("config + Scheduled");
    assert_eq!(values_of(row), [("10", "config"), ("4", "before")]);
    assert_eq!(r.filtered_init.len(), 1, "{:?}", r.filtered_init);
    assert!(r.missing_exec.is_empty(), "{:?}", r.missing_exec);
}

#[test]
fn mirv_movie_filename_scheduled_is_banned_not_reported_as_a_noop() {
    let r = report(
        "noop_vs_banned_scheduled",
        &[],
        &["mirv_movie_filename foo"],
        120,
    );

    assert!(
        r.filtered_scheduled.is_empty(),
        "{:?}",
        r.filtered_scheduled
    );
    assert!(
        r.banned_scheduled
            .iter()
            .any(|b| b.cvar == "mirv_movie_filename"),
        "{:?}",
        r.banned_scheduled
    );
}

#[test]
fn a_banned_command_in_scheduled_commands_is_reported() {
    let r = report("banned_scheduled", &[], &["host_framerate 0.05"], 120);

    assert_eq!(r.banned_scheduled.len(), 1, "{:?}", r.banned_scheduled);
    assert_eq!(r.banned_scheduled[0].cvar, "host_framerate");
    assert!(r.banned_init.is_empty());
}

#[test]
fn tier_2_cvars_and_initial_command_decal_cvars_are_never_reported_as_banned() {
    // mirv_movie_fps is redundant-with-a-setting, not dangerous, in either
    // list. r_decals/mirv_fov/gl_widescreenfov are exactly how the decal
    // flush is meant to be configured when set once as Initial Commands —
    // only scheduling one of them is refused (see the test below).
    let r = report(
        "not_banned",
        &[
            "mirv_movie_fps 500",
            "r_decals \"256\"",
            "mirv_fov 90",
            "gl_widescreenfov 1",
        ],
        &["mirv_movie_fps 500"],
        120,
    );

    assert!(r.banned_init.is_empty(), "{:?}", r.banned_init);
    assert!(r.banned_scheduled.is_empty(), "{:?}", r.banned_scheduled);
}

#[test]
fn scheduling_a_decal_flush_cvar_is_reported_as_banned() {
    // Fine in Initial Commands (previous test); refused outright once
    // scheduled instead — see cfg_scan::SCHEDULED_BANNED_COMMANDS.
    let r = report(
        "scheduled_decal_cvars_banned",
        &[],
        &["r_decals 512", "mirv_fov 105", "gl_widescreenfov 1"],
        120,
    );

    let flagged: Vec<&str> = r.banned_scheduled.iter().map(|b| b.cvar.as_str()).collect();
    assert_eq!(
        flagged,
        vec!["r_decals", "mirv_fov", "gl_widescreenfov"],
        "{:?}",
        r.banned_scheduled
    );
    assert!(r.banned_init.is_empty());
}

#[test]
fn the_default_ring_is_reported_when_nothing_states_r_decals() {
    let (_dir, game) = fake_game_without_r_decals("decal_default_unset");
    let r = report_for(game, Vec::new(), Vec::new(), Some(120), Some(true), None);
    assert_eq!(
        r.decal_default_ring,
        Some(crate::patch::PatcherConfig::default().decal_ring_limit)
    );
}

#[test]
fn a_config_stating_r_decals_zero_is_reported_as_a_flush_noop_not_the_default() {
    // Regression, in two stages. First: this used to fire even though
    // movie.cfg names r_decals (fake_game's movie.cfg always does) — "no
    // r_decals value is set anywhere" was simply false whenever a config
    // states one. Second: once that was fixed to defer to an override
    // row instead, r_decals stopped being silently pinned to the app's
    // default at all (see cfg_scan / ring_limit's doc comments — a config
    // now gets the same standing Initial Commands do, same as mirv_fov
    // already had) — so there is no override to defer to either, and the
    // config's own 0 flows straight through as the noop it actually is.
    let r = report("decal_default_config_states_it", &[], &[], 120);
    assert_eq!(r.decal_default_ring, None, "{:?}", r.decal_default_ring);
    assert!(r.decal_flush_is_noop, "{:?}", r);
    assert!(
        conflict(&r, "r_decals").is_none(),
        "nothing overrides it anymore — the config's own value now stands: {:?}",
        r.conflicts
    );
}

#[test]
fn the_default_ring_is_not_reported_once_the_user_states_one() {
    let r = report("decal_default_stated", &["r_decals \"512\""], &[], 120);
    assert_eq!(r.decal_default_ring, None);
}

#[test]
fn an_explicit_zero_r_decals_is_reported_as_a_flush_noop() {
    // A stated 0 is respected (same as any other stated value) — the
    // pipeline's pin only ever fires when nothing states one at all — so
    // the flush genuinely runs against an empty ring the whole demo.
    let r = report("decal_noop_stated_zero", &["r_decals \"0\""], &[], 120);
    assert!(r.decal_flush_is_noop, "{:?}", r);
    // Not the same fact as "no value is set anywhere" — a value IS set,
    // it is just zero.
    assert_eq!(r.decal_default_ring, None);
}

#[test]
fn a_nonzero_effective_r_decals_is_not_reported_as_a_flush_noop() {
    // fake_game()'s movie.cfg states r_decals 0 (needed for the test
    // above) and is now genuinely a noop case, so this one needs a
    // fixture that states nothing at all — falling through to the app's
    // nonzero default.
    let (_dir, game) = fake_game_without_r_decals("decal_noop_nonzero");
    let r = report_for(game, Vec::new(), Vec::new(), Some(120), Some(true), None);
    assert!(!r.decal_flush_is_noop, "{:?}", r);
}

#[test]
fn a_nonzero_r_decals_a_config_states_is_not_reported_as_a_flush_noop() {
    let (_dir, game) = fake_game_with_r_decals("decal_noop_config_nonzero", "512");
    let r = report_for(game, Vec::new(), Vec::new(), Some(120), Some(true), None);
    assert!(!r.decal_flush_is_noop, "{:?}", r);
    assert!(conflict(&r, "r_decals").is_none(), "{:?}", r.conflicts);
}

#[test]
fn a_zero_r_decals_is_not_reported_as_a_flush_noop_when_flush_is_off() {
    let (_dir, game) = fake_game("decal_noop_flush_off");
    let r = report_for(
        game,
        vec!["r_decals \"0\"".to_string()],
        Vec::new(),
        Some(120),
        Some(false),
        None,
    );
    assert!(!r.decal_flush_is_noop, "{:?}", r);
}

#[test]
fn the_default_ring_is_not_reported_when_flush_is_off() {
    let (_dir, game) = fake_game("decal_default_flush_off");
    let r = report_for(game, Vec::new(), Vec::new(), Some(120), Some(false), None);
    assert_eq!(r.decal_default_ring, None);
}

#[test]
fn a_quoted_r_decals_the_user_typed_is_the_one_in_effect() {
    // Regression: real .cfg syntax quotes every value, and the app used
    // to parse r_decals from Initial Commands without unquoting first --
    // `r_decals "512"` silently read as "nothing stated" and the app
    // appended its own default afterward, overruling the user's own line.
    // It conflicts with movie.cfg's 0, and it is what applies.
    let r = report("quoted_decals", &["r_decals \"512\""], &[], 120);

    let row = conflict(&r, "r_decals").expect("512 vs movie.cfg's 0");
    assert_eq!(values_of(row), [("0", "config"), ("512", "initial")]);
    assert_eq!(row.effective.kind, "initial", "no app value overrules it");
}

#[test]
fn one_row_per_cvar_even_when_two_commands_set_it() {
    // Regression: typing the value the app also appends produced two rows
    // saying the identical thing, which reads as a bug rather than as two
    // facts.
    let r = report("dupe", &["mirv_movie_fps 120"], &[], 120);

    let rows: Vec<_> = r
        .conflicts
        .iter()
        .filter(|c| c.cvar.eq_ignore_ascii_case("mirv_movie_fps"))
        .collect();
    assert_eq!(rows.len(), 1, "{:?}", r.conflicts);
    assert_eq!(
        values_of(rows[0]),
        [("300", "config"), ("120", "initial"), ("120", "app")]
    );
}

#[test]
fn a_typed_command_the_app_overrides_names_the_app_as_the_effective_value() {
    // The screenshot case: mirv_movie_fps 500 typed by hand, Capture FPS at
    // 120 appended after it. The typed value never applies.
    let r = report("shadow", &["mirv_movie_fps 500"], &[], 120);

    let row = conflict(&r, "mirv_movie_fps").expect("reported");
    assert_eq!(
        values_of(row),
        [("300", "config"), ("500", "initial"), ("120", "app")]
    );
    assert_eq!(row.effective.value, "120");
    assert_eq!(row.effective.kind, "app", "the app appended the winner");
}

#[test]
fn an_app_value_overriding_a_config_is_reported_with_nothing_typed() {
    // What FROM_APP_NOTE used to cover: movie.cfg's mirv_movie_fps 300
    // quietly replaced by Capture FPS.
    let r = report("app_only", &[], &[], 120);

    let row = conflict(&r, "mirv_movie_fps").expect("reported");
    assert_eq!(values_of(row), [("300", "config"), ("120", "app")]);
}

#[test]
fn a_scheduled_r_decals_is_reported_as_breaking_the_flush() {
    let r = report("hazard", &[], &["r_decals 128"], 120);

    let hazards: Vec<_> = r.custom.iter().filter(|c| c.kind == "hazard").collect();
    assert_eq!(hazards.len(), 1, "{:?}", r.custom);
    assert_eq!(hazards[0].cvar, "r_decals");
    assert!(
        conflict(&r, "r_decals").is_none(),
        "the hazard must not also be listed as a conflict: {:?}",
        r.conflicts
    );
}

#[test]
fn a_scheduled_command_conflicts_with_whatever_it_displaces() {
    // Scheduled commands run last of all, so they beat the init commands as
    // well as the configs. Not mirv_fov/r_decals/gl_widescreenfov -- those
    // are hazards regardless of what they'd otherwise displace, and are
    // covered by their own tests.
    let r = report("custom", &["sensitivity 3"], &["sensitivity 5"], 120);

    let row = conflict(&r, "sensitivity").expect("reported");
    assert_eq!(values_of(row), [("3", "initial"), ("5", "before")]);
    assert_eq!(row.effective.value, "5");
}

#[test]
fn a_writable_config_cfg_is_reported_and_a_read_only_one_is_not() {
    let dir = Scratch::new("cfg_readonly");
    assert!(!config_cfg_is_writable(dir.path()), "no config.cfg at all");

    let cfg = dir.path().join("config.cfg");
    std::fs::write(&cfg, "sensitivity 2\n").unwrap();
    assert!(config_cfg_is_writable(dir.path()));

    let mut perms = std::fs::metadata(&cfg).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&cfg, perms.clone()).unwrap();
    assert!(!config_cfg_is_writable(dir.path()));

    // Put it back so the scratch folder can be removed.
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(&cfg, perms).unwrap();
}

#[test]
fn a_bound_key_in_a_config_is_never_treated_as_a_setting() {
    // config.cfg here binds F7 to `r_decals 4000`. Nothing happens until
    // someone presses F7, and warning about it would be noise.
    let r = report("bind", &[], &[], 120);

    assert!(
        !r.conflicts
            .iter()
            .flat_map(|c| &c.values)
            .any(|v| v.value == "4000"),
        "{:?}",
        r.conflicts
    );
}

// ── Row order and wording, list by list ──────────────────────────────────
//
// Pinned before `cfg_report` was split into stages (#1177), so each stage
// is held to the rows it produced as one function.

#[test]
fn unseen_rows_name_each_config_setting_with_its_file_and_line() {
    // Flush off: nothing pins r_decals. mirv_fov is read from the config
    // directly, mirv_movie_fps is pinned by the app, and the pipeline
    // never reads hud_deathnotice_time, so it is not "unseen" either.
    let (_dir, game) = fake_game("unseen_rows");
    let r = report_for(game, Vec::new(), Vec::new(), Some(120), Some(false), None);

    let rows: Vec<(&str, &str, &str, usize)> = r
        .unseen
        .iter()
        .map(|u| (u.cvar.as_str(), u.value.as_str(), u.file.as_str(), u.line))
        .collect();
    assert_eq!(rows, [("r_decals", "0", "movie.cfg", 1)]);
}

#[test]
fn an_initial_command_the_demo_filter_drops_names_nothing() {
    // `kill` anywhere on the line makes the engine drop it, so its
    // r_decals never lands and the config's value is still unseen.
    let (_dir, game) = fake_game("unseen_filtered");
    let init = vec!["r_decals 500 ; kill".to_string()];
    let patched = report_for(
        game.clone(),
        init.clone(),
        Vec::new(),
        Some(120),
        Some(false),
        Some(false),
    );
    assert_eq!(
        patched
            .unseen
            .iter()
            .map(|u| u.cvar.as_str())
            .collect::<Vec<_>>(),
        ["r_decals"]
    );
    // With Seek Between Clips on, nothing is filtered and it counts.
    let hooked = report_for(game, init, Vec::new(), Some(120), Some(false), Some(true));
    assert!(hooked.unseen.is_empty());
}

#[test]
fn unseen_rows_follow_the_watched_cvars_order_not_the_configs() {
    // An Initial Command stating the fov makes a config's other fov cvar
    // unseen. The config states default_fov first; the rows still list
    // r_decals first (`cfg_scan::WATCHED_CVARS`). Flush off, so nothing
    // pins r_decals.
    let root = Scratch::new("cfgrep_unseen_order_off");
    let dod = root.join("dod");
    std::fs::create_dir_all(&dod).unwrap();
    std::fs::write(dod.join("config.cfg"), "exec movie.cfg\n").unwrap();
    std::fs::write(
        dod.join("movie.cfg"),
        "default_fov \"90\"\nr_decals \"0\"\n",
    )
    .unwrap();
    let exe = root.join("hl.exe");
    std::fs::write(&exe, b"").unwrap();
    let r = report_for(
        exe.to_string_lossy().to_string(),
        vec!["mirv_fov 100".to_string()],
        Vec::new(),
        Some(120),
        Some(false),
        None,
    );

    let rows: Vec<(&str, &str, usize)> = r
        .unseen
        .iter()
        .map(|u| (u.cvar.as_str(), u.value.as_str(), u.line))
        .collect();
    assert_eq!(rows, [("r_decals", "0", 2), ("default_fov", "90", 1)]);
}

#[test]
fn noop_rows_list_initial_commands_before_the_configs() {
    let (_dir, game) = fake_game_with_mirv_movie_filename("noop_order");
    let r = report_for(
        game,
        vec!["mirv_movie_filename take".to_string()],
        Vec::new(),
        Some(120),
        Some(true),
        None,
    );

    let rows: Vec<(&str, &str, &str)> = r
        .noop_init
        .iter()
        .map(|n| (n.cvar.as_str(), n.command.as_str(), n.source.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            (
                "mirv_movie_filename",
                "mirv_movie_filename take",
                "Initial Commands"
            ),
            (
                "mirv_movie_filename",
                "mirv_movie_filename clip",
                "movie.cfg, line 1"
            ),
        ]
    );
}

#[test]
fn banned_scheduled_lists_banned_commands_before_the_decal_flush_ones() {
    let r = report(
        "banned_order",
        &[],
        &["r_decals 128", "host_framerate 0.05"],
        120,
    );

    let rows: Vec<(&str, &str)> = r
        .banned_scheduled
        .iter()
        .map(|b| (b.cvar.as_str(), b.command.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            ("host_framerate", "host_framerate 0.05"),
            ("r_decals", "r_decals 128"),
        ]
    );
}

#[test]
fn a_banned_scheduled_command_sets_no_value_in_the_value_rules() {
    // Initial 1 against Scheduled 2 would be a conflict, but the
    // scheduled one is refused and has its own, louder row.
    let r = report("banned_no_value", &["dem_speed 1"], &["dem_speed 2"], 120);

    assert!(
        r.banned_scheduled.iter().any(|b| b.cvar == "dem_speed"),
        "{:?}",
        r.banned_scheduled
    );
    assert!(conflict(&r, "dem_speed").is_none(), "{:?}", r.conflicts);
}

#[test]
fn a_hazard_row_names_the_command_and_nothing_it_displaces() {
    let r = report("hazard_row", &[], &["gl_widescreenfov 1"], 120);

    let rows: Vec<(&str, &str, &str, &str, &str)> = r
        .custom
        .iter()
        .map(|c| {
            (
                c.command.as_str(),
                c.cvar.as_str(),
                c.kind.as_str(),
                c.replaced_value.as_str(),
                c.source.as_str(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [("gl_widescreenfov 1", "gl_widescreenfov", "hazard", "", "")]
    );
}

#[test]
fn renamed_names_list_the_configs_then_initial_then_scheduled_commands() {
    let r = report_with_files(
        "renamed_order",
        &[(
            "dod/config.cfg",
            "exec movie.cfg\ndodstudio_hide_scoreboard \"1\"\n",
        )],
        &["dodstudio_hide_scoreboard 0"],
        vec![scheduled("dodstudio_hide_scoreboard 1", "Before", 2.0)],
        None,
    );

    let rows: Vec<(&str, &str, &str)> = r
        .renamed
        .iter()
        .map(|n| (n.old.as_str(), n.new.as_str(), n.source.as_str()))
        .collect();
    let (old, new) = ("dodstudio_hide_scoreboard", "ds_hud_hide_scoreboard");
    assert_eq!(
        rows,
        [
            (old, new, "config.cfg, line 2"),
            (old, new, "Initial Commands"),
            (old, new, "Scheduled Commands"),
        ]
    );
}
