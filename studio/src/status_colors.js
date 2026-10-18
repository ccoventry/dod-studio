// status_colors.js
// One colour per highlight status (#527), shared by Highlight Details' status
// dropdown and the Master Demo Queue's Status counts (queue_counts.js),
// so the two can't drift apart again.

/**
 * A highlight's status, as stored on the streak and saved in projects. The
 * same words are the dropdown's text (STRINGS.HIGHLIGHTS.STATUS_OPTIONS);
 * these are for comparing (#35).
 */
export const HIGHLIGHT_STATUS = Object.freeze({
  NONE: 'None',
  PENDING: 'Pending',
  CAPTURED: 'Captured',
  RENDERED: 'Rendered',
});

export const STATUS_COLORS = Object.freeze({
  [HIGHLIGHT_STATUS.NONE]: '#555',
  [HIGHLIGHT_STATUS.PENDING]: '#ffa726',
  [HIGHLIGHT_STATUS.CAPTURED]: '#2196f3',
  [HIGHLIGHT_STATUS.RENDERED]: '#4caf50',
});

/** A status's colour; an unknown status gets None's grey. */
export function statusColor(status) {
  return STATUS_COLORS[status] ?? STATUS_COLORS.None;
}

/** A Master Demo Queue count: the status's colour, or grey at zero. */
export function statusCountColor(status, count) {
  return count > 0 ? statusColor(status) : STATUS_COLORS.None;
}

/** One colour per Review mark (#44, #661), plus no mark, for the queue's
 *  Review counts; none of them is a Status colour, so the two columns side by
 *  side never read as each other. */
export const REVIEW_COLORS = Object.freeze({
  Unmarked: '#e0e0e0',
  Keep: '#26c6da',
  Maybe: '#ffd54f',
  Skip: '#ef5350',
});

/** A Review count: the mark's colour, or grey at zero. */
export function reviewCountColor(mark, count) {
  return count > 0 ? (REVIEW_COLORS[mark] ?? STATUS_COLORS.None) : STATUS_COLORS.None;
}
