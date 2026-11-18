import { describe, it, expect } from 'vitest';
import { recordRender, relocateRender, rendersOf } from './take_renders.js';
import {
  movesFromRecord, movesFromBackup, mergeMoves,
  relinkMap, moveCounts, relinkDefaultPath,
} from './vegas_relink_map.js';

const render = (started, path, extra = {}) => ({
  started_unix_ms: started, stream: 'all', codec: 'prores', fps: 60, outcome: 'finished', output_path: path, ...extra,
});

/** Moves `takeKey`'s render started at `started` to `to`, as Find Moved Videos does. */
function move(index, takeKey, started, to) {
  const known = rendersOf(index, takeKey).find((r) => r.started_unix_ms === started);
  relocateRender(index, takeKey, known, { output_path: to });
}

const fromRecord = (index) => mergeMoves(index, movesFromRecord(index));

describe('the relink list from the moves Studio kept', () => {
  it('maps every old path of every render to where it is now', () => {
    const index = { a: ['u1'], b: ['u2'] };
    recordRender(index, 'a', render(1, 'E:\\Unsorted\\a.mov'));
    recordRender(index, 'b', render(2, 'E:\\Unsorted\\b.mov'));
    move(index, 'a', 1, 'F:\\sorted\\a.mov');
    move(index, 'a', 1, 'H:\\Movie\\a.mov');
    expect(fromRecord(index)).toEqual([
      { from: 'E:\\Unsorted\\a.mov', to: 'H:\\Movie\\a.mov', why: 'moved' },
      { from: 'F:\\sorted\\a.mov', to: 'H:\\Movie\\a.mov', why: 'moved' },
    ]);
  });

  it('is empty for a project whose videos never moved, or never rendered', () => {
    const index = { a: ['u1'] };
    expect(fromRecord(index)).toEqual([]);
    recordRender(index, 'a', render(1, 'E:/a.mov'));
    expect(fromRecord(index)).toEqual([]);
    expect(fromRecord(undefined)).toEqual([]);
  });

  it('leaves out a path a later render was saved to after the video there moved away', () => {
    const index = { a: ['u1'] };
    recordRender(index, 'a', render(1, 'E:/clip.mov'));
    move(index, 'a', 1, 'H:/movie/clip.mov');
    // A render made afterwards wrote the same name again: E:/clip.mov is that render now.
    recordRender(index, 'a', render(5, 'E:/clip.mov'));
    expect(fromRecord(index)).toEqual([]);
  });

  it('keeps the move when the render still claiming the old path is an older one it overwrote', () => {
    const index = { a: ['u1'] };
    // The old render's file was overwritten by the newer render (Find Moved
    // Videos calls it superseded and leaves its record), which then moved.
    recordRender(index, 'a', render(1, 'E:/clip.mov'));
    recordRender(index, 'a', render(5, 'E:/clip.mov'));
    move(index, 'a', 5, 'H:/movie/clip.mov');
    expect(fromRecord(index)).toEqual([{ from: 'E:/clip.mov', to: 'H:/movie/clip.mov', why: 'moved' }]);
  });
});

describe('the relink list from a project backup', () => {
  const backup = (takeRenders) => JSON.stringify({ version: '0.12.0', takeIndex: {}, takeRenders });

  it('matches renders by take key, start and stream and maps backup path to path now', () => {
    const index = { a: ['u1'], b: ['u2'] };
    recordRender(index, 'a', render(1, 'H:/movie/a.mov'));
    recordRender(index, 'b', render(2, 'E:/b.mov'));
    recordRender(index, 'b', render(2, 'E:/b_hud.mov', { stream: 'hud:hudcolor' }));
    const { moves, matched, unmatched } = movesFromBackup(index, backup({
      a: [render(1, 'E:/Unsorted/a.mov')],
      b: [render(2, 'E:/b.mov'), render(2, 'D:/old/b_hud.mov', { stream: 'hud:hudcolor' })],
      gone: [render(9, 'E:/gone.mov')],
    }));
    expect({ matched, unmatched }).toEqual({ matched: 3, unmatched: 1 });
    expect(mergeMoves(index, moves)).toEqual([
      { from: 'E:/Unsorted/a.mov', to: 'H:/movie/a.mov', why: 'moved' },
      { from: 'D:/old/b_hud.mov', to: 'E:/b_hud.mov', why: 'moved' },
    ]);
  });

  it('adds to the moves Studio kept, once per old path', () => {
    const index = { a: ['u1'] };
    recordRender(index, 'a', render(1, 'E:/a.mov'));
    move(index, 'a', 1, 'H:/a.mov');
    const { moves } = movesFromBackup(index, backup({ a: [render(1, 'e:\\A.MOV')] }));
    expect(mergeMoves(index, [...movesFromRecord(index), ...moves])).toEqual([
      { from: 'E:/a.mov', to: 'H:/a.mov', why: 'moved' },
    ]);
  });

  it('reads an older project file format, and refuses text that is not a project', () => {
    const index = { a: ['u1'] };
    recordRender(index, 'a', render(1, 'H:/a.mov'));
    // No schema: the oldest format; migrations run on it as on an open.
    expect(movesFromBackup(index, JSON.stringify({ takeRenders: { a: [render(1, 'E:/a.mov')] } })).matched).toBe(1);
    expect(() => movesFromBackup(index, 'not json')).toThrow();
    expect(() => movesFromBackup(index, '[1,2]')).toThrow('not a project file');
    expect(movesFromBackup(index, backup(undefined)).moves).toEqual([]);
  });
});

describe('the file', () => {
  it('has the format, version and moves, and counts the moves', () => {
    const moves = [{ from: 'a', to: 'b', why: 'moved' }, { from: 'c', to: 'd', why: 'moved' }];
    expect(relinkMap(moves)).toEqual({ format: 'dodstudio-relink-map', version: 1, moves });
    expect(moveCounts(moves)).toEqual({ moved: 2 });
  });

  it('opens the save dialog in the folder holding most of the videos now', () => {
    const index = { a: ['u1'], b: ['u2'], c: ['u3'] };
    recordRender(index, 'a', render(1, 'H:\\Movie\\a.mov'));
    recordRender(index, 'b', render(2, 'H:\\Movie\\b.mov'));
    recordRender(index, 'c', render(3, 'E:\\c.mov'));
    expect(relinkDefaultPath(index)).toBe('H:\\Movie\\dod_highlights.relink.json');
    expect(relinkDefaultPath({})).toBe('dod_highlights.relink.json');
  });
});
