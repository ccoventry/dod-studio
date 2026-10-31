import { STRINGS } from './strings.js';
import { BYTES_PER_MB } from './byte_size.js';

const PATH_PROBLEM_REASONS = {
  not_absolute: STRINGS.CAPTURE.pathProblem.notAbsolute,
  malformed: STRINGS.CAPTURE.pathProblem.malformed,
  not_found: STRINGS.CAPTURE.pathProblem.notFound,
  not_a_directory: STRINGS.CAPTURE.pathProblem.notADirectory,
};

const MAX_PROBLEM_PATHS_SHOWN = 3;

/** Turns a list of non-"ok" diagnostics into a point-form list of reasons
 *  (one bullet per line, `warningEl`'s `white-space: pre-line` renders the
 *  `\n`s), capped so a long invalid list doesn't turn into a wall of text.
 *  Falls back to a single plain sentence when there's just one problem. */
export function describeProblemPaths(problems) {
  const shown = problems.slice(0, MAX_PROBLEM_PATHS_SHOWN)
    .map((d) => PATH_PROBLEM_REASONS[d.status]?.(d.path) || STRINGS.CAPTURE.pathProblem.unusable(d.path));
  const remaining = problems.length - MAX_PROBLEM_PATHS_SHOWN;
  if (remaining > 0) shown.push(STRINGS.CAPTURE.andNMore(remaining));
  return shown.length === 1 ? shown[0] : shown.map((s) => `• ${s}`).join('\n');
}

export function formatPreviewSize(bytes) {
  return STRINGS.CAPTURE.megabytesLabel((bytes / BYTES_PER_MB).toFixed(2));
}
