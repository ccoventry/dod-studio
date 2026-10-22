// overview_draw.js — drawing an overview scene (native::overview::scene) with
// the page's edits applied, onto any canvas at any scale. The Overviews page
// draws it on screen with this, and the export draws it three times larger
// and scales it down, so the file is what the page showed.

import { themeOf, mapTitle } from './overview_themes.js';
import { paper, squaredPaper, grid, frame, titleCard, titleCardBox, areaEdges } from './overview_paper.js';
import { drawOverlay } from './overview_overlay.js';

/** Spawn protection's look, until changed. Colours are #rrggbb. */
export const SPAWN_PROTECTION = {
  fill: 'tint', // 'tint' | 'hatch' | 'none'
  line: 'team', // 'team' | 'hazard' | 'none'
  allies: '#28c83c',
  axis: '#dc2828',
  stripe1: '#ffcc00',
  stripe2: '#1a1a1a',
};

/** A fresh, empty set of edits. */
export function emptyEdits() {
  return {
    version: 2,
    // Hidden areas, for every theme: { at, hidden }, keyed by the area's
    // anchor (a world point), so they survive a rebuild.
    areas: [],
    // Colours, one set per theme id (overview_themes.js), so each theme
    // keeps its own: { areas: [{ at, colour }], faces: [{ face, colour }] },
    // faces keyed by the BSP face index.
    colours: {},
    // Text placed by hand, at a world point.
    labels: [],
    // Flag names typed over the game's, keyed by the flag's world position.
    flagNames: [],
    // Spawn names renamed or dragged away from their spawn:
    // { team, at, name?, offset? }, `at` the world point of the spawn group
    // the name belongs to, `name` typed over "Allies spawn"/"Axis spawn",
    // `offset` in world units from where the page would put it.
    spawnNames: [],
    // Where the map title was dragged to, in image pixels from its place in
    // the top right, or null. Whether it shows is `show.title`, which is
    // left out until set: each theme has its own default (titleShown).
    titleOffset: null,
    // The title card's second line (#580): null for the credit found with
    // the map (scene.credit), else what was typed; '' for none.
    credit: null,
    show: {
      spawns: true,
      spawnLabels: true,
      spawnProtection: true,
      flags: true,
      flagLabels: true,
      capZones: true,
      breakables: true,
      slopes: true,
      stairs: true,
      water: true,
    },
    format: 'tga',
    // overview_themes.js: how it looks. Null until chosen for this map: the
    // page then uses the theme last picked on any map.
    theme: null,
    // Also write <map>_hd.tga, which DoD Studio's hook tiles from in game.
    hd: true,
    // How spawn protection is drawn (when shown): its floor filled, and the
    // line where you walk into it, both only on floor a player can reach.
    spawnProtection: { ...SPAWN_PROTECTION },
    // The map file's checksum when these edits were made (overview_fit.js).
    mapChecksum: null,
    // Edits that fit nothing on the map as built now, kept to try again
    // (overview_fit.js); colours per theme, as above.
    aside: { areas: [], flagNames: [], colours: {}, mapChecksum: null },
  };
}

const list = (v) => (Array.isArray(v) ? v : []);

/** `{ theme: { areas, faces } }` with both lists always there. */
function colourSets(raw) {
  const out = {};
  for (const [id, set] of Object.entries(raw && typeof raw === 'object' ? raw : {})) {
    out[id] = { areas: list(set?.areas), faces: list(set?.faces) };
  }
  return out;
}

/**
 * Splits an area/face list of a version 1 file (one set of colours for
 * every theme) into hidden areas and the colours of the theme it was made
 * in, so nothing done before per-theme colours is lost.
 */
function splitOld(areas, faces, theme) {
  const coloured = list(areas).filter((e) => e.colour).map((e) => ({ at: e.at, colour: e.colour }));
  const hidden = list(areas).filter((e) => e.hidden).map((e) => ({ at: e.at, hidden: true }));
  const sets = coloured.length || list(faces).length ? { [theme]: { areas: coloured, faces: list(faces) } } : {};
  return { hidden, sets };
}

