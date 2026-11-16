// ============================================================
// studio/src-tauri/src/capture_manager.rs
//
// Headless CaptureManager — bridges the native patch pipeline
// into Tauri's managed-state system.
//
// Isolation contract:
//   - MUST NOT import any egui / eframe symbols.
//   - All shared fields use Arc<Mutex<T>> for safe cross-thread access.
//   - Blocking I/O is offloaded to tokio::task::spawn_blocking so the
//     async Tauri command never parks the main thread.
// ============================================================

use native::shared::lock::lock_or_recover;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// How many `PatchJob`s may run through `StreamPatcher::patch` at once. Each
/// job is self-contained (own source/output paths, only shared reads of the
/// patcher config and cancel token -- see issue #114), so this is a plain
/// CPU/IO-bound worker pool, not one balanced across output drives: the
/// patched demo file itself always lands on `capture_directories[0]`
/// regardless of which drive a job's `CaptureBlock`s later route their
/// recorded video to (see issue #8), so per-drive concurrency would give no
/// real parallelism here.
const PATCH_CONCURRENCY: usize = 4;

use native::capture_engine::clip_verdict::{demo_ordinals, recording_order};
use native::capture_engine::{CaptureJob, EngineEvent, spawn_capture_engine};
use native::log_markdown;
use native::patch::{
    CaptureBlock, CaptureStreak, CommandRelation, CustomCommand, PatcherConfig, StreamPatcher,
    build_batch_queue,
};
use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::clip_verdicts::ClipEvidence;
use crate::game_process_manager::saved_settings;

/// Every patch job's stage, folded into the one status line a batch shows
/// (#75). Jobs report from their own worker threads, so each has an atomic
/// slot: 0 not running, 1 clearing decals, 2 + percent while writing.
struct PatchProgress {
    slots: Vec<std::sync::atomic::AtomicU32>,
    /// Each job's demo, for the line (#952).
    names: Vec<String>,
    started: std::time::Instant,
    /// Milliseconds after `started` of the last throttled emit.
    last_emit_ms: std::sync::atomic::AtomicU32,
}

const PATCH_SLOT_IDLE: u32 = 0;
const PATCH_SLOT_CLEARING: u32 = 1;
const PATCH_SLOT_WRITING: u32 = 2;

/// What `PatchProgress::summary` says: the line, how many jobs' worth of
/// work is done (fractional, counting written percent, for the bar), and
/// whether that number moves: false while every running job is in the decal
/// pass, which reports no percentage, so the bar slides instead (#952).
struct PatchSummary {
    status: String,
    index: f64,
    measured: bool,
}

impl PatchProgress {
    fn new(names: Vec<String>) -> Self {
        Self {
            slots: (0..names.len())
                .map(|_| std::sync::atomic::AtomicU32::new(PATCH_SLOT_IDLE))
                .collect(),
            names,
            started: std::time::Instant::now(),
            last_emit_ms: std::sync::atomic::AtomicU32::new(0),
        }
    }

    /// Records a job's stage. True when the line should be sent now: always
    /// for a change of stage, and for percentages at most every ~33ms across
    /// the whole batch, per CLAUDE.md's telemetry throttle.
    fn set(&self, job: usize, stage: native::patch::PatchStage) -> bool {
        use std::sync::atomic::Ordering;
        let value = match stage {
            native::patch::PatchStage::ClearingDecals => PATCH_SLOT_CLEARING,
            native::patch::PatchStage::Writing { percent } => PATCH_SLOT_WRITING + percent as u32,
        };
        let previous = self.slots[job].swap(value, Ordering::Relaxed);
        let stage_changed = (previous >= PATCH_SLOT_WRITING) != (value >= PATCH_SLOT_WRITING)
            || previous == PATCH_SLOT_IDLE;
        let now = self.started.elapsed().as_millis() as u32;
        let last = self.last_emit_ms.load(Ordering::Relaxed);
        if !stage_changed && now.wrapping_sub(last) < 33 {
            return false;
        }
        self.last_emit_ms.store(now, Ordering::Relaxed);
        true
    }

    fn finish(&self, job: usize) {
        self.slots[job].store(PATCH_SLOT_IDLE, std::sync::atomic::Ordering::Relaxed);
    }

    /// See `PatchSummary`.
    fn summary(&self, done: u32) -> PatchSummary {
        let mut running: Vec<(&str, Option<u8>)> = Vec::new();
        for (slot, name) in self.slots.iter().zip(&self.names) {
            match slot.load(std::sync::atomic::Ordering::Relaxed) {
                PATCH_SLOT_IDLE => {}
                PATCH_SLOT_CLEARING => running.push((name, None)),
                v => running.push((name, Some((v - PATCH_SLOT_WRITING).min(100) as u8))),
            }
        }
        let partial: f64 = running
            .iter()
            .filter_map(|(_, p)| *p)
            .map(|p| p as f64 / 100.0)
            .sum();
        PatchSummary {
            status: crate::messages::preparing_status(done, self.slots.len(), &running),
            index: done as f64 + partial,
            measured: running.is_empty() || running.iter().any(|(_, p)| p.is_some()),
        }
    }
}

/// A demo's file name for a status line, or the path when it has none.
fn demo_file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

// ── IPC payload type ───────────────────────────────────────────────────────────

/// Top-level payload from the frontend when the user triggers a capture batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturePayload {
    /// Absolute path to hlae.exe
    pub hlae_path: String,
    /// Absolute path to hl.exe
    pub game_path: String,
    /// Optional absolute path to ffmpeg.exe; falls back to bundled then PATH.
    #[serde(default)]
    pub ffmpeg_override_path: Option<String>,
    /// Optional override for `dodstudio_goldsrc_hooks.dll`; falls back to the bundled
    /// default -- see `PatcherConfig::goldsrc_hooks_dll_path`.
    #[serde(default)]
    pub goldsrc_hooks_dll_path: Option<String>,
    #[serde(default = "default_resolution_width")]
    pub resolution_width: i32,
    #[serde(default = "default_resolution_height")]
    pub resolution_height: i32,
    #[serde(default)]
    pub ffmpeg_capture: bool,
    /// Codec id for direct-to-video capture; unknown ids fall back to the
    /// default rather than failing the batch.
    #[serde(default)]
    pub ffmpeg_capture_codec: String,
    /// `frame_sequence`, `direct_to_video`, `obs` or `agr`. Absent on payloads from a
    /// frontend predating the selector, in which case `ffmpeg_capture` above
    /// still decides — see `PatcherConfig::normalise_capture_mode`.
    #[serde(default)]
    pub capture_mode: String,
    #[serde(default)]
    pub obs_host: String,
    #[serde(default)]
    pub obs_port: u16,
    #[serde(default)]
    pub obs_password: String,
    /// `ObsRecordingQuality`'s id; empty or unknown is the default.
    #[serde(default)]
    pub obs_recording_quality: String,
    #[serde(default)]
    pub save_local_patched_copy: bool,
    /// `PatcherConfig::hook_runs_batch`: on unless the payload says otherwise.
    #[serde(default = "native::patch::types::default_hook_runs_batch")]
    pub hook_runs_batch: bool,
    /// The open game to run the batch in instead of launching one (#725),
    /// from `running_game_for_batch`. Only a Seek Between Clips batch can.
    #[serde(default)]
    pub reuse_game_pid: Option<u32>,
    /// `PatcherConfig::keep_game_behind`.
    #[serde(default = "crate::settings_manager::default_keep_game_behind")]
    pub keep_game_behind: bool,
    /// Highlight streaks to capture.
    pub streaks: Vec<SerializedStreak>,
    /// Each streak's durable id (`take_index.js`'s `streakUid`), index-aligned
    /// with `streaks`, recorded in the manifest (#545).
    #[serde(default)]
    pub streak_uids: Vec<String>,
    /// The project file the batch was started from, when it had one.
    #[serde(default)]
    pub project_path: Option<String>,
    /// Pre-roll added before each streak (seconds). Converted → ticks at 100 Hz.
    pub pre_roll_seconds: f32,
    /// Post-roll padding after each streak (seconds).
    pub post_roll_seconds: f32,
    /// Directories used for dynamic drive failover and capture routing.
    pub capture_directories: Vec<String>,
    pub capture_fps: i32,
    /// OBS mode's own capture rate — see `PatcherConfig::obs_capture_fps`.
    #[serde(default = "default_obs_capture_fps_payload")]
    pub obs_capture_fps: i32,
    /// AGR mode's own rate — see `PatcherConfig::agr_fps`. 0 (or absent)
    /// means `native::patch::DEFAULT_AGR_FPS`.
    #[serde(default)]
    pub agr_fps: i32,
    /// Output drives for AOT capacity simulation and media routing.
    pub drives: Vec<String>,
    #[serde(default)]
    pub record_start_lead: f32,
    #[serde(default)]
    pub record_stop_trail: f32,
    #[serde(default = "default_initial_delay")]
    pub initial_delay: f32,
    #[serde(default = "default_fast_forward_speed")]
    pub fast_forward_speed: f32,
    #[serde(default)]
    pub auto_clear_logs: bool,
    #[serde(default)]
    pub auto_clear_previews: bool,
    #[serde(default)]
    pub auto_clear_temp_demos: bool,
    /// Timestamped batch id (e.g. `session_20260813_142233`) stamped by the
    /// frontend before dispatch; routes capture output into its own
    /// subfolder instead of colliding in the export root.
    #[serde(default)]
    pub session_id: String,
    /// Raw console commands injected once at demo-load time, before any
    /// scheduled/highlight commands.
    #[serde(default)]
    pub init_commands: Vec<String>,
    /// User-defined commands scheduled relative to each highlight's bounds.
    #[serde(default)]
    pub custom_commands: Vec<CustomCommandPayload>,
    /// Clear accumulated wall decals ahead of every recorded clip, so the
    /// second and later takes cut from one demo don't start dirty. Absent
    /// means "leave it at the pipeline default".
    #[serde(default)]
    pub decal_flush: Option<bool>,
    /// Decal ring size the flush pins `r_decals` to. Absent means the default.
    #[serde(default)]
    pub decal_ring_limit: Option<u32>,
    /// Gaps between kills inside a clip: kept, or recorded sped up (#831).
    /// Absent means kept.
    #[serde(default)]
    pub kill_gaps: native::patch::types::KillGaps,
}

