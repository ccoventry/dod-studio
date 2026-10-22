// players_ui.js
// Configuration > Players (#428): one row per player with the spray and the
// crosshair they get in a demo, what everyone not listed gets, and whether the
// game uses the table at all ("Use in the game", sent to a running game as
// ds_player_overrides). The players come from the queue's demos (the
// player index, lane 'players'); the sprays are the PNGs in the install's
// logos folder, the crosshairs a 64x64 tile of a sprite in its sprites
// folder, picked from a 4x4 preview of the sprite. settings_sync.js saves
// getPlayers(); the Rust side mirrors it for the hook DLL.

import { listen } from '@tauri-apps/api/event';
import {
  listLogoPngs,
  listCrosshairSprites,
  crosshairSpriteImage,
  setPlayerOverrides,
  indexDemoPlayers,
} from './ipc_bridge.js';
import { STRINGS } from './strings.js';
import { createCrosshairImport } from './crosshair_import_ui.js';
import {
  normalizePlayers,
  playerOptions as groupOptions,
  playerFromInput,
  addPlayer,
  updatePlayer,
  removePlayer,
  playerLabel,
  tileRect,
  missingPngs,
  missingSprites,
  canPreviewSprites,
  OWN_SPRAY,
  SPRAY_SIZES,
  GRID,
  TILES,
} from './players.js';

// The setting as last edited or loaded.
let current = normalizePlayers(null);
// The files in logos and sprites at the last refresh.
let pngs = [];
let sprites = [];
// The queue's players, as picker options.
let options = [];
let onEdit = () => {};
let getPaths = () => ({ hlPath: '', gameFolder: '' });
// Which tile picker is open: a row's index, 'fallback', or null.
let openPicker = null;
// Each sprite's decoded first frame as a canvas (null when it can't be read),
// by file name, for this hl.exe and game folder.
const spriteImages = new Map();
let imagesFor = '';

const $ = (sel) => document.querySelector(sel);
const S = () => STRINGS.PLAYERS;

/** The table to save. */
export function getPlayers() {
  return normalizePlayers(current);
}

/** Sets the tab from saved settings. */
export function setPlayers(raw) {
  current = normalizePlayers(raw);
  render();
}

function edit(next) {
  current = next;
  render();
  onEdit();
}

/** A sprite decoded once, as a canvas the previews draw from. Null, asked
 *  for and kept nowhere, until both paths are known: settings_sync.js renders
 *  the tab before createPlayersUi wires them, and its refresh draws again
 *  once they are (#852). */
function spriteCanvas(file) {
  const paths = getPaths();
  if (!canPreviewSprites(paths)) return Promise.resolve(null);
  const { hlPath, gameFolder } = paths;
  const key = `${hlPath}|${gameFolder}`;
  if (key !== imagesFor) {
    spriteImages.clear();
    imagesFor = key;
  }
  if (!spriteImages.has(file)) {
    spriteImages.set(file, crosshairSpriteImage(hlPath, gameFolder, file).then((img) => {
      if (!img || !img.width || !img.height) return null;
      const bytes = Uint8ClampedArray.from(atob(img.rgba), (c) => c.charCodeAt(0));
      const canvas = document.createElement('canvas');
      canvas.width = img.width;
      canvas.height = img.height;
      canvas.getContext('2d')?.putImageData(new ImageData(bytes, img.width, img.height), 0, 0);
      return canvas;
    }).catch(() => null));
  }
  return spriteImages.get(file);
}

/** A dropdown of `files` holding `value`, marked when it isn't there, with
 *  `emptyLabel` as the empty choice and `extra` (`[{ value, label, title }]`)
 *  after it. */
function fileSelect(files, value, emptyLabel, missingLabel, onPick, extra = []) {
  const select = document.createElement('select');
  const names = [...files];
  const special = extra.some((e) => e.value === value);
  if (value && !special && !names.some((n) => n.toLowerCase() === value.toLowerCase())) names.unshift(value);
  select.append(new Option(emptyLabel, ''));
  for (const e of extra) {
    const option = new Option(e.label, e.value);
    option.title = e.title;
    select.append(option);
  }
  for (const name of names) {
    const missing = !files.some((n) => n.toLowerCase() === name.toLowerCase());
    select.append(new Option(missing ? missingLabel(name) : name, name));
  }
  select.value = value || '';
  select.addEventListener('change', () => onPick(select.value));
  return select;
}

