// project_log.js
// The activity log's lines for projects (#873): what was saved or opened,
// and what the check for moved or changed demos found. One line per action,
// with counts; the paths are the ones the user picked.

/** A project file was written. */
export function projectSavedLine(path, demoCount) {
  return `[project] saved ${path}: ${demoCount} demo(s)`;
}

/** A project file was opened. */
export function projectOpenedLine(path, demoCount) {
  return `[project] opened ${path}: ${demoCount} demo(s)`;
}

/** A project file could not be written or opened. */
export function projectFailedLine(action, path, err) {
  return `[project] could not ${action}${path ? ` ${path}` : ''}: ${err}`;
}

/** A new, empty session replaced the open one. */
export const NEW_SESSION_LINE = '[project] new session';

/** What the search for a project's missing demos found. */
export function missingDemosLine(checked, missing, foundElsewhere) {
  if (missing === 0) return `[project] all ${checked} demo(s) are where the project says`;
  return `[project] ${missing} of ${checked} demo(s) are missing, ${foundElsewhere} found elsewhere`;
}

/** Demos replaced on disk since they were scanned. */
export function changedDemosLine(count, willRescan) {
  return `[project] ${count} demo(s) changed since they were scanned${willRescan ? ', rescanning' : ', left as they are'}`;
}
