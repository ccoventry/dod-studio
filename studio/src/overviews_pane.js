// overviews_pane.js — the Overviews page (#371): pick an install and a map,
// see the overview made from the map, recolour areas, hide what shouldn't
// show, add labels, and save it where the game reads it.
//
// The scene (what to draw) comes from native::overview; the edits are the
// page's own, saved per map as they change. Drawing is overview_draw.js's.

import { themedConfirm } from './themed_confirm.js';
import { showToast } from './toast.js';
import { listen } from '@tauri-apps/api/event';
import { STRINGS } from './strings.js';
import {
  overviewInstalls, overviewMaps, overviewScene, overviewLoadEdits,
  overviewSaveEdits, overviewResetEdits, overviewExport, overviewExportHd, overviewFlagIcons, overviewScreenHeight,
} from './ipc_bridge.js';
import {
  emptyEdits, normaliseEdits, drawOverview, faceAt, labelAt, setAreaEdit,
  areaEdit, setFaceColour, flagName, setFlagName, flagOffset, setFlagOffset, flagLabelAt, toWorld, renderExport, renderHd, toBase64,
} from './overview_draw.js';
import { THEMES } from './overview_themes.js';
import { fitEdits } from './overview_fit.js';

const INSTALL_KEY = 'overviews.install';
// Each map keeps the theme it was made in; the one last picked on any map
// is what a map not yet given one opens in.
const THEME_KEY = 'overviews.theme';
// The screen height the flag icon preview is sized for.
const FLAG_SCREEN_KEY = 'overviews.flagScreen';
const SCREEN_HEIGHTS = [480, 600, 720, 768, 900, 1024, 1080, 1200, 1440, 2160];
const UNDO_LIMIT = 100;

function storageGet(key) {
  try { return localStorage.getItem(key); } catch { return null; }
}
function storageSet(key, value) {
  try { localStorage.setItem(key, value); } catch { /* per-viewer convenience only */ }
}

const hex = (c) => `#${c.map((v) => v.toString(16).padStart(2, '0')).join('')}`;
const rgb = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));

