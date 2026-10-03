import { describe, it, expect } from 'vitest';
import { emptyEdits, toWorld } from './overview_draw.js';
import { fitEdits } from './overview_fit.js';

const square = (x, y, size = 10) => [[x, y], [x + size, y], [x + size, y + size], [x, y + size]];
const transform = { zoom: 1.45, origin: [20, -752, -40], rotated: false, height: 0 };
const world = (px, py) => toWorld(transform, px, py);

// Two areas side by side: the left one (0) under pixels 0-100, the right
// one (1) under 200-300; a raised piece of area 1 inside area 0.
function scene(checksum = 111) {
  return {
    map: 'dod_test',
    transform,
    checksum,
    areas: [
      { id: 0, indoor: false, colour: [1, 1, 1], anchor: world(50, 50), cells: 50 },
      { id: 1, indoor: true, colour: [2, 2, 2], anchor: world(250, 50), cells: 50 },
    ],
    faces: [
      { points: square(0, 0, 100), z: 0, area: 0, face: 7 },
      { points: square(200, 0, 100), z: 0, area: 1, face: 8 },
      { points: square(10, 10, 20), z: 64, area: 1, face: 9 },
    ],
    flags: [{ name: 'Plaza', at: [50, 50], world: [1000, 2000, 0] }],
  };
}

const edits = (patch) => ({ ...emptyEdits(), ...patch });

describe('fitEdits', () => {
  it('keeps edits that still fit as they are', () => {
    const e = edits({ areas: [{ at: world(250, 50), colour: [9, 9, 9] }], mapChecksum: 111 });
    const { edits: out, unplaced } = fitEdits(scene(), e);
    expect(out.areas).toEqual([{ at: world(250, 50), colour: [9, 9, 9] }]);
    expect(unplaced).toEqual({ areas: 0, faces: 0, flagNames: 0 });
  });

  it('moves an area colour to the area now under its point', () => {
    // The area was reshaped: its old anchor is elsewhere inside it.
    const e = edits({ areas: [{ at: world(280, 80), colour: [9, 9, 9] }] });
    const { edits: out } = fitEdits(scene(), e);
    expect(out.areas).toEqual([{ at: world(250, 50), colour: [9, 9, 9] }]);
  });

  it('takes the top floor when floors are stacked', () => {
    const e = edits({ areas: [{ at: world(20, 20), hidden: true }] });
    expect(fitEdits(scene(), e).edits.areas[0].at).toEqual(world(250, 50));
  });

  it('sets aside an area colour with no floor under it, and fits it again later', () => {
    const e = edits({ areas: [{ at: world(600, 600), colour: [9, 9, 9] }] });
    const first = fitEdits(scene(), e);
    expect(first.edits.areas).toEqual([]);
    expect(first.unplaced.areas).toBe(1);
    const grown = scene();
    grown.faces.push({ points: square(550, 550, 100), z: 0, area: 0, face: 10 });
    expect(fitEdits(grown, first.edits).edits.areas).toEqual([{ at: world(50, 50), colour: [9, 9, 9] }]);
  });

  it('sets piece colours aside on another version of the map, and back on the right one', () => {
    const e = edits({ faces: [{ face: 8, colour: [5, 5, 5] }], mapChecksum: 111 });
    const changed = fitEdits(scene(222), e);
    expect(changed.edits.faces).toEqual([]);
    expect(changed.unplaced.faces).toBe(1);
    expect(changed.edits.mapChecksum).toBe(222);
    const back = fitEdits(scene(111), changed.edits);
    expect(back.edits.faces).toEqual([{ face: 8, colour: [5, 5, 5] }]);
    expect(back.unplaced.faces).toBe(0);
  });

  it('adopts the checksum of edits saved before there was one', () => {
    const e = edits({ faces: [{ face: 8, colour: [5, 5, 5] }] });
    const { edits: out } = fitEdits(scene(), e);
    expect(out.faces).toHaveLength(1);
    expect(out.mapChecksum).toBe(111);
  });

  it('moves a flag name to a flag that moved a little, not a long way', () => {
    const moved = fitEdits(scene(), edits({ flagNames: [{ at: [1030, 2010], name: 'Square' }] }));
    expect(moved.edits.flagNames).toEqual([{ at: [1000, 2000], name: 'Square' }]);
    const gone = fitEdits(scene(), edits({ flagNames: [{ at: [3000, 2000], name: 'Square' }] }));
    expect(gone.edits.flagNames).toEqual([]);
    expect(gone.unplaced.flagNames).toBe(1);
  });
});
