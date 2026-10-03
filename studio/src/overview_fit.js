// overview_fit.js — fitting saved Overviews edits onto the scene built today
// (#371). Edits are kept by world position (areas by a point inside them,
// labels and flag names where they were put) and per-piece colours by the
// piece's number in the map file. A newer builder can reshape areas, and a
// newer release of the map renumbers its pieces, so on opening a map:
//
// - an area edit whose point is no longer its area's anchor moves to the
//   area under that point (the top floor there);
// - a flag name moves to the nearest flag within FLAG_REACH of where it was;
// - piece colours made on another version of the map (the map file's
//   checksum differs) are set aside, not put on whatever pieces now carry
//   those numbers;
//
// and anything that fits nowhere is kept in `edits.aside` (tried again next
// time, saved with the rest), counted for the page to say so.

import { toPixel, insidePolygon } from './overview_draw.js';

/** How far (world units) a flag may have moved and keep its typed name. */
const FLAG_REACH = 64;

const near = (a, b, d) => Math.abs(a[0] - b[0]) <= d && Math.abs(a[1] - b[1]) <= d;

/** The area of the top floor under world point `at`, or null. */
function areaUnder(scene, at) {
  const [x, y] = toPixel(scene.transform, at[0], at[1]);
  let top = null;
  for (const face of scene.faces) {
    if (insidePolygon(face.points, x, y) && (!top || face.z >= top.z)) top = face;
  }
  return top ? scene.areas[top.area] ?? null : null;
}

/**
 * `edits` fitted to `scene`: `{ edits, unplaced }`, where `unplaced` counts
 * what is in `edits.aside` now (`areas`, `faces`, `flagNames`).
 */
export function fitEdits(scene, edits) {
  const aside = { areas: [], faces: [], flagNames: [], mapChecksum: null, ...(edits.aside || {}) };

  // Areas: exact anchors first, then by position, one edit per area.
  const all = [...edits.areas, ...aside.areas];
  const byArea = new Map();
  const left = [];
  for (const e of all) {
    const area = scene.areas.find((a) => near(a.anchor, e.at, 1));
    if (area && !byArea.has(area.id)) byArea.set(area.id, { ...e, at: area.anchor });
    else if (!area) left.push(e);
  }
  const areasAside = [];
  for (const e of left) {
    const area = areaUnder(scene, e.at);
    if (area && !byArea.has(area.id)) byArea.set(area.id, { ...e, at: area.anchor });
    else if (!area) areasAside.push(e);
  }

  // Piece colours: only on the version of the map they were made on.
  const checksum = scene.checksum || null;
  let faces = edits.faces;
  let facesAside = aside.faces;
  let asideChecksum = aside.mapChecksum;
  if (checksum && edits.mapChecksum && edits.mapChecksum !== checksum && faces.length) {
    facesAside = faces;
    asideChecksum = edits.mapChecksum;
    faces = [];
  }
  if (checksum && asideChecksum === checksum && facesAside.length) {
    faces = [...faces, ...facesAside.filter((a) => !faces.some((f) => f.face === a.face))];
    facesAside = [];
    asideChecksum = null;
  }

  // Flag names: the flag where they were put, else the nearest one close by.
  const flagNames = [];
  const flagsAside = [];
  const named = new Set();
  const candidates = [...edits.flagNames, ...aside.flagNames];
  const exact = (e) => scene.flags.find((f) => near(f.world, e.at, 1));
  for (const e of candidates.filter(exact)) {
    const flag = exact(e);
    if (named.has(flag)) continue;
    named.add(flag);
    flagNames.push({ ...e, at: [flag.world[0], flag.world[1]] });
  }
  for (const e of candidates.filter((c) => !exact(c))) {
    const flag = scene.flags
      .filter((f) => !named.has(f) && near(f.world, e.at, FLAG_REACH))
      .sort((a, b) => Math.hypot(a.world[0] - e.at[0], a.world[1] - e.at[1]) - Math.hypot(b.world[0] - e.at[0], b.world[1] - e.at[1]))[0];
    if (flag) {
      named.add(flag);
      flagNames.push({ ...e, at: [flag.world[0], flag.world[1]] });
    } else {
      flagsAside.push(e);
    }
  }

  const fitted = {
    ...edits,
    areas: [...byArea.values()],
    faces,
    flagNames,
    mapChecksum: checksum ?? edits.mapChecksum ?? null,
    aside: { areas: areasAside, faces: facesAside, flagNames: flagsAside, mapChecksum: asideChecksum },
  };
  return {
    edits: fitted,
    unplaced: { areas: areasAside.length, faces: facesAside.length, flagNames: flagsAside.length },
  };
}
