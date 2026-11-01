// take_renders.js
//
// Every video rendered for the project (#1264), kept in the project file so a
// deleted take still lists its videos and a whole project can go to VEGAS.
//
// Same pattern as take_record.js (#439): the project file's `takeIndex` stays
// `take_key -> uid[]`, and the renders live in a sibling `takeRenders` key,
// `take_key -> [{ started_unix_ms, stream, codec, custom_codec_args?, fps,
// output_path, output_size_bytes? }]`, oldest first: one entry per finished
// render, in the shape of the take folder's own history (#438), so the render
// history's line builder reads both. Older builds ignore the key (and drop it
// when they save). In memory the map rides on the take index as a
// non-enumerable property, so JSON.stringify of the index never includes it.

import { captureOf } from './take_record.js';

const RENDERS = 'takeRenders';

/** The fields a recorded render keeps. */
const RENDER_FIELDS = ['started_unix_ms', 'stream', 'codec', 'custom_codec_args', 'fps', 'output_path', 'output_size_bytes'];

/** The index's render map, made on first use. */
function renderMapOf(takeIndex) {
  if (!Object.prototype.hasOwnProperty.call(takeIndex, RENDERS)) {
    Object.defineProperty(takeIndex, RENDERS, { value: {}, enumerable: false, writable: true, configurable: true });
  }
  return takeIndex[RENDERS];
}

/** The videos recorded for `takeKey`, oldest first (empty when none). */
export function rendersOf(takeIndex, takeKey) {
  return takeIndex?.[RENDERS]?.[takeKey] || [];
}

const sameRender = (a, b) => a.started_unix_ms === b.started_unix_ms && a.stream === b.stream;

/** Whether `attempt` is a finished render with a video to record. */
function isRecordable(attempt) {
  return !!attempt && typeof attempt.output_path === 'string' && attempt.output_path !== ''
    && Number.isFinite(attempt.started_unix_ms)
    && (attempt.outcome === undefined || attempt.outcome === 'finished');
}

/**
 * Records a finished render of a take the project knows (one `takeIndex`
 * holds). An attempt already recorded (same start and stream) is left as it
 * is. Returns true when the record changed.
 */
export function recordRender(takeIndex, takeKey, attempt) {
  if (!takeIndex || !takeKey || !Object.prototype.hasOwnProperty.call(takeIndex, takeKey)) return false;
  if (!isRecordable(attempt)) return false;
  const list = rendersOf(takeIndex, takeKey);
  if (list.some((known) => sameRender(known, attempt))) return false;
  const render = {};
  RENDER_FIELDS.forEach((field) => {
    const value = attempt[field];
    if (value !== undefined && value !== null && value !== '') render[field] = value;
  });
  renderMapOf(takeIndex)[takeKey] = [...list, render].sort((a, b) => a.started_unix_ms - b.started_unix_ms);
  return true;
}

/** Records every finished render in a take folder's history (#438). Returns
 *  how many were new to the project. */
export function recordTakeFolderRenders(takeIndex, takeKey, history) {
  return (history || []).filter((attempt) => recordRender(takeIndex, takeKey, attempt)).length;
}

/** Puts a loaded project's `takeRenders` onto its take index. */
export function attachRenders(takeIndex, saved) {
  if (!saved || typeof saved !== 'object' || Array.isArray(saved)) return;
  Object.entries(saved).forEach(([key, list]) => {
    if (Array.isArray(list)) renderMapOf(takeIndex)[key] = list.filter(isRecordable);
  });
}

/** Carries `from`'s renders for the takes `to` still holds. */
export function carryRenders(from, to) {
  const saved = from?.[RENDERS];
  if (!saved) return;
  Object.keys(saved).forEach((key) => {
    if (Object.prototype.hasOwnProperty.call(to, key)) renderMapOf(to)[key] = saved[key];
  });
}

/** The map to write as the project's `takeRenders`: left out (undefined)
 *  while empty, so a project that never rendered is saved as it was. */
export function rendersToSave(takeIndex) {
  const saved = takeIndex?.[RENDERS];
  if (!saved) return undefined;
  const kept = Object.fromEntries(Object.entries(saved).filter(([, list]) => list.length > 0));
  return Object.keys(kept).length > 0 ? kept : undefined;
}

/**
 * Every video recorded for the project, in movie order: takes by when they
 * were captured (a take with no date last, then by key), each take's renders
 * oldest first. Each is `{ takeKey, uids, ...render }`. Pure: whether a file
 * is still there is the caller's question (`locateRenderedVideos`). The list
 * a whole-project VEGAS export starts from (#1264 part 2).
 */
export function projectRenderedVideos(takeIndex) {
  const keys = Object.keys(takeIndex?.[RENDERS] || {})
    .filter((key) => Object.prototype.hasOwnProperty.call(takeIndex, key));
  const capturedAt = (key) => captureOf(takeIndex, key)?.captured_at;
  keys.sort((a, b) => {
    const at = capturedAt(a);
    const bt = capturedAt(b);
    if (at != null && bt != null && at !== bt) return at - bt;
    if ((at == null) !== (bt == null)) return at == null ? 1 : -1;
    return a.localeCompare(b);
  });
  return keys.flatMap((takeKey) => rendersOf(takeIndex, takeKey)
    .map((render) => ({ takeKey, uids: takeIndex[takeKey], ...render })));
}

/** The videos of `videos` that `located` (same order, the path each is at
 *  now or null) found, each with its current path. */
export function existingVideos(videos, located) {
  return videos
    .map((video, i) => (located?.[i] ? { ...video, output_path: located[i] } : null))
    .filter(Boolean);
}
