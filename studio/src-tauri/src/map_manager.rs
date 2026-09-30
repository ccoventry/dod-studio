// map_manager.rs
// Whether the demos on screen can actually be played, and on the right map.
//
// Kept out of the scan pipeline deliberately. Scanning parses every frame of
// every demo and is the slow, threaded part of loading a folder; this reads 544
// bytes per demo and one map file per distinct map, so it runs after a scan
// without slowing it, and a failure here costs a badge rather than the folder.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::capture_manager::CustomCommandPayload;
use native::patch::map_check::{self, MapStatus};
use native::patch::map_fetch;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapCheckRow {
    pub demo_path: String,
    pub demo_name: String,
    pub map_name: String,
    /// The build the demo was recorded on. Absent for HLTV demos, which do not
    /// record one.
    pub expected_checksum: Option<u32>,
    /// `ok` | `wrongBuild` | `missing` | `unverifiable` | `unreadableMap` |
    /// `unreadableDemo`
    pub state: String,
    pub detail: String,
    /// Whether the demo can be played at all.
    pub playable: bool,
}

/// Where maps live for a configured `hl.exe`, or an error a person can act on.
fn maps_dir(game_path: &str) -> Result<PathBuf, String> {
    let exe = Path::new(game_path);
    map_check::maps_dir_for_exe(exe)
        .ok_or_else(|| crate::messages::no_map_folder_beside_exe(game_path))
}

/// Check a list of demos against the map library.
///
/// Map files are read once each however many demos want them, because the
/// checksum walks the whole file and a folder of 400 demos covers maybe twenty
/// maps.
#[tauri::command]
pub async fn check_demo_maps(
    demo_paths: Vec<String>,
    game_path: String,
) -> Result<Vec<MapCheckRow>, String> {
    let dir = maps_dir(&game_path)?;

    tokio::task::spawn_blocking(move || {
        let mut seen: HashMap<String, MapStatus> = HashMap::new();
        let mut rows = Vec::with_capacity(demo_paths.len());

        for path in demo_paths {
            let p = PathBuf::from(&path);
            let demo_name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.clone());

            let reference = match map_check::map_reference(&p) {
                Ok(r) => r,
                Err(e) => {
                    rows.push(MapCheckRow {
                        demo_path: path,
                        demo_name,
                        map_name: String::new(),
                        expected_checksum: None,
                        state: "unreadableDemo".to_string(),
                        detail: e,
                        playable: false,
                    });
                    continue;
                }
            };

            // Keyed on name AND wanted build: two demos of the same map name
            // wanting different builds are genuinely different questions.
            let cache_key = format!(
                "{}:{}",
                reference.map_name,
                reference.expected_checksum.unwrap_or(0)
            );
            let status = seen
                .entry(cache_key)
                .or_insert_with(|| map_check::status_of(&reference, &dir))
                .clone();

            let state = match &status {
                MapStatus::Ok { .. } => "ok",
                MapStatus::WrongBuild { .. } => "wrongBuild",
                MapStatus::Missing => "missing",
                MapStatus::Unverifiable => "unverifiable",
                MapStatus::Unreadable { .. } => "unreadableMap",
            };

            rows.push(MapCheckRow {
                demo_path: path,
                demo_name,
                detail: status.summary(&reference.map_name),
                playable: status.is_playable(),
                map_name: reference.map_name,
                expected_checksum: reference.expected_checksum,
                state: state.to_string(),
            });
        }

        rows
    })
    .await
    .map_err(crate::messages::map_check_failed)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfgWarningRow {
    pub cvar: String,
    pub value: String,
    pub file: String,
    pub line: usize,
}

/// One value stated for a cvar, and where (`cfg_scan::StatedValue`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValueSourceRow {
    pub value: String,
    /// `config` | `initial` | `app` | `before` | `after`
    pub kind: String,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub offset_seconds: Option<f32>,
}

impl From<&native::patch::cfg_scan::StatedValue> for ValueSourceRow {
    fn from(v: &native::patch::cfg_scan::StatedValue) -> Self {
        use native::patch::cfg_scan::ValueSource;
        let (kind, file, line, offset_seconds) = match &v.source {
            ValueSource::Config { file, line } => (
                "config",
                Some(
                    file.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| file.to_string_lossy().to_string()),
                ),
                Some(*line),
                None,
            ),
            ValueSource::Initial => ("initial", None, None, None),
            ValueSource::App => ("app", None, None, None),
            ValueSource::ScheduledBefore { offset_seconds } => {
                ("before", None, None, Some(*offset_seconds))
            }
            ValueSource::ScheduledAfter { offset_seconds } => {
                ("after", None, None, Some(*offset_seconds))
            }
        };
        ValueSourceRow {
            value: v.value.clone(),
            kind: kind.to_string(),
            file,
            line,
            offset_seconds,
        }
    }
}

