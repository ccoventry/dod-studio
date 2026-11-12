//! Every console name the hook registers is in the #555 old-to-new table
//! (`goldsrc-hooks/src/old_names.rs`), or listed as new since. Reads sources
//! only, so it runs in the fast `Source checks` CI job, before any compile of
//! `native` (which re-exports the same table as `patch::console_prefix`).

#[path = "../../goldsrc-hooks/src/old_names.rs"]
mod old_names;
use old_names::OLD_NAMES;

/// The prefix of every console name the hook registers (`goldsrc-hooks`'
/// `names.rs`; `native::patch::console_prefix::PREFIX` is the same string).
const PREFIX: &str = "ds_";
/// The prefix before #555.
const OLD_PREFIX: &str = "dodstudio_";

/// Names the hook registers that never had a `dodstudio_` one.
const NEW_SINCE_555: &[&str] = &[
    "debug_hang_report_seconds",
    "debug_log_gun_lowered",
    "debug_log_server_sounds",
    "debug_log_view_bob",
    "debug_record_frame_times",
    "debug_texture",
    "hud_crosshair_outline_color",
    "hud_crosshair_outline_size",
    "hud_crosshair_scale",
    "demo_show_load_progress",
    "hud_hide_center_text",
    "hud_hide_hints",
    "hud_hide_plugin_text",
    "hud_hide_teammate_info",
    "hud_overview_hide_speaker",
    "map_apply_lighting",
    "map_reload_lighting",
    "map_run_cfgs",
    "map_weather",
    "map_wind",
    "player_change_name",
    "player_fix_model_flicker",
    "sound_headshot_farther",
    "sound_headshot_louder",
    "sound_headshot_pov_farther",
    "sound_headshot_pov_louder",
    "sound_mute_flag_capture",
    "sound_mute_victory_music",
    "sound_restore_gunshots",
    "spec_hide_text",
    "spec_show_player_info",
    "spec_xray_allies_color",
    "spec_xray_axis_color",
    "spec_xray_show",
    "ui_review_info",
    "ui_review_keys",
    "view_show_own_blood",
];

/// The new names have to be ones the hook registers, or a migrated
/// setting and a config warning would both point at a name that does
/// nothing. The hook is a cdylib, so its sources are read instead. And
/// every name the hook registers is in the table, so none was missed.
#[test]
fn every_new_name_is_one_the_hook_registers() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../goldsrc-hooks/src");
    let mut code = String::new();
    let mut dirs = vec![src];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                code.push_str(&std::fs::read_to_string(&path).unwrap());
            }
        }
    }
    assert!(
        code.contains(&format!("concat!(\"{PREFIX}\", $suffix)")),
        "names.rs no longer uses {PREFIX}"
    );
    for (old, new) in OLD_NAMES {
        assert!(old.starts_with(OLD_PREFIX), "{old}");
        let suffix = new.strip_prefix(PREFIX).unwrap();
        assert!(
            code.contains(&format!("console_name!(\"{suffix}\")")),
            "{old} -> {new}: {new} is not registered by goldsrc-hooks"
        );
    }
    // Every other suffix but the two doc examples in names.rs, and
    // comments that write `console_name!("...")`.
    let mut registered: Vec<&str> = code
        .split("console_name!(\"")
        .skip(1)
        .filter_map(|s| s.split('"').next())
        .filter(|s| {
            !s.is_empty()
                && *s != "status"
                && *s != "killfeed"
                && s.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
        .collect();
    registered.sort_unstable();
    registered.dedup();
    for suffix in registered {
        let name = format!("{PREFIX}{suffix}");
        assert!(
            OLD_NAMES.iter().any(|(_, new)| *new == name) || NEW_SINCE_555.contains(&suffix),
            "{name} has no old name in old_names.rs; a name added since #555 goes in NEW_SINCE_555"
        );
    }
}
