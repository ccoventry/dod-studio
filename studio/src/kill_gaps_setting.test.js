import { describe, it, expect } from 'vitest';
import { killGapsFrom, estimateKillGaps, KILL_GAPS_DEFAULTS } from './kill_gaps_setting.js';

describe('killGapsFrom', () => {
  it('starts with gaps kept, over 8 s at 4x', () => {
    expect(killGapsFrom(undefined)).toEqual({ mode: 'keep', over_seconds: 8, speed: 4 });
    expect(KILL_GAPS_DEFAULTS.mode).toBe('keep');
  });

  it('keeps usable values and reads numbers typed as text', () => {
    expect(killGapsFrom({ mode: 'fast_forward', over_seconds: '12', speed: '6' }))
      .toEqual({ mode: 'fast_forward', over_seconds: 12, speed: 6 });
  });

  it('puts back the default for an unusable field', () => {
    expect(killGapsFrom({ mode: 'cut', over_seconds: 0, speed: 1 }))
      .toEqual({ mode: 'keep', over_seconds: 8, speed: 4 });
    expect(killGapsFrom({ over_seconds: 'lots', speed: NaN }).over_seconds).toBe(8);
  });

  it('clamps the speed to Review\'s 16x', () => {
    expect(killGapsFrom({ mode: 'fast_forward', speed: 40 }).speed).toBe(16);
  });
});

describe('estimateKillGaps', () => {
  it('bills sped-up gaps only for Fast-forward on a Seek Between Clips batch', () => {
    const ff = { mode: 'fast_forward', over_seconds: 10, speed: 4 };
    expect(estimateKillGaps(ff, true)).toEqual({ overSeconds: 10, speed: 4 });
    expect(estimateKillGaps(ff, false)).toBeNull();
    expect(estimateKillGaps({ ...ff, mode: 'keep' }, true)).toBeNull();
  });
});
