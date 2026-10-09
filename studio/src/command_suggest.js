// command_suggest.js
// Type-ahead for the Commands tab (#215): typing the start of a console name
// in Initial or Scheduled Commands lists every name it could be, the way the
// game's console completes. The names are console_commands_data.js's (read
// from the game's DLLs, HLAE and this repo by goldsrc-hooks/tools/
// console_names.py). A name Studio refuses or overrides says so in the list,
// before the warning banner would.

import { GAME_NAMES, HLAE_NAMES, DODSTUDIO_NAMES } from './console_commands_data.js';
import { STRINGS } from './strings.js';

/** At most this many suggestions show at once. */
export const MAX_SUGGESTIONS = 12;

// Copies of native::patch::cfg_scan's tiers (command_suggest.test.js checks
// them against cfg_scan.rs). Banned for two reasons, so two lists.
export const OWNED_BY_STUDIO = ['mirv_recordmovie_start', 'mirv_recordmovie_stop', 'mirv_movie_ffmpeg', 'host_framerate'];
export const GAME_QUITS_OVER = ['r_drawentities', 'cl_lw'];
export const SCHEDULED_BANNED = ['r_decals', 'mirv_fov', 'gl_widescreenfov', 'mirv_movie_filename', 'mirv_agr'];
export const MID_DEMO_HAZARDS = ['r_decals', 'mirv_fov', 'gl_widescreenfov', 'mirv_movie_filename',
  'mirv_recordmovie_start', 'mirv_recordmovie_stop', 'mirv_movie_fps', 'mirv_movie_ffmpeg', 'host_framerate', 'mirv_agr'];
export const NOOP_EVERYWHERE = ['exec', 'quit'];
export const NOOP_IN_INIT = ['mirv_movie_filename'];

/** Every name, once: `{ name, source, kind, builds, hint }`. */
const ALL = (() => {
  const byName = new Map();
  const add = (entry) => { if (!byName.has(entry.name)) byName.set(entry.name, entry); };
  DODSTUDIO_NAMES.forEach(([name, hint]) => add({ name, source: 'dodstudio', kind: null, builds: 'both', hint }));
  HLAE_NAMES.forEach((name) => add({ name, source: 'hlae', kind: 'cmd', builds: 'both', hint: '' }));
  GAME_NAMES.forEach(([name, kind, builds]) => add({ name, source: 'game', kind, builds, hint: '' }));
  return [...byName.values()].sort((a, b) => a.name.localeCompare(b.name));
})();

/** What Studio does with `name` typed in this list, or null when nothing:
 *  `{ level: 'refused' | 'warned' | 'noop', text }`. */
export function tierNote(name, { scheduled }) {
  const S = STRINGS.COMMAND_SUGGEST;
  if (OWNED_BY_STUDIO.includes(name)) return { level: 'refused', text: S.OWNED_BY_STUDIO };
  if (GAME_QUITS_OVER.includes(name)) return { level: 'refused', text: S.GAME_QUITS_OVER };
  if (scheduled && SCHEDULED_BANNED.includes(name)) return { level: 'refused', text: S.SCHEDULED_BANNED };
  if (NOOP_EVERYWHERE.includes(name)) return { level: 'noop', text: S.NOOP_EVERYWHERE };
  if (!scheduled && NOOP_IN_INIT.includes(name)) return { level: 'noop', text: S.NOOP_IN_INIT };
  if (MID_DEMO_HAZARDS.includes(name)) return { level: 'warned', text: S.HAS_A_SETTING };
  return null;
}

/** The console name being typed: the first word, while the cursor is still
 *  in it. Null once a space has been typed after it. */
export function typedName(value, cursor = value.length) {
  const before = String(value).slice(0, cursor);
  if (/\s/.test(before.trimStart())) return null;
  return before.trimStart();
}

/**
 * The names starting with `prefix` (case doesn't matter), an exact match
 * first, then alphabetical; at most `limit`. Each carries its `note` for
 * Initial (`scheduled: false`) or Scheduled Commands.
 */
export function suggestions(prefix, { scheduled = false, limit = MAX_SUGGESTIONS } = {}) {
  const p = String(prefix || '').toLowerCase();
  if (!p) return [];
  const hits = ALL.filter((e) => e.name.startsWith(p));
  hits.sort((a, b) => (b.name === p) - (a.name === p));
  return hits.slice(0, limit).map((e) => ({ ...e, note: tierNote(e.name, { scheduled }) }));
}

/** `value` with its first word replaced by `name`, and a space after it so
 *  the value can follow. The rest of the line is kept. */
export function acceptSuggestion(value, name) {
  const rest = String(value).replace(/^\s*\S*/, '');
  return rest ? `${name}${rest}` : `${name} `;
}

/**
 * Adds type-ahead to a text input. `scheduled` says which list it is in.
 * Up and Down move through the list, Enter or Tab takes the highlighted
 * name, Escape closes it; a click takes one too.
 */
export function attachCommandSuggest(input, { scheduled = false } = {}) {
  let list = null;
  let items = [];
  let active = 0;

  function close() {
    list?.remove();
    list = null;
    items = [];
  }

  function take(entry) {
    input.value = acceptSuggestion(input.value, entry.name);
    const caret = entry.name.length + 1;
    input.setSelectionRange(caret, caret);
    close();
    input.dispatchEvent(new Event('input', { bubbles: true }));
  }

  function render() {
    const name = typedName(input.value, input.selectionStart ?? input.value.length);
    items = name ? suggestions(name, { scheduled }) : [];
    // Nothing more to say when the name is already typed in full.
    if (!items.length || (items.length === 1 && items[0].name === name && !items[0].note)) {
      close();
      return;
    }
    active = Math.min(active, items.length - 1);
    if (!list) {
      list = document.createElement('ul');
      list.className = 'command-suggest';
      list.setAttribute('role', 'listbox');
      document.body.appendChild(list);
    }
    const box = input.getBoundingClientRect();
    list.style.left = `${box.left + window.scrollX}px`;
    list.style.top = `${box.bottom + window.scrollY + 2}px`;
    list.style.minWidth = `${box.width}px`;
    list.replaceChildren(...items.map((entry, i) => {
      const li = document.createElement('li');
      li.className = 'command-suggest-item';
      li.setAttribute('role', 'option');
      li.classList.toggle('active', i === active);
      if (entry.note) li.classList.add(`command-suggest-${entry.note.level}`);
      const nameEl = document.createElement('span');
      nameEl.className = 'command-suggest-name';
      nameEl.textContent = entry.name;
      const meta = document.createElement('span');
      meta.className = 'command-suggest-meta';
      meta.textContent = entry.note?.text || entry.hint || STRINGS.COMMAND_SUGGEST.describe(entry.source, entry.kind, entry.builds);
      li.title = [entry.note?.text, entry.hint].filter(Boolean).join(' ') || meta.textContent;
      li.append(nameEl, meta);
      // mousedown, so the input keeps focus (a blur would close the list).
      li.addEventListener('mousedown', (e) => {
        e.preventDefault();
        take(entry);
      });
      return li;
    }));
  }

  input.setAttribute('autocomplete', 'off');
  input.setAttribute('spellcheck', 'false');
  input.addEventListener('input', () => { active = 0; render(); });
  input.addEventListener('focus', render);
  input.addEventListener('blur', close);
  input.addEventListener('keydown', (e) => {
    if (!list || !items.length) return;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      active = (active + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
      render();
    } else if (e.key === 'Enter' || e.key === 'Tab') {
      e.preventDefault();
      take(items[active]);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      close();
    }
  });
}
