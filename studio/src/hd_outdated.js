// hd_outdated.js
// The HD page's "built with an older version" lines (#1155): one per style
// whose stamped output is behind what this copy of the app would build. The
// backend (native::hd::outdated) decides what is behind; this words it. Build
// does the rest: it repaints only the part that is out of date.

import { STRINGS } from './strings.js';

/**
 * @param {Array<{style: string, step: string}> | null | undefined} outdated  the status report's `outdated`
 * @returns {string[]}  one line per style, styles in the report's order
 */
export function outdatedLines(outdated) {
  const byStyle = new Map();
  for (const { style, step } of outdated || []) {
    const what = STRINGS.HD.outdatedSteps[step];
    if (!what) continue; // a step this page has no words for (a newer backend)
    byStyle.set(style, [...(byStyle.get(style) || []), what]);
  }
  return [...byStyle].map(([style, whats]) => STRINGS.HD.outdatedStyle(style, whats.join(' and ')));
}

/** Fills `box` (hidden when there is nothing to say) from the status report's `outdated`. */
export function renderOutdated(box, outdated) {
  if (!box) return;
  const lines = outdatedLines(outdated);
  box.replaceChildren(...lines.map((text) => {
    const p = document.createElement('p');
    p.textContent = text;
    return p;
  }));
  box.hidden = !lines.length;
}
