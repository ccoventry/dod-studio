//! The `gl_max_size` an install will run with, for the HD page (#680).
//!
//! The game shows every texture at the largest power of two `gl_max_size`
//! allows, HD replacements included (`texture_hires`' `replacement_cap`), and
//! the engine's own default is 256. An install whose configs leave it there
//! shows HD files shrunk back to 256 px with nothing saying so. This reads
//! what the configs and Studio's Initial Commands leave it at, read-only: the
//! game's `.cfg` files are the user's ([`cfg_scan`](crate::patch::cfg_scan)).

use serde::Serialize;

use crate::patch::cfg_scan::{self, CfgScan};

const CVAR: &str = "gl_max_size";
/// What the engine starts with when nothing sets it.
pub const ENGINE_DEFAULT: u32 = 256;
/// The engine's floor and the hook's ceiling: `GL_Upload32` rounds anything
/// smaller up to 128, and `texture_hires` stops at 4096 (its `MIN_GL_MAX_SIZE`
/// and `MAX_SIDE`).
const MIN_SIDE: u32 = 128;
const MAX_SIDE: u32 = 4096;

/// Where the value comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    /// Studio's Initial Commands, which run after every config.
    InitialCommands,
    /// The last line an executed config sets it on.
    Config { file: String, line: usize },
    /// Nothing sets it.
    EngineDefault,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GlMaxSize {
    /// As written, e.g. `"2048"`.
    pub value: String,
    /// The largest side the game shows a texture at with that value.
    pub shows_at: u32,
    pub source: Source,
}

/// What `scan`'s configs and then `init_commands` leave `gl_max_size` at.
pub fn effective(scan: &CfgScan, init_commands: &[String]) -> GlMaxSize {
    let (value, source) = if let Some(value) = cfg_scan::effective_in(init_commands, CVAR) {
        (value, Source::InitialCommands)
    } else if let Some(setting) = scan.effective(CVAR) {
        (
            setting.value.clone(),
            Source::Config {
                file: setting.file_name(),
                line: setting.line,
            },
        )
    } else {
        (ENGINE_DEFAULT.to_string(), Source::EngineDefault)
    };
    GlMaxSize {
        shows_at: shows_at(&value),
        value,
        source,
    }
}

/// The largest power of two at most `value`, as the engine bounds it; an
/// unreadable value reads as the floor, as the engine's `atof` makes it 0.
fn shows_at(value: &str) -> u32 {
    let max = value
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite() && *v >= MIN_SIDE as f32)
        .map_or(MIN_SIDE, |v| v.min(MAX_SIDE as f32) as u32);
    1 << (31 - max.leading_zeros())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::cfg_scan::CvarSetting;
    use std::path::PathBuf;

    fn config(value: &str, file: &str, line: usize, auto_executed: bool) -> CvarSetting {
        CvarSetting {
            cvar: CVAR.to_string(),
            value: value.to_string(),
            file: PathBuf::from(file),
            line,
            auto_executed,
        }
    }

    fn scan_of(settings: Vec<CvarSetting>) -> CfgScan {
        CfgScan {
            settings,
            ..CfgScan::default()
        }
    }

    #[test]
    fn nothing_set_is_the_engine_default() {
        let got = effective(&CfgScan::default(), &[]);
        assert_eq!(got.value, "256");
        assert_eq!(got.shows_at, 256);
        assert_eq!(got.source, Source::EngineDefault);
    }

    #[test]
    fn the_last_executed_config_line_wins() {
        let scan = scan_of(vec![
            config("256", "config.cfg", 40, true),
            config("2048", "movie.cfg", 3, true),
            // A config nothing execs sets nothing.
            config("128", "old.cfg", 1, false),
        ]);
        let got = effective(&scan, &[]);
        assert_eq!(got.value, "2048");
        assert_eq!(got.shows_at, 2048);
        assert_eq!(
            got.source,
            Source::Config {
                file: "movie.cfg".to_string(),
                line: 3
            }
        );
    }

    #[test]
    fn initial_commands_win_over_every_config() {
        let scan = scan_of(vec![config("256", "config.cfg", 40, true)]);
        let init = ["mirv_fov 90".to_string(), "gl_max_size 1024".to_string()];
        let got = effective(&scan, &init);
        assert_eq!(got.value, "1024");
        assert_eq!(got.source, Source::InitialCommands);
    }

    #[test]
    fn the_size_shown_is_the_power_of_two_the_engine_allows() {
        assert_eq!(shows_at("1500"), 1024);
        assert_eq!(shows_at("2048"), 2048);
        assert_eq!(shows_at("8192"), 4096);
        assert_eq!(shows_at("64"), 128);
        assert_eq!(shows_at("big"), 128);
        assert_eq!(shows_at(" 512 "), 512);
    }
}
