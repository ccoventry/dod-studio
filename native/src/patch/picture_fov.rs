// patch/picture_fov.rs
// The project's own field of view (Project Settings › Picture, #1085): the
// line a batch sends for it, and what the project's Initial Commands and the
// game's configs set it to, for the frontend.
//
// A project always has a field of view. One made before it had the setting
// takes, on first open, the value its own lines give (a `mirv_fov <n>`, or
// one in a cfg those lines `exec`), so its look doesn't change; with none,
// the game's 90. The batch then sends it after every line
// (`builder::user_init_commands`), so a `mirv_fov` in a cfg no longer has the
// last word, and Studio warns where one sets a different value.
//
// Reads config files, never writes one (`cfg_scan`'s rule).

use std::path::{Path, PathBuf};

use super::{PatcherConfig, cfg_scan};

/// The cvar the project's field of view is sent as.
pub const FOV_CVAR: &str = "mirv_fov";

/// The game's own field of view, a project's when nothing else says one.
pub const DEFAULT_FOV: f32 = 90.0;

/// The range Studio accepts, as the Picture row does (1 to 179).
const MIN_FOV: f32 = 1.0;
const MAX_FOV: f32 = 179.0;

/// Whether `fov` is a field of view a capture can use.
pub fn is_usable(fov: f32) -> bool {
    fov.is_finite() && (MIN_FOV..=MAX_FOV).contains(&fov)
}

/// The command that sets the field of view to `fov`: `mirv_fov 90`, with
/// no `.0` on a whole number.
pub fn fov_line(fov: f32) -> String {
    format!("{FOV_CVAR} {fov}")
}

/// The user's Initial Commands as a batch sends them: with the project's
/// field of view, every `mirv_fov <n>` line left out
/// (`cfg_scan::project_owned_commands`; Clean clips carries one, and older
/// projects typed theirs), since the project's comes after them all anyway.
/// Unchanged without one. `mirv_fov handleZoom ...` stays.
pub fn lines_sent(config: &PatcherConfig) -> Vec<String> {
    if config.picture_fov.is_none() {
        return config.init_commands.clone();
    }
    let left_out = lines_left_out(config);
    config
        .init_commands
        .iter()
        .filter(|line| !left_out.contains(&line.trim().to_string()))
        .cloned()
        .collect()
}

/// The `mirv_fov <n>` lines [`lines_sent`] leaves out, trimmed. Empty
/// without the project's field of view.
pub fn lines_left_out(config: &PatcherConfig) -> Vec<String> {
    if config.picture_fov.is_none() {
        return Vec::new();
    }
    cfg_scan::project_owned_commands(&config.init_commands)
        .into_iter()
        .map(|(_, command)| command)
        .collect()
}

/// Why Start Capture Batch refuses: the first of the Scheduled `commands`
/// that sets `mirv_fov <n>` while the project sets the field of view
/// (`picture_fov` `Some`), pointing at Picture. Initial Commands' own are
/// left out of what is sent instead ([`lines_sent`]), so they are not
/// refused.
pub fn scheduled_refusal<'a>(
    picture_fov: Option<f32>,
    commands: impl IntoIterator<Item = &'a str>,
) -> Option<String> {
    picture_fov?;
    let commands: Vec<String> = commands.into_iter().map(str::to_string).collect();
    cfg_scan::project_owned_commands(&commands)
        .into_iter()
        .next()
        .map(|(_, command)| crate::messages::project_fov_scheduled_refused(&command))
}

/// One value stated for `mirv_fov`, and where: `file` `None` is a line of
/// Initial Commands itself.
#[derive(Debug, Clone, PartialEq)]
pub struct StatedFov {
    pub value: f32,
    pub file: Option<PathBuf>,
    pub line: usize,
}

