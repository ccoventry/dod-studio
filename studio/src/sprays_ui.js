// sprays_ui.js
// Configuration > Sprays (#428): a table of players and the spray each gets
// in a demo, and what everyone not listed gets, per demo type. The players
// come from the queue's demos (the player index, lane 'sprays'); the sprays
// are the PNGs in the game install's logos folder. settings_sync.js saves
// getSprays(); the Rust side mirrors it for the hook DLL.

import { listen } from '@tauri-apps/api/event';
import { listLogoPngs, indexDemoPlayers } from './ipc_bridge.js';
import { STRINGS } from './strings.js';
import {
  normalizeSprays,
  sprayPlayerOptions,
  playerFromInput,
  setPlayerSpray,
  removePlayer,
  playerLabel,
  missingPngs,
} from './sprays.js';

// The setting as last edited or loaded.
let current = normalizeSprays(null);
// The PNGs in logos at the last refresh, and whether there was a folder to list.
let pngs = [];
// The queue's players, as picker options.
let playerOptions = [];
let onEdit = () => {};

const $ = (sel) => document.querySelector(sel);

/** The table to save. */
export function getSprays() {
  return normalizeSprays(current);
}

/** Sets the section from saved settings. */
export function setSprays(raw) {
  current = normalizeSprays(raw);
  render();
}

function edit(next) {
  current = next;
  render();
  onEdit();
}

/** A PNG dropdown holding `value`, marked when it isn't in logos. */
function pngSelect(value, onPick, { allowEmpty = false } = {}) {
  const S = STRINGS.SPRAYS;
  const select = document.createElement('select');
  const names = [...pngs];
  if (value && !names.some((n) => n.toLowerCase() === value.toLowerCase())) names.unshift(value);
  if (allowEmpty || !value) {
    select.append(new Option(S.PICK_PNG, ''));
  }
  for (const name of names) {
    const missing = !pngs.some((n) => n.toLowerCase() === name.toLowerCase());
    select.append(new Option(missing ? S.pngMissingOption(name) : name, name));
  }
  select.value = value || '';
  select.addEventListener('change', () => onPick(select.value));
  return select;
}

function renderRows() {
  const list = $('#sprays-list');
  if (!list) return;
  const S = STRINGS.SPRAYS;
  if (!current.players.length) {
    const empty = document.createElement('div');
    empty.className = 'sprays-empty';
    empty.textContent = S.NO_PLAYERS;
    list.replaceChildren(empty);
    return;
  }
  list.replaceChildren(...current.players.map((row, i) => {
    const item = document.createElement('div');
    item.className = 'sprays-row';
    const who = document.createElement('span');
    who.className = 'sprays-player';
    who.textContent = playerLabel(row);
    who.title = row.steam_id ? S.MATCHED_BY_STEAMID : S.MATCHED_BY_NAME;
    const arrow = document.createElement('span');
    arrow.className = 'sprays-arrow';
    arrow.textContent = '→';
    const select = pngSelect(row.png, (png) => edit(setPlayerSpray(current, row, png)));
    const remove = document.createElement('button');
    remove.className = 'sprays-x';
    remove.textContent = '×';
    remove.title = S.REMOVE_TITLE;
    remove.setAttribute('aria-label', S.removeAria(playerLabel(row)));
    remove.addEventListener('click', () => edit(removePlayer(current, i)));
    item.append(who, arrow, select, remove);
    return item;
  }));
}

function renderFallback(kind) {
  const choice = $(`#sprays-${kind}-fallback`);
  const holder = $(`#sprays-${kind}-fallback-png`);
  if (!choice || !holder) return;
  choice.value = current[`${kind}_fallback`];
  const usesPng = current[`${kind}_fallback`] === 'png';
  holder.style.display = usesPng ? '' : 'none';
  holder.replaceChildren(pngSelect(current[`${kind}_fallback_png`], (png) => {
    edit({ ...current, [`${kind}_fallback_png`]: png });
  }, { allowEmpty: true }));
}

function renderPicker() {
  const options = $('#sprays-player-options');
  if (options) {
    options.replaceChildren(...playerOptions.map((o) => new Option(o.label, o.label)));
  }
  const add = $('#sprays-add-png');
  if (add) {
    const keep = add.value;
    add.replaceChildren(new Option(STRINGS.SPRAYS.PICK_PNG, ''), ...pngs.map((n) => new Option(n, n)));
    add.value = pngs.includes(keep) ? keep : '';
  }
  refreshAddButton();
}

