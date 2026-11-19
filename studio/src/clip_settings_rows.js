// clip_settings_rows.js
// The rows of Project settings › Clip settings (#1085, build plan step 8):
// each one a console setting the project's Initial Commands can hold, with
// the values it offers, the game's own value for it (ROW_DEFAULTS) and
// whether the game acts on it at once or only at the next map load. The tab
// is an editor over those lines (clip_settings_logic.js): a row shows the
// last line that sets its cvar, else the game's own value, and an edit
// rewrites that line.
//
// Sections and order follow the in-game Settings tab (goldsrc-hooks
// settings_tab.rs), as the canvas does. Labels and hints are in strings.js
// (CLIP_SETTINGS.ROWS, by row id).

/** A 0/1 setting. */
const toggle = (id, cvar, extra = {}) => ({ id, cvar, kind: 'toggle', values: ['0', '1'], when: 'once', ...extra });
/** A setting with a few numbered choices, `values` in order. */
const select = (id, cvar, values, extra = {}) => ({ id, cvar, kind: 'select', values, when: 'once', ...extra });
/** A setting typed as a number. */
const number = (id, cvar, extra = {}) => ({ id, cvar, kind: 'number', when: 'once', ...extra });

/**
 * The sections, top to bottom, each with its rows. Row kinds:
 * - `toggle`, `select`, `number`: an Initial Commands line `<cvar> <value>`;
 * - `picture`: shown here, set on the Project tab (field of view, which the
 *   project owns and which wins over any preset line);
 * - `elsewhere`: a setting another page owns, shown with a link and never
 *   written as a line (sprays and crosshairs follow the Players table, which
 *   Studio sends itself).
 * `when: 'load'` marks a setting the game reads as a map loads: an Initial
 * Commands line runs after the map has loaded, so it reaches the next demo
 * of a batch, not the first (build plan step 13 adds the stage before).
 */
export const CLIP_SETTINGS_SECTIONS = Object.freeze([
  {
    id: 'hud',
    rows: [
      toggle('hud_draw', 'hud_draw'),
      toggle('draw_gun', 'r_drawviewmodel'),
      toggle('hide_crosshair', 'ds_hud_hide_crosshair'),
      toggle('hide_scoreboard', 'ds_hud_hide_scoreboard'),
      toggle('hide_teammate_info', 'ds_hud_hide_teammate_info'),
      toggle('hide_voice_menus', 'ds_hud_hide_voice_menus'),
      toggle('hide_hints', 'ds_hud_hide_hints'),
      toggle('show_scope', 'ds_hud_show_scope'),
      toggle('hide_map_text', 'ds_hud_hide_map_text'),
      toggle('hide_clan_text', 'ds_hud_hide_clan_text'),
      toggle('hide_center_text', 'ds_hud_hide_center_text'),
      toggle('hide_plugin_text', 'ds_hud_hide_plugin_text'),
    ],
  },
  {
    id: 'view',
    rows: [
      { id: 'fov', cvar: 'mirv_fov', kind: 'picture', when: 'once' },
      toggle('hide_red_screen', 'ds_view_hide_headshot_red_screen'),
      toggle('hide_screen_shake', 'ds_view_hide_screen_shake'),
      select('own_blood', 'ds_view_show_own_blood', ['0', '1', '2', '3', '4']),
      toggle('use_cl_bob', 'ds_view_use_cl_bob'),
    ],
  },
  {
    id: 'spectating',
    rows: [
      toggle('match_pov', 'ds_spec_match_pov'),
      select('player_info', 'ds_spec_show_player_info', ['0', '1', '2', '3']),
      toggle('deathcam', 'ds_spec_deathcam'),
      toggle('hide_bars', 'ds_spec_hide_bars'),
      toggle('hide_camera_text', 'ds_spec_hide_text'),
      toggle('hide_hltv_messages', 'ds_spec_hide_hltv_messages'),
    ],
  },
  {
    id: 'players',
    rows: [
      { id: 'sprays', cvar: null, kind: 'elsewhere', when: 'once' },
      select('xray', 'ds_spec_xray_show', ['0', '1', '2']),
      toggle('hide_hand_signals', 'ds_player_hide_hand_signals'),
      select('overview_players', 'ds_hud_overview_players', ['0', '1', '2', '3', '4']),
      select('overview_marker', 'ds_hud_overview_team_marker', ['0', '1', '2']),
      toggle('overview_speaker', 'ds_hud_overview_hide_speaker'),
    ],
  },
  {
    id: 'map',
    rows: [
      toggle('hd_textures', 'ds_hd_enabled', { when: 'load' }),
      select('weather', 'ds_map_weather', ['0', '1', '2', '3']),
      toggle('map_lighting', 'ds_map_apply_lighting'),
      toggle('map_shaders', 'ds_map_allow_shaders'),
      toggle('map_cfgs', 'ds_map_run_cfgs', { when: 'load' }),
      number('decals', 'r_decals', { min: 0, integer: true }),
    ],
  },
  {
    id: 'sound',
    rows: [
      toggle('mute_voice', 'ds_sound_mute_voice_commands'),
      toggle('mute_victory', 'ds_sound_mute_victory_music'),
      toggle('mute_flags', 'ds_sound_mute_flag_capture'),
      toggle('restore_gunshots', 'ds_sound_restore_gunshots'),
      number('recording_volume', 'mirv_movie_sound_volume', { min: 0, step: '0.05' }),
    ],
  },
]);

/** Every row, in page order. */
export const CLIP_SETTINGS_ROWS = Object.freeze(CLIP_SETTINGS_SECTIONS.flatMap((s) => s.rows));

