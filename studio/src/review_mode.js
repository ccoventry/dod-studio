// review_mode.js
// Review highlights (#623): the Master Demo Queue's ticked demos (or, with
// none ticked, every demo it shows) go to the game, which plays every highlight in turn (or only those with the Review
// mark the "Review" box picks, #661); each Yes/Maybe/No the user gives there
// comes back over the game's events pipe and lands on that highlight's row
// (review_queue.js says how). In an HLTV demo only the players the "HLTV"
// box picks are played, with the camera on them (#822); by default, the
// players who didn't record that half themselves.

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
 */
export function initReviewMode({ getDemos, getCheckedPaths, getShownPaths, getRecorders, recorderOf, onChanged }) {
  const btn = document.querySelector('#review-highlights-btn');
  if (!btn) return;
  let answers = 0;
  let running = false;

  // The HLTV box lists the queue's recorders, checked each time it opens so
  // it follows the queue, and rebuilt only when that list changed so the
  // open dropdown isn't swapped out under the click; the pick survives a
  // rebuild while that player is still there.
  const hltvSelect = document.querySelector('#review-hltv-select');
  let hltvKey = null;
  const fillHltvSelect = () => {
    if (!hltvSelect) return;
    const recorders = getRecorders?.() || [];
    const key = recorders.map((p) => `${p.id}\t${p.label}`).join('\n');
    if (key === hltvKey) return;
    hltvKey = key;
    const keep = hltvSelect.value;
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
    for (const p of recorders) add(p.id, p.label);
    hltvSelect.value = [...hltvSelect.options].some((o) => o.value === keep) ? keep : HLTV_PLAYERS.MISSING;
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
    }
  }).catch((err) => console.error('[review] could not listen for answers:', err));

  btn.addEventListener('click', async () => {
    const minKills = parseInt(document.querySelector('#input-min-kills')?.value || '1', 10) || 1;
    const covers = document.querySelector('#review-covers-select')?.value;
    const ticked = getCheckedPaths();
    const paths = reviewDemoPaths(ticked, getShownPaths());
    const hltvPlayers = hltvPlayersFor(hltvSelect?.value, getRecorders?.() || [], { demos: getDemos(), recorderOf });
    const { highlights, skipped, notChosen } = buildReviewQueue(getDemos(), paths, minKills, covers, hltvPlayers);
    if (highlights.length === 0) {
      let why = STRINGS.REVIEW.NOTHING_TO_REVIEW;
      if (skipped > 0) why = STRINGS.REVIEW.ONLY_OLD_HIGHLIGHTS;
      else if (notChosen > 0) why = STRINGS.REVIEW.hltvNoneChosen(hltvSelect?.value, notChosen);
      else if (paths.length > 0) why = STRINGS.REVIEW.nothingCovered(covers, ticked.length > 0);
      showToast(why, 'info');
      return;
    }
    btn.disabled = true;
    try {
      // A game started with other launch settings is closed and started
      // again, if the user says so; otherwise nothing is sent (#666).
      if (!(await closeGameWithOtherSettings())) return;
      const gap = fastForwardGap(
        document.querySelector('#review-fast-forward-cb')?.checked === true,
        document.querySelector('#review-fast-forward-gap')?.value,
      );
      const started = await startHighlightReview(highlights, gap);
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
