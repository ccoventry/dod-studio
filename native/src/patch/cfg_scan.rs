// patch/cfg_scan.rs
// What the game's own config files set behind the pipeline's back.
//
// STRICTLY READ-ONLY. This module never writes, edits, moves or deletes a
// config file. The user's configs are theirs; the most this does is say what it
// found and let them decide.
//
// The problem it solves is concrete. The flush sizes its sweep to `r_decals`
// and derives its on-screen test from `mirv_fov`, and both were assumed to come
// from the app's own init commands. They do not have to. A `config.cfg` ending
// in `exec movie.cfg`, and a `movie.cfg` setting `mirv_fov 105`, means the
// engine renders at a FOV the pipeline never hears about — and the only symptom
// is a slightly-too-narrow cone deciding what a camera can see.
//
// Two things this deliberately does not treat as settings:
//
//   * `bind "F7" "r_decals 4000"` — the cvar is a payload, not an execution.
//     Nothing happens until a key is pressed.
//   * `alias foo "r_decals 0"` — same.
//
// Both appear in real DoD configs, and counting them would raise a warning
// about a value the engine never applied.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Cvars the pipeline reads or depends on. Anything here, set anywhere but the
/// app's init commands, is worth telling the user about.
pub const WATCHED_CVARS: &[&str] = &["r_decals", "mirv_fov", "default_fov"];

/// Configs the engine executes on its own at start-up. Everything else is only
/// reached by being `exec`'d from one of these.
const ENTRY_POINTS: &[&str] = &["valve.rc", "config.cfg", "autoexec.cfg", "userconfig.cfg"];

/// Bounds on following `exec` chains, so a config that execs itself — or a
/// folder of hundreds — cannot turn a scan into a hang.
const MAX_DEPTH: usize = 8;
const MAX_FILES: usize = 64;
const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// A full `config.cfg` is a couple of hundred assignments. This is a ceiling on
/// something pathological, not a working limit.
const MAX_SETTINGS: usize = 4096;

/// Commands whose arguments are stored, not run.
const NOT_EXECUTED: &[&str] = &["bind", "unbind", "alias", "bindtoggle", "+bind"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CvarSetting {
    pub cvar: String,
    pub value: String,
    pub file: PathBuf,
    pub line: usize,
    /// Whether this file is reached from a config the engine runs by itself.
    /// A config nobody execs sets nothing.
    pub auto_executed: bool,
}

impl CvarSetting {
    pub fn file_name(&self) -> String {
        display_name(&self.file)
    }
}

/// A config's file name for a warning, or the whole path when it has none.
fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

#[derive(Debug, Clone, Default)]
pub struct CfgScan {
    /// Every watched cvar assignment found, in the order the engine would reach
    /// them — so the last entry for a cvar is the one that wins.
    pub settings: Vec<CvarSetting>,
    pub files_read: usize,
    /// Configs present in the folder that nothing execs. Reported separately:
    /// they set nothing today, but a `movie.cfg` is one `exec` away from doing
    /// so, and a user reading a warning deserves to know it is there.
    pub unreferenced: Vec<PathBuf>,
}

impl CfgScan {
    /// The value the engine would end up with for a cvar, considering only
    /// configs it actually executes. Last one wins, as the console does.
    pub fn effective(&self, cvar: &str) -> Option<&CvarSetting> {
        self.settings
            .iter()
            .rfind(|s| s.auto_executed && s.cvar.eq_ignore_ascii_case(cvar))
    }

    /// Every watched cvar that an executed config sets.
    pub fn effective_settings(&self) -> Vec<&CvarSetting> {
        WATCHED_CVARS
            .iter()
            .filter_map(|c| self.effective(c))
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.settings.is_empty() && self.unreferenced.is_empty()
    }
}

/// Where a value was stated, for the two value-warning rules (#216).
#[derive(Debug, Clone, PartialEq)]
pub enum ValueSource {
    /// A line in a config the engine executes.
    Config { file: PathBuf, line: usize },
    /// An Initial Command the user typed.
    Initial,
    /// An Initial Command the pipeline appends for itself
    /// (`builder::final_init_commands`), e.g. `mirv_movie_fps` from Capture FPS.
    App,
    /// A Scheduled Command, `offset_seconds` before each highlight.
    ScheduledBefore { offset_seconds: f32 },
    /// A Scheduled Command, `offset_seconds` after each highlight.
    ScheduledAfter { offset_seconds: f32 },
}

impl ValueSource {
    pub fn is_config(&self) -> bool {
        matches!(self, ValueSource::Config { .. })
    }

    pub fn is_scheduled(&self) -> bool {
        matches!(
            self,
            ValueSource::ScheduledBefore { .. } | ValueSource::ScheduledAfter { .. }
        )
    }
}

/// One value stated for a cvar, and where.
#[derive(Debug, Clone, PartialEq)]
pub struct StatedValue {
    pub value: String,
    pub source: ValueSource,
}

/// Rule 1: a cvar given two or more different values across the configs,
/// Initial Commands and Scheduled `Before` commands.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueConflict {
    pub cvar: String,
    /// Every value stated, in the order the engine runs them: configs, then
    /// Initial Commands, then Scheduled `Before` commands furthest back first.
    pub values: Vec<StatedValue>,
    /// The last of `values`: the one in effect when the highlight plays.
    pub effective: StatedValue,
}

