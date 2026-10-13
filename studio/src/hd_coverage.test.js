import { describe, it, expect } from 'vitest';
import {
  mostPerType, styleGaps, gapsSentence, mapCoverage, coverageSentence, uncoveredChosen, mapListWarning,
} from './hd_coverage.js';

// The 2026-09-26 install from #426: ultrasharp built everywhere,
// ultrasharpv2 only for anzio's textures and skins.
const STATUS = {
  types: [
    { asset_type: 'world', folders: [
      { name: 'overrides', files: 9000, bytes: 1 },
      { name: 'ultrasharp', files: 5164, bytes: 1 },
      { name: 'ultrasharpv2', files: 124, bytes: 1 },
    ] },
    { asset_type: 'models', folders: [
      { name: 'ultrasharp', files: 1304, bytes: 1 },
      { name: 'ultrasharpv2', files: 22, bytes: 1 },
    ] },
    { asset_type: 'sprites', folders: [{ name: 'ultrasharp', files: 2219, bytes: 1 }] },
    { asset_type: 'sky', folders: [{ name: 'ultrasharp', files: 210, bytes: 1 }] },
    { asset_type: 'detail', folders: [] },
  ],
};

describe('hd_coverage (#426)', () => {
  it('measures each type against the fullest style, never overrides', () => {
    expect(mostPerType(STATUS)).toEqual({ world: 5164, models: 1304, sprites: 2219, sky: 210, detail: 0 });
  });

  it('lists what a partial style is missing, in type order', () => {
    expect(styleGaps(STATUS, 'ultrasharpv2')).toEqual([
      { asset_type: 'world', files: 124, most: 5164 },
      { asset_type: 'models', files: 22, most: 1304 },
      { asset_type: 'sprites', files: 0, most: 2219 },
      { asset_type: 'sky', files: 0, most: 210 },
    ]);
  });

  it('has nothing to say about a complete style or one never built', () => {
    expect(styleGaps(STATUS, 'ultrasharp')).toEqual([]);
    expect(styleGaps(STATUS, 'remacri')).toEqual([]);
    expect(styleGaps(null, 'ultrasharp')).toEqual([]);
    expect(gapsSentence('ultrasharp', [])).toBe('');
  });

  it('says it in one sentence', () => {
    expect(gapsSentence('ultrasharpv2', styleGaps(STATUS, 'ultrasharpv2'))).toBe(
      'ultrasharpv2 covers only part of the game: it has no sprites or skies, and only 124 of 5,164 map textures'
      + ' and 22 of 1,304 model skins. Wherever it has none, the game shows the stock texture.');
    expect(gapsSentence('x', [{ asset_type: 'world', files: 0, most: 3 }]))
      .toBe('x covers only part of the game: it has no map textures. Wherever it has none, the game shows the stock texture.');
  });
});

// native::hd::map_textures::WorldCoverage: ultrasharp built for every map,
// ultrasharpv2 for anzio only, plain half-way through kalt.
const COVERAGE = {
  styles: ['plain', 'ultrasharp', 'ultrasharpv2'],
  maps: [
    { map: 'dod_anzio', total: 124, built: { plain: 124, ultrasharp: 124, ultrasharpv2: 124 } },
    { map: 'dod_Kalt', total: 71, built: { plain: 30, ultrasharp: 71, ultrasharpv2: 0 } },
    { map: 'dod_avalanche', total: 116, built: { plain: 116, ultrasharp: 116, ultrasharpv2: 0 } },
    { map: 'dod_empty', total: 0, built: { plain: 0, ultrasharp: 0, ultrasharpv2: 0 } },
  ],
  unreadable: [],
  millis: 1,
};

describe('hd_coverage per map (#426)', () => {
  it('sorts the maps into every, some and none, leaving out maps with nothing to build', () => {
    expect(mapCoverage(COVERAGE, 'ultrasharpv2')).toEqual({
      full: ['dod_anzio'], part: [], none: ['dod_Kalt', 'dod_avalanche'],
    });
    expect(mapCoverage(COVERAGE, 'plain')).toEqual({
      full: ['dod_anzio', 'dod_avalanche'], part: [{ map: 'dod_Kalt', built: 30, total: 71 }], none: [],
    });
    expect(mapCoverage(COVERAGE, 'remacri').none).toEqual(['dod_anzio', 'dod_Kalt', 'dod_avalanche']);
    expect(mapCoverage(null, 'plain')).toEqual({ full: [], part: [], none: [] });
  });

  it('says it in one sentence', () => {
    expect(coverageSentence('ultrasharp', mapCoverage(COVERAGE, 'ultrasharp')))
      .toBe('ultrasharp has every map texture for all 3 maps.');
    expect(coverageSentence('ultrasharpv2', mapCoverage(COVERAGE, 'ultrasharpv2')))
      .toBe('ultrasharpv2: every map texture for 1 map and none for 2 maps.');
    expect(coverageSentence('plain', mapCoverage(COVERAGE, 'plain')))
      .toBe('plain: every map texture for 2 maps and some for 1 map.');
    expect(coverageSentence('remacri', mapCoverage(COVERAGE, 'remacri')))
      .toBe('remacri: no map textures for 3 maps.');
    expect(coverageSentence('x', mapCoverage(null, 'x'))).toBe('');
  });

  it('warns about the chosen maps the style has not fully built', () => {
    // Names compare without case: the list is lowercased, the files are not.
    const chosen = ['dod_kalt', 'dod_avalanche', 'dod_anzio', 'dod_nowhere'];
    const v2 = uncoveredChosen(COVERAGE, 'ultrasharpv2', chosen);
    expect(v2).toEqual([{ map: 'dod_Kalt', built: 0, total: 71 }, { map: 'dod_avalanche', built: 0, total: 116 }]);
    expect(mapListWarning('ultrasharpv2', v2, true))
      .toBe('ultrasharpv2 has no map textures for dod_Kalt and 1 more map in your map list; those maps will show stock textures.');
    expect(mapListWarning('ultrasharpv2', v2, false))
      .toBe('ultrasharpv2 has no map textures for dod_Kalt and 1 more map; those maps will show stock textures.');
    expect(mapListWarning('ultrasharpv2', v2.slice(0, 1), true))
      .toBe('ultrasharpv2 has no map textures for dod_Kalt in your map list; those maps will show stock textures.');

    const plain = uncoveredChosen(COVERAGE, 'plain', chosen);
    expect(mapListWarning('plain', plain, true)).toBe(
      'plain has only some of the map textures for dod_Kalt in your map list; those maps will show stock textures wherever it has none.');
    expect(mapListWarning('plain', uncoveredChosen(COVERAGE, 'plain', ['dod_anzio']), true)).toBe('');
    expect(mapListWarning('ultrasharp', uncoveredChosen(COVERAGE, 'ultrasharp', chosen), false)).toBe('');

    // A style with none of one map and some of another.
    const mixed = uncoveredChosen({ maps: [
      { map: 'dod_a', total: 10, built: { s: 4 } }, { map: 'dod_b', total: 10, built: {} },
    ] }, 's', ['dod_a', 'dod_b']);
    expect(mapListWarning('s', mixed, false))
      .toBe('s is missing map textures for dod_a and 1 more map; those maps will show stock textures wherever it has none.');
  });
});
