// game_config/config_values.rs
// What the game's start-up configs set a few cvars to, for Project
// settings › Clip settings (#1085): with Use Clip settings on, a batch sends
// every row after the configs, so a row warns where a config sets its cvar
// to another value (the frontend compares; this only reads).
//
// Reads config files, never writes one (`cfg_scan`'s rule).

use std::path::{Path, PathBuf};

use super::cfg_scan;

/// The value the start-up configs leave one cvar at, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigValue {
    /// The cvar as it was asked for.
    pub cvar: String,
    /// The value, without surrounding quotes.
    pub value: String,
    pub file: PathBuf,
    pub line: usize,
}

/// What the configs the game runs at start-up in `game_dir` (`<hl.exe
/// dir>/dod`) leave each of `cvars` at, the last one winning, as the
/// console does. Only the cvars a config sets, in `cvars`' order. Reads the
/// configs fresh, not `scan_cached`: Studio asks this while it runs, and a
/// config edited since must show.
pub fn start_up_values(game_dir: &Path, cvars: &[String]) -> Vec<ConfigValue> {
    let scan = cfg_scan::scan(game_dir);
    cvars
        .iter()
        .filter_map(|cvar| {
            scan.effective(cvar).map(|s| ConfigValue {
                cvar: cvar.clone(),
                value: cfg_scan::unquote(&s.value),
                file: s.file.clone(),
                line: s.line,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// A game folder whose `config.cfg` execs `movie.cfg`, and a cfg nothing
    /// execs.
    fn fake_game(tag: &str) -> (Scratch, PathBuf) {
        let root = Scratch::new(format_args!("config_values_{tag}"));
        let dod = root.join("dod");
        std::fs::create_dir_all(&dod).unwrap();
        std::fs::write(
            dod.join("config.cfg"),
            "hud_draw 0\nds_hud_hide_hints \"1\"\nexec movie.cfg\n",
        )
        .unwrap();
        std::fs::write(dod.join("movie.cfg"), "hud_draw 1\nr_decals 512\n").unwrap();
        std::fs::write(dod.join("unused.cfg"), "ds_spec_xray_show 2\n").unwrap();
        (root, dod)
    }

    #[test]
    fn a_cvar_takes_the_last_value_the_start_up_configs_give_it() {
        let (_root, dod) = fake_game("last");
        let found = start_up_values(&dod, &names(&["hud_draw"]));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].value, "1");
        assert_eq!(found[0].file.file_name().unwrap(), "movie.cfg");
        assert_eq!(found[0].line, 1);
    }

    #[test]
    fn a_quoted_value_comes_back_without_its_quotes() {
        let (_root, dod) = fake_game("quoted");
        let found = start_up_values(&dod, &names(&["ds_hud_hide_hints"]));
        assert_eq!(found[0].value, "1");
    }

    #[test]
    fn a_cvar_no_config_sets_and_a_config_nothing_execs_are_left_out() {
        let (_root, dod) = fake_game("left_out");
        let found = start_up_values(
            &dod,
            &names(&["ds_spec_xray_show", "ds_hud_show_scope", "r_decals"]),
        );
        let cvars: Vec<&str> = found.iter().map(|v| v.cvar.as_str()).collect();
        assert_eq!(cvars, ["r_decals"]);
    }

    #[test]
    fn a_folder_with_no_configs_sets_nothing() {
        let root = Scratch::new(format_args!("config_values_empty"));
        assert!(start_up_values(&root.join("dod"), &names(&["hud_draw"])).is_empty());
    }
}
