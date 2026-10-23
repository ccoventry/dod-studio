// install_picker.js
// Configuration > Paths > Half-Life Executable as a list of the Half-Life
// installs found in this PC's Steam libraries (#931, native::sys::installs),
// each with its engine build, so switching between a pre-Anniversary and a
// 25th Anniversary install is one pick. A dropdown you can type into
// (searchable_select.js). The path itself stays in #hl-path-input (hidden),
// which every launch reads and settings_sync.js saves; this only sets it.
// Browse still picks any hl.exe, and a path that isn't one of the found
// installs shows as its own row.
//
// Switching sets hl.exe and the Game Folder and nothing else. "Hide" takes
// an install off the list (saved as hidden_installs); "Show hidden installs"
// at the list's end brings the hidden ones back to unhide.

import { findInstalls, listGameFolders } from './ipc_bridge.js';
import { createSearchableSelect } from './searchable_select.js';
import { closeGameWithOtherSettings } from './running_game_guard.js';
import { DEFAULT_GAME_FOLDER } from './game_folder.js';
import { getGameFolder, setGameFolder } from './game_folder_ui.js';
import { STRINGS } from './strings.js';

/** A path as Windows compares it: either slash, any case, no trailing one. */
export function pathKey(p) {
  return String(p || '').trim().replace(/\//g, '\\').replace(/\\+$/, '').toLowerCase();
}

/** Whether `install` (from findInstalls) is in the hidden list. */
export function isHidden(install, hidden) {
  const key = pathKey(install.path);
  return (hidden || []).some((h) => pathKey(h) === key);
}

/** `hidden` with `folder` added or taken out. */
export function withHidden(hidden, folder, hide) {
  const key = pathKey(folder);
  const rest = (hidden || []).filter((h) => pathKey(h) !== key);
  return hide ? [...rest, folder] : rest;
}

/** "Half-Life - PRE-Anniversary for Movies (pre-Anniversary)". */
export function installText(install) {
  const S = STRINGS.CAPTURE_CONFIG;
  return S.installOption(install.name, S.INSTALL_BUILD[install.build] || '');
}

/**
 * The dropdown's rows: each install not hidden (or every one, hidden ones
 * marked, with `showHidden`), the current hl.exe when it is none of them,
 * and a row to show or hide the hidden ones when there are any. The current
 * install is always listed, hidden or not. `onHide(folder, hide)` runs from
 * a row's Hide/Show button; `onToggleHidden()` from the last row.
 * @returns {{ value, text, title, action?, command? }[]}
 */
export function installRows(installs, { current = '', hidden = [], showHidden = false, onHide = () => {}, onToggleHidden = () => {} } = {}) {
  const S = STRINGS.CAPTURE_CONFIG;
  const rows = [];
  const currentKey = pathKey(current);
  let hiddenCount = 0;
  for (const install of installs || []) {
    const hiddenNow = isHidden(install, hidden);
    const isCurrent = currentKey && pathKey(install.hl_exe) === currentKey;
    if (hiddenNow) hiddenCount += 1;
    if (hiddenNow && !showHidden && !isCurrent) continue;
    const text = installText(install);
    rows.push({
      value: install.hl_exe,
      text: hiddenNow ? S.installHiddenOption(text) : text,
      title: install.hl_exe,
      action: hiddenNow
        ? { label: S.INSTALL_UNHIDE, title: S.INSTALL_UNHIDE_TITLE, run: () => onHide(install.path, false) }
        : { label: S.INSTALL_HIDE, title: S.INSTALL_HIDE_TITLE, run: () => onHide(install.path, true) },
    });
  }
  if (currentKey && !rows.some((r) => pathKey(r.value) === currentKey)) {
    const short = String(current).split(/[\\/]/).slice(-2).join('\\');
    rows.push({ value: current, text: S.installOther(short), title: current });
  }
  if (hiddenCount > 0) {
    rows.push({
      value: '\u0000toggle-hidden',
      text: showHidden ? S.HIDE_HIDDEN_INSTALLS : S.showHiddenInstalls(hiddenCount),
      title: '',
      command: onToggleHidden,
    });
  }
  return rows;
}

/**
 * The Game Folder after switching to an install whose folders are
 * `newFolders` (list_game_folders): the same-named folder when the new
 * install has one (a `dod_movies` copy in both), else `dod`.
 */
export function switchedGameFolder(currentFolder, newFolders) {
  const want = String(currentFolder || '').toLowerCase();
  return (newFolders || []).find((f) => f.toLowerCase() === want) || DEFAULT_GAME_FOLDER;
}

/** The install Setup fills in by itself: the only one not hidden, when
 *  hl.exe is still blank; else null. */
export function installToPrefill(installs, hidden, hlPath) {
  if (String(hlPath || '').trim()) return null;
  const shown = (installs || []).filter((i) => !isHidden(i, hidden));
  return shown.length === 1 ? shown[0] : null;
}

// The saved hidden list, read and written by settings_sync.js.
let hiddenInstalls = [];

export function getHiddenInstalls() {
  return [...hiddenInstalls];
}

export function setHiddenInstalls(list) {
  hiddenInstalls = Array.isArray(list) ? list.filter((p) => typeof p === 'string' && p.trim()) : [];
}

const pathInput = () => document.querySelector('#hl-path-input');

/**
 * Fills #hl-path-input with the one install on this PC when it is blank
 * (Setup's first step, #931). True when it did. A pick goes through the
 * field's change event, which saves the settings.
 */
export async function prefillSingleInstall() {
  const input = pathInput();
  if (!input || input.value.trim()) return false;
  const install = installToPrefill(await findInstalls(''), hiddenInstalls, input.value);
  if (!install) return false;
  input.value = install.hl_exe;
  input.dispatchEvent(new Event('change'));
  return true;
}

/**
 * Wires the dropdown. `persist` saves the settings (a hide).
 * @returns {{ refresh: () => Promise<void> }}  finds the installs again and
 *   shows the saved path, for when hl.exe changes (Browse, load)
 */
export function createInstallPicker({ persist }) {
  const box = document.querySelector('#hl-install-choice');
  let installs = [];
  let showHidden = false;
  let generation = 0;
  if (!box) return { refresh: async () => {} };

  const dropdown = createSearchableSelect(box, {
    noMatch: STRINGS.CAPTURE_CONFIG.INSTALL_NO_MATCH,
    onPick: (value) => { switchTo(value); },
  });
  box.placeholder = STRINGS.CAPTURE_CONFIG.INSTALL_NONE;
  box.title = STRINGS.CAPTURE_CONFIG.INSTALL_PICKER_TITLE;

  function render() {
    const current = pathInput()?.value.trim() || '';
    const rows = installRows(installs, {
      current,
      hidden: hiddenInstalls,
      showHidden,
      onHide: (folder, hide) => {
        hiddenInstalls = withHidden(hiddenInstalls, folder, hide);
        render();
        persist();
      },
      onToggleHidden: () => {
        showHidden = !showHidden;
        render();
      },
    });
    dropdown.setOptions(rows);
    dropdown.setValue(rows.find((r) => !r.command && pathKey(r.value) === pathKey(current))?.value ?? '');
  }

  async function switchTo(hlExe) {
    const input = pathInput();
    if (!input || pathKey(hlExe) === pathKey(input.value)) return;
    // A running game keeps the install it started from (#666): close it or
    // keep the old install.
    const closed = await closeGameWithOtherSettings({ game_path: hlExe }, { switching: true });
    if (!closed) {
      render();
      return;
    }
    let folders = [];
    try {
      folders = (await listGameFolders(hlExe)) || [];
    } catch {
      // Logged by the bridge; `dod` then.
    }
    setGameFolder(switchedGameFolder(getGameFolder(), folders));
    input.value = hlExe;
    // Saves, re-lists the Game Folder and re-checks every hl.exe reader
    // (config_wiring.js, setup_wiring.js).
    input.dispatchEvent(new Event('change'));
  }

  async function refresh() {
    const mine = ++generation;
    render();
    const found = await findInstalls(pathInput()?.value.trim() || '');
    if (mine !== generation) return;
    installs = Array.isArray(found) ? found : [];
    render();
  }

  pathInput()?.addEventListener('change', () => { refresh(); });
  return { refresh };
}
