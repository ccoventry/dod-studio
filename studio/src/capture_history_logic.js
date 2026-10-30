// capture_history_logic.js
//
// The lines of a highlight's capture history (#439), without the DOM: which
// takes a highlight was captured into, and what each line says given what the
// disk showed. Pure, so the take-gone, shared and old-record cases are tested
// on their own (capture_history_logic.test.js); capture_history.js draws them.

import { STRINGS } from './strings.js';
import { captureOf } from './take_record.js';
import { formatWhen, historyLines } from './render_history.js';

const MODE_LABELS = () => ({
  frame_sequence: STRINGS.CAPTURE_CONFIG.CAPTURE_MODE_FRAMES,
  direct_to_video: STRINGS.CAPTURE_CONFIG.CAPTURE_MODE_VIDEO,
  obs: STRINGS.CAPTURE_CONFIG.CAPTURE_MODE_OBS,
  agr: STRINGS.CAPTURE_CONFIG.CAPTURE_MODE_AGR,
});

/**
 * Every take `uid` was captured into, newest first, each as
 * `{ takeKey, uids, record }`: `uids` from the take index, `record` the
 * capture facts beside it (take_record.js), empty for a take recorded before
 * #439. Such a take sorts last (no date).
 */
export function capturesOf(takeIndex, uid) {
  const captures = Object.entries(takeIndex || {})
    .filter(([, uids]) => uids.includes(uid))
    .map(([takeKey, uids]) => ({ takeKey, uids, record: captureOf(takeIndex, takeKey) || {} }));
  return captures.sort((a, b) =>
    (b.record.captured_at || 0) - (a.record.captured_at || 0) || b.takeKey.localeCompare(a.takeKey));
}

/** The take folders of `captures`, in order, for one disk check ('' when a
 *  record has no folder, so the answers line up). */
export function folderList(captures) {
  return captures.map((c) => c.record.take_folder || '');
}

function settingsText(record) {
  const mode = record.capture_mode ? (MODE_LABELS()[record.capture_mode] || record.capture_mode) : '';
  const fps = record.capture_fps > 0 ? `${record.capture_fps} fps` : '';
  return [mode, fps].filter(Boolean).join(' · ') || STRINGS.CAPTURE_HISTORY.NO_SETTINGS;
}

/**
 * Where the take stands: `unknown` (no folder recorded), `checking` (disk
 * answer not in yet: `state` undefined), `unchecked` (the check failed: null),
 * `exists` or `gone`.
 */
function takeStatus(record, state) {
  if (!record.take_folder) return 'unknown';
  if (state === undefined) return 'checking';
  if (state === null) return 'unchecked';
  return state.exists ? 'exists' : 'gone';
}

/** The take's renders, newest first: the attempt's line, and the video's path
 *  when it finished and the file is still there. */
function rendersOf(state, formatSize) {
  return [...(state?.renders || [])].reverse().map((attempt) => ({
    text: historyLines([attempt], formatSize)[0],
    videoPath: attempt.outcome === 'finished' && attempt.output_exists ? attempt.output_path : null,
  }));
}

/**
 * One line of the list. `state` is the disk's answer for this take (see
 * takeStatus). `labelOfUid(uid)` names another highlight, or returns null
 * when it is no longer in the queue.
 */
export function describeCapture(capture, uid, state, { labelOfUid, formatSize }) {
  const { takeKey, uids, record } = capture;
  return {
    takeKey,
    when: record.captured_at ? formatWhen(record.captured_at) : STRINGS.CAPTURE_HISTORY.NO_DATE,
    settings: settingsText(record),
    takeName: takeKey.split('/').pop(),
    folder: record.take_folder || null,
    status: takeStatus(record, state),
    shared: uids.filter((other) => other !== uid)
      .map((other) => labelOfUid(other) || STRINGS.CAPTURE_HISTORY.UNKNOWN_HIGHLIGHT),
    renders: rendersOf(state, formatSize),
  };
}