/// Rule 1 of #216: a cvar given different values across the configs, Initial
/// Commands and Scheduled `Before` commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValueConflictRow {
    pub cvar: String,
    /// Every value, in the order the engine runs them.
    pub values: Vec<ValueSourceRow>,
    pub effective: ValueSourceRow,
    /// True when a Scheduled `Before` is among the values, so the row shows
    /// under Scheduled Commands rather than Initial Commands.
    pub scheduled: bool,
}

/// Rule 2 of #216: an unpaired Scheduled `After` that differs from the
/// baseline, so every clip after the first records at its value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AsymmetricAfterRow {
    pub cvar: String,
    pub after: ValueSourceRow,
    pub baseline: ValueSourceRow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomCommandWarning {
    pub command: String,
    pub cvar: String,
    /// `hazard`. Value conflicts are `conflicts`/`asymmetric` now (#216).
    pub kind: String,
    /// What this displaces, and where that came from.
    pub replaced_value: String,
    pub source: String,
}

/// A command from `cfg_scan::BANNED_COMMANDS` found in either command list.
/// Refused outright — see that constant's doc comment for why these specific
/// cvars, and not the wider `MID_DEMO_HAZARDS` set, get this treatment.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BannedCommandRow {
    pub cvar: String,
    pub command: String,
}

/// A config sets a `cfg_scan::FATAL_CVARS` entry to something other than the
/// one value DoD's own client will not quit the game over.
///
/// Distinct from `BannedCommandRow`: that one is a command the user *typed*
/// into Initial or Scheduled Commands, which the app can simply refuse to run.
/// This is a value already sitting in a config file the app never writes to
/// (see `cfg_scan`'s module doc) -- the most it can do is say so.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfgFatalRow {
    pub cvar: String,
    pub value: String,
    pub required: String,
    pub file: String,
    pub line: usize,
}

