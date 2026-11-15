// movie_copy.js
// Setup's Make a movie copy (#939): the rules the dialog applies before and
// while copying. Pure, so they are tested without a DOM; movie_copy_ui.js
// draws the dialog and runs the copy (native::shared::movie_copy does it).
import { STRINGS } from './strings.js';
import { formatBytes } from './setup_checklist.js';

/** The name Setup suggests, when the backend hasn't said. */
export const DEFAULT_COPY_NAME = 'dod_movies';

// Windows device names: no folder can carry them.
const RESERVED = new Set([
  'con', 'prn', 'aux', 'nul',
  ...Array.from({ length: 9 }, (_, i) => `com${i + 1}`),
  ...Array.from({ length: 9 }, (_, i) => `lpt${i + 1}`),
]);

/**
 * Why `name` can't be the copy's folder, or ''. The same rules as
 * native::shared::movie_copy::copy_name_problem, which checks again before
 * copying; `existing` is every name in the Half-Life folder, matched
 * without case, as Windows does.
 */
export function copyNameProblem(name, existing = []) {
  const S = STRINGS.SETUP.MOVIE_COPY;
  const value = typeof name === 'string' ? name : '';
  if (!value) return S.NAME_EMPTY;
  if (!/^[A-Za-z0-9_]+$/.test(value)) return S.NAME_BAD_CHAR;
  if (RESERVED.has(value.toLowerCase())) return S.NAME_RESERVED;
  const taken = (existing || []).find((n) => n.toLowerCase() === value.toLowerCase());
  if (taken) return S.nameExists(taken);
  return '';
}

/** `base`, or `base2`, `base3`… when the Half-Life folder already has it,
 *  so a second copy starts on a name it can take. */
export function suggestCopyName(existing = [], base = DEFAULT_COPY_NAME) {
  const taken = new Set((existing || []).map((n) => n.toLowerCase()));
  if (!taken.has(base.toLowerCase())) return base;
  for (let i = 2; ; i += 1) {
    if (!taken.has(`${base}${i}`.toLowerCase())) return `${base}${i}`;
  }
}

/** The bytes the copy writes. */
export function copyBytes(plan, includeDemos) {
  if (!plan) return 0;
  return (plan.bytes || 0) + (includeDemos ? plan.demo_bytes || 0 : 0);
}

/** Whether the copy fits with the backend's headroom to spare; an unknown
 *  free space doesn't block (the backend checks again). */
export function copyFits(plan, includeDemos) {
  if (!plan || typeof plan.free_bytes !== 'number') return true;
  return copyBytes(plan, includeDemos) + (plan.headroom_bytes || 0) <= plan.free_bytes;
}

/**
 * Everything the dialog shows before Copy, from movie_copy_plan's answer.
 * @returns {{ nameProblem: string, sizeLine: string, demosLine: string,
 *   spaceLine: string, fits: boolean, canCopy: boolean }}
 */
export function copyCheck(plan, name, includeDemos) {
  const S = STRINGS.SETUP.MOVIE_COPY;
  const nameProblem = copyNameProblem(name, plan?.existing);
  if (!plan) {
    return { nameProblem, sizeLine: '', demosLine: '', spaceLine: '', fits: true, canCopy: false };
  }
  const files = (plan.files || 0) + (includeDemos ? plan.demo_files || 0 : 0);
  const bytes = copyBytes(plan, includeDemos);
  const fits = copyFits(plan, includeDemos);
  let spaceLine;
  if (typeof plan.free_bytes !== 'number') spaceLine = S.FREE_UNKNOWN;
  else if (!fits) spaceLine = S.noRoom(formatBytes(bytes), formatBytes(plan.free_bytes));
  else spaceLine = S.free(formatBytes(plan.free_bytes));
  return {
    nameProblem,
    sizeLine: S.size(files, formatBytes(bytes)),
    demosLine: plan.demo_files ? S.demosAdd(plan.demo_files, formatBytes(plan.demo_bytes)) : '',
    spaceLine,
    fits,
    canCopy: !nameProblem && fits,
  };
}

/** The bar's width, 0-100, from a `movie_copy_progress` event. */
export function progressPercent(p) {
  if (!p || !(p.total_bytes > 0)) return p && p.done_files >= p.total_files ? 100 : 0;
  return Math.min(100, Math.max(0, (p.done_bytes / p.total_bytes) * 100));
}

/** The line under the bar: bytes so far and the file being copied. */
export function progressLine(p) {
  if (!p) return STRINGS.SETUP.MOVIE_COPY.STARTING;
  return STRINGS.SETUP.MOVIE_COPY.progress(formatBytes(p.done_bytes), formatBytes(p.total_bytes), p.file || '');
}
