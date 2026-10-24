use crate::capture_manager::CustomCommandPayload;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// A named set of Initial and Scheduled Commands (#442). Same shape as
/// `AppSettings`' two lists; the frontend (`command_profiles.js`) trims them
/// and drops blank rows before saving.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandProfile {
    pub name: String,
    #[serde(default)]
    pub init_commands: Vec<String>,
    #[serde(default)]
    pub custom_commands: Vec<CustomCommandPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub hlae_path: String,
    pub hl_path: String,
    /// The game folder beside `hl.exe`, the `<mod>` in `-game <mod>` (#402).
    /// `dod` for a file written before it existed. Mirrored into
    /// `native::shared::paths::set_game_folder` on load and on every save,
    /// which is where every path and the launch line read it from.
    #[serde(default = "default_game_folder")]
    pub game_folder: String,
    pub ffmpeg_path: Option<String>,
    /// Override for `dodstudio_goldsrc_hooks.dll` (see `native::patch::PatcherConfig`'s
    /// field of the same name). Blank/absent means "use the bundled default
    /// beside this app's own install" -- see
    /// `native::patch::default_goldsrc_hooks_dll_path`.
    ///
    /// The key keeps its original spelling although the DLL was renamed to
    /// `dodstudio_goldsrc_hooks.dll`: it is persisted in the user's settings file
    /// and never shown, so renaming it would silently discard an existing
    /// override (the one a dev build relies on) to change a string nobody
    /// reads.
    #[serde(default)]
    pub goldsrc_hooks_dll_path: Option<String>,
    /// The hook DLL's in-game Demos tab reads this and `demo_folder_history`
    /// out of settings.json for its Go to list (#779): keep the key names.
    pub pinned_folders: Vec<String>,
    /// Analyzer sidebar's "Recent" quick-links tier — most-recent-first,
    /// capped at 10, pushed whenever a folder selection yields a non-empty
    /// demo listing, and by + Add Folder's scans (#533). Mirrors dev's
    /// `settings.demo_folder_history`.
    #[serde(default)]
    pub demo_folder_history: Vec<String>,
    /// File → Open Recent (#935): project files opened or saved, newest
    /// first, at most 10 (`recent_projects.js` keeps the list).
    #[serde(default)]
    pub recent_projects: Vec<String>,
    /// Where a `.dem` opened with DoD Studio from Explorer goes (#528):
    /// `analyzer` (the Demo Analyzer) or `queue` (Studio's queue).
    #[serde(default = "default_dem_open_target")]
    pub dem_open_target: String,
    /// `.json` projects whose "switch to .dodstudio?" offer was answered
    /// "Keep as .json" (#528, `project_convert.js`): never offered again.
    #[serde(default)]
    pub kept_json_projects: Vec<String>,
    /// Gates the Demo Analyzer Explorer tree's per-subfolder "(N)" demo-count
    /// badge (Quick Links counts are always shown, matching dev — see
    /// docs/archive/tauri_parity_audit.md Area 1). Defaults to `false`, matching
    /// dev's own `AppSettings::default()`.
    #[serde(default)]
    pub scan_folders_for_demos: bool,
    /// Demo Analyzer Explorer sidebar's drag-to-resize width, in pixels.
    #[serde(default = "default_analyzer_explorer_width")]
    pub analyzer_explorer_width: i32,
    pub language: String,
    #[serde(default = "default_capture_fps")]
    pub capture_fps: i32,
    /// OBS mode's own capture rate — see `PatcherConfig::obs_capture_fps`'s
    /// doc comment for why this is a separate field rather than sharing
    /// `capture_fps` with frame-sequence/direct-to-video.
    #[serde(default = "default_obs_capture_fps")]
    pub obs_capture_fps: i32,
    /// AGR mode's own rate — see `PatcherConfig::agr_fps`. 0 means "the same
    /// as `capture_fps`", which is also what a file written before it existed
    /// gets.
    #[serde(default)]
    pub agr_fps: i32,
    pub pre_roll_seconds: f32,
    pub post_roll_seconds: f32,
    #[serde(default = "default_resolution_width")]
    pub resolution_width: i32,
    #[serde(default = "default_resolution_height")]
    pub resolution_height: i32,
    /// Whether the pipeline sweeps the decal ring between clips.
    ///
    /// Distinct from `r_decals` in `init_commands`, which says how many decals
    /// the engine keeps. Off here means "capture as the engine would, bullet
    /// holes and all" — which `r_decals 0` cannot express, since that turns
    /// decals off entirely and changes how the capture looks.
    ///
    /// Defaults on, including for settings files written before it existed.
    #[serde(default = "default_decal_flush")]
    pub decal_flush: bool,
    /// Whether HLAE pipes frames to FFmpeg instead of writing a BMP sequence.
    #[serde(default)]
    pub ffmpeg_capture: bool,
    /// Codec id for direct-to-video capture. Stored as the string id rather
    /// than the enum so a settings file naming a codec this build does not
    /// know still loads — `CaptureCodec::from_str_id` falls back to the
    /// default instead of failing the whole file.
    #[serde(default = "default_capture_codec")]
    pub ffmpeg_capture_codec: String,
    /// How this batch records: `frame_sequence`, `direct_to_video`, `obs` or
    /// `agr`.
    ///
    /// Stored as the string id for the same reason the codec is — a settings
    /// file naming a mode this build does not know loads and degrades to the
    /// path that always works, rather than failing the whole file.
    ///
    /// A file written before this existed has no value here and only
    /// `ffmpeg_capture`; `PatcherConfig::normalise_capture_mode` promotes that,
    /// so an upgrade keeps whatever the user had selected.
    #[serde(default = "default_capture_mode")]
    pub capture_mode: String,
    /// obs-websocket host, when `capture_mode` is `obs`.
    #[serde(default = "default_obs_host")]
    pub obs_host: String,
    #[serde(default = "default_obs_port")]
    pub obs_port: u16,
    /// The user's obs-websocket password. Empty when OBS has authentication
    /// switched off. Never logged — see `ObsConfig::redacted`.
    #[serde(default)]
    pub obs_password: String,
    /// Path to `obs64.exe`/`obs.exe`, for the "Launch OBS" button. Spawn-and-
    /// forget, same as `launch_standalone_game` — OBS is the user's own
    /// software, not ours to manage, so nothing here tracks its lifecycle.
    #[serde(default)]
    pub obs_exe_path: String,
    #[serde(default)]
    pub auto_clear_logs: bool,
    #[serde(default)]
    pub auto_clear_previews: bool,
    /// On by default: keeping the patched demos is Save Local Patched Copy's
    /// job, so these are only leftovers.
    #[serde(default = "default_auto_clear_temp_demos")]
    pub auto_clear_temp_demos: bool,
    /// Recorded seconds before the first kill and after the last one. 0 gave
    /// a clip that starts on the kill shot.
    #[serde(default = "default_record_start_lead")]
    pub record_start_lead: f32,
    #[serde(default = "default_record_stop_trail")]
    pub record_stop_trail: f32,
    #[serde(default = "default_initial_delay")]
    pub initial_delay: f32,
    #[serde(default = "default_fast_forward_speed")]
    pub fast_forward_speed: f32,
    /// Capture Output pool — required, no fallback (see capture_pane.js's
    /// refreshLaunchGuard/buildCapturePayload). Doubles as Render Studio's
    /// scan-input locations (see render_pane.js's initRenderUI call) — the
    /// two used to be separately-configured lists (`render_folders`) until
    /// that redundancy was found to leave any capture written to a drive
    /// only in this list invisible to Render Studio's scan.
    #[serde(default)]
    pub target_drives: Vec<String>,
    #[serde(default)]
    pub init_commands: Vec<String>,
    #[serde(default)]
    pub custom_commands: Vec<CustomCommandPayload>,
    /// Named sets of both command lists (#442), applied from Configuration >
    /// Commands. Applying one only fills the two lists above, so the commands
    /// are checked exactly like typed ones.
    #[serde(default)]
    pub command_profiles: Vec<CommandProfile>,
    /// The profile the lists were last applied from or saved to, so the
    /// "(edited)" mark survives a restart. Empty when there is none.
    #[serde(default)]
    pub command_profile_active: String,
    /// The user's changed copies of a built-in profile that were renamed at
    /// load (`builtin_profiles::separate_builtin_copies`), e.g. "Clean clips
    /// (mine)", until the Commands tab has said so once (#911). The frontend
    /// empties it when the notice is closed.
    #[serde(default)]
    pub builtin_profile_notice: Vec<String>,
    #[serde(default)]
    pub save_local_patched_copy: bool,
    /// `PatcherConfig::hook_runs_batch` (#434): Seek Between Clips, the
    /// default route. The setting stays as the switch back to patching.
    #[serde(default = "native::patch::types::default_hook_runs_batch")]
    pub hook_runs_batch: bool,
    /// Which one-time changes to saved values have run (`MIGRATIONS`), so a
    /// new default can reach a settings file that saved the old one.
    #[serde(default)]
    pub settings_migrations: u32,
    /// `PatcherConfig::keep_game_behind`.
    #[serde(default = "default_keep_game_behind")]
    pub keep_game_behind: bool,
    #[serde(default = "default_render_codec")]
    pub render_codec: String,
    /// Raw FFmpeg args, only meaningful when `render_codec == "custom"`.
    #[serde(default)]
    pub render_custom_codec_args: String,
    #[serde(default = "default_render_fps")]
    pub render_fps: i32,
    #[serde(default = "default_render_max_concurrent")]
    pub render_max_concurrent: i32,
    /// Named render setups (#108), applied from Configuration > Render
    /// Output. Each carries a codec, its custom args, source FPS and
    /// concurrency; the frontend owns the shape (`render_presets.js`).
    #[serde(default)]
    pub render_presets: Vec<serde_json::Value>,
    /// How many demos a scan parses at once (#246). Each worker holds a
    /// whole analysis, roughly 1.2 GB at peak; see `SCAN_CONCURRENCY`.
    #[serde(default = "default_scan_workers")]
    pub scan_workers: i32,
    /// JIT multi-drive export pool for Render Studio.
    #[serde(default)]
    pub render_export_dirs: Vec<String>,
    /// OS toast notification toggles (issue #98) — each stage independently
    /// switchable, not one global mute. The three that fire once per demo or
    /// per clip default off: a 40-clip batch was 40+ toasts.
    #[serde(default = "default_notify_patching")]
    pub notify_patching: bool,
    #[serde(default = "default_notify_demo_loading")]
    pub notify_demo_loading: bool,
    #[serde(default = "default_notify_between_clips")]
    pub notify_between_clips: bool,
    #[serde(default = "default_notify_captures_done")]
    pub notify_captures_done: bool,
    #[serde(default = "default_notify_renders_done")]
    pub notify_renders_done: bool,
    #[serde(default = "default_notify_error")]
    pub notify_error: bool,
    #[serde(default = "default_notify_updates")]
    pub notify_updates: bool,
    /// Which release channel `check_for_update` polls: `"stable"` (built from
    /// `main`) or `"experimental"` (built from `dev`, on-demand). See issue #133.
    #[serde(default = "default_update_channel")]
    pub update_channel: String,
    #[serde(default = "default_auto_check_updates")]
    pub auto_check_updates: bool,
    /// "When a batch finishes" (#440): `true` queues and starts a Render
    /// Studio job for every take the batch verified, so each clip ends up as
    /// one video with audio in the export folder without a Render tab visit.
    /// Off by default, and for every settings file written before it existed,
    /// so nobody's machine starts encoding after a batch without being asked.
    #[serde(default)]
    pub finish_clips_after_batch: bool,
    /// The codec each capture mode's clips are finished with, as a
    /// `RenderCodec` string id, plus `"render_tab"` for "whatever Render
    /// Settings' own codec is". Strings for the same reason `render_codec` is
    /// one: a value this build does not know still loads.
    #[serde(default = "default_finish_codec_obs")]
    pub finish_codec_obs: String,
    #[serde(default = "default_finish_codec_render_tab")]
    pub finish_codec_video: String,
    #[serde(default = "default_finish_codec_render_tab")]
    pub finish_codec_frames: String,
    /// "After a clip is finished" (#644): `"keep"`, `"recycle"` (the Recycle
    /// Bin) or `"delete"` (for good). A string so a value this build does not
    /// know still loads, and reads as `"keep"`
    /// (`native::hlcr::take_cleanup::AfterFinish::from_setting`). Keep by
    /// default, and for every settings file written before it existed.
    #[serde(default = "default_take_after_finish")]
    pub take_after_finish: String,
    /// "Only when space is needed" (#644), read only with `"delete"`: takes
    /// stay until a capture or finish job is short of room.
    #[serde(default)]
    pub take_delete_only_when_space_needed: bool,
    /// "Render while capturing" (#861): each take renders as soon as the
    /// batch has recorded it, one render at a time, and a batch short of room
    /// holds between clips until renders free enough. Off by default: how
    /// much a render slows the capture depends on the machine.
    #[serde(default)]
    pub render_while_capturing: bool,
    /// How a highlight's clip name is built (#441), in the placeholder syntax
    /// of `studio/src/clip_name.js`. Configuration > Render Output.
    #[serde(default = "default_clip_name_template")]
    pub clip_name_template: String,
    /// The Demo Auditor's rename templates (#469), one per demo type, in the
    /// same syntax with `studio/src/demo_rename.js`'s placeholders.
    #[serde(default = "default_demo_rename_pov_template")]
    pub demo_rename_pov_template: String,
    #[serde(default = "default_demo_rename_hltv_template")]
    pub demo_rename_hltv_template: String,
    /// Lower-cases every new demo name, so a template needs no `:lower`
    /// on each placeholder.
    #[serde(default)]
    pub demo_rename_lowercase: bool,
    /// Demo Auditor > Duplicates' "Show demos backed up in archives" (#929,
    /// .zip, .7z and .rar): off by default, since a first scan of big
    /// archives decompresses the demos in them that a loose demo shares a
    /// size with.
    #[serde(default)]
    pub audit_look_in_zips: bool,
    /// "Skip setup for now" in the Setup window (#599): it stops opening by
    /// itself at launch. Finishing setup clears it, so a required path that
    /// goes missing later opens the window again.
    #[serde(default)]
    pub setup_dismissed: bool,
    /// The one-time look at the pinned folders ran (#533): older builds
    /// pinned every folder + Add Folder scanned, and Studio lists them once
    /// so the user can unpin the ones they never chose.
    #[serde(default)]
    pub pinned_folders_reviewed: bool,
    /// "Include subfolders" beside + Add Folder (#531): a folder scan also
    /// reads its subfolders' demos. On by default, today's behaviour.
    #[serde(default = "default_scan_include_subfolders")]
    pub scan_include_subfolders: bool,
    /// Configuration > Players (#428): the spray and crosshair each player
    /// gets in a demo, what everyone else gets, and whether the game uses it.
    /// Mirrored for the hook DLL by `players_tab::mirror_table` on load and
    /// on every save.
    #[serde(default)]
    pub players: native::player_overrides::PlayerTable,
    /// Configuration > Paths' install list (#931): the folders of installs
    /// "Hide from this list" took off it. Only the list reads it.
    #[serde(default)]
    pub hidden_installs: Vec<String>,
    /// The Capture page's Review controls (#623, #661, #665), kept across
    /// restarts (`studio/src/review_settings.js`): which Review marks a
    /// review plays, the order, whether Yes also ticks the highlight for
    /// capture, and the fast-forward over long gaps.
    #[serde(default = "default_review_covers")]
    pub review_covers: String,
    #[serde(default = "default_review_order")]
    pub review_order: String,
    #[serde(default = "default_true")]
    pub review_tick_yes: bool,
    #[serde(default = "default_true")]
    pub review_fast_forward: bool,
    #[serde(default = "default_review_fast_forward_gap")]
    pub review_fast_forward_gap: i32,
    /// This PC's full-read rate in bytes per second, learned from its own
    /// scans (`studio/src/rescan_estimate.js`, #937); 0 until a scan has read
    /// a demo in full, and the rescan prompt then assumes 3 s per demo.
    #[serde(default)]
    pub scan_read_bytes_per_sec: f64,
    /// The folder each Browse button last picked from, keyed by the field's
    /// id in `studio/src/pick_path.js` (#936), so an empty field's dialog
    /// starts there instead of wherever any other picker last was.
    #[serde(default)]
    pub pick_folders: BTreeMap<String, String>,
    /// The update Rescan Recent Sessions' start-up offer last ran for
    /// (`studio/src/rescan_recent.js`'s `offerStamp`, #938): the app version
    /// plus the highlight detection version. Empty until it has run once.
    #[serde(default)]
    pub rescan_offered_for: String,
}

