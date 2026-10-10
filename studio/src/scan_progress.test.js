import { describe, it, expect } from 'vitest';
import { fromCacheText, timeLeftText, parseClock } from './scan_progress.js';

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