function refreshAddButton() {
  const button = $('#sprays-add-btn');
  if (!button) return;
  button.disabled = !($('#sprays-player-input')?.value.trim() && $('#sprays-add-png')?.value);
}

function renderWarning() {
  const box = $('#sprays-warning');
  if (!box) return;
  const S = STRINGS.SPRAYS;
  const lines = [];
  if (!pngs.length) lines.push(S.NO_PNGS);
  const missing = missingPngs(current, pngs);
  if (missing.length) lines.push(S.pngsMissing(missing));
  if (current.hltv_fallback === 'png' && !current.hltv_fallback_png) lines.push(S.HLTV_FALLBACK_UNSET);
  if (current.pov_fallback === 'png' && !current.pov_fallback_png) lines.push(S.POV_FALLBACK_UNSET);
  box.textContent = lines.join('\n');
  box.style.whiteSpace = 'pre-line';
  box.style.display = lines.length ? '' : 'none';
}

function render() {
  renderRows();
  renderFallback('pov');
  renderFallback('hltv');
  renderPicker();
  renderWarning();
}

/**
 * Wires the section.
 * @param {object} app  getDemos() the queue's demos, getHlPath() the
 *   Half-Life Executable field, getGameFolder() the game folder, persist()
 *   saves the settings file.
 * @returns {{ refresh: () => Promise<void> }}  re-lists the PNGs and the
 *   queue's players
 */
export function createSpraysUi({ getDemos, getHlPath, getGameFolder, persist }) {
  onEdit = persist;
  let pngGeneration = 0;
  let playersRequest = 0;
  let playersWanted = 0;
  let playersSeen = 0;
  const found = new Map();

  function showPlayersStatus() {
    const status = $('#sprays-players-status');
    const refreshBtn = $('#sprays-refresh-btn');
    const busy = playersSeen < playersWanted;
    if (refreshBtn) refreshBtn.disabled = busy;
    if (!status) return;
    status.textContent = busy
      ? STRINGS.SPRAYS.readingPlayers(playersSeen, playersWanted)
      : STRINGS.SPRAYS.playersFound(playerOptions.length, playersWanted);
  }

  listen('demo_players', (event) => {
    const p = event.payload || {};
    if (p.lane !== 'sprays' || p.requestId !== playersRequest) return;
    playersSeen += 1;
    found.set(p.path, p.players || []);
    playerOptions = sprayPlayerOptions([...found.values()].flat());
    renderPicker();
    showPlayersStatus();
  }).catch((err) => console.error('Failed to register demo_players listener (sprays):', err));

  async function refreshPngs() {
    const mine = ++pngGeneration;
    const hlPath = getHlPath();
    let listed = [];
    if (hlPath) {
      listed = await listLogoPngs(hlPath, getGameFolder());
    }
    if (mine !== pngGeneration) return;
    pngs = Array.isArray(listed) ? listed : [];
    render();
  }

  async function refreshPlayers() {
    const paths = (getDemos() || []).map((d) => d.path).filter(Boolean);
    playersRequest += 1;
    found.clear();
    playerOptions = [];
    playersWanted = paths.length;
    playersSeen = 0;
    renderPicker();
    showPlayersStatus();
    if (!paths.length) return;
    const request = playersRequest;
    await indexDemoPlayers(paths, request, 'sprays');
    // Whatever came back, the lookup is over for this request.
    if (request === playersRequest) {
      playersSeen = playersWanted;
      showPlayersStatus();
    }
  }

  async function refresh() {
    await Promise.all([refreshPngs(), refreshPlayers()]);
  }

  $('#sprays-player-input')?.addEventListener('input', refreshAddButton);
  $('#sprays-add-png')?.addEventListener('change', refreshAddButton);
  $('#sprays-add-btn')?.addEventListener('click', () => {
    const input = $('#sprays-player-input');
    const png = $('#sprays-add-png')?.value || '';
    const player = playerFromInput(input?.value, playerOptions);
    if (!player || !png) return;
    edit(setPlayerSpray(current, player, png));
    if (input) input.value = '';
    refreshAddButton();
  });
  $('#sprays-refresh-btn')?.addEventListener('click', () => { refresh(); });
  for (const kind of ['pov', 'hltv']) {
    $(`#sprays-${kind}-fallback`)?.addEventListener('change', (e) => {
      edit({ ...current, [`${kind}_fallback`]: e.target.value });
    });
  }
  // Each visit to the tab re-reads both, so PNGs dropped into logos and
  // demos added to the queue since show up.
  document.querySelector('.config-tab-btn[data-tab="tab-sprays"]')
    ?.addEventListener('click', () => { refresh(); });

  render();
  return { refresh };
}
