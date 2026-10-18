// queue_counts.js
// The Master Demo Queue's two count columns: Status (Pending / Captured /
// Rendered) and Review (unmarked / Keep / Maybe / Skip). Each is one cell of
// numbers separated by slashes, every number in its own colour, so five
// columns' worth of counts fit in two. The header carries a matching legend
// (the same letters in the same colours), and each cell's tooltip spells the
// numbers out in words.

import { HIGHLIGHT_STATUS, STATUS_COLORS, statusCountColor, REVIEW_COLORS, reviewCountColor } from './status_colors.js';
import { CURATION } from './take_index.js';
import { STRINGS } from './strings.js';

/** A highlight's status as the queue counts it: unset counts as None. */
const statusOf = (s) => s.status || HIGHLIGHT_STATUS.NONE;

/** The Status column's parts, in display order. */
export function statusCounts(streaks) {
  const list = streaks || [];
  return [HIGHLIGHT_STATUS.PENDING, HIGHLIGHT_STATUS.CAPTURED, HIGHLIGHT_STATUS.RENDERED].map((status) => {
    const count = list.filter((s) => statusOf(s) === status).length;
    return { key: status, count, color: statusCountColor(status, count), label: STRINGS.WORKSPACE.COUNT_LABELS[status] };
  });
}

/** The Review column's parts, in display order: unmarked first. */
export function reviewCounts(streaks) {
  const list = streaks || [];
  return [null, CURATION.KEEP, CURATION.MAYBE, CURATION.SKIP].map((mark) => {
    const count = list.filter((s) => (s.curation || null) === mark).length;
    const key = mark || 'Unmarked';
    return { key, count, color: reviewCountColor(key, count), label: STRINGS.WORKSPACE.COUNT_LABELS[key] };
  });
}

/** "2 Pending, 5 Captured, 0 Rendered": a count cell's tooltip. */
export function countsTitle(parts) {
  return parts.map((p) => `${p.count} ${p.label}`).join(', ');
}

/** Spans for `parts`, each number in its colour, joined by grey slashes. */
function fillCounts(el, parts, text) {
  parts.forEach((p, i) => {
    if (i > 0) {
      const slash = document.createElement('span');
      slash.className = 'count-slash';
      slash.textContent = ' / ';
      el.appendChild(slash);
    }
    const span = document.createElement('span');
    span.style.color = p.color;
    span.textContent = text(p);
    el.appendChild(span);
  });
}

/** One count cell, e.g. 2 / 5 / 0, with its tooltip. */
export function countsCell(parts) {
  const td = document.createElement('td');
  td.className = 'queue-counts-cell';
  td.title = countsTitle(parts);
  fillCounts(td, parts, (p) => `${p.count}`);
  return td;
}

/**
 * Fills a header's legend (`.count-legend` inside the th) with the parts'
 * short letters in their colours, never greyed: P / C / R, – / K / M / S.
 */
export function fillLegend(el, parts) {
  if (!el) return;
  el.textContent = '';
  const full = parts.map((p) => ({ ...p, color: p.key in STATUS_COLORS ? STATUS_COLORS[p.key] : REVIEW_COLORS[p.key] }));
  fillCounts(el, full, (p) => STRINGS.WORKSPACE.COUNT_LETTERS[p.key]);
}

/** Draws both headers' legends; called once at start-up. */
export function initCountLegends() {
  fillLegend(document.querySelector('#status-count-legend'), statusCounts([]));
  fillLegend(document.querySelector('#review-count-legend'), reviewCounts([]));
}
