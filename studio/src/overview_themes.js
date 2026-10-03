// overview_themes.js — how an overview looks (#371): the colours a floor
// starts with, what goes behind and around the floors, and water. The edits
// (a recoloured area or piece, hidden areas, labels) sit on top of any theme.
//
// Each theme:
//   floor(scene, face, area) -> [r, g, b]   a floor's colour before edits
//   voidFill: bool        black for enclosed space and a rim round the floors
//   outline: [r,g,b]|null a line round the floors' outer edge instead
//   water: [r, g, b]
//   voidColour: [r,g,b]   the enclosed space's colour, if not the scene's
//   paper: bool           aged paper under everything (overview_paper.js)
//   edges: bool           thin dark lines round every area
//   frame: bool           the ruler frame, grid and title card

const clamp01 = (v) => Math.max(0, Math.min(1, v));

/** The floors' height range (5th to 95th percentile), kept on the scene. */
function heightRange(scene) {
  if (!scene._heights) {
    const zs = scene.faces.map((f) => f.z).sort((a, b) => a - b);
    const at = (q) => zs[Math.min(zs.length - 1, Math.max(0, Math.round(q * (zs.length - 1))))] ?? 0;
    scene._heights = [at(0.05), at(0.95)];
  }
  return scene._heights;
}

/** A grey from dark (low) to light (high) for a floor's height. */
function greyFor(scene, z) {
  const [lo, hi] = heightRange(scene);
  const t = hi > lo ? clamp01((z - lo) / (hi - lo)) : 0.5;
  const v = Math.round(118 + t * (226 - 118));
  return [v, v, Math.round(v * 0.98)];
}

export const THEMES = [
  {
    // The user's hand-made dod_harrington look: a colour per area.
    id: 'colours',
    floor: (scene, face, area) => (area ? area.colour : [145, 145, 130]),
    voidFill: true,
    outline: null,
    water: [64, 208, 213],
  },
  {
    // Untextured floors, lighter the higher they are, on the game's
    // see-through green, edged in dark grey (dod_saints2_b4e, dod_solitude2).
    id: 'grey',
    floor: (scene, face) => greyFor(scene, face.z),
    voidFill: false,
    outline: [62, 62, 60],
    // Grey too, darker than the lowest floor (118), so it still reads as water.
    water: [92, 94, 97],
  },
  {
    // Valve's own overviews (anzio, chemille, donner...): aged paper in a
    // ruler frame, buildings you can't enter as black blocks, the rest pale
    // and outlined, a title card.
    id: 'classic',
    floor: (scene, face, area) => (area?.indoor ? [240, 238, 231] : [214, 208, 192]),
    voidFill: true,
    voidColour: [38, 36, 33],
    outline: null,
    water: [150, 182, 196],
    paper: true,
    edges: true,
    frame: true,
  },
];

/** The title card's text for a map: its name without "dod_" or a version. */
export function mapTitle(map) {
  let name = String(map || '').replace(/^dod_/i, '').replace(/^(cevo|ktp)_/i, '');
  // Versions and league tags off the end, as many as there are
  // (railroad2_s10a, cevo_russka_mtek, anjou_a4_v04).
  const tail = /_(v?\d+[a-z]?|[a-z]\d+[a-z]?|beta\d*|final\d*|test\d*|mtek|gg|ktp\d*)$/i;
  while (tail.test(name)) name = name.replace(tail, '');
  return name.replace(/_/g, ' ').toUpperCase();
}

/** The theme an edits object asks for, or the first. */
export function themeOf(edits) {
  return THEMES.find((t) => t.id === edits?.theme) || THEMES[0];
}
