// map_warnings.js
// Surfaces the one thing a demo cannot tell you by looking at it: whether the
// map it was recorded on is here, and whether it is the same build.
//
// Missing means the demo will not play. Wrong build means it will play, look
// approximately right, and every coordinate the tools take from that map will
// refer to a different world — which is the case worth a banner, because
// nothing else in the app would ever mention it.
//
// Grouped by map rather than by demo: twenty demos short of one map is one
// download, and a list of twenty rows saying the same thing is not a list.
//
// A demo recorded on an old build of its map plays right when Studio's map
// builds hold that build (#1547): it is no problem, and the note under the
// banner says which build it plays on. A problem row offers to add a build.

import { listen } from '@tauri-apps/api/event';
import { checkDemoMaps, downloadMap } from './ipc_bridge.js';
import { mapDownloadLine, showBytes, showSliding } from './download_progress.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';
import { escapeHtml } from './html.js';
import { createMapBuilds } from './map_builds.js';
import { addLibraryUse, addOutcome, buildKey, buildText, fileName } from './map_builds_logic.js';

/** Problem key -> { mapName, expectedChecksum, state, demos: [names], detail,
 *  copyBeside }. */
let problems = new Map();
/** Build key -> { mapName, checksum, demos }: demos playing on a library build. */
let libraryUses = new Map();
/** The banner's map-build actions, once wired. */
let mapBuilds = null;

/** One problem per map AND wanted build: two demos of a map recorded on
 *  different builds are different questions, and a demo that is fine must not
 *  clear the warning for one that is not (#1539). */
function problemKey(row) {
  return buildKey(row.mapName, row.expectedChecksum);
}

/** The problems on the banner, for tests. */
export function listMapProblems() {
  return [...problems.values()];
}

/** The demos playing on a library build, for tests. */
export function listLibraryUses() {
  return [...libraryUses.values()];
}
let dismissed = false;

function bannerEl() {
  return document.querySelector('#map-warning-banner');
}

/**
 * Check every demo path against the map library and redraw the banner.
 *
 * Never throws: a map check that fails leaves the queue exactly as it was.
 */
export async function refreshMapWarnings(demoPaths, gamePath) {
  if (!gamePath || !Array.isArray(demoPaths) || demoPaths.length === 0) {
    render();
    return;
  }

  const rows = await checkDemoMaps(demoPaths, gamePath);
  if (!Array.isArray(rows)) return;

  for (const row of rows) {
    if (row.mapName && row.state !== 'fromLibrary') libraryUses.delete(problemKey(row));
    // Plays right, on the build Studio puts in place (#1547).
    if (row.state === 'fromLibrary') {
      problems.delete(problemKey(row));
      addLibraryUse(libraryUses, row.mapName, row.expectedChecksum, row.demoName);
      continue;
    }
    // `unverifiable` is a demo that states no build (an HLTV demo states it in
    // its server info since #1539, so this is a cut-short or unknown one). The
    // map is present; there is simply nothing to compare. Not a problem to report.
    if (row.state === 'ok' || row.state === 'unverifiable') {
      // A map that has since been fixed should stop being listed.
      if (row.mapName) problems.delete(problemKey(row));
      continue;
    }
    if (row.state === 'unreadableDemo') continue;

    const key = problemKey(row);
    const existing = problems.get(key);
    if (existing) {
      if (!existing.demos.includes(row.demoName)) existing.demos.push(row.demoName);
      continue;
    }
    problems.set(key, {
      mapName: row.mapName,
      expectedChecksum: row.expectedChecksum ?? null,
      state: row.state,
      detail: row.detail,
      demos: [row.demoName],
      copyBeside: row.copyBeside ?? null,
    });
  }

  if (problems.size > 0) dismissed = false;
  render();
}

/** Forget everything — used when the queue is cleared. */
export function resetMapWarnings() {
  problems = new Map();
  libraryUses = new Map();
  dismissed = false;
  render();
}

