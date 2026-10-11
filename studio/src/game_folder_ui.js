// game_folder_ui.js
// Configuration > Paths > Game Folder (#402): which folder beside hl.exe
// DoD Studio launches with `-game` and reads and writes in (`dod` unless
// set). A dropdown of the folders holding a liblist.gam, so a typo can't
// happen; a text field when none can be listed (no hl.exe set yet, or an
// unreadable folder). settings_sync.js saves getGameFolder().

import { listGameFolders } from './ipc_bridge.js';
import { DEFAULT_GAME_FOLDER, gameFolderProblem, gameFolderOptions } from './game_folder.js';
import { STRINGS } from './strings.js';

// The setting as last chosen or loaded. The form shows it; a value typed
// into the text field only replaces it once it passes gameFolderProblem.
let current = DEFAULT_GAME_FOLDER;
// The folders found beside hl.exe at the last refresh.
let found = [];

const selectEl = () => document.querySelector('#config-game-folder');
const textEl = () => document.querySelector('#config-game-folder-text');
const warningEl = () => document.querySelector('#config-game-folder-warning');

/** The game folder to save and launch with. */
export function getGameFolder() {
  return current;
}

/** Sets the field from saved settings. */
export function setGameFolder(name) {
  current = typeof name === 'string' && name.trim() && !gameFolderProblem(name.trim())
    ? name.trim()
    : DEFAULT_GAME_FOLDER;
  render();
}

function showWarning(message) {
  const el = warningEl();
  if (!el) return;
  el.textContent = message;
  el.style.display = message ? '' : 'none';
}

function render() {
  const select = selectEl();
  const text = textEl();
  const listed = found.length > 0;
  if (select) {
    select.style.display = listed ? '' : 'none';
    select.replaceChildren(...gameFolderOptions(found, current).map((o) => {
      const option = document.createElement('option');
      option.value = o.value;
      option.textContent = o.missing ? STRINGS.CAPTURE_CONFIG.gameFolderNotFoundOption(o.value) : o.value;
      option.selected = o.selected;
      return option;
    }));
  }
  if (text) {
    text.style.display = listed ? 'none' : '';
    text.value = current;
  }
  const missing = listed && !found.some((f) => f.toLowerCase() === current.toLowerCase());
  showWarning(missing ? STRINGS.CAPTURE_CONFIG.gameFolderMissing(current) : '');
}

/**
 * Wires the field. `getHlPath` reads the Half-Life Executable field;
 * `persist` saves the settings file.
 * @returns {{ refresh: () => Promise<void> }}  re-lists the folders, for
 *   when the hl.exe path changes
 */
export function createGameFolderUi({ getHlPath, persist }) {
  // A path change can start a refresh while the last is still listing; only
  // the newest one draws.
  let generation = 0;
  async function refresh() {
    const mine = ++generation;
    const hlPath = getHlPath();
    let listed = [];
    if (hlPath) {
      try {
        listed = await listGameFolders(hlPath);
      } catch {
        // Logged by the bridge. The text field still lets it be set.
        listed = [];
      }
    }
    if (mine !== generation) return;
    found = Array.isArray(listed) ? listed : [];
    render();
  }

  selectEl()?.addEventListener('change', (e) => {
    current = e.target.value || DEFAULT_GAME_FOLDER;
    render();
    persist();
  });

  textEl()?.addEventListener('change', (e) => {
    const value = e.target.value.trim();
    const problem = gameFolderProblem(value);
    if (problem) {
      // Not saved: the backend would refuse it, and the last good value
      // stays in effect.
      showWarning(problem);
      return;
    }
    current = value;
    render();
    persist();
  });

  render();
  return { refresh };
}
