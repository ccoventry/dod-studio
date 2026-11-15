import { describe, it, expect } from 'vitest';
import { alreadySplitNote, damageNotice, sizeText } from './analyzer_damage.js';

describe('damageNotice', () => {
  it('says where a damaged demo stopped being read, and why', () => {
    const report = {
      demo_info: {
        damage: {
          read_up_to_secs: 125.9,
          frame_index: 7,
          byte_offset: 1000,
          bytes_unread: 15 * 1024 * 1024,
          reason: 'message length too long: 70000',
        },
      },
    };
    expect(damageNotice(report)).toBe(
      'Damaged demo: read up to 2:05. The rest of the file (15.0 MB) doesn\'t parse '
      + '(message length too long: 70000), so everything here stops at 2:05.',
    );
  });

  it('is null for a demo that read to the end, and for an older cache entry', () => {
    expect(damageNotice({ demo_info: { damage: null } })).toBeNull();
    expect(damageNotice({ demo_info: {} })).toBeNull();
    expect(damageNotice(null)).toBeNull();
  });
});

describe('alreadySplitNote', () => {
  const MAPS = ['dod_anzio', 'dod_pandemic_aim', 'dod_thunder2_b5a'];
  const SPLIT = { split: true, files: ['m1_h2_repaired_split1_anzio.dem', 'm1_h2_repaired_split3_thunder2_b5a.dem'] };

  it('says a demo damaged before its level change was repaired and split, naming the pieces', () => {
    // The analysis stops at the damage, so it knows one map; the repair
    // check's scan found the other two.
    expect(alreadySplitNote(['dod_anzio'], MAPS, SPLIT)).toEqual({
      text: 'Already repaired and split into its 3 maps (dod_anzio, dod_pandemic_aim, dod_thunder2_b5a) next to it.',
      title: 'Split next to it into: m1_h2_repaired_split1_anzio.dem, m1_h2_repaired_split3_thunder2_b5a.dem',
    });
  });

  it('adds nothing when the multi-map notice above already says it was split', () => {
    expect(alreadySplitNote(['dod_anzio', 'dod_pandemic_aim'], MAPS, SPLIT)).toBeNull();
  });

  it('adds nothing until the maps are split', () => {
    expect(alreadySplitNote(['dod_anzio'], MAPS, { split: false, files: [] })).toBeNull();
    expect(alreadySplitNote(['dod_anzio'], MAPS, null)).toBeNull();
  });
});

describe('sizeText', () => {
  it('reads in KB under a megabyte and in MB from one', () => {
    expect(sizeText(633)).toBe('1 KB');
    expect(sizeText(1500)).toBe('2 KB');
    expect(sizeText(1024 * 1024)).toBe('1.0 MB');
  });
});