fn default_true() -> bool {
    true
}
fn default_review_covers() -> String {
    "all".to_string()
}
fn default_review_order() -> String {
    "time".to_string()
}
/// Keep in step with `index.html`'s `#review-fast-forward-gap`.
fn default_review_fast_forward_gap() -> i32 {
    8
}

fn default_game_folder() -> String {
    native::shared::paths::DEFAULT_GAME_FOLDER.to_string()
}
/// 1920x1080 when the primary screen holds it, else 1280x720
/// (`native::sys::screen`). Only a new install or a file missing the field
/// gets it; a saved size is never changed. Read once, so the two halves agree.
fn default_resolution() -> (i32, i32) {
    static SIZE: std::sync::OnceLock<(i32, i32)> = std::sync::OnceLock::new();
    *SIZE.get_or_init(|| {
        native::sys::screen::default_capture_resolution(native::sys::screen::primary_screen_size())
    })
}
fn default_resolution_width() -> i32 {
    default_resolution().0
}
fn default_obs_capture_fps() -> i32 {
    120
}
fn default_resolution_height() -> i32 {
    default_resolution().1
}
fn default_initial_delay() -> f32 {
    3.0
}
fn default_fast_forward_speed() -> f32 {
    0.05
}
fn default_render_codec() -> String {
    "prores".to_string()
}
fn default_render_fps() -> i32 {
    300
}
fn default_render_max_concurrent() -> i32 {
    2
}
fn default_scan_workers() -> i32 {
    crate::capture_manager::SCAN_CONCURRENCY as i32
}
fn default_analyzer_explorer_width() -> i32 {
    260
}
fn default_decal_flush() -> bool {
    true
}
fn default_capture_codec() -> String {
    native::patch::CaptureCodec::default()
        .to_str_id()
        .to_string()
}
fn default_capture_mode() -> String {
    native::patch::CaptureMode::default()
        .to_str_id()
        .to_string()
}
/// 120 rather than 300: 1280x720 frames at 300 fps are ~50 GB per minute of
/// clip and more than a SATA SSD writes.
fn default_capture_fps() -> i32 {
    120
}
fn default_auto_clear_temp_demos() -> bool {
    true
}
fn default_record_start_lead() -> f32 {
    5.0
}
fn default_record_stop_trail() -> f32 {
    5.0
}
fn default_obs_host() -> String {
    "127.0.0.1".to_string()
}
fn default_obs_port() -> u16 {
    4455
}
fn default_notify_patching() -> bool {
    false
}
fn default_notify_demo_loading() -> bool {
    false
}
fn default_notify_between_clips() -> bool {
    false
}
fn default_notify_captures_done() -> bool {
    true
}
fn default_notify_renders_done() -> bool {
    true
}
fn default_notify_error() -> bool {
    true
}
fn default_notify_updates() -> bool {
    true
}
fn default_update_channel() -> String {
    "stable".to_string()
}
/// Keep in step with `DEFAULT_TEMPLATE` in `studio/src/clip_name.js`.
fn default_clip_name_template() -> String {
    "{map}_{name}_{kills}k_{weapons}_{time}".to_string()
}
/// Keep in step with `DEFAULT_POV_TEMPLATE` in `studio/src/demo_rename.js`.
fn default_demo_rename_pov_template() -> String {
    "{name}_{kills}k_v_{opponent}_{map}".to_string()
}
/// Keep in step with `DEFAULT_HLTV_TEMPLATE` in `studio/src/demo_rename.js`.
fn default_demo_rename_hltv_template() -> String {
    "{allies}_v_{axis}_{map}_{date}".to_string()
}
fn default_auto_check_updates() -> bool {
    true
}
fn default_scan_include_subfolders() -> bool {
    true
}
pub(crate) fn default_keep_game_behind() -> bool {
    true
}
/// An OBS take already is a finished file with its audio in it, so the
/// default finish is a plain copy (`RenderCodec::SourceCopy`).
fn default_finish_codec_obs() -> String {
    "source_copy".to_string()
}
fn default_finish_codec_render_tab() -> String {
    "render_tab".to_string()
}
fn default_take_after_finish() -> String {
    "keep".to_string()
}
fn default_dem_open_target() -> String {
    "analyzer".to_string()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            hlae_path: String::new(),
            hl_path: String::new(),
            game_folder: default_game_folder(),
            ffmpeg_path: None,
            goldsrc_hooks_dll_path: None,
            pinned_folders: Vec::new(),
            demo_folder_history: Vec::new(),
            recent_projects: Vec::new(),
            dem_open_target: default_dem_open_target(),
            kept_json_projects: Vec::new(),
            scan_folders_for_demos: false,
            analyzer_explorer_width: default_analyzer_explorer_width(),
            language: "en".to_string(),
            capture_fps: default_capture_fps(),
            obs_capture_fps: default_obs_capture_fps(),
            agr_fps: 0,
            pre_roll_seconds: 5.0,
            post_roll_seconds: 1.0,
            resolution_width: default_resolution_width(),
            resolution_height: default_resolution_height(),
            decal_flush: default_decal_flush(),
            ffmpeg_capture: false,
            ffmpeg_capture_codec: default_capture_codec(),
            capture_mode: default_capture_mode(),
            obs_host: default_obs_host(),
            obs_port: default_obs_port(),
            obs_password: String::new(),
            obs_exe_path: String::new(),
            auto_clear_logs: false,
            auto_clear_previews: false,
            auto_clear_temp_demos: default_auto_clear_temp_demos(),
            record_start_lead: default_record_start_lead(),
            record_stop_trail: default_record_stop_trail(),
            initial_delay: default_initial_delay(),
            fast_forward_speed: default_fast_forward_speed(),
            target_drives: Vec::new(),
            // Seeded on first run only. `Default` is reached when there is no
            // settings file at all, so nobody's saved init commands are ever
            // appended to — silently changing the FOV or the decal ring of a
            // capture someone had already configured would be a far worse
            // trade than a new user having to discover these two lines exist.
            //
            // Both are values the pipeline reads back: `r_decals` states the
            // decal ring the flush sizes its sweep to, and `mirv_fov` states
            // the FOV the on-screen test is derived from. 90 is the engine's
            // own default and the right starting point; change it here and the
            // flush follows.
            init_commands: vec!["r_decals 256".to_string(), "mirv_fov 90".to_string()],
            custom_commands: Vec::new(),
            command_profiles: Vec::new(),
            command_profile_active: String::new(),
            builtin_profile_notice: Vec::new(),
            save_local_patched_copy: false,
            hook_runs_batch: native::patch::types::default_hook_runs_batch(),
            settings_migrations: MIGRATIONS,
            keep_game_behind: default_keep_game_behind(),
            render_codec: default_render_codec(),
            render_custom_codec_args: String::new(),
            render_fps: default_render_fps(),
            render_max_concurrent: default_render_max_concurrent(),
            render_presets: Vec::new(),
            scan_workers: default_scan_workers(),
            render_export_dirs: Vec::new(),
            notify_patching: default_notify_patching(),
            notify_demo_loading: default_notify_demo_loading(),
            notify_between_clips: default_notify_between_clips(),
            notify_captures_done: default_notify_captures_done(),
            notify_renders_done: default_notify_renders_done(),
            notify_error: default_notify_error(),
            notify_updates: default_notify_updates(),
            update_channel: default_update_channel(),
            auto_check_updates: default_auto_check_updates(),
            finish_clips_after_batch: false,
            finish_codec_obs: default_finish_codec_obs(),
            finish_codec_video: default_finish_codec_render_tab(),
            finish_codec_frames: default_finish_codec_render_tab(),
            take_after_finish: default_take_after_finish(),
            take_delete_only_when_space_needed: false,
            render_while_capturing: false,
            clip_name_template: default_clip_name_template(),
            demo_rename_pov_template: default_demo_rename_pov_template(),
            demo_rename_hltv_template: default_demo_rename_hltv_template(),
            demo_rename_lowercase: false,
            audit_look_in_zips: false,
            setup_dismissed: false,
            pinned_folders_reviewed: false,
            scan_include_subfolders: default_scan_include_subfolders(),
            players: native::player_overrides::PlayerTable::default(),
            hidden_installs: Vec::new(),
            review_covers: default_review_covers(),
            review_order: default_review_order(),
            review_tick_yes: true,
            review_fast_forward: true,
            review_fast_forward_gap: default_review_fast_forward_gap(),
            scan_read_bytes_per_sec: 0.0,
            pick_folders: BTreeMap::new(),
            rescan_offered_for: String::new(),
        }
    }
}

