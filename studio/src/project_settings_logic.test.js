import { describe, it, expect } from 'vitest';
import {
  PROJECT_FIELDS, resolveSettings, blockFromForm, followFormEdits, keysDifferingFromForm,
  madeWithOf, installFor, installToOffer, withMadeWith,
} from './project_settings_logic.js';

const PC = 'pc-a';
const OTHER_PC = 'pc-b';
const PRE = 'D:/Steam/steamapps/common/Half-Life - PRE-Anniversary for Movies/hl.exe';
const POST = 'D:/Steam/steamapps/common/Half-Life - POST-Anniversary for Movies/hl.exe';

/** A settings payload as persistAppSettings builds it, owned keys only. */
function appForm(extra = {}) {
  return {
    capture_mode: 'frame_sequence',
    resolution_width: 1920,
    resolution_height: 1080,
    capture_fps: 120,
    obs_capture_fps: 60,
    agr_fps: 0,
    finish_clips_after_batch: false,
    finish_codec_obs: 'source_copy',
    finish_codec_video: 'render_tab',
    finish_codec_frames: 'render_tab',
    record_start_lead: 5,
    record_stop_trail: 5,
    kill_gaps: { mode: 'keep', over_seconds: 8, speed: 4 },
    command_profile_active: 'Clean clips',
    init_commands: ['mirv_fov 90', 'cl_bob 0'],
    custom_commands: [{ command: 'slowmo', relation: 'Before', offset_seconds: 2 }],
    clip_name_template: '{player}_{map}',
    target_drives: ['E:/captures'],
    render_export_dirs: ['F:/renders'],
    hl_path: PRE,
    ...extra,
  };
}

describe('resolveSettings', () => {
  it('returns the form as it is when there is no project block', () => {
    const form = appForm();
    expect(resolveSettings(form, null, PC)).toEqual(form);
  });

  it('puts the project value over the form value for each setting it sets', () => {
    const block = { picture: { width: 2560, height: 1440, captureMode: 'obs' }, clipNames: { template: '{map}' } };
    const resolved = resolveSettings(appForm(), block, PC);
    expect(resolved).toMatchObject({ resolution_width: 2560, resolution_height: 1440, capture_mode: 'obs', clip_name_template: '{map}' });
    expect(resolved.capture_fps).toBe(120);
  });

  it('answers only the keys the reader asked about', () => {
    const block = { picture: { width: 2560, captureFps: 300 } };
    expect(resolveSettings({ resolution_width: 1920 }, block, PC)).toEqual({ resolution_width: 2560 });
  });

  it('leaves a bad hand-edited value to the form', () => {
    const block = { picture: { width: -5, height: '1440', captureMode: 'webcam', captureFps: 1.5 } };
    expect(resolveSettings(appForm(), block, PC)).toMatchObject({
      resolution_width: 1920, resolution_height: 1080, capture_mode: 'frame_sequence', capture_fps: 120,
    });
  });

  it('takes AGR rate 0 from the project as the same-as-Capture-FPS value', () => {
    expect(resolveSettings({ agr_fps: 30 }, { picture: { agrFps: 0 } }, PC)).toEqual({ agr_fps: 0 });
  });

  it('uses only this PC\'s folders', () => {
    const block = { folders: { [OTHER_PC]: { destinations: ['X:/theirs'] }, [PC]: { exports: ['G:/mine'] } } };
    const resolved = resolveSettings(appForm(), block, PC);
    expect(resolved.target_drives).toEqual(['E:/captures']);
    expect(resolved.render_export_dirs).toEqual(['G:/mine']);
  });

  it('ignores the folders without a PC id', () => {
    const block = { folders: { '': { destinations: ['X:/x'] } } };
    expect(resolveSettings(appForm(), block, '').target_drives).toEqual(['E:/captures']);
  });

  it('normalises a project kill-gaps value and scheduled commands', () => {
    const block = {
      clipLength: { killGaps: { mode: 'fast_forward', over_seconds: 10, speed: 99 } },
      clipSettings: { customCommands: [{ command: 'x', relation: 'sideways', offset_seconds: 1 }] },
    };
    const resolved = resolveSettings(appForm(), block, PC);
    expect(resolved.kill_gaps).toEqual({ mode: 'fast_forward', over_seconds: 10, speed: 16 });
    expect(resolved.custom_commands).toEqual([{ command: 'x', relation: 'Before', offset_seconds: 1 }]);
  });

  it('does not change the form or the block', () => {
    const form = appForm();
    const block = { clipSettings: { initCommands: ['a'] } };
    const resolved = resolveSettings(form, block, PC);
    resolved.init_commands.push('b');
    expect(block.clipSettings.initCommands).toEqual(['a']);
    expect(form.init_commands).toEqual(['mirv_fov 90', 'cl_bob 0']);
  });
});

