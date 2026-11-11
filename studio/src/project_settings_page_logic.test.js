// The Project settings page's rules (#1085, build plan step 6): what the
// Project tab starts from, what a pending edit changes, what stops Apply,
// and the block Apply writes.

import { describe, it, expect } from 'vitest';
import {
  PROJECT_TAB_KEYS, PAGE_KEYS, sizeChoiceOf, rateChoiceOf, rateKeyFor, rateOf, codecKeyFor, startingValues,
  changedKeys, hasChanges, pendingProblems, blockWithPending, templateOrDefault, killGapsWith,
  withFolder, withoutFolder, sameDriveLines, installState,
} from './project_settings_page_logic.js';
import { DEFAULT_TEMPLATE } from './clip_name.js';
import { STRINGS } from './strings.js';

const S = STRINGS.PROJECT_SETTINGS_PAGE;
const POST = 'C:/Steam/steamapps/common/Half-Life - POST-Anniversary for Movies/hl.exe';

// The settings file's values, as Configuration holds them.
const form = (extra = {}) => ({
  capture_mode: 'frame_sequence', resolution_width: 1920, resolution_height: 1080,
  capture_fps: 120, obs_capture_fps: 120, agr_fps: 0,
  finish_clips_after_batch: false, finish_codec_obs: 'source_copy', finish_codec_video: 'render_tab', finish_codec_frames: 'render_tab',
  record_start_lead: 5, record_stop_trail: 5, kill_gaps: { mode: 'keep', over_seconds: 8, speed: 4 },
  clip_name_template: DEFAULT_TEMPLATE, target_drives: ['E:/captures'], render_export_dirs: ['F:/clips'],
  hl_path: POST, init_commands: ['mirv_fov 90'], ...extra,
});

const block = () => ({
  picture: { width: 2560, height: 1440, captureFps: 300 },
  clipLength: { beforeSeconds: 3 },
  folders: { 'pc-a': { destinations: ['G:/takes'], exports: [] } },
  install: { madeWith: { name: 'Half-Life - POST-Anniversary for Movies', hlExe: POST } },
  futureField: { kept: true },
});

describe('startingValues', () => {
  it("takes the project's values over the app's", () => {
    const start = startingValues(form(), block(), 'pc-a');
    expect(start).toMatchObject({ resolution_width: 2560, resolution_height: 1440, capture_fps: 300, record_start_lead: 3 });
    expect(start.target_drives).toEqual(['G:/takes']);
  });

  it("falls back to the app's for anything the project doesn't set", () => {
    expect(startingValues(form(), block(), 'pc-a')).toMatchObject({ record_stop_trail: 5, capture_mode: 'frame_sequence' });
  });

  it("uses the app's folders on a PC the project has none for", () => {
    expect(startingValues(form(), block(), 'pc-b').target_drives).toEqual(['E:/captures']);
  });

  it('holds only the keys the page edits, both tabs', () => {
    expect(Object.keys(startingValues(form(), block(), 'pc-a')).sort()).toEqual([...PAGE_KEYS].sort());
    expect(PAGE_KEYS).toEqual(expect.arrayContaining([...PROJECT_TAB_KEYS, 'init_commands', 'custom_commands', 'command_profile_active']));
  });

  it('shares no array with the form, so an edit never reaches it', () => {
    const app = form();
    const start = startingValues(app, null, 'pc-a');
    start.target_drives.push('Z:/x');
    expect(app.target_drives).toEqual(['E:/captures']);
  });
});

describe('changedKeys and hasChanges', () => {
  it('finds nothing when nothing was edited', () => {
    const start = startingValues(form(), block(), 'pc-a');
    expect(hasChanges(start, structuredClone(start))).toBe(false);
  });

  it('names each setting edited, folders and objects included', () => {
    const start = startingValues(form(), block(), 'pc-a');
    const pending = { ...structuredClone(start), capture_fps: 60, target_drives: ['G:/takes', 'H:/more'],
      kill_gaps: { mode: 'fast_forward', over_seconds: 8, speed: 4 } };
    expect(changedKeys(start, pending)).toEqual(['capture_fps', 'kill_gaps', 'target_drives']);
  });

  it('counts an edit put back as no change', () => {
    const start = startingValues(form(), block(), 'pc-a');
    expect(hasChanges(start, { ...structuredClone(start), capture_fps: 300 })).toBe(false);
  });
});