/** Fills in anything an older or partial edits file lacks. */
export function normaliseEdits(raw) {
  const base = emptyEdits();
  if (!raw || typeof raw !== 'object') return base;
  const theme = themeOf(raw).id;
  const now = Array.isArray(raw.faces) || list(raw.areas).some((e) => e.colour)
    ? splitOld(raw.areas, raw.faces, theme)
    : { hidden: list(raw.areas), sets: {} };
  const aside = raw.aside || {};
  const asideNow = Array.isArray(aside.faces) || list(aside.areas).some((e) => e.colour)
    ? splitOld(aside.areas, aside.faces, theme)
    : { hidden: list(aside.areas), sets: {} };
  const { faces: _old, ...rest } = raw;
  return {
    ...base,
    ...rest,
    version: 2,
    areas: now.hidden,
    colours: { ...colourSets(raw.colours), ...now.sets },
    labels: list(raw.labels),
    flagNames: list(raw.flagNames),
    spawnNames: list(raw.spawnNames),
    show: { ...base.show, ...(raw.show || {}) },
    spawnProtection: { ...base.spawnProtection, ...(raw.spawnProtection || {}) },
    aside: {
      areas: asideNow.hidden,
      flagNames: list(aside.flagNames),
      colours: { ...colourSets(aside.colours), ...asideNow.sets },
      mapChecksum: aside.mapChecksum ?? null,
    },
  };
}

const near = (a, b, d) => Math.abs(a[0] - b[0]) <= d && Math.abs(a[1] - b[1]) <= d;

/** The colours of the theme the edits are drawn in. */
export function themeColours(edits) {
  return edits.colours?.[themeOf(edits).id] || { areas: [], faces: [] };
}

function withThemeColours(edits, patch) {
  const id = themeOf(edits).id;
  return { ...edits, colours: { ...(edits.colours || {}), [id]: { ...themeColours(edits), ...patch } } };
}

/**
 * An area's edit in the current theme, `{ at, hidden, colour? }`, or
 * undefined: whether it is hidden (every theme) and its colour (this one).
 */
export function areaEdit(edits, area) {
  const hidden = !!edits.areas.find((e) => near(e.at, area.anchor, 1))?.hidden;
  const colour = themeColours(edits).areas.find((e) => near(e.at, area.anchor, 1))?.colour;
  if (!hidden && !colour) return undefined;
  return colour ? { at: area.anchor, hidden, colour } : { at: area.anchor, hidden };
}

/** Changes one area's edit; `patch` null removes it (in this theme). */
export function setAreaEdit(edits, area, patch) {
  const next = patch ? { ...(areaEdit(edits, area) || {}), ...patch } : {};
  const away = (e) => !near(e.at, area.anchor, 1);
  const hidden = edits.areas.filter(away);
  const coloured = themeColours(edits).areas.filter(away);
  return withThemeColours(
    { ...edits, areas: next.hidden ? [...hidden, { at: area.anchor, hidden: true }] : hidden },
    { areas: next.colour ? [...coloured, { at: area.anchor, colour: next.colour }] : coloured },
  );
}

/** Colours one floor piece in the current theme; null takes it back. */
/** The pieces of `area` coloured on their own in the current theme. */
export function paintedPieces(scene, edits, area) {
  const own = new Set(themeColours(edits).faces.map((e) => e.face));
  return new Set(scene.faces.filter((f) => f.area === area.id && own.has(f.face)).map((f) => f.face));
}

/** Clears the own colours of `area`'s pieces in the current theme. */
export function clearAreaPieces(scene, edits, area) {
  const mine = new Set(scene.faces.filter((f) => f.area === area.id).map((f) => f.face));
  return withThemeColours(edits, { faces: themeColours(edits).faces.filter((e) => !mine.has(e.face)) });
}

export function setFaceColour(edits, face, colour) {
  const rest = themeColours(edits).faces.filter((e) => e.face !== face.face);
  return withThemeColours(edits, { faces: colour ? [...rest, { face: face.face, colour }] : rest });
}

// A flag's entry in edits.flagNames: { at, name?, offset? }, `at` the
// flag's world position, `name` typed over the game's, `offset` (world
// units from the flag) where its name was dragged to.
const flagEntry = (edits, flag) => edits.flagNames.find((e) => near(e.at, flag.world, 1));

function setFlagEntry(edits, flag, patch) {
  const rest = edits.flagNames.filter((e) => !near(e.at, flag.world, 1));
  const next = { ...(flagEntry(edits, flag) || {}), ...patch, at: [flag.world[0], flag.world[1]] };
  if (next.name == null) delete next.name;
  if (next.offset == null) delete next.offset;
  return { ...edits, flagNames: next.name != null || next.offset ? [...rest, next] : rest };
}

export function flagName(edits, flag) {
  return flagEntry(edits, flag)?.name ?? flag.name;
}

export function setFlagName(edits, flag, name) {
  return setFlagEntry(edits, flag, { name: name == null || name === flag.name ? null : name });
}

/** Where a flag's name has been dragged to (world units from it), or null. */
export function flagOffset(edits, flag) {
  return flagEntry(edits, flag)?.offset ?? null;
}

