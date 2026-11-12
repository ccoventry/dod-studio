//! The full settings a take was captured with, and a hash over them (part of
//! #1085, the UI facelift's "History with full settings").
//!
//! `take_meta` already records the rate a take was captured at, because the
//! render needs it (#14). This is everything else a capture used that changes
//! what the take shows: the picture size and field of view, the install and
//! the hook DLL, the Initial and Scheduled Commands, the clip length around
//! the kills and the kill-gap setting. It is built once per batch from the
//! batch's own `PatcherConfig`, so it says what the capture used, not what
//! the settings say now, and goes beside every take of the batch
//! (`dodstudio_take.json`) and into the project's take record.
//!
//! **The hash** lets a later step say "these two takes were captured the same
//! way" without comparing every field: one string over the sorted Initial
//! Commands, the sorted Scheduled Commands and the other settings. Sorted,
//! because two lists holding the same lines in another order are the same
//! settings to the user. It starts with [`HASH_VERSION`], so a hash made by
//! another scheme reads as "can't tell", never as "different". A take with
//! no settings recorded (every take captured before this) has no hash and is
//! never compared.

use crate::patch::types::{KillGapMode, KillGaps};
use crate::patch::{CaptureMode, CommandRelation, CustomCommand, PatcherConfig};
use std::path::Path;

/// The scheme of [`CaptureSettings::hash`]. Raise it when the text hashed
/// changes, so hashes from the old scheme never match the new ones.
pub const HASH_VERSION: u32 = 1;

/// What [`CaptureSettings::hook`] says when the launch used the DLL that
/// ships with Studio, not an override.
pub const HOOK_BUNDLED: &str = "bundled";

/// OBS mode's rate when the setting is unset, as the frontend counts it
/// (`take_record.js` `captureRateOf`).
const OBS_DEFAULT_FPS: i32 = 60;

/// One Scheduled Command as a take records it.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct ScheduledCommand {
    pub command: String,
    /// Seconds from the clip's start (`Before`) or end (`After`).
    pub offset_seconds: f32,
    pub relation: CommandRelation,
}

impl From<&CustomCommand> for ScheduledCommand {
    fn from(c: &CustomCommand) -> Self {
        Self {
            command: c.command.trim().to_string(),
            offset_seconds: c.offset,
            relation: c.relation,
        }
    }
}

/// Everything a capture used that changes what its take shows. Every field
/// defaults, so a file from a build that recorded fewer still reads.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default)]
pub struct CaptureSettings {
    /// `v<HASH_VERSION>-<16 hex digits>`, over everything below but
    /// `app_version` ([`CaptureSettings::hash_of`]).
    pub hash: String,
    /// `CaptureMode::to_str_id`.
    pub mode: String,
    /// The rate the take was recorded at: OBS's own rate in OBS mode, the
    /// AGR rate in AGR mode, else `mirv_movie_fps`.
    pub fps: i32,
    /// Direct-to-video's codec id; empty for the other modes.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub codec: String,
    pub width: i32,
    pub height: i32,
    /// The horizontal field of view the capture ran at: the project's
    /// (Picture), else `mirv_fov` from Initial Commands, else the game's
    /// configs, else the default (`capture_fov_resolved`).
    pub fov: f32,
    /// The install's folder name (`hl.exe`'s folder), not its path, so the
    /// record means the same on another PC.
    pub install: String,
    /// [`HOOK_BUNDLED`], an override DLL as `<its folder>/<file name>`, or
    /// empty when no hook DLL was found.
    pub hook: String,
    /// Initial Commands, in the order they ran.
    pub lines: Vec<String>,
    /// Scheduled Commands, in the order they were listed.
    pub commands: Vec<ScheduledCommand>,
    /// Seconds recorded before the first kill and after the last.
    pub before: f32,
    pub after: f32,
    pub kill_gaps: KillGaps,
    pub decal_flush: bool,
    /// The Studio build that captured it. Not in the hash: a new release
    /// with the same settings captures the same take.
    pub app_version: String,
}