/// Rule 2: a Scheduled `After` command with no `Before` for the same cvar,
/// whose value differs from the baseline. Scheduled Commands fire around every
/// highlight, so nothing puts the value back: the first clip records at the
/// baseline and every later clip at the `After`'s value.
#[derive(Debug, Clone, PartialEq)]
pub struct AsymmetricAfter {
    pub cvar: String,
    /// The last unpaired `After` to fire, which is the value every later clip
    /// starts from.
    pub after: StatedValue,
    /// Rule 1's effective value, which the first clip records at.
    pub baseline: StatedValue,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ValueWarnings {
    pub conflicts: Vec<ValueConflict>,
    pub asymmetric: Vec<AsymmetricAfter>,
}

/// A Scheduled Command, as the value rules need it.
#[derive(Debug, Clone, Copy)]
pub struct ScheduledCommand<'a> {
    pub command: &'a str,
    pub after: bool,
    pub offset_seconds: f32,
}

/// Whether two stated values are the same setting. Case-insensitive, and
/// numeric where both sides are numbers, so `1` and `1.0` agree.
fn same_value(a: &str, b: &str) -> bool {
    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a.eq_ignore_ascii_case(b),
    }
}

/// The two value-conflict rules from #216, replacing the old override,
/// shadowed and "Scheduled Commands override earlier values" warnings.
///
/// **Rule 1, conflicting values.** Pool every value stated for a cvar in the
/// executed configs (duplicates included), `init_commands` and the
/// `Before` commands. If they all agree, however many there are, say nothing.
/// Otherwise report it once, naming every value and the effective one: the
/// last in firing order.
///
/// **Rule 2, asymmetric value.** An `After` command with no `Before` for the
/// same cvar ("unpaired"), whose value differs from Rule 1's effective value.
/// An `After`'s value is never part of Rule 1's pool, paired or not, so once
/// any `Before` pairs it neither rule looks at it again. That is the accepted
/// gap in #216: a paired `After` is presumed to be a deliberate restore.
///
/// `init_commands` is the list the engine receives (`final_init_commands`),
/// whose first `user_count` entries the user typed and the rest the pipeline
/// appends. `scheduled` must already be in firing order (`Before`s furthest
/// back first, then `After`s nearest first) and exclude anything reported
/// elsewhere as a hazard or banned.
///
/// A conflict made only of config lines is **not** reported: those are
/// between the user's own files, nothing the app sends is involved, and the
/// scanner records every `word value` line as an assignment, including
/// commands such as `echo`, which would make config-only "conflicts" noisy.
pub fn value_warnings(
    scan: &CfgScan,
    init_commands: &[String],
    user_count: usize,
    scheduled: &[ScheduledCommand],
) -> ValueWarnings {
    // Pool per cvar, in firing order, cvars in first-seen order and spelling.
    // A Vec rather than a map: a config is a couple of hundred lines.
    let mut pool: Vec<(String, Vec<StatedValue>)> = Vec::new();
    let mut add = |cvar: &str, value: String, source: ValueSource| {
        let stated = StatedValue { value, source };
        match pool.iter_mut().find(|(c, _)| c.eq_ignore_ascii_case(cvar)) {
            Some((_, values)) => values.push(stated),
            None => pool.push((cvar.to_string(), vec![stated])),
        }
    };

    for setting in scan.settings.iter().filter(|s| s.auto_executed) {
        add(
            &setting.cvar,
            setting.value.clone(),
            ValueSource::Config {
                file: setting.file.clone(),
                line: setting.line,
            },
        );
    }
    for (index, command) in init_commands.iter().enumerate() {
        if let Some((cvar, value)) = assigned_cvar(command) {
            let source = if index < user_count {
                ValueSource::Initial
            } else {
                ValueSource::App
            };
            add(&cvar, value, source);
        }
    }
    for s in scheduled.iter().filter(|s| !s.after) {
        if let Some((cvar, value)) = assigned_cvar(s.command) {
            add(
                &cvar,
                value,
                ValueSource::ScheduledBefore {
                    offset_seconds: s.offset_seconds,
                },
            );
        }
    }

    let mut out = ValueWarnings::default();
    for (cvar, values) in &pool {
        let effective = values.last().expect("a pool entry has a value").clone();
        let disagree = values
            .iter()
            .any(|v| !same_value(&v.value, &effective.value));
        let app_side = values.iter().any(|v| !v.source.is_config());
        if disagree && app_side {
            out.conflicts.push(ValueConflict {
                cvar: cvar.clone(),
                values: values.clone(),
                effective,
            });
        }
    }

    // Rule 2. The last unpaired After per cvar is what later clips start from.
    let mut last_after: Vec<(String, StatedValue)> = Vec::new();
    for s in scheduled.iter().filter(|s| s.after) {
        if let Some((cvar, value)) = assigned_cvar(s.command) {
            let stated = StatedValue {
                value,
                source: ValueSource::ScheduledAfter {
                    offset_seconds: s.offset_seconds,
                },
            };
            match last_after
                .iter_mut()
                .find(|(c, _)| c.eq_ignore_ascii_case(&cvar))
            {
                Some((_, slot)) => *slot = stated,
                None => last_after.push((cvar, stated)),
            }
        }
    }
    for (cvar, after) in last_after {
        let Some((_, values)) = pool.iter().find(|(c, _)| c.eq_ignore_ascii_case(&cvar)) else {
            // Nothing states a baseline, so there is nothing known to differ
            // from. The old warnings were silent here too.
            continue;
        };
        let paired = values
            .iter()
            .any(|v| matches!(v.source, ValueSource::ScheduledBefore { .. }));
        let baseline = values.last().expect("a pool entry has a value");
        if !paired && !same_value(&after.value, &baseline.value) {
            out.asymmetric.push(AsymmetricAfter {
                cvar,
                after,
                baseline: baseline.clone(),
            });
        }
    }
    out
}

