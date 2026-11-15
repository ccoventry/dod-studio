// capture_field_errors.js
// Puts the reason Start Capture is blocked on the Configuration field that
// causes it (#25): a blank hl.exe or hlae.exe path, no Capture Output folder.
// Start is on the Capture tab and the fields are on Configuration, so the
// banner and toasts stay; this is what the user finds when they go there.

import { STRINGS } from './strings.js';
import { setFieldError, clearFieldError } from './field_error.js';

/**
 * The reason for each field, '' when that field is fine.
 * AGR FPS counts only in AGR mode (no default stands in for it there); the
 * other modes ignore the box.
 * @param {{ hlPath: string, hlaePath: string, driveCount: number, captureMode?: string, agrFps?: number }} s
 */
export function captureFieldProblems({ hlPath, hlaePath, driveCount, captureMode, agrFps }) {
  const F = STRINGS.CAPTURE_FIELDS;
  return {
    hl: hlPath?.trim() ? '' : F.HL_PATH_EMPTY,
    hlae: hlaePath?.trim() ? '' : F.HLAE_PATH_EMPTY,
    drive: driveCount > 0 ? '' : F.OUTPUT_DIR_EMPTY,
    agr: captureMode === 'agr' && !(Number.isFinite(agrFps) && agrFps > 0) ? F.AGR_FPS_EMPTY : '',
  };
}

// The hl.exe path input is hidden (the install list sets it), so the mark
// goes on the visible list.
const FIELDS = [
  ['hl', '#hl-install-choice', null],
  ['hlae', '#hlae-path-input', null],
  ['drive', '#drive-path-input', '#target-drive-list'],
  ['agr', '#config-agr-fps', null],
];

/**
 * Marks or clears the capture fields from the current form. Only touches a
 * mark this module set, so the Render tab's own error on the same folder field
 * is left alone.
 * @param {boolean} show false clears our marks (nothing is being captured yet)
 */
export function syncCaptureFieldErrors({ hlPath, hlaePath, driveCount, captureMode, agrFps }, show = true) {
  const problems = captureFieldProblems({ hlPath, hlaePath, driveCount, captureMode, agrFps });
  for (const [key, selector] of FIELDS) {
    const input = document.querySelector(selector);
    if (!input) continue;
    const message = show ? problems[key] : '';
    if (message) {
      setFieldError(input, message, { autoClear: false });
      input.dataset.captureFieldError = '1';
    } else if (input.dataset.captureFieldError) {
      clearFieldError(input);
      delete input.dataset.captureFieldError;
    }
  }
}
