// take_index.js
//
// Stable identity for highlights, so status survives things that rebuild the
// demo objects underneath it (a re-scan, a project save/load round trip).
//
// Highlights have no id of their own — nothing in CaptureStreak is unique and
// durable — so this derives a content-addressed one. Kill frames are used
// rather than start_tick/end_tick because editing a highlight's Kill Range
// moves the ticks but never mutates the kills array, so the uid stays put
// across range edits.

import { HIGHLIGHT_STATUS } from './status_colors.js';
import { uidsOf, mapUids, recordInto } from './take_record.js';

/**
 * Content-addressed id for one highlight, stable across re-scans, project
 * save/load, and Kill Range edits. Cached onto the streak after first use.
 *
 * Falls back to tick bounds when a streak has no kills (only really happens
 * for synthetic/test data) — still stable, just less precise.
 */
export function streakUid(demoPath, streak) {
  if (streak.uid) return streak.uid;

  const kills = streak.kills || [];
  const firstKill = kills.length ? kills[0][0] : streak.start_tick;
  const lastKill = kills.length ? kills[kills.length - 1][0] : streak.end_tick;
  const uid = `${demoPath}#${streak.player_index}#${firstKill}#${lastKill}`;

  // Non-enumerable so it never leaks into the capture payload sent over IPC,
  // where the Rust side would reject an unknown field.
  Object.defineProperty(streak, 'uid', { value: uid, enumerable: false, writable: true });
  return uid;
}

// ── Curation (#44) ──────────────────────────────────────────────────────────
//
// An editorial mark, separate from `status` (which drives what the pipeline
// does): unset = not reviewed yet, Keep, Maybe (look again later, #661), or
// Skip. Skip unticks the row and locks it, so no batch can include it; Maybe
// leaves the row tickable. Frontend-only, persisted in the project file with
// the rest of the streak.

export const CURATION = { KEEP: 'Keep', MAYBE: 'Maybe', SKIP: 'Skip' };

const MARKS = new Set(Object.values(CURATION));

/** True for a highlight marked Skip: never selectable, never captured. */
export function isSkipped(streak) {
  return streak?.curation === CURATION.SKIP;
}

/** Sets the mark ('' clears it). Skip also unticks the row. */
export function setCuration(streak, value) {
  if (MARKS.has(value)) streak.curation = value;
  else delete streak.curation;
  if (value === CURATION.SKIP) streak.selected = false;
}

// ── Status source (#105) ────────────────────────────────────────────────────
//
// Status moves freely in both directions from the dropdown, but a status set
// by hand carries `statusByHand` so it can be told apart from one a verified
// capture or render set. Enumerable on purpose: it has to survive the project
// file's JSON round trip, and the Rust side ignores fields it doesn't know.

/**
 * Sets `status` from the dropdown. Returns what it replaced, for Undo
 * (restoreStatus).
 */
export function setStatusByHand(streak, status) {
  const previous = { status: streak.status, statusByHand: streak.statusByHand === true };
  streak.status = status;
  streak.statusByHand = true;
  return previous;
}

/** Puts back what setStatusByHand replaced. */
export function restoreStatus(streak, previous) {
  if (previous.status === undefined) delete streak.status;
  else streak.status = previous.status;
  if (previous.statusByHand) streak.statusByHand = true;
  else delete streak.statusByHand;
}

/**
 * A verified capture or render confirmed `status` on disk: sets it and
 * clears the set-by-hand mark. Returns true if the status value changed.
 */
export function setVerifiedStatus(streak, status) {
  const changed = streak.status !== status;
  streak.status = status;
  delete streak.statusByHand;
  return changed;
}

// ── Durable take index ──────────────────────────────────────────────────────
//
// take_key -> take record (take_record.js: the uids, and when and how the take
// was captured; old projects hold a bare uid[]). Recorded when a capture batch verifies a block on disk
// (capture_pane.js) so a render finishing later — possibly after a restart,
// re-scan, or project reload that produced entirely new streak objects — can
// still resolve which highlights to flip to Rendered. Plain object (not a
// Map) so it round-trips through JSON.stringify/parse with the rest of the
// project file with no extra conversion step.

/**
 * Records that the highlights identified by `uids` produced the take at
 * `takeKey`. Additive: a take key can cover more than one highlight (an
 * overlap merge recording several highlights into one block), and recording
 * the same key again (e.g. a re-capture) just unions the uid sets rather
 * than dropping the earlier ones.
 */
export function recordTake(takeIndex, takeKey, uids, meta) {
  if (!takeIndex || !takeKey || !uids || uids.length === 0) return;
  takeIndex[takeKey] = recordInto(takeIndex[takeKey], uids, meta);
}

/**
 * Marks the highlights a captured block covers Captured and records its take
 * in the take index, so a render of it resolves back to them. Shared by the
 * post-batch verification and Render While Capturing (#861), which renders
 * (and may delete) a take before the batch ends. `{ advanced, markCleared }`.
 * The record keeps when and how the take was captured (#439): `block` is a
 * verified block or a `capture_clip_ready` payload (which carries its own
 * rate), the dispatch's own rate (what the mode really records at) wins, then
 * `captureFps` for a block that carries none.
 */