/// A command from `cfg_scan::NOOP_IN_INIT_COMMANDS` or
/// `NOOP_EVERYWHERE_COMMANDS` found somewhere that has no effect — never
/// blocking, just something the user should stop expecting to matter.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoopCommandRow {
    pub cvar: String,
    pub command: String,
    /// "Initial Commands", "Scheduled Commands", or "<file>, line <n>" for a
    /// config the engine executes.
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CfgReport {
    /// Values the pipeline reads that a config sets and no init command names.
    pub unseen: Vec<CfgWarningRow>,
    /// Rule 1 (#216): cvars given different values across the configs,
    /// Initial Commands (the pipeline's own included) and Scheduled `Before`
    /// commands. Replaces the old override and shadowed lists.
    pub conflicts: Vec<ValueConflictRow>,
    /// Rule 2 (#216): unpaired Scheduled `After` commands that leave a
    /// different value in place for every later clip.
    pub asymmetric: Vec<AsymmetricAfterRow>,
    /// Scheduled commands that must not run mid-demo.
    pub custom: Vec<CustomCommandWarning>,
    /// Banned commands (`cfg_scan::BANNED_COMMANDS`) found in Initial
    /// Commands. Not merely advisory: `start_capture_batch` must refuse to
    /// run while this is non-empty.
    pub banned_init: Vec<BannedCommandRow>,
    /// Same, found in Scheduled (Custom) Commands, plus
    /// `cfg_scan::SCHEDULED_BANNED_COMMANDS` — cvars that are fine in Initial
    /// Commands but refused here because the decal flush sizes itself
    /// against them once, before the demo plays.
    pub banned_scheduled: Vec<BannedCommandRow>,
    /// Initial / Scheduled Commands too long for one ConsoleCommand frame
    /// (`native::patch::too_long_commands`). Blocking, like the banned lists:
    /// `start_capture_batch` refuses them too (#453).
    pub too_long_init: Vec<String>,
    pub too_long_scheduled: Vec<String>,
    /// The `r_decals` ring size this capture will silently use, when nothing
    /// — no config file, no Initial Command — states one (see
    /// `ring_limit_from_init` / `ring_limit_from_game_config`). `None`
    /// whenever Flush Decals Between Clips is off (nothing pins a value at
    /// all) or either of those does state it — their value applies, not the
    /// default. Informational, not a warning: there is nothing wrong with
    /// taking the default, only a silent decision worth surfacing.
    pub decal_default_ring: Option<u32>,
    /// True when Flush Decals Between Clips is on but the `r_decals` value
    /// that will actually reach the engine (`ring_limit`) is 0 — the flush
    /// still runs its full sweep every clip and finds nothing in the ring to
    /// clear, real work for no effect. Reachable by an explicit `r_decals 0`
    /// in Initial Commands, or an executed config stating it with nothing in
    /// Initial Commands overriding — `ring_limit` gives both equal standing,
    /// same as `capture_fov_resolved` does for `mirv_fov`.
    pub decal_flush_is_noop: bool,
    /// Commands that do nothing wherever they were found — see
    /// `cfg_scan::NOOP_IN_INIT_COMMANDS` / `NOOP_EVERYWHERE_COMMANDS`. Found
    /// in Initial Commands, a config the engine executes, or Scheduled
    /// Commands (the last only for `NOOP_EVERYWHERE_COMMANDS` — GoldSrc drops
    /// those from its own message stream regardless of when they arrive;
    /// `NOOP_IN_INIT_COMMANDS` entries are dangerous rather than inert once
    /// scheduled, so they show up in `banned_scheduled` instead).
    pub noop_init: Vec<NoopCommandRow>,
    pub noop_scheduled: Vec<NoopCommandRow>,
    /// `cfg_scan::FATAL_CVARS` entries a config sets wrong -- DoD's own
    /// client quits the game outright the moment it renders a HUD frame with
    /// one of these not at its required value. Not blocking (nothing here can
    /// force a fix to a file the app never writes), but the most severe
    /// warning this report carries: everything else degrades a capture,
    /// this one crashes the game.
    pub fatal_cvars: Vec<CfgFatalRow>,
    /// `dod/config.cfg` exists and is not read-only (#478). The engine
    /// rewrites that file from its current values whenever the game quits,
    /// so anything Initial or Scheduled Commands set ends up saved in it.
    /// Advisory: the app never changes the file or its attributes.
    pub config_cfg_writable: bool,
}

/// Whether `<game_dir>/config.cfg` exists and is not read-only. The engine's
/// own `Host_WriteConfiguration` ("This file is overwritten whenever you
/// change your user settings in the game.") rewrites only that file; the
/// user's other configs are never written back.
fn config_cfg_is_writable(game_dir: &Path) -> bool {
    std::fs::metadata(game_dir.join("config.cfg"))
        .is_ok_and(|m| m.is_file() && !m.permissions().readonly())
}

