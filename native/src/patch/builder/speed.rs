// patch/builder/speed.rs
// The helper cfg's speed aliases: `sys_fast_forward`, which the batch runs
// between clips, and `sys_normal_speed`, which the pre-roll runs before each
// one. Both routes run them — the patched demo as ConsoleCommand frames, the
// hook's batch (`runtime_batch.rs`) for the settle stretch after its seek.
// Beside them, `sys_ramp_on` / `sys_ramp_off`, which speed up a gap between
// kills inside a clip (#831), on the hook's batch only.

use crate::patch::cfg_scan::{self, CfgScan};
use crate::patch::types::PatcherConfig;

/// What `sys_fast_forward` sets each of [`FAST_FORWARD_CVARS`] to (#724). With
/// the engine's frame cap in force a `host_framerate` fast-forward advances
/// one demo step per rendered frame, so the cap (`fps_max`, ~100 without
/// `fps_override`, and vsync at the monitor's rate) is what bounds it: 11.9x
/// real time at 238 fps, measured on both builds. Lifting the cap alone gave
/// 16-37x; with `r_norefresh 1` on top (the 3D pass skipped, HUD and swap
/// kept) 32-75x. `gl_vsync` exists only on the pre-Anniversary build; the
/// Anniversary engine prints "Unknown command" and goes on, so it is sent
/// to both rather than deciding the build here.
const FAST_FORWARD_VALUES: &[(&str, &str)] = &[
    ("fps_override", "1"),
    ("fps_max", "1000"),
    ("gl_vsync", "0"),
];

/// What the engine starts each of [`FAST_FORWARD_CVARS`] at when nothing sets
/// it — read from both `hw.dll`s' cvar tables (`fps_max` "100.0",
/// `fps_override` "0", `gl_vsync` "1"). `sys_normal_speed` falls back to these
/// only when neither Initial Commands nor an executed config states a value,
/// which for the two archived ones (`fps_max`, `gl_vsync`) means a missing
/// `config.cfg`.
const ENGINE_DEFAULTS: &[(&str, &str)] =
    &[("fps_override", "0"), ("fps_max", "100"), ("gl_vsync", "1")];

/// The `host_framerate` fast-forward runs at: Configuration's FF Speed
/// (`fast_forward_speed`), or the 0.05 default when that is not a positive
/// number. `host_framerate 0` is real time, so 0 cannot mean "fast".
pub(crate) fn fast_forward_host_framerate(config: &PatcherConfig) -> f32 {
    let v = config.fast_forward_speed;
    if v.is_finite() && v > 0.0 { v } else { 0.05 }
}

/// `sys_fast_forward`'s body: the speed, then the cap lifted, then drawing
/// off. `r_norefresh` last, so a frame is never skipped at the capped rate.
pub fn fast_forward_alias_body(config: &PatcherConfig) -> String {
    let mut body = format!("host_framerate {}", fast_forward_host_framerate(config));
    for (cvar, value) in FAST_FORWARD_VALUES {
        body.push_str(&format!("; {cvar} {value}"));
    }
    body.push_str("; r_norefresh 1");
    body
}

/// `sys_normal_speed`'s body: drawing back on and real time first, then the
/// cap put back to what the user has. The alias runs at the pre-roll, 2-4 s
/// of real time before `mirv_recordmovie_start`, so the first drawn frames
/// and the vsync change land in the lead the capture trims.
///
/// `init_commands` is `final_init_commands`' list, not the user's alone: OBS
/// mode pins `fps_override 1; fps_max <obs fps>` there, and that pin is what
/// every clip has to get back.
pub fn normal_speed_alias_body(init_commands: &[String], scan: Option<&CfgScan>) -> String {
    let mut body = String::from("sys_autodir; clear; r_norefresh 0; host_framerate 0");
    for (cvar, default) in ENGINE_DEFAULTS {
        body.push_str(&format!(
            "; {cvar} {}",
            restored_value(cvar, default, init_commands, scan)
        ));
    }
    body
}

