import { describe, it, expect } from 'vitest';
import {
  emptyEdits, normaliseEdits, setAreaEdit, areaEdit, setFaceColour, faceColour,
  insidePolygon, faceAt, toPixel, toWorld, spawnLabels, flagName, setFlagName,
  labelAt, toBase64, flagOffset, setFlagOffset, flagLabelAt, flagLabelSpot,
  spawnNameSpots, setSpawnName, setSpawnOffset, resetSpawn, spawnChanged, spawnName, spawnNameAt, titleShown,
} from './overview_draw.js';
import { mapTitle } from './overview_themes.js';

const square = (x, y, size = 10) => [[x, y], [x + size, y], [x + size, y + size], [x, y + size]];

function scene() {
  return {
    map: 'dod_test',
    width: 1024,
    height: 768,
    transform: { zoom: 1.45, origin: [20, -752, -40], rotated: false, height: 0 },
    areas: [
      { id: 0, indoor: false, colour: [94, 94, 85], anchor: [100, 200], cells: 50 },
      { id: 1, indoor: true, colour: [146, 155, 247], anchor: [300, 400], cells: 50 },
    ],
    faces: [
      { points: square(0, 0, 100), z: 0, area: 0, stairs: false, face: 7 },
      { points: square(20, 20, 20), z: 50, area: 1, stairs: false, face: 8 },
      { points: square(60, 60, 10), z: 10, area: 0, stairs: true, face: 9 },
    ],
    water: [],
    cap_zones: [],
    flags: [{ name: 'Plaza', at: [50, 50], world: [1, 2, 3] }],
    allies: [],
    axis: [],
    palette: [],
    background: [0, 255, 0],
    void: [16, 17, 14],
  };
}

describe('edits', () => {
  it('fills in what an old or broken file lacks', () => {
    expect(normaliseEdits(null)).toEqual(emptyEdits());
    const e = normaliseEdits({ show: { flags: false }, labels: 'nope' });
    expect(e.show.flags).toBe(false);
    expect(e.show.spawns).toBe(true);
    expect(e.labels).toEqual([]);
  });

  it('reads a version 1 file: its colours become those of the theme it was made in', () => {
    const e = normaliseEdits({
      version: 1,
      theme: 'classic',
      areas: [{ at: [1, 2], hidden: true, colour: [9, 9, 9] }, { at: [3, 4], hidden: false, colour: [8, 8, 8] }],
      faces: [{ face: 7, colour: [5, 5, 5] }],
    });
    expect(e.version).toBe(2);
    expect(e.areas).toEqual([{ at: [1, 2], hidden: true }]);
    expect(e.colours).toEqual({
      classic: { areas: [{ at: [1, 2], colour: [9, 9, 9] }, { at: [3, 4], colour: [8, 8, 8] }], faces: [{ face: 7, colour: [5, 5, 5] }] },
    });
    expect(e.faces).toBeUndefined();
  });

  it('keeps colours per theme, and hiding for every theme', () => {
    const s = scene();
    let e = setAreaEdit({ ...emptyEdits(), theme: 'colours' }, s.areas[1], { colour: [1, 2, 3] });
    e = setAreaEdit(e, s.areas[0], { hidden: true });
    const classic = { ...e, theme: 'classic' };
    expect(areaEdit(classic, s.areas[1])).toBeUndefined();
    expect(areaEdit(classic, s.areas[0]).hidden).toBe(true);
    const painted = setAreaEdit(classic, s.areas[1], { colour: [7, 7, 7] });
    expect(areaEdit(painted, s.areas[1]).colour).toEqual([7, 7, 7]);
    expect(areaEdit({ ...painted, theme: 'colours' }, s.areas[1]).colour).toEqual([1, 2, 3]);
  });

  it('keys an area edit by its anchor and removes it with null', () => {
    const s = scene();
    let e = setAreaEdit(emptyEdits(), s.areas[1], { colour: [1, 2, 3] });
    expect(areaEdit(e, s.areas[1]).colour).toEqual([1, 2, 3]);
    expect(areaEdit(e, s.areas[0])).toBeUndefined();
    e = setAreaEdit(e, s.areas[1], null);
    expect(e.areas).toEqual([]);
  });

  it('a dragged flag name keeps its place when renamed, and loses it on reset', () => {
    const s = scene();
    const flag = s.flags[0];
    let e = setFlagOffset(emptyEdits(), flag, [40, -10]);
    e = setFlagName(e, flag, 'Square');
    expect(flagOffset(e, flag)).toEqual([40, -10]);
    expect(flagName(e, flag)).toBe('Square');
    e = setFlagName(e, flag, 'Plaza');
    expect(flagOffset(e, flag)).toEqual([40, -10]);
    e = setFlagOffset(e, flag, null);
    expect(e.flagNames).toEqual([]);
  });

  it("finds a flag's name where it is drawn", () => {
    const s = scene();
    const flag = s.flags[0];
    const [x, y] = flagLabelSpot(s, emptyEdits(), flag);
    expect(y).toBeGreaterThan(flag.at[1]);
    expect(flagLabelAt(s, emptyEdits(), x, y)).toBe(flag);
    expect(flagLabelAt(s, emptyEdits(), flag.at[0], flag.at[1])).toBeNull();
    const hidden = { ...emptyEdits(), show: { ...emptyEdits().show, flagLabels: false } };
    expect(flagLabelAt(s, hidden, x, y)).toBeNull();
  });

  it('typed flag names replace the game name, and the game name clears them', () => {
    const s = scene();
    let e = setFlagName(emptyEdits(), s.flags[0], 'Square');
    expect(flagName(e, s.flags[0])).toBe('Square');
    e = setFlagName(e, s.flags[0], 'Plaza');
    expect(e.flagNames).toEqual([]);
  });
});

