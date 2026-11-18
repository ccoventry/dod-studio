// capture_in_effect.js
// The capture mode, size and OBS rate that Start's checks read (#1085):
// Configuration's fields with the open project's values over them, the same
// values the batch payload takes (project_settings.js's effectiveSettings).
// So Start never launches OBS for a project that captures frames, never skips
// OBS's pre-flight for one that captures with OBS, and the OBS check sets OBS
// up at the size and rate the batch records at.

import { effectiveSettings } from './project_settings.js';
import { numberField } from './number_field.js';

/** The capture mode a batch started now uses. */
export function captureModeInEffect() {
  return effectiveSettings({ capture_mode: document.querySelector('#config-capture-mode')?.value }).capture_mode;
}

/** The game size and OBS frame rate a batch started now uses, as the OBS
 *  check takes them: `{ gameWidth, gameHeight, obsCaptureFps }`. */
export function obsSizeAndRateInEffect() {
  const own = effectiveSettings({
    resolution_width: numberField('#config-res-width', 1280, { integer: true, positive: true }),
    resolution_height: numberField('#config-res-height', 720, { integer: true, positive: true }),
    obs_capture_fps: numberField('#config-obs-capture-fps', 120, { integer: true, positive: true }),
  });
  return { gameWidth: own.resolution_width, gameHeight: own.resolution_height, obsCaptureFps: own.obs_capture_fps };
}
