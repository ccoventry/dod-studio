import { describe, it, expect, vi } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({ demoSplitStatus: vi.fn() }));
vi.mock('./split_pane.js', () => ({ SHORT_MAP_SECONDS: 60 }));

const { createSplitStatusCheck, isAlreadySplit, RECHECK_AFTER_MS } = await import('./split_status.js');

const SPLIT = { split: true, files: ['a_split1_armory.dem', 'a_split2_anzio.dem'] };
const NOT_SPLIT = { split: false, files: [] };
const MAPS = ['dod_armory', 'dod_anzio'];

/** A checker over `answers` (one per call, in order) and a clock the test moves. */
function checker(...answers) {
  const clock = { ms: 0 };
  const ask = vi.fn(() => {
    const next = answers.shift();
    return next instanceof Error ? Promise.reject(next) : Promise.resolve(next);
  });
  return { clock, ask, status: createSplitStatusCheck({ ask, now: () => clock.ms }) };
}

describe('createSplitStatusCheck', () => {
  it('knows nothing about a demo before its first check', () => {
    const { status } = checker(SPLIT);
    expect(status.last('C:/demos/a.dem')).toBeNull();
  });

  it('asks the backend with the demo and its maps, and remembers the answer', async () => {
    const { ask, status } = checker(SPLIT);
    expect(await status.check('C:/demos/a.dem', MAPS)).toEqual(SPLIT);
    expect(ask).toHaveBeenCalledWith('C:/demos/a.dem', MAPS);
    expect(status.last('C:/demos/a.dem')).toEqual(SPLIT);
  });

  it('reuses a fresh answer, so a queue redraw does not ask again', async () => {
    const { ask, clock, status } = checker(SPLIT);
    await status.check('C:/demos/a.dem', MAPS);
    clock.ms = RECHECK_AFTER_MS - 1;
    expect(await status.check('C:/demos/a.dem', MAPS)).toEqual(SPLIT);
    expect(ask).toHaveBeenCalledTimes(1);
  });

  it('asks again once the answer is old: split pieces deleted since bring the hint back', async () => {
    const { clock, status } = checker(SPLIT, NOT_SPLIT);
    await status.check('C:/demos/a.dem', MAPS);
    clock.ms = RECHECK_AFTER_MS;
    expect(await status.check('C:/demos/a.dem', MAPS)).toEqual(NOT_SPLIT);
    expect(status.last('C:/demos/a.dem')).toEqual(NOT_SPLIT);
  });

  it('shares one call between two checks of the same demo at once', async () => {
    const { ask, status } = checker(SPLIT);
    const [one, two] = await Promise.all([status.check('C:/demos/a.dem', MAPS), status.check('C:/demos/a.dem', MAPS)]);
    expect(one).toEqual(SPLIT);
    expect(two).toEqual(SPLIT);
    expect(ask).toHaveBeenCalledTimes(1);
  });

  it('answers null when the backend fails, and asks again next time', async () => {
    const { ask, status } = checker(new Error('could not read the demo'), SPLIT);
    expect(await status.check('C:/demos/a.dem', MAPS)).toBeNull();
    expect(status.last('C:/demos/a.dem')).toBeNull();
    expect(await status.check('C:/demos/a.dem', MAPS)).toEqual(SPLIT);
    expect(ask).toHaveBeenCalledTimes(2);
  });
});

describe('isAlreadySplit', () => {
  it('is true only for an answer that says split', () => {
    expect(isAlreadySplit(SPLIT)).toBe(true);
    expect(isAlreadySplit(NOT_SPLIT)).toBe(false);
    expect(isAlreadySplit({ split: false, files: ['a_split1_armory.dem'] })).toBe(false);
    expect(isAlreadySplit(null)).toBe(false);
  });
});