/** The button showing a tile of `sprite`, which opens the picker. */
function tileButton(sprite, tile, isOpen, onToggle) {
  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'players-tile';
  button.disabled = !sprite;
  button.setAttribute('aria-expanded', String(isOpen));
  button.title = S().TILE_TITLE;
  const preview = document.createElement('canvas');
  preview.width = 32;
  preview.height = 32;
  preview.className = 'players-tile-preview';
  const label = document.createElement('span');
  label.textContent = tile ? S().tileLabel(tile) : S().PICK_TILE;
  button.append(preview, label);
  const rect = tileRect(tile);
  if (sprite && rect) {
    spriteCanvas(sprite).then((canvas) => {
      if (canvas) preview.getContext('2d')?.drawImage(canvas, rect.x, rect.y, rect.w, rect.h, 0, 0, 32, 32);
    });
  }
  button.addEventListener('click', onToggle);
  return button;
}

/** The 4x4 picker for `sprite`: the sprite behind, a numbered button per
 *  tile. Without a readable sprite, the numbers alone. */
function tilePicker(sprite, tile, onPick) {
  const picker = document.createElement('div');
  picker.className = 'players-picker';
  const grid = document.createElement('div');
  grid.className = 'players-grid';
  const canvas = document.createElement('canvas');
  canvas.width = 256;
  canvas.height = 256;
  canvas.className = 'players-grid-image';
  grid.append(canvas);
  for (let t = 1; t <= TILES; t += 1) {
    const cell = document.createElement('button');
    cell.type = 'button';
    cell.className = `players-cell${t === tile ? ' selected' : ''}`;
    cell.dataset.tile = String(t);
    cell.textContent = String(t);
    cell.title = S().tileLabel(t);
    cell.style.gridColumn = String(((t - 1) % GRID) + 1);
    cell.style.gridRow = String(Math.floor((t - 1) / GRID) + 1);
    cell.addEventListener('click', () => onPick(t));
    grid.append(cell);
  }
  const note = document.createElement('div');
  note.className = 'players-picker-note';
  picker.append(grid, note);
  spriteCanvas(sprite).then((image) => {
    if (image) {
      canvas.getContext('2d')?.drawImage(image, 0, 0, 256, 256);
      if (image.width !== 256 || image.height !== 256) {
        note.textContent = S().notGridSize(image.width, image.height);
      }
    } else {
      grid.classList.add('players-grid-numbers');
      note.textContent = S().NO_PREVIEW;
    }
  });
  return picker;
}

function crosshairCell(sprite, tile, pickerKey, onChange) {
  const holder = document.createElement('span');
  holder.className = 'players-crosshair';
  const select = fileSelect(sprites, sprite, S().FALLBACK_CROSSHAIR_OPTION, S().spriteMissingOption, (picked) => {
    onChange({ crosshair_sprite: picked, crosshair_tile: picked ? (tile || 1) : 0 });
  });
  select.title = S().SPRITE_TITLE;
  const isOpen = openPicker === pickerKey;
  const button = tileButton(sprite, tile, isOpen, () => {
    openPicker = isOpen ? null : pickerKey;
    render();
  });
  holder.append(select, button);
  return holder;
}

function renderRows() {
  const list = $('#players-list');
  if (!list) return;
  if (!current.players.length) {
    const empty = document.createElement('div');
    empty.className = 'players-empty';
    empty.textContent = S().NO_PLAYERS;
    list.replaceChildren(empty);
    return;
  }
  const head = document.createElement('div');
  head.className = 'players-row players-head';
  for (const text of [S().COLUMN_PLAYER, S().COLUMN_SPRAY, S().COLUMN_CROSSHAIR, '']) {
    const cell = document.createElement('span');
    cell.textContent = text;
    head.append(cell);
  }
  const rows = [head];
  current.players.forEach((row, i) => {
    const item = document.createElement('div');
    item.className = 'players-row';
    const who = document.createElement('span');
    who.className = 'players-name';
    who.textContent = playerLabel(row);
    who.title = row.steam_id ? S().MATCHED_BY_STEAMID : S().MATCHED_BY_NAME;
    const spray = fileSelect(pngs, row.png, S().FALLBACK_SPRAY_OPTION, S().pngMissingOption, (png) => {
      edit(updatePlayer(current, i, { png }));
    }, [{ value: OWN_SPRAY, label: S().FROM_DEMO_OPTION, title: S().FROM_DEMO_TITLE }]);
    spray.className = 'players-spray';
    const crosshair = crosshairCell(row.crosshair_sprite, row.crosshair_tile, i, (patch) => {
      edit(updatePlayer(current, i, patch));
    });
    const remove = document.createElement('button');
    remove.type = 'button';
    remove.className = 'players-x';
    remove.textContent = '×';
    remove.title = S().REMOVE_TITLE;
    remove.setAttribute('aria-label', S().removeAria(playerLabel(row)));
    remove.addEventListener('click', () => {
      openPicker = null;
      edit(removePlayer(current, i));
    });
    item.append(who, spray, crosshair, remove);
    rows.push(item);
    if (openPicker === i && row.crosshair_sprite) {
      rows.push(tilePicker(row.crosshair_sprite, row.crosshair_tile, (t) => {
        openPicker = null;
        edit(updatePlayer(current, i, { crosshair_tile: t }));
      }));
    }
  });
  list.replaceChildren(...rows);
}

