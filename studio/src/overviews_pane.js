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
  emptyEdits, normaliseEdits, drawOverview, SPAWN_PROTECTION, faceAt, faceColour, labelAt, setAreaEdit,
  areaEdit, setFaceColour, paintedPieces, clearAreaPieces, flagName, setFlagName, flagOffset, setFlagOffset, flagLabelAt, toWorld,
  spawnNameAt, setSpawnOffset, spawnNameSpots, setSpawnName, resetSpawn, spawnChanged, defaultSpawnName,
  titleShown, titleAt, titleCredit, renderExport, renderHd, toBase64,
} from './overview_draw.js';
import { THEMES, themeOf } from './overview_themes.js';
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
  const recentBox = pane.querySelector('#ov-recent');
  const customColour = pane.querySelector('#ov-custom-colour');
  const clearColourBtn = pane.querySelector('#ov-clear-colour-btn');
  const showBox = pane.querySelector('#ov-show');
  const flagList = pane.querySelector('#ov-flag-names');
  const labelList = pane.querySelector('#ov-labels');
  const formatSelect = pane.querySelector('#ov-format');
  const themeSelect = pane.querySelector('#ov-theme');
  const flagScreenSelect = pane.querySelector('#ov-flag-screen');
  const spawnList = pane.querySelector('#ov-spawn-names');
  const titleReset = pane.querySelector('#ov-title-reset');
  titleReset.addEventListener('click', () => change({ ...edits, titleOffset: null }));
  // The credit on the title's second line (#580): typing the found credit
  // back, or ↺, goes back to following the map; an empty field is none.
  const creditInput = pane.querySelector('#ov-credit');
  const creditReset = pane.querySelector('#ov-credit-reset');
  creditInput.addEventListener('change', () => {
    const typed = creditInput.value.trim();
    change({ ...edits, credit: typed === (scene?.credit ?? '') ? null : typed });
  });
  creditReset.addEventListener('click', () => change({ ...edits, credit: null }));
  const hdBox = pane.querySelector('#ov-hd');
  const saveBtn = pane.querySelector('#ov-save-btn');
  const resetBtn = pane.querySelector('#ov-reset-btn');
  const saveStatus = pane.querySelector('#ov-save-status');
  const undoBtn = pane.querySelector('#ov-undo-btn');
  const showAreasBox = pane.querySelector('#ov-show-areas');
  const showAreasText = showAreasBox.parentElement.querySelector('span');
  const hoverBox = pane.querySelector('#ov-hover-preview');
  const flagIconsBox = pane.querySelector('#ov-flag-icons');
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
    if (lastPointer && typeof updateHover === 'function') hover = hoverAt(lastPointer);
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
      flagIcons: flagIconsBox.checked ? { icons: flagIcons, screenHeight: flagScreen } : null,
      view: { ox, oy, cw: canvas.width, ch: canvas.height },
      overlay: { outlines: showAreasBox.checked ? outlineKind() : null, hover: hoverBox.checked ? hover : null },
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

  // Scroll to zoom around the pointer; drag with the right or middle
  // button, or with Space held, to move about.
  canvas.addEventListener('contextmenu', (event) => event.preventDefault());
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
    setCursor('grab');
  });
  document.addEventListener('keyup', (event) => {
    if (event.code !== 'Space') return;
    spaceDown = false;
    updateCursor(lastPointer);
  });
  canvas.addEventListener('mousedown', (event) => {
    if (!scene || !(event.button === 1 || event.button === 2 || (event.button === 0 && spaceDown))) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    pan = { x: event.clientX, y: event.clientY, centre };
    setCursor('grabbing');
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
    updateCursor(lastPointer);
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
    const spawn = label || flag ? null : spawnNameAt(scene, edits, x, y);
    const onTitle = !label && !flag && !spawn && titleAt(scene, edits, x, y);
    if (onTitle) {
      drag = { title: true, start: [x, y], from: edits.titleOffset || [0, 0], moved: false, before: edits };
      setCursor('grabbing');
      return;
    }
    if (flag) {
      drag = { flag, moved: false, before: edits };
      setCursor('grabbing');
      return;
    }
    if (spawn) {
      drag = { spawn, moved: false, before: edits };
      setCursor('grabbing');
      return;
    }
    if (label) {
      selectedLabel = label.id;
      drag = { id: label.id, moved: false, before: edits };
      setCursor('grabbing');
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
    // Pick from map (or Ctrl-click with any tool): take the colour there.
    if (mode === 'pick' || event.ctrlKey) {
      const taken = faceColour(scene, edits, face);
      if (taken) {
        colour = taken;
        customColour.value = hex(taken);
        remember(taken);
      }
      if (mode === 'pick') setMode(modeBeforePick);
      return;
    }
    remember(colour);
    if (mode === 'face') {
      change(setFaceColour(edits, face, colour));
    } else {
      const area = scene.areas[face.area];
      const old = areaEdit(edits, area) || {};
      const next = { hidden: !!old.hidden, colour };
      // Shift paints over the pieces coloured on their own too.
      const base = event.shiftKey ? clearAreaPieces(scene, edits, area) : edits;
      change(setAreaEdit(base, area, next.hidden || next.colour ? next : null));
    }
  });

  window.addEventListener('mousemove', (event) => {
    if (!drag || !scene) return;
    const [x, y] = pixelOf(event);
    const world = toWorld(scene.transform, x, y);
    if (drag.title) {
      edits = { ...edits, titleOffset: [Math.round(drag.from[0] + x - drag.start[0]), Math.round(drag.from[1] + y - drag.start[1])] };
    } else if (drag.flag) {
      edits = setFlagOffset(edits, drag.flag, [world[0] - drag.flag.world[0], world[1] - drag.flag.world[1]]);
    } else if (drag.spawn) {
      const home = toWorld(scene.transform, drag.spawn.home[0], drag.spawn.home[1]);
      edits = setSpawnOffset(scene, edits, drag.spawn.label, [world[0] - home[0], world[1] - home[1]]);
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
      renderSidePanels();
    }
    drag = null;
    updateCursor(lastPointer);
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

  // The tool to go back to after Pick from map.
  let modeBeforePick = 'area';
  function setMode(next) {
    if (next === 'pick' && mode !== 'pick') modeBeforePick = mode;
    pane.querySelector(`.ov-mode[data-mode="${next}"]`)?.click();
  }
  pane.querySelectorAll('.ov-mode').forEach((btn) => {
    btn.addEventListener('click', () => {
      if (btn.dataset.mode === 'pick' && mode !== 'pick') modeBeforePick = mode;
      mode = btn.dataset.mode;
      pane.querySelectorAll('.ov-mode').forEach((b) => b.classList.toggle('active', b === btn));
      canvas.dataset.mode = mode;
      updateCursor(lastPointer);
      updateHover(lastPointer);
      renderOutlineLabel();
      if (showAreasBox.checked && scene) draw();
    });
  });

  // ── Cursors ───────────────────────────────────────────────────────────
  // What a click does shows in the pointer: a bucket fills an area, a brush
  // paints one piece, an eye hides, a text cursor adds a label, a hand over
  // anything that drags (labels, flag and spawn names), and while moving
  // about the map.
  const svgCursor = (body, x, y, fallback) =>
    `url("data:image/svg+xml,${encodeURIComponent(`<svg xmlns='http://www.w3.org/2000/svg' width='24' height='24' viewBox='0 0 24 24'>${body}</svg>`)}") ${x} ${y}, ${fallback}`;
  const CURSORS = {
    area: svgCursor("<path d='M4 10l7-7 8 8-7 7z' fill='white' stroke='black' stroke-width='1.5'/><path d='M4 10h15' stroke='black' stroke-width='1.5'/><path d='M20 14c0 0 2.5 3 2.5 4.5a2.5 2.5 0 0 1-5 0c0-1.5 2.5-4.5 2.5-4.5z' fill='#3b82f6' stroke='black'/>", 20, 21, 'crosshair'),
    // The bucket with a "+": Shift held, painting over coloured pieces too.
    areaOver: svgCursor("<path d='M4 10l7-7 8 8-7 7z' fill='white' stroke='black' stroke-width='1.5'/><path d='M4 10h15' stroke='black' stroke-width='1.5'/><path d='M20 14c0 0 2.5 3 2.5 4.5a2.5 2.5 0 0 1-5 0c0-1.5 2.5-4.5 2.5-4.5z' fill='#3b82f6' stroke='black'/><circle cx='5' cy='19' r='4.5' fill='#facc15' stroke='black'/><path d='M5 16.5v5M2.5 19h5' stroke='black' stroke-width='1.6'/>", 20, 21, 'crosshair'),
    face: svgCursor("<path d='M14 3l7 7-8 8-4-4z' fill='white' stroke='black' stroke-width='1.5'/><path d='M9 14c-3 0-5 2-5 4 0 1.5-1 2.5-2 3 4 1 8-1 8-4z' fill='#3b82f6' stroke='black' stroke-width='1.2'/>", 2, 21, 'crosshair'),
    hide: svgCursor("<path d='M2 12s4-6 10-6 10 6 10 6-4 6-10 6S2 12 2 12z' fill='white' stroke='black' stroke-width='1.5'/><circle cx='12' cy='12' r='3' fill='black'/><path d='M3 21L21 3' stroke='black' stroke-width='2.5'/><path d='M3 21L21 3' stroke='white' stroke-width='1'/>", 12, 12, 'crosshair'),
    label: 'text',
    pick: svgCursor("<path d='M14.5 4.5l5 5-9.5 9.5H5v-5z' fill='white' stroke='black' stroke-width='1.5'/><path d='M16 2.5a2.1 2.1 0 0 1 3 0l2.5 2.5a2.1 2.1 0 0 1 0 3L19.5 10 14 4.5z' fill='black'/><path d='M5 19l-2.5 2.5' stroke='black' stroke-width='2'/>", 2, 22, 'crosshair'),
  };
  let lastPointer = null;
  function setCursor(cursor) {
    canvas.style.cursor = cursor;
  }

  // ── What a click would change, and the map's areas ─────────────────────
  // overview_overlay.js draws both; `hover` is redrawn only when what is
  // under the pointer changes.
  let hover = null;
  let altDown = false;
  // The page's editing aids, one choice for every map and remembered:
  // hover highlight on unless turned off, the others off unless turned on.
  for (const [box, key, start] of [
    [showAreasBox, 'overviews.showAreas', false],
    [hoverBox, 'overviews.hoverPreview', true],
    [flagIconsBox, 'overviews.flagIcons', false],
  ]) {
    const saved = storageGet(key);
    box.checked = saved == null ? start : saved === '1';
    box.addEventListener('change', () => {
      storageSet(key, box.checked ? '1' : '0');
      flagScreenSelect.disabled = !flagIconsBox.checked;
      draw();
    });
  }
  // "Show areas" outlines what the tool works on: pieces for Colour piece,
  // areas otherwise; Alt shows the other while held.
  function outlineKind() {
    const pieces = mode === 'face';
    return pieces !== altDown ? 'pieces' : 'areas';
  }
  function renderOutlineLabel() {
    showAreasText.textContent = mode === 'face' ? STRINGS.OVERVIEWS.SHOW_PIECES : STRINGS.OVERVIEWS.SHOW_AREAS;
    showAreasBox.parentElement.title = mode === 'face' ? STRINGS.OVERVIEWS.SHOW_PIECES_TIP : STRINGS.OVERVIEWS.SHOW_AREAS_TIP;
  }

  function hoverAt(event) {
    if (!scene || !event || drag || pan || spaceDown || mode === 'label' || mode === 'pick' || ctrlDown) return null;
    const [x, y] = pixelOf(event);
    if (labelAt(scene, edits, x, y) || flagLabelAt(scene, edits, x, y) || spawnNameAt(scene, edits, x, y) || titleAt(scene, edits, x, y)) return null;
    const face = faceAt(scene, edits, x, y, { includeHidden: mode === 'hide' });
    if (!face) return null;
    if (mode === 'face') return { kind: 'piece', face };
    const area = scene.areas[face.area];
    const keep = mode === 'area' && !shiftDown ? paintedPieces(scene, edits, area) : null;
    return { kind: 'area', area: face.area, hide: mode === 'hide' ? !areaEdit(edits, area)?.hidden : null, keep };
  }
  const hoverKey = (h) => (h ? `${h.kind}:${h.kind === 'piece' ? h.face.face : h.area}:${h.hide}:${h.keep?.size ?? '-'}` : '');
  // Ctrl: pick the colour under the pointer, with any painting tool.
  let ctrlDown = false;
  for (const type of ['keydown', 'keyup']) {
    document.addEventListener(type, (event) => {
      if (event.key !== 'Control') return;
      const down = type === 'keydown';
      if (down === ctrlDown) return;
      ctrlDown = down;
      if (!scene) return;
      updateCursor(lastPointer);
      hover = hoverAt(lastPointer);
      drawSoon();
    });
  }
  // Shift: Colour area paints over pieces coloured on their own.
  let shiftDown = false;
  for (const type of ['keydown', 'keyup']) {
    document.addEventListener(type, (event) => {
      if (event.key !== 'Shift') return;
      const down = type === 'keydown';
      if (down === shiftDown) return;
      shiftDown = down;
      if (!scene || mode !== 'area') return;
      updateCursor(lastPointer);
      hover = hoverAt(lastPointer);
      drawSoon();
    });
  }
  function updateHover(event) {
    const next = hoverAt(event);
    if (hoverKey(next) === hoverKey(hover)) return;
    hover = next;
    drawSoon();
  }
  canvas.addEventListener('mouseleave', () => {
    if (hover) {
      hover = null;
      drawSoon();
    }
  });
  for (const type of ['keydown', 'keyup']) {
    document.addEventListener(type, (event) => {
      if (event.key !== 'Alt') return;
      const down = type === 'keydown';
      if (down) event.preventDefault();
      if (down === altDown) return;
      altDown = down;
      if (showAreasBox.checked && scene) draw();
    });
  }
  window.addEventListener('blur', () => {
    if (altDown) {
      altDown = false;
      if (scene) draw();
    }
  });
  function updateCursor(event) {
    if (!scene || pan) return;
    if (spaceDown) return setCursor('grab');
    if (drag) return setCursor('grabbing');
    if (event) {
      const [x, y] = pixelOf(event);
      if (labelAt(scene, edits, x, y) || flagLabelAt(scene, edits, x, y) || spawnNameAt(scene, edits, x, y) || titleAt(scene, edits, x, y)) return setCursor('grab');
    }
    if (ctrlDown && mode !== 'label') return setCursor(CURSORS.pick);
    setCursor((mode === 'area' && shiftDown ? CURSORS.areaOver : CURSORS[mode]) || 'crosshair');
  }
  canvas.addEventListener('mousemove', (event) => {
    lastPointer = event;
    updateCursor(event);
    updateHover(event);
  });
  canvas.addEventListener('mouseleave', () => {
    lastPointer = null;
  });
  undoBtn.addEventListener('click', undo);
  undoBtn.disabled = true;

  // ── Colours ────────────────────────────────────────────────────────────
  // Colours used lately, newest first, for every map and theme: switching
  // scheme doesn't mean hunting for them again.
  const RECENT_KEY = 'overviews.recentColours';
  const RECENT_MAX = 8;
  let recent = [];
  try {
    recent = (JSON.parse(storageGet(RECENT_KEY) || '[]') || []).filter((c) => Array.isArray(c) && c.length === 3);
  } catch {
    recent = [];
  }
  function remember(c) {
    if (!c) return;
    recent = [c, ...recent.filter((r) => hex(r) !== hex(c))].slice(0, RECENT_MAX);
    storageSet(RECENT_KEY, JSON.stringify(recent));
    renderPalette();
  }
  const swatchFor = (c) => {
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
    return swatch;
  };

  function renderPalette() {
    recentBox.innerHTML = '';
    for (const c of recent) recentBox.appendChild(swatchFor(c));
    if (!recent.length) recentBox.textContent = STRINGS.OVERVIEWS.RECENT_NONE;
    palette.innerHTML = '';
    // The theme shown's own colours (overview_themes.js).
    const colours = scene ? themeOf(edits).palette?.(scene) || scene.palette || [] : [];
    for (const c of colours) palette.appendChild(swatchFor(c));
    clearColourBtn.classList.toggle('active', colour === null);
  }
  customColour.addEventListener('input', () => {
    colour = rgb(customColour.value);
    renderPalette();
  });
  customColour.addEventListener('change', () => remember(rgb(customColour.value)));
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

  // Spawn protection's look: fill, line, and every colour.
  const spBox = pane.querySelector('#ov-sp');
  const spControls = {
    fill: pane.querySelector('#ov-sp-fill'),
    line: pane.querySelector('#ov-sp-line'),
    allies: pane.querySelector('#ov-sp-allies'),
    axis: pane.querySelector('#ov-sp-axis'),
    stripe1: pane.querySelector('#ov-sp-stripe1'),
    stripe2: pane.querySelector('#ov-sp-stripe2'),
  };
  for (const [key, control] of Object.entries(spControls)) {
    // Colours preview while dragging; one undo step when let go.
    control.addEventListener('input', () => {
      if (control.type !== 'color') return;
      edits = { ...edits, spawnProtection: { ...edits.spawnProtection, [key]: control.value } };
      draw();
    });
    control.addEventListener('change', () => {
      const before = history.length;
      change({ ...edits, spawnProtection: { ...edits.spawnProtection, [key]: control.value } });
      if (control.type === 'color' && history.length > before) history[history.length - 1] = { ...history[history.length - 1], spawnProtection: spBefore };
    });
    control.addEventListener('focus', () => {
      spBefore = { ...edits.spawnProtection };
    });
  }
  let spBefore = { ...SPAWN_PROTECTION };
  pane.querySelector('#ov-sp-reset').addEventListener('click', () => change({ ...edits, spawnProtection: { ...SPAWN_PROTECTION } }));

  function renderSidePanels() {
    showBox.querySelectorAll('input[data-show]').forEach((box) => {
      // The title's default depends on the theme until it is set.
      box.checked = box.dataset.show === 'title' ? titleShown(edits) : !!edits.show[box.dataset.show];
      box.disabled = !scene;
    });
    titleReset.disabled = !scene || !titleShown(edits) || !edits.titleOffset;
    creditInput.value = scene ? titleCredit(scene, edits) : '';
    creditInput.placeholder = STRINGS.OVERVIEWS.CREDIT_PLACEHOLDER;
    creditInput.disabled = !scene || !titleShown(edits);
    creditReset.disabled = creditInput.disabled || edits.credit == null;
    formatSelect.value = edits.format || 'tga';
    const look = { ...SPAWN_PROTECTION, ...(edits.spawnProtection || {}) };
    for (const [key, control] of Object.entries(spControls)) {
      control.value = look[key];
      control.disabled = !scene || !edits.show.spawnProtection;
    }
    spBox.classList.toggle('ov-off', !edits.show.spawnProtection);
    pane.querySelector('#ov-sp-reset').disabled = !scene || !edits.show.spawnProtection;
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
    flagScreenSelect.disabled = !flagIconsBox.checked;

    // One row per spawn name the map shows: type a name over it, drag it on
    // the map to move it, and ↺ puts both back.
    spawnList.innerHTML = '';
    const spots = scene ? spawnNameSpots(scene, edits) : [];
    if (scene && !spots.length) {
      const p = document.createElement('p');
      p.className = 'hd-hint';
      p.textContent = STRINGS.OVERVIEWS.NO_SPAWN_NAMES;
      spawnList.appendChild(p);
    }
    for (const spot of spots) {
      const row = document.createElement('div');
      row.className = 'ov-flag-row';
      const input = document.createElement('input');
      input.type = 'text';
      input.value = spot.name;
      input.placeholder = defaultSpawnName(spot.team);
      input.title = STRINGS.OVERVIEWS.SPAWN_NAME_TIP;
      input.addEventListener('change', () => change(setSpawnName(scene, edits, spot.label, input.value)));
      const reset = document.createElement('button');
      reset.className = 'ov-flag-reset';
      reset.textContent = '↺';
      reset.title = STRINGS.OVERVIEWS.spawnResetTip(defaultSpawnName(spot.team));
      reset.disabled = !spawnChanged(scene, edits, spot.label);
      reset.addEventListener('click', () => change(resetSpawn(scene, edits, spot.label)));
      row.append(input, reset);
      spawnList.appendChild(row);
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
    defaultTheme = themeSelect.value;
    storageSet(THEME_KEY, defaultTheme);
    change({ ...edits, theme: defaultTheme });
    renderPalette();
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
      hover = null;
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
