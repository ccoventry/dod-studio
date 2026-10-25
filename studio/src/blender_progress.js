// blender_progress.js
// What the Blender page's progress line says and what its bar shows (#956):
// each step names itself with the block sliding until there are frames to
// count, then Render's bar fills by frames. Each view is `{ text, bar }`,
// `bar` as `step_bar.js`'s `drawBar` takes it.
import { STRINGS } from './strings.js';
import { durationText } from './time_format.js';

/// A step that hasn't counted any frames yet, `secs` after it started: the
/// clock joins the line after its first second.
export function blenderStartView(kind, secs = 0) {
  const line = STRINGS.BLENDER.STARTING[kind] || STRINGS.BLENDER.STARTING.render;
  return {
    text: secs >= 1 ? STRINGS.BLENDER.soFar(line, durationText(secs)) : line,
    bar: { slide: true },
  };
}

/// A `blender_progress` event that counts frames (`done` of `total`).
export function blenderFramesView(p, secs) {
  return {
    text: STRINGS.BLENDER.frames(p.done, p.total, p.frame_secs ? durationText(p.frame_secs) : '', durationText(secs)),
    bar: { pct: Math.min(100, Math.round((p.done / p.total) * 100)) },
  };
}
