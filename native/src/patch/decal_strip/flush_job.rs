// patch/decal_strip/flush_job.rs
// ── Batch-pipeline pre-pass ──────────────────────────────────────────────────
//
// `StreamPatcher::patch` streams its input as a file, while `clean_demo_decals`
// is a whole-file parse and rewrite — the two cannot share a buffer. So the
// flush runs ahead of the patch, writing cleaned bytes to a scratch demo that
// the patch then streams from. It lives inside `patch()` rather than at its
// call sites (the CLI, `spawn_patch_batch`, and two in `capture_manager`) so
// those cannot drift apart on whether decals get cleaned.

use super::{
    DECALS_PER_POSITION, DecalCleanError, DecalCleanOptions, DecalCleanStats, FlushSource,
    VisibilityBasis, clean_demo_decals,
};
use crate::patch::types::{PatchJob, PatcherConfig};
use crate::patch::{cfg_scan, decal_atlas};

/// A cleaned copy of a source demo, alive only as long as the patch reading it.
/// Removed on drop, which covers the error and cancellation paths as well as
/// the ordinary one.
pub struct CleanedSource {
    path: std::path::PathBuf,
}

impl CleanedSource {
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for CleanedSource {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Scratch demos left behind by a process that was killed rather than unwound.
///
/// Drop covers the error and cancellation paths, but nothing runs when a
/// process is terminated outright, and each of these is the size of a demo —
/// tens or hundreds of megabytes. Swept on the way past rather than tracked,
/// since there is nowhere durable to track them.
///
/// Age-gated so a capture running concurrently in another process cannot have
/// its scratch deleted out from under it. No flush takes anything like this
/// long; the margin is for a machine that was asleep mid-run.
const SCRATCH_STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);

/// Filename prefix identifying a flush scratch demo, so the sweep can recognise
/// its own leavings and nothing else.
const SCRATCH_PREFIX: &str = "dodstudio_decalflush_";

fn sweep_stale_scratch(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(SCRATCH_PREFIX)
        {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .map(|age| age > SCRATCH_STALE_AFTER)
            .unwrap_or(false);
        if stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Scratch path for one cleaned demo. Kept in the system temp directory rather
/// than beside the output: the capture directories are scanned for takes and
/// swept by the auto-clear passes, neither of which should ever see this file.
fn scratch_path(source_demo: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEQ: AtomicU32 = AtomicU32::new(0);

    let stem = std::path::Path::new(source_demo)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "demo".to_string());

    let dir = std::env::temp_dir();
    sweep_stale_scratch(&dir);
    dir.join(format!(
        "{}{}_{}_{}.dem",
        SCRATCH_PREFIX,
        stem,
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Extra half-angle allowed beyond the frame's own corner.
///
/// Cameras are sampled every fourth in-window frame, so a fast turn can carry a
/// spot into shot between two samples and never be seen by the test. This is
/// the margin against that, and against the FOV in effect differing slightly
/// from the one configured.
const CAMERA_TURN_MARGIN_DEGREES: f32 = 6.0;

/// Half-angle off the view axis that must be treated as on screen, for a given
/// capture FOV and frame shape.
///
/// A cone is a crude stand-in for a frustum, and the part that decides the
/// answer is the CORNER of the frame, not its edge. At 105 degrees horizontal
/// on a 16:9 frame a decal can sit ~56 degrees off the view axis and still be
/// in shot; the horizontal half-angle alone would say 52.5, and the 40 degrees
/// this code used before — justified as "generous" against a 90-degree FOV —
/// was under even the horizontal half-angle of 45. It was rejecting nothing
/// between 40 and 49 degrees that a 90-degree capture genuinely showed, and
/// nothing between 40 and 56 at 105.
///
/// Erring wide only costs candidate positions. Erring narrow puts a decal on
/// screen during a recorded clip, which is the one defect this pass exists to
/// avoid, so the bias here is deliberate and one-directional.
pub fn on_screen_half_angle(fov_degrees: f32, width: i32, height: i32) -> f32 {
    let fov = fov_degrees.clamp(1.0, 179.0);
    let aspect = if width > 0 && height > 0 {
        width as f32 / height as f32
    } else {
        4.0 / 3.0
    };

    let tan_h = (fov.to_radians() / 2.0).tan();
    let tan_v = tan_h / aspect.max(0.01);
    let corner = (tan_h * tan_h + tan_v * tan_v).sqrt().atan().to_degrees();

    (corner + CAMERA_TURN_MARGIN_DEGREES).min(89.0)
}

/// The FOV a capture will actually run at.
///
/// Read out of `init_commands` when HLAE's `mirv_fov` is among them, because
/// that is the value the engine will be using and a separate setting could
/// silently disagree with it. The last one wins, matching the console. Falls
/// back to the configured default when nothing sets it.
pub fn capture_fov(config: &PatcherConfig) -> f32 {
    capture_fov_from_init(&config.init_commands).unwrap_or(config.capture_fov)
}

/// The FOV `init_commands` states, if it states one at all.
///
/// Separate from `capture_fov` because "nothing was stated" and "the stated
/// value happens to equal the default" are different facts, and collapsing them
/// into one number loses the difference. `capture_fov_resolved` needs it: the
/// app's own seeded default is `mirv_fov 90`, so a user who takes the defaults
/// and has `mirv_fov 105` in a config would otherwise have their explicit 90
/// silently replaced by the config's 105.
pub fn capture_fov_from_init(init_commands: &[String]) -> Option<f32> {
    for cmd in init_commands.iter().rev() {
        let trimmed = cmd.trim();
        let Some(rest) = trimmed
            .strip_prefix("mirv_fov")
            .or_else(|| trimmed.strip_prefix("default_fov"))
        else {
            continue;
        };
        // Guard against matching a longer command that merely starts the same.
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        // Real .cfg syntax quotes every value ("105", not 105) — the same
        // convention "Load from .cfg file…" carries straight into Initial
        // Commands. Parsing the raw token instead of the unquoted value
        // means this silently reads a quoted line as "nothing stated" and
        // falls through to the default, which is the whole bug this exists
        // to prevent.
        if let Ok(v) = cfg_scan::unquote(rest.trim()).parse::<f32>()
            && v > 0.0
        {
            return Some(v);
        }
    }
    None
}

/// The mod folder holding the game's configs, for a configured `hl.exe`.
fn game_dir_for(config: &PatcherConfig) -> Option<std::path::PathBuf> {
    let dir = std::path::Path::new(&config.game_path)
        .parent()?
        .join("dod");
    dir.is_dir().then_some(dir)
}

/// The FOV a capture will actually run at, including what the game's own
/// configs set.
///
/// `capture_fov` reads the app's init commands, which was the whole story only
/// as long as nothing else set it. It is not: a `config.cfg` ending in
/// `exec movie.cfg`, with `mirv_fov 105` inside, renders at 105 while the
/// pipeline sizes its on-screen test for the configured default — a cone some
/// seven degrees too narrow, calling in-shot positions hidden.
///
/// So the order is init commands, then an executed config, then the configured
/// default. `ring_limit` (r_decals) now follows exactly this same order —
/// it used to be the one deliberate exception, on the reasoning that adopting
/// a `movie.cfg`'s `r_decals 0` would silently stand the flush down. That
/// objection is gone now that a resolved 0 with Flush Decals on is its own
/// loud, reported fact (`decal_flush_is_noop` in the studio report)
/// rather than something this function would have hidden by disagreeing with
/// the config. User-requested symmetry, 2026-09-05.
///
/// Reads config files. Never writes one.
pub fn capture_fov_resolved(config: &PatcherConfig) -> f32 {
    // An init command is the app's own statement and outranks a config —
    // including when it states the same number the default happens to be. The
    // seeded default IS `mirv_fov 90`, so treating that as "nothing was said"
    // would hand every defaulted install over to whatever its movie.cfg says.
    if let Some(stated) = capture_fov_from_init(&config.init_commands) {
        return stated;
    }

    let Some(dir) = game_dir_for(config) else {
        return config.capture_fov;
    };
    let scan = cfg_scan::scan_cached(&dir);
    for cvar in ["mirv_fov", "default_fov"] {
        if let Some(setting) = scan.effective(cvar)
            && let Ok(v) = setting.value.parse::<f32>()
            && v > 0.0
        {
            return v;
        }
    }
    config.capture_fov
}

/// Report anything the game's configs set that the pipeline depends on.
///
/// Read-only, and advisory: the user's configs are theirs to change. This only
/// makes sure a value the app never hears about does not stay invisible.
fn warn_about_game_cfgs(config: &PatcherConfig) {
    let Some(dir) = game_dir_for(config) else {
        return;
    };
    let scan = cfg_scan::scan_cached(&dir);
    let found = scan.effective_settings();
    if found.is_empty() {
        return;
    }

    // Once per folder per run. This is called per demo, and a batch of forty
    // would otherwise write the same paragraph into the activity log forty
    // times — which is how a warning worth reading becomes one nobody reads.
    {
        static WARNED: std::sync::OnceLock<
            std::sync::RwLock<std::collections::HashSet<std::path::PathBuf>>,
        > = std::sync::OnceLock::new();
        let warned = WARNED.get_or_init(Default::default);
        let Ok(mut w) = warned.write() else {
            return;
        };
        if !w.insert(dir.clone()) {
            return;
        }
    }

    let lines: Vec<String> = found
        .iter()
        .map(|s| {
            format!(
                "`{} {}` in `{}` line {}",
                s.cvar,
                s.value,
                s.file_name(),
                s.line
            )
        })
        .collect();

    crate::log_markdown(&format!(
        "⚠️ **The game's own configs set values this pipeline depends on**: {}. Nothing here \
         changes them — they are yours. But `r_decals` decides how many decals the engine keeps, \
         and the sweep is sized to it; `mirv_fov` decides what counts as on screen. Stating them \
         in the app's Init Commands instead puts them where the pipeline can see them. The FOV \
         above has been used for this capture; `r_decals` has not, because the pipeline's own pin \
         runs after the config and would override it anyway.",
        lines.join(", ")
    ));
}

/// `r_decals` as stated in `init_commands`, if it is stated there at all.
///
/// The last one wins, matching the console. Clamped to the engine's own
/// ceiling, because the engine clamps it too and a sweep sized past the ceiling
/// would spend its extra positions turning a ring that had already come round.
pub fn ring_limit_from_init(init_commands: &[String]) -> Option<u32> {
    for cmd in init_commands.iter().rev() {
        let trimmed = cmd.trim();
        let Some(rest) = trimmed.strip_prefix("r_decals") else {
            continue;
        };
        // Guard against matching a longer command that merely starts the same.
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        // Quoted, same as capture_fov_from_init above and for the same
        // reason: real .cfg syntax quotes every value, and a raw parse on
        // `"512"` fails silently, reading a stated line as unstated.
        if let Ok(v) = cfg_scan::unquote(rest.trim()).parse::<u32>() {
            return Some(v.min(crate::patch::MAX_RENDER_DECALS));
        }
    }
    None
}

/// How many decals the ring will hold, and so how many positions a full sweep
/// has to turn.
///
/// This is one number, and `r_decals` is where the engine reads it. A separate
/// setting could only ever agree with the cvar or silently disagree with it, so
/// an `init_commands` entry is the authority when there is one, an executed
/// config is next, and the configured default fills in when neither states it.
/// Same precedence as `capture_fov_resolved` — genuinely the same rule now,
/// not merely similar (see that function's doc comment for the history of why
/// `r_decals` used to be the one exception).
pub fn ring_limit(config: &PatcherConfig) -> u32 {
    ring_limit_from_init(&config.init_commands)
        .or_else(|| ring_limit_from_game_config(config))
        .unwrap_or_else(|| config.decal_ring_limit.min(crate::patch::MAX_RENDER_DECALS))
}

/// `r_decals` as an executed config states it, if one does and the value
/// actually parses — same shape as `ring_limit_from_init`, just for the other
/// place a value can come from. Clamped the same way, for the same reason.
///
/// Read-only, like everything touching the user's configs — this decides what
/// the pipeline treats as the standing value, never what the config says.
/// See [`crate::patch::cfg_scan`].
pub fn ring_limit_from_game_config(config: &PatcherConfig) -> Option<u32> {
    let dir = game_dir_for(config)?;
    let setting = cfg_scan::scan_cached(&dir).effective("r_decals")?.clone();
    setting
        .value
        .parse::<u32>()
        .ok()
        .map(|v| v.min(crate::patch::MAX_RENDER_DECALS))
}

/// Clean options for the batch pipeline, as distinct from the `strip_decals`
/// CLI's.
///
/// The one difference that matters is `inject_r_decals_command`. The CLI has no
/// `init_commands` to pin the cvar from, so it inserts a `ConsoleCommand` frame
/// into the playback entry. In the pipeline that insertion would shift every
/// later frame ordinal by +1 and silently desync `job.scheduled_commands` from
/// `StreamPatcher`'s `frame_counter` — every scheduled command firing a frame
/// late, with nothing in the output bytes to show for it. So here the cvar is
/// pinned from `init_commands` (see `builder`) and no frame is inserted. The
/// burst itself only pushes messages into frames that already exist, so it
/// moves no ordinal and is safe.
/// Where the game keeps its maps, derived from the configured `hl.exe`.
///
/// `game_path` points at the executable, and DoD's content sits beside it, so
/// maps are `<hl.exe dir>/dod/maps`. Returned as `None` when that does not
/// resolve to a real directory rather than handing the loader a path that can
/// only fail per demo.
fn maps_dir_for(config: &PatcherConfig) -> Option<std::path::PathBuf> {
    crate::patch::map_check::maps_dir_for_exe(std::path::Path::new(&config.game_path))
}

fn flush_options(config: &PatcherConfig) -> DecalCleanOptions {
    DecalCleanOptions {
        ring_limit: ring_limit(config),
        inject_r_decals_command: false,
        // The pipeline is the only caller that accumulates a store. The CLI and
        // the probe rig stay self-contained, so a one-off experiment never
        // writes coordinates that a real capture would later rely on.
        atlas_dir: Some(decal_atlas::default_dir()),
        maps_dir: maps_dir_for(config),
        // Derived, never assumed — and looked for everywhere the engine would
        // find it, not just in the app's own init commands. A movie shot at
        // `mirv_fov 105` puts the frame corner ~16 degrees further out than the
        // fixed 40 this used to carry, and that 105 is as likely to be sitting
        // in the user's `movie.cfg` as in the app.
        visibility_cone_degrees: on_screen_half_angle(
            capture_fov_resolved(config),
            config.resolution_width,
            config.resolution_height,
        ),
        // One experiment, reachable from a real capture because the engine is
        // the only thing that can answer it — see `require_pvs_hidden`.
        // Env-gated rather than exposed as a setting: it is a question being
        // asked once, not a mode anyone should be choosing between.
        require_pvs_hidden: std::env::var("DOD_FLUSH_PVS_ONLY").is_ok(),
        // Forces the map-geometry source, which is otherwise unreachable on a
        // map whose coordinate store is already full — see `map_geometry_only`.
        map_geometry_only: std::env::var("DOD_FLUSH_MAP_GEOMETRY_ONLY").is_ok(),
        ..Default::default()
    }
}

/// The frames each of a job's blocks records between, in the frame-ordinal
/// domain that `StreamPatcher`'s `frame_counter` and `job.scheduled_commands`
/// already share.
///
/// All or nothing: `None` unless every block contributes a window. Stripping
/// keys off these, so a block that contributed none would have its own recorded
/// clip treated as outside every window — scrubbing the bullet holes that land
/// during the action. Dirty walls are much the lesser defect.
///
/// A start of 0 means the bounds never got filled in. Equal bounds do not: with
/// no record lead or trail configured, a single-kill highlight genuinely does
/// start and stop recording on one frame. Only an inverted window is rejected —
/// an `r_stop` clamped back to an end-of-demo frame ahead of its own start.
fn keep_windows_for(job: &PatchJob) -> Option<Vec<(i32, i32)>> {
    let windows: Vec<(i32, i32)> = job
        .blocks
        .iter()
        .filter(|b| b.record_start_tick > 0 && b.record_stop_tick >= b.record_start_tick)
        .map(|b| (b.record_start_tick, b.record_stop_tick))
        .collect();

    (windows.len() == job.blocks.len()).then_some(windows)
}

/// Cleans a job's source demo of wall decals outside its recorded clips, and
/// sweeps the decal ring ahead of each one.
///
/// Returns `None` when there is nothing to do, and — deliberately — also when
/// the flush fails. Decal hygiene is cosmetic; losing an entire capture batch
/// over it would not be. Every such path logs loudly first, because this is a
/// feature whose failures are invisible in the output bytes: a demo that was
/// not cleaned patches and records exactly like one that was, and only looks
/// wrong on screen.
pub fn prepare_flushed_source(
    job: &PatchJob,
    config: &PatcherConfig,
    cancel: crate::patch::Cancel<'_>,
) -> Result<Option<CleanedSource>, crate::patch::Cancelled> {
    // First, before the ~110MB read below: a job that has not started its
    // flush when Cancel is pressed should do none of it. With PATCH_CONCURRENCY
    // jobs in flight, this is what keeps the queued ones from each adding
    // another full pass to the wait. See #193.
    if cancel.requested() {
        return Err(crate::patch::Cancelled);
    }

    if !config.decal_flush {
        return Ok(None);
    }

    // `r_decals 0` turns decals off outright. There is then no ring to turn and
    // no bullet hole to clear, and a sweep sized zero would be a burst with
    // nowhere to put anything. Flush Decals Between Clips being on and r_decals
    // being 0 is a real, reachable contradiction — logged the same as every
    // other skip path, since this one is otherwise completely silent.
    if ring_limit(config) == 0 {
        crate::log_markdown(
            "⚠️ **Decal flush skipped** — Flush Decals Between Clips is on, but r_decals \
             resolves to 0 (stated in Initial Commands, or the app's own configured default). \
             There is no ring to sweep. Capture continues; walls will not be cleaned between clips.",
        );
        return Ok(None);
    }

    // The primer job and preview jobs carry no blocks: nothing is being
    // recorded from them, so there is no clip to keep clean.
    if job.blocks.is_empty() {
        return Ok(None);
    }

    warn_about_game_cfgs(config);

    let keep_windows = match keep_windows_for(job) {
        Some(w) => w,
        None => {
            crate::log_markdown(&format!(
                "⚠️ **Decal flush skipped** — not every block in `{}` carries usable record \
                 bounds. Capture continues; walls will not be cleaned between clips.",
                job.source_demo
            ));
            return Ok(None);
        }
    };

    let bytes = match std::fs::read(&job.source_demo) {
        Ok(b) => b,
        Err(e) => {
            crate::log_markdown(&format!(
                "⚠️ **Decal flush skipped** — could not read `{}`: {}",
                job.source_demo, e
            ));
            return Ok(None);
        }
    };

    let opts = flush_options(config);

    let (cleaned, stats) = match clean_demo_decals(&bytes, &keep_windows, &opts, cancel) {
        Ok(v) => v,
        // Not a failure, and deliberately silent: the user asked for the
        // batch to stop. Reporting it as a flush failure would put a warning
        // in the capture log for something they did on purpose.
        Err(DecalCleanError::Cancelled) => return Err(crate::patch::Cancelled),
        Err(e) => {
            crate::log_markdown(&format!(
                "⚠️ **Decal flush failed** on `{}`: {}. Capture continues with the unmodified \
                 demo; walls will not be cleaned between clips.",
                job.source_demo, e
            ));
            return Ok(None);
        }
    };
    drop(bytes);

    // Last chance before spending a ~110MB write on output about to be thrown
    // away. After this the scratch file exists and `CleanedSource`'s own Drop
    // owns removing it, so the copy loop in `engine.rs` -- which checks the
    // same token once per frame -- is the right place for any later check.
    if cancel.requested() {
        return Err(crate::patch::Cancelled);
    }

    let path = scratch_path(&job.source_demo);
    if let Err(e) = std::fs::write(&path, &cleaned) {
        crate::log_markdown(&format!(
            "⚠️ **Decal flush skipped** — could not write scratch demo `{}`: {}",
            path.display(),
            e
        ));
        return Ok(None);
    }

    report(job, &stats, &keep_windows, &opts);
    Ok(Some(CleanedSource { path }))
}

/// Writes the flush result to the capture log. The counts are informational;
/// the warnings below are not — each marks a way the sweep can come out
/// structurally correct and still be wrong on screen.
fn report(
    job: &PatchJob,
    stats: &DecalCleanStats,
    keep_windows: &[(i32, i32)],
    opts: &DecalCleanOptions,
) {
    // The source is on the main line, not just in the warning below it: a
    // sweep anchored on tiled planes and one anchored on a computed floor point
    // produce identical counts and completely different odds of working.
    let source = match stats.flush_source {
        Some(FlushSource::TiledPlane) => "tiled planes",
        Some(FlushSource::MapGeometry) => "the map's own geometry",
        Some(FlushSource::MapAtlas) => "the map coordinate store",
        Some(FlushSource::HarvestedNearSpawn) => "harvested decals",
        Some(FlushSource::PlayerFloorPath) => "floor under the player's path",
        Some(FlushSource::ComputedSpawnFloor) => "computed spawn floor",
        Some(FlushSource::Override) => "caller override",
        None => "none",
    };

    // Say so loudly. A run under the experiment gate is not a normal capture,
    // and a log that does not mention it is a log someone will later read as
    // evidence about the shipped behaviour.
    if opts.require_pvs_hidden {
        crate::log_markdown(
            "🧪 **EXPERIMENT: `DOD_FLUSH_PVS_ONLY` is set.** The sweep was placed only where leaf \
             visibility says the engine never renders it. This is the test of whether a decal on \
             an unrendered face still allocates a ring slot — if old bullet holes survive into a \
             clip, it does not. Unset the variable for a normal capture.",
        );
    }
    if opts.map_geometry_only {
        crate::log_markdown(
            "🧪 **EXPERIMENT: `DOD_FLUSH_MAP_GEOMETRY_ONLY` is set.** The sweep was placed from \
             the map's own world faces alone — no harvested decals, no tiled planes, no \
             coordinate store, no floor path. This is how the source meant for maps nobody has \
             captured yet gets watched on a map somebody can watch. Unset the variable for a \
             normal capture.",
        );
    }

    crate::log_markdown(&format!(
        "🧹 **Decal flush** on `{}`: stripped {} wall decals and {} sprays outside {} clip(s); \
         injected {} flush decals across {} of {} position(s) from {}, in {} burst(s); \
         r_decals pinned to {}.",
        job.source_demo,
        stats.temp_entity_stripped,
        stats.player_spray_stripped,
        keep_windows.len(),
        stats.flush_decals_injected,
        stats.flush_positions,
        stats.flush_positions_wanted,
        source,
        stats.bursts_placed,
        opts.ring_limit
    ));

    // What the on-screen decision was actually able to consult. This changes
    // how much the position count is worth: 68 positions chosen against real
    // geometry and 68 chosen against a bare cone are not the same claim, and
    // the counts look identical.
    match stats.visibility_basis {
        VisibilityBasis::Geometry => crate::log_markdown(&format!(
            "👁️ **Placement used map geometry** — {} world faces{}. Positions were kept only \
             where the map says the camera cannot see them, not merely where they fall outside \
             the frame.",
            stats.map_faces,
            if stats.map_has_vis {
                ", with visibility data"
            } else {
                ", but the map carries no visibility data, so every in-frame candidate needed a \
                 trace"
            }
        )),
        VisibilityBasis::ConeOnly => crate::log_markdown(
            "⚠️ **Placement used the frame cone alone** — no map geometry was available, so the \
             pass cannot tell a wall from a sightline. It rejects spots that are genuinely \
             hidden, and where that leaves it with nothing it will settle for a marginal one.",
        ),
    }

    // What the map's store contributed. Worth its own line: a demo that
    // sweeps only because earlier demos proved the surface is a different
    // situation from one that stands on its own, and the difference is
    // invisible in the position count.
    if let Some(map) = &stats.atlas_map {
        crate::log_markdown(&format!(
            "🗺️ **Map coordinate store** for `{}`: {} coordinate(s) already known, {} added by \
             this demo, {} now held{}.",
            map,
            stats.atlas.known,
            stats.atlas.added,
            stats.atlas.total,
            // The harvest still runs under the gate — refusing the store as a
            // *source* is not a reason to stop feeding it — but saying those
            // coordinates were "available to the flush" would be untrue.
            if opts.map_geometry_only {
                " (the experiment gate refused them as a source for this run)"
            } else {
                " and available to the flush"
            }
        ));
    }

    // The map's own faces, reached only when everything proven fell short.
    // Worth saying out loud: normally it means this demo's own decals and the
    // store between them could not fill a sweep, and the map covered the
    // difference. On a map nothing has been harvested from yet that is the
    // expected path, not a warning.
    //
    // Under the experiment gate the same line would state a reason that is
    // false — the proven sources did not fall short, they were never asked —
    // and a log that misreports why a source was used is worse than one that
    // says nothing, because it is the log someone reads back months later as
    // evidence.
    if stats.map_candidates > 0 {
        let why = if opts.map_geometry_only {
            "every source drawn from the match was refused by the experiment gate"
        } else {
            "the demo's decals and the coordinate store together could not fill a sweep"
        };
        crate::log_markdown(&format!(
            "🧱 **Placed from the map's own geometry** — {}, so {} point(s) were sampled off the \
             map's world faces and {} of those stayed clear of every in-clip camera. This source \
             owes nothing to where anyone shot or walked, which is what lets it cover a map the \
             store has never seen.",
            why, stats.map_candidates, stats.map_camera_safe
        ));
    }

    // Too few distinct spots. Past MAX_OVERLAP_DECALS at one position the
    // engine recycles a decal instead of allocating, and a recycled decal never
    // advances the ring — so the sweep stops short of a full revolution and
    // some of the old decals survive it.
    if stats.flush_positions < stats.flush_positions_wanted {
        crate::log_markdown(&format!(
            "⚠️ **Partial decal sweep** — only {} of the {} distinct positions a full ring \
             revolution needs, so some decals will survive into the clip. {} tiles were laid \
             across the demo's proven planes and {} of those stayed clear of every in-clip \
             camera. An `r_decals {}` init command would give a complete sweep of a smaller \
             ring instead.",
            stats.flush_positions,
            stats.flush_positions_wanted,
            stats.tiled_candidates,
            stats.tiled_camera_safe,
            // The largest ring this many positions can fully turn, backing the
            // over-provision margin out of the burst it implies.
            (stats.flush_positions * DECALS_PER_POSITION).saturating_sub(opts.burst_margin)
        ));
    }

    // Positions closer to the lens than preferred. Not a defect on its own —
    // the surface each is drawn on cleared the on-screen test, so none is ever
    // in shot — but it narrows the margin against a camera turn falling between
    // two samples.
    if let Some(nearest) = stats.min_camera_distance
        && nearest < opts.min_camera_clearance
    {
        crate::log_markdown(&format!(
            "ℹ️ **Decal flush spots are closer to the camera than preferred** — nearest \
                 approach {:.0} units against a {:.0}-unit preference. The surface each one is \
                 drawn on cleared the line-of-sight test, so none should be in shot; this is the \
                 margin narrowing, not a decal on screen.",
            nearest, opts.min_camera_clearance
        ));
    }

    // The one outright defect this pass can introduce: its own decals on
    // screen. The cone test is sampled every fourth frame, so a non-zero count
    // here has to be looked at rather than trusted.
    if stats.flush_on_camera_frames > 0 {
        crate::log_markdown(&format!(
            "⚠️ **Flush decals may be on camera** — the chosen spot(s) fall inside the camera cone \
             on {} of {} sampled in-clip frames. Review the takes.",
            stats.flush_on_camera_frames, stats.camera_samples
        ));
    }

    // A gap too short to fit a whole sweep before the clip opens.
    for (window_start, placed, wanted) in &stats.bursts_short {
        crate::log_markdown(&format!(
            "⚠️ **Short decal burst** before frame {} — placed {} of {}. That clip is not \
             guaranteed to start clean.",
            window_start, placed, wanted
        ));
    }

    // A computed floor point is geometry the demo never proved. If it misses a
    // surface the engine creates nothing and the entire sweep no-ops silently.
    if stats.flush_source == Some(FlushSource::ComputedSpawnFloor) {
        crate::log_markdown(
            "⚠️ **Decal flush anchored on a computed floor point** — the demo contained no real \
             decal to borrow a proven surface from. If that point misses geometry, the sweep does \
             nothing at all.",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::types::CaptureBlock;
    use crate::test_support::Scratch;

    /// `source_demo` deliberately points at nothing: every case below must
    /// decide to skip before it ever opens the file, so a read attempt would
    /// surface as a failure rather than a silent fallback.
    fn job_with_blocks(blocks: Vec<CaptureBlock>) -> PatchJob {
        PatchJob {
            source_demo: "no_such_demo_should_ever_be_read.dem".to_string(),
            output_demo: std::path::PathBuf::from("unused_output.dem"),
            streaks: Vec::new(),
            target_player: None,
            init_commands: Vec::new(),
            scheduled_commands: Vec::new(),
            director_events: Vec::new(),
            block_routes: Vec::new(),
            blocks,
        }
    }

    fn block(block_index: usize, record_start_tick: i32, record_stop_tick: i32) -> CaptureBlock {
        CaptureBlock {
            demo_name: "dodstudio_chain_01".to_string(),
            block_index,
            drive_index: 0,
            take_folder: std::path::PathBuf::from("take"),
            take_key: String::new(),
            source_streak_indices: vec![block_index],
            start_tick: record_start_tick,
            end_tick: record_stop_tick,
            record_start_tick,
            record_stop_tick,
        }
    }

    /// `job_with_blocks`'s `source_demo` points at nothing, so this passing at
    /// all proves the cancelled job returned before reading the demo -- a read
    /// attempt would surface as a skip, not an `Err`.
    #[test]
    fn a_cancelled_batch_abandons_the_flush_before_reading_the_demo() {
        let job = job_with_blocks(vec![block(0, 100, 200)]);
        let mut config = PatcherConfig::default();
        config.decal_flush = true;

        let token = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let result = prepare_flushed_source(&job, &config, crate::patch::Cancel::new(&token));

        assert!(
            matches!(result, Err(crate::patch::Cancelled)),
            "a cancelled batch must not fall back to the unflushed demo and carry on"
        );
    }

    /// The mirror of the above: an uncancelled token must change nothing, or
    /// every flush would start returning Cancelled.
    #[test]
    fn an_unset_token_leaves_the_flush_decision_alone() {
        let job = job_with_blocks(Vec::new());
        let mut config = PatcherConfig::default();
        config.decal_flush = true;

        let token = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let result = prepare_flushed_source(&job, &config, crate::patch::Cancel::new(&token));

        // No blocks -> nothing to keep clean -> a plain skip, not an error.
        assert!(matches!(result, Ok(None)));
    }

    #[test]
    fn the_pipeline_never_inserts_the_r_decals_frame() {
        // Inserting that ConsoleCommand frame shifts every later frame ordinal
        // by +1, which desyncs the scheduled capture commands from the
        // patcher's frame counter — invisibly, since the demo still parses and
        // plays. The pipeline pins the cvar from init_commands instead.
        let config = PatcherConfig {
            decal_ring_limit: 128,
            ..PatcherConfig::default()
        };
        let opts = flush_options(&config);

        assert!(
            !opts.inject_r_decals_command,
            "the pipeline must not insert a console-command frame — init_commands owns r_decals"
        );
        assert_eq!(
            opts.ring_limit, 128,
            "the configured ring size must reach the sweep"
        );
    }

    #[test]
    fn an_init_command_states_the_ring_and_the_sweep_is_sized_to_it() {
        // One number, read where the engine reads it. A separate setting could
        // only agree with the cvar or silently disagree, and a sweep sized to
        // the wrong one under-clears with nothing in the output to show it.
        let config = PatcherConfig {
            decal_ring_limit: 128,
            init_commands: vec!["r_decals 512".to_string()],
            ..PatcherConfig::default()
        };

        assert_eq!(ring_limit(&config), 512);
        assert_eq!(flush_options(&config).ring_limit, 512);
    }

    /// A game folder laid out as the engine expects: `hl.exe` with `dod/`
    /// beside it. Returns the path to the exe, and the guard the caller must
    /// hold -- dropping it here would delete the folder before it was read.
    fn fake_game(
        tag: &str,
        config_cfg: &str,
        movie_cfg: Option<&str>,
    ) -> (Scratch, std::path::PathBuf) {
        let root = Scratch::new(format_args!("fov_cfg_{tag}"));
        let dod = root.join("dod");
        std::fs::create_dir_all(&dod).unwrap();
        std::fs::write(dod.join("config.cfg"), config_cfg).unwrap();
        if let Some(movie) = movie_cfg {
            std::fs::write(dod.join("movie.cfg"), movie).unwrap();
        }
        let exe = root.join("hl.exe");
        std::fs::write(&exe, b"").unwrap();
        (root, exe)
    }

    #[test]
    fn a_fov_the_game_config_sets_is_found_rather_than_assumed() {
        // The real shape, and the reason this exists: config.cfg ends with
        // `exec movie.cfg`, movie.cfg carries `mirv_fov 105`, and the app was
        // never told. Sizing the cone for the default 90 makes it ~7 degrees
        // too narrow and calls in-shot positions hidden.
        let (_root, exe) = fake_game("found", "exec movie.cfg\n", Some("mirv_fov \"105\"\n"));
        let config = PatcherConfig {
            game_path: exe.to_string_lossy().to_string(),
            ..PatcherConfig::default()
        };

        assert_eq!(
            capture_fov(&config),
            config.capture_fov,
            "init commands say nothing"
        );
        assert_eq!(capture_fov_resolved(&config), 105.0, "the config does");
    }

    #[test]
    fn an_init_command_stating_the_default_value_still_outranks_the_config() {
        // The trap: `mirv_fov 90` is what the app seeds into a new install's
        // init commands, and 90 is also the configured default. Deciding
        // "was one stated?" by comparing against the default would read that as
        // silence and hand every defaulted install to its movie.cfg — here,
        // rendering the cone for 105 when the user asked for 90.
        let (_root, exe) = fake_game(
            "states_default",
            "exec movie.cfg\n",
            Some("mirv_fov \"105\"\n"),
        );
        let config = PatcherConfig {
            game_path: exe.to_string_lossy().to_string(),
            init_commands: vec!["mirv_fov 90".to_string()],
            ..PatcherConfig::default()
        };

        assert_eq!(
            config.capture_fov, 90.0,
            "the default this could be confused with"
        );
        assert_eq!(capture_fov_from_init(&config.init_commands), Some(90.0));
        assert_eq!(capture_fov_resolved(&config), 90.0, "the user asked for 90");
    }

    #[test]
    fn an_init_command_outranks_the_game_config() {
        let (_root, exe) = fake_game("outrank", "exec movie.cfg\n", Some("mirv_fov \"105\"\n"));
        let config = PatcherConfig {
            game_path: exe.to_string_lossy().to_string(),
            init_commands: vec!["mirv_fov 120".to_string()],
            ..PatcherConfig::default()
        };

        assert_eq!(capture_fov_resolved(&config), 120.0);
    }

    #[test]
    fn a_config_that_switches_decals_off_is_now_adopted_like_fov_is() {
        // Regression: this used to assert the opposite — that a config's
        // `r_decals 0` was ignored in favor of the app's own default, on the
        // reasoning that adopting it would silently stand the flush down.
        // Now that stand-down is its own loud, reported fact
        // (decal_flush_is_noop) rather than something to hide by disagreeing
        // with the config, so r_decals follows mirv_fov's precedence exactly:
        // init commands, then an executed config, then the app's default.
        let (_root, exe) = fake_game("decals_off", "exec movie.cfg\n", Some("r_decals \"0\"\n"));
        let config = PatcherConfig {
            game_path: exe.to_string_lossy().to_string(),
            ..PatcherConfig::default()
        };

        assert_eq!(
            ring_limit(&config),
            0,
            "the config's own value now wins, same as mirv_fov's would"
        );
    }

    #[test]
    fn a_nonzero_r_decals_the_game_config_sets_is_adopted() {
        let (_root, exe) = fake_game(
            "decals_from_config",
            "exec movie.cfg\n",
            Some("r_decals \"512\"\n"),
        );
        let config = PatcherConfig {
            game_path: exe.to_string_lossy().to_string(),
            decal_ring_limit: 128,
            ..PatcherConfig::default()
        };

        assert_eq!(
            ring_limit(&config),
            512,
            "the config's value, not the app's own default"
        );
    }

    #[test]
    fn an_init_command_still_outranks_a_game_config_for_r_decals() {
        let (_root, exe) = fake_game(
            "decals_init_outranks",
            "exec movie.cfg\n",
            Some("r_decals \"0\"\n"),
        );
        let config = PatcherConfig {
            game_path: exe.to_string_lossy().to_string(),
            init_commands: vec!["r_decals 512".to_string()],
            ..PatcherConfig::default()
        };

        assert_eq!(
            ring_limit(&config),
            512,
            "stated in Initial Commands, so it wins outright"
        );
    }

    #[test]
    fn the_last_r_decals_wins_like_the_console_does() {
        let config = PatcherConfig {
            init_commands: vec!["r_decals 512".to_string(), "r_decals 64".to_string()],
            ..PatcherConfig::default()
        };

        assert_eq!(ring_limit(&config), 64);
    }

    #[test]
    fn a_quoted_r_decals_in_init_commands_is_still_recognised() {
        // Real .cfg syntax quotes every value, and "Load from .cfg file…"
        // carries that straight into Initial Commands — a raw, unquoted
        // parse would silently read `r_decals "512"` as unstated and fall
        // through to the app's own default instead of the user's value.
        let config = PatcherConfig {
            decal_ring_limit: 128,
            init_commands: vec!["r_decals \"512\"".to_string()],
            ..PatcherConfig::default()
        };

        assert_eq!(ring_limit_from_init(&config.init_commands), Some(512));
        assert_eq!(ring_limit(&config), 512);
    }

    #[test]
    fn a_quoted_mirv_fov_in_init_commands_is_still_recognised() {
        let config = PatcherConfig {
            capture_fov: 90.0,
            init_commands: vec!["mirv_fov \"105\"".to_string()],
            ..PatcherConfig::default()
        };

        assert_eq!(capture_fov_from_init(&config.init_commands), Some(105.0));
        assert_eq!(capture_fov(&config), 105.0);
    }

    #[test]
    fn the_configured_default_fills_in_when_nothing_states_it() {
        let config = PatcherConfig {
            decal_ring_limit: 128,
            init_commands: vec!["mirv_fov 105".to_string()],
            ..PatcherConfig::default()
        };

        assert_eq!(ring_limit_from_init(&config.init_commands), None);
        assert_eq!(ring_limit(&config), 128);
    }

    #[test]
    fn a_ring_past_the_engine_ceiling_is_clamped_to_it() {
        // The engine clamps r_decals to MAX_RENDER_DECALS, so positions beyond
        // that would be spent turning a ring that had already come round.
        let config = PatcherConfig {
            init_commands: vec!["r_decals 99999".to_string()],
            ..PatcherConfig::default()
        };

        assert_eq!(ring_limit(&config), crate::patch::MAX_RENDER_DECALS);
    }

    #[test]
    fn a_longer_command_that_merely_starts_the_same_is_not_the_ring() {
        let config = PatcherConfig {
            decal_ring_limit: 256,
            init_commands: vec!["r_decals_enabled 1".to_string()],
            ..PatcherConfig::default()
        };

        assert_eq!(ring_limit_from_init(&config.init_commands), None);
        assert_eq!(ring_limit(&config), 256);
    }

    #[test]
    fn decals_switched_off_entirely_leaves_the_demo_alone() {
        // r_decals 0 is no decals at all: no ring to turn, no bullet holes to
        // clear, and a sweep sized zero would be a burst with nowhere to put
        // anything.
        let config = PatcherConfig {
            init_commands: vec!["r_decals 0".to_string()],
            ..PatcherConfig::default()
        };
        let job = job_with_blocks(vec![block(0, 1000, 2000)]);

        assert_eq!(ring_limit(&config), 0);
        assert!(
            prepare_flushed_source(&job, &config, crate::patch::Cancel::never())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn flush_disabled_leaves_the_demo_alone() {
        let config = PatcherConfig {
            decal_flush: false,
            ..PatcherConfig::default()
        };
        let job = job_with_blocks(vec![block(0, 1000, 2000)]);

        assert!(
            prepare_flushed_source(&job, &config, crate::patch::Cancel::never())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn jobs_with_no_blocks_are_skipped() {
        // The primer job and preview jobs record nothing, so there is no clip
        // to keep clean and nothing to strip against.
        let job = job_with_blocks(Vec::new());

        assert!(
            prepare_flushed_source(
                &job,
                &PatcherConfig::default(),
                crate::patch::Cancel::never()
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn one_block_missing_its_record_bounds_skips_the_whole_job() {
        // A partial window set would strip the decals landing inside the clip
        // whose bounds went missing — worse than the dirty walls being fixed.
        let job = job_with_blocks(vec![block(0, 1000, 2000), block(1, 0, 0)]);

        assert!(keep_windows_for(&job).is_none());
        assert!(
            prepare_flushed_source(
                &job,
                &PatcherConfig::default(),
                crate::patch::Cancel::never()
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn an_inverted_record_window_skips_the_whole_job() {
        // A record stop clamped back to the end of the demo can land ahead of
        // its own start. That window keeps nothing, so the clip would be
        // stripped rather than protected.
        let job = job_with_blocks(vec![block(0, 1000, 2000), block(1, 9000, 8000)]);

        assert!(keep_windows_for(&job).is_none());
    }

    #[test]
    fn a_single_frame_record_window_is_usable() {
        // With no record lead or trail, a one-kill highlight really does start
        // and stop on the same frame. Rejecting that would silently disable the
        // flush for the whole job.
        let job = job_with_blocks(vec![block(0, 1000, 2000), block(1, 5000, 5000)]);

        assert_eq!(
            keep_windows_for(&job),
            Some(vec![(1000, 2000), (5000, 5000)]),
            "equal record bounds are a real one-frame window, not a missing one"
        );
    }

    #[test]
    fn the_cone_covers_the_corner_of_the_frame_not_its_edge() {
        // A decal at the corner of the frame is further off the view axis than
        // one at the edge, so the horizontal half-angle is not enough on its
        // own. Testing against it is what let a spot at 47 degrees pass as
        // "hidden" during a 90-degree capture.
        for fov in [75.0f32, 90.0, 105.0, 120.0] {
            let half = on_screen_half_angle(fov, 1920, 1080);
            assert!(
                half > fov / 2.0,
                "at {} degrees the cone was {:.1}, which does not even reach the frame edge at {:.1}",
                fov,
                half,
                fov / 2.0
            );
        }
    }

    #[test]
    fn a_wider_capture_needs_a_wider_cone() {
        let at90 = on_screen_half_angle(90.0, 1920, 1080);
        let at105 = on_screen_half_angle(105.0, 1920, 1080);
        assert!(
            at105 > at90 + 5.0,
            "105 gave {:.1} against 90's {:.1} — a movie shot wide would keep spots that are in shot",
            at105,
            at90
        );
    }

    #[test]
    fn the_old_fixed_cone_was_too_narrow_at_every_common_fov() {
        // Regression guard for the bug this replaced: 40 degrees, justified as
        // "generous" against a 90-degree FOV, was under even the 45-degree
        // horizontal half-angle.
        const OLD_FIXED_CONE: f32 = 40.0;
        for (fov, w, h) in [
            (90.0f32, 1920, 1080),
            (90.0, 1280, 960),
            (105.0, 1920, 1080),
        ] {
            assert!(
                on_screen_half_angle(fov, w, h) > OLD_FIXED_CONE,
                "fov {} at {}x{} still fits inside the old fixed cone",
                fov,
                w,
                h
            );
        }
    }

    #[test]
    fn a_taller_frame_pushes_the_corner_further_out() {
        // 4:3 puts more of the frame vertically, so its corner sits further off
        // the axis than a 16:9 corner at the same horizontal FOV.
        let wide = on_screen_half_angle(90.0, 1920, 1080);
        let tall = on_screen_half_angle(90.0, 1280, 960);
        assert!(
            tall > wide,
            "4:3 {:.1} should exceed 16:9 {:.1}",
            tall,
            wide
        );
    }

    #[test]
    fn a_nonsense_resolution_falls_back_rather_than_dividing_by_zero() {
        let half = on_screen_half_angle(90.0, 0, 0);
        assert!(half.is_finite() && half > 45.0);
    }

    #[test]
    fn mirv_fov_in_the_init_commands_wins() {
        // The capture runs at whatever the console was last told, so reading it
        // from there is the only way the two cannot disagree.
        let mut config = PatcherConfig {
            capture_fov: 90.0,
            ..PatcherConfig::default()
        };
        assert_eq!(capture_fov(&config), 90.0);

        config.init_commands = vec!["exec autoexec".to_string(), "mirv_fov 105".to_string()];
        assert_eq!(capture_fov(&config), 105.0);

        // Last one wins, as at the console.
        config.init_commands.push("mirv_fov 100".to_string());
        assert_eq!(capture_fov(&config), 100.0);
    }

    #[test]
    fn a_command_that_merely_starts_the_same_is_not_mistaken_for_it() {
        let config = PatcherConfig {
            capture_fov: 90.0,
            init_commands: vec!["mirv_fov_something_else 12".to_string()],
            ..PatcherConfig::default()
        };
        assert_eq!(capture_fov(&config), 90.0);
    }

    #[test]
    fn the_pipeline_derives_its_cone_from_the_capture_fov() {
        // The whole point: a movie shot at mirv_fov 105 must not be planned
        // against a 90-degree frame.
        let config = PatcherConfig {
            resolution_width: 1920,
            resolution_height: 1080,
            init_commands: vec!["mirv_fov 105".to_string()],
            ..PatcherConfig::default()
        };
        let opts = flush_options(&config);
        assert!(
            (opts.visibility_cone_degrees - on_screen_half_angle(105.0, 1920, 1080)).abs() < 0.01
        );
        assert!(opts.visibility_cone_degrees > 55.0);
    }

    #[test]
    fn the_sweep_takes_only_its_own_stale_leavings() {
        // This deletes files, so what it will not touch matters more than what
        // it will. A concurrent capture's scratch is young; everything else in
        // the temp directory is not ours at any age.
        use std::time::{Duration, SystemTime};

        let dir = Scratch::new("sweep_test");

        let stale = dir.join(format!("{}old_1_0.dem", SCRATCH_PREFIX));
        let fresh = dir.join(format!("{}live_2_0.dem", SCRATCH_PREFIX));
        let other = dir.join("someone_elses_file.dem");
        for p in [&stale, &fresh, &other] {
            std::fs::write(p, b"x").unwrap();
        }

        // Backdate the stale one well past the threshold.
        let long_ago = SystemTime::now() - SCRATCH_STALE_AFTER - Duration::from_secs(60);
        filetime::set_file_mtime(&stale, filetime::FileTime::from_system_time(long_ago)).unwrap();

        sweep_stale_scratch(&dir);

        assert!(
            !stale.exists(),
            "an orphaned scratch demo should be removed"
        );
        assert!(
            fresh.exists(),
            "a scratch demo young enough to belong to a running capture must survive"
        );
        assert!(
            other.exists(),
            "files that are not ours must never be touched"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
