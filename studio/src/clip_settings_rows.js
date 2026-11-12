// clip_settings_rows.js
// The rows of Project settings › Clip settings (#1085, build plan step 8):
// each one a console setting the project's Initial Commands can hold, with
// the values it offers and whether the game acts on it at once or only at
// the next map load. The tab is an editor over those lines
// (clip_settings_logic.js), so a row holds no value of its own: it shows the
// last line that sets its cvar, and an edit rewrites that line.
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

/** The cvars the rows show, lower case: a line setting one of them belongs
 *  to its row, never to Your own commands. Not field of view's: its row
 *  shows the project's value, and a `mirv_fov` line stays in Your own
 *  commands, where its note says a batch leaves it out (#1085). */
export const ROW_CVARS = Object.freeze(
  CLIP_SETTINGS_ROWS.filter((r) => r.cvar && r.kind !== 'picture').map((r) => r.cvar.toLowerCase()),
);
