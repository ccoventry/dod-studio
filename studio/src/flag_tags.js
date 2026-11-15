// flag_tags.js
// What was going on at the flags at a highlight's kills (#1132). The scan
// keeps a list of tags per kill in `streak.flag_tags` (beside `kills`), from
// the killer's side: 'while_capping' (the killer stood in a flag's capture
// area, capping it), 'flag_defence' (the victim did) and 'cleared_flag'
// (the kill stopped a timed capture). Only what the demo shows: capping is
// known for a POV demo's recording player alone (analysis/src/capture_area.rs),
// so most kills carry none.

import { STRINGS } from './strings.js';
import { escapeHtml } from './html.js';

/** The order the chips show in. */
export const FLAG_TAGS = ['while_capping', 'flag_defence', 'cleared_flag'];

/** How many of the Kill Range's kills carry each tag, as `[tag, count]` in
 *  `FLAG_TAGS` order, only tags with a count. Empty when the highlight
 *  carries no tags (a project saved before #1132, until scanned again). */
export function flagTagCounts(streak) {
  const perKill = streak?.flag_tags;
  if (!Array.isArray(perKill) || perKill.length === 0) return [];
  const start = Math.min(streak.start_index ?? 0, perKill.length - 1);
  const end = Math.max(start, Math.min(streak.end_index ?? perKill.length - 1, perKill.length - 1));
  const range = perKill.slice(start, end + 1);
  return FLAG_TAGS
    .map((tag) => [tag, range.filter((tags) => Array.isArray(tags) && tags.includes(tag)).length])
    .filter(([, count]) => count > 0);
}

/** The Highlight Details chips for the Kills cell: one per tag, with a
 *  count when more than one kill has it, the explanation in its tooltip. */
export function flagTagChips(streak) {
  return flagTagCounts(streak)
    .map(([tag, count]) => {
      const label = STRINGS.HIGHLIGHTS.FLAG_TAG_LABELS[tag];
      const title = STRINGS.HIGHLIGHTS.FLAG_TAG_TITLES[tag];
      const text = count > 1 ? STRINGS.HIGHLIGHTS.flagTagCount(label, count) : label;
      return `<span class="flag-tag-chip" data-tag="${tag}" title="${escapeHtml(title)}">${escapeHtml(text)}</span>`;
    })
    .join('');
}
