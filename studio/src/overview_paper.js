// overview_paper.js — the printed-map pieces of the Classic and Grid paper
// themes (#371): aged or squared paper, the ruler frame with A-G across and
// 1-5 down, the faint grid, a title card (any theme), and thin dark lines
// round every area. All drawn by the program (nothing copied from the game's
// own overviews), at the game's 1024x768 and scaled by `s` like the rest of
// the drawing.

/** A small seeded random generator, so a map's paper is the same each time. */
function random(seedText) {
  let a = 2166136261;
  for (const c of seedText) a = Math.imul(a ^ c.charCodeAt(0), 16777619);
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function canvasOf(width, height) {
  return typeof OffscreenCanvas !== 'undefined'
    ? new OffscreenCanvas(width, height)
    : Object.assign(document.createElement('canvas'), { width, height });
}

/** Aged paper at 1024x768: a warm base, soft stains, grain, darker edges. */
export function paper(scene, cache) {
  if (cache?.paper && cache.paperFor === scene.map) return cache.paper;
  const w = scene.width;
  const h = scene.height;
  const c = canvasOf(w, h);
  const ctx = c.getContext('2d');
  const rnd = random(scene.map);
  ctx.fillStyle = 'rgb(228,216,184)';
  ctx.fillRect(0, 0, w, h);
  // Stains: big soft blotches, lighter and darker.
  for (let i = 0; i < 26; i++) {
    const x = rnd() * w;
    const y = rnd() * h;
    const r = 60 + rnd() * 220;
    const dark = rnd() < 0.55;
    const g = ctx.createRadialGradient(x, y, 0, x, y, r);
    g.addColorStop(0, dark ? `rgba(150,120,70,${0.05 + rnd() * 0.1})` : `rgba(255,250,235,${0.05 + rnd() * 0.12})`);
    g.addColorStop(1, 'rgba(0,0,0,0)');
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, w, h);
  }
  // Darker towards the edges.
  const v = ctx.createRadialGradient(w / 2, h / 2, Math.min(w, h) * 0.35, w / 2, h / 2, Math.max(w, h) * 0.7);
  v.addColorStop(0, 'rgba(0,0,0,0)');
  v.addColorStop(1, 'rgba(90,60,20,0.28)');
  ctx.fillStyle = v;
  ctx.fillRect(0, 0, w, h);
  // Grain.
  const img = ctx.getImageData(0, 0, w, h);
  for (let i = 0; i < img.data.length; i += 4) {
    const n = (rnd() - 0.5) * 14;
    img.data[i] += n;
    img.data[i + 1] += n;
    img.data[i + 2] += n * 0.8;
  }
  ctx.putImageData(img, 0, 0);
  if (cache) Object.assign(cache, { paper: c, paperFor: scene.map });
  return c;
}

/**
 * Squared paper, drawn straight onto `ctx` at its scale so the lines stay
 * sharp: off-white, a fine square every 8 pixels and a stronger line every
 * fifth, as engineering paper has.
 */
export function squaredPaper(ctx, scene, s) {
  const w = scene.width;
  const h = scene.height;
  ctx.save();
  ctx.fillStyle = 'rgb(250,250,246)';
  ctx.fillRect(0, 0, w * s, h * s);
  const step = 8;
  for (const [every, colour, width] of [[1, 'rgba(70,130,190,0.16)', 0.6], [5, 'rgba(70,130,190,0.34)', 1]]) {
    ctx.strokeStyle = colour;
    ctx.lineWidth = Math.max(width, width * s);
    ctx.beginPath();
    for (let x = 0; x <= w; x += step * every) {
      ctx.moveTo(x * s, 0);
      ctx.lineTo(x * s, h * s);
    }
    for (let y = 0; y <= h; y += step * every) {
      ctx.moveTo(0, y * s);
      ctx.lineTo(w * s, y * s);
    }
    ctx.stroke();
  }
  ctx.restore();
}

/** The frame's width, in 1024x768 pixels. */
export const FRAME = 16;

/** The Classic theme's ink: a black frame with paper-coloured cells, and a
 *  brown grid. A theme can bring its own (overview_themes.js `ink`). */
const CLASSIC_INK = { frame: 'rgb(24,22,20)', light: 'rgb(222,212,186)', grid: 'rgba(110,90,55,0.35)' };
const COLUMNS = 'ABCDEFG';
const ROWS = 5;

