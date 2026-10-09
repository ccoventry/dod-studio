import { describe, it, expect } from 'vitest';
import { mostPerType, styleGaps, gapsSentence } from './hd_coverage.js';

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