pub fn settings_path() -> PathBuf {
    // One implementation of "where this app's %APPDATA% folder is", shared with
    // the activity log. It used to be spelled out again here, which is how the
    // two could have disagreed after #257 renamed the folder.
    native::shared::paths::get_appdata_dir().join("settings.json")
}

/// How many one-time changes to saved values exist; `migrate` runs the ones
/// past a file's `settings_migrations`. The frontend saves settings without
/// this field, so `save_settings` keeps the loaded value.
pub const MIGRATIONS: u32 = 2;

impl AppSettings {
    pub fn load_or_default() -> Self {
        let path = settings_path();
        if path.exists()
            && let Ok(content) = fs::read_to_string(&path)
            && let Ok(mut settings) = serde_json::from_str::<AppSettings>(&content)
        {
            settings.migrate();
            if settings.migrate_console_names() {
                // Saved at once, so the file on disk matches what Studio shows.
                let _ = settings.save();
            }
            return settings;
        }
        Self::default()
    }

    /// Moves every hook console name from before #555 (`dodstudio_*`) in
    /// Initial Commands, Scheduled Commands and the command profiles to the
    /// name that replaced it (`native::patch::console_prefix::migrated`, from
    /// the hook's own old -> new table). The game has no
    /// aliases for the old names, so a list left as it was would do nothing.
    /// True when anything changed.
    pub fn migrate_console_names(&mut self) -> bool {
        fn fix(command: &mut String, changed: &mut bool) {
            if let Some(new) = native::patch::console_prefix::migrated(command) {
                *command = new;
                *changed = true;
            }
        }
        let mut changed = false;
        let lists = std::iter::once((&mut self.init_commands, &mut self.custom_commands)).chain(
            self.command_profiles
                .iter_mut()
                .map(|p| (&mut p.init_commands, &mut p.custom_commands)),
        );
        for (init, scheduled) in lists {
            for command in init.iter_mut() {
                fix(command, &mut changed);
            }
            for payload in scheduled.iter_mut() {
                fix(&mut payload.command, &mut changed);
            }
        }
        changed
    }

