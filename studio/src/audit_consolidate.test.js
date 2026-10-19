import { describe, it, expect } from 'vitest';
import {
  consolidateRequest, hasDemos, planView, consolidateProgressView, singlesAfterDelete,
} from './audit_consolidate.js';

const GROUPS = [
  {
    size: 100,
    files: [
      { path: 'C:\\HL\\dod\\m.dem', in_use: 'game_folder' },
      { path: 'F:\\dl\\m.dem', in_use: null },
    ],
  },
  { size: 5, files: [{ path: 'D:\\a.dem', in_use: 'queue' }, { path: 'E:\\a.dem', in_use: null }] },
];

const plan = (over = {}) => ({
  destination: 'F:\\Demos',
  moves: [{}, {}],
  recycle: [{}],
  renamed: 0,
  already_there: 0,
  stay_in_game_folders: 0,
  copies_in_game_folders: 0,
  move_bytes: 3 * 1024 * 1024 * 1024,
  copy_bytes: 0,
  free_bytes: 10,
  enough_space: true,
  ...over,
});

describe('audit_consolidate (#872)', () => {
  it('moves each kept copy and every single, and recycles the other copies', () => {
    const req = consolidateRequest({
      groups: GROUPS,
      singles: [{ path: 'G:\\solo.dem', size: 7, in_use: null }],
      roots: ['C:\\'],
      destination: 'F:\\Demos',
      layout: 'subfolders',
      includeGameFolders: false,
    });
    expect(req.keep).toEqual([
      { path: 'C:\\HL\\dod\\m.dem', size: 100, game_folder: true },
      { path: 'D:\\a.dem', size: 5, game_folder: false },
      { path: 'G:\\solo.dem', size: 7, game_folder: false },
    ]);
    expect(req.recycle.map((d) => d.path)).toEqual(['F:\\dl\\m.dem', 'E:\\a.dem']);
    expect(req).toMatchObject({ destination: 'F:\\Demos', layout: 'subfolders', include_game_folders: false, roots: ['C:\\'] });
    expect(consolidateRequest({ groups: [], layout: 'nonsense' }).layout).toBe('flat');
  });

  it('needs an audit with demos', () => {
    expect(hasDemos([], [])).toBe(false);
    expect(hasDemos([], [{}])).toBe(true);
    expect(hasDemos(GROUPS, undefined)).toBe(true);
  });

  it('previews what moves, recycles and renames', () => {
    expect(planView(plan({ renamed: 1, stay_in_game_folders: 2 }))).toEqual({
      text: 'Move 2 demos (3.00 GB) into F:\\Demos, recycle 1 copy, rename 1. 2 in game folders left where they are.',
      ready: true,
    });
    expect(planView(plan({ moves: [], recycle: [] })).ready).toBe(false);
    expect(planView(null).ready).toBe(false);
  });

  it('refuses to start when the copies between drives do not fit', () => {
    const view = planView(plan({ enough_space: false, copy_bytes: 2 * 1024 ** 3, free_bytes: 1024 ** 3 }));
    expect(view.ready).toBe(false);
    expect(view.text).toBe('Not enough space: the demos copied from other drives need 2.00 GB and 1.00 GB is free.');
  });

  it('measures moves in bytes and recycling in files', () => {
    const moving = consolidateProgressView({
      step: 'moving', files_done: 1, files_total: 4, bytes_done: 512 * 1024 ** 2, bytes_total: 1024 ** 3, current: 'C:\\a.dem',
    });
    expect(moving).toEqual({ pct: 50, text: 'Moving demos: 1 / 4 (512.0 MB of 1.00 GB)', current: 'C:\\a.dem' });
    expect(consolidateProgressView({ step: 'recycling', files_done: 3, files_total: 4 }).text)
      .toBe('Moving copies to the Recycle Bin: 3 / 4');
    expect(consolidateProgressView({ step: 'undoing', files_done: 0, files_total: 2, bytes_done: 0, bytes_total: 0 }).text)
      .toMatch(/^Putting demos back: 0 \/ 2/);
    expect(consolidateProgressView(null).pct).toBe(0);
  });

  it('a group a delete left with one file joins the singles', () => {
    expect(singlesAfterDelete(GROUPS, ['E:\\a.dem'])).toEqual([{ path: 'D:\\a.dem', size: 5, in_use: 'queue' }]);
    expect(singlesAfterDelete(GROUPS, [])).toEqual([]);
  });
});
