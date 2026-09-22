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

import { checkDemoMaps, downloadMap } from './ipc_bridge.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

/** Map name -> { mapName, expectedChecksum, state, demos: [names], detail }. */
let problems = new Map();
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
    // `unverifiable` is an HLTV demo, whose header records no build. The map is
    // present; there is simply nothing to compare. Not a problem to report.
    if (row.state === 'ok' || row.state === 'unverifiable') {
      // A map that has since been fixed should stop being listed.
      if (row.mapName) problems.delete(row.mapName);
      continue;
    }
    if (row.state === 'unreadableDemo') continue;

    const existing = problems.get(row.mapName);
    if (existing) {
      if (!existing.demos.includes(row.demoName)) existing.demos.push(row.demoName);
      continue;
    }
    problems.set(row.mapName, {
      mapName: row.mapName,
      expectedChecksum: row.expectedChecksum ?? null,
      state: row.state,
      detail: row.detail,
      demos: [row.demoName],
    });
  }

  if (problems.size > 0) dismissed = false;
  render();
}

/** Forget everything — used when the queue is cleared. */
export function resetMapWarnings() {
  problems = new Map();
  dismissed = false;
  render();
}

function label(state) {
  if (state === 'missing') return STRINGS.MAPS.MISSING_LABEL;
  if (state === 'wrongBuild') return STRINGS.MAPS.WRONG_BUILD_LABEL;
  return STRINGS.MAPS.UNREADABLE_LABEL;
}

function render() {
  const el = bannerEl();
  if (!el) return;

  if (dismissed || problems.size === 0) {
    el.hidden = true;
    el.innerHTML = '';
    return;
  }

  const entries = [...problems.values()];
  const demoCount = entries.reduce((n, p) => n + p.demos.length, 0);

  el.hidden = false;
  el.style.cssText =
    'margin: 8px 0; padding: 10px 12px; border: 1px solid #b58900; ' +
    'border-radius: 4px; background: #2a2410; color: #e8dCB0; font-size: 12px;';

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
          <button class="map-download-btn" data-map="${p.mapName}">
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

/**
 * Wire the banner's buttons once, at start-up.
 *
 * `getGamePath` is read at click time rather than captured, because the hl.exe
 * path can be set after a scan has already run.
 */
export function initMapWarnings(getGamePath) {
  const el = bannerEl();
  if (!el) return;

  el.addEventListener('click', async (event) => {
    const target = event.target;
    if (!(target instanceof HTMLElement)) return;

    if (target.id === 'map-dismiss-btn') {
      dismissed = true;
      render();
      return;
    }

    const wanted =
      target.id === 'map-download-all-btn'
        ? [...problems.keys()]
        : target.classList.contains('map-download-btn')
          ? [target.dataset.map]
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

    for (const mapName of wanted) {
      const problem = problems.get(mapName);
      if (!problem) continue;
      try {
        const result = await downloadMap(mapName, problem.expectedChecksum, gamePath);
        problems.delete(mapName);
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

    target.textContent = previous;
    render();
  });
}
