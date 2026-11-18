// The Clip settings tab's rules (#1085, build plan step 8): rows read and
// rewrite Initial Commands lines in place, Your own commands holds the rest,
// presets copy in, and a mirv_fov line stays in Your own commands (the
// project's field of view is sent after it, #1085).

import { describe, it, expect } from 'vitest';
import {
  parseSetting, rowValue, withRowValue, ownEntries, withLineAt, withoutLineAt, withRowLinesMerged,
  fovFromLines, presetPick, isUsableNumber, numberRowValue, isUsableFov, clipProblems,
  rowsAreOn, shownRowValue, linesWithRows, differsFromConfig,
} from './clip_settings_logic.js';
import { CLIP_SETTINGS_ROWS, SENT_ROWS, ROW_DEFAULTS } from './clip_settings_rows.js';
import { STRINGS } from './strings.js';

const S = STRINGS.CLIP_SETTINGS;

describe('parseSetting', () => {
  it('reads a name and value, a .cfg quote taken off', () => {
    expect(parseSetting('ds_hud_hide_scoreboard 1')).toEqual({ name: 'ds_hud_hide_scoreboard', value: '1' });
    expect(parseSetting('  mirv_fov "105" ')).toEqual({ name: 'mirv_fov', value: '105' });
    expect(parseSetting('R_Decals 256')).toEqual({ name: 'r_decals', value: '256' });
  });

  it('reads nothing a row could show: blank, comments, joined commands, a bare name', () => {
    for (const line of ['', '   ', '// ds_hud_hide_scoreboard 1', 'mirv_fov 90; r_decals 128', 'ds_map_clear_decals', 'exec']) {
      expect(parseSetting(line)).toBeNull();
    }
  });
});

describe('rowValue and withRowValue', () => {
  const lines = ['r_decals 256', 'cl_righthand 1', 'ds_hud_hide_scoreboard 0', 'exec mine.cfg', 'ds_hud_hide_scoreboard "1"'];

  it('reads the last line for the cvar, as the game does', () => {
    expect(rowValue(lines, 'ds_hud_hide_scoreboard')).toBe('1');
    expect(rowValue(lines, 'ds_hud_show_scope')).toBeNull();
  });

  it('rewrites the last line in place and drops the earlier ones, everything else as it was', () => {
    expect(withRowValue(lines, 'ds_hud_hide_scoreboard', '0'))
      .toEqual(['r_decals 256', 'cl_righthand 1', 'exec mine.cfg', 'ds_hud_hide_scoreboard 0']);
    expect(withRowValue(lines, 'r_decals', '4096')[0]).toBe('r_decals 4096');
  });

  it('adds a line at the end for a cvar no line sets', () => {
    expect(withRowValue(['cl_righthand 1'], 'ds_hud_show_scope', '1')).toEqual(['cl_righthand 1', 'ds_hud_show_scope 1']);
  });

  it('drops every line for the cvar on null (a blank number box: back to the game\'s own)', () => {
    expect(withRowValue(lines, 'ds_hud_hide_scoreboard', null)).toEqual(['r_decals 256', 'cl_righthand 1', 'exec mine.cfg']);
  });

  it('changes nothing it was not asked to, the original list untouched', () => {
    const before = [...lines];
    withRowValue(lines, 'ds_hud_hide_scoreboard', '0');
    expect(lines).toEqual(before);
  });
});

describe('Your own commands', () => {
  const lines = ['r_decals 256', 'cl_righthand 1', '', 'mirv_fov 90', 'exec mine.cfg', 'ds_spec_xray_show 1'];

  it('lists the lines no row shows, with their place in the list', () => {
    // mirv_fov too: it is no row's, so its refused note shows (#1085).
    expect(ownEntries(lines)).toEqual([
      { index: 1, text: 'cl_righthand 1' }, { index: 2, text: '' }, { index: 3, text: 'mirv_fov 90' },
      { index: 4, text: 'exec mine.cfg' },
    ]);
  });

  it('lists every line with Use Clip settings off, since no row shows', () => {
    expect(ownEntries(lines, { rowsOn: false }).map((e) => e.text)).toEqual(lines);
  });

  it('edits and removes by that place', () => {
    expect(withLineAt(lines, 1, 'cl_righthand 0')[1]).toBe('cl_righthand 0');
    expect(withoutLineAt(lines, 2)).toHaveLength(lines.length - 1);
  });

  it('merges a row setting typed there into its row, keeping the typed (last) one', () => {
    const typed = [...lines, 'ds_spec_xray_show 2'];
    expect(withRowLinesMerged(typed)).toEqual(['r_decals 256', 'cl_righthand 1', '', 'mirv_fov 90', 'exec mine.cfg', 'ds_spec_xray_show 2']);
    expect(withRowLinesMerged(lines)).toBe(lines);
  });
});

