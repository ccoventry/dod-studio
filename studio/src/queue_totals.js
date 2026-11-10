// queue_totals.js
// The Master Demo Queue's totals row: one row under the header, each total
// under the column it adds up (Demo File, Highlights, Selected, Status,
// Review), for the demos the list shows. Under the header rather than at the
// end, so a long queue's totals are on screen without scrolling.

import { listedStreaks } from './queue_filters.js';
import { statusCounts, reviewCounts, countsCell } from './queue_counts.js';
import { STRINGS } from './strings.js';

const ROW_ID = 'master-totals-row';

/** What the row shows for `demos`: the same highlights each row counts. */
export function queueTotals(demos) {
  const streaks = (demos || []).flatMap((d) => listedStreaks(d));
  return {
    demos: (demos || []).length,
    highlights: streaks.length,
    selected: streaks.filter((s) => s.selected === true).length,
    status: statusCounts(streaks),
    review: reviewCounts(streaks),
  };
}

function plainCell(text, title) {
  const td = document.createElement('td');
  td.textContent = text;
  if (title) td.title = title;
  return td;
}

/**
 * Draws the row for `shown`, the demos listed after filters, out of
 * `queued` in the queue; hides it while nothing is listed.
 */
export function renderQueueTotals(shown, queued) {
  const head = document.querySelector('#master-demo-table thead');
  if (!head) return;
  let row = document.getElementById(ROW_ID);
  if (!row) {
    row = document.createElement('tr');
    row.id = ROW_ID;
    row.className = 'queue-totals-row';
    head.appendChild(row);
  }
  const list = shown || [];
  row.hidden = list.length === 0;
  row.textContent = '';
  if (row.hidden) return;
  const t = queueTotals(list);
  const W = STRINGS.WORKSPACE;
  const name = plainCell(W.totalsLabel(t.demos, queued), W.TOTALS_TITLE);
  name.className = 'queue-totals-name';
  const centred = (text) => {
    const td = plainCell(text, W.TOTALS_TITLE);
    td.className = 'col-center';
    return td;
  };
  row.append(
    plainCell(''),
    name,
    centred(`${t.highlights}`),
    centred(`${t.selected}`),
    countsCell(t.status),
    countsCell(t.review),
    plainCell(''),
  );
}
