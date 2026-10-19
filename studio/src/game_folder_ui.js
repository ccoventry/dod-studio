// game_folder_ui.js
// Configuration > Paths > Game Folder (#402): which folder beside hl.exe
// DoD Studio launches with `-game` and reads and writes in (`dod` unless
// set). Always a dropdown of the folders holding a liblist.gam and a
// cl_dlls/client.dll (#768), so a typo can't happen; with nothing to list
// (no hl.exe set yet, a path with no file, an unreadable folder) it is
// disabled on the saved value and says why. settings_sync.js saves getGameFolder().

import { listGameFolders } from './ipc_bridge.js';
import { DEFAULT_GAME_FOLDER, gameFolderProblem, gameFolderOptions } from './game_folder.js';
import { STRINGS } from './strings.js';

// The setting as last chosen or loaded. The form shows it.
let current = DEFAULT_GAME_FOLDER;
// The folders found beside hl.exe at the last refresh.
let found = [];

const selectEl = () => document.querySelector('#config-game-folder');
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
  const listed = found.length > 0;
  if (select) {
    select.disabled = !listed;
    // With nothing listed the saved value is the only option, unmarked: it
    // isn't missing, nothing could be looked at.
    const options = listed
      ? gameFolderOptions(found, current)
      : [{ value: current, missing: false, selected: true }];
    select.replaceChildren(...options.map((o) => {
      const option = document.createElement('option');
      option.value = o.value;
      option.textContent = o.missing ? STRINGS.CAPTURE_CONFIG.gameFolderNotFoundOption(o.value) : o.value;
      option.selected = o.selected;
      return option;
    }));
  }
  const S = STRINGS.CAPTURE_CONFIG;
  if (!listed) {
    showWarning(S.GAME_FOLDER_NO_LIST);
    return;
  }
  const missing = !found.some((f) => f.toLowerCase() === current.toLowerCase());
  showWarning(missing ? S.gameFolderMissing(current) : '');
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
        // Logged by the bridge. The field stays disabled on the saved value.
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

  render();
  return { refresh };
}