describe('a rate typed badly under a mode that is then left', () => {
  it('is not a change, so Apply never writes it into the project', () => {
    const start = startingValues(form({ capture_mode: 'obs' }), null, 'pc-a');
    const pending = { ...structuredClone(start), capture_mode: 'direct_to_video', obs_capture_fps: NaN, capture_fps: 60 };
    expect(pendingProblems(pending)).toEqual({});
    expect(changedKeys(start, pending)).toEqual(['capture_mode', 'capture_fps']);
    const next = blockWithPending(block(), start, pending, 'pc-a');
    expect(next.picture.obsCaptureFps).toBeUndefined();
  });

  it('is still a change in the mode that uses it, where it blocks Apply', () => {
    const start = startingValues(form({ capture_mode: 'obs' }), null, 'pc-a');
    const pending = { ...structuredClone(start), obs_capture_fps: NaN };
    expect(changedKeys(start, pending)).toEqual(['obs_capture_fps']);
    expect(pendingProblems(pending)).toEqual({ obs_capture_fps: S.BAD_RATE });
  });
});

describe('pendingProblems and the AGR fallback', () => {
  it("refuses AGR's 0 while the Capture FPS it stands for is unusable", () => {
    const start = startingValues(form(), null, 'pc-a');
    expect(pendingProblems({ ...start, capture_mode: 'agr', agr_fps: 0, capture_fps: NaN })).toEqual({ agr_fps: S.BAD_RATE });
  });
});

describe('pendingProblems', () => {
  const ok = () => startingValues(form(), null, 'pc-a');

  it('finds none in good values', () => {
    expect(pendingProblems(ok())).toEqual({});
  });

  it('refuses a size that is not a whole number above 0', () => {
    expect(pendingProblems({ ...ok(), resolution_width: NaN })).toEqual({ resolution_width: S.BAD_SIZE });
    expect(pendingProblems({ ...ok(), resolution_height: 0 })).toEqual({ resolution_width: S.BAD_SIZE });
  });

  it('checks the rate the capture mode uses, and only that one', () => {
    expect(pendingProblems({ ...ok(), capture_fps: 0 })).toEqual({ capture_fps: S.BAD_RATE });
    expect(pendingProblems({ ...ok(), capture_mode: 'obs', capture_fps: 0, obs_capture_fps: 60 })).toEqual({});
    expect(pendingProblems({ ...ok(), capture_mode: 'obs', obs_capture_fps: 1.5 })).toEqual({ obs_capture_fps: S.BAD_RATE });
  });

  it("lets AGR's 0 stand for the same as Capture FPS", () => {
    expect(pendingProblems({ ...ok(), capture_mode: 'agr', agr_fps: 0 })).toEqual({});
    expect(pendingProblems({ ...ok(), capture_mode: 'agr', agr_fps: NaN })).toEqual({ agr_fps: S.BAD_RATE });
  });

  it('refuses negative or blank seconds', () => {
    expect(pendingProblems({ ...ok(), record_start_lead: -1, record_stop_trail: NaN }))
      .toEqual({ record_start_lead: S.BAD_SECONDS, record_stop_trail: S.BAD_SECONDS });
  });

  it('checks the gap length and speed only while gaps fast-forward', () => {
    expect(pendingProblems({ ...ok(), kill_gaps: { mode: 'fast_forward', over_seconds: 8, speed: 1 } }))
      .toHaveProperty('kill_gaps');
    expect(pendingProblems({ ...ok(), kill_gaps: { mode: 'fast_forward', over_seconds: 8, speed: 99 } }))
      .toHaveProperty('kill_gaps');
    expect(pendingProblems({ ...ok(), kill_gaps: { mode: 'keep', over_seconds: 8, speed: 4 } })).toEqual({});
  });

  it("says what is wrong with a clip-name template, in the template check's words", () => {
    const problems = pendingProblems({ ...ok(), clip_name_template: '{nope}' });
    expect(problems.clip_name_template).toBe(STRINGS.CLIP_NAME.unknownPlaceholder('{nope}'));
  });
});

