// project_migrations.js
// Brings a project file from an older format up to this build's, and says
// what that came to (the one-time summary), as analysis/src/cache/migrations.rs
// does for the analyzer cache. Part of #1085's project-file safety.
//
// A file from schema N goes through each step from N up to CURRENT_SCHEMA, on
// its parsed JSON. Rule for every step: add to what's saved, never reshape it,
// so a build that doesn't know the new data keeps it or loses only what the
// backup covers (project_file.js).

import { takeCaptureStates } from './ipc_bridge.js';
import { knownTakeFolders } from './project_renders.js';
import { isClipInProject } from './take_index.js';
import { STRINGS } from './strings.js';

/** Schema 2 is the file saying what it is: `schema` and `saveNeeds`, stamped
 *  by project_file.js on save. Nothing already in the file changes. */
function v1ToV2() {}

/** Schema 3 adds `settings`, the project's own settings (#1085). Nothing here
 *  can fill it: what goes in is this PC's Configuration (decision 6), which
 *  only a window knows, so project_settings.js copies it in where a project
 *  opens in one, and the next save writes it. A file a batch catch-up raised
 *  (batch_catch_up.js) gets it at its next open the same way. */
function v2ToV3() {}

/** Every step there is, oldest first. `upgrade(data)` edits the parsed file
 *  in place. A step goes from one schema that was written to disk to the next
 *  one that was. */
export const MIGRATIONS = [
  { from: 1, to: 2, upgrade: v1ToV2 },
  { from: 2, to: 3, upgrade: v2ToV3 },
];

/** The schema this build writes. */
export const CURRENT_SCHEMA = MIGRATIONS[MIGRATIONS.length - 1].to;

/**
 * Runs the steps from `schema` up on `data`, in place. A file newer than this
 * build goes through none. Throws when no step leads on from a schema, which
 * means the list above is broken: the project then fails to open, rather than
 * opening half read.
 */
export function migrateProject(data, schema) {
  let at = schema;
  while (at < CURRENT_SCHEMA) {
    const step = MIGRATIONS.find((m) => m.from === at);
    if (!step) throw new Error(STRINGS.PROJECT_FILE.noMigrationStep(at));
    step.upgrade(data);
    at = step.to;
  }
}

/** How many highlights of `demos` are clips of the project. */
function countClips(demos, takeIndex) {
  const taken = new Set(Object.values(takeIndex || {}).flat());
  return (demos || []).reduce((sum, demo) =>
    sum + (demo.streaks || []).filter((s) => isClipInProject(demo.path, s, taken)).length, 0);
}

/**
 * The one-time summary's counts, for the first save that raises a project's
 * schema: `{ clips, takesFound, takesNotFound }`. A take with no recorded
 * folder counts as not found. Both take counts are null when the disk check
 * failed: the update has already happened, so that only leaves them out.
 * Reads the disk, off the main thread (`fetchStates`, swappable for tests).
 */
export async function updateSummary(demos, takeIndex, fetchStates = takeCaptureStates) {
  const clips = countClips(demos, takeIndex);
  const folders = knownTakeFolders(takeIndex).map((take) => take.folder);
  const takes = Object.keys(takeIndex || {}).length;
  const states = folders.length > 0 ? await fetchStates(folders) : [];
  if (!Array.isArray(states)) return { clips, takesFound: null, takesNotFound: null };
  const takesFound = states.filter((state) => state?.exists).length;
  return { clips, takesFound, takesNotFound: takes - takesFound };
}