function renderSprayFallback(kind) {
  const choice = $(`#players-${kind}-fallback`);
  const holder = $(`#players-${kind}-fallback-png`);
  if (!choice || !holder) return;
  choice.value = current[`${kind}_fallback`];
  holder.style.display = current[`${kind}_fallback`] === 'png' ? '' : 'none';
  holder.replaceChildren(fileSelect(pngs, current[`${kind}_fallback_png`], S().PICK_PNG, S().pngMissingOption, (png) => {
    edit({ ...current, [`${kind}_fallback_png`]: png });
  }));
}

function renderCrosshairFallback() {
  const choice = $('#players-crosshair-fallback');
  const holder = $('#players-crosshair-fallback-pick');
  const pickerHolder = $('#players-crosshair-fallback-picker');
  if (!choice || !holder) return;
  choice.value = current.crosshair_fallback;
  const usesSprite = current.crosshair_fallback === 'sprite';
  holder.style.display = usesSprite ? '' : 'none';
  holder.replaceChildren(crosshairCell(
    current.crosshair_fallback_sprite,
    current.crosshair_fallback_tile,
    'fallback',
    (patch) => edit({
      ...current,
      crosshair_fallback_sprite: patch.crosshair_sprite,
      crosshair_fallback_tile: patch.crosshair_tile || current.crosshair_fallback_tile,
    }),
  ));
  if (pickerHolder) {
    const open = usesSprite && openPicker === 'fallback' && current.crosshair_fallback_sprite;
    pickerHolder.replaceChildren(...(open ? [tilePicker(
      current.crosshair_fallback_sprite,
      current.crosshair_fallback_tile,
      (t) => {
        openPicker = null;
        edit({ ...current, crosshair_fallback_tile: t });
      },
    )] : []));
  }
}

function renderPicker() {
  const list = $('#players-player-options');
  if (list) list.replaceChildren(...options.map((o) => new Option(o.label, o.label)));
  refreshAddButton();
}

function refreshAddButton() {
  const button = $('#players-add-btn');
  if (button) button.disabled = !$('#players-player-input')?.value.trim();
}

function renderWarning() {
  const box = $('#players-warning');
  if (!box) return;
  const lines = [];
  if (!pngs.length) lines.push(S().NO_PNGS);
  const missing = missingPngs(current, pngs);
  if (missing.length) lines.push(S().pngsMissing(missing));
  const missingSpr = missingSprites(current, sprites);
  if (sprites.length && missingSpr.length) lines.push(S().spritesMissing(missingSpr));
  if (current.hltv_fallback === 'png' && !current.hltv_fallback_png) lines.push(S().HLTV_FALLBACK_UNSET);
  if (current.pov_fallback === 'png' && !current.pov_fallback_png) lines.push(S().POV_FALLBACK_UNSET);
  if (!current.use_in_game) lines.push(S().NOT_IN_GAME);
  box.textContent = lines.join('\n');
  box.style.whiteSpace = 'pre-line';
  box.style.display = lines.length ? '' : 'none';
}

function render() {
  const use = $('#players-use-in-game');
  if (use) use.checked = current.use_in_game;
  const size = $('#players-spray-size');
  if (size) {
    if (!size.options.length) {
      size.append(...SPRAY_SIZES.map((n) => new Option(S().spraySizeOption(n), String(n))));
    }
    size.value = String(current.spray_size);
  }
  renderRows();
  renderSprayFallback('pov');
  renderSprayFallback('hltv');
  renderCrosshairFallback();
  renderPicker();
  renderWarning();
}