/// Cvars that must not change once a demo is playing.
///
/// `r_decals` is the reason this list exists. It bounds how far the engine's
/// rotating decal index may travel before it wraps; it evicts nothing.
/// Lowering it mid-demo strands every decal sitting above the new limit —
/// permanently, for the rest of playback — and the decal flush's entire
/// design rests on the ring being set once at demo load and never touched
/// again.
///
/// `mirv_fov` is the same shape of problem: `decal_strip::capture_fov_resolved`
/// reads it once, from `init_commands`/the detected game config, as a pre-pass
/// before the demo plays, and sizes the whole sweep's on-screen test against
/// that single value. A Scheduled Command changing it mid-demo does not
/// retroactively resize anything — the flush already decided what counts as
/// on screen for the entire clip.
///
/// `gl_widescreenfov` widens the effective on-screen FOV for a wide aspect
/// ratio the same way `mirv_fov`/`default_fov` do, but `capture_fov_resolved`
/// never reads it at all — the pipeline has no idea it exists, let alone that
/// it changed. A mid-demo toggle is strictly worse than a mid-demo `mirv_fov`:
/// the flush's sizing goes wrong with nothing anywhere that could have caught it.
///
/// The rest are `builder::write_helper_cfg`'s own recording mechanics —
/// `mirv_movie_filename` is what the `<demo>_route_N` aliases set once per
/// block to route that block's frames to the right take folder, and a
/// scheduled one firing mid-clip would silently write frames into whatever
/// folder it named instead, with nothing to notice the manifest and the disk
/// have diverged. `mirv_recordmovie_start`/`_stop` are what `sys_record_start`/
/// `sys_record_stop` schedule at the block's own record bounds — a stray one
/// races that and can start or end a take at the wrong tick. `mirv_movie_fps`
/// is pinned once at load (see `builder::final_init_commands`) and everything
/// downstream — the fps stamped into take metadata, Render Studio's own
/// expectation — assumes that never changes mid-batch. `mirv_movie_ffmpeg`
/// configures the direct-to-video encoder pipe the same way, once, before
/// anything records into it. `mirv_agr` is AGR capture mode's recorder: the
/// route aliases start it into each block's own file and `sys_record_stop`
/// ends it, so a scheduled one would end a take early or start one into a file
/// nobody planned.
///
/// `mirv_movie_separate_hud` deliberately is NOT here. It used to be, but the
/// only reason was that the pipeline always re-appended its own value to
/// Initial Commands after the user's, making anything the user set — Initial
/// or Scheduled — moot. That checkbox is gone (#214; typing the
/// command into Initial Commands directly is the only way to use it now), and
/// with it the one confirmed reason to flag this cvar at all. Nothing in this
/// codebase has actually tested what a mid-demo toggle does — unlike
/// `r_decals`/`mirv_fov`, which are measured, this would be a guess by
/// analogy, so it stays untracked rather than asserting a mechanism nobody
/// has verified.
/// `host_framerate` is `sys_fast_forward`/`sys_normal_speed`'s own mechanism
/// for the real-time run-up before recording (`docs/goldsrc_dod_quirks.md`'s
/// audio-resync entry) — a scheduled one races that timing, not the record
/// itself (recording pins its own timestep regardless).
///
/// All of these share the same failure mode: the capture still completes and
/// still looks plausible.
pub const MID_DEMO_HAZARDS: &[&str] = &[
    "r_decals",
    "mirv_fov",
    "gl_widescreenfov",
    "mirv_movie_filename",
    "mirv_recordmovie_start",
    "mirv_recordmovie_stop",
    "mirv_movie_fps",
    "mirv_movie_ffmpeg",
    "host_framerate",
    "mirv_agr",
];