    /// Applies the one-time changes this file hasn't had.
    pub fn migrate(&mut self) {
        if self.settings_migrations < 1 {
            // Seek Between Clips was off by default while experimental; a
            // file saved then has the old default and gets the new one once
            // (#434). Turning it off again after this sticks.
            self.hook_runs_batch = true;
        }
        // Migration 2 added Clean clips as an editable profile (#898). It is
        // built in now (#911), so 2 has nothing left to do, and the copies it
        // made are sorted out below with every other profile that has a
        // built-in's name. That runs at every load rather than once, so a
        // built-in added later takes its name the same way.
        let kept = crate::builtin_profiles::separate_builtin_copies(
            &mut self.command_profiles,
            &mut self.command_profile_active,
        );
        for name in kept {
            if !self.builtin_profile_notice.contains(&name) {
                self.builtin_profile_notice.push(name);
            }
        }
        self.settings_migrations = MIGRATIONS;
    }

    pub fn save(&self) -> Result<(), String> {
        let path = settings_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(crate::messages::failed_to_serialize_settings)?;
        fs::write(&path, json)
            .map_err(|e| crate::messages::failed_to_write_settings_file(&path, e))?;
        Ok(())
    }
}

pub struct SettingsManager {
    pub inner: Arc<Mutex<AppSettings>>,
}