/**
 * Wires the tab.
 * @param {object} app  getDemos() the queue's demos, getHlPath() the
 *   Half-Life Executable field, getGameFolder() the game folder, persist()
 *   saves the settings file.
 * @returns {{ refresh: () => Promise<void> }}  re-lists the files and the
 *   queue's players
 */
export function createPlayersUi({ getDemos, getHlPath, getGameFolder, persist }) {
  onEdit = persist;
  getPaths = () => ({ hlPath: getHlPath(), gameFolder: getGameFolder() });
  let filesGeneration = 0;
  let playersRequest = 0;
  let playersWanted = 0;
  let playersSeen = 0;
  const found = new Map();

  function showPlayersStatus() {
    const status = $('#players-players-status');
    const refreshBtn = $('#players-refresh-btn');
    const busy = playersSeen < playersWanted;
    if (refreshBtn) refreshBtn.disabled = busy;
    if (!status) return;
    status.textContent = busy
      ? S().readingPlayers(playersSeen, playersWanted)
      : S().playersFound(options.length, playersWanted);
  }

  listen('demo_players', (event) => {
    const p = event.payload || {};
    if (p.lane !== 'players' || p.requestId !== playersRequest) return;
    playersSeen += 1;
    found.set(p.path, p.players || []);
    options = groupOptions([...found.values()].flat());
    renderPicker();
    showPlayersStatus();
  }).catch((err) => console.error('Failed to register demo_players listener (players):', err));

  async function refreshFiles() {
    const mine = ++filesGeneration;
    const { hlPath, gameFolder } = getPaths();
    let listed = [[], []];
    if (hlPath) {
      listed = await Promise.all([listLogoPngs(hlPath, gameFolder), listCrosshairSprites(hlPath, gameFolder)]);
    }
    if (mine !== filesGeneration) return;
    pngs = Array.isArray(listed[0]) ? listed[0] : [];
    sprites = Array.isArray(listed[1]) ? listed[1] : [];
    spriteImages.clear();
    render();
    crosshairImport.refresh();
  }

  // "Make your own crosshair…" and "Add a crosshair file…" (#842); a file
  // added re-lists the sprites so the crosshair lists offer it.
  const crosshairImport = createCrosshairImport({
    getPaths,
    getSprites: () => sprites,
    onAdded: () => refreshFiles(),
  });

  async function refreshPlayers() {
    const paths = (getDemos() || []).map((d) => d.path).filter(Boolean);
    playersRequest += 1;
    found.clear();
    options = [];
    playersWanted = paths.length;
    playersSeen = 0;
    renderPicker();
    showPlayersStatus();
    if (!paths.length) return;
    const request = playersRequest;
    await indexDemoPlayers(paths, request, 'players');
    // Whatever came back, the lookup is over for this request.
    if (request === playersRequest) {
      playersSeen = playersWanted;
      showPlayersStatus();
    }
  }

  async function refresh() {
    await Promise.all([refreshFiles(), refreshPlayers()]);
  }

  $('#players-use-in-game')?.addEventListener('change', (e) => {
    const on = !!e.target.checked;
    edit({ ...current, use_in_game: on });
    // A running game follows at once; one started later gets it at launch.
    setPlayerOverrides(on);
  });
  $('#players-player-input')?.addEventListener('input', refreshAddButton);
  $('#players-add-btn')?.addEventListener('click', () => {
    const input = $('#players-player-input');
    const player = playerFromInput(input?.value, options);
    if (!player) return;
    const { table } = addPlayer(current, player);
    if (input) input.value = '';
    edit(table);
  });
  $('#players-refresh-btn')?.addEventListener('click', () => { refresh(); });
  for (const kind of ['pov', 'hltv']) {
    $(`#players-${kind}-fallback`)?.addEventListener('change', (e) => {
      edit({ ...current, [`${kind}_fallback`]: e.target.value });
    });
  }
  $('#players-spray-size')?.addEventListener('change', (e) => {
    edit({ ...current, spray_size: Number(e.target.value) });
  });
  $('#players-crosshair-fallback')?.addEventListener('change', (e) => {
    edit({ ...current, crosshair_fallback: e.target.value });
  });
  // Each visit to the tab re-reads the folders and the queue's players, so
  // files dropped in and demos added since show up.
  document.querySelector('.config-tab-btn[data-tab="tab-players"]')
    ?.addEventListener('click', () => { refresh(); });

  render();
  return { refresh };
}
