// overviews_pane.js — the Overviews page (#371): pick an install and a map,
// see the overview made from the map, recolour areas, hide what shouldn't
// show, add labels, and save it where the game reads it.
//
// The scene (what to draw) comes from native::overview; the edits are the
// page's own, saved per map as they change. Drawing is overview_draw.js's.

import { themedConfirm } from './themed_confirm.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';
import {
  overviewInstalls, overviewMaps, overviewScene, overviewLoadEdits,
  overviewSaveEdits, overviewResetEdits, overviewExport, overviewExportHd,
} from './ipc_bridge.js';
import {
  emptyEdits, normaliseEdits, drawOverview, faceAt, labelAt, setAreaEdit,
  areaEdit, setFaceColour, flagName, setFlagName, toWorld, renderExport, renderHd, toBase64,
} from './overview_draw.js';
import { THEMES } from './overview_themes.js';

const INSTALL_KEY = 'overviews.install';
// The theme is one choice for every map, not saved with a map's edits.
const THEME_KEY = 'overviews.theme';
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
  const palette = pane.querySelector('#ov-palette');
  const customColour = pane.querySelector('#ov-custom-colour');
  const clearColourBtn = pane.querySelector('#ov-clear-colour-btn');
  const showBox = pane.querySelector('#ov-show');
  const flagList = pane.querySelector('#ov-flag-names');
  const labelList = pane.querySelector('#ov-labels');
  const formatSelect = pane.querySelector('#ov-format');
  const themeSelect = pane.querySelector('#ov-theme');
  const targetSelect = pane.querySelector('#ov-target');
  const hdBox = pane.querySelector('#ov-hd');
  const saveBtn = pane.querySelector('#ov-save-btn');
  const resetBtn = pane.querySelector('#ov-reset-btn');
  const saveStatus = pane.querySelector('#ov-save-status');
  const undoBtn = pane.querySelector('#ov-undo-btn');
  const footer = document.querySelector('#footer-overviews-summary');

  let installs = [];
  let install = '';
  let maps = [];
  let scene = null;
  let theme = THEMES.some((t) => t.id === storageGet(THEME_KEY)) ? storageGet(THEME_KEY) : THEMES[0].id;
  let edits = { ...emptyEdits(), theme };
  let history = [];
  let mode = 'area';
  // The colour clicks paint with; null puts an area's own colour back.
  let colour = [146, 155, 247];
  let selectedLabel = null;
  let drag = null;
  let saveTimer = null;
  let loaded = false;
  let loadToken = 0;
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
    edits = { ...history.pop(), theme };
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
    if (!scene) {
      canvas.style.display = 'none';
      empty.style.display = '';
      return;
    }
    canvas.style.display = '';
    empty.style.display = 'none';
    const s = layout();
    drawOverview(canvas.getContext('2d'), scene, edits, s, { cache, selectedLabel });
  }

  function pixelOf(event) {
    const r = canvas.getBoundingClientRect();
    return [((event.clientX - r.left) / r.width) * scene.width, ((event.clientY - r.top) / r.height) * scene.height];
  }

  // ── Canvas clicks ──────────────────────────────────────────────────────
  canvas.addEventListener('mousedown', (event) => {
    if (!scene || event.button !== 0) return;
    const [x, y] = pixelOf(event);
    const label = labelAt(scene, edits, x, y);
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
    edits = { ...edits, labels: edits.labels.map((l) => (l.id === drag.id ? { ...l, world } : l)) };
    drag.moved = true;
    draw();
  });

  window.addEventListener('mouseup', () => {
    if (!drag) return;
    if (drag.moved) {
      history.push(drag.before);
      undoBtn.disabled = false;
      persistSoon();
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
    themeSelect.value = edits.theme || 'colours';
    themeSelect.disabled = !scene;
    targetSelect.value = edits.target || 'addon';
    hdBox.checked = edits.hd !== false;

    flagList.innerHTML = '';
    if (scene && !scene.flags.length) {
      const p = document.createElement('p');
      p.className = 'hd-hint';
      p.textContent = STRINGS.OVERVIEWS.NO_FLAGS;
      flagList.appendChild(p);
    }
    for (const flag of scene?.flags || []) {
      const input = document.createElement('input');
      input.type = 'text';
      input.value = flagName(edits, flag);
      input.placeholder = flag.name;
      input.addEventListener('change', () => change(setFlagName(edits, flag, input.value.trim() || flag.name)));
      flagList.appendChild(input);
    }

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
    theme = themeSelect.value;
    storageSet(THEME_KEY, theme);
    edits = { ...edits, theme };
    draw();
    renderSidePanels();
  });
  for (const t of THEMES) {
    const opt = document.createElement('option');
    opt.value = t.id;
    opt.textContent = STRINGS.OVERVIEWS.THEMES[t.id] || t.id;
    themeSelect.appendChild(opt);
  }
  targetSelect.addEventListener('change', () => change({ ...edits, target: targetSelect.value }, { remember: false }));
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
      row.classList.toggle('active', (pending ?? scene?.map) === entry.name);
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

  async function openMap(name) {
    const token = ++loadToken;
    title.textContent = `${name} — ${STRINGS.OVERVIEWS.BUILDING}`;
    try {
      const [built, saved] = await Promise.all([overviewScene(install, name), overviewLoadEdits(name).catch(() => null)]);
      if (token !== loadToken) return;
      scene = built;
      edits = { ...normaliseEdits(saved), theme };
      history = [];
      undoBtn.disabled = true;
      selectedLabel = null;
      for (const key of Object.keys(cache)) delete cache[key];
      title.textContent = name;
      if (footer) footer.textContent = STRINGS.OVERVIEWS.footer(name, scene.areas.length, scene.faces.length);
      saveStatus.textContent = '';
    } catch {
      if (token === loadToken) title.textContent = name;
      return;
    }
    renderPalette();
    renderSidePanels();
    renderMapList();
    draw();
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
        target: edits.target || 'addon',
        format: edits.format || 'tga',
        width: image.width,
        height: image.height,
        rgba: toBase64(image.rgba),
        transform: scene.transform,
      });
      const written = [...result.written];
      if (edits.hd !== false) {
        const hd = renderHd(scene, edits);
        const path = await overviewExportHd(
          { install, map: scene.map, target: edits.target || 'addon', width: hd.width, height: hd.height },
          new Uint8Array(hd.rgba.buffer),
        );
        written.push(path);
      }
      const lines = [STRINGS.OVERVIEWS.saved(written)];
      if (result.backed_up.length) lines.push(STRINGS.OVERVIEWS.backedUp(result.backed_up));
      saveStatus.textContent = lines.join(' ');
      showToast(STRINGS.OVERVIEWS.savedToast(scene.map), 'success');
      const entry = maps.find((m) => m.name === scene.map);
      if (entry) {
        entry.has_ours = true;
        renderMapList();
      }
    } catch {
      saveStatus.textContent = '';
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
    edits = { ...emptyEdits(), theme };
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
