// capture_mode_default.js
// The capture mode of a new install: Video (Ut Video) when HLAE's FFmpeg is
// found, else Frame sequence, which records with any HLAE. The settings file
// keeps `capture_mode` empty until the mode is settled, so the check can run
// once HLAE's path is known (the Setup window asks for it after first start).
// Until then the select shows Frame sequence and a batch captures that way.
// Picking a mode by hand, or HLAE's FFmpeg being found, settles it for good.

let undecided = false;

/** The mode the select shows for a loaded settings file, and whether it is
 *  still to be decided (no `capture_mode` and no older `ffmpeg_capture`). */
export function loadCaptureMode(settings) {
  undecided = !settings?.capture_mode && !settings?.ffmpeg_capture;
  if (settings?.capture_mode) return settings.capture_mode;
  return settings?.ffmpeg_capture ? 'direct_to_video' : 'frame_sequence';
}

/** What persistAppSettings saves: empty while undecided. */
export function captureModeToSave(shown) {
  return undecided ? '' : shown;
}

export function captureModeUndecided() {
  return undecided;
}

/** The user picked a mode in the select: it stands from now on. */
export function captureModeChosen() {
  undecided = false;
}

/**
 * The mode to settle on for a `check_hlae_ffmpeg` answer, or null to keep
 * waiting: there's no HLAE install to ask yet, or its hook DLL is missing (a
 * wrong exe). Found FFmpeg settles on Video; none yet keeps waiting, so
 * pointing HLAE at FFmpeg later (Setup's next step) still picks Video.
 */
export function settledCaptureMode(check) {
  if (!check || check.missing_hook_dll) return null;
  return check.usable ? 'direct_to_video' : null;
}

/**
 * Settles an undecided mode from a check answer. Returns the mode it set, or
 * null when nothing changed. `select` is #config-capture-mode.
 */
export function settleCaptureMode(check, select) {
  if (!undecided || !select) return null;
  const mode = settledCaptureMode(check);
  if (!mode) return null;
  undecided = false;
  select.value = mode;
  return mode;
}