impl SettingsManager {
    pub fn new() -> Self {
        let settings = AppSettings::load_or_default();
        apply_game_folder(&settings);
        // At start too, so the hook's copy follows a settings.json edited or
        // restored while Studio was closed.
        crate::players_tab::mirror_table(&settings);
        Self {
            inner: Arc::new(Mutex::new(settings)),
        }
    }
}

/// Hands the Game folder setting to `native` (#402). A file someone edited
/// by hand to an unusable name falls back to `dod` and says so, rather than
/// launching the game at a folder Studio cannot name.
pub fn apply_game_folder(settings: &AppSettings) {
    if let Err(e) = native::shared::paths::set_game_folder(&settings.game_folder) {
        native::log_markdown(&format!(
            "⚠️ **Settings** — {e}. Using `{}` until it is changed in Configuration → Paths (#402).",
            native::shared::paths::DEFAULT_GAME_FOLDER
        ));
        let _ = native::shared::paths::set_game_folder("");
    }
}

/// The folders beside `hl.exe` that the Game folder setting can name: those
/// holding a `liblist.gam` (#402). Empty when `game_path` has no folder or
/// it can't be read.
#[tauri::command]
pub async fn list_game_folders(game_path: String) -> Result<Vec<String>, String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        Ok(std::path::Path::new(&game_path)
            .parent()
            .map(native::shared::paths::list_game_folders)
            .unwrap_or_default())
    }))
    .await
}

