// camera_data.js
// Demos with no camera data (#920). A file rebuilt from server traffic keeps
// every server message, so its kills and scoreboard read fine, but none of
// its frames carries the recorder's view: it plays, previews and captures as
// a frozen camera at the map origin. Each scanned demo carries
// `no_camera_data` from the analyzer; the Master Queue marks those demos, a
// capture batch or a Launch Preview asks before starting on one, and the
// Demo Analyzer says so above its tabs.

import { themedConfirm } from './themed_confirm.js';
import { STRINGS } from './strings.js';

/** Whether a scanned demo (or an analyzer report's `demo_info`) has no
 *  camera data. One scanned before the check existed has no flag, and isn't
 *  marked. */
export function hasNoCameraData(demo) {
  return demo?.no_camera_data === true;
}

/**
 * Asks before starting on demos with no camera data. Resolves true to go
 * ahead: none has that problem, or the user chose to anyway.
 */
export async function confirmNoCameraData(demos, { preview = false } = {}) {
  const frozen = (demos || []).filter(hasNoCameraData);
  if (!frozen.length) return true;
  const S = STRINGS.CAMERA_DATA;
  return themedConfirm(preview ? S.PREVIEW_MESSAGE : S.batchMessage(frozen.length), {
    title: preview ? S.PREVIEW_TITLE : S.batchTitle(frozen.length),
    confirmLabel: preview ? S.PREVIEW_ANYWAY : S.START_ANYWAY,
    details: frozen.map((d) => ({ primary: d.name, secondary: S.DETAIL })),
  });
}

/** The Master Queue's mark for a demo with no camera data, or null. */
export function noCameraDataBadge(demo) {
  if (!hasNoCameraData(demo)) return null;
  const badge = document.createElement('span');
  badge.className = 'no-camera-badge';
  badge.textContent = STRINGS.CAMERA_DATA.BADGE;
  badge.title = STRINGS.CAMERA_DATA.BADGE_TITLE;
  return badge;
}

/** The Demo Analyzer's notice for `report`, or null. */
export function noCameraDataNotice(report) {
  return hasNoCameraData(report?.demo_info) ? STRINGS.CAMERA_DATA.ANALYZER_NOTICE : null;
}

/** Fills `el` with the notice for `report`, or hides it. */
export function renderNoCameraDataBanner(el, report) {
  if (!el) return;
  const notice = noCameraDataNotice(report);
  el.hidden = !notice;
  el.textContent = notice || '';
}
