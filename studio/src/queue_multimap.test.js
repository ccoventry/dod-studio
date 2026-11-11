import { describe, it, expect, vi } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({ demoSplitStatus: vi.fn() }));
vi.mock('./split_pane.js', () => ({ SHORT_MAP_SECONDS: 60 }));

const { alreadySplitNoteTitle, isMultiMapDemo, pickedMultiMapDemos } = await import('./queue_multimap.js');

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

describe('alreadySplitNoteTitle', () => {
  const twoMaps = demo(['dod_armory', 'dod_anzio']);

  it('names the split files once the maps were split next to the demo', () => {
    const title = alreadySplitNoteTitle(twoMaps, { split: true, files: ['a_split1_armory.dem', 'a_split2_anzio.dem'] });
    expect(title).toContain('recorded 2 maps (dod_armory, dod_anzio)');
    expect(title).toContain('a_split1_armory.dem, a_split2_anzio.dem');
    expect(title).toContain('uses the first map');
  });

  it('is null (the Split button stays) when not split, partly split, or not known yet', () => {
    expect(alreadySplitNoteTitle(twoMaps, { split: false, files: [] })).toBeNull();
    expect(alreadySplitNoteTitle(twoMaps, { split: false, files: ['a_split1_armory.dem'] })).toBeNull();
    expect(alreadySplitNoteTitle(twoMaps, null)).toBeNull();
  });
});
