// review_from_game.js
// A review the game asked for (#1142): the in-game Review tab's From the
// start and From here buttons ask Studio, over its game pipe, to review the
// demo the game has loaded. Studio reviews it as its Review highlights button
// would for that demo alone: every highlight, from the first, or from the
// first at or after the demo time the game was at ("Highlight 3 of 15", and
// Back still reaches 1 and 2). A demo not in the Master Demo Queue is added
// and scanned first, as a dropped demo is; one whose highlights predate
// demo-player times is scanned again. Each step is told to the game, which
// shows it on its Review tab, and the end (sent, or why not) too.

import { listen } from '@tauri-apps/api/event';
import { gameReviewProgress, gameReviewFinished } from './ipc_bridge.js';
import { REVIEW_COVERS } from './review_queue.js';
import { fileNameOf, samePath } from './path_display.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

/** The page's event for a request (review_manager.rs's GAME_REQUEST_EVENT). */
export const GAME_REVIEW_EVENT = 'game_review_request';

/** The queued demo at `path`, or null. */
export function findQueuedDemo(demos, path) {
  return (demos || []).find((d) => samePath(d.path, path)) || null;
}

/** True when a queued demo has highlights but none the game can find: all
 *  scanned before demo-player times were kept, so a rescan comes first. */
export function needsRescan(demo) {
  const streaks = demo?.streaks || [];
  return streaks.length > 0 && streaks.every((s) => !(s.viewdemo_times || []).length);
}

/** The demo time a request starts from, or null for the first highlight. */
export function startFromOf(request) {
  const start = request?.start;
  return start?.kind === 'at' && Number.isFinite(start.seconds) ? start.seconds : null;
}

/**
 * Starts one request's review. `deps`:
 *  - getDemos(): the Master Demo Queue's demos
 *  - scan(paths): adds/rescans demos, resolving once they are merged
 *  - isScanning(): a scan is running
 *  - startReview(paths, options): review_mode.js's, resolving to null once
 *    sent or to why not
 *  - progress(text), finish(error): what the game is told
 * Resolves once the game has been told how it ended.
 */
export async function handleGameReviewRequest(request, deps) {
  const { getDemos, scan, isScanning, startReview, progress, finish } = deps;
  const path = request?.demo;
  const name = fileNameOf(path || '');
  let demo = findQueuedDemo(getDemos(), path);
  if (!demo || needsRescan(demo)) {
    if (isScanning()) {
      await finish(STRINGS.REVIEW.FROM_GAME_BUSY);
      return;
    }
    if (!demo) showToast(STRINGS.REVIEW.fromGameAddingToast(name), 'info', 8000);
    await progress(STRINGS.REVIEW.FROM_GAME_SCANNING);
    await scan([path]);
    demo = findQueuedDemo(getDemos(), path);
    if (!demo) {
      await finish(STRINGS.REVIEW.fromGameNotAdded(name));
      return;
    }
  }
  if (!(demo.streaks || []).length) {
    await finish(STRINGS.REVIEW.fromGameNoHighlights(name));
    return;
  }
  await progress(STRINGS.REVIEW.FROM_GAME_STARTING);
  const why = await startReview([demo.path], {
    covers: REVIEW_COVERS.ALL,
    startFrom: startFromOf(request),
    fromGame: true,
  });
  await finish(why);
}

/**
 * Listens for the game's requests. `scan`, `getDemos`, `isScanning` and
 * `startReview` as handleGameReviewRequest takes them.
 */
export function startReviewFromGame({ getDemos, scan, isScanning, startReview }) {
  listen(GAME_REVIEW_EVENT, (event) => {
    const request = event.payload || {};
    handleGameReviewRequest(request, {
      getDemos,
      scan,
      isScanning,
      startReview,
      progress: (text) => gameReviewProgress(request.id, text),
      finish: (error) => gameReviewFinished(request.id, error),
    }).catch((err) => {
      console.error('[review] the game\'s review request failed:', err);
      gameReviewFinished(request.id, String(err?.message ?? err));
    });
  }).catch((err) => console.error('[review] could not listen for the game\'s requests:', err));
}