/// Commands refused wherever a command can be typed (Initial Commands and
/// Scheduled Commands alike), not merely shadowed or flagged as a mid-demo
/// hazard the way the rest of `MID_DEMO_HAZARDS` is. Two different reasons
/// land a command here:
///
///   * **the pipeline owns it outright** — no dedicated setting exists, and no
///     scenario has been found where a user typing one is anything but a
///     misunderstanding (`mirv_recordmovie_start`/`_stop`, `mirv_movie_ffmpeg`,
///     `host_framerate`);
///   * **DoD's own client has no legitimate non-default value for it at
///     all** — `r_drawentities`, `cl_lw`.
///
/// Distinct from `mirv_movie_fps`, which the pipeline also always pins but
/// which corresponds to a real setting (Output Format → Capture FPS) —
/// typing that is redundant, not dangerous, so it stays shadowed-with-a-
/// warning rather than refused. `mirv_movie_separate_hud` is not here either,
/// and not in `MID_DEMO_HAZARDS`: no setting exists behind it any more
/// (#214), Initial Commands is simply the intended way to use
/// it, and typing it in Scheduled Commands instead is untracked rather than
/// flagged — see `MID_DEMO_HAZARDS`'s own doc comment for why. Also distinct
/// from `mirv_movie_filename`, which used to be here too — see
/// `SCHEDULED_BANNED_COMMANDS` for why it moved.
///
/// - `mirv_recordmovie_start` / `mirv_recordmovie_stop` — the pipeline's own
///   `sys_record_start`/`sys_record_stop` scheduling relies on being the only
///   thing calling these, at exactly the block's own record bounds.
/// - `mirv_movie_ffmpeg` — the direct-to-video encoder pipe, configured once
///   before anything records into it.
/// - `host_framerate` — `sys_fast_forward`/`sys_normal_speed`'s own mechanism
///   for the real-time run-up before recording. Floated as possibly having a
///   legitimate creative use (frame-by-frame stepping, per
///   `docs/goldsrc_dod_quirks.md`'s High-Precision Frame Pacing entry) and
///   rejected: "It's dangerous and nobody uses that."
/// - `r_drawentities` / `cl_lw` — DoD 1.3's `client.dll` runs a cvar-enforcement
///   check inside `CHud::Redraw`: if either is not `1`, it forces the correct
///   value back, prints an error, and calls `quit` — the process exits
///   outright rather than merely correcting course. Refused here for both,
///   because Initial/Scheduled Commands are cheap to restrict and this module
///   cannot see whether a user's own configs also turned cheats on.
///
///   The two are *not* equally reachable, though, and `FATAL_CVARS` draws the
///   distinction that matters when scanning a user's config files. `cl_lw` is
///   an ordinary client cvar and simply takes the value it is given.
///   `r_drawentities` is on GoldSrc's own hardcoded clamp list: while
///   `sv_cheats` is `0` the engine resets it to `1.0` and the value never
///   survives long enough for DoD's client to see it. Verified live and in
///   the binaries — `hw.dll` `0x1d455c9`, gated on `sv_cheats` at `0x1e56404`.
/// - `dodstudio_batch` / `dodstudio_schedule` — the hook DLL's runners for a
///   batch run without patched demos (#434). One started from a batch's own
///   commands would replace the batch that is running.
pub const BANNED_COMMANDS: &[&str] = &[
    "mirv_recordmovie_start",
    "mirv_recordmovie_stop",
    "mirv_movie_ffmpeg",
    "host_framerate",
    "dodstudio_batch",
    "dodstudio_schedule",
    "r_drawentities",
    "cl_lw",
];

/// A cvar DoD's own client quits the game over, plus what it takes for that to
/// actually be reachable.
///
/// A command typed into Initial or Scheduled Commands is refused outright via
/// `BANNED_COMMANDS` — but a config file the user already has is a different
/// problem: nothing here writes to it (STRICTLY READ-ONLY, module-level
/// doc), so the most this can do is detect and warn. See `fatal_cvar_hazards`.
///
/// Both cvars are checked inside `CHud::Redraw` (`client.dll` RVA `0x1936e20`),
/// so the failure is not confined to start-up: a config that only sets one
/// after the HUD is already drawing is exactly as fatal as setting it before
/// launch. The check runs when the HUD *draws*, which is also why it does not
/// fire while the console is open — see `docs/goldsrc_dod_quirks.md`.
pub struct FatalCvar {
    pub cvar: &'static str,
    /// The only value that does not quit the game.
    pub required: &'static str,
    /// Whether reaching DoD's client requires cheats to be on.
    ///
    /// GoldSrc clamps some renderer cvars itself, on a hardcoded list gated on
    /// `sv_cheats` (`hw.dll` `0x1d455c9`). While `sv_cheats` is `0` the engine
    /// resets `r_drawentities` to `1.0` and the value never survives to be
    /// read, so a config setting it is inert — reporting that as fatal would
    /// block a capture over a harmless line. `cl_lw` has no such clamp and is
    /// always fatal.
    pub needs_sv_cheats: bool,
}