/// The value `sys_normal_speed` puts back for `cvar`: Initial Commands first,
/// then the config chain the engine executes, then the engine default — the
/// precedence `hd::gl_max_size::effective` and `decal_strip::ring_limit` use,
/// since it is the order the engine itself applied them in at demo load.
///
/// Only a finite number is taken: all three are numeric cvars, and a stray
/// quote or semicolon in a config's value would otherwise end the alias early.
fn restored_value(
    cvar: &str,
    default: &str,
    init_commands: &[String],
    scan: Option<&CfgScan>,
) -> String {
    let numeric = |v: String| v.parse::<f32>().is_ok_and(f32::is_finite).then_some(v);
    cfg_scan::effective_in(init_commands, cvar)
        .and_then(numeric)
        .or_else(|| {
            scan.and_then(|s| s.effective(cvar))
                .map(|s| s.value.clone())
                .and_then(numeric)
        })
        .unwrap_or_else(|| default.to_string())
}

/// The helper cfg's `sys_ramp_on` / `sys_ramp_off`, which play a long gap
/// between two kills inside a clip sped up (#831, `kill_gaps.rs`).
/// `dem_speed` scales the demo player's clock; HLAE goes on writing one frame
/// per `1/fps` of engine time, so the take holds the gap at exactly that
/// speed, and its sound stays in step (measured on both builds, #831).
/// Defined in every batch, so the names always resolve; only a batch with
/// Fast-forward gaps schedules them. With gaps kept the speed is the
/// setting's default, never run.
pub fn ramp_alias_lines(config: &PatcherConfig) -> String {
    let speed = config
        .kill_gaps
        .ramp_speed()
        .unwrap_or(crate::patch::types::DEFAULT_KILL_GAP_SPEED);
    format!(
        "alias {} \"dem_speed {speed}\"\nalias {} \"dem_speed 1\"\n",
        super::kill_gaps::RAMP_ON,
        super::kill_gaps::RAMP_OFF
    )
}

