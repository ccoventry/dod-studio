import { describe, it, expect } from 'vitest';
import { splitProgressView } from './split_progress.js';

describe('splitProgressView', () => {
  it('reads as reading the demo before any map is written', () => {
    expect(splitProgressView({ fraction: 0.2, stage: 'reading', map: '', part: 0, parts: 0 }))
      .toEqual({ pct: 20, text: 'Reading the demo… 20%' });
  });

  it('names the map and which of the maps while writing and checking', () => {
    expect(splitProgressView({ fraction: 0.4, stage: 'writing', map: 'dod_anzio', part: 1, parts: 2 }).text)
      .toBe('Writing dod_anzio (1 of 2)… 40%');
    expect(splitProgressView({ fraction: 0.655, stage: 'checking', map: 'dod_anzio', part: 1, parts: 2 }).text)
      .toBe('Checking dod_anzio (1 of 2)… 66%');
  });

  it('leaves out "1 of 1" for a single map', () => {
    expect(splitProgressView({ fraction: 0.5, stage: 'writing', map: 'dod_lennon2', part: 1, parts: 1 }).text)
      .toBe('Writing dod_lennon2… 50%');
  });

  it('starts at 0% reading when there is nothing yet, and stays within 0-100', () => {
    expect(splitProgressView(null)).toEqual({ pct: 0, text: 'Reading the demo… 0%' });
    expect(splitProgressView({ fraction: 1.2, stage: 'checking', map: 'x', part: 1, parts: 1 }).pct).toBe(100);
  });
});