/**
 * The game's own value for each row's cvar, by cvar: what a row shows while
 * no line sets it, and what a batch sends for it, with Use Clip settings on.
 * The `ds_` defaults are the ones goldsrc-hooks registers the cvar with
 * (`commands/install.rs`; docs/dodstudio_commands.md), so a row left alone
 * sends exactly what the hook would do with no line at all.
 */
export const ROW_DEFAULTS = Object.freeze({
  hud_draw: '1', // the game's own (client.dll's HUD registers it "1", as the HL SDK's hud.cpp does)
  r_drawviewmodel: '1', // GoldSrc's own (the engine registers it "1")
  ds_hud_hide_crosshair: '0', // install.rs: CROSSHAIR_NAME "0"
  ds_hud_hide_scoreboard: '0', // install.rs: SCOREBOARD_NAME "0"
  ds_hud_hide_teammate_info: '0', // install.rs: teammate_info "0"
  ds_hud_hide_voice_menus: '0', // install.rs: voice_menus "0"
  ds_hud_hide_hints: '1', // install.rs: hint_panel "1"
  ds_hud_show_scope: '1', // install.rs: demo_scope "1"
  ds_hud_hide_map_text: '0', // install.rs: map_text "0"
  ds_hud_hide_clan_text: '0', // install.rs: clan_text "0"
  ds_hud_hide_center_text: '0', // install.rs: center_text "0"
  ds_hud_hide_plugin_text: '0', // install.rs: plugin_text "0"
  ds_view_hide_headshot_red_screen: '0', // install.rs: screen_effects FLASH_NAME "0"
  ds_view_hide_screen_shake: '0', // install.rs: screen_effects SHAKE_NAME "0"
  ds_view_show_own_blood: '0', // install.rs: own_blood "0"
  ds_view_use_cl_bob: '0', // install.rs: pov_live_bob "0"
  ds_spec_match_pov: '1', // lib.rs: SPEC_MATCH_POV_DEFAULT = LEVEL_MAX (1)
  ds_spec_show_player_info: '0', // install.rs: nameplates "0"
  ds_spec_deathcam: '0', // install.rs: spectator_deathcam "0"
  ds_spec_hide_bars: '0', // install.rs: spectator_bars "0"
  ds_spec_hide_text: '0', // install.rs: spectator_text "0"
  ds_spec_hide_hltv_messages: '0', // install.rs: hltv_messages "0"
  ds_spec_xray_show: '0', // install.rs: spec_xray "0"
  ds_player_hide_hand_signals: '0', // install.rs: HAND_SIGNALS_NAME "0"
  ds_hud_overview_players: '0', // install.rs: overview_players "0"
  ds_hud_overview_team_marker: '0', // install.rs: overview_marker "0"
  ds_hud_overview_hide_speaker: '0', // install.rs: overview_speaker "0"
  // None (null): the game decides, and a batch sends a line only when one
  // sets it (SENT_ONLY_WHEN_SET). The hook starts it on when the install has
  // a dodstudio_hd folder and off when it hasn't (goldsrc-hooks
  // texture_hires::starts_on, lib.rs), then the in-game Settings tab's saved
  // value (dodstudio_ui\settings.cfg, applied on the first frame) wins, and
  // a batch's Initial Commands win over both. So a fixed 1 would turn HD back
  // on for a player who turned it off there, and on an install with no HD
  // folder it would install the texture hook mid-session (five hw.dll
  // detours, the raised texture size ceiling and the leftover-texture fix)
  // that the game otherwise never installs, to load nothing.
  ds_hd_enabled: null,
  ds_map_weather: '0', // install.rs: map_weather "0"
  ds_map_apply_lighting: '1', // install.rs: map_light "1"
  ds_map_allow_shaders: '0', // install.rs: world_shaders "0"
  ds_map_run_cfgs: '1', // install.rs: map_cfgs "1"
  r_decals: '4096', // GoldSrc's own (hw.dll registers it 4096, docs/goldsrc_hw_dll_survey.md)
  ds_sound_mute_voice_commands: '0', // install.rs: VOICE_NAME "0"
  ds_sound_mute_victory_music: '0', // install.rs: map_sounds families "0"
  ds_sound_mute_flag_capture: '0', // install.rs: map_sounds families "0"
  ds_sound_restore_gunshots: '1', // install.rs: missing_shots "1"
  mirv_movie_sound_volume: '0.4', // HLAE's own (AfxHookGoldSrc registers it "0.4")
});

/** The cvars with no one value of the game's (null above): a row for one
 *  shows "Game's own" while no line sets it, and a batch sends a line for
 *  it only when one does. */
export const SENT_ONLY_WHEN_SET = Object.freeze(['ds_hd_enabled']);

/** The rows a batch sends a line for with Use Clip settings on, in page
 *  order: every row that is a line (not field of view, not sprays), those in
 *  SENT_ONLY_WHEN_SET only when a line sets them. */
export const SENT_ROWS = Object.freeze(CLIP_SETTINGS_ROWS.filter((r) => ['toggle', 'select', 'number'].includes(r.kind)));

/** The cvars the rows show, lower case: a line setting one of them belongs
 *  to its row, never to Your own commands. Not field of view's: its row
 *  shows the project's value, and a `mirv_fov` line stays in Your own
 *  commands, where its note says a batch leaves it out (#1085). */
export const ROW_CVARS = Object.freeze(
  CLIP_SETTINGS_ROWS.filter((r) => r.cvar && r.kind !== 'picture').map((r) => r.cvar.toLowerCase()),
);
