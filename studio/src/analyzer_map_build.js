// analyzer_map_build.js
// The Demo Analyzer's notice for a demo recorded on a different build of its
// map than the installed one (#1539). Such a demo plays with no warning and
// can draw wrong, and the Kill Map and the kill distances are measured on the
// installed map's geometry. POV demos have always carried the build; HLTV
// demos now say it through their server info, so both are checked. When
// Studio's map builds hold the demo's build, the notice says the demo plays
// on it instead (#1547).

import { checkDemoMaps, logFrontendEvent } from './ipc_bridge.js';
import { STRINGS } from './strings.js';
import { buildText } from './map_builds_logic.js';

/** The map-check row that says the demo's map is a different build, or null.
 *  `rows` is what `checkDemoMaps` returned for one demo. */
export function mapBuildProblem(rows) {
  if (!Array.isArray(rows)) return null;
  return rows.find((row) => row?.state === 'wrongBuild') ?? null;
}

/** The row worth a notice: a different build, or one Studio's map builds
 *  hold (#1547). Null otherwise. */
export function mapBuildRow(rows) {
  if (!Array.isArray(rows)) return null;
  return mapBuildProblem(rows) ?? rows.find((row) => row?.state === 'fromLibrary') ?? null;
}

/** The notice's text for a map-check row, or null when there is none. */
export function mapBuildNotice(row) {
  if (!row) return null;
  if (row.state === 'fromLibrary') {
    return STRINGS.ANALYZER.fromLibraryMapBuildNotice(row.mapName, buildText(row.expectedChecksum));
  }
  return STRINGS.ANALYZER.wrongMapBuildNotice(row.mapName, row.olderRelease === true);
}

/** The two builds, for the banner's tooltip: the detail the notice leaves out.
 *  Empty for a demo played from the map builds, whose notice names its build. */
export function mapBuildDetail(row) {
  return row && row.state !== 'fromLibrary'
    ? STRINGS.ANALYZER.wrongMapBuildDetail(buildText(row.expectedChecksum), buildText(row.installedChecksum))
    : '';
}

/** Checks `demoPath` against the maps beside the configured hl.exe.
 *  Resolves with the row worth a notice (`mapBuildRow`), or null: no game
 *  path, a map that is fine or missing, and a failed check all read as
 *  "nothing to say". */
export async function findMapBuildProblem(demoPath, gamePath) {
  if (!gamePath) return null;
  const row = mapBuildRow(await checkDemoMaps([demoPath], gamePath));
  if (row?.state === 'fromLibrary') {
    logFrontendEvent(`Demo Analyzer: ${row.demoName} plays on build ${buildText(row.expectedChecksum)} of ${row.mapName} from Studio's map builds`);
  } else if (row) {
    logFrontendEvent(`Demo Analyzer: ${row.demoName} was recorded on build ${buildText(row.expectedChecksum)} of ${row.mapName}, the installed one is ${buildText(row.installedChecksum)}`);
  }
  return row;
}

/** Fills `el` with the notice for `row`, or hides it: a warning for a
 *  different build, plain news for one played from the map builds. */
export function renderMapBuildBanner(el, row) {
  if (!el) return;
  const notice = mapBuildNotice(row);
  el.hidden = !notice;
  el.textContent = notice || '';
  el.title = mapBuildDetail(row);
  const news = row?.state === 'fromLibrary';
  el.classList?.toggle('analyzer-info-banner', news);
  el.classList?.toggle('analyzer-warning-banner', !news);
}