fn default_initial_delay() -> f32 {
    3.0
}
fn default_fast_forward_speed() -> f32 {
    0.05
}
fn default_resolution_width() -> i32 {
    1280
}
fn default_obs_capture_fps_payload() -> i32 {
    120
}
fn default_resolution_height() -> i32 {
    720
}

/// One custom command row — serialisable across the Tauri IPC boundary.
/// `relation` is a plain string ("Before" | "After") rather than
/// `CommandRelation` directly so an unrecognised value fails safe (defaults
/// to `Before` in the `From` below) instead of rejecting the whole batch
/// payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomCommandPayload {
    pub command: String,
    pub relation: String,
    pub offset_seconds: f32,
}

/// The pipeline's form of a row: anything but "After" is `Before`.
impl From<&CustomCommandPayload> for CustomCommand {
    fn from(c: &CustomCommandPayload) -> Self {
        CustomCommand {
            command: c.command.clone(),
            offset: c.offset_seconds,
            relation: match c.relation.as_str() {
                "After" => CommandRelation::After,
                _ => CommandRelation::Before,
            },
        }
    }
}

/// One highlight streak — serialisable across the Tauri IPC boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedStreak {
    pub start_tick: i32,
    pub end_tick: i32,
    /// Absolute path to the source .dem file.
    pub source_demo: String,
    pub target_player: Option<String>,
    pub kill_count: usize,
    pub timeline_string: String,
    pub duration_string: String,
    pub player_index: usize,
    /// Raw kill events: (tick, abs_time_secs, weapon_name).
    pub kills: Vec<(i32, f32, String)>,
    pub start_index: usize,
    pub end_index: usize,
    pub total_demo_frames: i32,
    pub demo_fps: f32,
    pub viewdemo_times: Vec<f32>,
    /// Tick at which `svc_gametime` resets (i.e. the match clock zero point).
    /// `None` when the scanner could not locate the reset event.
    #[serde(default)]
    pub match_start_tick: Option<i32>,
    /// Per-frame absolute timestamps extracted from the demo binary.
    /// Transported as a plain `Vec` so it crosses the JSON IPC boundary;
    /// deserialization wraps it in `Arc<Vec<f32>>` before handing it to the
    /// native patch engine.
    #[serde(default)]
    pub frame_times: Vec<f32>,
    /// For the clip name (#441): the killer's side, and each kill's victim
    /// and victim's side. Empty on a demo from a project saved before these
    /// existed, until it is scanned again.
    #[serde(default)]
    pub faction: Option<String>,
    #[serde(default)]
    pub victims: Vec<String>,
    #[serde(default)]
    pub victim_factions: Vec<String>,
    /// Whether each kill was a headshot (#1128). Empty on a demo scanned
    /// before it existed: `scanIsCurrent` rescans it.
    #[serde(default)]
    pub headshots: Vec<::analysis::Headshot>,
    /// What was going on at the flags at each kill (#1132): `while_capping`,
    /// `flag_defence`, `cleared_flag`. Empty on a demo scanned before it
    /// existed: `scanIsCurrent` rescans it.
    #[serde(default)]
    pub flag_tags: Vec<Vec<::analysis::FlagTag>>,
    /// The scanned file's size and start hash, checked against the file on
    /// disk before a batch or preview patches it (#196). Absent from
    /// projects saved before it existed.
    #[serde(default)]
    pub source_key: Option<String>,
    /// `CaptureStreak::spectated`. Absent from projects saved before it
    /// existed: such a demo is scanned again (queue_scan.js).
    #[serde(default)]
    pub spectated: bool,
    /// `CaptureStreak::steam_id`.
    #[serde(default)]
    pub steam_id: Option<String>,
    /// `CaptureStreak::recorded_this_demo`. Absent from projects saved
    /// before #573, which the frontend tells by it (queue_filters.js).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_this_demo: Option<bool>,
}

impl From<SerializedStreak> for CaptureStreak {
    fn from(s: SerializedStreak) -> Self {
        CaptureStreak {
            start_tick: s.start_tick,
            end_tick: s.end_tick,
            source_demo: s.source_demo,
            target_player: s.target_player,
            kill_count: s.kill_count,
            timeline_string: s.timeline_string,
            duration_string: s.duration_string,
            player_index: s.player_index,
            kills: s.kills,
            start_index: s.start_index,
            end_index: s.end_index,
            total_demo_frames: s.total_demo_frames,
            demo_fps: s.demo_fps,
            viewdemo_times: s.viewdemo_times,
            // Restore the Arc wrapper that was flattened for JSON transport.
            frame_times: std::sync::Arc::new(s.frame_times),
            match_start_tick: s.match_start_tick,
            status: native::patch::types::HighlightStatus::Pending,
            source_key: s.source_key,
            spectated: s.spectated,
            steam_id: s.steam_id,
            recorded_this_demo: s.recorded_this_demo.unwrap_or(false),
        }
    }
}

// ── Managed state ──────────────────────────────────────────────────────────────

/// How many of a capture batch's clips are recorded, from the engine's own
/// console markers (#76). The progress bar used to count game sessions, and a
/// chained batch is often a single session, so it sat at one end for the
/// whole capture.
#[derive(Debug, Default)]
struct ClipTally {
    /// Clips on every demo loaded so far, the current one included. This is
    /// the "X of Y clips total" the notifications show.
    through_current_demo: u32,
    /// Clips recorded.
    done: u32,
}

impl ClipTally {
    /// A demo starts: every clip of the demos before it is recorded.
    fn demo_loading(&mut self, clip_count: u32) {
        self.done = self.through_current_demo;
        self.through_current_demo += clip_count;
    }

    /// Clip `clip_idx` (1-based) of the current demo is next, so the ones
    /// before it are recorded.
    fn fast_forward_to(&mut self, clip_idx: u32, clip_count_this_demo: u32) {
        let before_this_demo = self
            .through_current_demo
            .saturating_sub(clip_count_this_demo);
        self.done = before_this_demo + clip_idx.saturating_sub(1);
    }

    /// The game session ended: every clip it loaded is recorded.
    fn session_finished(&mut self) {
        self.done = self.through_current_demo;
    }
}

/// Tauri managed state for the capture subsystem.
pub struct CaptureManager {
    /// Guards against duplicate batch launches.
    pub is_running: Arc<Mutex<bool>>,
    /// Raised to `true` to request mid-batch cancellation.
    pub cancel_token: Arc<std::sync::atomic::AtomicBool>,
    /// Cached config from the most recent run (used for status queries).
    pub last_config: Arc<Mutex<Option<PatcherConfig>>>,
    /// Recording blocks the most recent batch planned, used to verify takes
    /// against disk once the batch ends.
    pub last_manifest: Arc<Mutex<Option<CaptureManifest>>>,
    /// Whether the running batch's start has reached the game, for the
    /// close prompt (#545). Reset to `Preparing` when a batch starts.
    pub batch_start: Arc<native::game_handoff::BatchStart>,
}

impl CaptureManager {
    pub fn new() -> Self {
        Self {
            is_running: Arc::new(Mutex::new(false)),
            cancel_token: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            last_config: Arc::new(Mutex::new(None)),
            last_manifest: Arc::new(Mutex::new(None)),
            batch_start: Arc::new(native::game_handoff::BatchStart::default()),
        }
    }

    /// Returns `true` if a batch is currently executing.
    pub fn is_running(&self) -> bool {
        *lock_or_recover(&self.is_running)
    }
}

// ── Internal helpers ───────────────────────────────────────────────────────────

fn config_from_payload(payload: &CapturePayload) -> PatcherConfig {
    let mut cfg = PatcherConfig::default();
    cfg.hlae_path = payload.hlae_path.clone();
    cfg.game_path = payload.game_path.clone();
    cfg.pre_roll_seconds = payload.pre_roll_seconds;
    cfg.post_roll_seconds = payload.post_roll_seconds;
    cfg.capture_directories = payload
        .capture_directories
        .iter()
        .map(std::path::PathBuf::from)
        .collect();
    cfg.capture_fps = payload.capture_fps;
    cfg.obs_capture_fps = payload.obs_capture_fps;
    cfg.agr_fps = payload.agr_fps;
    cfg.record_start_lead = payload.record_start_lead;
    cfg.record_stop_trail = payload.record_stop_trail;
    cfg.initial_delay = payload.initial_delay;
    cfg.fast_forward_speed = payload.fast_forward_speed;
    cfg.kill_gaps = payload.kill_gaps;
    cfg.ffmpeg_override_path = payload.ffmpeg_override_path.clone();
    cfg.goldsrc_hooks_dll_path =
        crate::updater_manager::allowed_hook_dll(payload.goldsrc_hooks_dll_path.clone());
    cfg.resolution_width = payload.resolution_width;
    cfg.resolution_height = payload.resolution_height;
    cfg.ffmpeg_capture = payload.ffmpeg_capture;
    cfg.ffmpeg_capture_codec =
        native::patch::CaptureCodec::from_str_id(&payload.ffmpeg_capture_codec);
    if !payload.capture_mode.is_empty() {
        cfg.capture_mode = native::patch::CaptureMode::from_str_id(&payload.capture_mode);
    }
    cfg.obs = native::patch::ObsConfig {
        host: if payload.obs_host.is_empty() {
            "127.0.0.1".to_string()
        } else {
            payload.obs_host.clone()
        },
        port: if payload.obs_port == 0 {
            4455
        } else {
            payload.obs_port
        },
        password: payload.obs_password.clone(),
        recording_quality: native::patch::ObsRecordingQuality::from_str_id(
            &payload.obs_recording_quality,
        ),
    };
    cfg.save_local_patched_copy = payload.save_local_patched_copy;
    cfg.hook_runs_batch = payload.hook_runs_batch;
    cfg.keep_game_behind = payload.keep_game_behind;
    cfg.auto_clear_logs = payload.auto_clear_logs;
    cfg.auto_clear_previews = payload.auto_clear_previews;
    cfg.auto_clear_temp_demos = payload.auto_clear_temp_demos;
    cfg.session_id = payload.session_id.clone();
    cfg.init_commands = payload.init_commands.clone();
    cfg.custom_commands = payload
        .custom_commands
        .iter()
        .map(CustomCommand::from)
        .collect();
    if let Some(v) = payload.decal_flush {
        cfg.decal_flush = v;
    }
    if let Some(v) = payload.decal_ring_limit {
        cfg.decal_ring_limit = v;
    }
    // Capture Output is the sole (required) source of output directories —
    // the frontend already blocks the batch if `drives` is empty.
    cfg.primary_media_dir = payload.drives.first().map(std::path::PathBuf::from);
    // Last, so it sees every field the payload set: reconciles the capture mode
    // with the legacy `ffmpeg_capture` flag and applies what the mode implies.
    // Everything downstream branches on `capture_mode`, so this must run before
    // any of it does.
    cfg.normalise_capture_mode();
    cfg
}

