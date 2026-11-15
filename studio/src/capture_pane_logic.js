import { STRINGS } from './strings.js';
import { BYTES_PER_MB } from './byte_size.js';

/** How many not-yet-created folders the launch banner names before "...and N more". */
export const MAX_PROBLEM_PATHS_SHOWN = 3;

export function formatPreviewSize(bytes) {
  return STRINGS.CAPTURE.megabytesLabel((bytes / BYTES_PER_MB).toFixed(2));
}
