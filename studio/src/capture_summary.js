// capture_summary.js
// The capture summary strip (#443): one line above Start Capture Batch that
// says how the batch is about to run, each part linking to its setting.
// Pure, so it can be tested on its own; capture_summary_ui.js draws it.

import { STRINGS } from './strings.js';

/**
 * The strip's parts, in order: `{ key, text, tab, field, blocking }`, where
 * `tab` is the Configuration tab (a `.config-tab-btn`'s `data-tab`) and
 * `field` the selector to focus there. `blocking` marks a part that keeps
 * Start Capture Batch disabled.
 *
 * `setup`: `{ mode, codecLabel, obsFps, width, height, fps, scheduledCount,
 * bannedCount, decalFlush, destinations }`.
 */
export function summaryParts(setup) {
  const s = STRINGS.CAPTURE_SUMMARY;
  const parts = [];
  const mode = setup.mode || 'frame_sequence';
  let modeText;
  if (mode === 'direct_to_video') modeText = s.modeVideo(setup.codecLabel || '');
  else if (mode === 'obs') modeText = s.modeObs(setup.obsFps);
  else modeText = s.MODE_FRAMES;
  parts.push({ key: 'mode', text: modeText, tab: 'tab-output-format', field: '#config-capture-mode' });
  parts.push({
    key: 'format',
    text: s.format(setup.width, setup.height, setup.fps),
    tab: 'tab-output-format',
    field: '#config-res-width',
  });
  if (setup.bannedCount > 0) {
    parts.push({
      key: 'banned',
      text: s.banned(setup.bannedCount),
      tab: 'tab-custom-commands',
      field: '#tab-custom-commands input, #tab-custom-commands textarea',
      blocking: true,
    });
  }
  parts.push({
    key: 'scheduled',
    text: s.scheduled(setup.scheduledCount || 0),
    tab: 'tab-custom-commands',
    field: '#tab-custom-commands input, #tab-custom-commands textarea',
  });
  parts.push({
    key: 'decals',
    text: setup.decalFlush ? s.DECALS_CLEARED : s.DECALS_KEPT,
    tab: 'tab-pipeline',
    field: '#config-decal-flush',
  });
  if (!setup.destinations) {
    parts.push({
      key: 'destination',
      text: s.NO_DESTINATION,
      tab: 'tab-drive-overrides',
      field: '#drive-path-input',
      blocking: true,
    });
  }
  return parts;
}
