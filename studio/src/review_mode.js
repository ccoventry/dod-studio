// review_mode.js
// Review highlights (#623): the Master Demo Queue's ticked demos (or, with
// none ticked, every demo it shows) go to the game, which plays every highlight in turn (or only those with the Review
// mark the "Review" box picks, #661); each Yes/Maybe/No the user gives there
// comes back over the game's events pipe and lands on that highlight's row
// (review_queue.js says how). In an HLTV demo only the players the "HLTV"
// box picks are played, with the camera on them (#822); by default, the
// players who didn't record that half themselves, or any players picked
// beside it (#828, review_hltv_picker.js).

import { listen } from '@tauri-apps/api/event';
import { startHighlightReview, stopHighlightReview } from './ipc_bridge.js';
import { buildReviewQueue, applyReviewAnswer, fastForwardGap, reviewDemoPaths, hltvPlayersFor, HLTV_PLAYERS } from './review_queue.js';
import { closeGameWithOtherSettings } from './running_game_guard.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

/**
 * Wires the Master Demo Queue's Review highlights button.
 *  - getDemos(): the queue's demos
 *  - getCheckedPaths(): the ticked rows' paths
 *  - getShownPaths(): the paths the queue shows under its search and filters,
 *    reviewed when nothing is ticked
 *  - getRecorders(): every player who recorded a demo in the queue, for the
 *    HLTV box (master_pane.js's getQueueRecorders)
 *  - recorderOf(demo): who recorded a demo, `{ id, name }` or null, to tell
 *    which players an HLTV demo's half is missing
 *  - onChanged(): re-render and mark the project dirty after an answer
 *  - hltvPick: review_hltv_picker.js's picker, which holds the box's choice
 *    and the players "Players I pick" reviews
 * Returns `{ startReview }`, which review_from_game.js starts the game's own
 * requests with (#1142).
 */
export function initReviewMode({ getDemos, getCheckedPaths, getShownPaths, getRecorders, recorderOf, onChanged, hltvPick = null }) {
  const btn = document.querySelector('#review-highlights-btn');
  if (!btn) return { startReview: async () => STRINGS.REVIEW.NOT_STARTED };
  let answers = 0;
  let running = false;

  // The HLTV box lists the queue's recorders, checked each time it opens so
  // it follows the queue, and rebuilt only when that list changed so the
  // open dropdown isn't swapped out under the click; the pick (the
  // picker's, which a loaded project sets) survives a rebuild while that
  // player is still there.
  const hltvSelect = document.querySelector('#review-hltv-select');
  let hltvKey = null;
  const fillHltvSelect = () => {
    if (!hltvSelect) return;
    const recorders = getRecorders?.() || [];
    const key = recorders.map((p) => `${p.id}\t${p.label}`).join('\n');
    if (key === hltvKey) return;
    hltvKey = key;
    const keep = hltvPick?.choice() ?? hltvSelect.value;
    hltvSelect.textContent = '';
    const add = (value, label) => {
      const option = document.createElement('option');
      option.value = value;
      option.textContent = label;
      hltvSelect.appendChild(option);
    };
    add(HLTV_PLAYERS.MISSING, STRINGS.REVIEW.HLTV_MISSING);
    add(HLTV_PLAYERS.RECORDERS, STRINGS.REVIEW.HLTV_RECORDERS);
    add(HLTV_PLAYERS.ALL, STRINGS.REVIEW.HLTV_ALL);
    add(HLTV_PLAYERS.PICKED, STRINGS.REVIEW.HLTV_PICKED);
    for (const p of recorders) add(p.id, p.label);
    hltvSelect.value = [...hltvSelect.options].some((o) => o.value === keep) ? keep : HLTV_PLAYERS.MISSING;
    hltvPick?.refresh();
  };
  fillHltvSelect();
  hltvSelect?.addEventListener('mousedown', fillHltvSelect);
  hltvSelect?.addEventListener('focus', fillHltvSelect);

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
    } else if (e.kind === 'refused') {
      // Launch Preview or a seek sent during a review (#880): the game
      // refused it, so say why and offer the way out.
      showToast(STRINGS.REVIEW.refusedToast(e.command), 'warning', 8000, {
        action: { label: STRINGS.REVIEW.STOP, onClick: () => stopHighlightReview().catch(() => {}) },
      });
    }
  }).catch((err) => console.error('[review] could not listen for answers:', err));

  /**
   * Sends the review of `paths`' highlights (those `covers` takes in, with
   * the HLTV, order and fast-forward boxes as they stand) to the game.
   * `tickedAny`: the paths are the ticked rows, for the "nothing covered"
   * message. `startFrom`: the game starts at the first highlight at or after
   * this demo time (#1142). `fromGame`: the game itself asked, so a running
   * game with other launch settings is refused rather than offered to close
   * (that game is the one asking). Resolves to null once the review is sent,
   * or to why it wasn't, which has been shown too.
   */
  async function startReview(paths, { covers, tickedAny = false, startFrom = null, fromGame = false } = {}) {
    fillHltvSelect();
    const picked = hltvPick?.players() || [];
    const hltvPlayers = hltvPlayersFor(hltvSelect?.value, getRecorders?.() || [], { demos: getDemos(), recorderOf, picked });
    const order = document.querySelector('#review-order-select')?.value;
    const { highlights, skipped, notChosen } = buildReviewQueue(getDemos(), paths, covers, hltvPlayers, order);
    if (highlights.length === 0) {
      let why = STRINGS.REVIEW.NOTHING_TO_REVIEW;
      if (skipped > 0) why = STRINGS.REVIEW.ONLY_OLD_HIGHLIGHTS;
      else if (notChosen > 0) why = STRINGS.REVIEW.hltvNoneChosen(hltvSelect?.value, notChosen, picked.length);
      else if (paths.length > 0) why = STRINGS.REVIEW.nothingCovered(covers, tickedAny);
      showToast(why, 'info');
      return why;
    }
    btn.disabled = true;
    try {
      // A game started with other launch settings is closed and started
      // again, if the user says so; otherwise nothing is sent (#666).
      if (!fromGame && !(await closeGameWithOtherSettings())) return STRINGS.REVIEW.NOT_STARTED;
      const gap = fastForwardGap(
        document.querySelector('#review-fast-forward-cb')?.checked === true,
        document.querySelector('#review-fast-forward-gap')?.value,
      );
      const started = await startHighlightReview(highlights, gap, startFrom);
      running = true;
      answers = 0;
      showToast(STRINGS.REVIEW.startedToast(started.count, started.launched, skipped), 'success', 8000, {
        action: { label: STRINGS.REVIEW.STOP, onClick: () => stopHighlightReview().catch(() => {}) },
      });
      return null;
    } catch (err) {
      // ipc_bridge has shown the error.
      return String(err?.message ?? err);
    } finally {
      btn.disabled = false;
    }
  }

  btn.addEventListener('click', () => {
    const ticked = getCheckedPaths();
    startReview(reviewDemoPaths(ticked, getShownPaths()), {
      covers: document.querySelector('#review-covers-select')?.value,
      tickedAny: ticked.length > 0,
    });
  });

  return { startReview };
}