export function initOverviewsPane() {
  const pane = document.querySelector('#pane-overviews');
  if (!pane) return;
  const installSelect = pane.querySelector('#ov-install-select');
  const filterInput = pane.querySelector('#ov-map-filter');
  const mapList = pane.querySelector('#ov-map-list');
  const title = pane.querySelector('#ov-map-title');
  const canvas = pane.querySelector('#ov-canvas');
  const wrap = pane.querySelector('#ov-canvas-wrap');
  const empty = pane.querySelector('#ov-empty');
  const building = pane.querySelector('#ov-building');
  const buildingText = pane.querySelector('#ov-building-text');
  const buildingFill = pane.querySelector('#ov-building-fill');
  const palette = pane.querySelector('#ov-palette');
  const customColour = pane.querySelector('#ov-custom-colour');
  const clearColourBtn = pane.querySelector('#ov-clear-colour-btn');
  const showBox = pane.querySelector('#ov-show');
  const flagList = pane.querySelector('#ov-flag-names');
  const labelList = pane.querySelector('#ov-labels');
  const formatSelect = pane.querySelector('#ov-format');
  const themeSelect = pane.querySelector('#ov-theme');
  const flagScreenSelect = pane.querySelector('#ov-flag-screen');
  const hdBox = pane.querySelector('#ov-hd');
  const saveBtn = pane.querySelector('#ov-save-btn');
  const resetBtn = pane.querySelector('#ov-reset-btn');
  const saveStatus = pane.querySelector('#ov-save-status');
  const undoBtn = pane.querySelector('#ov-undo-btn');
  const zoomIn = pane.querySelector('#ov-zoom-in');
  const zoomOut = pane.querySelector('#ov-zoom-out');
  const zoomFit = pane.querySelector('#ov-zoom-fit');
  const footer = document.querySelector('#footer-overviews-summary');

  let installs = [];
  let install = '';
  let maps = [];
  let scene = null;
  let defaultTheme = THEMES.some((t) => t.id === storageGet(THEME_KEY)) ? storageGet(THEME_KEY) : THEMES[0].id;
  let edits = { ...emptyEdits(), theme: defaultTheme };
  let history = [];
  let mode = 'area';
  // The colour clicks paint with; null puts an area's own colour back.
  let colour = [146, 155, 247];
  let selectedLabel = null;
  let drag = null;
  let saveTimer = null;
  let loaded = false;
  let loadToken = 0;
  // The game's flag icons for the map shown, and the screen height they are
  // previewed at (the game's own, from its settings, unless picked here).
  let flagIcons = [];
  let gameScreen = null;
  let flagScreen = Number(storageGet(FLAG_SCREEN_KEY)) || 720;
  // The map being built: highlighted in the list straight away.
  let opening = null;
  const cache = {};

  function gamePath() {
    return document.querySelector('#hl-path-input')?.value?.trim() || '';
  }

  // ── Edits ──────────────────────────────────────────────────────────────
  function change(next, { remember = true } = {}) {
    if (remember) {
      history.push(edits);
      if (history.length > UNDO_LIMIT) history.shift();
    }
    edits = next;
    undoBtn.disabled = history.length === 0;
    persistSoon();
    draw();
    renderSidePanels();
  }

  function undo() {
    if (!history.length) return;
    edits = history.pop();
    undoBtn.disabled = history.length === 0;
    persistSoon();
    draw();
    renderSidePanels();
  }

  function persistSoon() {
    if (!scene) return;
    const map = scene.map;
    const snapshot = edits;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      overviewSaveEdits(map, snapshot).catch(() => {});
      const entry = maps.find((m) => m.name === map);
      if (entry && !entry.has_edits) {
        entry.has_edits = true;
        renderMapList();
      }
    }, 400);
  }

  // ── Drawing ────────────────────────────────────────────────────────────
  // ── Zoom ──────────────────────────────────────────────────────────────
  // The canvas keeps the size the whole map fits; zoomed in, it shows a
  // window of the map, drawn sharp at that size (drawOverview's `view`).
  // `centre` is the image pixel in the middle of the window.
  const MAX_ZOOM = 16;
  let zoom = 1;
  let centre = [512, 384];
  function clampView() {
    if (!scene) return;
    zoom = Math.max(1, Math.min(MAX_ZOOM, zoom));
    const halfW = scene.width / zoom / 2;
    const halfH = scene.height / zoom / 2;
    centre = [
      Math.max(halfW, Math.min(scene.width - halfW, centre[0])),
      Math.max(halfH, Math.min(scene.height - halfH, centre[1])),
    ];
  }
  function viewOrigin() {
    return [centre[0] - scene.width / zoom / 2, centre[1] - scene.height / zoom / 2];
  }
  /** Zooms by `factor`, keeping image pixel `at` under the same point. */
  function zoomBy(factor, at = centre) {
    const [ox, oy] = viewOrigin();
    const fx = (at[0] - ox) / (scene.width / zoom);
    const fy = (at[1] - oy) / (scene.height / zoom);
    zoom = Math.max(1, Math.min(MAX_ZOOM, zoom * factor));
    centre = [at[0] - (fx - 0.5) * (scene.width / zoom), at[1] - (fy - 0.5) * (scene.height / zoom)];
    clampView();
    drawSoon();
  }
  let drawQueued = false;
  function drawSoon() {
    if (drawQueued) return;
    drawQueued = true;
    requestAnimationFrame(() => {
      drawQueued = false;
      draw();
    });
  }
  function resetZoom() {
    zoom = 1;
    centre = scene ? [scene.width / 2, scene.height / 2] : centre;
  }

  function layout() {
    if (!scene) return 1;
    const box = wrap.getBoundingClientRect();
    const scale = Math.max(0.2, Math.min((box.width - 8) / scene.width, (box.height - 8) / scene.height));
    const dpr = window.devicePixelRatio || 1;
    canvas.style.width = `${Math.floor(scene.width * scale)}px`;
    canvas.style.height = `${Math.floor(scene.height * scale)}px`;
    canvas.width = Math.floor(scene.width * scale * dpr);
    canvas.height = Math.floor(scene.height * scale * dpr);
    return scale * dpr;
  }

  function draw() {
    building.hidden = !opening;
    if (!scene) {
      canvas.style.display = 'none';
      empty.style.display = opening ? 'none' : '';
      return;
    }
    canvas.style.display = '';
    empty.style.display = 'none';
    const s = layout();
    clampView();
    const [ox, oy] = viewOrigin();
    drawOverview(canvas.getContext('2d'), scene, edits, s * zoom, {
      cache,
      selectedLabel,
      flagIcons: { icons: flagIcons, screenHeight: flagScreen },
      view: { ox, oy, cw: canvas.width, ch: canvas.height },
    });
    zoomFit.textContent = `${Math.round(zoom * 100)}%`;
    zoomOut.disabled = zoom <= 1;
    zoomIn.disabled = zoom >= MAX_ZOOM;
  }

  function pixelOf(event) {
    const r = canvas.getBoundingClientRect();
    const [ox, oy] = viewOrigin();
    return [
      ox + ((event.clientX - r.left) / r.width) * (scene.width / zoom),
      oy + ((event.clientY - r.top) / r.height) * (scene.height / zoom),
    ];
  }

  // Scroll to zoom around the pointer; drag with the middle button, or
  // with Space held, to move about.
  canvas.addEventListener('wheel', (event) => {
    if (!scene) return;
    event.preventDefault();
    zoomBy(Math.exp(-event.deltaY * 0.0015), pixelOf(event));
  }, { passive: false });
  let pan = null;
  let spaceDown = false;
  document.addEventListener('keydown', (event) => {
    if (event.code !== 'Space' || pane.style.display === 'none' || event.target.closest('input, select, textarea, button')) return;
    event.preventDefault();
    spaceDown = true;
    canvas.style.cursor = 'grab';
  });
  document.addEventListener('keyup', (event) => {
    if (event.code !== 'Space') return;
    spaceDown = false;
    canvas.style.cursor = '';
  });
  canvas.addEventListener('mousedown', (event) => {
    if (!scene || !(event.button === 1 || (event.button === 0 && spaceDown))) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    pan = { x: event.clientX, y: event.clientY, centre };
    canvas.style.cursor = 'grabbing';
  });
  window.addEventListener('mousemove', (event) => {
    if (!pan) return;
    const r = canvas.getBoundingClientRect();
    centre = [
      pan.centre[0] - ((event.clientX - pan.x) / r.width) * (scene.width / zoom),
      pan.centre[1] - ((event.clientY - pan.y) / r.height) * (scene.height / zoom),
    ];
    drawSoon();
  });
  window.addEventListener('mouseup', () => {
    if (!pan) return;
    pan = null;
    canvas.style.cursor = spaceDown ? 'grab' : '';
  });
  zoomIn.addEventListener('click', () => scene && zoomBy(1.5));
  zoomOut.addEventListener('click', () => scene && zoomBy(1 / 1.5));
  zoomFit.addEventListener('click', () => {
    if (!scene) return;
    resetZoom();
    draw();
  });

  // ── Canvas clicks ──────────────────────────────────────────────────────
  canvas.addEventListener('mousedown', (event) => {
    if (!scene || event.button !== 0) return;
    const [x, y] = pixelOf(event);
    const label = labelAt(scene, edits, x, y);
    const flag = label ? null : flagLabelAt(scene, edits, x, y);
    if (flag) {
      drag = { flag, moved: false, before: edits };
      return;
    }
    if (label) {
      selectedLabel = label.id;
      drag = { id: label.id, moved: false, before: edits };
      draw();
      renderSidePanels();
      return;
    }
    if (mode === 'label') {
      const id = `l${Date.now().toString(36)}`;
      const world = toWorld(scene.transform, x, y);
      selectedLabel = id;
      change({ ...edits, labels: [...edits.labels, { id, text: STRINGS.OVERVIEWS.NEW_LABEL_TEXT, world, size: 15 }] });
      labelList.querySelector(`input[data-label="${id}"]`)?.select();
      return;
    }
    if (mode === 'hide') {
      const face = faceAt(scene, edits, x, y, { includeHidden: true });
      if (!face) return;
      const area = scene.areas[face.area];
      const hidden = !!areaEdit(edits, area)?.hidden;
      const old = areaEdit(edits, area) || {};
      const next = { ...old, hidden: !hidden };
      delete next.at;
      change(setAreaEdit(edits, area, next.hidden || next.colour ? next : null));
      return;
    }
    const face = faceAt(scene, edits, x, y);
    if (!face) return;
    if (mode === 'face') {
      change(setFaceColour(edits, face, colour));
    } else {
      const area = scene.areas[face.area];
      const old = areaEdit(edits, area) || {};
      const next = { hidden: !!old.hidden, colour };
      change(setAreaEdit(edits, area, next.hidden || next.colour ? next : null));
    }
  });

  window.addEventListener('mousemove', (event) => {
    if (!drag || !scene) return;
    const [x, y] = pixelOf(event);
    const world = toWorld(scene.transform, x, y);
    if (drag.flag) {
      edits = setFlagOffset(edits, drag.flag, [world[0] - drag.flag.world[0], world[1] - drag.flag.world[1]]);
    } else {
      edits = { ...edits, labels: edits.labels.map((l) => (l.id === drag.id ? { ...l, world } : l)) };
    }
    drag.moved = true;
    draw();
  });

  window.addEventListener('mouseup', () => {
    if (!drag) return;
    if (drag.moved) {
      history.push(drag.before);
      undoBtn.disabled = false;
      persistSoon();
      if (drag.flag) renderSidePanels();
    }
    drag = null;
  });

  document.addEventListener('keydown', (event) => {
    if (pane.style.display === 'none' || !scene) return;
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'z' && !event.target.closest('input, select, textarea')) {
      event.preventDefault();
      undo();
    }
    if ((event.key === 'Delete') && selectedLabel && !event.target.closest('input, select, textarea')) {
      change({ ...edits, labels: edits.labels.filter((l) => l.id !== selectedLabel) });
      selectedLabel = null;
    }
  });

  pane.querySelectorAll('.ov-mode').forEach((btn) => {
    btn.addEventListener('click', () => {
      mode = btn.dataset.mode;
      pane.querySelectorAll('.ov-mode').forEach((b) => b.classList.toggle('active', b === btn));
      canvas.dataset.mode = mode;
    });
  });
  undoBtn.addEventListener('click', undo);
  undoBtn.disabled = true;

  // ── Colours ────────────────────────────────────────────────────────────
  function renderPalette() {
    palette.innerHTML = '';
    const colours = scene?.palette || [];
    for (const c of colours) {
      const swatch = document.createElement('button');
      swatch.className = 'ov-swatch';
      swatch.style.background = hex(c);
      swatch.title = hex(c);
      swatch.classList.toggle('active', !!colour && hex(c) === hex(colour));
      swatch.addEventListener('click', () => {
        colour = c;
        customColour.value = hex(c);
        renderPalette();
      });
      palette.appendChild(swatch);
    }
    clearColourBtn.classList.toggle('active', colour === null);
  }
  customColour.addEventListener('input', () => {
    colour = rgb(customColour.value);
    renderPalette();
  });
  clearColourBtn.addEventListener('click', () => {
    colour = null;
    renderPalette();
  });

  // ── Side panels ────────────────────────────────────────────────────────
  showBox.querySelectorAll('input[data-show]').forEach((box) => {
    box.addEventListener('change', () => {
      change({ ...edits, show: { ...edits.show, [box.dataset.show]: box.checked } });
    });
  });

  function renderSidePanels() {
    showBox.querySelectorAll('input[data-show]').forEach((box) => {
      box.checked = !!edits.show[box.dataset.show];
      box.disabled = !scene;
    });
    formatSelect.value = edits.format || 'tga';
    themeSelect.value = edits.theme || defaultTheme;
    themeSelect.disabled = !scene;
    hdBox.checked = edits.hd !== false;

    flagList.innerHTML = '';
    if (scene && !scene.flags.length) {
      const p = document.createElement('p');
      p.className = 'hd-hint';
      p.textContent = STRINGS.OVERVIEWS.NO_FLAGS;
      flagList.appendChild(p);
    }
    for (const flag of scene?.flags || []) {
      const row = document.createElement('div');
      row.className = 'ov-flag-row';
      const input = document.createElement('input');
      input.type = 'text';
      input.value = flagName(edits, flag);
      input.placeholder = flag.name;
      input.addEventListener('change', () => change(setFlagName(edits, flag, input.value.trim() || flag.name)));
      // Drag the name on the map to move it; this puts it back.
      const reset = document.createElement('button');
      reset.className = 'ov-flag-reset';
      reset.textContent = '↺';
      reset.title = STRINGS.OVERVIEWS.FLAG_RESET_TIP;
      reset.disabled = !flagOffset(edits, flag);
      reset.addEventListener('click', () => change(setFlagOffset(edits, flag, null)));
      row.append(input, reset);
      flagList.appendChild(row);
    }
    flagScreenSelect.disabled = !edits.show.flagIcons;

    labelList.innerHTML = '';
    for (const label of edits.labels) {
      const row = document.createElement('div');
      row.className = 'ov-label-row';
      row.classList.toggle('active', label.id === selectedLabel);
      const input = document.createElement('input');
      input.type = 'text';
      input.value = label.text;
      input.dataset.label = label.id;
      input.addEventListener('focus', () => {
        selectedLabel = label.id;
        draw();
      });
      input.addEventListener('input', () => {
        edits = { ...edits, labels: edits.labels.map((l) => (l.id === label.id ? { ...l, text: input.value } : l)) };
        draw();
      });
      input.addEventListener('change', () => {
        history.push({ ...edits, labels: edits.labels.map((l) => (l.id === label.id ? { ...l, text: label.text } : l)) });
        undoBtn.disabled = false;
        persistSoon();
      });
      const size = document.createElement('select');
      size.title = STRINGS.OVERVIEWS.LABEL_SIZE_TITLE;
      for (const px of [11, 15, 20, 26]) {
        const opt = document.createElement('option');
        opt.value = px;
        opt.textContent = `${px}`;
        size.appendChild(opt);
      }
      size.value = label.size || 15;
      size.addEventListener('change', () => change({
        ...edits,
        labels: edits.labels.map((l) => (l.id === label.id ? { ...l, size: Number(size.value) } : l)),
      }));
      const del = document.createElement('button');
      del.textContent = '✕';
      del.title = STRINGS.OVERVIEWS.DELETE_LABEL;
      del.addEventListener('click', () => {
        if (selectedLabel === label.id) selectedLabel = null;
        change({ ...edits, labels: edits.labels.filter((l) => l.id !== label.id) });
      });
      row.append(input, size, del);
      labelList.appendChild(row);
    }
    saveBtn.disabled = !scene;
    resetBtn.disabled = !scene;
  }

  formatSelect.addEventListener('change', () => change({ ...edits, format: formatSelect.value }, { remember: false }));
  themeSelect.addEventListener('change', () => {
    defaultTheme = themeSelect.value;
    storageSet(THEME_KEY, defaultTheme);
    change({ ...edits, theme: defaultTheme });
  });
  for (const t of THEMES) {
    const opt = document.createElement('option');
    opt.value = t.id;
    opt.textContent = STRINGS.OVERVIEWS.THEMES[t.id] || t.id;
    themeSelect.appendChild(opt);
  }
  hdBox.addEventListener('change', () => change({ ...edits, hd: hdBox.checked }, { remember: false }));

  // ── Installs and maps ──────────────────────────────────────────────────
  function renderMapList() {
    const filter = filterInput.value.trim().toLowerCase();
    // Keyboard focus stays on the picked row across a rebuild.
    const hadFocus = mapList.contains(document.activeElement);
    mapList.innerHTML = '';
    for (const entry of maps) {
      if (filter && !entry.name.toLowerCase().includes(filter)) continue;
      const row = document.createElement('button');
      row.className = 'ov-map-row';
      row.classList.toggle('active', (pending ?? opening ?? scene?.map) === entry.name);
      row.dataset.map = entry.name;
      const name = document.createElement('span');
      name.textContent = entry.name;
      row.appendChild(name);
      const badge = (text, tip, kind) => {
        const b = document.createElement('span');
        b.className = `ov-badge ov-badge-${kind}`;
        b.textContent = text;
        b.title = tip;
        row.appendChild(b);
      };
      if (entry.has_edits) badge(STRINGS.OVERVIEWS.BADGE_EDITED, STRINGS.OVERVIEWS.BADGE_EDITED_TIP, 'edited');
      if (entry.has_ours) badge(STRINGS.OVERVIEWS.BADGE_OURS, STRINGS.OVERVIEWS.BADGE_OURS_TIP, 'ours');
      else if (entry.has_overview) badge(STRINGS.OVERVIEWS.BADGE_HAS_OVERVIEW, STRINGS.OVERVIEWS.BADGE_HAS_OVERVIEW_TIP, 'has');
      row.addEventListener('click', () => openMap(entry.name));
      mapList.appendChild(row);
    }
    if (hadFocus) mapList.querySelector('.ov-map-row.active')?.focus();
  }
  filterInput.addEventListener('input', renderMapList);

  // Up and Down move through the map list (from the filter box too) and open
  // each map, after a short pause so holding a key doesn't build every one.
  let pending = null;
  let pendingTimer = null;
  function step(delta) {
    const rows = [...mapList.querySelectorAll('.ov-map-row')];
    if (!rows.length) return;
    let at = rows.findIndex((r) => r.classList.contains('active'));
    at = at < 0 ? (delta > 0 ? 0 : rows.length - 1) : Math.max(0, Math.min(rows.length - 1, at + delta));
    rows.forEach((r, i) => r.classList.toggle('active', i === at));
    rows[at].focus();
    rows[at].scrollIntoView({ block: 'nearest' });
    pending = rows[at].dataset.map;
    clearTimeout(pendingTimer);
    pendingTimer = setTimeout(() => {
      const name = pending;
      pending = null;
      if (name && name !== scene?.map) openMap(name);
    }, 200);
  }
  for (const el of [mapList, filterInput]) {
    el.addEventListener('keydown', (event) => {
      if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
      event.preventDefault();
      step(event.key === 'ArrowDown' ? 1 : -1);
    });
  }

  async function loadMaps() {
    maps = [];
    renderMapList();
    if (!install) return;
    try {
      maps = await overviewMaps(install);
    } catch {
      maps = [];
    }
    renderMapList();
  }

  installSelect.addEventListener('change', () => {
    install = installSelect.value;
    storageSet(INSTALL_KEY, install);
    scene = null;
    title.textContent = STRINGS.OVERVIEWS.NO_MAP_TITLE;
    draw();
    renderSidePanels();
    loadMaps();
  });

  async function loadInstalls() {
    try {
      installs = await overviewInstalls(gamePath());
    } catch {
      installs = [];
    }
    installSelect.innerHTML = '';
    for (const i of installs) {
      const opt = document.createElement('option');
      opt.value = i.path;
      opt.textContent = i.name;
      installSelect.appendChild(opt);
    }
    if (!installs.length) {
      empty.textContent = STRINGS.OVERVIEWS.NO_INSTALLS;
      return;
    }
    // The last one used, else the stock install people play on.
    const remembered = storageGet(INSTALL_KEY);
    const stock = installs.find((i) => i.name === 'Half-Life');
    install = installs.find((i) => i.path === remembered)?.path || stock?.path || installs[0].path;
    installSelect.value = install;
    await loadMaps();
  }

  function showProgress(fraction) {
    buildingText.textContent = STRINGS.OVERVIEWS.building(opening, fraction);
    buildingFill.style.width = `${Math.round(fraction * 100)}%`;
  }
  listen('overview_progress', (event) => {
    if (event.payload?.map === opening) showProgress(event.payload.fraction);
  });

  // ── The game's flag icons, previewed ───────────────────────────────────
  // Read from the install for the map shown (overview_flag_icons), sized
  // for a screen height: the one picked here, else the game's own.
  function renderScreenOptions() {
    flagScreenSelect.innerHTML = '';
    const heights = [...new Set([...SCREEN_HEIGHTS, flagScreen, gameScreen].filter(Boolean))].sort((a, b) => a - b);
    for (const h of heights) {
      const opt = document.createElement('option');
      opt.value = h;
      opt.textContent = STRINGS.OVERVIEWS.flagScreen(h, h === gameScreen);
      flagScreenSelect.appendChild(opt);
    }
    flagScreenSelect.value = flagScreen;
  }
  flagScreenSelect.addEventListener('change', () => {
    flagScreen = Number(flagScreenSelect.value);
    storageSet(FLAG_SCREEN_KEY, String(flagScreen));
    draw();
  });
  overviewScreenHeight().then((h) => {
    gameScreen = h || null;
    if (gameScreen && !storageGet(FLAG_SCREEN_KEY)) flagScreen = gameScreen;
    renderScreenOptions();
    draw();
  });
  renderScreenOptions();

  async function loadFlagIcons(name) {
    flagIcons = [];
    try {
      const list = await overviewFlagIcons(install, name);
      if (scene?.map !== name) return;
      flagIcons = list.map((icon) => {
        const bytes = Uint8ClampedArray.from(atob(icon.rgba), (c) => c.charCodeAt(0));
        const image = document.createElement('canvas');
        image.width = icon.width;
        image.height = icon.height;
        image.getContext('2d').putImageData(new ImageData(bytes, icon.width, icon.height), 0, 0);
        return { world: icon.world, width: icon.width, height: icon.height, image };
      });
      draw();
    } catch {
      // No icons to preview: the box just shows nothing.
    }
  }

  // A newer pick makes the backend drop the older build (overview_manager.rs).
  // The last map goes at once: the page shows the one being built, not the
  // one before it.
  async function openMap(name) {
    const token = ++loadToken;
    opening = name;
    for (const row of mapList.querySelectorAll('.ov-map-row')) {
      row.classList.toggle('active', row.dataset.map === name);
    }
    scene = null;
    selectedLabel = null;
    title.textContent = name;
    if (footer) footer.textContent = '';
    showProgress(0);
    renderSidePanels();
    draw();
    try {
      const [built, saved] = await Promise.all([overviewScene(install, name), overviewLoadEdits(name, install).catch(() => null)]);
      if (token !== loadToken) return;
      opening = null;
      building.hidden = true;
      scene = built;
      resetZoom();
      const fit = fitEdits(built, normaliseEdits(saved));
      edits = { ...fit.edits, theme: fit.edits.theme || defaultTheme };
      history = [];
      undoBtn.disabled = true;
      selectedLabel = null;
      for (const key of Object.keys(cache)) delete cache[key];
      title.textContent = name;
      if (footer) footer.textContent = STRINGS.OVERVIEWS.footer(name, scene.areas.length, scene.faces.length);
      const { areas, faces, flagNames } = fit.unplaced;
      saveStatus.textContent = areas + faces + flagNames ? STRINGS.OVERVIEWS.keptAside(fit.unplaced) : '';
    } catch {
      if (token === loadToken) {
        opening = null;
        title.textContent = name;
        draw();
      }
      return;
    }
    renderPalette();
    renderSidePanels();
    renderMapList();
    draw();
    loadFlagIcons(name);
  }

  // ── Save and start over ────────────────────────────────────────────────
  saveBtn.addEventListener('click', async () => {
    if (!scene) return;
    saveBtn.disabled = true;
    saveStatus.textContent = STRINGS.OVERVIEWS.SAVING;
    try {
      const image = renderExport(scene, edits);
      const result = await overviewExport({
        install,
        map: scene.map,
        // The overview the game reads goes in dod, which every launch reads;
        // the high-quality copy, which only DoD Studio's hook reads, goes
        // in dod_addon (files.rs save_hd).
        target: 'game',
        format: edits.format || 'tga',
        width: image.width,
        height: image.height,
        rgba: toBase64(image.rgba),
        transform: scene.transform,
        // Kept beside the HD copy, so the edits go wherever the overview does.
        edits,
      });
      const written = [...result.written];
      if (edits.hd !== false) {
        const hd = renderHd(scene, edits);
        const path = await overviewExportHd(
          { install, map: scene.map, width: hd.width, height: hd.height },
          new Uint8Array(hd.rgba.buffer),
        );
        written.push(path);
      }
      const lines = [STRINGS.OVERVIEWS.saved(edits.hd !== false)];
      if (result.backed_up.length) lines.push(STRINGS.OVERVIEWS.BACKED_UP);
      saveStatus.textContent = lines.join(' ');
      saveStatus.title = STRINGS.OVERVIEWS.savedPaths([...written, ...result.backed_up]);
      showToast(STRINGS.OVERVIEWS.savedToast(scene.map), 'success');
      const entry = maps.find((m) => m.name === scene.map);
      if (entry) {
        entry.has_ours = true;
        renderMapList();
      }
    } catch {
      saveStatus.textContent = '';
      saveStatus.title = '';
    } finally {
      saveBtn.disabled = false;
    }
  });

  resetBtn.addEventListener('click', async () => {
    if (!scene) return;
    if (!(await themedConfirm(STRINGS.OVERVIEWS.resetConfirm(scene.map)))) return;
    try {
      await overviewResetEdits(scene.map);
    } catch {
      return;
    }
    clearTimeout(saveTimer);
    history.push(edits);
    edits = { ...emptyEdits(), theme: defaultTheme };
    undoBtn.disabled = false;
    selectedLabel = null;
    const entry = maps.find((m) => m.name === scene.map);
    if (entry) entry.has_edits = false;
    renderMapList();
    renderSidePanels();
    draw();
  });

  // ── First show ─────────────────────────────────────────────────────────
  pane.addEventListener('overviews-shown', () => {
    if (!loaded) {
      loaded = true;
      loadInstalls();
    }
    requestAnimationFrame(draw);
  });
  new ResizeObserver(() => { if (scene) draw(); }).observe(wrap);

  renderPalette();
  renderSidePanels();
  draw();
}