pub const FATAL_CVARS: &[FatalCvar] = &[
    FatalCvar {
        cvar: "r_drawentities",
        required: "1",
        needs_sv_cheats: true,
    },
    FatalCvar {
        cvar: "cl_lw",
        required: "1",
        needs_sv_cheats: false,
    },
];

/// Whether an executed config turns cheats on, which is what decides if a
/// `needs_sv_cheats` entry can reach DoD's client at all. Any non-zero numeric
/// value counts; anything unparseable is treated as off, matching the engine's
/// own float coercion of a cvar string.
fn sv_cheats_enabled(scan: &CfgScan) -> bool {
    scan.effective("sv_cheats")
        .is_some_and(|s| s.value.trim().parse::<f32>().is_ok_and(|v| v != 0.0))
}

/// One cvar a config sets to a value DoD's own client will quit the game over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FatalCvarSetting {
    pub cvar: String,
    pub value: String,
    /// The only value that does not crash — always `"1"` today, but named
    /// rather than hardcoded so a caller's message stays correct if
    /// `FATAL_CVARS` ever gains an entry with a different one.
    pub required: String,
    pub file: PathBuf,
    pub line: usize,
}

impl FatalCvarSetting {
    pub fn file_name(&self) -> String {
        display_name(&self.file)
    }
}

/// Every `FATAL_CVARS` entry an executed config sets to something other than
/// its required value.
///
/// Uses `CfgScan::effective`, the same last-one-wins resolution as everything
/// else here — a `movie.cfg` that first sets `r_drawentities 0` and later
/// corrects it back to `1` is not a hazard, because that is genuinely the
/// value the engine ends up with.
pub fn fatal_cvar_hazards(scan: &CfgScan) -> Vec<FatalCvarSetting> {
    let cheats = sv_cheats_enabled(scan);
    FATAL_CVARS
        .iter()
        .filter_map(|entry| {
            if entry.needs_sv_cheats && !cheats {
                // The engine clamps it back before anything downstream reads
                // it, so the config line is inert rather than fatal.
                return None;
            }
            let setting = scan.effective(entry.cvar)?;
            (setting.value != entry.required).then(|| FatalCvarSetting {
                cvar: setting.cvar.clone(),
                value: setting.value.clone(),
                required: entry.required.to_string(),
                file: setting.file.clone(),
                line: setting.line,
            })
        })
        .collect()
}

/// Commands from `list` that appear in `commands`, as (matched cvar, whole
/// trimmed line) pairs, in the order they were found.
fn commands_matching(list: &[&str], commands: &[String]) -> Vec<(String, String)> {
    commands
        .iter()
        .filter_map(|raw| {
            let trimmed = raw.trim();
            let head = trimmed.split_whitespace().next()?;
            list.iter()
                .find(|h| head.eq_ignore_ascii_case(h))
                .map(|h| ((*h).to_string(), trimmed.to_string()))
        })
        .collect()
}

/// Commands from `MID_DEMO_HAZARDS` that must not be run during playback.
pub fn mid_demo_hazards(commands: &[String]) -> Vec<(String, String)> {
    commands_matching(MID_DEMO_HAZARDS, commands)
}

/// Commands from `BANNED_COMMANDS` present anywhere in `commands` — Initial
/// or Scheduled alike. Refused outright, not merely shadowed or flagged.
pub fn banned_commands(commands: &[String]) -> Vec<(String, String)> {
    commands_matching(BANNED_COMMANDS, commands)
}

/// Cvars that are fine — expected, even — in Initial Commands, but must be
/// refused outright rather than merely flagged as a `MID_DEMO_HAZARDS`
/// warning when they show up in Scheduled Commands instead.
///
/// `r_decals`, `mirv_fov` and `gl_widescreenfov` all feed the decal flush's
/// one-time sizing pre-pass: `r_decals` bounds the decal ring, `mirv_fov`
/// sizes the on-screen sweep test against `decal_strip::capture_fov_resolved`'s
/// single read of it before the demo plays, and `gl_widescreenfov` widens the
/// effective FOV the same way without the flush even knowing the cvar exists.
/// A Scheduled Command changing any of them mid-demo does not retroactively
/// resize anything — the flush already decided what counts as on screen for
/// the entire clip — so a warning banner isn't enough here the way it is for
/// the rest of `MID_DEMO_HAZARDS`: the capture would complete and look
/// plausible while quietly being wrong, hence refused rather than a hazard.
///
/// `mirv_movie_filename` is a different shape of exception, moved here from
/// `BANNED_COMMANDS`: in Initial Commands (or a config) it is
/// not merely safe, it is inert — `build_batch_queue` schedules a fresh
/// `<demo>_route_N` alias (which sets it) at the same tick as every block's
/// own `sys_record_start`, for every block including the first, so a value
/// stated at demo load never survives to any actual recording (see
/// `NOOP_IN_INIT_COMMANDS`, which reports exactly that). Scheduled instead,
/// it fires mid-clip — between one block's route alias and the next — and
/// genuinely misroutes that block's frames, which is the danger it was
/// originally banned everywhere for.
///
/// `mirv_agr` (#450) is here rather than in `BANNED_COMMANDS`, a deliberate
/// call. Scheduled, it collides with AGR capture mode's own start/stop at each
/// block's bounds. Outside that mode a scheduled `mirv_agr start` names one
/// fixed file for every highlight, so each clip overwrites the last; AGR mode
/// is the way to get one file per clip. In Initial Commands it is not a collision: a
/// `mirv_agr start` there opens a file at demo load, and in AGR mode the first
/// block's own start simply closes it and opens the planned one (HLAE's start
/// closes any recording already open). So it is refused only where it can
/// actually misplace a take — the same shape as `mirv_movie_filename`.
pub const SCHEDULED_BANNED_COMMANDS: &[&str] = &[
    "r_decals",
    "mirv_fov",
    "gl_widescreenfov",
    "mirv_movie_filename",
    "mirv_agr",
];

