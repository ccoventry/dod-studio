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
 * `setup`: `{ mode, codecLabel, obsFps, agrFps, width, height, fps,
 * commandProfile, scheduledCount, bannedCount, decalFlush, finish,
 * destinations }`, where `commandProfile` is `{ name, edited }` (name empty
 * when the lists are on no profile) and `finish` is `{ enabled, codecLabel }`
 * for the "When a batch finishes" step.
 */
export function summaryParts(setup) {
  const s = STRINGS.CAPTURE_SUMMARY;
  const parts = [];
  const mode = setup.mode || 'frame_sequence';
  let modeText;
  if (mode === 'direct_to_video') modeText = s.modeVideo(setup.codecLabel || '');
  else if (mode === 'obs') modeText = s.modeObs(setup.obsFps);
  else if (mode === 'agr') modeText = s.modeAgr(setup.agrFps);
  else modeText = s.MODE_FRAMES;
  parts.push({ key: 'mode', text: modeText, tab: 'tab-output-format', field: '#config-capture-mode' });
  // AGR records no pixels, so the window size and Capture FPS don't apply.
  if (mode !== 'agr') {
    parts.push({
      key: 'format',
      text: s.format(setup.width, setup.height, setup.fps),
      tab: 'tab-output-format',
      field: '#config-res-width',
    });
  }
  // The command profile the lists came from, and whether they were edited
  // since (command_profiles.js's profileState decides both).
  const profile = setup.commandProfile || {};
  parts.push({
    key: 'commands',
    text: profile.name ? s.commands(profile.name, !!profile.edited) : s.COMMANDS_NONE,
    tab: 'tab-custom-commands',
    field: '#command-profile-select',
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
  // What the finish step (#440) will save the batch's clips as. AGR captures
  // nothing the finish step could render, so the part is left out there.
  if (mode !== 'agr') {
    const finish = setup.finish || {};
    parts.push({
      key: 'finish',
      text: finish.enabled ? s.finish(finish.codecLabel || '') : s.FINISH_OFF,
      tab: 'tab-render-output',
      field: finish.enabled ? finishCodecField(mode) : '#config-finish-clips',
    });
  }
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

/** The finish codec select that applies to `mode` (finish_clips.js reads the same one). */
function finishCodecField(mode) {
  if (mode === 'obs') return '#config-finish-codec-obs';
  if (mode === 'direct_to_video') return '#config-finish-codec-video';
  return '#config-finish-codec-frames';
}