/// The two alias lines the helper cfg carries.
pub fn speed_alias_lines(
    config: &PatcherConfig,
    init_commands: &[String],
    scan: Option<&CfgScan>,
) -> String {
    format!(
        "alias sys_normal_speed \"{}\"\nalias sys_fast_forward \"{}\"\n",
        normal_speed_alias_body(init_commands, scan),
        fast_forward_alias_body(config)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::cfg_scan::{CvarSetting, FAST_FORWARD_CVARS};

    fn scan_with(settings: &[(&str, &str)]) -> CfgScan {
        CfgScan {
            settings: settings
                .iter()
                .map(|(cvar, value)| CvarSetting {
                    cvar: cvar.to_string(),
                    value: value.to_string(),
                    file: "config.cfg".into(),
                    line: 1,
                    auto_executed: true,
                })
                .collect(),
            ..CfgScan::default()
        }
    }

    #[test]
    fn the_value_tables_cover_exactly_the_tiered_cvars() {
        // The tier list in cfg_scan is what the Commands tab warns about; the
        // alias must lift and restore the same set, no more and no less.
        let lifted: Vec<&str> = FAST_FORWARD_VALUES.iter().map(|(c, _)| *c).collect();
        let restored: Vec<&str> = ENGINE_DEFAULTS.iter().map(|(c, _)| *c).collect();
        assert_eq!(lifted, FAST_FORWARD_CVARS);
        assert_eq!(restored, FAST_FORWARD_CVARS);
    }

    #[test]
    fn fast_forward_lifts_the_cap_and_stops_drawing() {
        let mut config = PatcherConfig::default();
        config.fast_forward_speed = 0.05;
        assert_eq!(
            fast_forward_alias_body(&config),
            "host_framerate 0.05; fps_override 1; fps_max 1000; gl_vsync 0; r_norefresh 1"
        );
    }

    #[test]
    fn fast_forward_uses_ff_speed_and_falls_back_when_it_is_not_positive() {
        let mut config = PatcherConfig::default();
        config.fast_forward_speed = 0.1;
        assert_eq!(fast_forward_host_framerate(&config), 0.1);
        for bad in [0.0, -1.0, f32::NAN] {
            config.fast_forward_speed = bad;
            assert_eq!(fast_forward_host_framerate(&config), 0.05, "{bad}");
        }
    }

    #[test]
    fn normal_speed_with_nothing_stated_puts_back_the_engine_defaults() {
        assert_eq!(
            normal_speed_alias_body(&[], None),
            "sys_autodir; clear; r_norefresh 0; host_framerate 0; fps_override 0; fps_max 100; gl_vsync 1"
        );
        assert_eq!(
            normal_speed_alias_body(&[], Some(&CfgScan::default())),
            normal_speed_alias_body(&[], None)
        );
    }

    #[test]
    fn normal_speed_puts_back_the_configs_values() {
        let scan = scan_with(&[("fps_max", "250"), ("gl_vsync", "0")]);
        assert_eq!(
            normal_speed_alias_body(&[], Some(&scan)),
            "sys_autodir; clear; r_norefresh 0; host_framerate 0; fps_override 0; fps_max 250; gl_vsync 0"
        );
    }

    #[test]
    fn initial_commands_outrank_the_config() {
        // The engine ran the config first and the Initial Commands after, so
        // the Initial value is the one playback had before the fast-forward.
        let scan = scan_with(&[("fps_max", "250"), ("fps_override", "0")]);
        let init = vec!["fps_override 1".to_string(), "fps_max \"300\"".to_string()];
        assert_eq!(
            normal_speed_alias_body(&init, Some(&scan)),
            "sys_autodir; clear; r_norefresh 0; host_framerate 0; fps_override 1; fps_max 300; gl_vsync 1"
        );
    }

    #[test]
    fn a_config_nobody_execs_does_not_count() {
        let mut scan = scan_with(&[("fps_max", "250")]);
        scan.settings[0].auto_executed = false;
        assert!(normal_speed_alias_body(&[], Some(&scan)).contains("fps_max 100"));
    }

    #[test]
    fn a_value_that_is_not_a_number_falls_back_rather_than_break_the_alias() {
        let scan = scan_with(&[("fps_max", "lots\"; quit")]);
        let init = vec!["gl_vsync on".to_string()];
        let body = normal_speed_alias_body(&init, Some(&scan));
        assert!(body.contains("fps_max 100"), "{body}");
        assert!(body.contains("gl_vsync 1"), "{body}");
        assert!(!body.contains('"'), "{body}");
    }

    #[test]
    fn the_ramp_aliases_set_the_demo_speed_and_put_it_back() {
        let mut config = PatcherConfig::default();
        config.kill_gaps.mode = crate::patch::types::KillGapMode::FastForward;
        config.kill_gaps.speed = 6.0;
        assert_eq!(
            ramp_alias_lines(&config),
            "alias sys_ramp_on \"dem_speed 6\"\nalias sys_ramp_off \"dem_speed 1\"\n"
        );
        // Kept gaps: still defined, at the default speed.
        config.kill_gaps.mode = crate::patch::types::KillGapMode::Keep;
        assert!(ramp_alias_lines(&config).contains("\"dem_speed 4\""));
    }

    #[test]
    fn the_alias_lines_are_the_helper_cfgs_two() {
        let mut config = PatcherConfig::default();
        config.fast_forward_speed = 0.1;
        assert_eq!(
            speed_alias_lines(&config, &[], None),
            "alias sys_normal_speed \"sys_autodir; clear; r_norefresh 0; host_framerate 0; fps_override 0; fps_max 100; gl_vsync 1\"\n\
             alias sys_fast_forward \"host_framerate 0.1; fps_override 1; fps_max 1000; gl_vsync 0; r_norefresh 1\"\n"
        );
    }
}