// ── The demo command filter (#679) ──────────────────────────────────────────
//
// Every type-3 `ConsoleCommand` frame a demo carries goes through the same
// filter as `svc_stufftext` before it reaches the command buffer (`hw+0x1dce0`
// pre-Anniversary, `hw+0x1aaa30` on the 25th Anniversary build;
// `docs/goldsrc_hw_dll_survey.md` §13.3). A command it matches is dropped with
// nothing printed. Initial Commands and Scheduled Commands are both written
// into the patched demo as `ConsoleCommand` frames (`engine.rs`), so both are
// filtered; a config the engine execs at start-up is a different path and is
// not. The lists below are the filter's own, read offline from `hw.dll`.
//
// This is the tier `exec` and `quit` used to have to themselves: `dod-studio`
// once planned to `exec` a per-demo config and to inject `quit` at batch end,
// and neither ever did anything (`docs/hlae_protocols.md`, "Sandbox Escape").
// Both are just two cases of this filter.
//
// The third stage of the filter, which runs only when `cl_filterstuffcmd` is
// non-zero (default `0`), is not modelled.

/// Dropped when the command's name contains one of these, any case. A
/// substring test, so HLAE's `mirv_matte_setcolor` is dropped for `_set`.
pub const DEMO_FILTER_NAME_CONTAINS: &[&str] = &[
    "bind",
    "_set",
    "unbind",
    "retry",
    "quit",
    "_restart",
    "motd_write",
    "motdfile",
    "kill",
    "exit",
    "writecfg",
    "cl_filterstuffcmd",
    "unbindall",
];

/// Dropped when the command's name starts with one of these, any case.
pub const DEMO_FILTER_NAME_STARTS_WITH: &[&str] = &["connect"];

/// Dropped when the whole line starts with one of these.
pub const DEMO_FILTER_LINE_STARTS_WITH: &[&str] = &["alias "];

/// Dropped when one of these appears anywhere on the line, arguments
/// included. `exec` is exempt only in the `tfc` game directory, never in DoD.
pub const DEMO_FILTER_LINE_CONTAINS: &[&str] = &[
    "bind ",
    "unbind ",
    "_restart",
    "exit",
    "writecfg",
    "cl_filterstuffcmd",
    "unbindall",
    "exec",
];

/// Dropped when one of these starts a word anywhere on the line: at the
/// start, or after a space, `;` or newline. So `reconnect ` is not caught by
/// `connect `, but `echo got a kill` is caught by `kill`.
pub const DEMO_FILTER_WORD_STARTS_WITH: &[&str] = &[
    "connect ",
    "motd_write",
    "motdfile",
    "retry",
    "_set",
    "quit",
    "kill",
];