// ── Take verification ──────────────────────────────────────────────────────────

/// Every recording block a dispatched batch planned, flattened across jobs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureManifest {
    pub session_id: String,
    pub blocks: Vec<CaptureBlock>,
    /// `mirv_movie_fps` this batch was captured at, recorded beside the takes
    /// once they verify so Render Studio can tell when its own FPS setting
    /// disagrees. See `native::hlcr::take_meta`.
    pub capture_fps: i32,
    /// `CaptureMode::to_str_id` of the batch. Only AGR changes what the take
    /// metadata records (#450).
    #[serde(default)]
    pub capture_mode: String,
    /// The rate an AGR batch recorded at; unused for the movie modes.
    #[serde(default)]
    pub agr_fps: i32,
    /// Each payload highlight's durable id (`take_index.js`'s `streakUid`),
    /// index-aligned with the payload's `streaks`, so a batch Studio stopped
    /// watching can still be mapped back to its highlights after a restart
    /// (#545, `batch_catch_up`). Empty in manifests from before it existed.
    #[serde(default)]
    pub streak_uids: Vec<String>,
    /// Each payload highlight's source demo, index-aligned the same way.
    #[serde(default)]
    pub streak_sources: Vec<String>,
    /// The project file the batch was started from, when it had one.
    #[serde(default)]
    pub project_path: Option<String>,
    /// The full settings the batch captured with and their hash (part of
    /// #1085): written beside each take, sent with `capture_takes_verified`
    /// for the project's take record, and kept in the manifest file so a
    /// batch caught up after a restart records the same. `None` in
    /// manifests from before it existed.
    #[serde(default)]
    pub settings: Option<native::hlcr::take_settings::CaptureSettings>,
}

/// One block's post-batch verdict, checked against what's actually on disk.
#[derive(Debug, Clone, Serialize)]
pub struct VerifiedBlock {
    pub take_key: String,
    pub take_folder: String,
    pub demo_name: String,
    pub block_index: usize,
    /// Positions in the dispatched payload's `streaks` array that this block
    /// covers — several when overlapping highlights merged into one recording.
    pub source_streak_indices: Vec<usize>,
    /// Tier 1: the take folder exists and isn't empty.
    pub captured: bool,
    /// Tier 2: Render Studio's scanner would actually admit this take.
    pub renderable: bool,
    /// Rendered while the batch ran and deleted after (Render While
    /// Capturing, #861): captured, but nothing is left to render.
    pub rendered: bool,
    /// What the take folder holds on disk, for the batch results panel (#172).
    pub bytes: u64,
    /// The batch ended while this clip recorded (#1081): what's on disk is a
    /// partial take, so it counts as neither captured nor renderable, and is
    /// left for the user to delete. `cut_by` says what ended it. Also set for
    /// a take that stopped writing early (`short_take`).
    pub cut_short: bool,
    /// What cut the clip short, when the batch's end did.
    pub cut_by: Option<CutBy>,
    /// The take's video stops well before its audio (#1105): FFmpeg
    /// stopped writing mid-clip. A partial take like a Cancel's, with both
    /// lengths.
    pub short_take: Option<native::hlcr::take_length::ShortTake>,
    /// OBS started recording this many ms after the clip's first frame
    /// (#1109): the take is missing the clip's opening, so it counts as
    /// neither captured nor renderable, and is left for the user to delete.
    pub started_late_ms: Option<u64>,
    /// What the game said about the clip when it isn't whole (#1401): failed
    /// (not captured) or shorter than planned (captured, with a warning).
    pub clip_verdict: Option<native::capture_engine::clip_verdict::ClipVerdict>,
    /// How many gaps between kills the take recorded sped up (#831), for its
    /// label in the batch results panel.
    pub ramped_gaps: usize,
    /// The batch ended before it got to this clip (#1434): no take, but
    /// nothing went wrong with it, so it isn't missing.
    pub not_reached: bool,
}

/// What ended a batch while a clip was recording: the Last Batch panel and
/// the activity log name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CutBy {
    /// The user pressed Cancel (#1081).
    Cancel,
    /// The game closed or crashed mid-clip.
    GameClosed,
    /// The batch stopped on an error with the game still up (a stall, OBS
    /// lost, an error box): the engine ended the game.
    Stopped,
    /// DoD Studio closed while OBS recorded the clip, so nobody stopped the
    /// recording at the clip's end (#20). Only a catch-up after a restart
    /// says this.
    StudioClosed,
}

impl CutBy {
    pub(crate) fn log_text(self) -> &'static str {
        match self {
            CutBy::Cancel => "was cut short by Cancel",
            CutBy::GameClosed => "was cut short when the game closed",
            CutBy::Stopped => "was cut short when the batch stopped",
            CutBy::StudioClosed => "was still recording in OBS when DoD Studio closed",
        }
    }

    /// Why a batch that ended this way never reached its later clips (#1434).
    pub(crate) fn not_reached_text(self) -> &'static str {
        match self {
            CutBy::Cancel => "the batch was cancelled",
            CutBy::GameClosed => "the game closed",
            CutBy::Stopped => "the batch stopped",
            CutBy::StudioClosed => "DoD Studio closed",
        }
    }
}

/// Marks every take whose video stops well before its audio as partial
/// (#1105): not captured, not renderable, so Finish clips and the take
/// clean-up leave it alone. Takes with nothing on disk, or whose lengths
/// can't be read cheaply (OBS, AGR), are left as they are.
///
/// With `settle`, a take that looks short is looked at once more after it:
/// the game is killed the moment the batch ends, and HLAE's FFmpeg may still
/// be writing the last take's AVI header, which reads as 0 frames until it
/// is finalised.
fn mark_short_takes(
    manifest: &CaptureManifest,
    verified: &mut [VerifiedBlock],
    settle: Option<std::time::Duration>,
) {
    let judge = |i: usize| {
        native::hlcr::take_length::short_take(&manifest.blocks[i].take_folder, manifest.capture_fps)
    };
    let mut short: Vec<(usize, native::hlcr::take_length::ShortTake)> = verified
        .iter()
        .enumerate()
        .filter(|(_, v)| v.captured && !v.rendered)
        .filter_map(|(i, _)| judge(i).map(|s| (i, s)))
        .collect();
    if let Some(settle) = settle
        && !short.is_empty()
    {
        std::thread::sleep(settle);
        short = short
            .into_iter()
            .filter_map(|(i, _)| judge(i).map(|s| (i, s)))
            .collect();
    }
    for (i, s) in short {
        let v = &mut verified[i];
        v.short_take = Some(s);
        v.cut_short = true;
        v.captured = false;
        v.renderable = false;
    }
}

/// The clip recordings a batch's `START_RECORD` / `STOP_RECORD` markers
/// reported, for a Cancel (#1081).
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Records {
    started: usize,
    stopped: usize,
    /// The demo tick the last `START_RECORD` named: its block's
    /// `record_start_tick` (#861).
    last_start_tick: Option<i64>,
    /// The demo playing, by `DEMO_START`'s 1-based index.
    demo: Option<u32>,
    /// The demo the last `START_RECORD` came from.
    last_start_demo: Option<u32>,
}

impl Records {
    /// The demo playing, by `DEMO_START`'s 1-based index; `None` before the
    /// first one, or without markers.
    pub(crate) fn demo(&self) -> Option<u32> {
        self.demo
    }

    /// The demo the last `START_RECORD` came from.
    pub(crate) fn last_start_demo(&self) -> Option<u32> {
        self.last_start_demo
    }

    /// The demo tick the last `START_RECORD` named.
    pub(crate) fn last_start_tick(&self) -> Option<i64> {
        self.last_start_tick
    }

    /// `DEMO_START`: a clip the last demo left recording (it ended or stopped
    /// playing mid-clip, and the hook's batch skipped on) is over, so a
    /// Cancel later in the batch can't count it as still recording.
    pub(crate) fn demo_started(&mut self, job_idx: u32) {
        self.stopped = self.stopped.max(self.started);
        self.demo = Some(job_idx);
    }

    pub(crate) fn clip_started(&mut self, tick: Option<i64>) {
        self.started += 1;
        self.last_start_tick = tick;
        self.last_start_demo = self.demo;
    }

    pub(crate) fn clip_stopped(&mut self) {
        self.stopped += 1;
    }

    /// A demo started playing, so the batch may have left takes on disk.
    fn reached_game(&self) -> bool {
        self.demo.is_some() || self.started > 0
    }
}

/// How a batch that reached the game ended, for [`finish_batch`].
struct BatchEnd<'a> {
    /// The footer's last line.
    status: &'a str,
    /// The manifest file's outcome (#19): "complete", "cancelled" or
    /// "error". Anything but "planned" keeps the next start's catch-up
    /// (#545) from checking the batch again.
    outcome: &'a str,
    /// The batch failed: the line and toasts say so.
    error: bool,
    /// The clip recordings the markers reported, for [`mark_cut_short`].
    records: Records,
    cut_by: CutBy,
    /// What the game and OBS said about each clip (#1109, #1401).
    clips: ClipEvidence,
}

impl<'a> BatchEnd<'a> {
    fn complete() -> Self {
        BatchEnd {
            status: "Complete",
            outcome: "complete",
            error: false,
            records: Records::default(),
            cut_by: CutBy::Cancel,
            clips: ClipEvidence::default(),
        }
    }

    fn cancelled(records: Records) -> Self {
        BatchEnd {
            status: "Cancelled",
            outcome: "cancelled",
            error: false,
            records,
            cut_by: CutBy::Cancel,
            clips: ClipEvidence::default(),
        }
    }