/** The faint grid the ruler letters and numbers name. */
export function grid(ctx, scene, s, colour = CLASSIC_INK.grid) {
  const w = scene.width;
  const h = scene.height;
  ctx.save();
  ctx.strokeStyle = colour;
  ctx.lineWidth = Math.max(1, s);
  ctx.setLineDash([6 * s, 5 * s]);
  for (let i = 1; i < COLUMNS.length; i++) {
    const x = (FRAME + ((w - 2 * FRAME) * i) / COLUMNS.length) * s;
    ctx.beginPath();
    ctx.moveTo(x, FRAME * s);
    ctx.lineTo(x, (h - FRAME) * s);
    ctx.stroke();
  }
  for (let j = 1; j < ROWS; j++) {
    const y = (FRAME + ((h - 2 * FRAME) * j) / ROWS) * s;
    ctx.beginPath();
    ctx.moveTo(FRAME * s, y);
    ctx.lineTo((w - FRAME) * s, y);
    ctx.stroke();
  }
  ctx.restore();
}

/** The ruler frame: alternating light and dark cells, A-G and 1-5. */
export function frame(ctx, scene, s, ink = CLASSIC_INK) {
  const w = scene.width;
  const h = scene.height;
  const dark = ink?.frame || CLASSIC_INK.frame;
  const light = ink?.light || CLASSIC_INK.light;
  ctx.save();
  ctx.fillStyle = dark;
  ctx.fillRect(0, 0, w * s, FRAME * s);
  ctx.fillRect(0, (h - FRAME) * s, w * s, FRAME * s);
  ctx.fillRect(0, 0, FRAME * s, h * s);
  ctx.fillRect((w - FRAME) * s, 0, FRAME * s, h * s);
  ctx.font = `bold ${10 * s}px Arial, sans-serif`;
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  const bar = 6;
  const cells = (n, len, place) => {
    for (let i = 0; i < n; i++) {
      const a = FRAME + ((len - 2 * FRAME) * i) / n;
      const b = FRAME + ((len - 2 * FRAME) * (i + 1)) / n;
      place(i, a, b);
    }
  };
  cells(COLUMNS.length, w, (i, a, b) => {
    for (const y of [(FRAME - bar) / 2, h - FRAME + (FRAME - bar) / 2]) {
      if (i % 2 === 1) {
        ctx.fillStyle = light;
        ctx.fillRect(a * s, y * s, (b - a) * s, bar * s);
      }
      ctx.fillStyle = i % 2 === 1 ? dark : light;
      ctx.fillText(COLUMNS[i], ((a + b) / 2) * s, (y + bar / 2) * s);
    }
  });
  cells(ROWS, h, (i, a, b) => {
    for (const x of [(FRAME - bar) / 2, w - FRAME + (FRAME - bar) / 2]) {
      if (i % 2 === 1) {
        ctx.fillStyle = light;
        ctx.fillRect(x * s, a * s, bar * s, (b - a) * s);
      }
      ctx.fillStyle = i % 2 === 1 ? dark : light;
      ctx.fillText(String(i + 1), (x + bar / 2) * s, ((a + b) / 2) * s);
    }
  });
  ctx.restore();
}

// A serif whose digits sit on the line (Georgia's old-style figures made
// RAILROAD2's "2" look small), until fonts can be picked.
const TITLE_FONT = "'Palatino Linotype', 'Book Antiqua', Palatino, Cambria, 'Times New Roman', serif";
const SUBTITLE_FONT = "'Courier New', monospace";

let measurer = null;
function textWidth(font, text) {
  measurer ??= canvasOf(4, 4).getContext('2d');
  measurer.font = font;
  return measurer.measureText(text).width;
}

/**
 * Where the title card goes, in image pixels: top right, inside where the
 * ruler frame would be, moved by `offset` ([dx, dy]) when it was dragged.
 * `{ x, y, w, h }`.
 */
export function titleCardBox(scene, title, subtitle, offset = null) {
  const tw = textWidth(`bold 26px ${TITLE_FONT}`, title);
  const sw = subtitle ? textWidth(`13px ${SUBTITLE_FONT}`, subtitle) : 0;
  const w = Math.max(tw, sw) + 28;
  const h = subtitle ? 62 : 44;
  return {
    x: scene.width - FRAME - 14 - w + (offset?.[0] || 0),
    y: FRAME + 14 + (offset?.[1] || 0),
    w,
    h,
  };
}

