// review_mode.js
// Review highlights (#623): the Master Demo Queue's ticked demos go to the
// game, which plays every highlight in turn; each Yes/No the user gives there
// comes back over the game's events pipe and lands on that highlight's row
// (review_queue.js says how).

import { listen } from '@tauri-apps/api/event';
import { startHighlightReview, stopHighlightReview } from './ipc_bridge.js';
import { buildReviewQueue, applyReviewAnswer } from './review_queue.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

/**
 * Wires the Master Demo Queue's Review highlights button.
 *  - getDemos(): the queue's demos
 *  - getCheckedPaths(): the ticked rows' paths
 *  - onChanged(): re-render and mark the project dirty after an answer
 */
export function initReviewMode({ getDemos, getCheckedPaths, onChanged }) {
  const btn = document.querySelector('#review-highlights-btn');
  if (!btn) return;
  let answers = 0;
  let running = false;

  listen('review_event', (event) => {
    const e = event.payload || {};
    if (e.kind === 'answer') {
      // Read per answer, so the box can be changed mid-review.
      const tickYes = document.querySelector('#review-tick-yes-cb')?.checked === true;
      if (applyReviewAnswer(getDemos(), e, { tickYes })) {
        answers += 1;
        onChanged();
      } else {
        console.warn('[review] answer for a highlight no longer in the queue:', e);
      }
    } else if (e.kind === 'ended' && running) {
      running = false;
      showToast(STRINGS.REVIEW.endedToast(e.reason, answers), 'info', 8000);
    }
  }).catch((err) => console.error('[review] could not listen for answers:', err));

  btn.addEventListener('click', async () => {
    const minKills = parseInt(document.querySelector('#input-min-kills')?.value || '1', 10) || 1;
    const { highlights, skipped } = buildReviewQueue(getDemos(), getCheckedPaths(), minKills);
    if (highlights.length === 0) {
      showToast(skipped > 0 ? STRINGS.REVIEW.ONLY_OLD_HIGHLIGHTS : STRINGS.REVIEW.NOTHING_TO_REVIEW, 'info');
      return;
    }
    btn.disabled = true;
    try {
      const started = await startHighlightReview(highlights);
      running = true;
      answers = 0;
      showToast(STRINGS.REVIEW.startedToast(started.count, started.launched, skipped), 'success', 8000, {
        action: { label: STRINGS.REVIEW.STOP, onClick: () => stopHighlightReview().catch(() => {}) },
      });
    } catch {
      // ipc_bridge has shown the error.
    } finally {
      btn.disabled = false;
    }
  });
}
