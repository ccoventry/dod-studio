/**
 * Every hook console name from before #555, with the name that replaced it
 * (the signed-off table on #555). The new names put the area second
 * (`ds_hud_hide_scoreboard`), so they come from this table, not from a
 * prefix swap. A copy of `goldsrc-hooks/src/old_names.rs`, which the Rust
 * side shares; `console_renames.test.js` checks the two agree.
 */
export const OLD_NAMES = Object.freeze({
  dodstudio_allow_shaders: 'ds_map_allow_shaders',
  dodstudio_batch: 'ds_capture_batch',
  dodstudio_clear_decals: 'ds_map_clear_decals',
  dodstudio_console_in_panel: 'ds_ui_console_in_panel',
  dodstudio_deathmsg: 'ds_hud_deathmsg',
  dodstudio_debug_cloak_instead: 'ds_debug_cloak_instead',
  dodstudio_debug_hd_misses: 'ds_debug_hd_misses',
  dodstudio_debug_keep_detail_cache: 'ds_debug_keep_detail_cache',
  dodstudio_debug_log_spectator_target: 'ds_debug_log_spectator_target',
  dodstudio_debug_log_texture_loads: 'ds_debug_log_texture_loads',
  dodstudio_debug_log_weapon_model: 'ds_debug_log_weapon_model',
  dodstudio_debug_msglog: 'ds_debug_msglog',
  dodstudio_debug_position: 'ds_debug_position',
  dodstudio_debug_status: 'ds_debug_status',
  dodstudio_demo_fast_load: 'ds_demo_fast_load',
  dodstudio_demo_list_count_subfolders: 'ds_ui_demo_list_count_subfolders',
  dodstudio_demo_list_folders: 'ds_ui_demo_list_folders',
  dodstudio_demo_list_hide_empty: 'ds_ui_demo_list_hide_empty',
  dodstudio_demo_list_show_counts: 'ds_ui_demo_list_show_counts',
  dodstudio_demo_other_drives: 'ds_demo_other_drives',
  dodstudio_ex_interp_max: 'ds_demo_ex_interp_max',
  dodstudio_hd_enabled: 'ds_hd_enabled',
  dodstudio_hd_style: 'ds_hd_style',
  dodstudio_hide_asset: 'ds_map_hide_asset',
  dodstudio_hide_clan_text: 'ds_hud_hide_clan_text',
  dodstudio_hide_crosshair: 'ds_hud_hide_crosshair',
  dodstudio_hide_hand_signals: 'ds_player_hide_hand_signals',
  dodstudio_hide_headshot_red_screen: 'ds_view_hide_headshot_red_screen',
  dodstudio_hide_hltv_messages: 'ds_spec_hide_hltv_messages',
  dodstudio_hide_hudelement: 'ds_hud_hide_element',
  dodstudio_hide_map_text: 'ds_hud_hide_map_text',
  dodstudio_hide_scoreboard: 'ds_hud_hide_scoreboard',
  dodstudio_hide_screen_shake: 'ds_view_hide_screen_shake',
  dodstudio_hide_spectator_bars: 'ds_spec_hide_bars',
  dodstudio_hide_voice_menus: 'ds_hud_hide_voice_menus',
  dodstudio_hud_map_players: 'ds_hud_overview_players',
  dodstudio_hud_map_team_marker: 'ds_hud_overview_team_marker',
  dodstudio_mute_voice_commands: 'ds_sound_mute_voice_commands',
  dodstudio_objectives: 'ds_hud_objectives',
  dodstudio_overviewmap: 'ds_hud_overview_layout',
  dodstudio_panel: 'ds_ui_panel',
  dodstudio_player_overrides: 'ds_player_overrides',
  dodstudio_pov_live_bob: 'ds_view_bob',
  dodstudio_reload_demo: 'ds_demo_reload',
  dodstudio_remember_window_layout: 'ds_ui_remember_layout',
  dodstudio_resizable_windows: 'ds_ui_resizable_windows',
  dodstudio_review: 'ds_demo_review',
  dodstudio_run_in_background: 'ds_capture_run_in_background',
  dodstudio_schedule: 'ds_capture_schedule',
  dodstudio_seek_by: 'ds_demo_seek_by',
  dodstudio_seek_skip_between: 'ds_demo_seek_skip_between',
  dodstudio_seek_to: 'ds_demo_seek_to',
  dodstudio_show_demo_scope: 'ds_hud_show_scope',
  dodstudio_spec_deathcam: 'ds_spec_deathcam',
  dodstudio_spec_lock: 'ds_spec_lock',
  dodstudio_spec_match_pov: 'ds_spec_match_pov',
  dodstudio_spec_target: 'ds_spec_target',
  dodstudio_spray: 'ds_player_spray',
  dodstudio_stay_behind_while_capturing: 'ds_capture_stay_behind',
  dodstudio_viewdemo_in_panel: 'ds_ui_viewdemo_in_panel',
});

/** The hook's console prefix before #555. */
export const OLD_PREFIX = 'dodstudio_';

/** The current name for a pre-#555 one, or null. Case does not matter, as it
 * does not to the console. */
export function currentName(word) {
  return OLD_NAMES[String(word).toLowerCase()] ?? null;
}
