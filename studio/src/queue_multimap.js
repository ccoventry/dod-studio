// queue_multimap.js
// Master Demo Queue demos that recorded more than one map (#217). Such a demo
// works like any other for its first map: its highlights come from that map
// (the analysis stops at the map change) and a capture plays it; the maps
// after it are left out. Its row says so with a "2 maps · Split" button that
// writes each map out as a demo of its own, and once its maps were split next
// to it (split_status.js) a quiet "Already split" note takes the button's
// place. Nothing refuses such a demo.
//
// The splitting itself (Split Maps' IPC, the queue swap) is main.js's,
// through the callbacks passed in here.

import { splitProgressBar } from './split_progress.js';
import { isAlreadySplit, splitStatus } from './split_status.js';
import { STRINGS } from './strings.js';

/** Whether a queued demo recorded more than one map. False for a demo from a
 *  project saved before the map list existed (`signon_maps` missing). */
export function isMultiMapDemo(demo) {
  return Array.isArray(demo?.signon_maps) && demo.signon_maps.length > 1;
}

/** The demos with a highlight picked for the next batch that recorded more
 *  than one map. */
export function pickedMultiMapDemos(demos) {
  return (demos || []).filter((d) => isMultiMapDemo(d) && (d.streaks || []).some((s) => s.selected === true));
}

/**
 * The row's multi-map hint, or null for a one-map demo: the "2 maps · Split"
 * button, or the "Already split" note once `status` (split_status.js's
 * checker) finds the pieces next to the demo. It shows the last answer at
 * once and asks again, so a split made or deleted since shows on the next
 * redraw. Clicking the button runs `onSplit(demo, update)` and doesn't
 * select the row; the first `update(SplitProgress)` swaps the button for a
 * progress bar until it ends.
 */
export function multiMapHint(demo, onSplit, status = splitStatus) {
  if (!isMultiMapDemo(demo) || !onSplit) return null;
  const hint = document.createElement('span');
  hint.className = 'multimap-hint';
  const btn = splitButton(demo, onSplit);
  const note = document.createElement('span');
  note.className = 'multimap-split-note';
  note.textContent = STRINGS.WORKSPACE.MULTI_MAP_ALREADY_SPLIT;
  hint.append(btn, note);
  const show = (answer) => {
    if (btn.disabled) return; // splitting: its bar stays until it ends
    const title = alreadySplitNoteTitle(demo, answer);
    btn.hidden = title !== null;
    note.hidden = title === null;
    if (title !== null) note.title = title;
  };
  show(status.last(demo.path));
  status.check(demo.path, demo.signon_maps).then(show);
  return hint;
}

/** The "Already split" note's tooltip for `answer` (split_status.js's, or
 *  null when unknown), naming the files; null while the row should offer
 *  the Split button instead. */
export function alreadySplitNoteTitle(demo, answer) {
  return isAlreadySplit(answer) ? STRINGS.WORKSPACE.multiMapAlreadySplitTitle(demo.signon_maps, answer.files) : null;
}

/** Why the right-click menu's Split Maps can't run for this queue row, from
 *  its hint: the "Already split" note's tooltip (naming the pieces) while
 *  that note shows, a line while the row's split runs, else false. */
export function splitBlockedReason(row) {
  const note = row.querySelector('.multimap-split-note');
  if (note && !note.hidden) return note.title || STRINGS.WORKSPACE.MULTI_MAP_ALREADY_SPLIT;
  return row.querySelector('.multimap-split-btn')?.disabled ? STRINGS.WORKSPACE.MULTI_MAP_SPLITTING : false;
}

/** The "2 maps · Split" button itself. */
function splitButton(demo, onSplit) {
  const btn = document.createElement('button');
  btn.type = 'button';
  btn.className = 'multimap-split-btn';
  btn.textContent = STRINGS.WORKSPACE.multiMapSplitButton(demo.signon_maps.length);
  btn.title = STRINGS.WORKSPACE.multiMapSplitTitle(demo.signon_maps);
  btn.addEventListener('click', async (e) => {
    e.stopPropagation(); // do not select the row
    btn.disabled = true;
    let bar = null;
    const update = (p) => {
      if (!bar) {
        bar = splitProgressBar();
        bar.el.classList.add('multimap-split-progress');
        btn.after(bar.el);
        btn.hidden = true;
      }
      bar.update(p);
    };
    try {
      await onSplit(demo, update);
    } finally {
      bar?.el.remove();
      btn.hidden = false;
      btn.disabled = false;
    }
  });
  return btn;
}