describe('Use Clip settings', () => {
  const lines = ['cl_righthand 1', 'ds_hud_hide_scoreboard 1', 'exec mine.cfg', 'mirv_fov 90', 'r_decals 256'];

  it('is on only when the value is true: a project without it reads as Off', () => {
    expect(rowsAreOn({ clip_rows_on: true })).toBe(true);
    expect(rowsAreOn({})).toBe(false);
    expect(rowsAreOn({ clip_rows_on: false })).toBe(false);
  });

  it("shows a row's line, else the game's own value: never nothing", () => {
    expect(shownRowValue(lines, 'ds_hud_hide_scoreboard')).toBe('1');
    expect(shownRowValue(lines, 'ds_hud_show_scope')).toBe(ROW_DEFAULTS.ds_hud_show_scope);
    for (const r of SENT_ROWS) expect(shownRowValue([], r.cvar)).toBe(ROW_DEFAULTS[r.cvar]);
  });

  it('sends the own lines in order, then one line for every row with its shown value', () => {
    const sent = linesWithRows(lines);
    expect(sent.slice(0, 3)).toEqual(['cl_righthand 1', 'exec mine.cfg', 'mirv_fov 90']);
    expect(sent.slice(3)).toEqual(SENT_ROWS.map((r) => `${r.cvar} ${shownRowValue(lines, r.cvar)}`));
    expect(sent).toContain('r_decals 256');
    expect(sent).toContain(`ds_hud_show_scope ${ROW_DEFAULTS.ds_hud_show_scope}`);
  });

  it('sends every row even with no lines at all, each under 64 bytes', () => {
    const sent = linesWithRows([]);
    expect(sent).toHaveLength(SENT_ROWS.length);
    for (const line of sent) expect(new TextEncoder().encode(line).length).toBeLessThan(64);
  });

  it('compares a config value as the native rules do', () => {
    expect(differsFromConfig('1', '1.0')).toBe(false);
    expect(differsFromConfig('0', '1')).toBe(true);
    expect(differsFromConfig('ultrasharp', 'UltraSharp')).toBe(false);
  });
});

describe('field of view', () => {
  it('reads the last usable mirv_fov line', () => {
    expect(fovFromLines(['mirv_fov 90', 'mirv_fov "100"'])).toBe(100);
    expect(fovFromLines(['mirv_fov handleZoom enabled 1'])).toBeNull();
    expect(fovFromLines([])).toBeNull();
  });

  it('looks past a handleZoom line to the number before it (#1085)', () => {
    expect(fovFromLines(['mirv_fov 105', 'mirv_fov handleZoom enabled 1'])).toBe(105);
  });

  it('takes 1 to 179', () => {
    expect([1, 90, 179].every(isUsableFov)).toBe(true);
    expect([0, 180, NaN, '90', null].some(isUsableFov)).toBe(false);
  });
});

describe('presetPick', () => {
  it('copies the preset in: its lists cleaned, its name, no link', () => {
    const preset = { name: 'Clean clips', builtin: true, init_commands: ['r_decals 256', ' '], custom_commands: [] };
    const picked = presetPick(preset);
    expect(picked).toEqual({ command_profile_active: 'Clean clips', init_commands: ['r_decals 256'], custom_commands: [] });
    picked.init_commands.push('x 1');
    expect(preset.init_commands).toEqual(['r_decals 256', ' ']);
  });

  it('empties both lists for No preset', () => {
    expect(presetPick(null)).toEqual({ command_profile_active: '', init_commands: [], custom_commands: [] });
  });
});

describe('number rows and problems', () => {
  const decals = CLIP_SETTINGS_ROWS.find((r) => r.id === 'decals');
  const volume = CLIP_SETTINGS_ROWS.find((r) => r.id === 'recording_volume');

  it('takes a number at the row minimum, whole where the row says so', () => {
    expect(isUsableNumber(decals, '4096')).toBe(true);
    expect(isUsableNumber(decals, '25.5')).toBe(false);
    expect(isUsableNumber(volume, '0.25')).toBe(true);
    expect(isUsableNumber(volume, '-1')).toBe(false);
    expect(isUsableNumber(volume, 'loud')).toBe(false);
    expect(numberRowValue('  ')).toBeNull();
  });

  it('names a number row with no usable number, and a command with no usable seconds', () => {
    const problems = clipProblems({
      clip_rows_on: true,
      init_commands: ['r_decals lots', 'mirv_movie_sound_volume 0.25'],
      custom_commands: [{ command: 'cl_righthand 0', relation: 'Before', offset_seconds: NaN }],
    });
    expect(problems).toEqual({ 'clip-decals': S.badNumber(0), custom_commands: S.BAD_OFFSET });
  });

  it('leaves a number row line alone with Use Clip settings off: it is an own line, sent as typed', () => {
    expect(clipProblems({ clip_rows_on: false, init_commands: ['r_decals lots'], custom_commands: [] })).toEqual({});
  });

  it('lets a blank At each clip row through: a capture drops it', () => {
    expect(clipProblems({ init_commands: [], custom_commands: [{ command: ' ', offset_seconds: NaN }] })).toEqual({});
  });
});
