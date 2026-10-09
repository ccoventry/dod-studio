// blender_pane.js — the Blender page (#403): rebuild a highlight recorded with
// HLAE's `mirv_agr` in Blender and render it. Each button runs one of
// the repo's blender/ scripts through Blender (native::blender), so the page and
// the command line do the same thing. Every step writes into one work folder
// beside the .agr, so each finds the last one's output.

import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import { blenderStatus, blenderSetExe, blenderRun, blenderCancel, revealInExplorer } from './ipc_bridge.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

// Per-viewer conveniences only: the last take and assets folder picked.
const STORE_AGR = 'dodstudio.blender.agr';
const STORE_ASSETS = 'dodstudio.blender.assets';
const STORE_MAP = 'dodstudio.blender.map';

function load(key) {
  try { return localStorage.getItem(key) || ''; } catch { return ''; }
}

function save(key, value) {
  try { localStorage.setItem(key, value); } catch { /* storage unavailable */ }
}

function formatElapsed(secs) {
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  return m ? `${m}m ${String(s).padStart(2, '0')}s` : `${s}s`;
}

// `<agr folder>\<agr name>_blender`, as native::blender::WorkFiles names it.
function workDirFor(agr) {
  if (!agr) return '';
  const cut = Math.max(agr.lastIndexOf('\\'), agr.lastIndexOf('/'));
  const folder = cut >= 0 ? agr.slice(0, cut) : '.';
  const name = (cut >= 0 ? agr.slice(cut + 1) : agr).replace(/\.agr$/i, '');
  return `${folder}\\${name}_blender`;
}