impl CaptureSettings {
    /// The settings `config` captures with, hash filled in. Reads the game's
    /// configs (cached) when Initial Commands don't state the field of view.
    pub fn from_config(config: &PatcherConfig, app_version: &str) -> Self {
        let mut settings = Self {
            hash: String::new(),
            mode: config.capture_mode.to_str_id().to_string(),
            fps: recording_fps(config),
            codec: if config.capture_mode == CaptureMode::DirectToVideo {
                config.ffmpeg_capture_codec.to_str_id().to_string()
            } else {
                String::new()
            },
            width: config.resolution_width,
            height: config.resolution_height,
            fov: crate::patch::capture_fov_resolved(config),
            install: install_name(&config.game_path),
            hook: hook_label(config),
            // As sent: with the project's field of view after the user's own.
            lines: crate::patch::user_init_commands(config)
                .iter()
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
                .collect(),
            commands: config
                .custom_commands
                .iter()
                .map(ScheduledCommand::from)
                .collect(),
            before: config.pre_roll_seconds,
            after: config.post_roll_seconds,
            kill_gaps: config.kill_gaps,
            decal_flush: config.decal_flush,
            app_version: app_version.to_string(),
        };
        settings.hash = settings.hash_of();
        settings
    }

    /// The hash these settings have: FNV-1a 64 over [`Self::hashed_text`].
    /// Pure and stable across builds and platforms (no `DefaultHasher`).
    pub fn hash_of(&self) -> String {
        format!(
            "v{HASH_VERSION}-{:016x}",
            fnv1a64(self.hashed_text().as_bytes())
        )
    }

    /// The text the hash is taken over, one `name=value` per line, lines and
    /// commands sorted. Kill gaps kept hash the same whatever their unused
    /// length and speed say.
    fn hashed_text(&self) -> String {
        let mut lines: Vec<&str> = self.lines.iter().map(String::as_str).collect();
        lines.sort_unstable();
        let mut commands: Vec<String> = self
            .commands
            .iter()
            .map(|c| format!("{:?} {} {}", c.relation, c.offset_seconds, c.command))
            .collect();
        commands.sort_unstable();
        let gaps = match self.kill_gaps.mode {
            KillGapMode::Keep => "keep".to_string(),
            KillGapMode::FastForward => format!(
                "fast_forward over={} speed={}",
                self.kill_gaps.over_seconds, self.kill_gaps.speed
            ),
        };
        let mut text = format!(
            "mode={}\nfps={}\ncodec={}\nsize={}x{}\nfov={}\ninstall={}\nhook={}\nbefore={}\nafter={}\ngaps={}\ndecal_flush={}\n",
            self.mode,
            self.fps,
            self.codec,
            self.width,
            self.height,
            self.fov,
            self.install.to_lowercase(),
            self.hook.to_lowercase(),
            self.before,
            self.after,
            gaps,
            self.decal_flush,
        );
        for line in lines {
            text.push_str("line=");
            text.push_str(line);
            text.push('\n');
        }
        for command in commands {
            text.push_str("command=");
            text.push_str(&command);
            text.push('\n');
        }
        text
    }
}

/// The rate the take is recorded at in `config`'s mode.
fn recording_fps(config: &PatcherConfig) -> i32 {
    match config.capture_mode {
        CaptureMode::Obs if config.obs_capture_fps > 0 => config.obs_capture_fps,
        CaptureMode::Obs => OBS_DEFAULT_FPS,
        CaptureMode::Agr => config.effective_agr_fps(),
        _ => config.capture_fps,
    }
}

