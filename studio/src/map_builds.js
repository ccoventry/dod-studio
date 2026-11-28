// map_builds.js
// Adding a build of a map to Studio's map builds from the Maps needed banner
// (#1547): a .bsp from the file picker, or a copy found beside the installed
// map. The backend copies it (the source is only read) and names it by its
// build; the banner shows the copy's progress while it runs.

import { listen } from '@tauri-apps/api/event';
import { addMapBuild, logFrontendEvent } from './ipc_bridge.js';
import { showBytes } from './download_progress.js';
import { pickPath } from './pick_path.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';
import { addOutcome, buildText } from './map_builds_logic.js';

/**
 * The banner's map-build actions.
 *
 * `getGamePath()` is read at click time (hl.exe may be set after a scan).
 * The other arguments stand in for the dialog, the backend and the toasts in
 * tests. Returns { pickFile(), add({ mapName, wanted, source, statusEl }) }.
 */
export function createMapBuilds({
  getGamePath,
  pick = pickPath,
  add = addMapBuild,
  onProgress = (handler) => listen('map_build_progress', (event) => handler(event.payload)),
  toast = showToast,
  log = logFrontendEvent,
} = {}) {
  /** The status row for the copy under way, inside the banner. */
  let active = null;
  Promise.resolve(onProgress((payload) => {
    if (!active || payload?.map !== active.mapName) return;
    showBytes(active.bar, payload.done, payload.total);
  })).catch((err) => console.error('Failed to listen for map_build_progress:', err));

  /** The .bsp the user picks, or null on Cancel. */
  async function pickFile() {
    const picked = await pick({
      field: 'maps.add_build',
      multiple: false,
      filters: [{ name: STRINGS.MAP_BUILDS.MAP_FILES_FILTER_NAME, extensions: ['bsp'] }],
      title: STRINGS.MAP_BUILDS.PICK_TITLE,
    });
    return Array.isArray(picked) ? picked[0] ?? null : picked || null;
  }

  /** Adds `source` as a build of `mapName` for demos that want `wanted`,
   *  showing the copy in `statusEl`. Resolves with what the backend added,
   *  or null when it failed or no game path is set (both toasted). */
  async function addBuild({ mapName, wanted, source, statusEl }) {
    const gamePath = getGamePath();
    if (!gamePath) {
      toast(STRINGS.MAPS.NO_GAME_PATH, 'error');
      return null;
    }
    log(`Map builds: adding ${source} as a build of ${mapName}`);
    active = { mapName, bar: showStatus(statusEl, STRINGS.MAP_BUILDS.addingLine(mapName)) };
    try {
      const added = await add(mapName, source, gamePath);
      const build = buildText(added.checksum);
      if (addOutcome(added, wanted) === 'matches') {
        toast(STRINGS.MAP_BUILDS.addedToast(mapName, build), 'success');
      } else {
        toast(STRINGS.MAP_BUILDS.otherBuildToast(mapName, build, buildText(wanted)), 'info');
      }
      return added;
    } catch (err) {
      toast(STRINGS.MAP_BUILDS.addFailedToast(mapName, err), 'error');
      return null;
    } finally {
      active = null;
    }
  }

  return { pickFile, add: addBuild };
}

/** How long a map build warning stays: it names demos, and comes as a batch
 *  or a preview starts, while the user looks at the game. */
const WARNING_MS = 12000;
let warningsStarted = false;

/**
 * Shows every `map_build_warning` the backend sends (a batch or preview whose
 * demos won't play on their map version, a build that couldn't be put in
 * place) as a warning toast. Once per page; later calls do nothing.
 */
export function startMapBuildWarnings({ listenTo = listen, toast = showToast } = {}) {
  if (warningsStarted) return;
  warningsStarted = true;
  Promise.resolve(
    listenTo('map_build_warning', (event) => {
      if (typeof event?.payload === 'string' && event.payload) toast(event.payload, 'warning', WARNING_MS);
    })
  ).catch((err) => console.error('Failed to listen for map_build_warning:', err));
}

/** A step line and an empty bar in `el` (the banner), replacing an earlier
 *  one. Returns the bar, or null without an element. */
function showStatus(el, line) {
  if (!el) return null;
  el.querySelector('#map-build-status')?.remove();
  el.insertAdjacentHTML(
    'beforeend',
    `<div id="map-build-status" style="display:flex; align-items:center; gap:8px; margin-top:6px;">
      <span style="flex:1"></span>
      <progress id="map-build-bar" style="width:160px;"></progress>
    </div>`
  );
  el.querySelector('#map-build-status span').textContent = line;
  return el.querySelector('#map-build-bar');
}
