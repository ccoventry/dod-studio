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
    /// Lines that use a hook console name from before #555 (`dodstudio_*`),
    /// which the game no longer knows. Reported, never rewritten.
    pub renamed: Vec<RenamedConsoleName>,
}

/// A config line using a console name #555 renamed: the old name as written,
/// the name that replaced it, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenamedConsoleName {
    pub old: String,
    pub new: String,
    pub file: PathBuf,
    pub line: usize,
}

impl RenamedConsoleName {
    pub fn file_name(&self) -> String {
        display_name(&self.file)
    }
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
        self.settings.is_empty() && self.unreferenced.is_empty() && self.renamed.is_empty()
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
    /// A line in a file an Initial Command `exec`s (Seek Between Clips only:
    /// the demo filter drops the `exec` in a patched batch).
    Exec { file: PathBuf, line: usize },
    /// A Scheduled Command, `offset_seconds` before each highlight.
    ScheduledBefore { offset_seconds: f32 },
    /// A Scheduled Command, `offset_seconds` after each highlight.
    ScheduledAfter { offset_seconds: f32 },
}

impl ValueSource {
    pub fn is_config(&self) -> bool {
        matches!(self, ValueSource::Config { .. })
    }

    /// A line in one of the user's own files: a config, or a file an
    /// Initial Command execs.
    pub fn is_users_file(&self) -> bool {
        matches!(self, ValueSource::Config { .. } | ValueSource::Exec { .. })
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
    value_warnings_with_execs(scan, init_commands, user_count, scheduled, &[])
}

/// [`value_warnings`], with each Initial Command `exec` that `execs` has
/// followed (`follow_initial_exec`) adding its file's settings where the
/// `exec` stands. They count as the user's files, like configs: a conflict
/// made only of config and exec'd-file lines is not reported, for the same
/// `echo` noise reason, but one with an Initial, app or Scheduled value is.
pub fn value_warnings_with_execs(
    scan: &CfgScan,
    init_commands: &[String],
    user_count: usize,
    scheduled: &[ScheduledCommand],
    execs: &[InitialExec],
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
        // `exec x.cfg` reads as an assignment too; it is a file to follow.
        if exec_target(command).is_none()
            && let Some((cvar, value)) = assigned_cvar(command)
        {
            let source = if index < user_count {
                ValueSource::Initial
            } else {
                ValueSource::App
            };
            add(&cvar, value, source);
        } else if let Some(followed) = execs.iter().find(|e| e.command == command.trim()) {
            for s in &followed.settings {
                add(
                    &s.cvar,
                    s.value.clone(),
                    ValueSource::Exec {
                        file: s.file.clone(),
                        line: s.line,
                    },
                );
            }
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
        #[allow(
            clippy::unwrap_used,
            clippy::expect_used,
            reason = "a pool entry is created with its first value"
        )]
        let effective = values.last().expect("a pool entry has a value").clone();
        let disagree = values
            .iter()
            .any(|v| !same_value(&v.value, &effective.value));
        let app_side = values.iter().any(|v| !v.source.is_users_file());
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
        #[allow(
            clippy::unwrap_used,
            clippy::expect_used,
            reason = "a pool entry is created with its first value"
        )]
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
/// itself (recording pins its own timestep regardless). `r_norefresh` is the
/// same pair's other switch (#724): the fast-forward draws nothing, and the
/// pre-roll turns drawing back on; a scheduled `1` inside a clip records
/// frames with no world in them.
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
    "r_norefresh",
    "mirv_agr",
];

/// The cvars `sys_fast_forward` sets to run uncapped between clips and
/// `sys_normal_speed` puts back at every pre-roll (#724; the values are
/// `builder::speed`'s). Not a tier of their own: in Initial Commands they are
/// honoured exactly — that value is what every clip gets back — and scheduled
/// they are reported with `mid_demo_hazards`, because a scheduled one lasts
/// only until the next clip's pre-roll restores the Initial value.
pub const FAST_FORWARD_CVARS: &[&str] = &["fps_override", "fps_max", "gl_vsync"];