describe('blockWithPending', () => {
  it('writes only what was edited, and keeps fields this build does not know', () => {
    const start = startingValues(form(), block(), 'pc-a');
    const next = blockWithPending(block(), start, { ...structuredClone(start), record_stop_trail: 2, capture_mode: 'obs' }, 'pc-a');
    expect(next.clipLength).toEqual({ beforeSeconds: 3, afterSeconds: 2 });
    expect(next.picture).toEqual({ width: 2560, height: 1440, captureFps: 300, captureMode: 'obs' });
    expect(next.futureField).toEqual({ kept: true });
  });

  it("puts folders under this PC's id only", () => {
    const start = startingValues(form(), block(), 'pc-a');
    const next = blockWithPending(block(), start, { ...structuredClone(start), render_export_dirs: ['F:/clips'] }, 'pc-a');
    expect(next.folders).toEqual({ 'pc-a': { destinations: ['G:/takes'], exports: ['F:/clips'] } });
  });

  it('leaves the block it was given as it was', () => {
    const before = block();
    const start = startingValues(form(), before, 'pc-a');
    blockWithPending(before, start, { ...structuredClone(start), capture_fps: 60 }, 'pc-a');
    expect(before).toEqual(block());
  });

  it('keeps a change made to the block meanwhile, to a setting not edited here', () => {
    const start = startingValues(form(), block(), 'pc-a');
    const meanwhile = { ...block(), clipNames: { template: '{map}' } };
    const next = blockWithPending(meanwhile, start, { ...structuredClone(start), capture_fps: 60 }, 'pc-a');
    expect(next.clipNames).toEqual({ template: '{map}' });
    expect(next.picture.captureFps).toBe(60);
  });
});

describe('the chips', () => {
  it('names the size chip a width and height match, else Custom', () => {
    expect(sizeChoiceOf(2560, 1440)).toBe('1440p');
    expect(sizeChoiceOf(2560, 1080)).toBe('custom');
  });

  it('names the rate chip, else Custom', () => {
    expect(rateChoiceOf(120)).toBe(120);
    expect(rateChoiceOf(90)).toBe('custom');
  });

  it("edits OBS's and AGR's own rates in their modes, Capture FPS in the others", () => {
    expect(rateKeyFor('obs')).toBe('obs_capture_fps');
    expect(rateKeyFor('agr')).toBe('agr_fps');
    expect(rateKeyFor('direct_to_video')).toBe('capture_fps');
  });

  it("shows AGR's 0 as the Capture FPS it stands for", () => {
    expect(rateOf({ agr_fps: 0, capture_fps: 300 }, 'agr')).toBe(300);
    expect(rateOf({ agr_fps: 60, capture_fps: 300 }, 'agr')).toBe(60);
  });

  it('has no finished-clip codec for AGR', () => {
    expect(codecKeyFor('agr')).toBe(null);
    expect(codecKeyFor('frame_sequence')).toBe('finish_codec_frames');
  });
});

describe('small edits', () => {
  it('reads a blank template as the default', () => {
    expect(templateOrDefault('  ')).toBe(DEFAULT_TEMPLATE);
    expect(templateOrDefault('{map}')).toBe('{map}');
  });

  it('keeps a typed gap value as typed while gaps fast-forward', () => {
    expect(killGapsWith({ mode: 'fast_forward', over_seconds: 8, speed: 4 }, { speed: NaN }))
      .toEqual({ mode: 'fast_forward', over_seconds: 8, speed: NaN });
  });

  it('cleans the gap value when Keep is picked, so a hidden bad value cannot stop Apply', () => {
    expect(killGapsWith({ mode: 'fast_forward', over_seconds: -2, speed: NaN }, { mode: 'keep' }))
      .toEqual({ mode: 'keep', over_seconds: 8, speed: 4 });
  });

  it('adds a folder once, whatever its case or slashes', () => {
    expect(withFolder(['E:/captures'], 'e:\\Captures\\')).toEqual(['E:/captures']);
    expect(withFolder(['E:/captures'], 'F:/more')).toEqual(['E:/captures', 'F:/more']);
    expect(withFolder(['E:/captures'], '  ')).toEqual(['E:/captures']);
  });

  it('removes the folder at an index', () => {
    expect(withoutFolder(['a', 'b', 'c'], 1)).toEqual(['a', 'c']);
  });

  it('warns about a second folder on the same drive', () => {
    expect(sameDriveLines(['E:/a', 'E:/b'])).toHaveLength(1);
    expect(sameDriveLines(['E:/a', 'F:/b'])).toEqual([]);
  });
});

describe('installState', () => {
  const madeWith = { name: 'Half-Life - POST-Anniversary for Movies', hlExe: POST };

  it('is picked when the hl.exe is the one picked', () => {
    expect(installState(madeWith, POST)).toBe('picked');
  });

  it('is picked when the install folder has the same name on another PC', () => {
    expect(installState(madeWith, 'D:/Games/Half-Life - POST-Anniversary for Movies/hl.exe')).toBe('picked');
  });

  it('is other when another install is picked', () => {
    expect(installState(madeWith, 'C:/Steam/steamapps/common/Half-Life - PRE-Anniversary for Movies/hl.exe')).toBe('other');
  });

  it('is none when the project names no install', () => {
    expect(installState(null, POST)).toBe('none');
  });
});
