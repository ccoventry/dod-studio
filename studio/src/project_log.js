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

/** A batch started: the project stays open until it ends. */
export function batchHeldLine() {
  return '[project] a batch started: holding Open, New, Clear and Restart until it ends';
}

/** The batch ended: how long it held the project, and what was turned away. */
export function batchReleasedLine(elapsedMs, refused) {
  return `[project] the batch ended after ${elapsedMs} ms: released, ${refused} attempt(s) refused`;
}

/** A project change was turned away during a batch. */
export function batchRefusedLine() {
  return '[project] refused a project change: a batch is running';
}

/** What the search for a project's missing demos found. */
export function missingDemosLine(checked, missing, foundElsewhere) {
  if (missing === 0) return `[project] all ${checked} demo(s) are where the project says`;
  return `[project] ${missing} of ${checked} demo(s) are missing, ${foundElsewhere} found elsewhere`;
}

/**
 * What a look at the queue's demo files found when something changed
 * (demo_file_recheck.js): `gone` and `back` are the names of the rows whose
 * files went or came back, `found` how many gone ones turned up under another
 * name or folder. Counts always; the names only when there are few.
 */
export function demoFilesRecheckedLine({ checked, gone, back, found, elapsedMs }) {
  const few = (names) => (names.length > 0 && names.length <= 3 ? ` (${names.join(', ')})` : '');
  return `[project] looked at ${checked} demo file(s) again in ${elapsedMs} ms: `
    + `${gone.length} gone${few(gone)}, ${found} of them found elsewhere; ${back.length} back${few(back)}`;
}

/** Review reached a demo whose file is missing and the user skipped it. */
export function reviewSkippedMissingLine(name, remaining) {
  return `[review] skipped ${name}: its file is missing; reviewing the other ${remaining} demo(s)`;
}

/** What the user chose for a missing demo when Review reached it. */
export function reviewMissingChoiceLine(name, choice) {
  return `[review] ${name} is missing: ${choice}`;
}

/** Demos replaced on disk since they were scanned. */
export function changedDemosLine(count, willRescan) {
  return `[project] ${count} demo(s) changed since they were scanned${willRescan ? ', rescanning' : ', left as they are'}`;
}

/** A finished render was added to the project's record of its videos (#1264). */
export function renderRecordedLine(takeKey, outputPath) {
  return `[project] recorded a render of ${takeKey}: ${outputPath}`;
}

/** What opening a project picked up from its take folders' render histories. */
export function rendersPickedUpLine(takes, added, elapsedMs) {
  return `[project] read the render history of ${takes} take folder(s): ${added} video(s) new to the project (${elapsedMs} ms)`;
}

/** The renders opening found were the only change, so they were saved with
 *  the open (#1345). */
export function rendersSavedOnOpenLine(path, added, marked) {
  return `[project] saved ${path} with the ${added} video(s) found in its take folders; ${marked} highlight(s) now Rendered`;
}

/** The take folders' render histories could not be read. */
export function rendersPickUpFailedLine(takes) {
  return `[project] could not read the render history of ${takes} take folder(s)`;
}

/** A save is about to raise a project's format: the backup comes first. */
export function projectBackupStartedLine(path, fromSchema, toSchema) {
  return `[project] updating ${path} from format ${fromSchema} to ${toSchema}: backing it up first`;
}

/** The backup before a format change is on disk. */
export function projectBackedUpLine(backup, elapsedMs) {
  return `[project] backed up to ${backup} (${elapsedMs} ms)`;
}

/** The one-time summary of a project's first save in a newer format. */
export function projectUpdatedLine(path, { clips, takesFound, takesNotFound }, backup) {
  const takes = takesFound === null ? 'takes not checked' : `${takesFound} take(s) found, ${takesNotFound} not found`;
  return `[project] updated ${path}: ${clips} clip(s), ${takes}, backup ${backup}`;
}

/** A project from a newer build opened: its unknown data is kept, unused. */
export function projectNewerLine(path, schema, keptKeys) {
  return `[project] ${path} is format ${schema}, newer than this build: keeping ${keptKeys} unknown key(s)`;
}

/** A save refused because the file needs a newer build to save over it. */
export function projectNeedsCopyLine(path) {
  return `[project] ${path} needs a newer build to save over it: left as it is`;
}
