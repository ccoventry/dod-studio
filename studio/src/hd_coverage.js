// hd_coverage.js — how much of the game each HD style covers (#426), from
// the HD page's status report. Pure, so it can be tested on its own.
//
// There is no list of every texture the game has to measure against, so a
// type's "whole" is the most files any style has built for it. A style with
// fewer is partial: the game shows the stock texture wherever it has none.

import { STRINGS } from './strings.js';

const OVERRIDES = 'overrides';

/** For each asset type, the most files any style (not `overrides`) has. */
export function mostPerType(status) {
  const most = {};
  for (const t of status?.types || []) {
    most[t.asset_type] = Math.max(0, ...t.folders.filter((f) => f.name !== OVERRIDES).map((f) => f.files));
  }
  return most;
}

/** What one style is missing next to the fullest style, per type:
 *  `{ asset_type, files, most }` for each type it has fewer files of, in the
 *  status report's type order. Empty when the style is complete, or built
 *  nowhere at all (the style list already says "not built yet"). */
export function styleGaps(status, style) {
  const most = mostPerType(status);
  const gaps = [];
  let builtAnywhere = false;
  for (const t of status?.types || []) {
    const files = t.folders.find((f) => f.name === style)?.files || 0;
    if (files > 0) builtAnywhere = true;
    if (files < most[t.asset_type]) gaps.push({ asset_type: t.asset_type, files, most: most[t.asset_type] });
  }
  return builtAnywhere ? gaps : [];
}

/** One sentence naming a partial style's gaps, or '' for a complete one. */
export function gapsSentence(style, gaps) {
  if (!gaps.length) return '';
  const name = (type) => STRINGS.HD.TYPE_NAMES_LOWER[type] || type;
  const none = gaps.filter((g) => g.files === 0).map((g) => name(g.asset_type));
  const some = gaps.filter((g) => g.files > 0)
    .map((g) => STRINGS.HD.someOf(g.files.toLocaleString(), g.most.toLocaleString(), name(g.asset_type)));
  return STRINGS.HD.styleGaps(style, none, some);
}