    /// What ended a batch that stopped early; `None` for a complete one,
    /// whose `cut_by` is only a default.
    fn ended_by(&self) -> Option<CutBy> {
        (self.outcome != "complete").then_some(self.cut_by)
    }

    fn with_clips(self, clips: ClipEvidence) -> BatchEnd<'a> {
        BatchEnd { clips, ..self }
    }

    /// The engine stopped the batch with `message`. `game_closed`: the game
    /// went away by itself rather than being ended by the engine.
    fn failed(message: &str, records: Records, game_closed: bool) -> BatchEnd<'_> {
        BatchEnd {
            status: message,
            outcome: "error",
            error: true,
            records,
            cut_by: if game_closed {
                CutBy::GameClosed
            } else {
                CutBy::Stopped
            },
            clips: ClipEvidence::default(),
        }
    }
}

/// Marks the clip the batch's end cut short (#1081), with what ended it.
/// With a clip still recording
/// (more starts than stops), it is the block of the demo playing whose
/// `record_start_tick` is the tick the last `START_RECORD` named. Without a
/// demo, the first block from position `stopped` in recording order with that
/// tick: a demo the hook's batch skipped sends no markers, so the recording
/// clip can sit further on than the count says (#861's `resolve_block` does
/// the same). With no tick, or none matching, the block at `stopped`.
/// Without markers both counts are 0 and nothing changes.
pub(crate) fn mark_cut_short(
    manifest: &CaptureManifest,
    verified: &mut [VerifiedBlock],
    records: Records,
    cut_by: CutBy,
) {
    if records.started <= records.stopped {
        return;
    }
    let order = recording_order(&manifest.blocks);
    let demo = demo_ordinals(&manifest.blocks);
    let by_tick = records.last_start_tick.and_then(|tick| {
        let starts_at = |i: usize| i64::from(manifest.blocks[i].record_start_tick) == tick;
        match records.last_start_demo {
            Some(d) => order.iter().find(|&&i| demo[i] == d && starts_at(i)),
            None => order.iter().skip(records.stopped).find(|&&i| starts_at(i)),
        }
    });
    let Some(&i) = by_tick.or_else(|| order.get(records.stopped)) else {
        return;
    };
    let block = &mut verified[i];
    // A cut clip's take is short too: the batch's end is the reason it gives.
    block.cut_short = block.captured || block.cut_short;
    if block.cut_short {
        block.cut_by = Some(cut_by);
    }
    block.short_take = None;
    block.captured = false;
    block.renderable = false;
}

/// One OBS drop-rate report as the `capture_obs_drop_rate` event (#172):
/// shares as percentages, the clip's take folder spelled as the manifest
/// spells it, so the Last Batch panel can put it on its row.
fn obs_drop_rate_payload(report: &native::obs::ObsDropReport) -> serde_json::Value {
    let pct = |share: f64| (share * 1000.0).round() / 10.0;
    serde_json::json!({
        "clip": report.clip,
        "take_folder": report.take_folder.as_ref().map(|p| p.to_string_lossy().into_owned()),
        "recording": report.recording,
        "skipped_pct": pct(report.clip_counts.skipped_share()),
        "missed_pct": pct(report.clip_counts.missed_share()),
        "skipped": report.clip_counts.output_skipped,
        "frames": report.clip_counts.output_total,
        "missed": report.clip_counts.render_skipped,
        "rendered": report.clip_counts.render_total,
        "recent_skipped_pct": pct(report.recent.skipped_share()),
        "recent_missed_pct": pct(report.recent.missed_share()),
        "lagging": report.lagging,
        "capture_fps": report.capture_fps,
    })
}

/// Everything under `path`, in bytes. 0 for a folder that isn't there.
fn folder_bytes(path: &Path) -> u64 {
    native::sys::folder_walk::Walker::new()
        .files(path)
        .iter()
        .map(|file| std::fs::metadata(file).map(|m| m.len()).unwrap_or(0))
        .sum()
}

fn take_folder_has_content(path: &Path) -> bool {
    // Our own metadata does not count as captured output. It is written into
    // this folder after verification, so it cannot affect the batch that made
    // it — but a re-verification of an older session would otherwise report a
    // capture that never happened.
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .flatten()
                .any(|e| !native::hlcr::take_meta::is_metadata(&e.file_name()))
        })
        .unwrap_or(false)
}

/// Whether a block's take was rendered and then deleted while the batch ran
/// (Render While Capturing, #861): its folder keeps only the render history,
/// which shows a finished render. That take was captured.
fn rendered_while_capturing(block_folder: &Path) -> bool {
    native::hlcr::take_meta::read_history(block_folder)
        .iter()
        .any(|r| r.outcome == native::hlcr::take_meta::RenderOutcome::Finished)
}

/// Checks each planned block against disk after a batch ends.
///
/// Two tiers on purpose: "captured" is the loose check that drives status, so
/// an unanticipated HLAE output layout degrades to a warning rather than
/// silently marking nothing; "renderable" reuses Render Studio's own admission
/// predicate so the two views can't disagree without saying so.
///
/// `on_checked(done)` is called as each take's size is read, the last step
/// and the slow one on a frame sequence's thousands of files (#952).
fn verify_capture_takes(
    manifest: &CaptureManifest,
    on_checked: &mut dyn FnMut(usize),
) -> Vec<VerifiedBlock> {
    verify_capture_takes_settling(
        manifest,
        Some(std::time::Duration::from_millis(1500)),
        on_checked,
    )
}

/// [`verify_capture_takes`], with the re-check of misses after `settle`, or
/// none: a batch caught up on after a restart (#545) ended long ago.
pub(crate) fn verify_capture_takes_settling(
    manifest: &CaptureManifest,
    settle: Option<std::time::Duration>,
    on_checked: &mut dyn FnMut(usize),
) -> Vec<VerifiedBlock> {
    let mut verified: Vec<VerifiedBlock> = manifest
        .blocks
        .iter()
        .map(|block| {
            let has_content = take_folder_has_content(&block.take_folder);
            let rendered = !has_content && rendered_while_capturing(&block.take_folder);
            VerifiedBlock {
                take_key: block.take_key.clone(),
                take_folder: block.take_folder.to_string_lossy().into_owned(),
                demo_name: block.demo_name.clone(),
                block_index: block.block_index,
                source_streak_indices: block.source_streak_indices.clone(),
                captured: has_content || rendered,
                renderable: native::hlcr::scanner::is_renderable_take(&block.take_folder),
                rendered,
                bytes: 0,
                cut_short: false,
                cut_by: None,
                short_take: None,
                started_late_ms: None,
                clip_verdict: None,
                ramped_gaps: block.ramped_gaps,
                not_reached: false,
            }
        })
        .collect();

    // HLAE is force-killed the moment the exit trigger appears, so a take can
    // still be mid-flush when we first look. Re-check only the misses once.
    if let Some(settle) = settle
        && verified.iter().any(|v| !v.captured)
    {
        std::thread::sleep(settle);
        for (v, block) in verified.iter_mut().zip(manifest.blocks.iter()) {
            if !v.captured {
                v.captured = take_folder_has_content(&block.take_folder);
            }
            if !v.renderable {
                v.renderable = native::hlcr::scanner::is_renderable_take(&block.take_folder);
            }
        }
    }
    mark_short_takes(manifest, &mut verified, settle);
    for (i, (v, block)) in verified.iter_mut().zip(manifest.blocks.iter()).enumerate() {
        v.bytes = folder_bytes(&block.take_folder);
        on_checked(i + 1);
    }

    verified
}

/// Records what each take was captured at, in the take's own block folder.
///
/// Per take rather than per session so the record travels with the take. Two
/// batches at different rates already land in different session folders, so a
/// session-level file would be right for that — until somebody moves a take,
/// at which point it would silently inherit the settings of wherever it landed.
///
/// Runs **after** verification and only for blocks that actually landed. A
/// metadata file in a block that produced nothing would be misleading on its
/// own, and `take_folder_has_content` decides a take was captured by asking
/// whether its folder is non-empty — that check now skips our own file, so the
/// ordering here is belt-and-braces rather than the only thing holding it up.
///
/// Best-effort throughout: a capture that succeeded must never be reported as
/// failed because a metadata file could not be written.
pub(crate) fn record_capture_settings(manifest: &CaptureManifest, blocks: &[VerifiedBlock]) {
    if manifest.capture_fps <= 0 {
        return;
    }
    let meta = if manifest.capture_mode == native::patch::CaptureMode::Agr.to_str_id() {
        native::hlcr::take_meta::SessionMeta::agr(
            manifest.session_id.clone(),
            manifest.capture_fps,
            manifest.agr_fps,
        )
    } else {
        native::hlcr::take_meta::SessionMeta::new(manifest.session_id.clone(), manifest.capture_fps)
    };

    for block in blocks.iter().filter(|b| b.captured) {
        let folder = Path::new(&block.take_folder);
        // A take rendered while the batch ran (#861) already has a history;
        // writing the settings keeps it.
        let meta = native::hlcr::take_meta::SessionMeta {
            renders: native::hlcr::take_meta::read_history(folder),
            // What the plan held for this block, so the take's length and
            // kills can be checked against it (#1084).
            plan: manifest
                .blocks
                .iter()
                .find(|b| b.take_folder == folder)
                .and_then(|b| b.plan.clone()),
            // Everything else the batch captured with (part of #1085).
            settings: manifest.settings.clone(),
            ..meta.clone()
        };
        if let Err(e) = native::hlcr::take_meta::write(folder, &meta) {
            log_markdown(
                &crate::messages::capture::take_verify_settings_not_recorded(folder.display(), e),
            );
        }
    }
}

/// What `check_takes` found: the `capture_takes_verified` payload and the
/// counts the batch's last status line gives (#952).
struct TakeReport {
    payload: serde_json::Value,
    captured: usize,
    total: usize,
    /// Captured takes shorter than planned (#1401).
    short: usize,
}

