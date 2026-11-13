import { describe, it, expect } from 'vitest';
import { fileChanges, focusRecheckDue, FOCUS_RECHECK_GAP_MS } from './demo_file_recheck_logic.js';

const demo = (path, missing = false) => ({ path, missing });
const checkedOf = (demos) => demos.map((d) => ({ demo: d, path: d.path }));

describe('fileChanges', () => {
  it('a file that was there and is gone is gone', () => {
    const a = demo('C:/d/a.dem');
    expect(fileChanges(checkedOf([a]), [false])).toEqual({ gone: [a], back: [] });
  });

  it('a missing demo whose file is there again is back', () => {
    const a = demo('C:/d/a.dem', true);
    expect(fileChanges(checkedOf([a]), [true])).toEqual({ gone: [], back: [a] });
  });

  it('a demo already missing and still gone is no change', () => {
    expect(fileChanges(checkedOf([demo('C:/d/a.dem', true)]), [false])).toEqual({ gone: [], back: [] });
  });

  it('a demo there and still there is no change', () => {
    expect(fileChanges(checkedOf([demo('C:/d/a.dem')]), [true])).toEqual({ gone: [], back: [] });
  });

  it('a row pointed at another file while the look ran is left alone', () => {
    const a = demo('C:/d/a.dem', true);
    const checked = checkedOf([a]);
    a.path = 'C:/d/a_renamed.dem';
    expect(fileChanges(checked, [false])).toEqual({ gone: [], back: [] });
  });

  it('a row with no answer is left alone', () => {
    const a = demo('C:/d/a.dem');
    expect(fileChanges(checkedOf([a]), [])).toEqual({ gone: [], back: [] });
  });

  it('sorts a mixed queue row by row', () => {
    const a = demo('C:/d/a.dem');
    const b = demo('C:/d/b.dem', true);
    const c = demo('C:/d/c.dem');
    expect(fileChanges(checkedOf([a, b, c]), [false, true, true])).toEqual({ gone: [a], back: [b] });
  });
});

describe('focusRecheckDue', () => {
  it('the first focus always looks', () => {
    expect(focusRecheckDue(null, 0)).toBe(true);
  });

  it('a focus inside the gap does not look again', () => {
    expect(focusRecheckDue(1000, 1000 + FOCUS_RECHECK_GAP_MS - 1)).toBe(false);
  });

  it('a focus once the gap has passed looks again', () => {
    expect(focusRecheckDue(1000, 1000 + FOCUS_RECHECK_GAP_MS)).toBe(true);
  });
});
