// status_colors.js
// One colour per highlight status (#527), shared by Highlight Details' status
// dropdown and the Master Demo Queue's Pending / Captured / Rendered columns,
// so the two can't drift apart again.

export const STATUS_COLORS = Object.freeze({
  None: '#555',
  Pending: '#ffa726',
  Captured: '#2196f3',
  Rendered: '#4caf50',
});

/** A status's colour; an unknown status gets None's grey. */
export function statusColor(status) {
  return STATUS_COLORS[status] ?? STATUS_COLORS.None;
}

/** A Master Demo Queue count column: the status's colour, or grey at zero. */
export function statusCountColor(status, count) {
  return count > 0 ? statusColor(status) : STATUS_COLORS.None;
}
