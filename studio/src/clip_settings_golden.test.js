// Golden test for #1085 step 8: a project that never set the new fields
// (field of view on Picture, the Clip settings tab) sends a batch exactly the
// commands #1364 sends, byte for byte; opening the page and pressing OK
// changes nothing; and only a project with a field of view, with the New UI
// on, sends one (`picture_fov`, after its lines).

import { describe, it, expect } from 'vitest';
import { resolveSettings, resolveCaptureSettings, blockFromForm, followFormEdits } from './project_settings_logic.js';
import { startingValues, changedKeys, blockWithPending } from './project_settings_page_logic.js';

const PC = 'pc-golden';

// Clean clips as shipped before step 8, then a few lines of the user's own,
// as a project file made by #1361/#1364 holds them.
const OLD_CLEAN_CLIPS = [
  'r_decals 256', 'mirv_fov 90', 'ds_spec_match_pov 1', 'ds_spec_hide_hltv_messages 1', 'ds_spec_hide_bars 1',
  'ds_spec_deathcam 0', 'ds_spec_hide_text 1', 'ds_hud_hide_scoreboard 1', 'ds_hud_hide_voice_menus 1',
  'ds_hud_hide_hints 1', 'ds_hud_hide_crosshair 0', 'ds_hud_hide_map_text 1', 'ds_hud_hide_clan_text 1',
  'ds_hud_hide_center_text 1', 'ds_hud_show_scope 1', 'ds_hud_overview_players 0', 'ds_hud_overview_team_marker 0',
  'ds_hud_overview_hide_speaker 0', 'ds_view_use_cl_bob 0', 'ds_view_hide_headshot_red_screen 1',
  'ds_view_hide_screen_shake 1', 'ds_player_hide_hand_signals 0', 'ds_sound_mute_voice_commands 0',
  'ds_sound_mute_victory_music 0', 'ds_sound_mute_flag_capture 0', 'ds_map_allow_shaders 0',
];
const OWN = ['cl_righthand 1', 'exec movie_extra.cfg', 'mirv_fov "105"', ''];
const SCHEDULED = [
  { command: 'cl_righthand 0', relation: 'Before', offset_seconds: 1 },
  { command: 'cl_righthand 1', relation: 'After', offset_seconds: 0.5 },
];

const appForm = () => ({
  capture_mode: 'frame_sequence', resolution_width: 2560, resolution_height: 1440,
  capture_fps: 300, obs_capture_fps: 120, agr_fps: 0,
  finish_clips_after_batch: false, finish_codec_obs: 'source_copy', finish_codec_video: 'render_tab', finish_codec_frames: 'render_tab',
  record_start_lead: 5, record_stop_trail: 5, kill_gaps: { mode: 'keep', over_seconds: 8, speed: 4 },
  clip_name_template: '{name}_{kills}k_{time}', target_drives: ['E:/caps'], render_export_dirs: ['F:/clips'],
  command_profile_active: 'Clean clips', init_commands: [...OLD_CLEAN_CLIPS, ...OWN], custom_commands: SCHEDULED,
  hl_path: 'C:/Games/Half-Life - PRE-Anniversary for Movies/hl.exe',
});

// What batchSettingsPayload asks the resolver, from a Configuration that has
// since moved on to other lines: the project's must win.
const batchAsk = () => ({
  capture_fps: 120, obs_capture_fps: 120, agr_fps: 0, record_start_lead: 3, record_stop_trail: 3,
  resolution_width: 1920, resolution_height: 1080, capture_mode: 'frame_sequence',
  kill_gaps: { mode: 'keep', over_seconds: 8, speed: 4 },
  init_commands: ['mirv_fov 75', 'ds_hud_hide_scoreboard 0'], custom_commands: [], target_drives: ['Z:/other'],
});

// The project file #1364 writes for that form.
const projectBlock = () => JSON.parse(JSON.stringify(blockFromForm(appForm(), { pcId: PC, madeWith: null })));

describe('a project that never set the new fields', () => {
  it("sends #1364's Initial and Scheduled Commands, byte for byte", () => {
    const before = resolveSettings(batchAsk(), projectBlock(), PC);
    const after = resolveCaptureSettings(batchAsk(), projectBlock(), PC);
    expect(JSON.stringify(after)).toBe(JSON.stringify(before));
    expect(JSON.stringify(after.init_commands)).toBe(JSON.stringify([...OLD_CLEAN_CLIPS, ...OWN]));
    expect(JSON.stringify(after.custom_commands)).toBe(JSON.stringify(SCHEDULED));
  });

  it('sends the form as it is with no project open', () => {
    expect(JSON.stringify(resolveCaptureSettings(batchAsk(), null, PC))).toBe(JSON.stringify(batchAsk()));
  });

  it('stays so after Configuration edits follow into it: it has no field of view to give', () => {
    const { block } = followFormEdits(projectBlock(), appForm(), { ...appForm(), capture_fps: 240 }, PC);
    expect(block.picture.fov).toBeUndefined();
    expect(resolveCaptureSettings(batchAsk(), block, PC).init_commands).toEqual([...OLD_CLEAN_CLIPS, ...OWN]);
  });

  it('is left exactly as it was by opening Project settings and pressing OK', () => {
    const start = startingValues(appForm(), projectBlock(), PC);
    expect(changedKeys(start, structuredClone(start))).toEqual([]);
    const next = blockWithPending(projectBlock(), start, structuredClone(start), PC);
    expect(JSON.stringify(next)).toBe(JSON.stringify(projectBlock()));
  });
});

describe('a project with a field of view', () => {
  const withFov = () => ({ ...projectBlock(), picture: { ...projectBlock().picture, fov: 100 } });

  it('sends it as picture_fov with the New UI on, the lines as they are (the backend sends it after them)', () => {
    const sent = resolveCaptureSettings(batchAsk(), withFov(), PC, { pictureFov: true });
    expect(sent.picture_fov).toBe(100);
    expect(JSON.stringify(sent.init_commands)).toBe(JSON.stringify([...OLD_CLEAN_CLIPS, ...OWN]));
  });

  it('sends what #1364 sends with the New UI off', () => {
    const sent = resolveCaptureSettings(batchAsk(), withFov(), PC);
    expect(JSON.stringify(sent)).toBe(JSON.stringify(resolveSettings(batchAsk(), withFov(), PC)));
  });

  it('writes it under picture.fov from the page, nothing else changed', () => {
    const start = startingValues(appForm(), projectBlock(), PC);
    expect(start.capture_fov).toBeUndefined();
    const next = blockWithPending(projectBlock(), start, { ...structuredClone(start), capture_fov: 100 }, PC);
    expect(next.picture.fov).toBe(100);
    expect({ ...next, picture: { ...next.picture, fov: undefined } }).toEqual({ ...projectBlock(), picture: { ...projectBlock().picture, fov: undefined } });
  });
});
