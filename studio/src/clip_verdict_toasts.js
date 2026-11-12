// clip_verdict_toasts.js
// The toasts that name the highlights of a batch's failed and short clips
// (#1401), from capture_takes_verified. The backend's take check puts the
// game's verdict on each take (`clip_verdict`): `{ failed: 'no_start' }` and
// the like for a clip with nothing usable recorded (not captured), or
// `{ short: { recorded_secs, planned_secs, kills_outside } }` for one that
// recorded less than planned (captured, with this warning). The summary
// toast counts them; these say which highlights they are, so the user
// knows what to look at or capture again. toast.js logs each one.

import { STRINGS } from './strings.js';
import { takeLabel } from './batch_results.js';

/// How many highlights a toast names before it says "and N more": Last
/// Batch lists every take.
export const NAMED_IN_A_TOAST = 3;

function named(blocks, dispatch) {
  const labels = blocks.slice(0, NAMED_IN_A_TOAST).map((b) => takeLabel(b, dispatch));
  return [labels, Math.max(0, blocks.length - NAMED_IN_A_TOAST)];
}

/// `[{ text, type }]`: one toast for the failed clips, one for the short
/// ones, each only when there are some.
export function clipVerdictToasts(payload, dispatch) {
  const blocks = payload?.blocks || [];
  const failed = blocks.filter((b) => b.clip_verdict?.failed && !b.captured);
  const short = blocks.filter((b) => b.clip_verdict?.short && b.captured);
  const toasts = [];
  if (failed.length) toasts.push({ text: STRINGS.CAPTURE.clipsFailed(...named(failed, dispatch)), type: 'error' });
  if (short.length) toasts.push({ text: STRINGS.CAPTURE.clipsShort(...named(short, dispatch)), type: 'warning' });
  return toasts;
}

/// How many takes the game reported failed, for the summary toast.
export function failedClipCount(payload) {
  return (payload?.blocks || []).filter((b) => b.clip_verdict?.failed && !b.captured).length;
}