/// Which of the filter's rules dropped a command, and the text it matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DemoFilterRule {
    NameContains(&'static str),
    NameStartsWith(&'static str),
    LineStartsWith(&'static str),
    LineContains(&'static str),
    WordStartsWith(&'static str),
}

impl DemoFilterRule {
    /// The rule's kind, as the frontend's strings key it.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::NameContains(_) => "nameContains",
            Self::NameStartsWith(_) => "nameStartsWith",
            Self::LineStartsWith(_) => "lineStartsWith",
            Self::LineContains(_) => "lineContains",
            Self::WordStartsWith(_) => "wordStartsWith",
        }
    }

    /// The text the rule matched, without the trailing space some carry.
    pub fn pattern(&self) -> &'static str {
        match self {
            Self::NameContains(p)
            | Self::NameStartsWith(p)
            | Self::LineStartsWith(p)
            | Self::LineContains(p)
            | Self::WordStartsWith(p) => p.trim_end(),
        }
    }
}

/// The first of the filter's rules `command` matches, in the engine's order
/// (name first, then the whole line), or `None` when the command gets
/// through. The name is the first word, as `Cmd_Argv(0)` sees it.
///
/// The name stage is case-insensitive in the engine. Whether the line stage
/// is was not read, so it is matched case-insensitively too: a false "this is
/// dropped" costs a glance, a missed one costs a clip.
pub fn demo_filter_rule(command: &str) -> Option<DemoFilterRule> {
    let line = command.trim().to_ascii_lowercase();
    let name = line.split_whitespace().next()?.trim_matches('"');

    if let Some(p) = DEMO_FILTER_NAME_CONTAINS.iter().find(|p| name.contains(*p)) {
        return Some(DemoFilterRule::NameContains(p));
    }
    if let Some(p) = DEMO_FILTER_NAME_STARTS_WITH
        .iter()
        .find(|p| name.starts_with(*p))
    {
        return Some(DemoFilterRule::NameStartsWith(p));
    }
    if let Some(p) = DEMO_FILTER_LINE_STARTS_WITH
        .iter()
        .find(|p| line.starts_with(*p))
    {
        return Some(DemoFilterRule::LineStartsWith(p));
    }
    if let Some(p) = DEMO_FILTER_LINE_CONTAINS.iter().find(|p| line.contains(*p)) {
        return Some(DemoFilterRule::LineContains(p));
    }
    let starts_a_word = |p: &str| {
        line.match_indices(p)
            .any(|(i, _)| i == 0 || matches!(line.as_bytes()[i - 1], b' ' | b';' | b'\n'))
    };
    DEMO_FILTER_WORD_STARTS_WITH
        .iter()
        .find(|p| starts_a_word(p))
        .map(|p| DemoFilterRule::WordStartsWith(p))
}

/// A command the demo filter drops, and the rule that drops it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoFiltered {
    /// The whole command, trimmed.
    pub command: String,
    pub rule: DemoFilterRule,
}

/// Every command in `commands` the demo filter drops, in order. Applies to
/// Initial and Scheduled Commands alike: both reach the game as demo frames.
pub fn demo_filtered_commands(commands: &[String]) -> Vec<DemoFiltered> {
    commands
        .iter()
        .filter_map(|raw| {
            demo_filter_rule(raw).map(|rule| DemoFiltered {
                command: raw.trim().to_string(),
                rule,
            })
        })
        .collect()
}

/// Commands that do nothing specifically in Initial Commands (or a config
/// the engine executes) — not because the engine drops them, but because the
/// pipeline itself always overwrites the value before anything downstream
/// could read it. Scheduling one instead is a different, genuinely dangerous
/// story — see `SCHEDULED_BANNED_COMMANDS`.
/// `dodstudio_run_in_background` is turned on by every batch after the
/// user's own commands.
pub const NOOP_IN_INIT_COMMANDS: &[&str] = &["mirv_movie_filename", "dodstudio_run_in_background"];

/// `NOOP_IN_INIT_COMMANDS` checked against Initial Commands. What the demo
/// filter drops is reported separately, by `demo_filtered_commands`.
pub fn noop_commands_in_init(commands: &[String]) -> Vec<(String, String)> {
    commands_matching(NOOP_IN_INIT_COMMANDS, commands)
}

/// Commands from `SCHEDULED_BANNED_COMMANDS` present in `commands`. Only
/// meaningful against Scheduled Commands — these three are exactly how the
/// decal flush is meant to be configured when used as Initial Commands, so
/// callers must not run this against `init_commands`.
pub fn scheduled_banned_commands(commands: &[String]) -> Vec<(String, String)> {
    commands_matching(SCHEDULED_BANNED_COMMANDS, commands)
}

/// What a list of commands leaves a cvar set to, last one winning.
pub fn effective_in(commands: &[String], cvar: &str) -> Option<String> {
    commands.iter().rev().find_map(|raw| {
        let trimmed = raw.trim();
        let mut parts = trimmed.split_whitespace();
        let head = parts.next()?;
        let value = parts.next()?;
        if parts.next().is_some() || !head.eq_ignore_ascii_case(cvar) {
            return None;
        }
        Some(unquote(value))
    })
}

/// The cvar a command assigns to, if it assigns to one at all.
pub fn assigned_cvar(command: &str) -> Option<(String, String)> {
    let trimmed = command.trim();
    let mut parts = trimmed.split_whitespace();
    let head = parts.next()?;
    let value = parts.next()?;
    if parts.next().is_some() || !is_cvar_name(head) {
        return None;
    }
    Some((head.to_string(), unquote(value)))
}

/// Scan a mod folder's configs. Never writes anything.
///
/// `game_dir` is the folder holding the configs — for DoD, `<hl.exe dir>/dod`.
pub fn scan(game_dir: &Path) -> CfgScan {
    let mut out = CfgScan::default();
    if !game_dir.is_dir() {
        return out;
    }

    let mut visited: HashSet<PathBuf> = HashSet::new();
    for entry in ENTRY_POINTS {
        let path = game_dir.join(entry);
        if path.is_file() {
            read_config(&path, game_dir, 0, &mut visited, &mut out, true);
        }
    }

    // Configs sitting in the folder that nothing reached.
    if let Ok(dir) = std::fs::read_dir(game_dir) {
        let mut loose: Vec<PathBuf> = dir
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.is_file()
                    && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("cfg"))
                    && !visited.contains(&normalise(p))
            })
            .collect();
        loose.sort();
        out.unreferenced = loose;
    }

    out
}

