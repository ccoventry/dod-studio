// config_checks_cmd.rs
// The Capture tab's advice on its settings: the config-file warnings
// (`native::patch::cfg_report`) and how long the pre-roll and post-roll need
// to be. Both only convert the frontend's arguments and call `native`.

use crate::capture_manager::CustomCommandPayload;
use native::patch::CustomCommand;
use native::patch::cfg_report::{CfgReport, CfgReportRequest, cfg_report};
use serde::{Deserialize, Serialize};

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
    hook_runs_batch: Option<bool>,
) -> Result<CfgReport, String> {
    let request = CfgReportRequest {
        game_path,
        init_commands,
        scheduled_commands: custom_commands.iter().map(CustomCommand::from).collect(),
        capture_fps,
        decal_flush,
        hook_runs_batch,
    };
    tokio::task::spawn_blocking(move || cfg_report(request))
        .await
        .map_err(crate::messages::config_scan_failed)
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
    cfg.custom_commands = custom_commands.iter().map(CustomCommand::from).collect();

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