/// Commands refused wherever a command can be typed (Initial Commands and
/// Scheduled Commands alike), not merely shadowed or flagged as a mid-demo
/// hazard the way the rest of `MID_DEMO_HAZARDS` is. Two different reasons
/// land a command here:
///
///   * **the pipeline owns it outright** — no dedicated setting exists, and no
///     scenario has been found where a user typing one is anything but a
///     misunderstanding (`mirv_recordmovie_start`/`_stop`, `mirv_movie_ffmpeg`,
///     `host_framerate`, `r_norefresh`);
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
/// - `r_norefresh` — the same pair's drawing switch (#724): `sys_fast_forward`
///   sets it so the fast-forward skips the 3D pass, `sys_normal_speed` clears
///   it at every pre-roll. A user's `1` is either undone before any clip
///   records (Initial Commands) or records clips with no world drawn
///   (scheduled); there is no value of it a capture can want.
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
/// - `ds_capture_batch` / `ds_capture_schedule` — the hook DLL's runners for a
///   batch run without patched demos (#434). One started from a batch's own
///   commands would replace the batch that is running.
/// - `ds_player_spray` — writes the game's own spray file (`tempdecal.wad`)
///   (#428). A demo draws the sprays it recorded (or Configuration > Players'),
///   never that file, so from a batch it would change the user's spray on
///   disk and nothing on screen.
pub const BANNED_COMMANDS: &[&str] = &[
    "mirv_recordmovie_start",
    "mirv_recordmovie_stop",
    "mirv_movie_ffmpeg",
    "host_framerate",
    "r_norefresh",
    "ds_capture_batch",
    "ds_capture_schedule",
    "ds_player_spray",
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

/// Commands from `MID_DEMO_HAZARDS` that must not be run during playback, and
/// the `FAST_FORWARD_CVARS` the next pre-roll would put back. Only meaningful
/// against Scheduled Commands: the latter are honoured in Initial Commands.
pub fn mid_demo_hazards(commands: &[String]) -> Vec<(String, String)> {
    let mut hits = commands_matching(MID_DEMO_HAZARDS, commands);
    hits.extend(commands_matching(FAST_FORWARD_CVARS, commands));
    hits
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
///
/// `dem_speed` (#831) is the demo player's speed, which the pipeline sets
/// itself: Fast-forward gaps between kills schedules `dem_speed <N>` and
/// `dem_speed 1` around each long gap (`builder::kill_gaps`). A scheduled one
/// would fight those, and one that outlasts its clip speeds up the next
/// clip's whole recording. In Initial Commands it does nothing harmful: the
/// hook's batch puts the speed back to 1 after each demo's init lines.
pub const SCHEDULED_BANNED_COMMANDS: &[&str] = &[
    "r_decals",
    "mirv_fov",
    "gl_widescreenfov",
    "mirv_movie_filename",
    "mirv_agr",
    "dem_speed",
];

// ── The demo command filter (#679) ──────────────────────────────────────────
//
// Every type-3 `ConsoleCommand` frame a demo carries goes through the same
// filter as `svc_stufftext` before it reaches the command buffer (`hw+0x1dce0`
// pre-Anniversary, `hw+0x1aaa30` on the 25th Anniversary build;
// `docs/goldsrc_hw_dll_survey.md` §13.3). A command it matches is dropped with
// nothing printed. A patched batch (Seek Between Clips off, or a batch that
// can't seek) writes Initial and Scheduled Commands into the demo as
// `ConsoleCommand` frames (`engine.rs`), so both are filtered there. A Seek
// Between Clips batch doesn't: the hook DLL runs both lists through
// `ClientCmd` (`runtime_batch.rs`, goldsrc-hooks `batch.rs`), which skips the
// filter, so the Commands tab reports this tier only while that setting is
// off. A config the engine execs at start-up is a different path and is not
// filtered either. The lists below are the filter's own, read offline from
// `hw.dll`.
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
/// Initial and Scheduled Commands alike, in a patched batch only: there both
/// reach the game as demo frames.
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
/// `ds_capture_run_in_background` and `ds_capture_stay_behind` are set by every
/// batch after the user's own commands, the second from
/// `PatcherConfig::keep_game_behind`.
pub const NOOP_IN_INIT_COMMANDS: &[&str] = &[
    "mirv_movie_filename",
    "ds_capture_run_in_background",
    "ds_capture_stay_behind",
];

/// `NOOP_IN_INIT_COMMANDS` checked against Initial Commands. What the demo
/// filter drops is reported separately, by `demo_filtered_commands`.
pub fn noop_commands_in_init(commands: &[String]) -> Vec<(String, String)> {
    commands_matching(NOOP_IN_INIT_COMMANDS, commands)
}

/// Hook console names from before #555 in Initial or Scheduled Commands, as
/// (old name, new name, whole trimmed command). Studio moves its saved lists
/// to the new names when settings load, so this only finds one typed or
/// pasted since. Reported as doing nothing, like `NOOP_IN_INIT_COMMANDS`: the
/// game no longer knows the name.
pub fn renamed_commands(commands: &[String]) -> Vec<(String, String, String)> {
    commands
        .iter()
        .flat_map(|raw| {
            let trimmed = raw.trim();
            super::console_prefix::old_names_in(trimmed)
                .into_iter()
                .map(move |(old, new)| (old, new, trimmed.to_string()))
        })
        .collect()
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
    let dirs = [game_dir.to_path_buf()];
    for entry in ENTRY_POINTS {
        let path = game_dir.join(entry);
        if path.is_file() {
            read_config(&path, &dirs, 0, &mut visited, &mut out, true);
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

// ── Following an Initial Command's `exec` ────────────────────────────────────
//
// In a Seek Between Clips batch the hook DLL runs Initial Commands through
// `ClientCmd`, which skips the demo filter, so an `exec movie.cfg` there runs
// the file. Its settings then join the value rules at the place of the `exec`.
// Read-only, like the rest of this module.

/// The folders the game looks in for a file an `exec` names, in order.
/// `Cmd_Exec_f` (both builds; `hw+0x1b5bde` on the 25th Anniversary one)
/// opens the file under the `GAMECONFIG` path ID, which is the game folder
/// alone, then `GAME`, which adds the `_addon` folder (every game Studio
/// starts has `-addons` on its launch line, `types.rs`), then every search
/// path, which adds `_downloads` (`GAMEDOWNLOAD`). So the game folder's copy
/// wins over `_addon`'s. The `valve` fallback folders are not modelled.
pub fn exec_search_dirs(game_dir: &Path) -> Vec<PathBuf> {
    let Some(name) = game_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
    else {
        return vec![game_dir.to_path_buf()];
    };
    let sibling = |suffix: &str| game_dir.with_file_name(format!("{name}{suffix}"));
    vec![
        game_dir.to_path_buf(),
        sibling("_addon"),
        sibling("_downloads"),
    ]
}

/// The file an `exec` command names, or `None` when `command` is not one.
pub fn exec_target(command: &str) -> Option<String> {
    let trimmed = command.trim();
    let head = trimmed.split_whitespace().next()?;
    if !head.eq_ignore_ascii_case("exec") {
        return None;
    }
    let rest = trimmed[head.len()..].trim_start();
    // A quoted name may hold spaces, as the console's tokenizer allows.
    let target = match rest.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next().unwrap_or(""),
        None => rest.split_whitespace().next().unwrap_or(""),
    };
    Some(target.trim().to_string()).filter(|t| !t.is_empty())
}

/// An Initial Command `exec <file>`, followed into the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitialExec {
    /// The command, trimmed.
    pub command: String,
    /// The file the game would run, or `None` when the game refuses the name
    /// or no folder in [`exec_search_dirs`] has it ("couldn't exec").
    pub file: Option<PathBuf>,
    /// Every assignment in it, and in the files it execs in turn, in the
    /// order the game runs them. Bounded like the start-up scan: depth, file
    /// count and size, and each file read once, so an exec loop ends.
    pub settings: Vec<CvarSetting>,
}

/// Follows `command` when it is an `exec`, reading only. `game_dir` is the
/// game folder (`<hl.exe dir>/dod`).
pub fn follow_initial_exec(game_dir: &Path, command: &str) -> Option<InitialExec> {
    let target = exec_target(command)?;
    let dirs = exec_search_dirs(game_dir);
    let file = resolve_exec(&dirs, &target);
    let mut out = CfgScan::default();
    if let Some(path) = &file {
        read_config(path, &dirs, 0, &mut HashSet::new(), &mut out, true);
    }
    Some(InitialExec {
        command: command.trim().to_string(),
        file,
        settings: out.settings,
    })
}

/// `dirs::canonicalize` would resolve symlinks and fail on missing files; all
/// this needs is a stable key so an `exec` cycle is recognised.
fn normalise(path: &Path) -> PathBuf {
    PathBuf::from(path.to_string_lossy().to_lowercase().replace('\\', "/"))
}

/// The file an `exec` line names, looked for in `dirs` in order, or `None`
/// when the game would not run one. `Cmd_Exec_f`'s own checks come first:
/// it refuses a name containing a backslash, `:`, `~` or `..`, or starting
/// with `/` ("invalid path"), one with more than one `.` ("invalid
/// filename"), and one not ending in `.cfg` or `.rc` ("not a .cfg or .rc
/// file"); it adds no extension.
fn resolve_exec(dirs: &[PathBuf], target: &str) -> Option<PathBuf> {
    let name = unquote(target);
    if name.is_empty()
        || ['\\', ':', '~'].iter().any(|c| name.contains(*c))
        || name.contains("..")
        || name.starts_with('/')
        || name.matches('.').count() > 1
    {
        return None;
    }
    let ext = name.rsplit_once('.').map(|(_, ext)| ext)?;
    if !ext.eq_ignore_ascii_case("cfg") && !ext.eq_ignore_ascii_case("rc") {
        return None;
    }
    dirs.iter().find_map(|dir| {
        let next = dir.join(&name);
        next.is_file().then_some(next)
    })
}

/// `dirs` are the folders an `exec` inside the file is looked for in.
fn read_config(
    path: &Path,
    dirs: &[PathBuf],
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

            // Ahead of the bind/alias skip: an old name in a bind fails just
            // the same, only later, when the key is pressed.
            for (old, new) in super::console_prefix::old_names_in(&command) {
                if out.renamed.len() < MAX_SETTINGS {
                    out.renamed.push(RenamedConsoleName {
                        old,
                        new,
                        file: path.to_path_buf(),
                        line: index + 1,
                    });
                }
            }

            if NOT_EXECUTED.contains(&head_lower.as_str()) {
                continue;
            }

            if head_lower == "exec" {
                if let Some(next) = parts.next().and_then(|t| resolve_exec(dirs, t)) {
                    read_config(&next, dirs, depth + 1, visited, out, auto_executed);
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
#[path = "cfg_scan_tests.rs"]
mod tests;
