// overview_fit.js — fitting saved Overviews edits onto the scene built today
// (#371). Edits are kept by world position (areas by a point inside them,
// labels and flag names where they were put) and per-piece colours by the
// piece's number in the map file. A newer builder can reshape areas, and a
// newer release of the map renumbers its pieces, so on opening a map:
//
// - an area edit (hidden, or a theme's colour) whose point is no longer its
//   area's anchor moves to the area under that point (the top floor there);
// - a flag name moves to the nearest flag within FLAG_REACH of where it was;
// - piece colours made on another version of the map (the map file's
//   checksum differs) are set aside, not put on whatever pieces now carry
//   those numbers;
//
// and anything that fits nowhere is kept in `edits.aside` (tried again next
// time, saved with the rest), counted for the page to say so. Colours are
// per theme (overview_draw.js); every theme's set is fitted.

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
 * Area edits (each `{ at, ... }`) moved onto today's areas: exact anchors
 * first, then by position, one edit per area. `{ kept, aside }`.
 */
function fitAreas(scene, edits) {
  const byArea = new Map();
  const left = [];
  for (const e of edits) {
    const area = scene.areas.find((a) => near(a.anchor, e.at, 1));
    if (area && !byArea.has(area.id)) byArea.set(area.id, { ...e, at: area.anchor });
    else if (!area) left.push(e);
  }
  const aside = [];
  for (const e of left) {
    const area = areaUnder(scene, e.at);
    if (area && !byArea.has(area.id)) byArea.set(area.id, { ...e, at: area.anchor });
    else if (!area) aside.push(e);
  }
  return { kept: [...byArea.values()], aside };
}

/** Flag names onto the flag where they were put, else the nearest close by. */
function fitFlagNames(scene, names) {
  const kept = [];
  const aside = [];
  const named = new Set();
  const exact = (e) => scene.flags.find((f) => near(f.world, e.at, 1));
  const distance = (f, e) => Math.hypot(f.world[0] - e.at[0], f.world[1] - e.at[1]);
  for (const e of names.filter(exact)) {
    const flag = exact(e);
    if (named.has(flag)) continue;
    named.add(flag);
    kept.push({ ...e, at: [flag.world[0], flag.world[1]] });
  }
  for (const e of names.filter((n) => !exact(n))) {
    const flag = scene.flags
      .filter((f) => !named.has(f) && near(f.world, e.at, FLAG_REACH))
      .sort((a, b) => distance(a, e) - distance(b, e))[0];
    if (flag) {
      named.add(flag);
      kept.push({ ...e, at: [flag.world[0], flag.world[1]] });
    } else {
      aside.push(e);
    }
  }
  return { kept, aside };
}

/**
 * `edits` fitted to `scene`: `{ edits, unplaced }`, where `unplaced` counts
 * what is in `edits.aside` now (`areas`, `faces`, `flagNames`; colours of
 * every theme together).
 */
export function fitEdits(scene, edits) {
  const aside = edits.aside || {};
  const checksum = scene.checksum || null;
  const changedMap = !!(checksum && edits.mapChecksum && edits.mapChecksum !== checksum);
  let asideChecksum = aside.mapChecksum ?? null;
  const unplaced = { areas: 0, faces: 0, flagNames: 0 };

  const hidden = fitAreas(scene, [...edits.areas, ...(aside.areas || [])]);
  unplaced.areas += hidden.aside.length;

  const colours = {};
  const coloursAside = {};
  const themes = new Set([...Object.keys(edits.colours || {}), ...Object.keys(aside.colours || {})]);
  for (const id of themes) {
    const now = edits.colours?.[id] || { areas: [], faces: [] };
    const old = aside.colours?.[id] || { areas: [], faces: [] };
    const areas = fitAreas(scene, [...now.areas, ...old.areas]);
    let faces = now.faces;
    let facesAside = old.faces;
    // Piece colours only on the version of the map they were made on.
    if (changedMap && faces.length) {
      facesAside = [...facesAside, ...faces];
      faces = [];
      asideChecksum = edits.mapChecksum;
    }
    if (checksum && asideChecksum === checksum && facesAside.length) {
      faces = [...faces, ...facesAside.filter((a) => !faces.some((f) => f.face === a.face))];
      facesAside = [];
    }
    colours[id] = { areas: areas.kept, faces };
    if (areas.aside.length || facesAside.length) coloursAside[id] = { areas: areas.aside, faces: facesAside };
    unplaced.areas += areas.aside.length;
    unplaced.faces += facesAside.length;
  }
  if (!unplaced.faces) asideChecksum = null;

  const flags = fitFlagNames(scene, [...edits.flagNames, ...(aside.flagNames || [])]);
  unplaced.flagNames = flags.aside.length;

  return {
    edits: {
      ...edits,
      areas: hidden.kept,
      colours,
      flagNames: flags.kept,
      mapChecksum: checksum ?? edits.mapChecksum ?? null,
      aside: { areas: hidden.aside, flagNames: flags.aside, colours: coloursAside, mapChecksum: asideChecksum },
    },
    unplaced,
  };
}
