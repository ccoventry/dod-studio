import { describe, it, expect, vi } from 'vitest';

vi.mock('./ipc_bridge.js', () => ({ findInstalls: vi.fn(), listGameFolders: vi.fn() }));
vi.mock('./running_game_guard.js', () => ({ closeGameWithOtherSettings: vi.fn() }));
vi.mock('./game_folder_ui.js', () => ({ getGameFolder: vi.fn(), setGameFolder: vi.fn() }));

import {
  pathKey, isHidden, withHidden, installText, installRows,
  switchedGameFolder, installToPrefill, getHiddenInstalls, setHiddenInstalls,
} from './install_picker.js';
import { STRINGS } from './strings.js';

const S = STRINGS.CAPTURE_CONFIG;
const common = 'C:\\Program Files (x86)\\Steam\\steamapps\\common';
const install = (name, build) => ({
  name,
  path: `${common}\\${name}`,
  hl_exe: `${common}\\${name}\\hl.exe`,
  build,
});
const stock = install('Half-Life', 'anniversary');
const pre = install('Half-Life - PRE-Anniversary for Movies', 'pre_anniversary');
const post = install('Half-Life - POST-Anniversary for Movies', 'anniversary');
const all = [stock, post, pre];

describe('paths', () => {
  it('compares paths the way Windows does', () => {
    expect(pathKey('C:/Games/Half-Life/')).toBe(pathKey('c:\\games\\half-life'));
  });
});

describe('installText', () => {
  it('names the folder and the engine build', () => {
    expect(installText(pre)).toBe('Half-Life - PRE-Anniversary for Movies (pre-Anniversary)');
    expect(installText(post)).toBe('Half-Life - POST-Anniversary for Movies (25th Anniversary)');
    expect(installText({ ...pre, build: null })).toBe(pre.name);
  });
});

describe('the hidden list', () => {
  it('adds and takes out an install by folder, whatever the spelling', () => {
    let hidden = withHidden([], stock.path, true);
    expect(isHidden(stock, hidden)).toBe(true);
    expect(isHidden({ ...stock, path: stock.path.toLowerCase().replace(/\\/g, '/') }, hidden)).toBe(true);
    expect(isHidden(pre, hidden)).toBe(false);
    hidden = withHidden(hidden, stock.path, true);
    expect(hidden).toHaveLength(1);
    hidden = withHidden(hidden, stock.path.toUpperCase(), false);
    expect(hidden).toEqual([]);
  });

  it('keeps only real paths from the settings file', () => {
    setHiddenInstalls([stock.path, '', 3, null]);
    expect(getHiddenInstalls()).toEqual([stock.path]);
    setHiddenInstalls(undefined);
    expect(getHiddenInstalls()).toEqual([]);
  });
});

describe('installRows', () => {
  it('lists every install with a Hide button and nothing else when none is hidden', () => {
    const rows = installRows(all, { current: pre.hl_exe });
    expect(rows.map((r) => r.text)).toEqual(all.map(installText));
    expect(rows.every((r) => r.action?.label === S.INSTALL_HIDE)).toBe(true);
    expect(rows[2].value).toBe(pre.hl_exe);
    expect(rows[2].title).toBe(pre.hl_exe);
  });

  it('leaves a hidden install out, with a row at the end to show it again', () => {
    const toggles = [];
    const rows = installRows(all, {
      current: pre.hl_exe,
      hidden: [stock.path],
      onToggleHidden: () => toggles.push(1),
    });
    expect(rows.map((r) => r.text)).toEqual([installText(post), installText(pre), S.showHiddenInstalls(1)]);
    rows[2].command();
    expect(toggles).toEqual([1]);
  });

  it('shows the hidden ones marked, with Show, when asked', () => {
    const calls = [];
    const rows = installRows(all, {
      hidden: [stock.path],
      showHidden: true,
      onHide: (folder, hide) => calls.push([folder, hide]),
    });
    expect(rows[0].text).toBe(S.installHiddenOption(installText(stock)));
    expect(rows[0].action.label).toBe(S.INSTALL_UNHIDE);
    rows[0].action.run();
    rows[1].action.run();
    expect(calls).toEqual([[stock.path, false], [post.path, true]]);
    expect(rows.at(-1).text).toBe(S.HIDE_HIDDEN_INSTALLS);
  });

  it('keeps the current install listed even when it is hidden', () => {
    const rows = installRows(all, { current: stock.hl_exe, hidden: [stock.path] });
    expect(rows[0].text).toBe(S.installHiddenOption(installText(stock)));
  });

  it('shows a browsed hl.exe that is not one of the installs as its own row', () => {
    const rows = installRows(all, { current: 'D:\\Games\\HL copy\\hl.exe' });
    expect(rows.at(-1)).toMatchObject({ value: 'D:\\Games\\HL copy\\hl.exe', text: S.installOther('HL copy\\hl.exe') });
    // The same path spelt another way is the install's own row.
    expect(installRows(all, { current: pre.hl_exe.replace(/\\/g, '/').toLowerCase() })).toHaveLength(3);
  });

  it('is empty with nothing found and nothing set', () => {
    expect(installRows([], {})).toEqual([]);
  });
});

describe('switchedGameFolder', () => {
  it('keeps a folder the new install also has, else dod', () => {
    expect(switchedGameFolder('dod_movies', ['dod', 'DOD_Movies'])).toBe('DOD_Movies');
    expect(switchedGameFolder('dod_movies', ['dod'])).toBe('dod');
    expect(switchedGameFolder('dod', ['dod', 'dod_movies'])).toBe('dod');
    expect(switchedGameFolder('dod_movies', [])).toBe('dod');
  });
});

describe('installToPrefill', () => {
  it('fills the one install when hl.exe is blank', () => {
    expect(installToPrefill([pre], [], '')).toBe(pre);
    expect(installToPrefill([pre], [], '  ')).toBe(pre);
  });

  it('fills nothing with several installs, none, or a path already set', () => {
    expect(installToPrefill(all, [], '')).toBeNull();
    expect(installToPrefill([], [], '')).toBeNull();
    expect(installToPrefill([pre], [], 'C:\\other\\hl.exe')).toBeNull();
  });

  it('counts only the installs not hidden', () => {
    expect(installToPrefill([stock, pre], [stock.path], '')).toBe(pre);
  });
});
