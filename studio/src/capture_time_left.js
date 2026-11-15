// capture_time_left.js
// The capture batch's time left (#643): the backend's estimate
// (studio/src-tauri/src/batch_eta.rs, sent with capture_status as
// `seconds_left` at each clip boundary), counted down between reports and
// rounded to minutes.

import { STRINGS } from './strings.js';

/**
 * @param {{seconds: number, at: number} | null} estimate  the last estimate
 *   and when it arrived (ms)
 * @param {number} now  ms
 * @returns {string}  " · about 14 min left", or '' with no estimate
 */
export function timeLeftSuffix(estimate, now) {
  if (!estimate || !Number.isFinite(estimate.seconds)) return '';
  const left = Math.max(0, estimate.seconds - (now - estimate.at) / 1000);
  if (left < 60) return STRINGS.CAPTURE.TIME_LEFT_UNDER_A_MINUTE;
  return STRINGS.CAPTURE.timeLeftMinutes(Math.round(left / 60));
}