/** The note under the banner: which demos play on a library build (#1547). */
function renderLibraryNote() {
  const note = document.querySelector('#map-builds-note');
  if (!note) return;
  note.hidden = libraryUses.size === 0;
  note.title = STRINGS.MAP_BUILDS.NOTE_HELP;
  note.innerHTML = [...libraryUses.values()]
    .map((use) => {
      const line = STRINGS.MAP_BUILDS.libraryNote(use.mapName, buildText(use.checksum), use.demos.length);
      return `<div class="map-builds-note-row">${escapeHtml(line)}</div>`;
    })
    .join('');
}

/** A problem row's map-build buttons: add a picked file, and use the copy
 *  found beside the map when there is one. */
function buildButtons(p) {
  const copy = p.copyBeside
    ? `<button class="map-use-copy-btn" data-key="${escapeHtml(p.key)}"
         title="${escapeHtml(STRINGS.MAP_BUILDS.useCopyTitle(p.copyBeside))}">
         ${escapeHtml(STRINGS.MAP_BUILDS.useCopyButton(fileName(p.copyBeside)))}
       </button>`
    : '';
  return `${copy}
    <button class="map-add-build-btn" data-key="${escapeHtml(p.key)}"
      title="${escapeHtml(STRINGS.MAP_BUILDS.ADD_BUTTON_TITLE)}">${STRINGS.MAP_BUILDS.ADD_BUTTON}</button>`;
}

function label(state) {
  if (state === 'missing') return STRINGS.MAPS.MISSING_LABEL;
  if (state === 'wrongBuild') return STRINGS.MAPS.WRONG_BUILD_LABEL;
  return STRINGS.MAPS.UNREADABLE_LABEL;
}

function render() {
  renderLibraryNote();
  const el = bannerEl();
  if (!el) return;

  if (dismissed || problems.size === 0) {
    el.hidden = true;
    el.innerHTML = '';
    return;
  }

  const entries = [...problems].map(([key, p]) => ({ ...p, key }));
  const demoCount = entries.reduce((n, p) => n + p.demos.length, 0);

  el.hidden = false;
  el.style.cssText =
    'margin: 8px 0; padding: 10px 12px; border: 1px solid var(--warn-note-border); ' +
    'border-radius: 4px; background: var(--warn-note-bg); color: var(--warn-note-text); font-size: 12px;';

  const rows = entries
    .map((p) => {
      const demos = STRINGS.MAPS.demoCount(p.demos.length);
      return `
        <div class="map-warning-row" data-map="${p.mapName}"
             style="display:flex; align-items:center; gap:8px; padding:3px 0;">
          <span style="flex:1">
            <strong>${p.mapName}</strong>
            <span style="opacity:.75"> — ${label(p.state)}, ${demos}</span>
          </span>
          ${buildButtons(p)}
          <button class="map-download-btn" data-key="${p.key}">
            ${STRINGS.MAPS.DOWNLOAD_BUTTON}
          </button>
        </div>`;
    })
    .join('');

  el.innerHTML = `
    <div style="display:flex; align-items:center; gap:8px; margin-bottom:6px;">
      <strong style="flex:1">${STRINGS.MAPS.BANNER_TITLE}</strong>
      <span style="opacity:.75">${STRINGS.MAPS.missingSummary(entries.length, demoCount)}</span>
      <button id="map-download-all-btn">${STRINGS.MAPS.DOWNLOAD_ALL_BUTTON}</button>
      <button id="map-dismiss-btn">${STRINGS.MAPS.DISMISS_BUTTON}</button>
    </div>
    ${rows}`;
}

/** A problem row's Add a build… or Use <copy> click (#1547): adds the file to
 *  Studio's map builds, the banner disabled meanwhile. When it is the build
 *  the row's demos want, they move from the problems to the note. */
