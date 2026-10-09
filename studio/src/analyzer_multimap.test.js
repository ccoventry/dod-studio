import { describe, it, expect } from 'vitest';
import { multiMapList } from './analyzer_multimap.js';

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

// Which maps a split keeps is native::demo_split::keep_at_least, tested there.