describe('faceColour', () => {
  it('a face colour beats its area, which beats stairs, which beat the default', () => {
    const s = scene();
    const stairs = s.faces[2];
    expect(faceColour(s, emptyEdits(), stairs)).toEqual([255, 255, 255]);
    let e = setAreaEdit(emptyEdits(), s.areas[0], { colour: [9, 9, 9] });
    expect(faceColour(s, e, stairs)).toEqual([9, 9, 9]);
    e = setFaceColour(e, stairs, [5, 5, 5]);
    expect(faceColour(s, e, stairs)).toEqual([5, 5, 5]);
  });

  it('stairs drop back to their area colour when white stairs are off', () => {
    const s = scene();
    const e = { ...emptyEdits(), show: { ...emptyEdits().show, stairs: false } };
    expect(faceColour(s, e, s.faces[2])).toEqual([94, 94, 85]);
  });

  it('a hidden area draws nothing', () => {
    const s = scene();
    const e = setAreaEdit(emptyEdits(), s.areas[0], { hidden: true });
    expect(faceColour(s, e, s.faces[0])).toBeNull();
  });
});

describe('hit testing', () => {
  it('finds the top face, and skips hidden ones unless asked', () => {
    const s = scene();
    expect(insidePolygon(square(0, 0), 5, 5)).toBe(true);
    expect(insidePolygon(square(0, 0), 15, 5)).toBe(false);
    expect(faceAt(s, emptyEdits(), 30, 30).face).toBe(8);
    expect(faceAt(s, emptyEdits(), 5, 5).face).toBe(7);
    const hidden = setAreaEdit(emptyEdits(), s.areas[1], { hidden: true });
    expect(faceAt(s, hidden, 30, 30).face).toBe(7);
    expect(faceAt(s, hidden, 30, 30, { includeHidden: true }).face).toBe(8);
  });

  it('finds a label by its text box', () => {
    const s = scene();
    const world = toWorld(s.transform, 500, 400);
    const e = { ...emptyEdits(), labels: [{ id: 'a', text: 'Church', world, size: 15 }] };
    expect(labelAt(s, e, 505, 402).id).toBe('a');
    expect(labelAt(s, e, 600, 400)).toBeNull();
  });
});

describe('transform', () => {
  it('matches native::overview::transform both ways round', () => {
    for (const rotated of [false, true]) {
      const t = { zoom: 1.45, origin: [20, -752, -40], rotated, height: 0 };
      const [px, py] = toPixel(t, -1432, -1520);
      const [x, y] = toWorld(t, px, py);
      expect(x).toBeCloseTo(-1432, 1);
      expect(y).toBeCloseTo(-1520, 1);
    }
    // World x runs up the image when not rotated.
    const t = { zoom: 1.45, origin: [20, -752, -40], rotated: false, height: 0 };
    expect(toPixel(t, 100, 0)[1]).toBeLessThan(toPixel(t, 0, 0)[1]);
  });
});

