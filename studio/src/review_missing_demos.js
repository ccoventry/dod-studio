// review_missing_demos.js
// Review reached a demo whose file is missing (#1376): ask what to do, one
// demo at a time. Locate… and Remove are what a missing row already offers,
// Use found copy appears when a matching file was found, and Skip and keep
// reviewing leaves the demo out of this review only. Closing the pop-up
// cancels the review. The question is the themed dialog's choices.

import { themedConfirm } from './themed_confirm.js';
import { logFrontendEvent } from './ipc_bridge.js';
import { isDemoTracked } from './take_index.js';
import { STRINGS } from './strings.js';
import { fileNameOf } from './path_display.js';
import { reviewSkippedMissingLine, reviewMissingChoiceLine } from './project_log.js';

export const MISSING_CHOICE = Object.freeze({
  LOCATE: 'locate', USE_FOUND: 'use-found', REMOVE: 'remove', SKIP: 'skip', CANCEL: 'cancel',
});

const nameOf = (demo) => demo.name || fileNameOf(demo.path);

/** Asks about one missing demo; resolves to a MISSING_CHOICE value. */
export async function askAboutMissingDemo(demo) {
  const s = STRINGS.REVIEW;
  const choices = [{ id: MISSING_CHOICE.LOCATE, label: s.MISSING_LOCATE }];
  if (demo.foundAt) choices.push({ id: MISSING_CHOICE.USE_FOUND, label: s.MISSING_USE_FOUND });
  choices.push(
    { id: MISSING_CHOICE.REMOVE, label: s.MISSING_REMOVE, destructive: true },
    { id: MISSING_CHOICE.SKIP, label: s.MISSING_SKIP },
  );
  const answer = await themedConfirm(s.missingMessage(nameOf(demo), demo.searchStopped === true), {
    title: s.MISSING_TITLE,
    cancelLabel: s.MISSING_CANCEL,
    choices,
  });
  return answer || MISSING_CHOICE.CANCEL;
}

/**
 * @param {object} app
 *   getDemos() the queue; lookMissing(demos) demo_file_recheck.js's, resolving
 *   to those of `demos` whose file is gone; locateDemoByHand(demo) and
 *   useFoundCopies(demos) project_demos.js's; confirmTrackedRemove(demo)
 *   queue_actions.js's requestTrackedDeleteConfirm; replaceDemos(demos) the
 *   queue's. `ask` and `log` are for tests.
 * @returns {Function} settleMissingDemos(demos), resolving to
 *   `{ ok, touched, dropped }`: `ok` false when the user closed the pop-up
 *   (the review is cancelled); `touched` when any missing demo was dealt
 *   with, so the caller builds the review's queue again; `dropped` the demos
 *   (rows) the review goes on without, skipped or removed.
 */
export function createReviewMissingDemos({
  getDemos, lookMissing, locateDemoByHand, useFoundCopies, confirmTrackedRemove, replaceDemos,
  ask = askAboutMissingDemo, log = logFrontendEvent,
}) {
  async function removeRow(demo) {
    if (isDemoTracked(demo) && !(await confirmTrackedRemove(demo))) return false;
    replaceDemos(getDemos().filter((d) => d !== demo));
    return true;
  }

  // Asks about one demo until it is dealt with. Resolves to 'present' (back
  // where the queue says), 'dropped', or 'cancel'.
  async function settleOne(demo) {
    for (;;) {
      // A located file that didn't match was scanned in this row's place.
      if (!getDemos().includes(demo)) return 'dropped';
      if (!demo.missing) return 'present';
      const choice = await ask(demo);
      log(reviewMissingChoiceLine(nameOf(demo), choice));
      if (choice === MISSING_CHOICE.CANCEL) return 'cancel';
      if (choice === MISSING_CHOICE.SKIP) return 'dropped';
      if (choice === MISSING_CHOICE.LOCATE) await locateDemoByHand(demo);
      else if (choice === MISSING_CHOICE.USE_FOUND) await useFoundCopies([demo]);
      else if (choice === MISSING_CHOICE.REMOVE && await removeRow(demo)) return 'dropped';
    }
  }

  return async function settleMissingDemos(demos) {
    const dropped = new Set();
    const missing = await lookMissing(demos);
    for (const demo of missing) {
      const outcome = await settleOne(demo);
      if (outcome === 'cancel') return { ok: false, touched: dropped.size > 0, dropped };
      if (outcome === 'dropped') {
        dropped.add(demo);
        if (getDemos().includes(demo)) log(reviewSkippedMissingLine(nameOf(demo), demos.length - dropped.size));
      }
    }
    return { ok: true, touched: missing.length > 0, dropped };
  };
}