async function addBuildFor(el, button) {
  const key = button.dataset.key;
  const problem = problems.get(key);
  if (!problem || !mapBuilds) return;
  let source = problem.copyBeside;
  if (button.classList.contains('map-add-build-btn')) {
    try {
      source = await mapBuilds.pickFile();
    } catch (err) {
      console.error('Error opening the map file dialog:', err);
      return;
    }
  }
  if (!source) return;

  el.querySelectorAll('button').forEach((b) => (b.disabled = true));
  button.textContent = STRINGS.MAP_BUILDS.ADDING;
  const added = await mapBuilds.add({
    mapName: problem.mapName,
    wanted: problem.expectedChecksum,
    source,
    statusEl: el,
  });
  if (added && addOutcome(added, problem.expectedChecksum) === 'matches') {
    problems.delete(key);
    for (const demo of problem.demos) {
      addLibraryUse(libraryUses, problem.mapName, problem.expectedChecksum, demo);
    }
  }
  render();
}

/**
 * Wire the banner's buttons once, at start-up.
 *
 * `getGamePath` is read at click time rather than captured, because the hl.exe
 * path can be set after a scan has already run.
 */
export function initMapWarnings(getGamePath, { mapBuildActions = null } = {}) {
  const el = bannerEl();
  if (!el) return;
  mapBuilds = mapBuildActions ?? createMapBuilds({ getGamePath });

  // The map being downloaded, for the banner's step line and bar (#957).
  let active = null;
  const showStep = (event) => {
    const line = el.querySelector('#map-download-line');
    const bar = el.querySelector('#map-download-bar');
    if (!active || !line) return;
    line.textContent = mapDownloadLine(active.map, active.index, active.count, event);
    if (event?.step === 'downloading' && event.total) showBytes(bar, event.downloaded, event.total);
    else showSliding(bar);
  };
  listen('map_download_progress', (event) => {
    if (active && event.payload?.map === active.map) showStep(event.payload);
  }).catch((err) => console.error('Failed to listen for map_download_progress:', err));

  el.addEventListener('click', async (event) => {
    const target = event.target;
    if (!(target instanceof HTMLElement)) return;

    if (target.id === 'map-dismiss-btn') {
      dismissed = true;
      render();
      return;
    }

    if (target.classList.contains('map-add-build-btn') || target.classList.contains('map-use-copy-btn')) {
      await addBuildFor(el, target);
      return;
    }

    const wanted =
      target.id === 'map-download-all-btn'
        ? [...problems.keys()]
        : target.classList.contains('map-download-btn')
          ? [target.dataset.key]
          : [];
    if (wanted.length === 0) return;

    const gamePath = getGamePath();
    if (!gamePath) {
      showToast(STRINGS.MAPS.NO_GAME_PATH, 'error');
      return;
    }

    // Disable the whole banner for the duration: two downloads of the same map
    // would race each other onto the same path.
    el.querySelectorAll('button').forEach((b) => (b.disabled = true));
    const previous = target.textContent;
    target.textContent = STRINGS.MAPS.DOWNLOADING;

    el.insertAdjacentHTML(
      'beforeend',
      `<div id="map-download-status" style="display:flex; align-items:center; gap:8px; margin-top:6px;">
        <span id="map-download-line" style="flex:1"></span>
        <progress id="map-download-bar" style="width:160px;"></progress>
      </div>`
    );

    for (const [i, key] of wanted.entries()) {
      const problem = problems.get(key);
      if (!problem) continue;
      const mapName = problem.mapName;
      active = { map: mapName, index: i + 1, count: wanted.length };
      showStep(null);
      try {
        const result = await downloadMap(mapName, problem.expectedChecksum, gamePath);
        problems.delete(key);
        showToast(
          result.alreadyCorrect
            ? STRINGS.MAPS.alreadyCorrectToast(mapName)
            : STRINGS.MAPS.installedToast(mapName),
          'success'
        );
        if (result.replacedPath) {
          showToast(STRINGS.MAPS.replacedNote(result.replacedPath), 'info');
        }
      } catch (err) {
        showToast(STRINGS.MAPS.downloadFailedToast(mapName, err), 'error');
      }
    }

    active = null;
    target.textContent = previous;
    render();
  });
}