export function markBlockCaptured(block, dispatch, takeIndex, captureFps) {
  let advanced = 0;
  let markCleared = false;
  // A block can cover several highlights — overlapping ones are recorded
  // as one continuous take — so every highlight it covers advances.
  const uids = [];
  const sourceIndices = block.source_streak_indices || [];
  sourceIndices.forEach(i => {
    const streak = dispatch?.streaks?.[i];
    if (!streak) return;
    const demoPath = dispatch?.demoPaths?.[i];
    uids.push(streakUid(demoPath, streak));
    // Merged blocks (2+ source highlights recorded into one take) get a
    // visible affordance in Highlight Details (detail_pane.js) so it's
    // obvious why the rows flipped together. Non-enumerable, same as
    // `.uid`, so it never leaks into a future capture payload sent
    // over IPC (buildCapturePayload pushes the live streak object).
    if (sourceIndices.length > 1 && block.take_key) {
      Object.defineProperty(streak, 'mergedTakeKey', { value: block.take_key, enumerable: false, configurable: true });
      Object.defineProperty(streak, 'mergedCount', { value: sourceIndices.length, enumerable: false, configurable: true });
    }
    // Status only ever moves forward. Re-capturing something already
    // rendered must not knock it back down to Captured -- unless that
    // Rendered was set by hand: a verified capture beats an unverified
    // claim (#105).
    if (streak.status === HIGHLIGHT_STATUS.RENDERED && !streak.statusByHand) return;
    if (streak.statusByHand) markCleared = true;
    if (setVerifiedStatus(streak, HIGHLIGHT_STATUS.CAPTURED)) advanced += 1;
  });
  // Recorded even when the take isn't renderable yet — a future render
  // still needs to resolve this take_key back to these highlights once
  // it succeeds, and the index outlives this dispatch's in-memory state.
  if (takeIndex && block.take_key) {
    recordTake(takeIndex, block.take_key, uids, {
      session_id: block.session_id ?? dispatch?.sessionId,
      captured_at: Date.now(),
      capture_mode: block.capture_mode ?? dispatch?.captureMode,
      capture_fps: dispatch?.captureFps ?? block.capture_fps ?? captureFps,
      take_folder: block.take_folder,
    });
  }
  return { advanced, markCleared };
}

/**
 * Points every uid of a relocated demo at its new path (#21). A uid starts
 * with the demo's path, so without this a moved demo's captured takes would
 * no longer resolve to its highlights.
 */
export function renameDemoInTakeIndex(takeIndex, oldPath, newPath) {
  if (!takeIndex) return;
  const oldPrefix = `${oldPath}#`;
  Object.keys(takeIndex).forEach((key) => {
    takeIndex[key] = mapUids(takeIndex[key], (uids) => uids.map((uid) =>
      uid.startsWith(oldPrefix) ? `${newPath}#${uid.slice(oldPrefix.length)}` : uid
    ));
  });
}

/**
 * Points each recorded uid in `renames` (old uid -> new uid) at its new one:
 * a split demo's highlights live on in its pieces under new uids (#217), and
 * their captured takes must still resolve to them.
 */
export function renameUidsInTakeIndex(takeIndex, renames) {
  if (!takeIndex || !renames || renames.size === 0) return;
  Object.keys(takeIndex).forEach((key) => {
    takeIndex[key] = mapUids(takeIndex[key], (uids) => Array.from(new Set(uids.map((uid) => renames.get(uid) ?? uid))));
  });
}

/**
 * Highlight uids the given take key maps to, or an empty array if this take
 * was never recorded (e.g. captured before Phase 3, or from a project saved
 * on an older version that had no take index at all).
 */
export function resolveTake(takeIndex, takeKey) {
  if (!takeIndex || !takeKey) return [];
  return uidsOf(takeIndex[takeKey]);
}

/**
 * Drops index entries for uids no longer present in the current highlight
 * set (a demo removed from the queue, say), so the index doesn't grow
 * forever across a project's lifetime. Entries left with zero surviving
 * uids are dropped entirely rather than kept as an empty array.
 */
export function pruneTakeIndex(takeIndex, knownUids) {
  const known = new Set(knownUids);
  const pruned = {};
  for (const [key, entry] of Object.entries(takeIndex || {})) {
    const kept = uidsOf(entry).filter(uid => known.has(uid));
    if (kept.length > 0) pruned[key] = mapUids(entry, () => kept);
  }
  return pruned;
}

// ── "Tracked" predicate (Workspace mode's Clear Untracked protection) ──────

/** True if a streak's Kill Range has been narrowed from the full range. */
export function isRangeModified(streak) {
  const kills = streak.kills || [];
  const maxKillIdx = Math.max(kills.length - 1, 0);
  return (streak.start_index || 0) > 0 || (streak.end_index ?? maxKillIdx) < maxKillIdx;
}

/**
 * True if this highlight carries anything the user did on purpose — real
 * pipeline-earned status (Pending/Captured/Rendered), a note, or a narrowed
 * kill range. This is deliberately a wide net: any one of the three is
 * enough to protect the row from Clear Untracked in Workspace mode.
 *
 * `Pending` counts because an unset status displays (and counts,
 * master_pane.js's countByStatus) as "None" — "Pending" only ever appears
 * once the user deliberately sets it, e.g. to flag a highlight for a later
 * capture pass without selecting it yet — so it is exactly the kind of
 * on-purpose signal this predicate exists to protect.
 */
function isHighlightTracked(streak) {
  if (!streak) return false;
  if (streak.status === HIGHLIGHT_STATUS.PENDING || streak.status === HIGHLIGHT_STATUS.CAPTURED || streak.status === HIGHLIGHT_STATUS.RENDERED) return true;
  if (streak.notes && streak.notes.trim()) return true;
  if (streak.clipName && streak.clipName.trim()) return true;
  if (streak.curation) return true;
  return isRangeModified(streak);
}

/** True if any of a demo's highlights are tracked (see isHighlightTracked). */
export function isDemoTracked(demo) {
  return (demo?.streaks || []).some(isHighlightTracked);
}
