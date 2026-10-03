// overview_overlay.js — what the Overviews page draws over the map while
// editing, never into the saved image (#371):
//
// - "Show areas": a faint outline round every area (or, with Alt held,
//   round every floor piece), to see how the map is divided;
// - the hover highlight: a bold outline and a light wash on exactly what a
//   click would change (the area, or one piece), so a click that would
//   only catch a sliver shows as a sliver first.
//
// Outlines are the edges of an area's pieces that no other of its pieces
// shares, in image pixels, worked out once per scene.

const keyOf = (p) => `${Math.round(p[0] * 20)},${Math.round(p[1] * 20)}`;

/** Per area id, its outline segments: piece edges used once in the area. */
function areaOutlines(scene) {
  if (scene._areaOutlines) return scene._areaOutlines;
  const byArea = new Map();
  for (const face of scene.faces) {
    let edges = byArea.get(face.area);
    if (!edges) byArea.set(face.area, (edges = new Map()));
    const pts = face.points;
    for (let i = 0; i < pts.length; i++) {
      const a = pts[i];
      const b = pts[(i + 1) % pts.length];
      const ka = keyOf(a);
      const kb = keyOf(b);
      if (ka === kb) continue;
      const key = ka < kb ? `${ka}|${kb}` : `${kb}|${ka}`;
      const seen = edges.get(key);
      if (seen) seen.count += 1;
      else edges.set(key, { count: 1, a, b });
    }
  }
  const out = new Map();
  for (const [area, edges] of byArea) {
    out.set(area, [...edges.values()].filter((e) => e.count === 1).map((e) => [e.a, e.b]));
  }
  scene._areaOutlines = out;
  return out;
}

function strokeSegments(ctx, segments, s) {
  ctx.beginPath();
  for (const [a, b] of segments) {
    ctx.moveTo(a[0] * s, a[1] * s);
    ctx.lineTo(b[0] * s, b[1] * s);
  }
  ctx.stroke();
}

function tracePolygon(ctx, points, s) {
  ctx.beginPath();
  points.forEach(([x, y], i) => (i ? ctx.lineTo(x * s, y * s) : ctx.moveTo(x * s, y * s)));
  ctx.closePath();
}

/**
 * Draws the editing overlay on `ctx` (already placed for the view), at
 * scale `s`. `outlines`: null, 'areas' or 'pieces'. `hover`: null, or
 * `{ kind: 'area' | 'piece', face, area, hide }` (`hide`: the click would
 * hide it, false: show it again).
 */
export function drawOverlay(ctx, scene, s, { outlines = null, hover = null } = {}) {
  ctx.save();
  ctx.lineJoin = 'round';
  ctx.lineCap = 'round';
  if (outlines === 'pieces') {
    ctx.strokeStyle = 'rgba(20,20,20,0.45)';
    ctx.lineWidth = Math.max(1, 0.6 * s);
    for (const face of scene.faces) {
      tracePolygon(ctx, face.points, s);
      ctx.stroke();
    }
  } else if (outlines === 'areas') {
    ctx.strokeStyle = 'rgba(20,20,20,0.55)';
    ctx.lineWidth = Math.max(1, 0.9 * s);
    for (const segments of areaOutlines(scene).values()) strokeSegments(ctx, segments, s);
  }
  if (hover) {
    // Light wash, then a bold outline in white over black so it shows on
    // any colour; red when the click would hide.
    const tint = hover.kind === 'area' && hover.hide === true ? 'rgba(255,60,60,0.30)' : 'rgba(255,255,255,0.30)';
    const faces = hover.kind === 'piece' ? [hover.face] : scene.faces.filter((f) => f.area === hover.area);
    ctx.fillStyle = tint;
    for (const face of faces) {
      tracePolygon(ctx, face.points, s);
      ctx.fill();
    }
    const segments = hover.kind === 'piece'
      ? hover.face.points.map((p, i, pts) => [p, pts[(i + 1) % pts.length]])
      : areaOutlines(scene).get(hover.area) || [];
    ctx.strokeStyle = 'rgba(0,0,0,0.85)';
    ctx.lineWidth = Math.max(2, 3 * s);
    strokeSegments(ctx, segments, s);
    ctx.strokeStyle = hover.kind === 'area' && hover.hide === true ? 'rgb(255,90,90)' : 'rgb(255,255,255)';
    ctx.lineWidth = Math.max(1, 1.4 * s);
    strokeSegments(ctx, segments, s);
  }
  ctx.restore();
}
