// overview_draw.js — drawing an overview scene (native::overview::scene) with
// the page's edits applied, onto any canvas at any scale. The Overviews page
// draws it on screen with this, and the export draws it three times larger
// and scales it down, so the file is what the page showed.

/** A fresh, empty set of edits. */
export function emptyEdits() {
  return {
    version: 1,
    // Keyed by the area's anchor (a world point), so they survive a rebuild.
    areas: [],
    // Keyed by the BSP face index.
    faces: [],
    // Text placed by hand, at a world point.
    labels: [],
    // Flag names typed over the game's, keyed by the flag's world position.
    flagNames: [],
    show: {
      spawns: true,
      spawnLabels: true,
      flags: true,
      flagLabels: true,
      capZones: true,
      stairs: true,
      water: true,
    },
    format: 'tga',
    target: 'addon',
  };
}

/** Fills in anything an older or partial edits file lacks. */
export function normaliseEdits(raw) {
  const base = emptyEdits();
  if (!raw || typeof raw !== 'object') return base;
  return {
    ...base,
    ...raw,
    areas: Array.isArray(raw.areas) ? raw.areas : [],
    faces: Array.isArray(raw.faces) ? raw.faces : [],
    labels: Array.isArray(raw.labels) ? raw.labels : [],
    flagNames: Array.isArray(raw.flagNames) ? raw.flagNames : [],
    show: { ...base.show, ...(raw.show || {}) },
  };
}

const near = (a, b, d) => Math.abs(a[0] - b[0]) <= d && Math.abs(a[1] - b[1]) <= d;

/** The edit for an area, matched by its anchor. */
export function areaEdit(edits, area) {
  return edits.areas.find((e) => near(e.at, area.anchor, 1));
}

/** Changes one area's edit; `patch` null removes it. */
export function setAreaEdit(edits, area, patch) {
  const rest = edits.areas.filter((e) => !near(e.at, area.anchor, 1));
  if (!patch) return { ...edits, areas: rest };
  const old = areaEdit(edits, area) || { at: area.anchor };
  return { ...edits, areas: [...rest, { ...old, ...patch, at: area.anchor }] };
}

export function setFaceColour(edits, face, colour) {
  const rest = edits.faces.filter((e) => e.face !== face.face);
  return { ...edits, faces: colour ? [...rest, { face: face.face, colour }] : rest };
}

export function flagName(edits, flag) {
  const typed = edits.flagNames.find((e) => near(e.at, flag.world, 1));
  return typed ? typed.name : flag.name;
}

export function setFlagName(edits, flag, name) {
  const rest = edits.flagNames.filter((e) => !near(e.at, flag.world, 1));
  if (name == null || name === flag.name) return { ...edits, flagNames: rest };
  return { ...edits, flagNames: [...rest, { at: [flag.world[0], flag.world[1]], name }] };
}

/** The colour a face is drawn in, or null when its area is hidden. */
export function faceColour(scene, edits, face) {
  const area = scene.areas[face.area];
  const areaChange = area ? areaEdit(edits, area) : null;
  if (areaChange?.hidden) return null;
  const own = edits.faces.find((e) => e.face === face.face);
  if (own) return own.colour;
  if (areaChange?.colour) return areaChange.colour;
  if (face.stairs && edits.show.stairs) return [255, 255, 255];
  return area ? area.colour : [145, 145, 130];
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

const css = (c) => `rgb(${c[0]},${c[1]},${c[2]})`;

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
export function drawOverview(ctx, scene, edits, s, { transparent = false, cache = null, selectedLabel = null } = {}) {
  const w = scene.width * s;
  const h = scene.height * s;
  ctx.save();
  ctx.clearRect(0, 0, w, h);
  if (!transparent) {
    ctx.fillStyle = css(scene.background);
    ctx.fillRect(0, 0, w, h);
  }

  // The void: enclosed space, black.
  const key = JSON.stringify([edits.areas.filter((e) => e.hidden), scene.map]);
  let mask = cache && cache.key === key ? cache.mask : null;
  if (!mask) {
    mask = voidMask(scene, edits);
    if (cache) Object.assign(cache, { key, mask, image: null });
  }
  let maskImage = cache?.image;
  if (!maskImage) {
    maskImage = layer(scene.width, scene.height);
    const mctx = maskImage.getContext('2d');
    const img = mctx.createImageData(scene.width, scene.height);
    for (let i = 0; i < mask.length; i++) {
      if (!mask[i]) continue;
      img.data[i * 4] = scene.void[0];
      img.data[i * 4 + 1] = scene.void[1];
      img.data[i * 4 + 2] = scene.void[2];
      img.data[i * 4 + 3] = 255;
    }
    mctx.putImageData(img, 0, 0);
    if (cache) cache.image = maskImage;
  }
  ctx.imageSmoothingEnabled = true;
  ctx.drawImage(maskImage, 0, 0, w, h);

  // Floors, lowest first, on their own layer so water can be clipped to them.
  const floors = layer(Math.ceil(w), Math.ceil(h));
  const f = floors.getContext('2d');
  f.lineJoin = 'round';
  f.lineWidth = Math.max(1, 0.8 * s);
  for (const face of scene.faces) {
    const colour = faceColour(scene, edits, face);
    if (!colour) continue;
    f.fillStyle = css(colour);
    f.strokeStyle = css(colour);
    polygon(f, face.points, s);
    f.fill();
    // Hides the hairline seams between neighbouring faces.
    f.stroke();
  }
  if (edits.show.water) {
    f.globalCompositeOperation = 'source-atop';
    f.fillStyle = 'rgb(64,208,213)';
    for (const pts of scene.water) {
      polygon(f, pts, s);
      f.fill();
    }
    f.globalCompositeOperation = 'source-over';
  }
  ctx.drawImage(floors, 0, 0);

  // Capture zones: a yellow rim just outside each zone's footprint.
  if (edits.show.capZones) {
    const rim = layer(Math.ceil(w), Math.ceil(h));
    const r = rim.getContext('2d');
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
    ctx.drawImage(rim, 0, 0);
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
    for (const l of spawnLabels(scene)) text(`${l.team} spawn`, l.at[0], l.at[1] + 7, 15, 'center');
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
      if (edits.show.flagLabels) {
        const name = flagName(edits, flag);
        if (name) {
          ctx.font = `bold ${15 * s}px Arial, sans-serif`;
          const right = x + 10 + ctx.measureText(name).width / s < scene.width - 4;
          text(name, right ? x + 10 : x - 10, y, 15, right ? 'left' : 'right');
        }
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

/** Base64 of a byte array, in chunks (a 3 MB spread would overflow the stack). */
export function toBase64(bytes) {
  let binary = '';
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode.apply(null, bytes.subarray(i, i + chunk));
  }
  return btoa(binary);
}