/// Runs verification for the batch that just ended, with "Checking the
/// takes: N of M" on the status line while it runs (#952, throttled to
/// ~33 ms like the patch line). `end.outcome` goes into the manifest file
/// (#19); `end.records` and `end.cut_by` find the clip the end cut short
/// ([`mark_cut_short`]). `None` when the batch planned no takes.
fn check_takes(
    app: &tauri::AppHandle,
    manifest_slot: &Arc<Mutex<Option<CaptureManifest>>>,
    end: &BatchEnd,
    rendered_while_capturing: bool,
) -> Option<TakeReport> {
    let outcome = end.outcome;
    let manifest = {
        let guard = lock_or_recover(manifest_slot);
        match guard.as_ref() {
            Some(m) if !m.blocks.is_empty() => m.clone(),
            _ => return None,
        }
    };

    let total = manifest.blocks.len();
    let emit_checking = |done: usize| {
        let _ = app.emit(
            "capture_status",
            serde_json::json!({
                "running": true,
                "stage": "checking",
                "index": done,
                "total": total,
                "status": crate::messages::checking_takes(done, total),
            }),
        );
    };
    emit_checking(0);
    let mut last_emit = std::time::Instant::now();
    let mut blocks = verify_capture_takes(&manifest, &mut |done| {
        if done == total || last_emit.elapsed() >= std::time::Duration::from_millis(33) {
            last_emit = std::time::Instant::now();
            emit_checking(done);
        }
    });
    mark_cut_short(&manifest, &mut blocks, end.records, end.cut_by);
    let short = crate::clip_verdicts::apply(&manifest, &mut blocks, &end.clips);
    // Only a batch that ended early left clips behind; a complete one
    // reached them all, and a take it lacks is missing.
    if outcome != "complete" {
        let not_reached =
            crate::unreached_clips::mark_not_reached(&manifest, &mut blocks, &end.records);
        if not_reached > 0 {
            log_markdown(&crate::messages::capture::take_verify_not_reached(
                not_reached,
                blocks.len(),
                end.cut_by.not_reached_text(),
            ));
        }
    }
    for block in blocks.iter().filter(|b| b.cut_short) {
        let why = match block.short_take {
            Some(s) => {
                crate::messages::capture::take_stopped_writing_video(s.video_secs, s.audio_secs)
            }
            None => block.cut_by.unwrap_or(end.cut_by).log_text().to_string(),
        };
        log_markdown(&crate::messages::capture::take_verify_cut_short(
            &block.take_key,
            why,
            &block.take_folder,
        ));
    }
    let captured_count = blocks.iter().filter(|b| b.captured).count();
    // A take already rendered and deleted (#861) isn't one Render Studio
    // missed.
    let renderable_count = blocks.iter().filter(|b| b.renderable || b.rendered).count();

    log_markdown(&crate::messages::capture::take_verify_session_summary(
        &manifest.session_id,
        captured_count,
        blocks.len(),
        renderable_count,
    ));
    for block in &blocks {
        log_markdown(&crate::messages::capture::take_verify_block_line(
            &block.take_key,
            block.captured,
            block.renderable,
            &block.take_folder,
        ));
    }

    // An AGR take's sound goes beside its `.agr` (#450).
    if manifest.capture_mode == native::patch::CaptureMode::Agr.to_str_id() {
        native::hlcr::agr_take::place_batch_sounds(&manifest.blocks);
    }
    record_capture_settings(&manifest, &blocks);
    crate::manifest_file::write(&manifest, outcome, Some(&blocks));

    Some(TakeReport {
        payload: serde_json::json!({
            "session_id": manifest.session_id,
            "total_count": blocks.len(),
            "captured_count": captured_count,
            "renderable_count": renderable_count,
            "blocks": blocks,
            // Both read by the frontend's automatic finish step (#440): it
            // only finishes a batch that ran to completion, and a frame
            // sequence has to be encoded at the rate it was captured at.
            "outcome": outcome,
            // What ended a batch that stopped early, for its clips it never
            // reached (#1434).
            "ended_by": end.ended_by(),
            "capture_fps": manifest.capture_fps,
            // The project's record of each take (part of #1085).
            "capture_settings": manifest.settings,
            // Render While Capturing (#861) queues every take itself, the
            // last ones after this goes out, so the finish step stays out.
            "rendered_while_capturing": rendered_while_capturing,
        }),
        captured: captured_count,
        short,
        total,
    })
}

/// A batch that ran (to its end, until cancelled, or until it failed in the
/// game) finishes: its takes are checked, then the running flag drops and
/// the last status line goes out with how many clips landed, then the
/// check's result. In that order: the footer's last line comes before what
/// Finish clips (#440) puts on it.
fn finish_batch(
    app: &tauri::AppHandle,
    is_running: &Arc<Mutex<bool>>,
    manifest_slot: &Arc<Mutex<Option<CaptureManifest>>>,
    end: BatchEnd,
    rendered_while_capturing: bool,
) {
    let report = check_takes(app, manifest_slot, &end, rendered_while_capturing);
    *lock_or_recover(is_running) = false;
    let mut status = serde_json::json!({
        "running": false,
        "status": end.status,
        "clips_captured": report.as_ref().map(|r| r.captured),
        "clips_total": report.as_ref().map(|r| r.total),
        "clips_short": report.as_ref().map(|r| r.short),
    });
    if end.error {
        status["error"] = serde_json::Value::Bool(true);
    }
    let _ = app.emit("capture_status", status);
    if let Some(report) = report {
        // An OBS capture test's clip (#646) is the test's alone: it must not
        // mark highlights captured, fill Last Batch or start Finish clips.
        let test = report.payload["session_id"]
            .as_str()
            .is_some_and(crate::obs_test_manager::is_test_session);
        let event = if test {
            "obs_test_takes_verified"
        } else {
            "capture_takes_verified"
        };
        let _ = app.emit(event, report.payload);
    }
}

// ── Public command handler ─────────────────────────────────────────────────────

/// `Some(message)` when `init_commands` or any `custom_commands` entry is too
/// long for a demo's ConsoleCommand frame (`native::patch::too_long_commands`),
/// names a `native::patch::cfg_scan::BANNED_COMMANDS` cvar (refused everywhere), or
/// `custom_commands` names a `SCHEDULED_BANNED_COMMANDS` one (fine in Initial
/// Commands, refused only here) — the first one found, checking the
/// everywhere-banned tier before the scheduled-only tier. `None` means clean.
///
/// Pure so it can be tested without a `tauri::AppHandle`; `start_capture_batch_impl`
/// is the only caller.
fn first_banned_command_error(
    init_commands: &[String],
    custom_commands: &[CustomCommandPayload],
) -> Option<String> {
    let custom_texts: Vec<String> = custom_commands.iter().map(|c| c.command.clone()).collect();

    // Each command is written into one ConsoleCommand frame as typed; one that
    // does not fit used to abort the whole app partway through patching (#453).
    let mut too_long = native::patch::too_long_commands(init_commands);
    too_long.extend(native::patch::too_long_commands(&custom_texts));
    if let Some(command) = too_long.first() {
        return Some(format!(
            "\"{command}\" is {} bytes — Initial and Scheduled Commands must be under {} bytes each to fit in a demo. Split it into shorter commands.",
            command.len(),
            native::patch::MAX_CONSOLE_CMD_LEN
        ));
    }

    let mut banned = native::patch::cfg_scan::banned_commands(init_commands);
    banned.extend(native::patch::cfg_scan::banned_commands(&custom_texts));
    if let Some((cvar, command)) = banned.first() {
        return Some(format!(
            "\"{command}\" is not allowed in Initial or Scheduled Commands — {cvar} is controlled entirely by the app."
        ));
    }

    // Fine in Initial Commands — that's how the decal flush is meant to be
    // configured — but the flush sizes itself against these once, before the
    // demo plays, so a Scheduled Command changing one mid-demo would silently
    // break that sizing rather than merely surprise.
    let scheduled_banned = native::patch::cfg_scan::scheduled_banned_commands(&custom_texts);
    scheduled_banned.first().map(|(cvar, command)| {
        format!(
            "\"{command}\" is not allowed in Scheduled Commands — {cvar} must not change mid-demo; set it in Initial Commands instead."
        )
    })
}

