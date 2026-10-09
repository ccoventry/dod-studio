import { describe, it, expect } from 'vitest';
import { multiMapList, mapsToKeep } from './analyzer_multimap.js';

const seg = (index, start, end) => ({ index, map: 'dod_lennon2', start_seconds: start, end_seconds: end });

describe('multiMapList', () => {
  it('lists the maps of a demo that recorded two or more', () => {
    expect(multiMapList({ state: { signon_maps: ['dod_lennon2', 'dod_lennon2'] } }))
      .toEqual(['dod_lennon2', 'dod_lennon2']);
  });

  it('is null for one map, and for an older cache entry with no list', () => {
    expect(multiMapList({ state: { signon_maps: ['dod_anzio'] } })).toBeNull();
    expect(multiMapList({ state: {} })).toBeNull();
    expect(multiMapList(null)).toBeNull();
  });
});

describe('mapsToKeep', () => {
  it('leaves out a map shorter than the minimum (the next map loading as recording stopped)', () => {
    // wsod25_grp3_h1_dyelife: 21:28 of lennon2, then 14 s of the second half.
    expect(mapsToKeep([seg(0, -2, 1288), seg(1, 1288, 1302)], 60)).toEqual([0]);
  });

  it('keeps every map that is long enough, in order', () => {
    expect(mapsToKeep([seg(0, 0, 1200), seg(1, 1200, 2400), seg(2, 2400, 2410)], 60)).toEqual([0, 1]);
  });

  it('keeps everything when every map is short', () => {
    expect(mapsToKeep([seg(0, 0, 20), seg(1, 20, 30)], 60)).toEqual([0, 1]);
  });
});