/// What sets the field of view around a project's Initial Commands.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FovSources {
    /// What the lines leave it at, the last one winning: a `mirv_fov <n>`
    /// line, or the last one in a file an `exec` line runs, where the `exec`
    /// stands. `None` when they set none.
    pub from_lines: Option<StatedFov>,
    /// Every config that sets it: the one the game's own start-up configs
    /// leave in effect (`config.cfg`, `movie.cfg` it execs, ...), then the
    /// last one in each file the lines `exec`. The project's field of view
    /// overrides each, so Studio warns where one differs.
    pub configs: Vec<StatedFov>,
}

/// `value` read as a field of view, when it is a usable number.
fn parsed(value: &str) -> Option<f32> {
    cfg_scan::unquote(value)
        .parse::<f32>()
        .ok()
        .filter(|v| is_usable(*v))
}

/// The last `mirv_fov` setting among `settings` (a file's, in run order).
fn last_in(settings: &[cfg_scan::CvarSetting]) -> Option<StatedFov> {
    settings.iter().rev().find_map(|s| {
        if !s.cvar.eq_ignore_ascii_case(FOV_CVAR) {
            return None;
        }
        Some(StatedFov {
            value: parsed(&s.value)?,
            file: Some(s.file.clone()),
            line: s.line,
        })
    })
}

