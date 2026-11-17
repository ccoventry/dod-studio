import { describe, it, expect } from 'vitest';
import { splitProgressView } from './split_progress.js';

describe('splitProgressView', () => {
  it('reads as reading the demo before any map is written', () => {
    expect(splitProgressView({ fraction: 0.2, stage: 'reading', map: '', part: 0, parts: 0 }))
      .toEqual({ pct: 20, text: 'Reading the demo… 20%' });
  });

  it('counts the maps done when several are written side by side', () => {
    expect(splitProgressView({ fraction: 0.4, stage: 'writing', map: 'dod_anzio', part: 1, parts: 2, done: 0 }).text)
      .toBe('Writing and checking 2 maps: 0 done… 40%');
    expect(splitProgressView({ fraction: 0.655, stage: 'finished', map: 'dod_anzio', part: 1, parts: 2, done: 1 }).text)
      .toBe('Writing and checking 2 maps: 1 done… 66%');
  });

  it('names a single map while writing and checking it', () => {
    expect(splitProgressView({ fraction: 0.5, stage: 'writing', map: 'dod_lennon2', part: 1, parts: 1 }).text)
      .toBe('Writing dod_lennon2… 50%');
    expect(splitProgressView({ fraction: 0.4, stage: 'waiting', map: 'dod_lennon2', part: 1, parts: 1 }).text)
      .toBe('Writing dod_lennon2… 40%');
    expect(splitProgressView({ fraction: 0.8, stage: 'checking', map: 'dod_lennon2', part: 1, parts: 1 }).text)
      .toBe('Checking dod_lennon2… 80%');
  });

  it('starts at 0% reading when there is nothing yet, and stays within 0-100', () => {
    expect(splitProgressView(null)).toEqual({ pct: 0, text: 'Reading the demo… 0%' });
    expect(splitProgressView({ fraction: 1.2, stage: 'checking', map: 'x', part: 1, parts: 1 }).pct).toBe(100);
  });
});