/// `hl.exe`'s folder name: `Half-Life - POST-Anniversary for Movies`.
fn install_name(game_path: &str) -> String {
    Path::new(game_path)
        .parent()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Which hook DLL the launch injects (`goldsrc_hooks_dll`'s pick).
fn hook_label(config: &PatcherConfig) -> String {
    let Some(dll) = config.goldsrc_hooks_dll() else {
        return String::new();
    };
    let is_override = config
        .goldsrc_hooks_dll_path
        .as_deref()
        .is_some_and(|wanted| Path::new(wanted) == dll);
    if !is_override {
        return HOOK_BUNDLED.to_string();
    }
    let file = dll.file_name().map(|n| n.to_string_lossy().into_owned());
    let folder = dll
        .parent()
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned());
    match (folder, file) {
        (Some(folder), Some(file)) => format!("{folder}/{file}"),
        (None, Some(file)) => file,
        _ => dll.display().to_string(),
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes
        .iter()
        .fold(OFFSET, |hash, &b| (hash ^ u64::from(b)).wrapping_mul(PRIME))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> CaptureSettings {
        let mut s = CaptureSettings {
            mode: "frame_sequence".into(),
            fps: 120,
            width: 1920,
            height: 1080,
            fov: 90.0,
            install: "Half-Life - POST-Anniversary for Movies".into(),
            hook: HOOK_BUNDLED.into(),
            lines: vec!["mirv_fov 90".into(), "hud_draw 0".into()],
            commands: vec![
                ScheduledCommand {
                    command: "slowmo".into(),
                    offset_seconds: 1.0,
                    relation: CommandRelation::Before,
                },
                ScheduledCommand {
                    command: "normal".into(),
                    offset_seconds: 0.5,
                    relation: CommandRelation::After,
                },
            ],
            before: 5.0,
            after: 1.0,
            app_version: "0.12.0".into(),
            ..Default::default()
        };
        s.hash = s.hash_of();
        s
    }

    #[test]
    fn the_hash_names_its_scheme_and_is_stable() {
        let s = settings();
        assert!(s.hash.starts_with("v1-"), "{}", s.hash);
        assert_eq!(s.hash.len(), "v1-".len() + 16);
        assert_eq!(s.hash, settings().hash_of());
    }

    #[test]
    fn lines_and_commands_in_another_order_hash_the_same() {
        let mut other = settings();
        other.lines.reverse();
        other.commands.reverse();
        assert_eq!(other.hash_of(), settings().hash);
    }

    #[test]
    fn a_changed_line_or_setting_changes_the_hash() {
        let base = settings().hash;
        let mut line = settings();
        line.lines[0] = "mirv_fov 100".into();
        assert_ne!(line.hash_of(), base);
        let mut size = settings();
        size.width = 2560;
        assert_ne!(size.hash_of(), base);
        let mut command = settings();
        command.commands[0].offset_seconds = 2.0;
        assert_ne!(command.hash_of(), base);
        let mut install = settings();
        install.install = "Half-Life - PRE-Anniversary for Movies".into();
        assert_ne!(install.hash_of(), base);
    }

    #[test]
    fn the_app_version_and_the_install_spelling_leave_the_hash_alone() {
        let mut other = settings();
        other.app_version = "0.13.0".into();
        other.install = other.install.to_uppercase();
        assert_eq!(other.hash_of(), settings().hash);
    }

    #[test]
    fn kept_gaps_hash_the_same_whatever_their_unused_numbers() {
        let mut other = settings();
        other.kill_gaps.over_seconds = 20.0;
        assert_eq!(other.hash_of(), settings().hash);
        other.kill_gaps.mode = KillGapMode::FastForward;
        assert_ne!(other.hash_of(), settings().hash);
    }

    #[test]
    fn built_from_a_config_it_records_what_the_batch_used() {
        let mut config = PatcherConfig {
            game_path: "C:/Games/Half-Life - POST-Anniversary for Movies/hl.exe".into(),
            resolution_width: 2560,
            resolution_height: 1440,
            capture_fps: 300,
            init_commands: vec!["  mirv_fov 105 ".into(), "".into()],
            custom_commands: vec![CustomCommand {
                command: " host_framerate 0 ".into(),
                offset: 0.5,
                relation: CommandRelation::After,
            }],
            pre_roll_seconds: 4.0,
            post_roll_seconds: 2.0,
            ..PatcherConfig::default()
        };
        config.normalise_capture_mode();
        let s = CaptureSettings::from_config(&config, "9.9.9");
        assert_eq!(s.mode, "frame_sequence");
        assert_eq!((s.width, s.height, s.fps), (2560, 1440, 300));
        assert_eq!(s.fov, 105.0);
        assert_eq!(s.install, "Half-Life - POST-Anniversary for Movies");
        assert_eq!(s.lines, vec!["mirv_fov 105"]);
        assert_eq!(s.commands[0].command, "host_framerate 0");
        assert_eq!((s.before, s.after), (4.0, 2.0));
        assert!(s.codec.is_empty());
        assert_eq!(s.app_version, "9.9.9");
        assert_eq!(s.hash, s.hash_of());
    }

    #[test]
    fn the_recorded_rate_is_the_one_the_mode_records_at() {
        let mut config = PatcherConfig {
            capture_fps: 300,
            obs_capture_fps: 0,
            agr_fps: 0,
            ..PatcherConfig::default()
        };
        config.capture_mode = CaptureMode::Obs;
        assert_eq!(recording_fps(&config), OBS_DEFAULT_FPS);
        config.obs_capture_fps = 120;
        assert_eq!(recording_fps(&config), 120);
        config.capture_mode = CaptureMode::Agr;
        assert_eq!(recording_fps(&config), 300);
        config.capture_mode = CaptureMode::DirectToVideo;
        assert_eq!(recording_fps(&config), 300);
    }

    #[test]
    fn a_file_from_a_build_that_recorded_less_still_reads() {
        let s: CaptureSettings = serde_json::from_str(r#"{"hash":"v1-00","width":1280}"#).unwrap();
        assert_eq!(s.width, 1280);
        assert!(s.lines.is_empty());
    }
}
