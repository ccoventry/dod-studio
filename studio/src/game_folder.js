// game_folder.js
// The Game folder setting's rules (#402): the <mod> in `-game <mod>`, `dod`
// by default. Pure, so the checks are tested without a DOM; game_folder_ui.js
// draws the field.

import { STRINGS } from './strings.js';

export const DEFAULT_GAME_FOLDER = 'dod';

// Characters Windows refuses in a file name, plus both separators: any of
// them would make the setting a path rather than a folder beside hl.exe.
const BAD_CHARS = /[/\\:*?"<>|\s\u0000-\u001f]/;

/**
 * Why `name` can't be a game folder, or '' when it can. The same rules as
 * native::shared::paths::validate_game_folder, which refuses the save too;
 * checking here says why under the field instead of in a failed-save toast.
 */
export function gameFolderProblem(name) {
  const S = STRINGS.CAPTURE_CONFIG;
  const value = typeof name === 'string' ? name : '';
  if (!value.trim()) return S.GAME_FOLDER_EMPTY;
  if (value === '.' || value.includes('..')) return S.GAME_FOLDER_DOTS;
  const bad = value.match(BAD_CHARS);
  if (bad) return S.gameFolderBadChar(bad[0]);
  if (/^[-+]/.test(value)) return S.GAME_FOLDER_OPTION;
  return '';
}

/**
 * The dropdown's options: every folder found beside hl.exe, plus `saved`
 * when it isn't one of them, so a saved choice is never silently swapped
 * for another. Names match without case, as Windows folders do.
 * @param {string[]} found  list_game_folders' answer
 * @param {string} saved  the setting now
 * @returns {{ value: string, missing: boolean, selected: boolean }[]}
 */
export function gameFolderOptions(found, saved) {
  const current = saved || DEFAULT_GAME_FOLDER;
  const folders = Array.isArray(found) ? found : [];
  const match = folders.find((f) => f.toLowerCase() === current.toLowerCase());
  const options = folders.map((value) => ({ value, missing: false, selected: value === match }));
  if (!match) options.unshift({ value: current, missing: true, selected: true });
  return options;
}
