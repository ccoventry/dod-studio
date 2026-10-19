// review_hltv_picker.js
// The HLTV box's "Players I pick" (#828): beside the box, the picked players
// as chips with an ×, a list to add anyone with a highlight in the queue's
// HLTV demos, and one to add a whole team from the Teams list (#445). It
// owns the box's choice too, so the project file can keep both
// (review_hltv_pick.js has the logic).

import { HLTV_PLAYERS } from './review_queue.js';
import {
  hltvDemoPlayers, hltvTeams, emptyHltvPick, normalizeHltvPick, savedHltvPick,
  addPickedPlayers, removePickedPlayer,
} from './review_hltv_pick.js';
import { STRINGS } from './strings.js';

/**
 * - getDemos(): the queue's demos
 * - getProjectTeams(): the Teams list's names and merges
 * - onChange(): the pick changed (mark the project dirty)
 *
 * Returns `{ choice(), players(), getState(), setState(raw) }`; getState and
 * setState are the project file's `review`.
 */
export function createHltvPicker({ getDemos, getProjectTeams, onChange = () => {} }) {
  const select = document.querySelector('#review-hltv-select');
  const box = document.querySelector('#review-hltv-pick');
  const chips = document.querySelector('#review-hltv-chips');
  const addPlayer = document.querySelector('#review-hltv-add-player');
  const addTeam = document.querySelector('#review-hltv-add-team');
  let pick = emptyHltvPick();

  const option = (list, value, label, title = '') => {
    const o = document.createElement('option');
    o.value = value;
    o.textContent = label;
    if (title) o.title = title;
    list.appendChild(o);
    return o;
  };

  // The two lists, refilled each time one opens so they follow the queue.
  let teams = [];
  function fillLists() {
    const players = hltvDemoPlayers(getDemos());
    if (addPlayer) {
      addPlayer.textContent = '';
      const picked = new Set(pick.players.map((p) => p.id));
      const left = players.filter((p) => !picked.has(p.id));
      option(addPlayer, '', players.length ? STRINGS.REVIEW.PICK_ADD_PLAYER : STRINGS.REVIEW.PICK_NO_PLAYERS);
      for (const p of left) option(addPlayer, p.id, p.label);
      addPlayer.disabled = left.length === 0;
      addPlayer.value = '';
    }
    if (addTeam) {
      teams = hltvTeams(getDemos(), getProjectTeams(), players);
      addTeam.textContent = '';
      option(addTeam, '', teams.length ? STRINGS.REVIEW.PICK_ADD_TEAM : STRINGS.REVIEW.PICK_NO_TEAMS);
      for (const t of teams) {
        option(addTeam, t.tag, STRINGS.REVIEW.pickTeamOption(t.name, t.players.length), t.players.map((p) => p.label).join('\n'));
      }
      addTeam.disabled = teams.length === 0;
      addTeam.value = '';
    }
  }

  // A picked player with no highlight in the queue's HLTV demos any more is
  // kept, dimmed, for when that demo is back.
  function renderChips() {
    if (!chips) return;
    chips.textContent = '';
    const here = new Set(hltvDemoPlayers(getDemos()).map((p) => p.id));
    if (!pick.players.length) {
      const none = document.createElement('span');
      none.className = 'review-hltv-none';
      none.textContent = STRINGS.REVIEW.PICK_NONE;
      chips.appendChild(none);
    }
    for (const p of pick.players) {
      const chip = document.createElement('span');
      chip.className = 'review-hltv-chip';
      chip.dataset.id = p.id;
      if (!here.has(p.id)) {
        chip.classList.add('review-hltv-chip-away');
        chip.title = STRINGS.REVIEW.PICK_NOT_IN_QUEUE_TITLE;
      }
      const name = document.createElement('span');
      name.textContent = p.label;
      const remove = document.createElement('button');
      remove.type = 'button';
      remove.className = 'review-hltv-chip-x';
      remove.textContent = '×';
      remove.title = STRINGS.REVIEW.PICK_REMOVE;
      remove.setAttribute('aria-label', STRINGS.REVIEW.PICK_REMOVE);
      remove.addEventListener('click', () => {
        pick.players = removePickedPlayer(pick.players, p.id);
        changed();
      });
      chip.append(name, remove);
      chips.appendChild(chip);
    }
  }

  function render() {
    if (box) box.hidden = pick.choice !== HLTV_PLAYERS.PICKED;
    renderChips();
    fillLists();
  }

  function changed() {
    render();
    onChange();
  }

  select?.addEventListener('change', () => {
    pick.choice = select.value;
    changed();
  });
  for (const list of [addPlayer, addTeam]) {
    list?.addEventListener('mousedown', fillLists);
    list?.addEventListener('focus', fillLists);
  }
  addPlayer?.addEventListener('change', () => {
    const p = hltvDemoPlayers(getDemos()).find((q) => q.id === addPlayer.value);
    if (!p) return;
    pick.players = addPickedPlayers(pick.players, [p]);
    changed();
  });
  addTeam?.addEventListener('change', () => {
    const team = teams.find((t) => t.tag === addTeam.value);
    if (!team) return;
    pick.players = addPickedPlayers(pick.players, team.players);
    changed();
  });

  render();

  return {
    /** The box's choice, which the HLTV box shows once its option exists. */
    choice: () => pick.choice,
    players: () => pick.players,
    getState: () => savedHltvPick(pick),
    setState(raw) {
      pick = normalizeHltvPick(raw);
      if (select && [...select.options].some((o) => o.value === pick.choice)) select.value = pick.choice;
      render();
    },
    refresh: render,
  };
}