/// Whether the game folder `folder` beside `hl.exe` lacks the
/// `resource\<folder>_<language>.txt` its in-game text needs, in the language
/// Steam runs in (#925); `None` when it has it, or it is `dod`.
#[tauri::command]
pub async fn game_folder_language_file(
    game_path: String,
    folder: String,
) -> Result<Option<native::shared::paths::MissingLanguageFile>, String> {
    crate::messages::flatten_spawn_blocking(tokio::task::spawn_blocking(move || {
        let language = native::sys::steam::steam_language().unwrap_or_else(|| "english".into());
        Ok(std::path::Path::new(&game_path).parent().and_then(|root| {
            native::shared::paths::missing_language_file(root, &folder, &language)
        }))
    }))
    .await
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// A settings.json saved by a pre-2026-08-18 build still has an
    /// `allocation_strategy` key on disk (the field was removed from
    /// `AppSettings` when the Chronological drive-allocation strategy was
    /// deleted). Since `AppSettings` doesn't set `#[serde(deny_unknown_fields)]`,
    /// serde must silently ignore the stale key rather than fail deserialization
    /// — otherwise every existing user's settings would revert to defaults on
    /// their next launch after upgrading.
    #[test]
    fn test_deserialize_ignores_legacy_allocation_strategy_field() {
        let legacy_json = r#"{
            "hlae_path": "C:/hlae/hlae.exe",
            "hl_path": "C:/dod/hl.exe",
            "ffmpeg_path": null,
            "pinned_folders": [],
            "language": "en",
            "capture_fps": 300,
            "pre_roll_seconds": 2.0,
            "post_roll_seconds": 0.6,
            "allocation_strategy": "Chronological"
        }"#;

        let settings: AppSettings = serde_json::from_str(legacy_json)
            .expect("legacy allocation_strategy field must not break deserialization");

        assert_eq!(settings.hlae_path, "C:/hlae/hlae.exe");
        assert_eq!(settings.capture_fps, 300);
        // Settings saved before #402 drive the `dod` folder, as they always did.
        assert_eq!(settings.game_folder, "dod");
        // Settings saved before #440 do not start finishing clips on their own.
        assert!(!settings.finish_clips_after_batch);
        assert_eq!(settings.finish_codec_obs, "source_copy");
        assert_eq!(settings.finish_codec_video, "render_tab");
        assert_eq!(settings.finish_codec_frames, "render_tab");
        // Settings saved before #644 keep every take.
        assert_eq!(settings.take_after_finish, "keep");
        assert!(!settings.take_delete_only_when_space_needed);
        // Settings saved before #861 don't render while capturing.
        assert!(!settings.render_while_capturing);
        // Settings saved before #246 get today's worker count.
        assert_eq!(
            settings.scan_workers,
            crate::capture_manager::SCAN_CONCURRENCY as i32
        );
        // Settings saved before #599 still open Setup when a path is missing.
        assert!(!settings.setup_dismissed);
        // Settings saved before #533 get the pinned-folder look once.
        assert!(!settings.pinned_folders_reviewed);
        // Settings saved before #531 keep walking into subfolders.
        assert!(settings.scan_include_subfolders);
    }

    #[test]
    fn test_setup_dismissed_roundtrip() {
        let original = AppSettings {
            setup_dismissed: true,
            ..AppSettings::default()
        };
        let json = serde_json::to_string(&original).expect("settings must serialize");
        let restored: AppSettings = serde_json::from_str(&json).expect("settings must deserialize");
        assert!(restored.setup_dismissed);
    }

    /// Open Recent's list survives a save, and a file from before it has none (#935).
    #[test]
    fn recent_projects_roundtrip_and_default() {
        let original = AppSettings {
            recent_projects: vec!["D:/projects/a.json".into(), "D:/projects/b.json".into()],
            ..AppSettings::default()
        };
        let json = serde_json::to_string(&original).expect("settings must serialize");
        let restored: AppSettings = serde_json::from_str(&json).expect("settings must deserialize");
        assert_eq!(restored.recent_projects, original.recent_projects);

        let older: AppSettings = serde_json::from_str(
            r#"{"hlae_path": "", "hl_path": "", "ffmpeg_path": null, "pinned_folders": [],
                "language": "en", "pre_roll_seconds": 2.0, "post_roll_seconds": 0.6}"#,
        )
        .expect("a settings file");
        assert!(older.recent_projects.is_empty());
    }

    /// A `.dem` opened from Explorer goes to the Demo Analyzer unless the
    /// user picked the queue, including for a file from before #528.
    #[test]
    fn dem_open_target_defaults_to_the_analyzer() {
        let older: AppSettings = serde_json::from_str(
            r#"{"hlae_path": "", "hl_path": "", "ffmpeg_path": null, "pinned_folders": [],
                "language": "en", "pre_roll_seconds": 2.0, "post_roll_seconds": 0.6}"#,
        )
        .expect("a settings file");
        assert_eq!(older.dem_open_target, "analyzer");
        assert!(older.kept_json_projects.is_empty());
        let queue = AppSettings {
            dem_open_target: "queue".into(),
            kept_json_projects: vec!["D:/projects/old.json".into()],
            ..AppSettings::default()
        };
        let json = serde_json::to_string(&queue).expect("settings must serialize");
        let restored: AppSettings = serde_json::from_str(&json).expect("settings must deserialize");
        assert_eq!(restored.dem_open_target, "queue");
        assert_eq!(restored.kept_json_projects, ["D:/projects/old.json"]);
    }

    /// A file saved while Seek Between Clips was experimental (off by default)
    /// gets the new default once; a later choice sticks (#434).
    #[test]
    fn seek_between_clips_turns_on_once_for_an_older_settings_file() {
        let file = |extra: &str| -> AppSettings {
            serde_json::from_str(&format!(
                r#"{{"hlae_path": "C:/hlae/hlae.exe", "hl_path": "C:/dod/hl.exe", "ffmpeg_path": null,
                    "pinned_folders": [], "language": "en", "capture_fps": 300,
                    "pre_roll_seconds": 2.0, "post_roll_seconds": 0.6{extra}}}"#
            ))
            .expect("a settings file")
        };
        let mut saved_off = file(r#", "hook_runs_batch": false"#);
        saved_off.migrate();
        assert!(saved_off.hook_runs_batch);
        assert_eq!(saved_off.settings_migrations, MIGRATIONS);

        let mut turned_off_again = file(r#", "hook_runs_batch": false, "settings_migrations": 1"#);
        turned_off_again.migrate();
        assert!(!turned_off_again.hook_runs_batch);

        // A file from before the setting existed has the new default.
        let mut older = file("");
        older.migrate();
        assert!(older.hook_runs_batch);
    }

    /// Hidden installs (#931) round-trip; a file from before them hides none.
    #[test]
    fn hidden_installs_roundtrip() {
        let legacy: AppSettings = serde_json::from_str(
            r#"{"hlae_path": "", "hl_path": "", "ffmpeg_path": null, "pinned_folders": [],
                "language": "en", "capture_fps": 300, "pre_roll_seconds": 2.0,
                "post_roll_seconds": 0.6}"#,
        )
        .expect("a settings file");
        assert!(legacy.hidden_installs.is_empty());
        let original = AppSettings {
            hidden_installs: vec!["D:/Steam/steamapps/common/Half-Life".to_string()],
            ..AppSettings::default()
        };
        let json = serde_json::to_string(&original).expect("settings must serialize");
        let restored: AppSettings = serde_json::from_str(&json).expect("settings must deserialize");
        assert_eq!(restored.hidden_installs, original.hidden_installs);
    }

    #[test]
    fn test_scan_workers_roundtrip() {
        let original = AppSettings {
            scan_workers: 5,
            ..AppSettings::default()
        };
        let json = serde_json::to_string(&original).expect("settings must serialize");
        let restored: AppSettings = serde_json::from_str(&json).expect("settings must deserialize");
        assert_eq!(restored.scan_workers, 5);
    }

    /// Settings saved before #442 load with no profiles; saved ones round-trip.
    #[test]
    fn test_command_profiles_roundtrip() {
        let legacy_json = r#"{
            "hlae_path": "C:/hlae/hlae.exe",
            "hl_path": "C:/dod/hl.exe",
            "ffmpeg_path": null,
            "pinned_folders": [],
            "language": "en",
            "capture_fps": 300,
            "pre_roll_seconds": 2.0,
            "post_roll_seconds": 0.6
        }"#;
        let legacy: AppSettings =
            serde_json::from_str(legacy_json).expect("settings without profiles must deserialize");
        assert!(legacy.command_profiles.is_empty());
        assert_eq!(legacy.command_profile_active, "");

        let original = AppSettings {
            command_profiles: vec![CommandProfile {
                name: "FOTW".to_string(),
                init_commands: vec!["mirv_fov 90".to_string()],
                custom_commands: vec![CustomCommandPayload {
                    command: "host_timescale 0.5".to_string(),
                    relation: "Before".to_string(),
                    offset_seconds: 1.5,
                }],
            }],
            command_profile_active: "FOTW".to_string(),
            ..AppSettings::default()
        };
        let json = serde_json::to_string(&original).expect("settings must serialize");
        let restored: AppSettings = serde_json::from_str(&json).expect("settings must deserialize");
        assert_eq!(restored.command_profile_active, "FOTW");
        assert_eq!(restored.command_profiles.len(), 1);
        assert_eq!(restored.command_profiles[0].init_commands, ["mirv_fov 90"]);
        assert_eq!(
            restored.command_profiles[0].custom_commands[0].offset_seconds,
            1.5
        );
    }

    /// #555: Initial Commands, Scheduled Commands and every profile move from
    /// the old `dodstudio_` hook names to the names that replaced them;
    /// anything else is kept as is.
    #[test]
    fn test_old_console_names_migrate_on_load() {
        let scheduled = |command: &str| CustomCommandPayload {
            command: command.to_string(),
            relation: "Before".to_string(),
            offset_seconds: 1.0,
        };
        let mut settings = AppSettings {
            init_commands: vec![
                "dodstudio_hide_scoreboard 1".to_string(),
                "mirv_fov 90".to_string(),
                "dodstudio_deathmsg block !76561197962734863".to_string(),
            ],
            custom_commands: vec![
                scheduled("dodstudio_spec_lock 1"),
                scheduled("host_timescale 0.5"),
            ],
            command_profiles: vec![CommandProfile {
                name: "Old".to_string(),
                init_commands: vec!["dodstudio_clear_decals".to_string()],
                custom_commands: vec![scheduled("dodstudio_seek_by -10;dodstudio_spec_target 3")],
            }],
            ..AppSettings::default()
        };

        assert!(settings.migrate_console_names());
        assert_eq!(
            settings.init_commands,
            [
                "ds_hud_hide_scoreboard 1",
                "mirv_fov 90",
                "ds_hud_deathmsg block !76561197962734863"
            ]
        );
        assert_eq!(settings.custom_commands[0].command, "ds_spec_lock 1");
        assert_eq!(settings.custom_commands[1].command, "host_timescale 0.5");
        assert_eq!(
            settings.command_profiles[0].init_commands,
            ["ds_map_clear_decals"]
        );
        assert_eq!(
            settings.command_profiles[0].custom_commands[0].command,
            "ds_demo_seek_by -10;ds_spec_target 3"
        );

        // Already moved: nothing to do, so nothing is saved again.
        assert!(!settings.migrate_console_names());
        assert!(!AppSettings::default().migrate_console_names());
    }

    #[test]
    fn test_default_settings_serialize_and_deserialize_roundtrip() {
        let original = AppSettings::default();
        let json = serde_json::to_string(&original).expect("default settings must serialize");
        let restored: AppSettings =
            serde_json::from_str(&json).expect("serialized default settings must deserialize");

        assert_eq!(restored.language, original.language);
        assert_eq!(restored.capture_fps, original.capture_fps);
        assert_eq!(restored.fast_forward_speed, original.fast_forward_speed);
        assert_eq!(restored.render_codec, original.render_codec);
        assert_eq!(
            restored.render_custom_codec_args,
            original.render_custom_codec_args
        );
    }

    /// The 0.2.0 fresh-install defaults, which a file missing the field gets
    /// too; a value someone saved is kept.
    #[test]
    fn fresh_install_defaults_and_missing_fields_match() {
        let fresh = AppSettings::default();
        let minimal: AppSettings = serde_json::from_str(
            r#"{"hlae_path": "", "hl_path": "", "ffmpeg_path": null, "pinned_folders": [],
                "language": "en", "pre_roll_seconds": 2.0, "post_roll_seconds": 0.6}"#,
        )
        .expect("a settings file missing the changed fields");
        let screen = native::sys::screen::default_capture_resolution(
            native::sys::screen::primary_screen_size(),
        );
        for s in [&fresh, &minimal] {
            assert_eq!((s.resolution_width, s.resolution_height), screen);
            assert_eq!(s.capture_fps, 120);
            assert_eq!(s.capture_mode, "frame_sequence");
            assert_eq!(s.record_start_lead, 5.0);
            assert_eq!(s.record_stop_trail, 5.0);
            assert!(s.auto_clear_temp_demos);
            assert!(!s.notify_patching);
            assert!(!s.notify_demo_loading);
            assert!(!s.notify_between_clips);
            assert!(s.notify_captures_done && s.notify_renders_done && s.notify_error);
            assert!(s.notify_updates);
            assert_eq!(s.review_covers, "all");
            assert_eq!(s.review_order, "time");
            assert!(s.review_tick_yes);
            assert!(s.review_fast_forward);
            assert_eq!(s.review_fast_forward_gap, 8);
            assert!(s.pick_folders.is_empty());
        }

        let saved: AppSettings = serde_json::from_str(
            r#"{"hlae_path": "", "hl_path": "", "ffmpeg_path": null, "pinned_folders": [],
                "language": "en", "capture_fps": 300, "capture_mode": "frame_sequence",
                "resolution_width": 2560, "resolution_height": 1440,
                "pre_roll_seconds": 2.0, "post_roll_seconds": 0.6,
                "record_start_lead": 0.0, "record_stop_trail": 0.0,
                "auto_clear_temp_demos": false, "notify_patching": true,
                "notify_demo_loading": true, "notify_between_clips": true,
                "review_tick_yes": false, "review_fast_forward_gap": 20}"#,
        )
        .expect("a settings file with the old values saved");
        assert_eq!(saved.capture_fps, 300);
        assert_eq!(
            (saved.resolution_width, saved.resolution_height),
            (2560, 1440)
        );
        assert_eq!(saved.capture_mode, "frame_sequence");
        assert_eq!(saved.record_start_lead, 0.0);
        assert_eq!(saved.record_stop_trail, 0.0);
        assert!(!saved.auto_clear_temp_demos);
        assert!(saved.notify_patching && saved.notify_demo_loading && saved.notify_between_clips);
        assert!(!saved.review_tick_yes);
        assert_eq!(saved.review_fast_forward_gap, 20);
        // Saved before #937: no read rate measured yet.
        assert_eq!(saved.scan_read_bytes_per_sec, 0.0);
    }

    #[test]
    fn the_scan_read_rate_roundtrips() {
        let original = AppSettings {
            scan_read_bytes_per_sec: 140e6,
            ..AppSettings::default()
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.scan_read_bytes_per_sec, 140e6);
    }

    #[test]
    fn the_rescan_offer_stamp_roundtrips_and_an_older_file_has_none() {
        let original = AppSettings {
            rescan_offered_for: "0.2.1+h1".into(),
            ..AppSettings::default()
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.rescan_offered_for, "0.2.1+h1");
        let older: AppSettings = serde_json::from_str(
            r#"{"hlae_path": "", "hl_path": "", "ffmpeg_path": null, "pinned_folders": [],
                "language": "en", "pre_roll_seconds": 2.0, "post_roll_seconds": 0.6}"#,
        )
        .expect("a settings file");
        assert_eq!(older.rescan_offered_for, "");
    }

    #[test]
    fn a_fresh_install_saves_no_profiles() {
        // Clean clips is built in (#911): the Commands tab lists it from
        // `builtin_profiles`, never from the settings file.
        let fresh = AppSettings::default();
        assert!(fresh.command_profiles.is_empty());
        let mut again = fresh.clone();
        again.migrate();
        assert!(again.command_profiles.is_empty());
        assert!(again.builtin_profile_notice.is_empty());
    }

    /// #911: settings migration 2 copied Clean clips into the file as an
    /// editable profile. An unchanged copy goes (the built-in one replaces
    /// it); a changed one stays as "Clean clips (mine)", with the notice.
    #[test]
    fn a_clean_clips_copy_from_migration_2_is_dropped_or_kept_as_mine() {
        let file = |extra: &str| -> AppSettings {
            serde_json::from_str(&format!(
                r#"{{"hlae_path": "", "hl_path": "", "ffmpeg_path": null, "pinned_folders": [],
                    "language": "en", "pre_roll_seconds": 2.0, "post_roll_seconds": 0.6{extra}}}"#
            ))
            .expect("a settings file")
        };
        // Clean clips as #898 first added it (dbfcaa64), before 0.2.0 changed it.
        let shipped = r#"["r_decals 256", "mirv_fov 90", "ds_spec_match_pov 1",
            "ds_spec_hide_hltv_messages 1", "ds_spec_hide_bars 1", "ds_spec_deathcam 0",
            "ds_hud_hide_scoreboard 1", "ds_hud_hide_voice_menus 1", "ds_hud_hide_crosshair 0",
            "ds_hud_hide_map_text 0", "ds_hud_hide_clan_text 1", "ds_hud_show_scope 1",
            "ds_hud_overview_players 0", "ds_hud_overview_team_marker 0", "ds_view_use_cl_bob 0",
            "ds_view_hide_headshot_red_screen 0", "ds_view_hide_screen_shake 0",
            "ds_player_hide_hand_signals 0", "ds_sound_mute_voice_commands 0",
            "ds_map_allow_shaders 0"]"#;

        // Unchanged: dropped, silently, and the active name stays on it.
        let mut unchanged = file(&format!(
            r#", "settings_migrations": 2, "command_profile_active": "Clean clips",
               "command_profiles": [{{"name": "FOTW", "init_commands": ["mirv_fov 100"]}},
                                    {{"name": "Clean clips", "init_commands": {shipped}}}]"#
        ));
        unchanged.migrate();
        let names: Vec<_> = unchanged
            .command_profiles
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names, ["FOTW"]);
        assert_eq!(unchanged.command_profile_active, "Clean clips");
        assert!(unchanged.builtin_profile_notice.is_empty());

        // Changed (one line added): kept under its new name, which the
        // active name follows, and the notice is due.
        let changed_lines = shipped.replacen('[', r#"["host_framerate 60", "#, 1);
        let mut changed = file(&format!(
            r#", "settings_migrations": 2, "command_profile_active": "Clean clips",
               "command_profiles": [{{"name": "Clean clips", "init_commands": {changed_lines}}}]"#
        ));
        changed.migrate();
        assert_eq!(changed.command_profiles.len(), 1);
        assert_eq!(changed.command_profiles[0].name, "Clean clips (mine)");
        assert_eq!(
            changed.command_profiles[0].init_commands[0],
            "host_framerate 60"
        );
        assert_eq!(changed.command_profile_active, "Clean clips (mine)");
        assert_eq!(changed.builtin_profile_notice, ["Clean clips (mine)"]);

        // The notice is not added twice, and survives a save until the
        // frontend empties it.
        changed.migrate();
        assert_eq!(changed.builtin_profile_notice, ["Clean clips (mine)"]);
        let json = serde_json::to_string(&changed).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.builtin_profile_notice, ["Clean clips (mine)"]);

        // From before migration 2: nothing is added any more.
        let mut older = file(
            r#", "settings_migrations": 1,
               "command_profiles": [{"name": "FOTW", "init_commands": ["mirv_fov 100"]}]"#,
        );
        older.migrate();
        assert_eq!(older.command_profiles.len(), 1);
        assert_eq!(older.settings_migrations, MIGRATIONS);
    }
}
