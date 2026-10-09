// queue_multimap.js
// Master Demo Queue demos that recorded more than one map (#217). The game's
// `viewdemo` stops at the first level change, so a capture can't reach past
// the first map, and the analysis behind Highlight Details covers one map
// only. Such a demo has to be split before it's useful: its row gets a
// "2 maps · Split" button, and Start Capture Batch refuses picks in it.
//
// The splitting itself (Split Maps' IPC, the queue swap) is main.js's,
// through the callbacks passed in here.

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

/** The row's "2 maps · Split" button, or null for a one-map demo. Clicking it
 *  runs `onSplit(demo)` and doesn't select the row. */
export function multiMapSplitButton(demo, onSplit) {
  if (!isMultiMapDemo(demo) || !onSplit) return null;
  const btn = document.createElement('button');
  btn.type = 'button';
  btn.className = 'multimap-split-btn';
  btn.textContent = STRINGS.WORKSPACE.multiMapSplitButton(demo.signon_maps.length);
  btn.title = STRINGS.WORKSPACE.multiMapSplitTitle(demo.signon_maps);
  btn.addEventListener('click', async (e) => {
    e.stopPropagation(); // do not select the row
    btn.disabled = true;
    try {
      await onSplit(demo);
    } finally {
      btn.disabled = false;
    }
  });
  return btn;
}
