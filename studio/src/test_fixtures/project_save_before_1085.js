// What a build from before #1085 does to a project file: project_session.js's
// load and save at origin/dev e429382d, cut down to the lines that touch the
// file's contents, on verbatim copies of its take modules. Do not edit.

import { pruneTakeIndex, streakUid } from './take_index_before_1085.js';
import { attachCaptures, capturesToSave } from './take_record_before_1085.js';
import { attachRenders, rendersToSave } from './take_renders_before_1085.js';

/** Loads `text` as that build does; what it keeps in memory. */
export function loadBefore1085(text) {
  const data = JSON.parse(text);
  const takeIndex = data.takeIndex || {};
  attachCaptures(takeIndex, data.takeCaptures);
  attachRenders(takeIndex, data.takeRenders);
  return { demos: data.demos, takeIndex, teams: data.teams, review: data.review, scanPaths: data.scanPaths };
}

/** The text that build saves for `project` (from loadBefore1085). */
export function saveBefore1085({ demos, takeIndex, teams, review, scanPaths }) {
  const uids = demos.flatMap((demo) => (demo.streaks || []).map((streak) => streakUid(demo.path, streak)));
  const prunedTakes = pruneTakeIndex(takeIndex, uids);
  return JSON.stringify({
    version: '0.12.0',
    scanPaths,
    demos,
    takeIndex: prunedTakes,
    takeCaptures: capturesToSave(prunedTakes),
    takeRenders: rendersToSave(prunedTakes),
    teams,
    review,
    mode: 'workspace',
  }, null, 2);
}
