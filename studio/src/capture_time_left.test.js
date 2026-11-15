import { describe, it, expect } from 'vitest';
import { timeLeftSuffix } from './capture_time_left.js';

describe('timeLeftSuffix', () => {
  it('says nothing without an estimate', () => {
    expect(timeLeftSuffix(null, 0)).toBe('');
    expect(timeLeftSuffix({ seconds: NaN, at: 0 }, 0)).toBe('');
  });

  it('rounds to minutes', () => {
    expect(timeLeftSuffix({ seconds: 14 * 60 + 20, at: 0 }, 0)).toBe(' · about 14 min left');
    expect(timeLeftSuffix({ seconds: 14 * 60 + 40, at: 0 }, 0)).toBe(' · about 15 min left');
  });

  it('counts down between reports', () => {
    expect(timeLeftSuffix({ seconds: 600, at: 1000 }, 1000 + 300 * 1000)).toBe(' · about 5 min left');
  });

  it('says under a minute near the end, and never goes below it', () => {
    expect(timeLeftSuffix({ seconds: 45, at: 0 }, 0)).toBe(' · under a minute left');
    expect(timeLeftSuffix({ seconds: 45, at: 0 }, 10 * 60 * 1000)).toBe(' · under a minute left');
  });
});
