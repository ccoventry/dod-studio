import { describe, it, expect, vi } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({ demoSplitStatus: vi.fn(), demoRepairable: vi.fn(), logFrontendEvent: vi.fn() }));
vi.mock('./split_pane.js', () => ({ SHORT_MAP_SECONDS: 60 }));

const { multiMapList, multiMapNotice } = await import('./analyzer_multimap.js');

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

describe('multiMapNotice', () => {
  const maps = ['dod_armory', 'dod_anzio'];

  it('says only the first map is used, and to split for the rest, until it is split', () => {
    const notice = multiMapNotice(maps, { split: false, files: [] });
    expect(notice.quiet).toBe(false);
    expect(notice.text).toContain('Only the first is analysed, and highlights and captures use it too.');
    expect(notice.text).toContain('Split it to use the rest.');
    expect(multiMapNotice(maps, null).quiet).toBe(false);
  });

  it('is a quiet line naming the split files once the maps were split next to it', () => {
    const notice = multiMapNotice(maps, { split: true, files: ['a_split1_armory.dem', 'a_split2_anzio.dem'] });
    expect(notice.quiet).toBe(true);
    expect(notice.text).toBe('This demo recorded 2 maps (dod_armory, dod_anzio) and was already split. This report covers the first.');
    expect(notice.title).toBe('Split next to it into: a_split1_armory.dem, a_split2_anzio.dem');
    expect(notice.text).not.toContain('Split it');
  });
});

// Which maps a split keeps is native::demo_split::keep_at_least, tested there.
