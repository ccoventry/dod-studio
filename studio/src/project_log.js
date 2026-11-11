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
export function projectUpdatedLine(path, { clips, takesFound, takesNotFound, settingsCopied = false }, backup) {
  const takes = takesFound === null ? 'takes not checked' : `${takesFound} take(s) found, ${takesNotFound} not found`;
  const settings = settingsCopied ? ', settings copied from Configuration' : '';
  return `[project] updated ${path}: ${clips} clip(s), ${takes}${settings}, backup ${backup}`;
}

/** A project from a newer build opened: its unknown data is kept, unused. */
export function projectNewerLine(path, schema, keptKeys) {
  return `[project] ${path} is format ${schema}, newer than this build: keeping ${keptKeys} unknown key(s)`;
}

/** A save refused because the file needs a newer build to save over it. */
export function projectNeedsCopyLine(path) {
  return `[project] ${path} needs a newer build to save over it: left as it is`;
}

/** A project without its own settings opened: Configuration's were copied in
 *  (#1085, decision 6), and are saved with it on the next save. */
export function projectSettingsCopiedLine(path) {
  return `[project] ${path} had no settings of its own: copied in Configuration's, saved with the next save`;
}

/** A project opened whose own settings differ from Configuration's: a
 *  capture uses the project's. Names the settings, not their values. */
export function projectSettingsDifferLine(path, keys) {
  return `[project] ${path} sets ${keys.length} setting(s) other than Configuration's, used for its captures: ${keys.join(', ')}`;
}

/** Configuration edits copied into the open project (until Configuration
 *  shows a read-only "This project sets" line instead). */
export function projectSettingsFollowedLine(keys) {
  return `[project] Configuration change copied into the open project: ${keys.join(', ')}`;
}

/** The project a Project settings page line is about: its file, or a
 *  session not saved yet. */
const pageProject = (path) => path || 'an untitled session';

/** The Project settings page opened (#1085). */
export function projectSettingsPageOpenedLine(path) {
  return `[project settings] Opened for ${pageProject(path)}`;
}

/** Pending edits written into the open project by `how` (Apply, OK, or
 *  Apply in the leave prompt). Names the settings, not their values. */
export function projectSettingsPageAppliedLine(path, how, keys) {
  return `[project settings] ${how}: ${keys.length} setting(s) changed in ${pageProject(path)}: ${keys.join(', ')}`;
}

/** What the project's Clip settings hold after an Apply that changed them
 *  (#1085 step 8): the preset it started from and how many lines, counts
 *  rather than the lines themselves. */
export function clipSettingsAppliedLine(path, { preset, initCount, scheduledCount, fov }) {
  const from = preset ? `from ${preset}` : 'no preset';
  const view = fov === undefined || fov === null ? 'field of view not set' : `field of view ${fov}`;
  return `[project settings] Clip settings for ${pageProject(path)}: ${from}, ${initCount} line(s), ${scheduledCount} at each clip, ${view}`;
}

/** A Clip settings preset saved on this PC from the Project settings page. */
export function clipPresetSavedLine(name, lists) {
  return `[project settings] Saved the Clip settings preset ${name} (${(lists.init_commands || []).length} line(s), ${(lists.custom_commands || []).length} at each clip)`;
}

/** The page closed by `how` (OK, Cancel, Discard, a project opening), with
 *  how many unapplied edits it dropped. */
export function projectSettingsPageClosedLine(path, how, dropped) {
  return `[project settings] Closed (${how}) for ${pageProject(path)}, ${dropped} unapplied change(s) dropped`;
}

/** The answer to "This project was made with ... Switch?". */
export function projectInstallOfferLine(path, madeWith, answer) {
  return `[project] ${path} was made with ${madeWith}: ${answer}`;
}