/** Moves a flag's name `offset` world units from the flag; null puts it back. */
export function setFlagOffset(edits, flag, offset) {
  return setFlagEntry(edits, flag, { offset });
}

/** Rough half-width of a 15 px bold name, in image pixels. */
const nameHalf = (name) => Math.max(12, (name.length * 15 * 0.6) / 2);

/**
 * The middle of a flag's name, in image pixels: where it was dragged to,
 * else under the flag, clear of the icon the game draws over it (about 64
 * image pixels across on the full map at 720p); above it at the bottom
 * edge; kept inside the image sideways.
 */
export function flagLabelSpot(scene, edits, flag) {
  const offset = flagOffset(edits, flag);
  if (offset) return toPixel(scene.transform, flag.world[0] + offset[0], flag.world[1] + offset[1]);
  const [x, y] = flag.at;
  const half = nameHalf(flagName(edits, flag));
  const cx = Math.max(half + 4, Math.min(scene.width - half - 4, x));
  const below = y + FLAG_CLEAR + 8 < scene.height - 4;
  return [cx, below ? y + FLAG_CLEAR : y - FLAG_CLEAR];
}

/** The flag whose name is under an image pixel, when names are shown. */
export function flagLabelAt(scene, edits, x, y) {
  if (!edits.show.flags || !edits.show.flagLabels) return null;
  for (const flag of scene.flags) {
    const name = flagName(edits, flag);
    if (!name) continue;
    const [lx, ly] = flagLabelSpot(scene, edits, flag);
    if (Math.abs(x - lx) <= nameHalf(name) && Math.abs(y - ly) <= 15 * 0.7) return flag;
  }
  return null;
}

/** The colour a face is drawn in, or null when its area is hidden. */
export function faceColour(scene, edits, face) {
  const area = scene.areas[face.area];
  const areaChange = area ? areaEdit(edits, area) : null;
  if (areaChange?.hidden) return null;
  const own = themeColours(edits).faces.find((e) => e.face === face.face);
  if (own) return own.colour;
  if (areaChange?.colour) return areaChange.colour;
  if (face.stairs && edits.show.stairs) return [255, 255, 255];
  return themeOf(edits).floor(scene, face, area);
}

