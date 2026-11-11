// take_record.js
//
// How and when each take was captured (#439), kept apart from the take index.
//
// The project file's `takeIndex` stays what it always was, `take_key -> uid[]`,
// because older Studio builds read it as exactly that and an object there
// breaks them (the owner moves between release, dev and test builds on one
// project). The capture facts live in a sibling `takeCaptures` key,
// `take_key -> { session_id, captured_at, capture_mode, capture_fps,
// take_folder }`, which older builds ignore (and drop when they save).
//
// In memory the map rides on the take index object as a non-enumerable
// property, so every module that already passes the index around reaches it
// without new plumbing, and JSON.stringify of the index never includes it.

const CAPTURES = 'takeCaptures';

/** The capture facts a record keeps. */
const CAPTURE_FIELDS = ['session_id', 'captured_at', 'capture_mode', 'capture_fps', 'take_folder'];

/** The index's capture map, made on first use. */
export function captureMapOf(takeIndex) {
  if (!Object.prototype.hasOwnProperty.call(takeIndex, CAPTURES)) {
    Object.defineProperty(takeIndex, CAPTURES, { value: {}, enumerable: false, writable: true, configurable: true });
  }
  return takeIndex[CAPTURES];
}

/** What is known of how `takeKey` was captured, or undefined. */
export function captureOf(takeIndex, takeKey) {
  return takeIndex?.[CAPTURES]?.[takeKey];
}

/**
 * Records the capture facts in `meta` for `takeKey`. A fact that is known
 * overwrites the old one (a re-capture into the same key is the newer
 * capture); an unknown one leaves it.
 */
export function recordCapture(takeIndex, takeKey, meta) {
  if (!takeIndex || !takeKey || !meta) return;
  const record = { ...captureOf(takeIndex, takeKey) };
  CAPTURE_FIELDS.forEach((field) => {
    const value = meta[field];
    if (value !== undefined && value !== null && value !== '') record[field] = value;
  });
  if (Object.keys(record).length > 0) captureMapOf(takeIndex)[takeKey] = record;
}

/** Puts a loaded project's `takeCaptures` onto its take index. */
export function attachCaptures(takeIndex, saved) {
  if (!saved || typeof saved !== 'object' || Array.isArray(saved)) return;
  Object.assign(captureMapOf(takeIndex), saved);
}

/** Carries `from`'s captures for the takes `to` still holds. */
export function carryCaptures(from, to) {
  const saved = from?.[CAPTURES];
  if (!saved) return;
  Object.keys(saved).forEach((key) => {
    if (Object.prototype.hasOwnProperty.call(to, key)) captureMapOf(to)[key] = saved[key];
  });
}

/** The map to write as the project's `takeCaptures`: left out (undefined)
 *  while empty, so a project that never recorded one is saved as it was. */
export function capturesToSave(takeIndex) {
  const saved = takeIndex?.[CAPTURES];
  return saved && Object.keys(saved).length > 0 ? saved : undefined;
}

/**
 * The rate a batch really recorded at. `capture_fps` is the frame-sequence and
 * direct-to-video setting only: OBS mode records at its own `obs_capture_fps`
 * (60 when unset, as the backend counts it) and AGR at `agr_fps` (0 = the
 * same as `capture_fps`). `undefined` when the source doesn't carry the OBS
 * rate (a batch manifest read back after a restart).
 */
export function captureRateOf(mode, { capture_fps, obs_capture_fps, agr_fps } = {}) {
  if (mode === 'obs') return obs_capture_fps === undefined ? undefined : (obs_capture_fps > 0 ? obs_capture_fps : 60);
  if (mode === 'agr') return agr_fps > 0 ? agr_fps : capture_fps;
  return capture_fps;
}