/// `scan`, reading each folder at most once per process.
///
/// The pipeline asks per demo — once to resolve the capture FOV and once to
/// report — so a batch of forty demos would otherwise re-read and re-parse the
/// same configs eighty times. Configs do not change mid-batch, and if the user
/// edits one the next run picks it up.
pub fn scan_cached(game_dir: &Path) -> std::sync::Arc<CfgScan> {
    static CACHE: std::sync::OnceLock<
        std::sync::RwLock<std::collections::HashMap<PathBuf, std::sync::Arc<CfgScan>>>,
    > = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let key = normalise(game_dir);

    if let Ok(read) = cache.read()
        && let Some(hit) = read.get(&key)
    {
        return std::sync::Arc::clone(hit);
    }

    let scanned = std::sync::Arc::new(scan(game_dir));
    if let Ok(mut write) = cache.write() {
        // Another thread may have inserted while this one was scanning. Either
        // result is correct; keeping the stored one keeps them all identical.
        return std::sync::Arc::clone(write.entry(key).or_insert(scanned));
    }
    scanned
}

/// `dirs::canonicalize` would resolve symlinks and fail on missing files; all
/// this needs is a stable key so an `exec` cycle is recognised.
fn normalise(path: &Path) -> PathBuf {
    PathBuf::from(path.to_string_lossy().to_lowercase().replace('\\', "/"))
}

fn read_config(
    path: &Path,
    game_dir: &Path,
    depth: usize,
    visited: &mut HashSet<PathBuf>,
    out: &mut CfgScan,
    auto_executed: bool,
) {
    if depth > MAX_DEPTH || out.files_read >= MAX_FILES {
        return;
    }
    if !visited.insert(normalise(path)) {
        return;
    }
    if std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) > MAX_FILE_BYTES {
        return;
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    out.files_read += 1;

    for (index, raw) in text.lines().enumerate() {
        for command in commands_in(raw) {
            let mut parts = command.split_whitespace();
            let Some(head) = parts.next() else { continue };
            let head_lower = head.to_lowercase();

            if NOT_EXECUTED.contains(&head_lower.as_str()) {
                continue;
            }

            if head_lower == "exec" {
                if let Some(target) = parts.next() {
                    let name = unquote(target);
                    // Config paths are relative to the mod folder, and a config
                    // is not a place to accept a path that climbs out of it.
                    if name.contains("..") || name.starts_with('/') || name.starts_with('\\') {
                        continue;
                    }
                    let next = game_dir.join(&name);
                    let next = if next.extension().is_none() {
                        next.with_extension("cfg")
                    } else {
                        next
                    };
                    if next.is_file() {
                        read_config(&next, game_dir, depth + 1, visited, out, auto_executed);
                    }
                }
                continue;
            }

            // Every assignment is recorded, not just the ones the pipeline
            // reads, so an init command can be checked against whatever the
            // user actually has — most people's configs do not mention
            // `r_decals` at all, and the ones that surprise you are the ones
            // nobody thought to watch for.
            if out.settings.len() >= MAX_SETTINGS || !is_cvar_name(head) {
                continue;
            }
            let Some(value) = parts.next() else {
                // No argument is a command, not an assignment: `+mlook`,
                // `stopsound`, `clear`.
                continue;
            };
            if parts.next().is_some() {
                // More than one argument is a sub-command, not an assignment —
                // `mirv_fov handleZoom enabled 1` appears in real movie configs
                // and sets no FOV.
                continue;
            }
            out.settings.push(CvarSetting {
                cvar: head.to_string(),
                value: unquote(value),
                file: path.to_path_buf(),
                line: index + 1,
                auto_executed,
            });
        }
    }
}

/// Split one config line into the commands the engine would run: comments
/// stripped, `;` separating commands, and semicolons inside quotes left alone.
fn commands_in(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                current.push(c);
            }
            '/' if !in_quotes && chars.peek() == Some(&'/') => break,
            ';' if !in_quotes => {
                out.push(std::mem::take(&mut current));
            }
            _ => current.push(c),
        }
    }
    out.push(current);

    out.into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Whether a token reads as a cvar name rather than a console verb.
///
/// `+mlook` and `-attack` are commands with a sign prefix, never assignments.
fn is_cvar_name(token: &str) -> bool {
    let mut chars = token.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub(crate) fn unquote(token: &str) -> String {
    token.trim().trim_matches('"').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Scratch;

    fn scratch(tag: &str) -> Scratch {
        Scratch::new(format_args!("cfg_scan_{tag}"))
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
                "r_drawentities",
                "cl_lw",
            ]
        );
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
}
