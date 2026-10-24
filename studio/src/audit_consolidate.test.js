import { describe, it, expect } from 'vitest';
import {
  consolidateRequest, hasDemos, planView, consolidateProgressView, singlesAfterDelete,
  recycledFollowPairs,
} from './audit_consolidate.js';
import { STRINGS } from './strings.js';

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
    // Each copy carries the kept copy the backend compares it with.
    expect(req.recycle.map((d) => [d.path, d.keep])).toEqual([
      ['F:\\dl\\m.dem', 'C:\\HL\\dod\\m.dem'],
      ['E:\\a.dem', 'D:\\a.dem'],
    ]);
    expect(req).toMatchObject({ destination: 'F:\\Demos', layout: 'subfolders', include_game_folders: false, roots: ['C:\\'] });
    expect(consolidateRequest({ groups: [], layout: 'nonsense' }).layout).toBe('flat');
  });

  it('never sends a row marked not an exact copy to be recycled', () => {
    const marked = [{ ...GROUPS[0], files: [...GROUPS[0].files, { path: 'H:\\m.dem', in_use: null }], notCopies: ['F:\\dl\\m.dem'] }];
    const req = consolidateRequest({ groups: marked, destination: 'F:\\Demos' });
    expect(req.recycle.map((d) => d.path)).toEqual(['H:\\m.dem']);
    expect(req.keep.map((d) => d.path)).toEqual(['C:\\HL\\dod\\m.dem']);
  });

  it('says which copies stayed because they differ from the demo kept', () => {
    const outcome = {
      moved: [{}], failed: [], recycled: [{}], cancelled: false,
      recycle_failed: [{ path: 'E:\\a.dem', error: 'x', not_a_copy: true }, { path: 'F:\\b.dem', error: 'y', not_a_copy: false }],
    };
    expect(STRINGS.AUDITOR.consolidatedStatus(outcome, 0))
      .toBe('Moved 1 demo, recycled 1 copy. 1 copy was kept: not an exact copy of the demo kept. 1 failed.');
  });

  it('needs an audit with demos', () => {
    expect(hasDemos([], [])).toBe(false);
    expect(hasDemos([], [{}])).toBe(true);
    expect(hasDemos(GROUPS, undefined)).toBe(true);
  });

  it('previews what moves, recycles and renames', () => {
    expect(planView(plan({ renamed: 1, stay_in_game_folders: 2 }))).toEqual({
      text: 'Move 2 demos (3.00 GB) into F:\\Demos, recycle 1 copy (each first checked byte for byte against the demo kept), rename 1. 2 in game folders left where they are.',
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
    expect(consolidateProgressView({ step: 'checking', files_done: 1, files_total: 4, current: 'E:\\a.dem' }))
      .toEqual({ pct: 25, text: 'Checking each copy matches the kept one byte for byte: 1 / 4', current: 'E:\\a.dem' });
    expect(consolidateProgressView({ step: 'undoing', files_done: 0, files_total: 2, bytes_done: 0, bytes_total: 0 }).text)
      .toMatch(/^Putting demos back: 0 \/ 2/);
    expect(consolidateProgressView(null).pct).toBe(0);
  });

  it('a group a delete left with one file joins the singles', () => {
    expect(singlesAfterDelete(GROUPS, ['E:\\a.dem'])).toEqual([{ path: 'D:\\a.dem', size: 5, in_use: 'queue' }]);
    expect(singlesAfterDelete(GROUPS, [])).toEqual([]);
  });

  it('a recycled copy points at its kept demo where that is now, so a queued row on it follows', () => {
    const req = {
      recycle: [
        { path: '\\\\?\\E:\\a.dem', keep: '\\\\?\\D:\\a.dem' },
        { path: 'F:\\dl\\m.dem', keep: 'C:\\HL\\dod\\m.dem' },
        { path: 'G:\\kept.dem', keep: 'D:\\x.dem' },
      ],
    };
    const outcome = {
      moved: [{ from: 'D:\\a.dem', to: 'H:\\All\\a.dem' }],
      recycled: ['\\\\?\\E:\\a.dem', 'f:\\DL\\m.dem'],
    };
    expect(recycledFollowPairs(req, outcome)).toEqual([
      { from: 'E:\\a.dem', to: 'H:\\All\\a.dem' },
      // A kept demo that stayed (a game folder) is where the row goes.
      { from: 'F:\\dl\\m.dem', to: 'C:\\HL\\dod\\m.dem' },
    ]);
    expect(recycledFollowPairs(req, { moved: [], recycled: [] })).toEqual([]);
  });
});
