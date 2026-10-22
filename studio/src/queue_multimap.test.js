import { describe, it, expect } from 'vitest';
import { isMultiMapDemo, pickedMultiMapDemos } from './queue_multimap.js';

const demo = (maps, selected = []) => ({
  path: 'C:/demos/a.dem',
  signon_maps: maps,
  streaks: selected.map((s) => ({ selected: s })),
});

describe('isMultiMapDemo', () => {
  it('is true from two maps, the same map twice included', () => {
    expect(isMultiMapDemo(demo(['dod_lennon2', 'dod_lennon2']))).toBe(true);
    expect(isMultiMapDemo(demo(['dod_anzio', 'dod_avalanche', 'dod_flash']))).toBe(true);
  });

  it('is false for one map, and for a demo saved before the map list existed', () => {
    expect(isMultiMapDemo(demo(['dod_anzio']))).toBe(false);
    expect(isMultiMapDemo({ path: 'C:/demos/old.dem', streaks: [] })).toBe(false);
    expect(isMultiMapDemo(null)).toBe(false);
  });
});

describe('pickedMultiMapDemos', () => {
  it('lists only multi-map demos with a highlight picked', () => {
    const picked = demo(['dod_lennon2', 'dod_lennon2'], [false, true]);
    const unpicked = demo(['dod_lennon2', 'dod_lennon2'], [false]);
    const oneMap = demo(['dod_anzio'], [true]);
    expect(pickedMultiMapDemos([picked, unpicked, oneMap])).toEqual([picked]);
  });
});
