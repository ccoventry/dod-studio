// status_colors.js
// One colour per highlight status (#527), shared by Highlight Details' status
// dropdown and the Master Demo Queue's Pending / Captured / Rendered columns,
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

/** A Master Demo Queue count column: the status's colour, or grey at zero. */
export function statusCountColor(status, count) {
  return count > 0 ? statusColor(status) : STATUS_COLORS.None;
}