export function initBlenderPane() {
  const pane = document.querySelector('#pane-blender');
  if (!pane) return;
  const $ = (id) => pane.querySelector(`#${id}`);
  const statusText = $('blender-status-text');
  const addonsText = $('blender-addons-text');
  const refreshBtn = $('blender-refresh-btn');
  const pickExeBtn = $('blender-pick-exe-btn');
  const resetExeBtn = $('blender-reset-exe-btn');
  const agrText = $('blender-agr-text');
  const pickAgrBtn = $('blender-pick-agr-btn');
  const assetsText = $('blender-assets-text');
  const pickAssetsBtn = $('blender-pick-assets-btn');
  const mapSelect = $('blender-map-select');
  const styleSelect = $('blender-style-select');
  const workText = $('blender-work-text');
  const openWorkBtn = $('blender-open-work-btn');
  const importBtn = $('blender-import-btn');
  const engineSelect = $('blender-engine-select');
  const framesInput = $('blender-frames-input');
  const sceneBtn = $('blender-scene-btn');
  const qualitySelect = $('blender-quality-select');
  const renderBtn = $('blender-render-btn');
  const encodeBtn = $('blender-encode-btn');
  const cancelBtn = $('blender-cancel-btn');
  const progressText = $('blender-progress');
  const lineText = $('blender-line');
  const resultsList = $('blender-results');

  let agr = load(STORE_AGR);
  let assets = load(STORE_ASSETS);
  let busy = false;
  let haveBlender = false;
  let started = 0;

  function gamePath() {
    return document.querySelector('#hl-path-input')?.value?.trim() || '';
  }

  function render() {
    agrText.textContent = agr || STRINGS.BLENDER.NONE_PICKED;
    assetsText.textContent = assets || STRINGS.BLENDER.NONE_PICKED;
    workText.textContent = agr ? STRINGS.BLENDER.workFolder(workDirFor(agr)) : '';
    openWorkBtn.disabled = !agr;
    const ready = haveBlender && agr && !busy;
    importBtn.disabled = !ready || !assets;
    sceneBtn.disabled = !ready || !assets || !mapSelect.value;
    renderBtn.disabled = !ready;
    encodeBtn.disabled = !ready;
    cancelBtn.disabled = !busy;
    for (const b of [pickExeBtn, resetExeBtn, pickAgrBtn, pickAssetsBtn]) b.disabled = busy;
  }

  async function refresh() {
    const game = gamePath();
    if (!game) {
      statusText.textContent = STRINGS.BLENDER.NEEDS_GAME_PATH;
      haveBlender = false;
      render();
      return;
    }
    let status;
    try {
      status = await blenderStatus(game);
    } catch {
      return; // already toasted
    }
    const b = status.blender;
    haveBlender = !!b.exe && !!b.scripts;
    if (!b.scripts) {
      statusText.textContent = STRINGS.BLENDER.NO_SCRIPTS;
    } else if (!b.exe) {
      statusText.textContent = STRINGS.BLENDER.NOT_FOUND;
    } else {
      statusText.textContent = STRINGS.BLENDER.using(b.exe, b.version, b.chosen, b.supported);
    }
    const missing = (b.addons || []).filter(([, present]) => !present).map(([name]) => name);
    addonsText.textContent = b.version
      ? (missing.length ? STRINGS.BLENDER.addonsMissing(missing) : STRINGS.BLENDER.ADDONS_OK)
      : '';

    const wantedMap = mapSelect.value || load(STORE_MAP);
    mapSelect.replaceChildren(...[new Option(STRINGS.BLENDER.PICK_MAP, '')]
      .concat(status.maps.map((m) => new Option(m, m))));
    if (status.maps.includes(wantedMap)) mapSelect.value = wantedMap;

    const wantedStyle = styleSelect.value;
    styleSelect.replaceChildren(
      new Option(STRINGS.BLENDER.STYLE_ORIGINAL, 'none'),
      ...status.styles.map((s) => new Option(s, s)),
    );
    styleSelect.value = status.styles.includes(wantedStyle) || wantedStyle === 'none'
      ? wantedStyle
      : (status.styles[0] || 'none');
    render();
  }

  function frames() {
    return framesInput.value.split(/[\s,]+/).filter(Boolean).map(Number).filter((n) => Number.isInteger(n) && n >= 0);
  }

  function showResults(outcome) {
    const items = [...outcome.saved, ...outcome.images];
    resultsList.replaceChildren(...items.map((path) => {
      const li = document.createElement('li');
      li.textContent = path;
      return li;
    }));
  }

  async function runStep(step, label) {
    if (busy) return;
    busy = true;
    started = Date.now();
    progressText.textContent = STRINGS.BLENDER.running(label);
    lineText.textContent = '';
    render();
    try {
      const outcome = await blenderRun(gamePath(), {
        agr,
        assets,
        map: mapSelect.value,
        style: styleSelect.value,
        step,
      });
      progressText.textContent = STRINGS.BLENDER.done(label, formatElapsed(outcome.elapsed_secs));
      showResults(outcome);
      showToast(STRINGS.BLENDER.done(label, formatElapsed(outcome.elapsed_secs)), 'success');
    } catch (err) {
      progressText.textContent = err === 'cancelled' ? STRINGS.BLENDER.CANCELLED : STRINGS.BLENDER.failed(label);
    } finally {
      busy = false;
      render();
    }
  }

  listen('blender_progress', (event) => {
    const p = event.payload;
    if (!busy) return;
    const elapsed = formatElapsed(p.elapsed_secs ?? Math.round((Date.now() - started) / 1000));
    if (p.total) {
      progressText.textContent = STRINGS.BLENDER.frames(p.done, p.total, p.frame_secs, elapsed);
    }
    if (p.line) lineText.textContent = p.line;
  });

  refreshBtn.addEventListener('click', refresh);

  pickExeBtn.addEventListener('click', async () => {
    let picked;
    try {
      picked = await open({
        title: STRINGS.BLENDER.PICK_EXE_TITLE,
        multiple: false,
        directory: false,
        filters: [{ name: 'blender.exe', extensions: ['exe'] }],
      });
    } catch (err) {
      console.error('Blender picker failed:', err);
      return;
    }
    if (!picked) return;
    try {
      await blenderSetExe(picked);
    } catch {
      return; // already toasted
    }
    refresh();
  });

  resetExeBtn.addEventListener('click', async () => {
    try {
      await blenderSetExe(null);
    } catch {
      return;
    }
    refresh();
  });

  pickAgrBtn.addEventListener('click', async () => {
    let picked;
    try {
      picked = await open({
        title: STRINGS.BLENDER.PICK_AGR_TITLE,
        multiple: false,
        directory: false,
        filters: [{ name: 'AfxGameRecord', extensions: ['agr'] }],
      });
    } catch (err) {
      console.error('AGR picker failed:', err);
      return;
    }
    if (!picked) return;
    agr = picked;
    save(STORE_AGR, agr);
    resultsList.replaceChildren();
    render();
  });

  pickAssetsBtn.addEventListener('click', async () => {
    let picked;
    try {
      picked = await open({ title: STRINGS.BLENDER.PICK_ASSETS_TITLE, directory: true, multiple: false });
    } catch (err) {
      console.error('Assets picker failed:', err);
      return;
    }
    if (!picked) return;
    assets = picked;
    save(STORE_ASSETS, assets);
    render();
  });

  mapSelect.addEventListener('change', () => {
    save(STORE_MAP, mapSelect.value);
    render();
  });

  openWorkBtn.addEventListener('click', () => {
    if (agr) revealInExplorer(workDirFor(agr)).catch(() => {});
  });

  importBtn.addEventListener('click', () => runStep({ kind: 'import' }, STRINGS.BLENDER.IMPORT_LABEL));

  sceneBtn.addEventListener('click', () => runStep(
    { kind: 'scene', engine: engineSelect.value, frames: frames() },
    STRINGS.BLENDER.SCENE_LABEL,
  ));

  renderBtn.addEventListener('click', () => runStep(
    { kind: 'render', quick: qualitySelect.value === 'quick' },
    STRINGS.BLENDER.RENDER_LABEL,
  ));

  encodeBtn.addEventListener('click', () => runStep(
    { kind: 'encode', quick: qualitySelect.value === 'quick' },
    STRINGS.BLENDER.ENCODE_LABEL,
  ));

  cancelBtn.addEventListener('click', () => {
    cancelBtn.disabled = true;
    blenderCancel().catch(() => {});
  });

  // First look when the page is opened, not at app start: finding Blender
  // runs it once to read its version.
  document.querySelector('.nav-tab-btn[data-nav="blender"]')
    ?.addEventListener('click', () => { if (!busy) refresh(); });

  render();
}
