// nameplate_cell.js
// The Players tab's Nameplate column (#429): a PNG per player, picked with
// the native file dialog, that the VEGAS nameplate script puts on its own
// track above each of their clips (clip_manifest.js carries the path there).
// The cell shows the file's name, and says plainly when the file has gone.
// Keyed by SteamID like Use as, so a row added by name gets none.

import { pickPath } from './pick_path.js';
import { STRINGS } from './strings.js';
import { fileNameOf } from './path_display.js';

const S = () => STRINGS.PLAYERS;

/**
 * Remembers which nameplate files are missing on disk, asked once per set
 * of paths, so a redraw of the table doesn't ask again.
 * @param {object} deps  exists(paths) resolves one boolean per path, or null
 *   when it couldn't tell (nothing is marked missing then); onChange() redraws
 *   once an answer changes what is missing.
 */
export function createNameplateCheck({ exists, onChange }) {
  let askedFor = '';
  let missing = new Set();
  return {
    /** Asks about `paths` unless it already did for exactly these. */
    watch(paths) {
      const wanted = [...new Set((paths || []).filter(Boolean))].sort();
      const key = wanted.join('\n');
      if (key === askedFor) return Promise.resolve();
      askedFor = key;
      if (!wanted.length) {
        missing = new Set();
        return Promise.resolve();
      }
      return Promise.resolve(exists(wanted)).then((answer) => {
        if (key !== askedFor || !Array.isArray(answer)) return;
        const next = new Set(wanted.filter((_, i) => answer[i] === false));
        const changed = next.size !== missing.size || [...next].some((p) => !missing.has(p));
        missing = next;
        if (changed) onChange();
      });
    },
    isMissing: (path) => missing.has(path),
    /** Asks again at the next watch, for a refresh of the tab. */
    reset() { askedFor = ''; },
  };
}

/**
 * One row's cell: a button showing the picked file (or "Pick a PNG…") that
 * opens the dialog, and × to clear it.
 * @param {object} row  `{ nameplate, steam_id }` and `who`, the row's label
 * @param {object} state  `missing`: the file isn't on disk
 * @param {(path: string) => void} onChange  the new path, '' for none
 */
export function nameplateCell({ nameplate, steam_id: steamId, who }, { missing }, onChange) {
  const holder = document.createElement('span');
  holder.className = 'players-nameplate';
  const pick = document.createElement('button');
  pick.type = 'button';
  pick.className = `players-nameplate-pick${missing ? ' missing' : ''}`;
  pick.disabled = !steamId;
  pick.setAttribute('aria-label', S().nameplateAria(who));
  const name = fileNameOf(nameplate);
  pick.textContent = !nameplate ? S().NAMEPLATE_PICK : missing ? S().nameplateMissing(name) : name;
  if (!steamId) pick.title = S().NAMEPLATE_NEEDS_STEAMID;
  else if (missing) pick.title = S().nameplateMissingTitle(nameplate);
  else pick.title = nameplate || S().NAMEPLATE_TITLE;
  pick.addEventListener('click', async () => {
    const picked = await pickPath({
      field: 'players.nameplate',
      current: nameplate,
      multiple: false,
      directory: false,
      title: S().NAMEPLATE_PICK_TITLE,
      filters: [{ name: S().NAMEPLATE_FILTER, extensions: ['png'] }],
    }).catch((err) => {
      console.error('Error opening the nameplate picker:', err);
      return null;
    });
    const path = Array.isArray(picked) ? picked[0] : picked;
    if (path) onChange(path);
  });
  holder.append(pick);
  if (nameplate) {
    const clear = document.createElement('button');
    clear.type = 'button';
    clear.className = 'players-nameplate-clear';
    clear.textContent = '×';
    clear.title = S().NAMEPLATE_CLEAR_TITLE;
    clear.setAttribute('aria-label', S().nameplateClearAria(who));
    clear.addEventListener('click', () => onChange(''));
    holder.append(clear);
  }
  return holder;
}
