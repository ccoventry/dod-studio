import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({ estimateRescan: vi.fn(), logFrontendEvent: vi.fn() }));

import {
  DEFAULT_SECS_PER_FULL_READ, estimateSeconds, formatDuration, rescanTimeLine, learnRate,
  learnFromScan, getScanReadRate, scanRateSettings, applyScanRateSettings, createRescanEstimate,
} from './rescan_estimate.js';

const MB = 1e6;

describe('the rescan time line', () => {
  it('rounds: seconds under a minute, minutes under an hour, then hours', () => {
    expect(formatDuration(0)).toBe('1 s');
    expect(formatDuration(0.2)).toBe('1 s');
    expect(formatDuration(2.4)).toBe('3 s');
    expect(formatDuration(59)).toBe('59 s');
    expect(formatDuration(60)).toBe('1 min');
    expect(formatDuration(240)).toBe('4 min');
    expect(formatDuration(59 * 60)).toBe('59 min');
    expect(formatDuration(60 * 60)).toBe('an hour');
    expect(formatDuration(85 * 60)).toBe('an hour');
    expect(formatDuration(150 * 60)).toBe('3 h');
  });

  it('cached and upgradable demos take milliseconds each: 158 is "About 3 s."', () => {
    expect(rescanTimeLine({ cached: 150, upgradable: 8, to_parse: 0, bytes_to_parse: 0 })).toBe('About 3 s.');
    expect(rescanTimeLine({ cached: 1, upgradable: 0, to_parse: 0, bytes_to_parse: 0 })).toBe('About 1 s.');
  });

  it('full reads go at the default 3 s each until a rate is measured', () => {
    const estimate = { cached: 0, upgradable: 0, to_parse: 80, bytes_to_parse: 4000 * MB };
    expect(estimateSeconds(estimate)).toBe(80 * DEFAULT_SECS_PER_FULL_READ);
    expect(rescanTimeLine(estimate)).toBe('About 4 min: 80 need a full read.');
    expect(rescanTimeLine({ to_parse: 1, bytes_to_parse: 50 * MB })).toBe('About 3 s: 1 needs a full read.');
  });

  it('and at the measured bytes per second after', () => {
    // 120 demos, 6 GB at 25 MB/s: 240 s, plus 38 cached.
    const estimate = { cached: 38, upgradable: 0, to_parse: 120, bytes_to_parse: 6000 * MB };
    expect(estimateSeconds(estimate, 25 * MB)).toBeCloseTo(240 + 38 * 0.015);
    expect(rescanTimeLine(estimate, 25 * MB)).toBe('About 4 min: 120 need a full read.');
    expect(rescanTimeLine({ to_parse: 400, bytes_to_parse: 20000 * MB }, 4 * MB)).toBe('About an hour: 400 need a full read.');
  });
});

describe('learning the read rate from scans', () => {
  const scan = (bytes, ms, extra = {}) => ({ full_reads: 1, full_read_bytes: bytes, not_cached_ms: ms, ...extra });

  it('the first measurement replaces the default', () => {
    expect(learnRate(0, scan(1400 * MB, 10_000))).toBe(140 * MB);
  });

  it('a later one moves it by its bytes, half way at most', () => {
    // 50 MB is 5% of the full weight: one small demo barely moves it.
    expect(learnRate(100 * MB, scan(50 * MB, 5_000))).toBeCloseTo(100 * MB + 0.05 * (10 * MB - 100 * MB));
    // A big scan moves it half way.
    expect(learnRate(100 * MB, scan(5000 * MB, 100_000))).toBe(75 * MB);
  });

  it('a cancelled scan, or one with no full reads, leaves it', () => {
    expect(learnRate(100 * MB, scan(500 * MB, 1_000, { cancelled: true }))).toBe(100 * MB);
    expect(learnRate(100 * MB, scan(0, 1_000))).toBe(100 * MB);
    expect(learnRate(100 * MB, scan(500 * MB, 0))).toBe(100 * MB);
    expect(learnRate(100 * MB, null)).toBe(100 * MB);
    // A scan from before #937's fields.
    expect(learnRate(100 * MB, { cached: 3, not_cached: 2, not_cached_ms: 900 })).toBe(100 * MB);
  });

  it('is kept in the settings file and logged when it changes', () => {
    applyScanRateSettings({});
    expect(getScanReadRate()).toBe(0);
    expect(scanRateSettings()).toEqual({ scan_read_bytes_per_sec: 0 });
    expect(learnFromScan(scan(0, 0))).toBeNull();
    expect(learnFromScan(scan(1400 * MB, 10_000))).toBe(
      "[scan] full reads went at 140 MB/s; this PC's read rate for rescan estimates is now 140 MB/s",
    );
    expect(scanRateSettings()).toEqual({ scan_read_bytes_per_sec: 140 * MB });
    applyScanRateSettings({ scan_read_bytes_per_sec: 'fast' });
    expect(getScanReadRate()).toBe(0);
    applyScanRateSettings({ scan_read_bytes_per_sec: 30 * MB });
    expect(getScanReadRate()).toBe(30 * MB);
  });
});

describe('createRescanEstimate', () => {
  beforeEach(() => applyScanRateSettings({}));

  it('turns the backend estimate into the line', async () => {
    const estimate = vi.fn(async () => ({ cached: 0, upgradable: 0, to_parse: 2, bytes_to_parse: 100 * MB }));
    const { timeLine } = createRescanEstimate({ estimate, log: vi.fn() });
    expect(await timeLine(['a.dem', 'b.dem'])).toBe('About 6 s: 2 need a full read.');
    expect(estimate).toHaveBeenCalledWith(['a.dem', 'b.dem']);
  });

  it('a failed estimate is no line, and logged', async () => {
    const log = vi.fn();
    const { timeLine } = createRescanEstimate({ estimate: async () => { throw new Error('no cache'); }, log });
    expect(await timeLine(['a.dem'])).toBeNull();
    expect(log).toHaveBeenCalledWith('[rescan] could not work out how long the rescan takes: Error: no cache');
  });
});
