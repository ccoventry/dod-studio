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
    let dir = crate::shared::paths::mod_dir_for_exe(std::path::Path::new(&config.game_path))?;
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
/// default. `ring_limit` (r_decals) follows exactly this same order. Adopting
/// a `movie.cfg`'s `r_decals 0` does not silently stand the flush down: a
/// resolved 0 with Flush Decals on is its own loud, reported fact
/// (`decal_flush_is_noop` in the studio report) rather than something this
/// function would hide by disagreeing with the config.
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

    crate::log_markdown(&crate::messages::decal_flush_game_cfgs_set_values(
        lines.join(", "),
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
        crate::log_markdown(crate::messages::DECAL_FLUSH_SKIPPED_NO_RING);
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
            crate::log_markdown(&crate::messages::decal_flush_skipped_no_record_bounds(
                &job.source_demo,
            ));
            return Ok(None);
        }
    };

    let bytes = match std::fs::read(&job.source_demo) {
        Ok(b) => b,
        Err(e) => {
            crate::log_markdown(&crate::messages::decal_flush_skipped_unreadable(
                &job.source_demo,
                e,
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
            crate::log_markdown(&crate::messages::decal_flush_failed(&job.source_demo, e));
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
        crate::log_markdown(&crate::messages::decal_flush_skipped_scratch_not_written(
            path.display(),
            e,
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
        crate::log_markdown(crate::messages::DECAL_FLUSH_EXPERIMENT_PVS_ONLY);
    }
    if opts.map_geometry_only {
        crate::log_markdown(crate::messages::DECAL_FLUSH_EXPERIMENT_MAP_GEOMETRY_ONLY);
    }

    crate::log_markdown(&crate::messages::decal_flush_summary(
        &job.source_demo,
        stats.temp_entity_stripped,
        stats.player_spray_stripped,
        keep_windows.len(),
        stats.flush_decals_injected,
        stats.flush_positions,
        stats.flush_positions_wanted,
        source,
        stats.bursts_placed,
        opts.ring_limit,
    ));

    // What the on-screen decision was actually able to consult. This changes
    // how much the position count is worth: 68 positions chosen against real
    // geometry and 68 chosen against a bare cone are not the same claim, and
    // the counts look identical.
    match stats.visibility_basis {
        VisibilityBasis::Geometry => {
            crate::log_markdown(&crate::messages::decal_flush_placement_used_geometry(
                stats.map_faces,
                if stats.map_has_vis {
                    crate::messages::DECAL_FLUSH_GEOMETRY_WITH_VIS
                } else {
                    crate::messages::DECAL_FLUSH_GEOMETRY_NO_VIS
                },
            ))
        }
        VisibilityBasis::ConeOnly => {
            crate::log_markdown(crate::messages::DECAL_FLUSH_PLACEMENT_CONE_ONLY)
        }
    }

    // What the map's store contributed. Worth its own line: a demo that
    // sweeps only because earlier demos proved the surface is a different
    // situation from one that stands on its own, and the difference is
    // invisible in the position count.
    if let Some(map) = &stats.atlas_map {
        crate::log_markdown(&crate::messages::decal_flush_atlas_summary(
            map,
            stats.atlas.known,
            stats.atlas.added,
            stats.atlas.total,
            // The harvest still runs under the gate — refusing the store as a
            // *source* is not a reason to stop feeding it — but saying those
            // coordinates were "available to the flush" would be untrue.
            if opts.map_geometry_only {
                crate::messages::DECAL_FLUSH_ATLAS_REFUSED_BY_GATE
            } else {
                crate::messages::DECAL_FLUSH_ATLAS_AVAILABLE
            },
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
            crate::messages::DECAL_FLUSH_MAP_WHY_GATE
        } else {
            crate::messages::DECAL_FLUSH_MAP_WHY_SHORT
        };
        crate::log_markdown(&crate::messages::decal_flush_placed_from_geometry(
            why,
            stats.map_candidates,
            stats.map_camera_safe,
        ));
    }

    // Too few distinct spots. Past MAX_OVERLAP_DECALS at one position the
    // engine recycles a decal instead of allocating, and a recycled decal never
    // advances the ring — so the sweep stops short of a full revolution and
    // some of the old decals survive it.
    if stats.flush_positions < stats.flush_positions_wanted {
        crate::log_markdown(&crate::messages::decal_flush_partial_sweep(
            stats.flush_positions,
            stats.flush_positions_wanted,
            stats.tiled_candidates,
            stats.tiled_camera_safe,
            // The largest ring this many positions can fully turn, backing the
            // over-provision margin out of the burst it implies.
            (stats.flush_positions * DECALS_PER_POSITION).saturating_sub(opts.burst_margin),
        ));
    }

    // Positions closer to the lens than preferred. Not a defect on its own —
    // the surface each is drawn on cleared the on-screen test, so none is ever
    // in shot — but it narrows the margin against a camera turn falling between
    // two samples.
    if let Some(nearest) = stats.min_camera_distance
        && nearest < opts.min_camera_clearance
    {
        crate::log_markdown(&crate::messages::decal_flush_spots_close_to_camera(
            nearest,
            opts.min_camera_clearance,
        ));
    }

    // The one outright defect this pass can introduce: its own decals on
    // screen. The cone test is sampled every fourth frame, so a non-zero count
    // here has to be looked at rather than trusted.
    if stats.flush_on_camera_frames > 0 {
        crate::log_markdown(&crate::messages::decal_flush_may_be_on_camera(
            stats.flush_on_camera_frames,
            stats.camera_samples,
        ));
    }

    // A gap too short to fit a whole sweep before the clip opens.
    for (window_start, placed, wanted) in &stats.bursts_short {
        crate::log_markdown(&crate::messages::decal_flush_short_burst(
            window_start,
            placed,
            wanted,
        ));
    }

    // A computed floor point is geometry the demo never proved. If it misses a
    // surface the engine creates nothing and the entire sweep no-ops silently.
    if stats.flush_source == Some(FlushSource::ComputedSpawnFloor) {
        crate::log_markdown(crate::messages::DECAL_FLUSH_COMPUTED_FLOOR_ANCHOR);
    }
}

#[cfg(test)]
#[path = "flush_job_tests.rs"]
mod tests;