/** The Classic card: paper-coloured, a little of the paper showing through. */
const CLASSIC_CARD = { fill: 'rgba(236,228,206,0.92)', border: 'rgb(40,36,30)', ink: 'rgb(28,26,22)' };

/** A title card: the map's name, and a line under it. `look` is a theme's
 *  own `{ fill, border, ink }`, else Classic's. */
export function titleCard(ctx, scene, s, title, subtitle, offset = null, look = null) {
  if (!title) return;
  const card = { ...CLASSIC_CARD, ...(look || {}) };
  ctx.save();
  const { x, y, w: boxW, h: boxH } = titleCardBox(scene, title, subtitle, offset);
  ctx.fillStyle = card.fill;
  ctx.fillRect(x * s, y * s, boxW * s, boxH * s);
  ctx.strokeStyle = card.border;
  ctx.lineWidth = 2 * s;
  ctx.strokeRect(x * s, y * s, boxW * s, boxH * s);
  ctx.fillStyle = card.ink;
  ctx.textAlign = 'center';
  ctx.textBaseline = 'top';
  ctx.font = `bold ${26 * s}px ${TITLE_FONT}`;
  ctx.fillText(title, (x + boxW / 2) * s, (y + 8) * s);
  if (subtitle) {
    ctx.font = `${13 * s}px ${SUBTITLE_FONT}`;
    ctx.fillText(subtitle, (x + boxW / 2) * s, (y + 40) * s);
  }
  ctx.restore();
}

/**
 * Fills a polygon into `ids` (a `w`-wide grid) with `id`, sampling pixel
 * centres: no anti-aliasing, so two faces that share an edge leave no seam
 * and no blended pixel between them.
 */
function fillIds(ids, w, h, points, id, k) {
  let y0 = Infinity;
  let y1 = -Infinity;
  for (const [, y] of points) {
    y0 = Math.min(y0, y * k);
    y1 = Math.max(y1, y * k);
  }
  const top = Math.max(0, Math.ceil(y0 - 0.5));
  const bottom = Math.min(h - 1, Math.floor(y1 - 0.5));
  const xs = [];
  for (let y = top; y <= bottom; y++) {
    const cy = y + 0.5;
    xs.length = 0;
    for (let i = 0, j = points.length - 1; i < points.length; j = i++) {
      const ay = points[i][1] * k;
      const by = points[j][1] * k;
      if ((ay > cy) !== (by > cy)) {
        const ax = points[i][0] * k;
        const bx = points[j][0] * k;
        xs.push(ax + ((cy - ay) * (bx - ax)) / (by - ay));
      }
    }
    xs.sort((a, b) => a - b);
    for (let n = 0; n + 1 < xs.length; n += 2) {
      const from = Math.max(0, Math.ceil(xs[n] - 0.5));
      const to = Math.min(w - 1, Math.floor(xs[n + 1] - 0.5));
      ids.fill(id, y * w + from, y * w + to + 1);
    }
  }
}

/**
 * Thin lines round the floors and round every building (indoor area), at
 * twice the game's 1024x768: each piece is filled in its own id, and a pixel
 * whose right or lower neighbour holds another id is an edge. Outdoors is
 * one piece, as on the printed maps.
 */
export function areaEdges(scene, visible, cache, key) {
  if (cache?.edges && cache.edgesFor === key) return cache.edges;
  const k = 2;
  const w = scene.width * k;
  const h = scene.height * k;
  const ids = new Uint16Array(w * h);
  for (const face of scene.faces) {
    if (!visible(face)) continue;
    const id = scene.areas[face.area]?.indoor ? face.area + 2 : 1;
    fillIds(ids, w, h, face.points, id, k);
  }
  const mask = new Uint8Array(w * h);
  for (let y = 0; y < h - 1; y++) {
    for (let x = 0; x < w - 1; x++) {
      const i = y * w + x;
      const a = ids[i];
      if (a !== ids[i + 1] || a !== ids[i + w]) mask[i] = 1;
    }
  }
  const out = canvasOf(w, h);
  const octx = out.getContext('2d');
  const img = octx.createImageData(w, h);
  for (let i = 0; i < mask.length; i++) {
    if (!mask[i]) continue;
    img.data[i * 4] = 34;
    img.data[i * 4 + 1] = 30;
    img.data[i * 4 + 2] = 26;
    img.data[i * 4 + 3] = 235;
  }
  octx.putImageData(img, 0, 0);
  if (cache) Object.assign(cache, { edges: out, edgesFor: key });
  return out;
}