describe('spawnLabels', () => {
  it('labels a group of four or more once, under it', () => {
    const s = scene();
    s.allies = [0, 1, 2, 3].map((i) => ({ name: 'Allies', at: [100 + i * 10, 100], world: [0, 0, 0] }));
    s.axis = [{ name: 'Axis', at: [500, 500], world: [0, 0, 0] }];
    const labels = spawnLabels(s);
    expect(labels).toHaveLength(1);
    expect(labels[0].team).toBe('Allies');
    expect(labels[0].at[1]).toBeGreaterThan(100);
  });
});

describe('spawn names (#581)', () => {
  const withSpawns = () => {
    const s = scene();
    s.allies = [0, 1, 2, 3].map((i) => ({ name: 'Allies', at: [100 + i * 10, 100], world: [0, 0, 0] }));
    s.axis = [0, 1, 2, 3].map((i) => ({ name: 'Axis', at: [600 + i * 10, 500], world: [0, 0, 0] }));
    return s;
  };

  it('are "Allies spawn" and "Axis spawn" until renamed', () => {
    const s = withSpawns();
    expect(spawnNameSpots(s, emptyEdits()).map((p) => p.name)).toEqual(['Allies spawn', 'Axis spawn']);
  });

  it('a typed name is drawn and found where it is, and keeps a drag', () => {
    const s = withSpawns();
    const [allies] = spawnNameSpots(s, emptyEdits());
    let e = setSpawnOffset(s, emptyEdits(), allies.label, [64, 0]);
    e = setSpawnName(s, e, allies.label, '  Beach landing  ');
    expect(spawnName(s, e, allies.label)).toBe('Beach landing');
    const spot = spawnNameSpots(s, e)[0];
    expect(spot.name).toBe('Beach landing');
    expect(spot.at).not.toEqual(spot.home);
    expect(spawnNameAt(s, e, spot.at[0], spot.at[1])?.team).toBe('Allies');
    expect(e.spawnNames).toHaveLength(1);
    expect(spawnChanged(s, e, allies.label)).toBe(true);
    // The other spawn is untouched.
    expect(spawnChanged(s, e, spawnNameSpots(s, e)[1].label)).toBe(false);
  });

  it('a blank or default name takes the name back but keeps the place; reset takes both', () => {
    const s = withSpawns();
    const [allies] = spawnNameSpots(s, emptyEdits());
    let e = setSpawnName(s, emptyEdits(), allies.label, 'Beach');
    e = setSpawnOffset(s, e, allies.label, [10, 10]);
    e = setSpawnName(s, e, allies.label, '');
    expect(spawnName(s, e, allies.label)).toBe('Allies spawn');
    expect(e.spawnNames[0].offset).toEqual([10, 10]);
    expect(setSpawnName(s, setSpawnName(s, emptyEdits(), allies.label, 'x'), allies.label, 'Allies spawn').spawnNames).toEqual([]);
    expect(resetSpawn(s, e, allies.label).spawnNames).toEqual([]);
  });
});

describe('titleShown (#581)', () => {
  it('follows the theme until set, then stays as set', () => {
    expect(titleShown({ ...emptyEdits(), theme: 'classic' })).toBe(true);
    expect(titleShown({ ...emptyEdits(), theme: 'grey' })).toBe(false);
    expect(titleShown({ ...emptyEdits(), theme: 'colours' })).toBe(false);
    const off = { ...emptyEdits(), theme: 'classic', show: { ...emptyEdits().show, title: false } };
    expect(titleShown(off)).toBe(false);
    const on = normaliseEdits({ theme: 'grey', show: { title: true } });
    expect(titleShown(on)).toBe(true);
  });
});

describe('toBase64', () => {
  it('encodes large arrays without overflowing the stack', () => {
    const bytes = new Uint8Array(200000).fill(65);
    expect(toBase64(bytes).slice(0, 4)).toBe('QUFB');
    expect(toBase64(new Uint8Array([0, 255]))).toBe('AP8=');
  });
});

describe('mapTitle', () => {
  it('drops dod_, league tags and versions', () => {
    expect(mapTitle('dod_anzio')).toBe('ANZIO');
    expect(mapTitle('dod_railroad2_s10a')).toBe('RAILROAD2');
    expect(mapTitle('dod_saints2_b4e')).toBe('SAINTS2');
    expect(mapTitle('dod_cevo_russka_mtek')).toBe('RUSSKA');
    expect(mapTitle('dod_anjou_a4_v04')).toBe('ANJOU');
    expect(mapTitle('dod_lennon5_b1')).toBe('LENNON5');
  });
});
