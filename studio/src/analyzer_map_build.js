// analyzer_map_build.js
// The Demo Analyzer's notice for a demo recorded on a different build of its
// map than the installed one (#1539). Such a demo plays with no warning and
// can draw wrong, and the Kill Map and the kill distances are measured on the
// installed map's geometry. POV demos have always carried the build; HLTV
// demos now say it through their server info, so both are checked.

import { checkDemoMaps, logFrontendEvent } from './ipc_bridge.js';
import { STRINGS } from './strings.js';

/** The map-check row that says the demo's map is a different build, or null.
 *  `rows` is what `checkDemoMaps` returned for one demo. */
export function mapBuildProblem(rows) {
  if (!Array.isArray(rows)) return null;
  return rows.find((row) => row?.state === 'wrongBuild') ?? null;
}

/** A build checksum as the 8 hex digits the map files are named by. */
function buildText(checksum) {
  return Number.isInteger(checksum) ? checksum.toString(16).padStart(8, '0') : '?';
}

/** The notice's text for a map-check row, or null when there is no problem. */
export function mapBuildNotice(row) {
  return row
    ? STRINGS.ANALYZER.wrongMapBuildNotice(row.mapName, buildText(row.expectedChecksum), buildText(row.installedChecksum))
    : null;
}

/** Checks `demoPath` against the maps beside the configured hl.exe.
 *  Resolves with the problem row, or null: no game path, a map that is fine
 *  or missing, and a failed check all read as "nothing to say". */
export async function findMapBuildProblem(demoPath, gamePath) {
  if (!gamePath) return null;
  const row = mapBuildProblem(await checkDemoMaps([demoPath], gamePath));
  if (row) {
    logFrontendEvent(`Demo Analyzer: ${row.demoName} was recorded on build ${buildText(row.expectedChecksum)} of ${row.mapName}, the installed one is ${buildText(row.installedChecksum)}`);
  }
  return row;
}

/** Fills `el` with the notice for `row`, or hides it. */
export function renderMapBuildBanner(el, row) {
  if (!el) return;
  const notice = mapBuildNotice(row);
  el.hidden = !notice;
  el.textContent = notice || '';
}
