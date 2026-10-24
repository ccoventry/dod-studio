// download_progress.js
// The lines and bars of the two downloads Studio makes: an update (Help ›
// Update) and a map (the Maps needed banner). Byte counts come from the
// backend at most every ~33 ms (download_progress.rs, #957).

import { STRINGS } from './strings.js';

const MB = 1024 * 1024;

/** Bytes as MB the way Explorer counts them: one decimal under 10, whole above. */
export function megabytes(bytes) {
  const mb = Math.max(0, bytes || 0) / MB;
  if (mb >= 10) return String(Math.round(mb));
  const one = mb.toFixed(1);
  return one.endsWith('.0') ? one.slice(0, -2) : one;
}

/** "12 of 45 MB", or "12 MB" when the server gave no size. */
export function bytesText(downloaded, total) {
  return total
    ? STRINGS.DOWNLOADS.bytesOf(megabytes(downloaded), megabytes(total))
    : STRINGS.DOWNLOADS.bytes(megabytes(downloaded));
}

/** The update's step line for one `update_download_progress` event. */
export function updateDownloadLine(event) {
  const { downloaded, total } = event || {};
  if (!downloaded) return STRINGS.UPDATE_MODAL.STATUS_DOWNLOADING;
  return STRINGS.UPDATE_MODAL.statusDownloadingBytes(bytesText(downloaded, total));
}

/** A map's step line for one `map_download_progress` event (or none yet),
 *  `index` counting from 1 of `count` maps in this click. */
export function mapDownloadLine(map, index, count, event) {
  if (event?.step === 'checking') return STRINGS.MAPS.checkingLine(map);
  const bytes = event?.downloaded ? bytesText(event.downloaded, event.total) : null;
  return STRINGS.MAPS.downloadingLine(map, index, count, bytes);
}

/** A `<progress>` showing bytes, or the sliding block when there's no size. */
export function showBytes(bar, downloaded, total) {
  if (!bar) return;
  if (!total) {
    showSliding(bar);
    return;
  }
  bar.max = total;
  bar.value = Math.min(downloaded || 0, total);
}

/** A `<progress>` with no value is the browser's own sliding block: for a step
 *  that can't be measured, such as the install. */
export function showSliding(bar) {
  bar?.removeAttribute('value');
}
