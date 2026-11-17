use super::PatcherConfig;

/// The `-cmdLine` string `build_hlae_process` hands to HLAE.
///
/// Read back off the built `Command` rather than reconstructed, so the test
/// fails if the flag stops reaching the argument that actually carries it.
fn cmd_line_of(cfg: &PatcherConfig, extra: &str) -> String {
    let cmd = cfg.build_hlae_process(extra);
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let at = args
        .iter()
        .position(|a| a == "-cmdLine")
        .expect("build_hlae_process must pass -cmdLine");
    args[at + 1].clone()
}

/// A config whose `hl.exe` sits beside a `hw.dll` holding `hw_dll`, at
/// `width`x`height`.
fn config_in(
    dir: &crate::test_support::Scratch,
    hw_dll: &[u8],
    width: i32,
    height: i32,
) -> PatcherConfig {
    std::fs::write(dir.join("hw.dll"), hw_dll).unwrap();
    PatcherConfig {
        game_path: dir.join("hl.exe").display().to_string(),
        resolution_width: width,
        resolution_height: height,
        ..Default::default()
    }
}

#[test]
fn a_pre_anniversary_window_goes_borderless_only_when_it_would_not_fit_the_screen() {
    let dir = crate::test_support::Scratch::new("launch_borderless_pre");
    let screen = crate::sys::screen::primary_screen_size();
    for (width, height) in [(1280, 720), (2560, 1440), (7680, 4320)] {
        let line = cmd_line_of(&config_in(&dir, b"MAX_PACKET_ENTITIES", width, height), "");
        assert!(line.contains("-windowed"), "{line}");
        assert_eq!(
            line.contains("-noborder"),
            crate::sys::screen::needs_borderless_window((width, height), screen),
            "{width}x{height} on {screen:?}: {line}"
        );
    }
}

#[test]
fn an_anniversary_window_keeps_its_title_bar_at_any_size() {
    let dir = crate::test_support::Scratch::new("launch_borderless_post");
    for (width, height) in [(1280, 720), (2560, 1440), (7680, 4320)] {
        let cfg = config_in(&dir, b"MAX_PACKET_ENTITIES sv_allow_shaders", width, height);
        let line = cmd_line_of(&cfg, "");
        assert!(
            line.contains(&format!("-windowed -w {width} -h {height}")),
            "{line}"
        );
        assert!(!line.contains("-noborder"), "{line}");
    }
}

#[test]
fn test_alpha_flags_are_sent_on_every_launch_shape() {
    // The three call sites, by their `extra_engine_args`: a capture batch,
    // Launch Preview, and the standalone "Launch Game (HLAE)" button. The
    // last two used to get no alpha flags at all, so a hand-driven session
    // that enabled separate HUD in the console wrote an all-white
    // `hudalpha` — an opaque matte, silently unusable. Pin all three.
    for extra in [
        "-condebug +exec dodstudio_helper.cfg +playdemo dodstudio_primer",
        "+viewdemo stem",
        "",
    ] {
        let line = cmd_line_of(&PatcherConfig::default(), extra);
        assert!(
            line.contains("-afxForceAlpha8 1"),
            "alpha flag missing for extra={extra:?}: {line}"
        );
        // The flags #69's bisect showed change nothing stay off.
        for unneeded in ["-gl", "-32bpp", "-afxRenderMode"] {
            assert!(
                !line.split(' ').any(|token| token == unneeded),
                "{unneeded} is sent again for extra={extra:?}: {line}"
            );
        }
    }
}

#[test]
fn test_condebug_is_passed_on_every_launch() {
    // The Studio reads the engine's console log for per-demo progress,
    // fast-forward-to-clip progress, crash detection and OBS markers, and
    // none of those fail loudly without it — they silently never fire. It
    // used to be a checkbox; there is deliberately no longer any way to
    // launch without it. See #226.
    let cfg = PatcherConfig::default();
    for extra in [
        "",
        "+viewdemo foo",
        "+exec dodstudio_helper.cfg +playdemo dodstudio_primer",
    ] {
        let line = cmd_line_of(&cfg, extra);
        assert!(
            line.contains("-condebug"),
            "launch args must carry -condebug, got: {line}"
        );
    }
}

#[test]
fn test_addons_is_passed_on_every_launch() {
    // DoD Studio's UI files live in dod_addon, which the engine only
    // searches with -addons; without it they silently don't load.
    for extra in ["", "+viewdemo foo", "+playdemo dodstudio_primer"] {
        let line = cmd_line_of(&PatcherConfig::default(), extra);
        let addons = line.find("-addons").expect("-addons present");
        assert!(
            line.find('+').is_none_or(|plus| addons < plus),
            "-addons must precede any +command: {line}"
        );
    }
}

#[test]
fn test_demoedit_is_passed_on_every_launch() {
    // Without it the demo player's Master/Events/Save row is hidden on any
    // install lacking a custom DemoPlayerDialog.res, which is how it went
    // missing unnoticed once before.
    for extra in ["", "+viewdemo foo", "+playdemo dodstudio_primer"] {
        let line = cmd_line_of(&PatcherConfig::default(), extra);
        let demoedit = line.find("-demoedit").expect("-demoedit present");
        assert!(
            line.find('+').is_none_or(|plus| demoedit < plus),
            "-demoedit must precede any +command: {line}"
        );
    }
}

#[test]
fn test_engine_flags_precede_console_commands() {
    // GoldSrc parses `-` switches off the command line and queues `+`
    // commands after; an alpha flag landing after a `+` would be read as
    // an argument to that command instead of a switch.
    let line = cmd_line_of(&PatcherConfig::default(), "+playdemo dodstudio_primer");
    let first_plus = line.find('+').expect("the console command is present");
    let alpha = line
        .find("-afxForceAlpha8")
        .expect("the alpha flag is present");
    assert!(
        alpha < first_plus,
        "alpha flag must precede any +command: {line}"
    );
}

#[test]
fn test_condebug_precedes_the_callers_own_args() {
    // `+exec`/`+playdemo` are commands the engine runs at startup, so a
    // switch placed after them is parsed as their argument rather than as a
    // switch. Keeping -condebug ahead of `extra_engine_args` is what stops
    // that, and nothing else in the process would report it.
    let line = cmd_line_of(&PatcherConfig::default(), "+playdemo dodstudio_primer");
    let condebug = line.find("-condebug").expect("-condebug present");
    let playdemo = line.find("+playdemo").expect("+playdemo present");
    assert!(
        condebug < playdemo,
        "-condebug must come before the caller's own args, got: {line}"
    );
}

#[test]
fn every_launch_names_this_studios_pipe_ahead_of_any_command() {
    for extra in ["", "+viewdemo foo", "+playdemo dodstudio_primer"] {
        let line = cmd_line_of(&PatcherConfig::default(), extra);
        let at = line
            .find(&crate::sys::studio_pipe::launch_arg())
            .unwrap_or_else(|| panic!("no -dodstudio_pipe in {line}"));
        assert!(
            line.find('+').is_none_or(|plus| at < plus),
            "-dodstudio_pipe must precede any +command: {line}"
        );
    }
}