/// Async entry point called by the Tauri `start_capture_batch` command.
pub async fn start_capture_batch_impl(
    app_handle: tauri::AppHandle,
    manager: &CaptureManager,
    payload: CapturePayload,
) -> Result<(), String> {
    // ── Guard: banned commands (native::patch::cfg_scan::BANNED_COMMANDS) ─────
    // The frontend already disables Start Capture Batch while one is present
    // (capture_pane.js's refreshLaunchGuard) — this is the same "the frontend
    // check is only as good as the frontend" reasoning as the separate-HUD
    // guard below: whatever reaches this command directly must be refused
    // independently, not trusted because some UI state claimed it was clean.
    if let Some(err) = first_banned_command_error(&payload.init_commands, &payload.custom_commands)
    {
        return Err(err);
    }

    // ── Guard: an AGR batch needs its own rate (the UI blocks it too) ─────────
    crate::capture_guards::require_agr_rate(&config_from_payload(&payload))?;

    // ── Guard: separate HUD cannot be captured through mirv_movie_ffmpeg ──────
    // HLAE writes blank frames to the HUD stream on the FFmpeg path — measured
    // both ways against one demo, see docs/direct_to_video_capture.md. Capture
    // and render both report success and the clip is a black rectangle, so
    // nothing downstream can catch it. Checked here as well as in the launch
    // guard because the frontend check is only as good as the frontend.
    //
    // Rejected before the running flag is claimed, so a refused batch does not
    // leave the manager wedged.
    // ── Guard: reject concurrent batches ──────────────────────────────────────
    {
        let mut running = lock_or_recover(&manager.is_running);
        if *running {
            return Err(crate::messages::CAPTURE_BATCH_ALREADY_RUNNING.to_string());
        }
        *running = true;
    }

    // ── Reset cancel token from any prior run ──────────────────────────────────
    manager
        .cancel_token
        .store(false, std::sync::atomic::Ordering::Relaxed);
    manager
        .batch_start
        .set(native::game_handoff::StartState::Preparing);

    // ── Build PatcherConfig from the IPC payload ───────────────────────────────
    let mut patcher_config = config_from_payload(&payload);
    // Configuration > Players' box (#428) is a saved setting, not part of
    // the batch's payload.
    patcher_config.player_overrides = Some(saved_settings(&app_handle).players.use_in_game);
    // Render While Capturing (#861) renders each take as it lands. Only a
    // batch the hook runs can hold for space, and only deleted takes free
    // any, so only then may the planner route more than the drives hold now.
    let render_while_capturing = crate::capture_room_manager::enabled(&app_handle)
        && patcher_config.capture_mode != native::patch::CaptureMode::Agr;
    patcher_config.render_while_capturing = render_while_capturing
        && patcher_config.hook_runs_batch
        && saved_settings(&app_handle).take_after_finish == "delete";
    {
        let mut slot = lock_or_recover(&manager.last_config);
        *slot = Some(patcher_config.clone());
    }

    let reuse_game_pid = payload.reuse_game_pid;
    // For the manifest (#545): which highlight each payload index is.
    let streak_uids = payload.streak_uids.clone();
    let streak_sources: Vec<String> = payload
        .streaks
        .iter()
        .map(|s| s.source_demo.clone())
        .collect();
    let project_path = payload.project_path.clone().filter(|p| !p.is_empty());

    // ── Convert IPC streaks → native CaptureStreak ────────────────────────────
    let raw_streaks: Vec<CaptureStreak> = payload
        .streaks
        .into_iter()
        .map(CaptureStreak::from)
        .collect();

    if raw_streaks.is_empty() {
        let mut running = lock_or_recover(&manager.is_running);
        *running = false;
        let _ = app_handle.emit(
            "capture_status",
            serde_json::json!({
                "running": false,
                "error": true,
                "status": crate::messages::NO_STREAKS_IN_PAYLOAD
            }),
        );
        return Err(crate::messages::NO_STREAKS_IN_PAYLOAD.to_string());
    }

    // ── Clone Arcs into the worker closure ────────────────────────────────────
    let is_running_arc = Arc::clone(&manager.is_running);
    let cancel_token_arc = Arc::clone(&manager.cancel_token);
    let batch_start_arc = Arc::clone(&manager.batch_start);
    let manifest_arc = Arc::clone(&manager.last_manifest);
    let hl_path = PathBuf::from(&patcher_config.game_path);

    let app_handle_clone = app_handle.clone();

    // ── Offload blocking I/O to a dedicated thread ────────────────────────────
    tokio::task::spawn_blocking(move || {
        // The first line at once (#952): the steps before the first count
        // (the unchanged check below, copies beside the game) report none.
        let _ = app_handle_clone.emit(
            "capture_status",
            serde_json::json!({
                "running": true,
                "stage": "preparing",
                "index": 0,
                "total": 0,
                "measured": false,
                "status": crate::messages::GETTING_DEMOS_READY,
            }),
        );
        // A different demo saved under a scanned one's name would take every
        // highlight tick with it and crash the game minutes in (#196). Stop
        // before anything is patched.
        if let Err(e) = native::patch::check_sources_unchanged(&raw_streaks)
            .map_err(crate::messages::failed_to_check_demo_sources)
        {
            log_markdown(&crate::messages::capture::capture_error_line(&e));
            let mut running = lock_or_recover(&is_running_arc);
            *running = false;
            let _ = app_handle_clone.emit(
                "capture_status",
                serde_json::json!({
                    "running": false,
                    "error": true,
                    "status": e
                }),
            );
            return;
        }

        // #644: with "Only when space is needed", finished takes make room on
        // the capture drives before build_batch_queue routes blocks to them.
        // The Capture footer shows each step.
        if crate::take_cleanup_manager::makes_room(&app_handle_clone) {
            let need = native::hlcr::take_cleanup::SpaceNeed {
                targets: patcher_config.capture_directories.clone(),
                pieces: native::patch::estimate_batch_capture_blocks(&raw_streaks, &patcher_config),
                fallback_pieces: None,
                headroom_bytes: native::sys::disk::MIN_DRIVE_HEADROOM_BYTES,
            };
            crate::take_cleanup_manager::make_room(
                &app_handle_clone,
                crate::take_cleanup_manager::RoomFor::Capture,
                need,
                Some(&cancel_token_arc),
                |line, done, total| {
                    let _ = app_handle_clone.emit(
                        "capture_status",
                        serde_json::json!({
                            "running": true,
                            "status": line,
                            "index": done,
                            "total": total
                        }),
                    );
                },
            );
            if cancel_token_arc.load(std::sync::atomic::Ordering::Relaxed) {
                let mut running = lock_or_recover(&is_running_arc);
                *running = false;
                let _ = app_handle_clone.emit(
                    "capture_status",
                    serde_json::json!({
                        "running": false,
                        "status": crate::messages::CAPTURE_CANCELLED_WHILE_MAKING_ROOM
                    }),
                );
                return;
            }
        }

        // Kept for the hook-run route, which places each tick by the kills'
        // demo-player times (#434). A highlight from a project saved before
        // those existed gets them from a fresh read of its demo first.
        let planned_streaks = if patcher_config.hook_runs_batch {
            let mut raw_streaks = raw_streaks.clone();
            crate::hook_batch::refresh_viewdemo_times(&app_handle_clone, &mut raw_streaks);
            raw_streaks
        } else {
            Vec::new()
        };
        let planned_capture_bytes = if render_while_capturing {
            native::patch::estimate_batch_capture_bytes(&raw_streaks, &patcher_config)
        } else {
            0
        };
        let (mut patch_jobs, drive_headroom) = match build_batch_queue(
            raw_streaks,
            &patcher_config,
            &std::collections::HashMap::new(),
        ) {
            Ok(v) => v,
            Err(e) => {
                log_markdown(&crate::messages::capture::build_batch_queue_failed(&e));
                let mut running = lock_or_recover(&is_running_arc);
                *running = false;
                let _ = app_handle_clone.emit(
                    "capture_status",
                    serde_json::json!({
                        "running": false,
                        "error": true,
                        // The error toast's text; the log line above names the step.
                        "status": e.to_string()
                    }),
                );
                return;
            }
        };

        // Built from the same flattened walk as the manifest, deliberately
        // adjacent to it. In OBS mode the engine consumes these positionally as
        // each block's AUDIO_SYNC marker arrives, so the two orders have to
        // agree — computing them apart is how they would come to disagree.
        let obs_take_folders: Vec<std::path::PathBuf> = patch_jobs
            .iter()
            .flat_map(|j| j.blocks.iter().map(|b| b.take_folder.clone()))
            .collect();

        {
            let manifest = CaptureManifest {
                session_id: patcher_config.session_id.clone(),
                blocks: patch_jobs
                    .iter()
                    .flat_map(|j| j.blocks.iter().cloned())
                    .collect(),
                capture_fps: patcher_config.capture_fps,
                capture_mode: patcher_config.capture_mode.to_str_id().to_string(),
                agr_fps: patcher_config.effective_agr_fps(),
                streak_uids,
                streak_sources,
                project_path,
                settings: Some(native::hlcr::take_settings::CaptureSettings::from_config(
                    &patcher_config,
                    env!("CARGO_PKG_VERSION"),
                )),
            };
            if let Some(settings) = &manifest.settings {
                log_markdown(&crate::messages::capture::take_settings_recorded(settings));
            }
            // On disk as well (#19), so a batch that goes wrong leaves a record
            // behind after the process is gone. Rewritten with outcomes at the end.
            crate::manifest_file::write(&manifest, "planned", None);
            let mut slot = lock_or_recover(&manifest_arc);
            *slot = Some(manifest);
        }

        if patch_jobs.is_empty() {
            log_markdown(crate::messages::capture::BUILD_BATCH_QUEUE_NO_JOBS);
            let mut running = lock_or_recover(&is_running_arc);
            *running = false;
            let _ = app_handle_clone.emit(
                "capture_status",
                serde_json::json!({
                    "running": false,
                    "status": crate::messages::capture::STATUS_NO_JOBS
                }),
            );
            return;
        }

        // ── A demo over the engine's entity limit plays a trimmed copy (#207) ──
        let crate::trimmed_demos::BatchCopies {
            copies: trimmed,
            pins: trim_pins,
        } = match crate::trimmed_demos::for_batch(
            &app_handle_clone,
            &patch_jobs,
            &patcher_config.game_path,
            &cancel_token_arc,
        ) {
            Ok(batch) => batch,
            Err(status) => {
                let mut running = lock_or_recover(&is_running_arc);
                *running = false;
                let _ = app_handle_clone.emit(
                    "capture_status",
                    serde_json::json!({
                        "running": false,
                        "error": true,
                        "status": status
                    }),
                );
                return;
            }
        };

        // ── Or hand the plan to the hook DLL instead of patching (#434) ─────────
        let runtime_batch = if patcher_config.hook_runs_batch {
            match crate::hook_batch::prepare(
                &patch_jobs,
                &planned_streaks,
                &patcher_config,
                &trimmed,
            ) {
                Ok(mut batch) => {
                    batch.reuse_game = reuse_game_pid;
                    Some(batch)
                }
                // A patched batch starts the game on its first demo, so it
                // can't run in the open game the frontend picked (#725).
                Err(why) if reuse_game_pid.is_some() => {
                    let status = crate::messages::batch_needs_fresh_game(&why);
                    log_markdown(&crate::messages::capture::batch_not_started_line(&status));
                    let mut running = lock_or_recover(&is_running_arc);
                    *running = false;
                    let _ = app_handle_clone.emit(
                        "capture_status",
                        serde_json::json!({
                            "running": false,
                            "error": true,
                            "status": status
                        }),
                    );
                    return;
                }
                Err(why) => {
                    log_markdown(&crate::messages::capture::batch_patched_instead_of_seeking(
                        why,
                    ));
                    // The Commands tab checked these for a seeking batch, which
                    // runs them all; a patched one puts them in the demo, where
                    // the game's filter drops some (`cfg_scan::demo_filter_rule`).
                    let mut commands = patcher_config.init_commands.clone();
                    commands.extend(
                        patcher_config
                            .custom_commands
                            .iter()
                            .map(|c| c.command.clone()),
                    );
                    if let Some(line) = crate::messages::dropped_when_patched(
                        &native::patch::cfg_scan::demo_filtered_commands(&commands),
                    ) {
                        log_markdown(&line);
                    }
                    None
                }
            }
        } else {
            None
        };

        // Fast-forwarded gaps need the hook's demo player (#831): a patched
        // batch records them at real time, and its takes' labels say so.
        if runtime_batch.is_none() {
            let kept = native::patch::builder::kill_gaps::keep_gaps(&mut patch_jobs);
            if kept > 0 {
                log_markdown(&native::patch::builder::kill_gaps::kept_gaps_message(kept));
                // The manifest was written before the route was known, and
                // its blocks carry the plans the takes are checked against.
                let mut slot = lock_or_recover(&manifest_arc);
                if let Some(manifest) = slot.as_mut() {
                    manifest
                        .blocks
                        .iter_mut()
                        .for_each(native::patch::builder::kill_gaps::keep_block_gaps);
                }
            }
        }

        // ── Write each job's patched demo before the capture engine copies it ───
        // build_batch_queue only plans where each patched demo should land
        // (output_demo paths, drive routing, scheduled commands) — it never
        // writes the bytes. Do that here, one StreamPatcher pass per job,
        // before spawn_capture_engine tries to copy patched_demo_path into
        // the game's dod/ directory (it would otherwise fail with "file not
        // found" since output_demo never existed).
        // Known upfront from the same per-job block lists build_batch_queue
        // already produced — cheap, and lets the demo-loading notification
        // show "X of Y clips total" without any per-demo lookback.
        let total_batch_clips: u32 = patch_jobs.iter().map(|j| j.blocks.len() as u32).sum();
        // Each demo with clips, in the order the game plays them: DEMO_START's
        // 1-based index counts these (the primer job has none), and the
        // status line names the demo by it (#952).
        let demo_names: Vec<String> = patch_jobs
            .iter()
            .filter(|j| !j.blocks.is_empty())
            .map(|j| demo_file_name(&j.source_demo))
            .collect();
        // What each clip records, in batch order, for the time left (#643).
        let clip_frames: Vec<i64> = patch_jobs
            .iter()
            .flat_map(|j| j.blocks.iter())
            .map(|b| i64::from(b.record_stop_tick) - i64::from(b.record_start_tick))
            .collect();
        // Render While Capturing (#861): follows the batch clip by clip.
        let capture_room = render_while_capturing.then(|| {
            let blocks: Vec<native::patch::types::CaptureBlock> = patch_jobs
                .iter()
                .flat_map(|j| j.blocks.iter().cloned())
                .collect();
            crate::capture_room_manager::CaptureRoom::new(
                app_handle_clone.clone(),
                &blocks,
                planned_capture_bytes,
                patcher_config.session_id.clone(),
                patcher_config.capture_fps,
                patcher_config.capture_mode.to_str_id().to_string(),
                Arc::clone(&cancel_token_arc),
                runtime_batch.is_some(),
            )
        });
        let hold = capture_room
            .as_ref()
            .map(|room| Arc::clone(&room.hold))
            .unwrap_or_default();
        // Every way out before the engine starts ends it too.
        let mut room_guard = RoomGuard(capture_room.clone());
        // The hook plays the original demos: nothing to patch or copy.
        let mut patch_jobs = if runtime_batch.is_some() {
            Vec::new()
        } else {
            patch_jobs
        };
        // A patched demo over the entity limit is patched from its trimmed
        // copy (#207). Its frames are the original's, so every tick holds.
        for job in &mut patch_jobs {
            if let Some(copy) = trimmed.get(&job.source_demo) {
                job.source_demo = copy.to_string_lossy().into_owned();
            }
        }
        let total_patch_jobs = patch_jobs.len() as u32;
        // One start + one end notification for the whole patching phase, not
        // per-demo like capture_demo_loading -- decal clearing means patching
        // is no longer instant, but a toast per demo patched would still be
        // noise (see issue #98 discussion).
        if total_patch_jobs > 0 {
            let _ = app_handle_clone.emit(
                "capture_patching_started",
                serde_json::json!({
                    "total": total_patch_jobs,
                }),
            );
        }
        // Completed-count progress rather than positional index -- jobs
        // finish out of order once patched concurrently, so "which index is
        // running" no longer means anything.
        let next_index = std::sync::atomic::AtomicUsize::new(0);
        let completed_count = std::sync::atomic::AtomicU32::new(0);
        // Concurrency is otherwise invisible to the UI: a "completed / total"
        // counter alone looks identical whether jobs ran one at a time or
        // several at once, especially since individual patch jobs are often
        // fast enough that a small batch never visibly lingers either way.
        let in_flight = std::sync::atomic::AtomicU32::new(0);
        // Only a real patch error populates this -- lets the aggregation
        // below tell "a peer job's failure forced cancel_token_arc true"
        // apart from a genuine user Cancel, which never sets it.
        let first_error: Mutex<Option<(String, std::io::Error)>> = Mutex::new(None);
        let progress = PatchProgress::new(
            patch_jobs
                .iter()
                .map(|j| demo_file_name(&j.source_demo))
                .collect(),
        );
        let emit_progress = |app: &tauri::AppHandle, done: u32, in_progress: u32| {
            let summary = progress.summary(done);
            let _ = app.emit(
                "capture_status",
                serde_json::json!({
                    "running": true,
                    "stage": "preparing",
                    "index": summary.index,
                    "total": total_patch_jobs,
                    "in_progress": in_progress,
                    "measured": summary.measured,
                    "status": summary.status
                }),
            );
        };

        std::thread::scope(|scope| {
            let worker_count = PATCH_CONCURRENCY.min(patch_jobs.len()).max(1);
            for _ in 0..worker_count {
                let app_handle_clone = app_handle_clone.clone();
                let next_index = &next_index;
                let completed_count = &completed_count;
                let in_flight = &in_flight;
                let first_error = &first_error;
                let patch_jobs = &patch_jobs;
                let patcher_config = &patcher_config;
                let cancel_token_arc = &cancel_token_arc;
                let progress = &progress;
                let emit_progress = &emit_progress;
                scope.spawn(move || {
                    loop {
                        if cancel_token_arc.load(std::sync::atomic::Ordering::Relaxed) {
                            break;
                        }
                        let idx = next_index.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if idx >= patch_jobs.len() {
                            break;
                        }
                        let job = &patch_jobs[idx];
                        in_flight.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        // Each stage change, and each percent written (throttled
                        // across the batch), updates the one status line.
                        let result = StreamPatcher::new(&job.source_demo, &job.output_demo)
                            .patch_with_stages(
                                job,
                                patcher_config,
                                cancel_token_arc,
                                &mut |stage| {
                                    if progress.set(idx, stage) {
                                        emit_progress(
                                            &app_handle_clone,
                                            completed_count
                                                .load(std::sync::atomic::Ordering::Relaxed),
                                            in_flight.load(std::sync::atomic::Ordering::Relaxed),
                                        );
                                    }
                                },
                            );
                        progress.finish(idx);
                        let still_running =
                            in_flight.fetch_sub(1, std::sync::atomic::Ordering::Relaxed) - 1;
                        match result {
                            Ok(()) => {
                                let done = completed_count
                                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                                    + 1;
                                emit_progress(&app_handle_clone, done, still_running);
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                                // Either a real user Cancel, or fallout from a
                                // peer job's failure just below -- either way
                                // this job's own output is incomplete.
                                let _ = std::fs::remove_file(&job.output_demo);
                                break;
                            }
                            Err(e) => {
                                {
                                    let mut slot = lock_or_recover(first_error);
                                    if slot.is_none() {
                                        *slot = Some((job.source_demo.clone(), e));
                                    }
                                }
                                // Stop everything else as fast as the existing
                                // interrupt mechanism allows -- a batch is
                                // already all-or-nothing (nothing opens the game
                                // until every job has patched clean), so letting
                                // peers run to completion after this would only
                                // waste CPU on jobs the batch is aborting anyway.
                                cancel_token_arc.store(true, std::sync::atomic::Ordering::Relaxed);
                                break;
                            }
                        }
                    }
                });
            }
        });

        let was_cancelled = cancel_token_arc.load(std::sync::atomic::Ordering::Relaxed);
        let failure = lock_or_recover(&first_error).take();

        if let Some((source_demo, e)) = failure {
            log_markdown(&crate::messages::capture::patch_failed_line(
                &source_demo,
                &e,
            ));
            let mut running = lock_or_recover(&is_running_arc);
            *running = false;
            let _ = app_handle_clone.emit(
                "capture_status",
                serde_json::json!({
                    "running": false,
                    "error": true,
                    "status": crate::messages::capture::patch_failed_status(&source_demo, &e)
                }),
            );
            return;
        }
        if was_cancelled {
            let mut running = lock_or_recover(&is_running_arc);
            *running = false;
            let _ = app_handle_clone.emit(
                "capture_status",
                serde_json::json!({
                    "running": false,
                    "status": "Cancelled"
                }),
            );
            return;
        }

        if total_patch_jobs > 0 {
            let _ = app_handle_clone.emit(
                "capture_patching_finished",
                serde_json::json!({
                    "total": total_patch_jobs,
                }),
            );
        }

        let capture_jobs: Vec<CaptureJob> = patch_jobs
            .into_iter()
            .map(|job| CaptureJob {
                patched_demo_path: job.output_demo,
            })
            .collect();

        let (engine_tx, engine_rx) = std::sync::mpsc::channel();

        // Spawn a listener to clear the running flag when the batch terminates and forward events to frontend
        let is_running_clone = Arc::clone(&is_running_arc);
        let manifest_for_listener = Arc::clone(&manifest_arc);
        let app_emitter = app_handle_clone.clone();
        let is_running_for_panic = Arc::clone(&is_running_arc);
        let app_emitter_for_panic = app_handle_clone.clone();
        let room = capture_room;
        let rendered_while_capturing = room.is_some();
        // The listener ends the room from here on.
        room_guard.0 = None;
        std::thread::spawn(move || {
            // Ends Render While Capturing however the listener ends.
            let _room_guard = RoomGuard(room.clone());
            // The batch's trimmed copies can't be pruned until it ends (#207).
            let _trim_pins = trim_pins;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // `through_current_demo` counts clips through and including the
                // current demo -- updated once per DemoLoading (never per
                // FastForwardToClip), so every fast-forward-to-clip notification
                // within a demo reports the same "X of Y clips total" the
                // demo-loading toast itself would have shown. See issue #98.
                let mut clips = ClipTally::default();
                // Clip recordings, for a Cancel (#1081).
                let mut records = Records::default();
                // The game went away by itself before the batch's end.
                let mut game_closed = false;
                // What the game and OBS said about each clip (#1109, #1401).
                let mut clip_evidence = ClipEvidence::default();
                // A demo by DEMO_START's 1-based index, for the line (#952).
                let demo_name = |job_idx: u32| {
                    (job_idx as usize)
                        .checked_sub(1)
                        .and_then(|i| demo_names.get(i))
                        .cloned()
                        .unwrap_or_default()
                };
                // Time left (#643): from when the engine starts, priced by
                // the clips done so far.
                let eta = crate::batch_eta::BatchEta::new(clip_frames);
                let mut started: Option<std::time::Instant> = None;
                let seconds_left = |started: Option<std::time::Instant>, done: u32| {
                    let elapsed = started?.elapsed().as_secs_f64();
                    eta.seconds_left(done, elapsed)
                };
                // Every line from here on counts clips (#952): the bar is one
                // bar for the whole batch, and the frontend places these on
                // the share after getting the demos ready. `stage` says which
                // step it is; a step that can't be measured (starting the
                // game, loading a demo) keeps the count where it was.
                let emit_step = |stage: &str, done: u32, status: String, left: Option<u64>| {
                    let _ = app_emitter.emit(
                        "capture_status",
                        serde_json::json!({
                            "running": true,
                            "stage": stage,
                            "index": done,
                            "total": total_batch_clips,
                            "status": status,
                            "seconds_left": left,
                        }),
                    );
                };
                while let Ok(event) = engine_rx.recv() {
                    match event {
                        EngineEvent::Starting(_) => {
                            started = Some(std::time::Instant::now());
                            emit_step(
                                "starting",
                                clips.done,
                                crate::messages::starting_the_game(None, demo_names.len()),
                                None,
                            );
                        }
                        EngineEvent::Launching(_) => {
                            emit_step(
                                "starting",
                                clips.done,
                                crate::messages::starting_the_game(
                                    demo_names.first().map(String::as_str),
                                    demo_names.len(),
                                ),
                                None,
                            );
                        }
                        EngineEvent::Finished(_) => {
                            // The game closed: the takes are checked next
                            // (`finish_batch`), which says so on the line.
                            clips.session_finished();
                        }
                        EngineEvent::DemoLoading(job_idx, total, clip_count) => {
                            clips.demo_loading(clip_count);
                            records.demo_started(job_idx);
                            let current_demo = demo_name(job_idx);
                            emit_step(
                                "loading",
                                clips.done,
                                crate::messages::loading_demo(&current_demo),
                                seconds_left(started, clips.done),
                            );
                            let _ = app_emitter.emit(
                                "capture_demo_loading",
                                serde_json::json!({
                                    "index": job_idx,
                                    "total": total,
                                    "clip_count": clip_count,
                                    "clips_so_far": clips.through_current_demo,
                                    "total_batch_clips": total_batch_clips,
                                }),
                            );
                        }
                        EngineEvent::FastForwardToClip(
                            job_idx,
                            total,
                            clip_idx,
                            clip_count_this_demo,
                        ) => {
                            clips.fast_forward_to(clip_idx, clip_count_this_demo);
                            let current_demo = demo_name(job_idx);
                            if total_batch_clips > 0 {
                                emit_step(
                                    "capturing",
                                    clips.done,
                                    crate::messages::capturing_clip(
                                        clips.done + 1,
                                        total_batch_clips,
                                        &current_demo,
                                    ),
                                    seconds_left(started, clips.done),
                                );
                            }
                            let _ = app_emitter.emit(
                                "capture_fast_forward_to_clip",
                                serde_json::json!({
                                    "demo_index": job_idx,
                                    "demo_total": total,
                                    "clip_index": clip_idx,
                                    "clip_count_this_demo": clip_count_this_demo,
                                    "clips_so_far": clips.through_current_demo,
                                    "total_batch_clips": total_batch_clips,
                                }),
                            );
                        }
                        EngineEvent::ObsDropRate(report) => {
                            // About once a second: no throttle needed.
                            let _ = app_emitter
                                .emit("capture_obs_drop_rate", obs_drop_rate_payload(&report));
                        }
                        EngineEvent::ClipStarted(tick) => {
                            records.clip_started(tick);
                            if let Some(room) = &room {
                                room.clip_started(tick);
                            }
                        }
                        EngineEvent::ClipRecorded(tick) => {
                            records.clip_stopped();
                            if let Some(room) = &room {
                                room.clip_recorded(tick);
                            }
                        }
                        EngineEvent::Held => {
                            if let Some(room) = &room {
                                room.game_held(true);
                            }
                        }
                        EngineEvent::Going => {
                            if let Some(room) = &room {
                                room.game_held(false);
                            }
                        }
                        EngineEvent::GameClosed => {
                            game_closed = true;
                        }
                        // Once per change in the game (#1339): no throttle.
                        EngineEvent::KillfeedLayout(warning) => {
                            let _ = app_emitter.emit(
                                "capture_killfeed_layout",
                                serde_json::json!({ "warning": warning }),
                            );
                        }
                        EngineEvent::ObsStartedLate(late) => {
                            if let Some(room) = &room {
                                room.clip_started_late(&late.take_folder);
                            }
                            clip_evidence.late_starts.push(late);
                        }
                        EngineEvent::ClipReported(clip) => clip_evidence.reports.push(clip),
                        // A batch that played a demo before it failed has
                        // takes to check, like a cancelled one: the finished
                        // ones are real, and the clip recording when it
                        // stopped is a partial take. Left unchecked, its
                        // manifest stays "planned" and the next start's
                        // catch-up (#545) counts that partial take captured.
                        EngineEvent::Error(msg) if records.reached_game() => {
                            finish_batch(
                                &app_emitter,
                                &is_running_clone,
                                &manifest_for_listener,
                                BatchEnd::failed(&msg, records, game_closed)
                                    .with_clips(std::mem::take(&mut clip_evidence)),
                                rendered_while_capturing,
                            );
                            break;
                        }
                        EngineEvent::Error(msg) => {
                            let mut running = lock_or_recover(&is_running_clone);
                            *running = false;
                            let _ = app_emitter.emit(
                                "capture_status",
                                serde_json::json!({
                                    "running": false,
                                    "error": true,
                                    "status": msg
                                }),
                            );
                            break;
                        }
                        EngineEvent::AllCompleted => {
                            finish_batch(
                                &app_emitter,
                                &is_running_clone,
                                &manifest_for_listener,
                                BatchEnd::complete().with_clips(std::mem::take(&mut clip_evidence)),
                                rendered_while_capturing,
                            );
                            break;
                        }
                        EngineEvent::Cancelled => {
                            // A cancelled batch still leaves real finished takes on
                            // disk — verify anyway rather than discarding them.
                            // The clip it cut short, if any, isn't one (#1081).
                            finish_batch(
                                &app_emitter,
                                &is_running_clone,
                                &manifest_for_listener,
                                BatchEnd::cancelled(records)
                                    .with_clips(std::mem::take(&mut clip_evidence)),
                                rendered_while_capturing,
                            );
                            break;
                        }
                    }
                }
            }));

            if let Err(panic_payload) = result {
                let msg = panic_payload
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| panic_payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "unknown panic payload".to_string());
                log_markdown(&crate::messages::capture::listener_panicked_line(&msg));
                let mut running = lock_or_recover(&is_running_for_panic);
                *running = false;
                let _ = app_emitter_for_panic.emit(
                    "capture_status",
                    serde_json::json!({
                        "running": false,
                        "error": true,
                        "status": crate::messages::capture::listener_panicked_status(&msg)
                    }),
                );
            }
        });

        // Delegate to the promoted library engine
        spawn_capture_engine(
            capture_jobs,
            Arc::new(hl_path),
            engine_tx,
            cancel_token_arc,
            patcher_config,
            drive_headroom,
            obs_take_folders,
            runtime_batch,
            batch_start_arc,
            hold,
        );
    });

    Ok(())
}