/** Ray-casting point-in-polygon, in image pixels. */
export function insidePolygon(points, x, y) {
  let inside = false;
  for (let i = 0, j = points.length - 1; i < points.length; j = i++) {
    const [xi, yi] = points[i];
    const [xj, yj] = points[j];
    if ((yi > y) !== (yj > y) && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}

/** The top face under an image pixel, skipping hidden areas unless asked. */
export function faceAt(scene, edits, x, y, { includeHidden = false } = {}) {
  for (let i = scene.faces.length - 1; i >= 0; i--) {
    const face = scene.faces[i];
    if (!includeHidden && faceColour(scene, edits, face) == null) continue;
    if (insidePolygon(face.points, x, y)) return face;
  }
  return null;
}

/** World (x, y) to image pixels, the game's way (native::overview::transform). */
export function toPixel(t, x, y) {
  const [ox, oy] = t.origin;
  if (t.rotated) [x, y] = [ox + (y - oy), oy - (x - ox)];
  const z = t.zoom;
  const v = (ox + 4096 / (1.33 * z) - x) / (6144 / z);
  const u = (oy + 4096 / z - y) / ((8192 * 4) / 3 / (1.33 * z));
  return [u * 1024, v * 768];
}

export function toWorld(t, px, py) {
  const [ox, oy] = t.origin;
  const z = t.zoom;
  const u = px / 1024;
  const v = py / 768;
  const x = ox + 4096 / (1.33 * z) - (v * 6144) / z;
  const y = oy + 4096 / z - (u * 8192 * 4) / 3 / (1.33 * z);
  return t.rotated ? [ox + oy - y, x - ox + oy] : [x, y];
}

/** One label per group of four or more spawns of a team, under the group. */
export function spawnLabels(scene) {
  const out = [];
  for (const [team, list] of [['Allies', scene.allies], ['Axis', scene.axis]]) {
    const groups = [];
    for (const m of list) {
      const g = groups.find((gr) => gr.some((p) => Math.abs(p[0] - m.at[0]) + Math.abs(p[1] - m.at[1]) < 40));
      if (g) g.push(m.at);
      else groups.push([m.at]);
    }
    groups.sort((a, b) => b.length - a.length);
    const placed = [];
    for (const g of groups) {
      const x = g.reduce((s, p) => s + p[0], 0) / g.length;
      const y = Math.max(...g.map((p) => p[1])) + 6;
      if (g.length >= 4 && placed.every((p) => Math.abs(p[0] - x) + Math.abs(p[1] - y) > 160)) {
        out.push({ team, at: [x, y] });
        placed.push([x, y]);
      }
    }
  }
  return out;
}

// A spawn name's key: its team and the world point of where the page puts
// it, so a rename or a drag survives a rebuild that leaves the spawns where
// they were.
const spawnKey = (scene, l) => toWorld(scene.transform, l.at[0], l.at[1]);
const spawnEntry = (scene, edits, l) => {
  const at = spawnKey(scene, l);
  return (edits.spawnNames || []).find((e) => e.team === l.team && near(e.at, at, 48));
};

/** What a spawn is called until renamed. */
export const defaultSpawnName = (team) => `${team} spawn`;

function setSpawnEntry(scene, edits, label, patch) {
  const at = spawnKey(scene, label);
  const rest = (edits.spawnNames || []).filter((e) => !(e.team === label.team && near(e.at, at, 48)));
  const next = { ...(spawnEntry(scene, edits, label) || {}), ...patch, team: label.team, at };
  if (next.name == null) delete next.name;
  if (next.offset == null) delete next.offset;
  return { ...edits, spawnNames: next.name != null || next.offset ? [...rest, next] : rest };
}

/** A spawn's name: the one typed, else "Allies spawn" / "Axis spawn". */
export function spawnName(scene, edits, label) {
  return spawnEntry(scene, edits, label)?.name ?? defaultSpawnName(label.team);
}

/** Renames a spawn; blank, null or the default name takes it back. */
export function setSpawnName(scene, edits, label, name) {
  const typed = name == null ? '' : String(name).trim();
  return setSpawnEntry(scene, edits, label, { name: !typed || typed === defaultSpawnName(label.team) ? null : typed });
}

/** Each spawn name with where it is drawn: `{ team, name, at, home }`, image pixels. */
export function spawnNameSpots(scene, edits) {
  return spawnLabels(scene).map((l) => {
    const home = [l.at[0], l.at[1] + 7];
    const e = spawnEntry(scene, edits, l);
    const name = e?.name ?? defaultSpawnName(l.team);
    if (!e?.offset) return { team: l.team, name, at: home, home, label: l };
    const w = toWorld(scene.transform, home[0], home[1]);
    return { team: l.team, name, at: toPixel(scene.transform, w[0] + e.offset[0], w[1] + e.offset[1]), home, label: l };
  });
}

/** Moves a spawn name `offset` world units from its place; null puts it back. */
export function setSpawnOffset(scene, edits, label, offset) {
  return setSpawnEntry(scene, edits, label, { offset });
}

/** Whether a spawn was renamed or moved. */
export function spawnChanged(scene, edits, label) {
  return !!spawnEntry(scene, edits, label);
}

/** Puts a spawn's name and place back. */
export function resetSpawn(scene, edits, label) {
  return setSpawnEntry(scene, edits, label, { name: null, offset: null });
}

/** The spawn name under an image pixel, when spawn names are shown. */
export function spawnNameAt(scene, edits, x, y) {
  if (!edits.show.spawnLabels) return null;
  for (const spot of spawnNameSpots(scene, edits)) {
    const half = nameHalf(spot.name);
    if (Math.abs(x - spot.at[0]) <= half && Math.abs(y - spot.at[1]) <= 15 * 0.7) return spot;
  }
  return null;
}

/** The title card's second line: the credit typed, else the one found with
 *  the map ("by ..."), else none. */
export function titleCredit(scene, edits) {
  return edits.credit ?? scene.credit ?? '';
}

/** Whether the map title shows: as set, else the theme's own default (on
 *  for the themes with a ruler frame, Classic's printed-map look). */
export function titleShown(edits) {
  return edits.show?.title ?? !!themeOf(edits).frame;
}

/** The title card's box, in image pixels, wherever it was dragged. */
export function titleBox(scene, edits) {
  return titleCardBox(scene, mapTitle(scene.map), titleCredit(scene, edits), edits.titleOffset);
}

/** Whether an image pixel is on the map title, when it shows. */
export function titleAt(scene, edits, x, y) {
  if (!titleShown(edits) || !mapTitle(scene.map)) return false;
  const b = titleBox(scene, edits);
  return x >= b.x && x <= b.x + b.w && y >= b.y && y <= b.y + b.h;
}

const css = (c) => `rgb(${c[0]},${c[1]},${c[2]})`;

/** How far a flag's name sits from the flag (its middle), in image pixels. */
const FLAG_CLEAR = 42;

function polygon(ctx, points, s) {
  ctx.beginPath();
  points.forEach(([x, y], i) => (i ? ctx.lineTo(x * s, y * s) : ctx.moveTo(x * s, y * s)));
  ctx.closePath();
}

function layer(width, height) {
  const c = typeof OffscreenCanvas !== 'undefined'
    ? new OffscreenCanvas(width, height)
    : Object.assign(document.createElement('canvas'), { width, height });
  return c;
}

/** Everything enclosed by the drawn floors (and a thin rim round them), as
 *  a 1024x768 mask: 1 where the void colour goes. */
function voidMask(scene, edits) {
  const w = scene.width;
  const h = scene.height;
  const c = layer(w, h);
  const ctx = c.getContext('2d');
  ctx.fillStyle = '#fff';
  ctx.strokeStyle = '#fff';
  ctx.lineWidth = 6;
  ctx.lineJoin = 'round';
  for (const face of scene.faces) {
    if (faceColour(scene, edits, face) == null) continue;
    polygon(ctx, face.points, 1);
    ctx.fill();
    ctx.stroke();
  }
  const data = ctx.getImageData(0, 0, w, h).data;
  const covered = new Uint8Array(w * h);
  for (let i = 0; i < w * h; i++) covered[i] = data[i * 4 + 3] > 0 ? 1 : 0;
  // Flood the outside from the border; whatever it can't reach is enclosed.
  const outside = new Uint8Array(w * h);
  const stack = [];
  const push = (i) => {
    if (!covered[i] && !outside[i]) {
      outside[i] = 1;
      stack.push(i);
    }
  };
  for (let x = 0; x < w; x++) {
    push(x);
    push((h - 1) * w + x);
  }
  for (let y = 0; y < h; y++) {
    push(y * w);
    push(y * w + w - 1);
  }
  while (stack.length) {
    const i = stack.pop();
    const x = i % w;
    if (x > 0) push(i - 1);
    if (x < w - 1) push(i + 1);
    if (i >= w) push(i - w);
    if (i < w * (h - 1)) push(i + w);
  }
  const mask = new Uint8Array(w * h);
  for (let i = 0; i < w * h; i++) mask[i] = outside[i] ? 0 : 1;
  return mask;
}

/**
 * Draws the scene onto `ctx`, `s` canvas pixels per image pixel.
 * `transparent` leaves the background clear (for the export, whose alpha
 * becomes the game's transparency) instead of the key green.
 * `cache` (an object) keeps the void mask between draws of unchanged edits.
 */
export function drawOverview(ctx, scene, edits, s, { transparent = false, cache = null, selectedLabel = null, flagIcons = null, view = null, overlay = null } = {}) {
  const w = scene.width * s;
  const h = scene.height * s;
  // `view`: draw only a window of the map, its top-left at image pixel
  // (ox, oy), onto a canvas cw x ch (the page zoomed in). Scratch layers
  // are the canvas's size, not the whole map's at that scale.
  const vx = view ? view.ox * s : 0;
  const vy = view ? view.oy * s : 0;
  const cw = view ? view.cw : Math.ceil(w);
  const ch = view ? view.ch : Math.ceil(h);
  ctx.save();
  ctx.clearRect(0, 0, cw, ch);
  ctx.translate(-vx, -vy);
  const theme = themeOf(edits);
  if (theme.paper === 'squared') {
    squaredPaper(ctx, scene, s);
  } else if (theme.paper) {
    ctx.imageSmoothingEnabled = true;
    ctx.drawImage(paper(scene, cache), 0, 0, w, h);
  } else if (!transparent) {
    ctx.fillStyle = css(scene.background);
    ctx.fillRect(0, 0, w, h);
  }

  // The void: enclosed space, black (themes that have one).
  const key = JSON.stringify([edits.areas.filter((e) => e.hidden), scene.map, theme.id]);
  let mask = !theme.voidFill ? null : cache && cache.key === key ? cache.mask : null;
  if (theme.voidFill && !mask) {
    mask = voidMask(scene, edits);
    if (cache) Object.assign(cache, { key, mask, image: null });
  }
  let maskImage = theme.voidFill ? cache?.image : null;
  if (theme.voidFill && !maskImage) {
    maskImage = layer(scene.width, scene.height);
    const mctx = maskImage.getContext('2d');
    const img = mctx.createImageData(scene.width, scene.height);
    for (let i = 0; i < mask.length; i++) {
      if (!mask[i]) continue;
      const v = theme.voidColour || scene.void;
      img.data[i * 4] = v[0];
      img.data[i * 4 + 1] = v[1];
      img.data[i * 4 + 2] = v[2];
      img.data[i * 4 + 3] = 255;
    }
    mctx.putImageData(img, 0, 0);
    if (cache) cache.image = maskImage;
  }
  ctx.imageSmoothingEnabled = true;
  if (maskImage) ctx.drawImage(maskImage, 0, 0, w, h);

  // Floors, lowest first, on their own layer so water can be clipped to them.
  const floors = layer(cw, ch);
  const f = floors.getContext('2d');
  f.translate(-vx, -vy);
  f.lineJoin = 'round';
  // A theme's outline: every floor stroked wide first, then filled over, so
  // only the line round the outside is left.
  if (theme.outline) {
    f.strokeStyle = css(theme.outline);
    f.lineWidth = 3 * s;
    for (const face of scene.faces) {
      if (!faceColour(scene, edits, face)) continue;
      polygon(f, face.points, s);
      f.stroke();
    }
  }
  f.lineWidth = Math.max(1, 0.8 * s);
  // Water goes in with the floors by height, painted only over what is
  // already drawn (the floors below it), so a bridge drawn later covers it.
  const water = edits.show.water ? [...scene.water].sort((a, b) => a.z - b.z) : [];
  let nextWater = 0;
  const waterBelow = (z) => {
    f.globalCompositeOperation = 'source-atop';
    f.fillStyle = css(theme.water);
    for (; nextWater < water.length && water[nextWater].z < z; nextWater++) {
      polygon(f, water[nextWater].points, s);
      f.fill();
    }
    f.globalCompositeOperation = 'source-over';
  };
  for (const face of scene.faces) {
    const colour = faceColour(scene, edits, face);
    if (!colour) continue;
    if (nextWater < water.length && water[nextWater].z < face.z) waterBelow(face.z);
    f.fillStyle = css(colour);
    f.strokeStyle = css(colour);
    polygon(f, face.points, s);
    f.fill();
    // Hides the hairline seams between neighbouring faces.
    f.stroke();
  }
  waterBelow(Infinity);
  // A theme can let its paper show through the floors a little.
  ctx.globalAlpha = theme.floorAlpha ?? 1;
  ctx.drawImage(floors, vx, vy);
  ctx.globalAlpha = 1;

  // Lines round every area, and the frame's grid, on themes that have them.
  if (theme.edges) {
    const visible = (face) => faceColour(scene, edits, face) != null;
    const edgeKey = JSON.stringify([edits.areas.filter((e) => e.hidden), scene.map]);
    ctx.drawImage(areaEdges(scene, visible, cache, edgeKey), 0, 0, w, h);
  }
  if (theme.frame) grid(ctx, scene, s, theme.ink?.grid);

  // Capture zones: a yellow rim just outside each zone's footprint.
  if (edits.show.capZones) {
    const rim = layer(cw, ch);
    const r = rim.getContext('2d');
    r.translate(-vx, -vy);
    r.fillStyle = r.strokeStyle = 'rgb(255,221,0)';
    r.lineJoin = 'round';
    r.lineWidth = 6 * s;
    for (const zone of scene.cap_zones) {
      for (const pts of zone) {
        polygon(r, pts, s);
        r.fill();
        r.stroke();
      }
    }
    r.globalCompositeOperation = 'destination-out';
    for (const zone of scene.cap_zones) {
      for (const pts of zone) {
        polygon(r, pts, s);
        r.fill();
      }
    }
    ctx.drawImage(rim, vx, vy);
  }

  // Spawn protection: the zone's floor filled and the line where you walk
  // into it, in the team's colour or hazard stripes, kept to the floors
  // (the zone itself runs through walls and empty space).
  if (edits.show.spawnProtection && scene.spawn_zones?.length) {
    const look = { ...SPAWN_PROTECTION, ...(edits.spawnProtection || {}) };
    const onFloors = (paint) => {
      const l = layer(cw, ch);
      const c = l.getContext('2d');
      c.translate(-vx, -vy);
      paint(c);
      c.globalAlpha = 1;
      c.setLineDash([]);
      c.globalCompositeOperation = 'destination-in';
      c.drawImage(floors, vx, vy);
      ctx.drawImage(l, vx, vy);
    };
    for (const zone of scene.spawn_zones) {
      const colour = zone.team === 'axis' ? look.axis : look.allies;
      if (look.fill !== 'none' && zone.polygons?.length) {
        onFloors((c) => {
          if (look.fill === 'hatch') {
            const tile = layer(Math.max(4, Math.round(10 * s)), Math.max(4, Math.round(10 * s)));
            const t = tile.getContext('2d');
            t.strokeStyle = colour;
            t.lineWidth = Math.max(1, 1.6 * s);
            t.beginPath();
            t.moveTo(0, tile.height);
            t.lineTo(tile.width, 0);
            t.stroke();
            c.fillStyle = c.createPattern(tile, 'repeat');
            c.globalAlpha = 0.7;
          } else {
            c.fillStyle = colour;
            c.globalAlpha = 0.28;
          }
          for (const pts of zone.polygons) {
            polygon(c, pts, s);
            c.fill();
          }
        });
      }
      if (look.line !== 'none') {
        onFloors((c) => {
          c.lineCap = 'butt';
          c.lineWidth = 3 * s;
          const strokeEdges = () => {
            c.beginPath();
            for (const [a, b] of zone.edges) {
              c.moveTo(a[0] * s, a[1] * s);
              c.lineTo(b[0] * s, b[1] * s);
            }
            c.stroke();
          };
          if (look.line === 'hazard') {
            c.strokeStyle = look.stripe2;
            strokeEdges();
            c.strokeStyle = look.stripe1;
            c.setLineDash([5 * s, 5 * s]);
            strokeEdges();
          } else {
            c.strokeStyle = colour;
            strokeEdges();
          }
        });
      }
    }
  }

  // Slopes too steep to stand on that a player still gets onto: an outline
  // only, since he slides off them.
  if (edits.show.slopes && scene.slope_edges?.length) {
    ctx.save();
    ctx.lineCap = 'round';
    ctx.lineWidth = 1.5 * s;
    ctx.strokeStyle = 'rgba(30,30,28,0.85)';
    ctx.beginPath();
    for (const [a, b] of scene.slope_edges) {
      ctx.moveTo(a[0] * s, a[1] * s);
      ctx.lineTo(b[0] * s, b[1] * s);
    }
    ctx.stroke();
    ctx.restore();
  }

  // Floors that break: a dotted outline, black on white so it shows on
  // any theme.
  if (edits.show.breakables && scene.breakable_edges?.length) {
    ctx.save();
    ctx.lineCap = 'butt';
    ctx.lineWidth = 2.5 * s;
    const strokeAll = () => {
      ctx.beginPath();
      for (const [a, b] of scene.breakable_edges) {
        ctx.moveTo(a[0] * s, a[1] * s);
        ctx.lineTo(b[0] * s, b[1] * s);
      }
      ctx.stroke();
    };
    ctx.strokeStyle = '#fff';
    strokeAll();
    ctx.strokeStyle = '#000';
    ctx.setLineDash([4 * s, 4 * s]);
    strokeAll();
    ctx.restore();
  }

  const text = (label, x, y, size, align = 'left') => {
    ctx.font = `bold ${size * s}px Arial, sans-serif`;
    ctx.textAlign = align;
    ctx.textBaseline = 'middle';
    ctx.lineJoin = 'round';
    ctx.lineWidth = Math.max(2, (size / 7.5) * s);
    ctx.strokeStyle = '#000';
    ctx.fillStyle = '#fff';
    ctx.strokeText(label, x * s, y * s);
    ctx.fillText(label, x * s, y * s);
  };

  if (edits.show.spawns) {
    for (const [list, colour] of [[scene.allies, 'rgb(40,200,60)'], [scene.axis, 'rgb(220,40,40)']]) {
      ctx.fillStyle = colour;
      ctx.strokeStyle = '#000';
      ctx.lineWidth = s;
      for (const m of list) {
        ctx.beginPath();
        ctx.arc(m.at[0] * s, m.at[1] * s, 3 * s, 0, Math.PI * 2);
        ctx.fill();
        ctx.stroke();
      }
    }
  }
  if (edits.show.spawnLabels) {
    for (const spot of spawnNameSpots(scene, edits)) text(spot.name, spot.at[0], spot.at[1], 15, 'center');
  }
  if (edits.show.flags) {
    for (const flag of scene.flags) {
      const [x, y] = flag.at;
      ctx.fillStyle = '#fff';
      ctx.strokeStyle = '#000';
      ctx.lineWidth = 2 * s;
      ctx.beginPath();
      ctx.arc(x * s, y * s, 6 * s, 0, Math.PI * 2);
      ctx.fill();
      ctx.stroke();
    }
    // The game's own flag icons, as big as they come out on the full map
    // at the screen height asked for (preview only; see flag_icons.rs).
    if (flagIcons?.icons?.length) {
      const k = 768 / (0.625 * flagIcons.screenHeight);
      ctx.save();
      ctx.imageSmoothingEnabled = true;
      for (const icon of flagIcons.icons) {
        const [x, y] = toPixel(scene.transform, icon.world[0], icon.world[1]);
        const w = icon.width * k;
        const h = icon.height * k;
        ctx.drawImage(icon.image, (x - w / 2) * s, (y - h / 2) * s, w * s, h * s);
      }
      ctx.restore();
    }
    if (edits.show.flagLabels) {
      for (const flag of scene.flags) {
        const name = flagName(edits, flag);
        if (!name) continue;
        const [lx, ly] = flagLabelSpot(scene, edits, flag);
        text(name, lx, ly, 15, 'center');
      }
    }
  }
  for (const label of edits.labels) {
    const [x, y] = toPixel(scene.transform, label.world[0], label.world[1]);
    text(label.text || '', x, y, label.size || 15, 'center');
    if (label.id === selectedLabel) {
      ctx.font = `bold ${(label.size || 15) * s}px Arial, sans-serif`;
      const tw = ctx.measureText(label.text || '').width;
      const th = (label.size || 15) * s;
      ctx.strokeStyle = 'rgb(255,221,0)';
      ctx.lineWidth = Math.max(1, s);
      ctx.setLineDash([4 * s, 3 * s]);
      ctx.strokeRect(x * s - tw / 2 - 3 * s, y * s - th / 2 - 2 * s, tw + 6 * s, th + 4 * s);
      ctx.setLineDash([]);
    }
  }
  if (theme.frame) frame(ctx, scene, s, theme.ink);
  // Off paper the card is solid: over the transparent background a
  // see-through card would let the game show through it.
  if (titleShown(edits)) {
    const look = theme.card || (theme.paper ? null : { fill: 'rgb(236,228,206)' });
    titleCard(ctx, scene, s, mapTitle(scene.map), titleCredit(scene, edits), edits.titleOffset, look);
  }
  // The page's editing aids (overview_overlay.js); never in an export.
  if (overlay) drawOverlay(ctx, scene, s, overlay);
  ctx.restore();
}

/** The label under an image pixel, using a rough box for its text. */
export function labelAt(scene, edits, x, y) {
  for (let i = edits.labels.length - 1; i >= 0; i--) {
    const label = edits.labels[i];
    const [lx, ly] = toPixel(scene.transform, label.world[0], label.world[1]);
    const size = label.size || 15;
    const half = Math.max(12, ((label.text || '').length * size * 0.6) / 2);
    if (Math.abs(x - lx) <= half && Math.abs(y - ly) <= size * 0.7) return label;
  }
  return null;
}

/**
 * The finished image: drawn three times larger and scaled down, background
 * transparent. Returns { width, height, rgba } with rgba a Uint8ClampedArray;
 * fully transparent pixels are the key green, as the game's loader expects.
 */
export function renderExport(scene, edits) {
  const k = 3;
  const big = layer(scene.width * k, scene.height * k);
  drawOverview(big.getContext('2d'), scene, edits, k, { transparent: true });
  const small = layer(scene.width, scene.height);
  const sctx = small.getContext('2d');
  sctx.imageSmoothingEnabled = true;
  sctx.imageSmoothingQuality = 'high';
  sctx.drawImage(big, 0, 0, scene.width, scene.height);
  const rgba = sctx.getImageData(0, 0, scene.width, scene.height).data;
  for (let i = 0; i < rgba.length; i += 4) {
    if (rgba[i + 3] < 8) {
      rgba[i] = 0;
      rgba[i + 1] = 255;
      rgba[i + 2] = 0;
      rgba[i + 3] = 0;
    }
  }
  return { width: scene.width, height: scene.height, rgba };
}

/**
 * The high-quality copy for DoD Studio's hook: drawn `k` times the game's size
 * (4096x3072 by default), background transparent and keyed green.
 */
export function renderHd(scene, edits, k = 4) {
  const c = layer(scene.width * k, scene.height * k);
  const ctx = c.getContext('2d');
  drawOverview(ctx, scene, edits, k, { transparent: true });
  const rgba = ctx.getImageData(0, 0, scene.width * k, scene.height * k).data;
  for (let i = 0; i < rgba.length; i += 4) {
    if (rgba[i + 3] < 8) {
      rgba[i] = 0;
      rgba[i + 1] = 255;
      rgba[i + 2] = 0;
      rgba[i + 3] = 0;
    }
  }
  return { width: scene.width * k, height: scene.height * k, rgba };
}

/** Base64 of a byte array, in chunks (a 3 MB spread would overflow the stack). */
export function toBase64(bytes) {
  let binary = '';
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode.apply(null, bytes.subarray(i, i + chunk));
  }
  return btoa(binary);
}
