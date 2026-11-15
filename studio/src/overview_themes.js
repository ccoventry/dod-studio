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
//   paper: true|'squared' aged (true) or squared paper under everything
//                         (overview_paper.js)
//   floorAlpha: number    how opaque the floors are (1 unless set), so a
//                         paper can show through
//   edges: bool           thin dark lines round every area
//   frame: bool           the ruler frame and grid; also shows the map
//                         title by default (any theme can show it)
//   ink: { frame, light, grid }  the frame's and grid's colours, if not
//                         Classic's
//   card: { fill, border, ink }  the title card's colours, if not Classic's
//   palette(scene)        the swatches the page offers: the theme's own
//                         colours first, so a repaint can match the look

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
    palette: (scene) => scene.palette,
  },
  {
    // Untextured floors, lighter the higher they are, on the game's
    // see-through green, edged in dark grey (dod_saints2_b4e, dod_solitude2).
    id: 'grey',
    floor: (scene, face) => greyFor(scene, face.z),
    voidFill: false,
    outline: [62, 62, 60],
    // The floors' own greys, low to high, then the edge and the water.
    palette: () => [
      ...[0, 0.17, 0.33, 0.5, 0.67, 0.83, 1].map((t) => {
        const v = Math.round(118 + t * (226 - 118));
        return [v, v, Math.round(v * 0.98)];
      }),
      [62, 62, 60],
      [92, 94, 97],
      [255, 255, 255],
    ],
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
    // Its paper and ink, then the muted colours a printed map would use.
    palette: () => [
      [240, 238, 231],
      [214, 208, 192],
      [190, 180, 158],
      [38, 36, 33],
      [150, 182, 196],
      [120, 70, 55],
      [96, 110, 70],
      [70, 86, 120],
      [176, 140, 70],
      [255, 255, 255],
    ],
    paper: true,
    edges: true,
    frame: true,
  },
  {
    // A plain plan drawn on engineering paper: squared paper in a ruler
    // frame, the floors one flat tone with the paper's squares faintly
    // through them, outlined in ink.
    id: 'gridpaper',
    floor: () => [222, 228, 236],
    voidFill: false,
    outline: [36, 54, 88],
    water: [150, 192, 228],
    paper: 'squared',
    floorAlpha: 0.86,
    frame: true,
    ink: { frame: 'rgb(36,54,88)', light: 'rgb(236,241,248)', grid: 'rgba(36,54,88,0.4)' },
    card: { fill: 'rgba(252,252,248,0.96)', border: 'rgb(36,54,88)', ink: 'rgb(28,40,66)' },
    // Its floor tone, paper and ink, then coloured pencils and a highlighter.
    palette: () => [
      [222, 228, 236],
      [250, 250, 246],
      [36, 54, 88],
      [150, 192, 228],
      [200, 62, 52],
      [62, 132, 78],
      [74, 118, 196],
      [128, 90, 168],
      [236, 196, 70],
      [150, 150, 150],
    ],
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