/// What `init_commands` and the game's configs in `game_dir` (`<hl.exe
/// dir>/dod`, `None` when there is no game folder) set the field of view to.
/// Reads only.
pub fn fov_sources(game_dir: Option<&Path>, init_commands: &[String]) -> FovSources {
    let mut out = FovSources::default();
    // Not `scan_cached`: Studio asks this while it runs, and a config edited
    // since must show.
    let scan = game_dir.map(cfg_scan::scan).unwrap_or_default();
    if let Some(stated) = scan
        .effective(FOV_CVAR)
        .and_then(|s| last_in(std::slice::from_ref(s)))
    {
        out.configs.push(stated);
    }
    for command in init_commands {
        if let Some(dir) = game_dir
            && let Some(exec) = cfg_scan::follow_initial_exec(dir, command)
        {
            if let Some(stated) = last_in(&exec.settings) {
                out.from_lines = Some(stated.clone());
                out.configs.push(stated);
            }
            continue;
        }
        if let Some((cvar, value)) = cfg_scan::assigned_cvar(command)
            && cvar.eq_ignore_ascii_case(FOV_CVAR)
            && let Some(value) = parsed(&value)
        {
            out.from_lines = Some(StatedFov {
                value,
                file: None,
                line: 0,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    /// A game folder whose `config.cfg` execs `movie.cfg` (`mirv_fov 105`, as
    /// POST's does), and a `dicewsod25.cfg` nothing execs at start-up
    /// (`mirv_fov 110`, as the user's WSOD25 project's lines exec).
    fn fake_game(tag: &str) -> (Scratch, PathBuf) {
        let root = Scratch::new(format_args!("picture_fov_{tag}"));
        let dod = root.join("dod");
        std::fs::create_dir_all(&dod).unwrap();
        std::fs::write(dod.join("config.cfg"), "exec movie.cfg\n").unwrap();
        std::fs::write(
            dod.join("movie.cfg"),
            "mirv_fov \"105\"\nmirv_fov handleZoom enabled 1\n",
        )
        .unwrap();
        std::fs::write(
            dod.join("dicewsod25.cfg"),
            "hud_draw 1\nmirv_fov 110\nr_decals 4096\n",
        )
        .unwrap();
        (root, dod)
    }

    fn lines(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_line_has_no_decimal_on_a_whole_number() {
        assert_eq!(fov_line(90.0), "mirv_fov 90");
        assert_eq!(fov_line(100.5), "mirv_fov 100.5");
    }

    /// With the project's field of view the lines' own `mirv_fov <n>` is
    /// left out (and logged), `handleZoom` kept; without it nothing is.
    #[test]
    fn the_lines_own_field_of_view_is_left_out_only_with_the_projects() {
        let mut config = PatcherConfig {
            init_commands: lines(&["mirv_fov 105", "mirv_fov handleZoom enabled 1", "cl_bob 0"]),
            ..PatcherConfig::default()
        };
        assert_eq!(lines_sent(&config), config.init_commands);
        assert!(lines_left_out(&config).is_empty());
        config.picture_fov = Some(90.0);
        assert_eq!(
            lines_sent(&config),
            ["mirv_fov handleZoom enabled 1", "cl_bob 0"]
        );
        assert_eq!(lines_left_out(&config), ["mirv_fov 105"]);
        assert_eq!(
            crate::messages::project_fov_sent(90.0, &lines_left_out(&config)),
            "[capture] Field of view 90 from Project Settings › Picture, sent after Initial Commands and any cfg they exec; left out 1 mirv_fov line(s) of Initial Commands (mirv_fov 105)"
        );
    }

    #[test]
    fn a_scheduled_mirv_fov_number_is_refused_only_with_the_projects() {
        let scheduled = ["mirv_fov handleZoom enabled 1", "mirv_fov 100"];
        assert_eq!(
            scheduled_refusal(Some(90.0), scheduled).as_deref(),
            Some(
                "\"mirv_fov 100\" is not allowed in Scheduled Commands: this project's field of view is set in Project Settings › Picture."
            )
        );
        assert_eq!(scheduled_refusal(None, scheduled), None);
        assert_eq!(
            scheduled_refusal(Some(90.0), ["mirv_fov handleZoom enabled 1"]),
            None
        );
    }

    #[test]
    fn a_mirv_fov_line_gives_the_field_of_view() {
        let sources = fov_sources(
            None,
            &lines(&["hud_draw 0", "mirv_fov \"100\"", "cl_bob 0"]),
        );
        assert_eq!(
            sources.from_lines.map(|s| (s.value, s.file)),
            Some((100.0, None))
        );
    }

    #[test]
    fn a_cfg_the_lines_exec_gives_it_where_the_exec_stands() {
        let (_root, dod) = fake_game("exec");
        let sources = fov_sources(Some(&dod), &lines(&["exec dicewsod25.cfg"]));
        let from = sources.from_lines.expect("the exec'd cfg sets one");
        assert_eq!(from.value, 110.0);
        assert_eq!(
            from.file.as_deref(),
            Some(dod.join("dicewsod25.cfg").as_path())
        );
        assert_eq!(from.line, 2);

        // A line after the exec has the last word; one before it does not.
        let after = fov_sources(Some(&dod), &lines(&["exec dicewsod25.cfg", "mirv_fov 95"]));
        assert_eq!(after.from_lines.map(|s| s.value), Some(95.0));
        let before = fov_sources(Some(&dod), &lines(&["mirv_fov 95", "exec dicewsod25.cfg"]));
        assert_eq!(before.from_lines.map(|s| s.value), Some(110.0));
    }

    /// POST's movie.cfg runs at game start, before the batch's lines: it is a
    /// config to warn about, not what the lines give.
    #[test]
    fn a_start_up_config_is_not_what_the_lines_give() {
        let (_root, dod) = fake_game("startup");
        let sources = fov_sources(Some(&dod), &lines(&["hud_draw 0"]));
        assert_eq!(sources.from_lines, None);
        assert_eq!(sources.configs.len(), 1);
        assert_eq!(sources.configs[0].value, 105.0);
        assert_eq!(
            sources.configs[0].file.as_deref(),
            Some(dod.join("movie.cfg").as_path())
        );
    }

    #[test]
    fn every_config_that_sets_it_is_listed_for_the_warning() {
        let (_root, dod) = fake_game("configs");
        let sources = fov_sources(Some(&dod), &lines(&["exec dicewsod25.cfg"]));
        let values: Vec<f32> = sources.configs.iter().map(|s| s.value).collect();
        assert_eq!(values, [105.0, 110.0]);
    }

    #[test]
    fn handle_zoom_and_unusable_values_set_nothing() {
        let sources = fov_sources(
            None,
            &lines(&[
                "mirv_fov handleZoom enabled 1",
                "mirv_fov 0",
                "mirv_fov wide",
            ]),
        );
        assert_eq!(sources.from_lines, None);
    }
}
