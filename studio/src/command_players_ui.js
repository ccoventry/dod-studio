// command_players_ui.js
// The Players cell of a Scheduled Command row: a chip per player with ×, and
// a box that picks from the queue's players or takes a typed SteamID or name.
// Empty, it reads "Everyone", which is what an empty list means.

import { addPlayer, playerChipLabel, playerFromText, queuePlayerOptions } from './command_players.js';
import { steamIdText } from './player_filter.js';
import { STRINGS } from './strings.js';

const DATALIST_ID = 'command-players-options';

/**
 * @param {Object} opts
 * @param {() => Array} opts.getDemos - the queue's demos, read each time the box is focused
 * @param {(demo) => ({id, name}|null)} [opts.recorderOf] - who recorded a demo
 * @returns {{ build: (item, notify: () => void) => HTMLElement }} a list_editor `custom` field builder
 */
export function createCommandPlayersField({ getDemos, recorderOf }) {
  let options = [];

  /** One datalist shared by every row, refilled when a box is focused so it
   *  follows the queue. */
  function refreshOptions() {
    options = queuePlayerOptions(getDemos?.() || [], recorderOf);
    let list = document.getElementById(DATALIST_ID);
    if (!list) {
      list = document.createElement('datalist');
      list.id = DATALIST_ID;
      document.body.appendChild(list);
    }
    list.replaceChildren(...options.map((o) => {
      const opt = document.createElement('option');
      opt.value = o.label;
      return opt;
    }));
  }

  function build(item, notify) {
    const cell = document.createElement('div');
    cell.className = 'command-players list-editor-field';
    cell.title = STRINGS.CAPTURE_CONFIG.COMMAND_PLAYERS_TITLE;

    const input = document.createElement('input');
    input.type = 'text';
    input.className = 'command-players-input';
    input.setAttribute('list', DATALIST_ID);
    input.setAttribute('aria-label', STRINGS.CAPTURE_CONFIG.COMMAND_PLAYERS_ARIA_LABEL);

    function renderChips() {
      const players = Array.isArray(item.players) ? item.players : [];
      cell.classList.toggle('command-players-everyone', players.length === 0);
      input.placeholder = players.length ? '' : STRINGS.CAPTURE_CONFIG.COMMAND_PLAYERS_EVERYONE;
      const chips = players.map((p, i) => {
        const chip = document.createElement('span');
        chip.className = 'command-player-chip';
        chip.textContent = playerChipLabel(p);
        chip.title = steamIdText(p.id) || STRINGS.CAPTURE_CONFIG.COMMAND_PLAYER_BY_NAME;
        const remove = document.createElement('button');
        remove.type = 'button';
        remove.className = 'command-player-remove';
        remove.textContent = '×';
        remove.title = STRINGS.CAPTURE_CONFIG.commandPlayerRemove(playerChipLabel(p));
        remove.addEventListener('click', () => {
          item.players = players.filter((_, j) => j !== i);
          renderChips();
          notify();
        });
        chip.appendChild(remove);
        return chip;
      });
      cell.replaceChildren(...chips, input);
    }

    function commit() {
      const player = playerFromText(input.value, options);
      input.value = '';
      if (!player) return;
      item.players = addPlayer(item.players, player);
      renderChips();
      notify();
      input.focus();
    }

    input.addEventListener('focus', refreshOptions);
    // Picking from the list fires `input` with the whole label; typing
    // waits for Enter or leaving the box.
    input.addEventListener('input', () => {
      if (options.some((o) => o.label === input.value)) commit();
    });
    input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        commit();
      }
    });
    input.addEventListener('change', () => { if (input.value.trim()) commit(); });

    renderChips();
    return cell;
  }

  return { build };
}
