// analyzer_map_build.js
// The Demo Analyzer's notice for a demo recorded on a different build of its
// map than the installed one (#1539). Such a demo plays with no warning and
// can draw wrong, and the Kill Map and the kill distances are measured on the
// installed map's geometry. POV demos have always carried the build; HLTV
// demos now say it through their server info, so both are checked.

import { checkDemoMaps } from './ipc_bridge.js';
import { STRINGS } from './strings.js';

/** The map-check row that says the demo's map is a different build, or null.
 *  `rows` is what `checkDemoMaps` returned for one demo. */
export function mapBuildProblem(rows) {
  if (!Array.isArray(rows)) return null;
  return rows.find((row) => row?.state === 'wrongBuild') ?? null;
}

/** The notice's text for a map-check row, or null when there is no problem. */
export function mapBuildNotice(row) {
  return row ? STRINGS.MAPS.analyzerWrongBuildNotice(row.detail) : null;
}

/** Checks `demoPath` against the maps beside the configured hl.exe.
 *  Resolves with the problem row, or null: no game path, a map that is fine
 *  or missing, and a failed check all read as "nothing to say". */
export async function findMapBuildProblem(demoPath, gamePath) {
  if (!gamePath) return null;
  return mapBuildProblem(await checkDemoMaps([demoPath], gamePath));
}

/** Fills `el` with the notice for `row`, or hides it. */
export function renderMapBuildBanner(el, row) {
  if (!el) return;
  const notice = mapBuildNotice(row);
  el.hidden = !notice;
  el.textContent = notice || '';
}