/// Scheduled commands in the order the engine reaches them.
///
/// Everything set before the highlight runs first — larger offsets are further
/// back, so they come earlier — then everything set after it, nearest first.
/// Which one is first matters: only the first command to touch a cvar displaces
/// what the configs and init commands left it at. The rest displace each other.
fn application_order(commands: &[CustomCommandPayload]) -> Vec<&CustomCommandPayload> {
    let is_after = |c: &CustomCommandPayload| c.relation == "After";
    let mut before: Vec<&CustomCommandPayload> = commands.iter().filter(|c| !is_after(c)).collect();
    before.sort_by(|a, b| {
        b.offset_seconds
            .partial_cmp(&a.offset_seconds)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut after: Vec<&CustomCommandPayload> = commands.iter().filter(|c| is_after(c)).collect();
    after.sort_by(|a, b| {
        a.offset_seconds
            .partial_cmp(&b.offset_seconds)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    before.into_iter().chain(after).collect()
}

/// What the game's own config files set, and what the app's init commands will
/// override.
///
/// Read-only, and advisory. Nothing in this app writes, edits or removes a
/// config file — the fix is always the user's to make.
#[tauri::command]
pub async fn scan_game_configs(
    game_path: String,
    init_commands: Vec<String>,
    custom_commands: Vec<CustomCommandPayload>,
    capture_fps: Option<i32>,
    decal_flush: Option<bool>,
) -> Result<CfgReport, String> {
    let exe = PathBuf::from(&game_path);
    let Some(dir) = exe.parent().map(|p| p.join("dod")) else {
        return Ok(CfgReport::default());
    };
    if !dir.is_dir() {
        return Ok(CfgReport::default());
    }

    tokio::task::spawn_blocking(move || {
        let scan = native::patch::cfg_scan::scan(&dir);
        let config_cfg_writable = config_cfg_is_writable(&dir);

        // The list the engine will actually receive, so the app's own additions
        // — the movie fps, the decal pin — are checked too. game_path is
        // needed for `ring_limit`/`final_init_commands` to find the same
        // executed configs `scan` above already found — without it, they
        // cannot see anything a config states and would disagree with what a
        // real capture actually does.
        let mut cfg = native::patch::PatcherConfig {
            init_commands: init_commands.clone(),
            game_path: game_path.clone(),
            ..Default::default()
        };
        if let Some(v) = capture_fps {
            cfg.capture_fps = v;
        }
        if let Some(v) = decal_flush {
            cfg.decal_flush = v;
        }

        // Nothing states r_decals themselves — neither Initial Commands nor
        // an auto-executed config (`ring_limit_from_game_config`, same
        // precedence `ring_limit` itself resolves) — so the app's own default
        // is about to silently apply. Worth saying so, even though there is
        // nothing actually wrong with taking the default. Guarded to a
        // nonzero ring: a 0 default would mean the flush is a no-op, which is
        // a different (and louder) fact than "here is the default" — see
        // decal_flush_is_noop below.
        let decal_default_ring = (cfg.decal_flush
            && native::patch::ring_limit_from_init(&cfg.init_commands).is_none()
            && native::patch::ring_limit_from_game_config(&cfg).is_none())
        .then(|| native::patch::ring_limit(&cfg))
        .filter(|&ring| ring > 0);
        // The "louder fact" the comment above alludes to: a 0 ring makes the
        // flush provably pointless, not merely undocumented. Checked
        // independently of decal_default_ring above — this fires whether the
        // 0 came from an explicit Initial Command, an executed config (same
        // precedence as everything else here — see `ring_limit`), or, if it
        // ever becomes settable, decal_ring_limit's own default being 0.
        let decal_flush_is_noop = cfg.decal_flush && native::patch::ring_limit(&cfg) == 0;
        let effective_commands = native::patch::final_init_commands(&cfg);

        // Anything the pipeline reads that a config sets and no init command
        // even names — the genuinely silent case. Naming it at the same value
        // still counts as seeing it, so that is not reported as unseen.
        let named: std::collections::HashSet<String> = effective_commands
            .iter()
            .filter_map(|c| c.split_whitespace().next().map(str::to_lowercase))
            .collect();
        // mirv_fov/default_fov are read directly from an executed config
        // whenever neither is stated in Initial Commands
        // (decal_strip::capture_fov_resolved) — no pin ever names them in
        // effective_commands, so `named` alone would never catch that they
        // are seen, and reporting them here too would tell the user to do
        // something the pipeline is already doing. Scoped to exactly that
        // case: a config naming one fov cvar while Initial Commands name the
        // *other* is a real (separate, more involved) cross-cvar precedence
        // question, left alone here.
        //
        // r_decals now works the identical way (`ring_limit_from_game_config`
        // — see `ring_limit`'s doc comment for why it used to be different):
        // a config's own value is read and used directly, with no pin needed
        // to make it "seen" the way `named` otherwise requires.
        let capture_fov_stated = native::patch::capture_fov_from_init(&init_commands).is_some();
        let r_decals_stated_in_init =
            native::patch::ring_limit_from_init(&cfg.init_commands).is_some();
        let unseen = scan
            .effective_settings()
            .into_iter()
            .filter(|s| !named.contains(&s.cvar.to_lowercase()))
            .filter(|s| {
                let is_fov_cvar = s.cvar.eq_ignore_ascii_case("mirv_fov")
                    || s.cvar.eq_ignore_ascii_case("default_fov");
                !is_fov_cvar || capture_fov_stated
            })
            .filter(|s| {
                !s.cvar.eq_ignore_ascii_case("r_decals")
                    || !cfg.decal_flush
                    || r_decals_stated_in_init
            })
            .map(|s| CfgWarningRow {
                cvar: s.cvar.clone(),
                value: s.value.clone(),
                file: s.file_name(),
                line: s.line,
            })
            .collect();

        // Refused outright, in both lists — see BANNED_COMMANDS' doc comment.
        let banned_init: Vec<BannedCommandRow> =
            native::patch::cfg_scan::banned_commands(&init_commands)
                .into_iter()
                .map(|(cvar, command)| BannedCommandRow { cvar, command })
                .collect();

        // Does nothing wherever found — see NOOP_IN_INIT_COMMANDS's doc
        // comment for why. Only mirv_movie_filename is checked against the
        // config scan: `exec`/`quit` (NOOP_EVERYWHERE_COMMANDS) can never
        // appear there in the first place — the scanner follows a config's
        // own `exec` as the real exec chain it is rather than recording it as
        // a setting, and `quit` takes no argument to record as one either.
        let mut noop_init: Vec<NoopCommandRow> =
            native::patch::cfg_scan::noop_commands_in_init(&init_commands)
                .into_iter()
                .map(|(cvar, command)| NoopCommandRow {
                    cvar,
                    command,
                    source: "Initial Commands".to_string(),
                })
                .collect();
        for &cvar in native::patch::cfg_scan::NOOP_IN_INIT_COMMANDS {
            if let Some(setting) = scan.effective(cvar) {
                noop_init.push(NoopCommandRow {
                    cvar: cvar.to_string(),
                    command: format!("{} {}", cvar, setting.value),
                    source: format!("{}, line {}", setting.file_name(), setting.line),
                });
            }
        }

        // Custom commands are scheduled into playback, so they run after the
        // configs AND after the init commands, and are the only place a value
        // can change mid-demo.
        let mut custom = Vec::new();
        let command_texts: Vec<String> =
            custom_commands.iter().map(|c| c.command.clone()).collect();
        let mut banned_scheduled: Vec<BannedCommandRow> =
            native::patch::cfg_scan::banned_commands(&command_texts)
                .into_iter()
                .map(|(cvar, command)| BannedCommandRow { cvar, command })
                .collect();
        // Fine as Initial Commands — that's how the decal flush is meant to be
        // configured — but refused outright here: see
        // `cfg_scan::SCHEDULED_BANNED_COMMANDS`.
        banned_scheduled.extend(
            native::patch::cfg_scan::scheduled_banned_commands(&command_texts)
                .into_iter()
                .map(|(cvar, command)| BannedCommandRow { cvar, command }),
        );
        // GoldSrc drops these from its own message stream regardless of when
        // they arrive — see NOOP_EVERYWHERE_COMMANDS. mirv_movie_filename is
        // not included here: scheduled, it is dangerous rather than inert
        // (already reported above via banned_scheduled).
        let noop_scheduled: Vec<NoopCommandRow> =
            native::patch::cfg_scan::noop_commands_in_scheduled(&command_texts)
                .into_iter()
                .map(|(cvar, command)| NoopCommandRow {
                    cvar,
                    command,
                    source: "Scheduled Commands".to_string(),
                })
                .collect();
        // Every scheduled `r_decals` breaks the flush, however many there are,
        // so the hazard list is not deduplicated the way the overrides are.
        for (cvar, command) in native::patch::cfg_scan::mid_demo_hazards(&command_texts) {
            custom.push(CustomCommandWarning {
                command,
                cvar,
                kind: "hazard".to_string(),
                replaced_value: String::new(),
                source: String::new(),
            });
        }

        // The two value rules (#216). Scheduled commands already reported as
        // hazards or banned are left out: each has its own, louder warning.
        let flagged: std::collections::HashSet<String> = custom
            .iter()
            .map(|w| w.cvar.to_lowercase())
            .chain(banned_scheduled.iter().map(|b| b.cvar.to_lowercase()))
            .collect();
        let scheduled: Vec<native::patch::cfg_scan::ScheduledCommand> =
            application_order(&custom_commands)
                .into_iter()
                .filter(|c| {
                    native::patch::cfg_scan::assigned_cvar(&c.command)
                        .is_none_or(|(cvar, _)| !flagged.contains(&cvar.to_lowercase()))
                })
                .map(|c| native::patch::cfg_scan::ScheduledCommand {
                    command: &c.command,
                    after: c.relation == "After",
                    offset_seconds: c.offset_seconds,
                })
                .collect();
        let values = native::patch::cfg_scan::value_warnings(
            &scan,
            &effective_commands,
            init_commands.len(),
            &scheduled,
        );
        let conflicts: Vec<ValueConflictRow> = values
            .conflicts
            .iter()
            .map(|c| ValueConflictRow {
                cvar: c.cvar.clone(),
                values: c.values.iter().map(ValueSourceRow::from).collect(),
                effective: ValueSourceRow::from(&c.effective),
                scheduled: c.values.iter().any(|v| v.source.is_scheduled()),
            })
            .collect();
        let asymmetric: Vec<AsymmetricAfterRow> = values
            .asymmetric
            .iter()
            .map(|a| AsymmetricAfterRow {
                cvar: a.cvar.clone(),
                after: ValueSourceRow::from(&a.after),
                baseline: ValueSourceRow::from(&a.baseline),
            })
            .collect();

        // Config-file values DoD's own client will quit the game over --
        // distinct from banned_init/banned_scheduled, which is about commands
        // typed into the pipeline's own fields (see CfgFatalRow's doc comment).
        let fatal_cvars: Vec<CfgFatalRow> = native::patch::cfg_scan::fatal_cvar_hazards(&scan)
            .into_iter()
            .map(|f| CfgFatalRow {
                file: f.file_name(),
                cvar: f.cvar,
                value: f.value,
                required: f.required,
                line: f.line,
            })
            .collect();

        CfgReport {
            unseen,
            conflicts,
            asymmetric,
            custom,
            banned_init,
            banned_scheduled,
            too_long_init: native::patch::too_long_commands(&init_commands),
            too_long_scheduled: native::patch::too_long_commands(&command_texts),
            decal_default_ring,
            decal_flush_is_noop,
            noop_init,
            noop_scheduled,
            fatal_cvars,
            config_cfg_writable,
        }
    })
    .await
    .map_err(crate::messages::config_scan_failed)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapFetchResult {
    pub map_name: String,
    pub installed_path: String,
    pub checksum: u32,
    pub bytes: u64,
    pub already_correct: bool,
    /// Where an existing file was moved to, when one was in the way. Nothing is
    /// ever overwritten in place.
    pub replaced_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RollFloorReport {
    pub pre_roll: f32,
    pub pre_roll_floor: f32,
    pub pre_roll_binding: String,
    pub post_roll: f32,
    pub post_roll_floor: f32,
    pub post_roll_binding: String,
    pub audio_resync: f32,
    pub sound_flush: f32,
    pub flush_lead: f32,
    pub scheduled_before: f32,
    pub scheduled_after: f32,
}

/// What the pre-roll and post-roll have to cover for this configuration.
///
/// The rolls stopped being a matter of taste once the audio resync, the sound
/// flush, the decal sweep's lead and the Scheduled Command offsets all started
/// measuring against them. Reported rather than enforced: the terms are
/// knowable, the engine's audio guidance is a 2-4s range rather than a
/// constant, and whether the decal burst itself needs real-time playback is
/// still unverified — so the number is advice, not a clamp.
#[tauri::command]
pub fn roll_floors(
    pre_roll: f32,
    post_roll: f32,
    record_start_lead: Option<f32>,
    record_stop_trail: Option<f32>,
    decal_flush: Option<bool>,
    custom_commands: Vec<CustomCommandPayload>,
) -> RollFloorReport {
    // The lead and trail matter: a scheduled offset anchors to the kill, and
    // recording starts a lead before that, so the lead already covers part of
    // the distance a Before command has to reach back.
    let mut cfg = native::patch::PatcherConfig {
        pre_roll_seconds: pre_roll,
        post_roll_seconds: post_roll,
        record_start_lead: record_start_lead.unwrap_or(0.0),
        record_stop_trail: record_stop_trail.unwrap_or(0.0),
        ..Default::default()
    };
    if let Some(v) = decal_flush {
        cfg.decal_flush = v;
    }
    cfg.custom_commands = custom_commands
        .iter()
        .map(|c| native::patch::CustomCommand {
            command: c.command.clone(),
            offset: c.offset_seconds,
            relation: match c.relation.as_str() {
                "After" => native::patch::CommandRelation::After,
                _ => native::patch::CommandRelation::Before,
            },
        })
        .collect();

    let f = native::patch::builder::roll_floors(&cfg);
    RollFloorReport {
        pre_roll,
        pre_roll_floor: f.pre_roll,
        pre_roll_binding: f.pre_roll_binding.to_string(),
        post_roll,
        post_roll_floor: f.post_roll,
        post_roll_binding: f.post_roll_binding.to_string(),
        audio_resync: f.audio_resync,
        sound_flush: f.sound_flush,
        flush_lead: f.flush_lead,
        scheduled_before: f.scheduled_before,
        scheduled_after: f.scheduled_after,
    }
}

/// Download one map and install it, verified against the build the demo wants.
///
/// This writes into the user's game folder and talks to the network, so it is
/// only ever reached from an explicit action — never from a scan, and never as
/// a side effect of starting a capture.
#[tauri::command]
pub async fn download_map(
    map_name: String,
    expected_checksum: Option<u32>,
    game_path: String,
) -> Result<MapFetchResult, String> {
    let dir = maps_dir(&game_path)?;

    tokio::task::spawn_blocking(move || {
        map_fetch::fetch_map(
            &map_name,
            expected_checksum,
            &dir,
            map_fetch::DEFAULT_MIRROR,
        )
        .map(|o| MapFetchResult {
            map_name: o.map_name,
            installed_path: o.installed.to_string_lossy().to_string(),
            checksum: o.checksum,
            bytes: o.bytes,
            already_correct: o.already_correct,
            replaced_path: o.replaced.map(|p| p.to_string_lossy().to_string()),
        })
    })
    .await
    .map_err(crate::messages::map_download_failed)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    #[test]
    fn a_game_path_with_no_map_folder_says_so_rather_than_failing_later() {
        let err = maps_dir("Z:/nowhere/hl.exe").unwrap_err();
        assert!(err.contains("dod/maps"), "{}", err);
    }

    // ── Config warning report ────────────────────────────────────────────────
    //
    // These drive the real command rather than a extracted helper, so the wiring
    // is covered too: the scan, `final_init_commands`, and the assembly. Two
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

    fn scheduled(command: &str, relation: &str, offset: f32) -> CustomCommandPayload {
        CustomCommandPayload {
            command: command.to_string(),
            relation: relation.to_string(),
            offset_seconds: offset,
        }
    }

    fn report_scheduled(tag: &str, custom: Vec<CustomCommandPayload>) -> CfgReport {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (_dir, game) = fake_game(tag);
        rt.block_on(scan_game_configs(
            game,
            Vec::new(),
            custom,
            Some(120),
            Some(true),
        ))
        .unwrap()
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

    fn report_full(
        tag: &str,
        init: &[&str],
        custom: Vec<CustomCommandPayload>,
        fps: i32,
    ) -> CfgReport {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (_dir, game) = fake_game(tag);
        rt.block_on(scan_game_configs(
            game,
            init.iter().map(|s| s.to_string()).collect(),
            custom,
            Some(fps),
            Some(true),
        ))
        .unwrap()
    }

    fn report(tag: &str, init: &[&str], custom: &[&str], fps: i32) -> CfgReport {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (_dir, game) = fake_game(tag);
        rt.block_on(scan_game_configs(
            game,
            init.iter().map(|s| s.to_string()).collect(),
            custom.iter().map(|s| scheduled(s, "Before", 2.0)).collect(),
            Some(fps),
            Some(true),
        ))
        .unwrap()
    }

    #[test]
    fn a_cvar_only_a_config_sets_is_reported_as_unseen() {
        // r_decals is only ever named in effective_commands when Flush
        // Decals is on — with it off, nothing pins the cvar at all, so a
        // config setting it really is invisible to the pipeline. That is
        // the genuinely silent case this category exists for.
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (_dir, game) = fake_game("unseen");
        let r = rt
            .block_on(scan_game_configs(
                game,
                Vec::new(),
                Vec::new(),
                Some(120),
                Some(false),
            ))
            .unwrap();

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
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (_dir, game) = fake_game_with_r_drawentities("fatal_config", "0", true);
        let r = rt
            .block_on(scan_game_configs(
                game,
                vec![],
                vec![],
                Some(120),
                Some(true),
            ))
            .unwrap();

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
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (_dir, game) = fake_game_with_r_drawentities("fatal_no_cheats", "0", false);
        let r = rt
            .block_on(scan_game_configs(
                game,
                vec![],
                vec![],
                Some(120),
                Some(true),
            ))
            .unwrap();

        assert!(r.fatal_cvars.is_empty(), "{:?}", r.fatal_cvars);
    }

    #[test]
    fn a_config_setting_r_drawentities_to_one_is_not_reported_as_fatal() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (_dir, game) = fake_game_with_r_drawentities("fatal_config_ok", "1", true);
        let r = rt
            .block_on(scan_game_configs(
                game,
                vec![],
                vec![],
                Some(120),
                Some(true),
            ))
            .unwrap();

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
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (_dir, game) = fake_game_with_mirv_movie_filename("noop_config");
        let r = rt
            .block_on(scan_game_configs(
                game,
                Vec::new(),
                Vec::new(),
                Some(120),
                Some(false),
            ))
            .unwrap();

        assert_eq!(r.noop_init.len(), 1, "{:?}", r.noop_init);
        assert_eq!(r.noop_init[0].cvar, "mirv_movie_filename");
        assert!(
            r.noop_init[0].source.contains("movie.cfg"),
            "{:?}",
            r.noop_init[0]
        );
    }

    #[test]
    fn exec_and_quit_in_initial_commands_are_reported_as_noops() {
        let r = report(
            "noop_exec_quit_init",
            &["exec somefile.cfg", "quit"],
            &[],
            120,
        );

        let flagged: Vec<&str> = r.noop_init.iter().map(|n| n.cvar.as_str()).collect();
        assert_eq!(flagged, vec!["exec", "quit"], "{:?}", r.noop_init);
        assert!(r.noop_init.iter().all(|n| n.source == "Initial Commands"));
    }

    #[test]
    fn exec_and_quit_in_scheduled_commands_are_reported_as_noops() {
        let r = report(
            "noop_exec_quit_scheduled",
            &[],
            &["exec somefile.cfg", "quit"],
            120,
        );

        let flagged: Vec<&str> = r.noop_scheduled.iter().map(|n| n.cvar.as_str()).collect();
        assert_eq!(flagged, vec!["exec", "quit"], "{:?}", r.noop_scheduled);
        assert!(
            r.noop_scheduled
                .iter()
                .all(|n| n.source == "Scheduled Commands")
        );
    }

    #[test]
    fn mirv_movie_filename_scheduled_is_banned_not_reported_as_a_noop() {
        let r = report(
            "noop_vs_banned_scheduled",
            &[],
            &["mirv_movie_filename foo"],
            120,
        );

        assert!(r.noop_scheduled.is_empty(), "{:?}", r.noop_scheduled);
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
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (_dir, game) = fake_game_without_r_decals("decal_default_unset");
        let r = rt
            .block_on(scan_game_configs(
                game,
                Vec::new(),
                Vec::new(),
                Some(120),
                Some(true),
            ))
            .unwrap();
        assert_eq!(
            r.decal_default_ring,
            Some(native::patch::PatcherConfig::default().decal_ring_limit)
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
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (_dir, game) = fake_game_without_r_decals("decal_noop_nonzero");
        let r = rt
            .block_on(scan_game_configs(
                game,
                Vec::new(),
                Vec::new(),
                Some(120),
                Some(true),
            ))
            .unwrap();
        assert!(!r.decal_flush_is_noop, "{:?}", r);
    }

    #[test]
    fn a_nonzero_r_decals_a_config_states_is_not_reported_as_a_flush_noop() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (_dir, game) = fake_game_with_r_decals("decal_noop_config_nonzero", "512");
        let r = rt
            .block_on(scan_game_configs(
                game,
                Vec::new(),
                Vec::new(),
                Some(120),
                Some(true),
            ))
            .unwrap();
        assert!(!r.decal_flush_is_noop, "{:?}", r);
        assert!(conflict(&r, "r_decals").is_none(), "{:?}", r.conflicts);
    }

    #[test]
    fn a_zero_r_decals_is_not_reported_as_a_flush_noop_when_flush_is_off() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (_dir, game) = fake_game("decal_noop_flush_off");
        let r = rt
            .block_on(scan_game_configs(
                game,
                vec!["r_decals \"0\"".to_string()],
                Vec::new(),
                Some(120),
                Some(false),
            ))
            .unwrap();
        assert!(!r.decal_flush_is_noop, "{:?}", r);
    }

    #[test]
    fn the_default_ring_is_not_reported_when_flush_is_off() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (_dir, game) = fake_game("decal_default_flush_off");
        let r = rt
            .block_on(scan_game_configs(
                game,
                Vec::new(),
                Vec::new(),
                Some(120),
                Some(false),
            ))
            .unwrap();
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
}