describe('blockFromForm', () => {
  it('copies every owned setting, so the project resolves to the form it came from', () => {
    const form = appForm();
    const block = blockFromForm(form, { pcId: PC, madeWith: madeWithOf(PRE) });
    const other = appForm({ resolution_width: 800, init_commands: [], target_drives: [], clip_name_template: 'x' });
    const { hl_path: _hl, ...owned } = form;
    expect(resolveSettings(other, block, PC)).toEqual({ ...owned, hl_path: PRE });
  });

  it('lays the block out as the build plan\'s data model', () => {
    const block = blockFromForm(appForm(), { pcId: PC, madeWith: madeWithOf(PRE) });
    expect(block.picture).toMatchObject({ captureMode: 'frame_sequence', width: 1920, height: 1080, finished: { enabled: false } });
    expect(block.clipLength).toMatchObject({ beforeSeconds: 5, afterSeconds: 5 });
    expect(block.clipSettings.preset).toBe('Clean clips');
    expect(block.clipNames.template).toBe('{player}_{map}');
    expect(block.folders).toEqual({ [PC]: { destinations: ['E:/captures'], exports: ['F:/renders'] } });
    expect(block.install.madeWith).toEqual({ name: 'Half-Life - PRE-Anniversary for Movies', hlExe: PRE });
  });

  it('has a path for every owned field', () => {
    const block = blockFromForm(appForm(), { pcId: PC });
    expect(keysDifferingFromForm(appForm(), block, PC)).toEqual([]);
    expect(PROJECT_FIELDS.length).toBe(17);
  });
});

describe('followFormEdits', () => {
  it('copies just the settings an edit changed', () => {
    const block = { picture: { width: 2560, captureFps: 300 } };
    const { block: next, changed } = followFormEdits(block, appForm(), appForm({ capture_fps: 240 }), PC);
    expect(changed).toEqual(['capture_fps']);
    expect(next.picture).toEqual({ width: 2560, captureFps: 240 });
  });

  it('keeps fields this build does not know', () => {
    const block = { picture: { width: 1 }, players: { a: 1 }, clipSettings: { future: true } };
    const { block: next } = followFormEdits(block, appForm(), appForm({ init_commands: ['x'] }), PC);
    expect(next.players).toEqual({ a: 1 });
    expect(next.clipSettings).toEqual({ future: true, initCommands: ['x'] });
  });

  it('returns the same block when no owned setting changed', () => {
    const block = { picture: {} };
    const result = followFormEdits(block, appForm(), appForm({ notify_error: false }), PC);
    expect(result).toEqual({ block, changed: [] });
  });

  it('puts a changed folder list under this PC only', () => {
    const block = { folders: { [OTHER_PC]: { destinations: ['X:/theirs'] } } };
    const { block: next } = followFormEdits(block, appForm(), appForm({ target_drives: ['E:/new'] }), PC);
    expect(next.folders).toEqual({ [OTHER_PC]: { destinations: ['X:/theirs'] }, [PC]: { destinations: ['E:/new'] } });
  });

  it('makes a newly picked install the one the project is made with', () => {
    const block = withMadeWith({}, PRE);
    const { block: next, changed } = followFormEdits(block, appForm(), appForm({ hl_path: POST }), PC);
    expect(changed).toEqual(['hl_path']);
    expect(next.install.madeWith.hlExe).toBe(POST);
  });

  it('leaves the install alone when the pick is already the project\'s', () => {
    const block = withMadeWith({}, POST);
    const { changed } = followFormEdits(block, appForm(), appForm({ hl_path: POST }), PC);
    expect(changed).toEqual([]);
  });
});

describe('keysDifferingFromForm', () => {
  it('names the settings the project sets to another value', () => {
    const block = { picture: { width: 1920, height: 1440 }, clipNames: { template: '{map}' } };
    expect(keysDifferingFromForm(appForm(), block, PC)).toEqual(['resolution_height', 'clip_name_template']);
  });
});

describe('installs', () => {
  const installs = [
    { name: 'Half-Life - PRE-Anniversary for Movies', hl_exe: PRE.replace(/\//g, '\\'), build: 'pre_anniversary' },
    { name: 'Half-Life - POST-Anniversary for Movies', hl_exe: POST.replace(/\//g, '\\'), build: 'anniversary' },
  ];

  it('remembers no install for a blank hl.exe', () => {
    expect(madeWithOf('  ')).toBe(null);
  });

  it('finds an install by its hl.exe whatever the slashes', () => {
    expect(installFor(installs, POST)?.build).toBe('anniversary');
  });

  it('offers the install the project was made with while another is picked', () => {
    expect(installToOffer(madeWithOf(POST), PRE, installs)?.build).toBe('anniversary');
  });

  it('offers nothing when the project\'s install is picked', () => {
    expect(installToOffer(madeWithOf(POST), POST, installs)).toBe(null);
  });

  it('finds a project from another PC by its install\'s folder name', () => {
    const elsewhere = madeWithOf('C:/Games/Steam/steamapps/common/Half-Life - POST-Anniversary for Movies/hl.exe');
    expect(installToOffer(elsewhere, PRE, installs)?.build).toBe('anniversary');
  });

  it('offers nothing when this PC has no such install, or the project names none', () => {
    expect(installToOffer(madeWithOf('C:/Other/Half-Life/hl.exe'), PRE, installs)).toBe(null);
    expect(installToOffer(null, PRE, installs)).toBe(null);
  });
});