/// Ends a batch's Render While Capturing (#861) when dropped.
struct RoomGuard(Option<Arc<crate::capture_room_manager::CaptureRoom>>);

impl Drop for RoomGuard {
    fn drop(&mut self) {
        if let Some(room) = &self.0 {
            room.finish();
        }
    }
}

// ── Initial Commands import ──────────────────────────────────────────────────
//
// A manually-`exec`'d movie config never gets a guaranteed exec point: a
// command-line `+exec` fires *before* the user's own config.cfg/movie.cfg
// chain, not after it, so config.cfg silently wins any cvar both set — see
// docs/goldsrc_dod_quirks.md's Command Precedence entry. STUFFTEXT-injected
// Initial Commands are the only mechanism confirmed to always win over
// everything, config.cfg/movie.cfg included, so importing a movie config's
// lines directly into Initial Commands is the reliable substitute.

/// Collapses runs of spaces/tabs outside double-quoted spans into a single
/// space, and trims the ends. A `.cfg` hand-aligned into columns —
/// `r_decals               "0"` — reads fine in a text editor but carries all
/// that padding into an Initial Command row, where it just looks broken.
/// Whitespace *inside* quotes (a path with a space in it, say) is left alone:
/// only alignment padding between tokens is collapsing, not a value's own
/// content.
fn collapse_whitespace_outside_quotes(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_quotes = false;
    let mut last_was_space = false;
    for c in line.chars() {
        if c == '"' {
            in_quotes = !in_quotes;
            out.push(c);
            last_was_space = false;
        } else if !in_quotes && (c == ' ' || c == '\t') {
            if !last_was_space {
                out.push(' ');
            }
            last_was_space = true;
        } else {
            out.push(c);
            last_was_space = false;
        }
    }
    out.trim().to_string()
}

/// Reads a `.cfg` file the user picked through a native dialog (never a typed
/// path — see CLAUDE.md's filesystem-picking rule) and returns its console
/// commands: one per line, blank lines and full-line `//` comments dropped,
/// alignment whitespace outside quotes collapsed. Read-only, same as
/// `cfg_scan.rs` — this never writes the file it reads.
#[tauri::command]
pub async fn read_cfg_commands(path: String) -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(move || {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| crate::messages::failed_to_read_file(&path, e))?;
        Ok(content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with("//"))
            .map(collapse_whitespace_outside_quotes)
            .collect())
    })
    .await
    .map_err(crate::messages::background_task_crashed)?
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "capture_manager_tests.rs"]
mod tests;
