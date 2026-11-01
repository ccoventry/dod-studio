// capture_history_logic.js
//
// The lines of a highlight's capture history (#439), without the DOM: which
// takes a highlight was captured into, and what each line says given what the
// disk showed. Pure, so the take-gone, shared and old-record cases are tested
// on their own (capture_history_logic.test.js); capture_history.js draws them.

import { STRINGS } from './strings.js';
import { captureOf } from './take_record.js';
import { rendersOf as recordedRendersOf } from './take_renders.js';
import { formatWhen, historyLines } from './render_history.js';

const MODE_LABELS = () => ({
  frame_sequence: STRINGS.CAPTURE_CONFIG.CAPTURE_MODE_FRAMES,
  direct_to_video: STRINGS.CAPTURE_CONFIG.CAPTURE_MODE_VIDEO,
  obs: STRINGS.CAPTURE_CONFIG.CAPTURE_MODE_OBS,
  agr: STRINGS.CAPTURE_CONFIG.CAPTURE_MODE_AGR,
});

/**
 * Every take `uid` was captured into, newest first, each as
 * `{ takeKey, uids, record, recordedRenders }`: `uids` from the take index,
 * `record` the capture facts beside it (take_record.js), empty for a take
 * recorded before #439, and `recordedRenders` the videos the project recorded
 * for it (take_renders.js, #1264). A take with no date sorts last.
 */
export function capturesOf(takeIndex, uid) {
  const captures = Object.entries(takeIndex || {})
    .filter(([, uids]) => uids.includes(uid))
    .map(([takeKey, uids]) => ({
      takeKey, uids, record: captureOf(takeIndex, takeKey) || {}, recordedRenders: recordedRendersOf(takeIndex, takeKey),
    }));
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

const sameAttempt = (a, b) => a.started_unix_ms === b.started_unix_ms && a.stream === b.stream;

/**
 * A take's renders, oldest first: every attempt the take folder's history
 * holds (`state.renders`, #438), plus each video the project recorded that
 * the folder no longer shows (its take deleted, #1264), as a finished one.
 */
export function renderEntriesOf(capture, state) {
  const onDisk = state?.renders || [];
  const recordedOnly = (capture.recordedRenders || [])
    .filter((render) => !onDisk.some((attempt) => sameAttempt(attempt, render)))
    .map((render) => ({ ...render, outcome: 'finished' }));
  return [...onDisk, ...recordedOnly].sort((a, b) => a.started_unix_ms - b.started_unix_ms);
}

/** Every finished video of `captures` (one disk state each, in order), once
 *  each, for one `locateRenderedVideos` call. */
export function videoPathList(captures, states) {
  const paths = captures.flatMap((capture, i) => renderEntriesOf(capture, states?.[i])
    .filter((attempt) => attempt.outcome === 'finished' && attempt.output_path)
    .map((attempt) => attempt.output_path));
  return [...new Set(paths)];
}

/**
 * Where a finished video is now, given `located` (path -> current path, or
 * null when it was looked for and not found). Not looked up yet: the disk's
 * own answer for a take-folder attempt, and no video for a recorded one.
 */
function placeOf(attempt, located) {
  if (located.has(attempt.output_path)) {
    const where = located.get(attempt.output_path);
    return where ? { path: where, missing: false } : { path: null, missing: true };
  }
  if (attempt.output_exists === undefined) return { path: null, missing: false };
  return attempt.output_exists ? { path: attempt.output_path, missing: false } : { path: null, missing: true };
}

/** The take's renders, newest first: the attempt's line, the video's path
 *  when it finished and the file is there (or was found again under another
 *  folder), and `missing` when it was moved or deleted and not found. */
function rendersOf(entries, located, formatSize) {
  return [...entries].reverse().map((attempt) => {
    if (attempt.outcome !== 'finished') {
      return { text: historyLines([attempt], formatSize)[0], videoPath: null, missing: false };
    }
    const place = placeOf(attempt, located);
    const shown = { ...attempt, output_path: place.path || attempt.output_path, output_exists: !place.missing };
    return { text: historyLines([shown], formatSize)[0], videoPath: place.path, missing: place.missing };
  });
}

/**
 * One line of the list. `state` is the disk's answer for this take (see
 * takeStatus). `labelOfUid(uid)` names another highlight, or returns null
 * when it is no longer in the queue. `located` is a Map of where the videos
 * are now (see placeOf), empty until that answer is in.
 */
export function describeCapture(capture, uid, state, { labelOfUid, formatSize, located = new Map() }) {
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
    renders: rendersOf(renderEntriesOf(capture, state), located, formatSize),
  };
}
