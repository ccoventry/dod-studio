//! The Capture tab's config warnings: what the game's own config files set,
//! what Initial and Scheduled Commands change, and what the pipeline refuses
//! or ignores (#216, #679). Built from [`cfg_scan`](crate::game_config::cfg_scan)'s
//! read of the game folder; the Tauri command only converts its arguments.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::patch::{CommandRelation, CustomCommand};

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
    /// `config` | `exec` | `initial` | `app` | `before` | `after`
    pub kind: String,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub offset_seconds: Option<f32>,
}

impl From<&crate::game_config::cfg_scan::StatedValue> for ValueSourceRow {
    fn from(v: &crate::game_config::cfg_scan::StatedValue) -> Self {
        use crate::game_config::cfg_scan::ValueSource;
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
            ValueSource::Exec { file, line } => (
                "exec",
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

impl From<&crate::game_config::cfg_scan::ValueConflict> for ValueConflictRow {
    fn from(c: &crate::game_config::cfg_scan::ValueConflict) -> Self {
        ValueConflictRow {
            cvar: c.cvar.clone(),
            values: c.values.iter().map(ValueSourceRow::from).collect(),
            effective: ValueSourceRow::from(&c.effective),
            scheduled: c.values.iter().any(|v| v.source.is_scheduled()),
        }
    }
}

impl From<&crate::game_config::cfg_scan::AsymmetricAfter> for AsymmetricAfterRow {
    fn from(a: &crate::game_config::cfg_scan::AsymmetricAfter) -> Self {
        AsymmetricAfterRow {
            cvar: a.cvar.clone(),
            after: ValueSourceRow::from(&a.after),
            baseline: ValueSourceRow::from(&a.baseline),
        }
    }
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

/// A command from `cfg_scan::NOOP_IN_INIT_COMMANDS` found somewhere that has
/// no effect — never blocking, just something the user should stop expecting
/// to matter.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoopCommandRow {
    pub cvar: String,
    pub command: String,
    /// "Initial Commands", "Scheduled Commands", or "<file>, line <n>" for a
    /// config the engine executes.
    pub source: String,
}

/// A hook console name from before #555 (`dodstudio_*`), in a config or a
/// command list (`cfg_scan::renamed_commands`, `CfgScan::renamed`). The game
/// no longer knows it, so the line does nothing; never blocking, and the app
/// never rewrites the config.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamedCommandRow {
    pub old: String,
    pub new: String,
    /// "Initial Commands", "Scheduled Commands", or "<file>, line <n>".
    pub source: String,
}

fn renamed_rows(commands: &[String], source: &str) -> Vec<RenamedCommandRow> {
    crate::game_config::cfg_scan::renamed_commands(commands)
        .into_iter()
        .map(|(old, new, _)| RenamedCommandRow {
            old,
            new,
            source: source.to_string(),
        })
        .collect()
}

/// An Initial or Scheduled Command the engine's demo command filter drops
/// (`cfg_scan::demo_filter_rule`, #679). Never blocking: the capture still
/// works, the command just never runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilteredCommandRow {
    pub command: String,
    /// `DemoFilterRule::kind`: "nameContains", "nameStartsWith",
    /// "lineStartsWith", "lineContains" or "wordStartsWith".
    pub rule: String,
    /// The text the rule matched, e.g. "_set".
    pub pattern: String,
}

fn filtered_rows(commands: &[String]) -> Vec<FilteredCommandRow> {
    crate::game_config::cfg_scan::demo_filtered_commands(commands)
        .into_iter()
        .map(|f| FilteredCommandRow {
            command: f.command,
            rule: f.rule.kind().to_string(),
            pattern: f.rule.pattern().to_string(),
        })
        .collect()
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
    /// (`crate::game_config::too_long_commands`). Blocking, like the banned lists:
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
    /// Commands that do nothing — see `cfg_scan::NOOP_IN_INIT_COMMANDS`.
    /// Found in Initial Commands or a config the engine executes. Scheduled,
    /// those entries are dangerous rather than inert, so they show up in
    /// `banned_scheduled` instead.
    pub noop_init: Vec<NoopCommandRow>,
    /// Initial / Scheduled Commands the engine's demo command filter drops
    /// (#679). A patched batch puts both lists in the demo, so both are
    /// filtered; a Seek Between Clips batch runs them through the hook DLL,
    /// which skips the filter, so both stay empty while that is on. A config
    /// the engine execs is not filtered, so configs are not checked.
    pub filtered_init: Vec<FilteredCommandRow>,
    pub filtered_scheduled: Vec<FilteredCommandRow>,
    /// Whether the filter above applied: Seek Between Clips is off.
    pub filter_applies: bool,
    /// Initial Commands `exec <file>` whose file the game won't find in any
    /// folder it looks in (`cfg_scan::exec_search_dirs`).
    pub missing_exec: Vec<String>,
    /// `cfg_scan::FATAL_CVARS` entries a config sets wrong -- DoD's own
    /// client quits the game outright the moment it renders a HUD frame with
    /// one of these not at its required value. Not blocking (nothing here can
    /// force a fix to a file the app never writes), but the most severe
    /// warning this report carries: everything else degrades a capture,
    /// this one crashes the game.
    pub fatal_cvars: Vec<CfgFatalRow>,
    /// Hook console names from before #555 in the configs, Initial Commands
    /// or Scheduled Commands, with the name that replaced each.
    pub renamed: Vec<RenamedCommandRow>,
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
fn application_order(commands: &[CustomCommand]) -> Vec<&CustomCommand> {
    let is_after = |c: &CustomCommand| c.relation == CommandRelation::After;
    let mut before: Vec<&CustomCommand> = commands.iter().filter(|c| !is_after(c)).collect();
    before.sort_by(|a, b| {
        b.offset
            .partial_cmp(&a.offset)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut after: Vec<&CustomCommand> = commands.iter().filter(|c| is_after(c)).collect();
    after.sort_by(|a, b| {
        a.offset
            .partial_cmp(&b.offset)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    before.into_iter().chain(after).collect()
}

/// What [`cfg_report`] checks: the game, both command lists and the batch
/// settings that change the report.
#[derive(Debug, Clone, Default)]
pub struct CfgReportRequest {
    /// The game's `hl.exe`, as Configuration has it.
    pub game_path: String,
    pub init_commands: Vec<String>,
    pub scheduled_commands: Vec<CustomCommand>,
    /// The batch's capture fps; `None` keeps the pipeline default.
    pub capture_fps: Option<i32>,
    /// Flush Decals; `None` keeps the pipeline default.
    pub decal_flush: Option<bool>,
    /// Seek Between Clips; `None` keeps the pipeline default.
    pub hook_runs_batch: Option<bool>,
}

/// What the game's own config files set, and what the app's init commands will
/// override.
///
/// Read-only, and advisory. Nothing in this app writes, edits or removes a
/// config file — the fix is always the user's to make. An empty report when
/// the game folder can't be found.
pub fn cfg_report(request: CfgReportRequest) -> CfgReport {
    let filter = DemoFilter::for_batch(request.hook_runs_batch);
    let Some(dir) = game_mod_dir(&request.game_path) else {
        return CfgReport::default();
    };
    let scan = crate::game_config::cfg_scan::scan(&dir);
    let config_cfg_writable = config_cfg_is_writable(&dir);
    let cfg = pipeline_config(&request);
    let effective_commands = crate::game_config::final_init_commands(&cfg);
    let init_commands = &request.init_commands;
    let command_texts: Vec<String> = request
        .scheduled_commands
        .iter()
        .map(|c| c.command.clone())
        .collect();

    let custom = hazard_rows(&command_texts);
    let banned_scheduled = banned_scheduled_rows(&command_texts);
    let values = value_rules(ValueRuleInputs {
        game_dir: &dir,
        scan: &scan,
        effective_commands: &effective_commands,
        init_commands,
        scheduled_commands: &request.scheduled_commands,
        flagged: &flagged_cvars(&custom, &banned_scheduled),
        filter,
    });

    CfgReport {
        unseen: unseen_settings(&scan, &cfg, &effective_commands, filter),
        conflicts: values.conflicts,
        asymmetric: values.asymmetric,
        custom,
        banned_init: banned_rows(init_commands),
        banned_scheduled,
        too_long_init: crate::game_config::too_long_commands(init_commands),
        too_long_scheduled: crate::game_config::too_long_commands(&command_texts),
        decal_default_ring: decal_default_ring(&cfg),
        decal_flush_is_noop: is_decal_flush_noop(&cfg),
        noop_init: noop_rows(&scan, init_commands),
        filtered_init: filter.rows(init_commands),
        filtered_scheduled: filter.rows(&command_texts),
        filter_applies: filter.applies,
        missing_exec: values.missing_exec,
        fatal_cvars: fatal_rows(&scan),
        renamed: renamed_everywhere(&scan, init_commands, &command_texts),
        config_cfg_writable,
    }
}

/// The mod folder (`dod/`) beside the game's `hl.exe`, when it exists.
fn game_mod_dir(game_path: &str) -> Option<PathBuf> {
    let dir = crate::shared::paths::mod_dir_for_exe(&PathBuf::from(game_path))?;
    dir.is_dir().then_some(dir)
}

/// Whether the engine's demo command filter (#679) sees this batch's
/// commands. Seek Between Clips: the hook DLL runs both command lists through
/// `ClientCmd`, which the demo filter never sees (`cfg_scan`'s filter
/// section), so the filter applies only when the batch is patched.
#[derive(Debug, Clone, Copy)]
struct DemoFilter {
    applies: bool,
}

impl DemoFilter {
    /// For a batch with Seek Between Clips as given (`None`: the default).
    fn for_batch(hook_runs_batch: Option<bool>) -> Self {
        DemoFilter {
            applies: !hook_runs_batch.unwrap_or_else(crate::patch::types::default_hook_runs_batch),
        }
    }

    /// Whether the engine drops `command` on the way in, wherever in the demo
    /// it arrives (`cfg_scan::demo_filter_rule`). Never, when the filter
    /// doesn't apply.
    fn drops(self, command: &str) -> bool {
        self.applies && crate::game_config::cfg_scan::demo_filter_rule(command).is_some()
    }

    /// The rows for `commands` the filter drops; empty when it doesn't apply.
    fn rows(self, commands: &[String]) -> Vec<FilteredCommandRow> {
        if self.applies {
            filtered_rows(commands)
        } else {
            Vec::new()
        }
    }
}

/// The patcher config a capture with this request would run with.
///
/// Built so the list the engine will actually receive is checked, the app's
/// own additions (the movie fps, the decal pin) included. `game_path` is
/// needed for `ring_limit`/`final_init_commands` to find the same executed
/// configs `cfg_scan::scan` finds — without it, they cannot see anything a
/// config states and would disagree with what a real capture actually does.
fn pipeline_config(request: &CfgReportRequest) -> crate::patch::PatcherConfig {
    let mut cfg = crate::patch::PatcherConfig {
        init_commands: request.init_commands.clone(),
        game_path: request.game_path.clone(),
        ..Default::default()
    };
    if let Some(v) = request.capture_fps {
        cfg.capture_fps = v;
    }
    if let Some(v) = request.decal_flush {
        cfg.decal_flush = v;
    }
    cfg
}

/// The `r_decals` ring the flush will silently use: Flush Decals is on and
/// nothing states r_decals itself — neither Initial Commands nor an
/// auto-executed config (`ring_limit_from_game_config`, same precedence
/// `ring_limit` itself resolves) — so the app's own default is about to
/// apply. Worth saying so, even though there is nothing actually wrong with
/// taking the default. Guarded to a nonzero ring: a 0 default would mean the
/// flush is a no-op, which is a different (and louder) fact than "here is the
/// default" — see [`is_decal_flush_noop`].
fn decal_default_ring(cfg: &crate::patch::PatcherConfig) -> Option<u32> {
    (cfg.decal_flush
        && crate::game_config::ring_limit_from_init(&cfg.init_commands).is_none()
        && crate::game_config::ring_limit_from_game_config(cfg).is_none())
    .then(|| crate::game_config::ring_limit(cfg))
    .filter(|&ring| ring > 0)
}

/// Whether Flush Decals is on with a 0 ring: the flush is provably
/// pointless, not merely undocumented. Independent of
/// [`decal_default_ring`]: this fires whether the 0 came from an explicit
/// Initial Command, an executed config (same precedence as everything else
/// here — see `ring_limit`), or, if it ever becomes settable,
/// decal_ring_limit's own default being 0.
fn is_decal_flush_noop(cfg: &crate::patch::PatcherConfig) -> bool {
    cfg.decal_flush && crate::game_config::ring_limit(cfg) == 0
}

/// Anything the pipeline reads (`cfg_scan::WATCHED_CVARS`, in that order)
/// that a config sets and no init command even names — the genuinely silent
/// case. Naming it at the same value still counts as seeing it, so that is
/// not reported as unseen. A command the demo filter drops never reaches the
/// game, so it names nothing.
fn unseen_settings(
    scan: &crate::game_config::cfg_scan::CfgScan,
    cfg: &crate::patch::PatcherConfig,
    effective_commands: &[String],
    filter: DemoFilter,
) -> Vec<CfgWarningRow> {
    let named: std::collections::HashSet<String> = effective_commands
        .iter()
        .filter(|c| !filter.drops(c))
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
    let delivered: Vec<String> = cfg
        .init_commands
        .iter()
        .filter(|c| !filter.drops(c))
        .cloned()
        .collect();
    let capture_fov_stated = crate::game_config::capture_fov_from_init(&delivered).is_some();
    let r_decals_stated_in_init = crate::game_config::ring_limit_from_init(&delivered).is_some();
    scan.effective_settings()
        .into_iter()
        .filter(|s| !named.contains(&s.cvar.to_lowercase()))
        .filter(|s| {
            let is_fov_cvar = s.cvar.eq_ignore_ascii_case("mirv_fov")
                || s.cvar.eq_ignore_ascii_case("default_fov");
            !is_fov_cvar || capture_fov_stated
        })
        .filter(|s| {
            !s.cvar.eq_ignore_ascii_case("r_decals") || !cfg.decal_flush || r_decals_stated_in_init
        })
        .map(|s| CfgWarningRow {
            cvar: s.cvar.clone(),
            value: s.value.clone(),
            file: s.file_name(),
            line: s.line,
        })
        .collect()
}

/// Commands refused outright (`cfg_scan::BANNED_COMMANDS`, see its doc
/// comment), in either list.
fn banned_rows(commands: &[String]) -> Vec<BannedCommandRow> {
    crate::game_config::cfg_scan::banned_commands(commands)
        .into_iter()
        .map(|(cvar, command)| BannedCommandRow { cvar, command })
        .collect()
}

/// Scheduled Commands refused outright: the [`banned_rows`] first, then the
/// ones fine as Initial Commands — that's how the decal flush is meant to be
/// configured — but refused here (`cfg_scan::SCHEDULED_BANNED_COMMANDS`).
fn banned_scheduled_rows(command_texts: &[String]) -> Vec<BannedCommandRow> {
    let mut rows = banned_rows(command_texts);
    rows.extend(
        crate::game_config::cfg_scan::scheduled_banned_commands(command_texts)
            .into_iter()
            .map(|(cvar, command)| BannedCommandRow { cvar, command }),
    );
    rows
}

/// Commands that do nothing wherever found (see `NOOP_IN_INIT_COMMANDS`'s
/// doc comment for why): Initial Commands first, then the value a config
/// the engine executes leaves in effect.
fn noop_rows(
    scan: &crate::game_config::cfg_scan::CfgScan,
    init_commands: &[String],
) -> Vec<NoopCommandRow> {
    let mut rows: Vec<NoopCommandRow> =
        crate::game_config::cfg_scan::noop_commands_in_init(init_commands)
            .into_iter()
            .map(|(cvar, command)| NoopCommandRow {
                cvar,
                command,
                source: "Initial Commands".to_string(),
            })
            .collect();
    for &cvar in crate::game_config::cfg_scan::NOOP_IN_INIT_COMMANDS {
        if let Some(setting) = scan.effective(cvar) {
            rows.push(NoopCommandRow {
                cvar: cvar.to_string(),
                command: format!("{} {}", cvar, setting.value),
                source: format!("{}, line {}", setting.file_name(), setting.line),
            });
        }
    }
    rows
}

/// Scheduled Commands that must not run mid-demo. Custom commands are
/// scheduled into playback, so they run after the configs AND after the init
/// commands, and are the only place a value can change mid-demo. Every
/// scheduled `r_decals` breaks the flush, however many there are, so the
/// list is not deduplicated the way the value rules are.
fn hazard_rows(command_texts: &[String]) -> Vec<CustomCommandWarning> {
    crate::game_config::cfg_scan::mid_demo_hazards(command_texts)
        .into_iter()
        .map(|(cvar, command)| CustomCommandWarning {
            command,
            cvar,
            kind: "hazard".to_string(),
            replaced_value: String::new(),
            source: String::new(),
        })
        .collect()
}

/// The cvars (lowercase) of Scheduled Commands already reported as hazards or
/// banned. The value rules leave them out: each has its own, louder warning.
fn flagged_cvars(
    hazards: &[CustomCommandWarning],
    banned_scheduled: &[BannedCommandRow],
) -> std::collections::HashSet<String> {
    hazards
        .iter()
        .map(|w| w.cvar.to_lowercase())
        .chain(banned_scheduled.iter().map(|b| b.cvar.to_lowercase()))
        .collect()
}

/// What [`value_rules`] reads.
struct ValueRuleInputs<'a> {
    game_dir: &'a Path,
    scan: &'a crate::game_config::cfg_scan::CfgScan,
    /// Initial Commands as the engine receives them, the app's own included.
    effective_commands: &'a [String],
    /// Initial Commands as the user typed them.
    init_commands: &'a [String],
    scheduled_commands: &'a [CustomCommand],
    /// From [`flagged_cvars`].
    flagged: &'a std::collections::HashSet<String>,
    filter: DemoFilter,
}

/// The two value rules' rows (#216), and the Initial `exec`s found missing on
/// the way.
struct ValueRules {
    conflicts: Vec<ValueConflictRow>,
    asymmetric: Vec<AsymmetricAfterRow>,
    missing_exec: Vec<String>,
}

/// The two value rules (#216) over the configs, Initial Commands and
/// Scheduled Commands. Commands the demo filter drops set nothing at all, so
/// they are left out; so are flagged Scheduled Commands. An `exec` that gets
/// through runs its file: it is followed (read-only), so its settings join
/// the value rules where the `exec` stands.
fn value_rules(input: ValueRuleInputs) -> ValueRules {
    let scheduled = value_rule_schedule(input.scheduled_commands, input.flagged, input.filter);
    // Initial Commands the demo filter drops set nothing either. The
    // pipeline's own additions all get through (pinned by a builder test),
    // so dropping from the whole list keeps the user's count right.
    let passes = |c: &&String| !input.filter.drops(c);
    let applied_init: Vec<String> = input
        .effective_commands
        .iter()
        .filter(passes)
        .cloned()
        .collect();
    let execs: Vec<crate::game_config::cfg_scan::InitialExec> = applied_init
        .iter()
        .filter_map(|c| crate::game_config::cfg_scan::follow_initial_exec(input.game_dir, c))
        .collect();
    let missing_exec: Vec<String> = execs
        .iter()
        .filter(|e| e.file.is_none())
        .map(|e| e.command.clone())
        .collect();
    let values = crate::game_config::cfg_scan::value_warnings_with_execs(
        input.scan,
        &applied_init,
        input.init_commands.iter().filter(passes).count(),
        &scheduled,
        &execs,
    );
    ValueRules {
        conflicts: values
            .conflicts
            .iter()
            .map(ValueConflictRow::from)
            .collect(),
        asymmetric: values
            .asymmetric
            .iter()
            .map(AsymmetricAfterRow::from)
            .collect(),
        missing_exec,
    }
}

/// The Scheduled Commands the value rules weigh, in the order the engine
/// reaches them ([`application_order`]): not the ones the demo filter drops,
/// nor the ones whose cvar is `flagged`.
fn value_rule_schedule<'a>(
    commands: &'a [CustomCommand],
    flagged: &std::collections::HashSet<String>,
    filter: DemoFilter,
) -> Vec<crate::game_config::cfg_scan::ScheduledCommand<'a>> {
    application_order(commands)
        .into_iter()
        .filter(|c| !filter.drops(&c.command))
        .filter(|c| {
            crate::game_config::cfg_scan::assigned_cvar(&c.command)
                .is_none_or(|(cvar, _)| !flagged.contains(&cvar.to_lowercase()))
        })
        .map(|c| crate::game_config::cfg_scan::ScheduledCommand {
            command: &c.command,
            after: c.relation == CommandRelation::After,
            offset_seconds: c.offset,
        })
        .collect()
}

/// Config-file values DoD's own client will quit the game over -- distinct
/// from the banned lists, which are about commands typed into the
/// pipeline's own fields (see [`CfgFatalRow`]'s doc comment).
fn fatal_rows(scan: &crate::game_config::cfg_scan::CfgScan) -> Vec<CfgFatalRow> {
    crate::game_config::cfg_scan::fatal_cvar_hazards(scan)
        .into_iter()
        .map(|f| CfgFatalRow {
            file: f.file_name(),
            cvar: f.cvar,
            value: f.value,
            required: f.required,
            line: f.line,
        })
        .collect()
}

/// Hook console names from before #555 in the configs, Initial Commands and
/// Scheduled Commands. Configs first, as the engine runs them first.
fn renamed_everywhere(
    scan: &crate::game_config::cfg_scan::CfgScan,
    init_commands: &[String],
    command_texts: &[String],
) -> Vec<RenamedCommandRow> {
    let mut rows: Vec<RenamedCommandRow> = scan
        .renamed
        .iter()
        .map(|r| RenamedCommandRow {
            old: r.old.clone(),
            new: r.new.clone(),
            source: format!("{}, line {}", r.file_name(), r.line),
        })
        .collect();
    rows.extend(renamed_rows(init_commands, "Initial Commands"));
    rows.extend(renamed_rows(command_texts, "Scheduled Commands"));
    rows
}

#[cfg(test)]
#[path = "cfg_report_tests.rs"]
mod tests;
