import { describe, it, expect } from 'vitest';
import { fromCacheText, timeLeftText, parseClock, cacheRunView } from './scan_progress.js';

describe('fromCacheText', () => {
  it('names the cached demos only when there are some', () => {
    expect(fromCacheText(0)).toBe('');
    expect(fromCacheText(5)).toBe(' (5 from the analyzer cache)');
  });
});

describe('timeLeftText', () => {
  it('says nothing until a parse has finished to measure the pace by', () => {
    expect(timeLeftText({ total: 10, cached: 4, parsed: 0 }, 5000)).toBe('');
    expect(timeLeftText({ total: 10, cached: 4, parsed: 2 }, 0)).toBe('');
  });

  it('says nothing once every demo is done', () => {
    expect(timeLeftText({ total: 10, cached: 4, parsed: 6 }, 9000)).toBe('');
  });

  it('goes by bytes when the event has them', () => {
    // 100 of 400 bytes in 10 s: 30 s for the other 300.
    expect(timeLeftText({ total: 10, cached: 4, parsed: 1, bytes_to_parse: 400, bytes_parsed: 100 }, 10000))
      .toBe(' · about 30 s left');
  });

  it('goes by the demo count without bytes, in minutes past 90 s', () => {
    expect(timeLeftText({ total: 10, cached: 0, parsed: 2 }, 60000)).toBe(' · about 4 min left');
  });
});

describe('parseClock', () => {
  it('starts once the cached demos are done, and starts again after a reset', () => {
    let now = 1000;
    const clock = parseClock(() => now);
    expect(clock.elapsed(2, 5)).toBe(0);
    now = 1500;
    expect(clock.elapsed(5, 5)).toBe(0);
    now = 4500;
    expect(clock.elapsed(6, 5)).toBe(3000);
    clock.reset();
    expect(clock.elapsed(0, 5)).toBe(0);
  });

  it('starts at once when the cache has none of the demos', () => {
    let now = 0;
    const clock = parseClock(() => now);
    expect(clock.elapsed(0, 0)).toBe(0);
    now = 800;
    expect(clock.elapsed(1, 0)).toBe(800);
  });
});

describe('cacheRunView', () => {
  const upgrading = { done: 0, total: 1486, cached: 0, upgrade: { running: true, done: 812, total: 1486, upgraded: 800 } };

  it('says it is upgrading, with the count and a bar of it', () => {
    const clock = parseClock(() => 1000);
    expect(cacheRunView(upgrading, clock)).toEqual({
      text: 'Upgrading cached analyses: 812 of 1,486',
      value: 812,
      max: 1486,
    });
  });

  it('does not start the parse clock while upgrading', () => {
    let now = 0;
    const clock = parseClock(() => now);
    cacheRunView(upgrading, clock);
    now = 5000;
    // The first parse report: the clock starts here, not at the upgrade.
    expect(clock.elapsed(0, 0)).toBe(0);
  });

  it('then counts the caching, the upgraded apart from the ones cached before', () => {
    const view = cacheRunView({
      done: 900, total: 1486, cached: 1300, already: 900, failed: 0,
      upgrade: { running: false, done: 1486, total: 1486, upgraded: 800 },
    }, parseClock(() => 0));
    expect(view.text).toBe('Caching 900 / 1486 (800 upgraded, 100 already cached)');
    expect([view.value, view.max]).toEqual([900, 1486]);
  });
});
