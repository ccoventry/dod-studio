import { describe, it, expect, vi } from 'vitest';

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('./ipc_bridge.js', () => ({ pickPathStates: vi.fn(), pickUserFolders: vi.fn() }));

import {
  chooseStartPath, createPickPath, defaultCandidates, folderToRemember, parentOf, PICK_FIELDS, STEAM_COMMON,
} from './pick_path.js';

const file = (path) => ({ path, is_file: true, exact: true, is_root: false });
const dir = (path) => ({ path, is_file: false, exact: true, is_root: false });
const nearest = (path, is_root = false) => ({ path, is_file: false, exact: false, is_root });

// A fake disk: `files` and `dirs` exist; anything else resolves to its
// nearest existing folder, as pick_path_states does.
function disk({ files = [], dirs = [] }) {
  const state = (p) => {
    if (!p) return null;
    if (files.includes(p)) return file(p);
    if (dirs.includes(p)) return dir(p);
    for (let up = parentOf(p); up; up = parentOf(up) === up ? '' : parentOf(up)) {
      if (dirs.includes(up)) return nearest(up, /^[A-Z]:\\$/.test(up));
    }
    return null;
  };
  return vi.fn(async (paths) => paths.map(state));
}

function harness({ files, dirs, picked = null, context = {}, user = { documents: 'C:\\Users\\me\\Documents', videos: 'C:\\Users\\me\\Videos', program_files: 'C:\\Program Files' } } = {}) {
  const open = vi.fn(async () => picked);
  const persist = vi.fn(async () => {});
  const p = createPickPath({
    open,
    pathStates: disk({ files, dirs }),
    userFolders: async () => user,
    getContext: () => context,
    persist,
  });
  return { p, open, persist };
}

describe('parentOf', () => {
  it('keeps a drive root its backslash', () => {
    expect(parentOf('C:\\Games\\hl.exe')).toBe('C:\\Games');
    expect(parentOf('C:\\Games')).toBe('C:\\');
    expect(parentOf('C:\\Games\\')).toBe('C:\\');
    expect(parentOf('D:/demos/a.dem')).toBe('D:/demos');
  });
});

describe('chooseStartPath', () => {
  it('a file field starts on its file, a folder field in its folder', () => {
    expect(chooseStartPath({ states: [file('C:\\HL\\hl.exe'), dir('C:\\Other')] })).toBe('C:\\HL\\hl.exe');
    expect(chooseStartPath({ directory: true, states: [dir('D:\\Demos'), dir('C:\\Other')] })).toBe('D:\\Demos');
  });

  it('a folder picker given a file starts in its folder', () => {
    expect(chooseStartPath({ directory: true, states: [file('C:\\HL\\hl.exe')] })).toBe('C:\\HL');
  });

  it('no path: the remembered folder, then the default', () => {
    expect(chooseStartPath({ states: [null, dir('C:\\Last'), dir('C:\\Default')] })).toBe('C:\\Last');
    expect(chooseStartPath({ states: [null, null, null, dir('C:\\Default')] })).toBe('C:\\Default');
    expect(chooseStartPath({ states: [null, null] })).toBeUndefined();
  });

  it('a moved path: the remembered folder first, then its nearest folder, never a drive root', () => {
    expect(chooseStartPath({ states: [nearest('C:\\HL'), dir('C:\\Last')] })).toBe('C:\\Last');
    expect(chooseStartPath({ states: [nearest('C:\\HL'), null, dir('C:\\Default')] })).toBe('C:\\HL');
    expect(chooseStartPath({ states: [nearest('C:\\', true), null, dir('C:\\Default')] })).toBe('C:\\Default');
  });
});

describe('folderToRemember', () => {
  it("is a file's folder, a folder's parent, a drive itself", () => {
    expect(folderToRemember('C:\\Demos\\a.dem')).toBe('C:\\Demos');
    expect(folderToRemember(['C:\\Demos\\a.dem', 'C:\\Demos\\b.dem'])).toBe('C:\\Demos');
    expect(folderToRemember('D:\\Demos\\Scrims')).toBe('D:\\Demos');
    expect(folderToRemember('G:\\')).toBe('G:\\');
    expect(folderToRemember(null)).toBeNull();
  });
});

describe('defaultCandidates', () => {
  const ctx = { hlPath: 'C:\\Steam\\steamapps\\common\\Half-Life\\hl.exe', gameFolder: 'dod' };
  it('game paths come from hl.exe', () => {
    expect(defaultCandidates('steam', ctx)[0]).toBe('C:\\Steam\\steamapps\\common');
    expect(defaultCandidates('demos', ctx)[0]).toBe('C:\\Steam\\steamapps\\common\\Half-Life\\dod');
    expect(defaultCandidates('steam', {})).toEqual([STEAM_COMMON]);
    expect(defaultCandidates('demos', {})).toEqual([]);
  });
  it('every field has a known default kind', () => {
    const kinds = new Set(['steam', 'game', 'demos', 'videos', 'documents', 'programs', null]);
    Object.values(PICK_FIELDS).forEach((k) => expect(kinds.has(k)).toBe(true));
  });
});

describe('createPickPath', () => {
  it('opens on the current file, with the other options as they were', async () => {
    const { p, open } = harness({ files: ['C:\\HL\\hl.exe'], dirs: ['C:\\HL'] });
    await p.pick({ field: 'config.hl', current: 'C:\\HL\\hl.exe', multiple: false, filters: [{ name: 'Exe', extensions: ['exe'] }] });
    expect(open).toHaveBeenCalledWith({ multiple: false, filters: [{ name: 'Exe', extensions: ['exe'] }], defaultPath: 'C:\\HL\\hl.exe' });
  });

  it('opens a folder field in its folder', async () => {
    const { p, open } = harness({ dirs: ['D:\\Demos'] });
    await p.pick({ field: 'auditor.target_folder', current: 'D:\\Demos', directory: true });
    expect(open.mock.calls[0][0].defaultPath).toBe('D:\\Demos');
  });

  it('a missing value starts in the folder this field last picked from', async () => {
    const { p, open } = harness({ dirs: ['D:\\Clips', 'C:\\Users\\me\\Videos'] });
    p.setFolders({ 'combine.add_files': 'D:\\Clips', 'queue.add_files': 'E:\\Elsewhere' });
    await p.pick({ field: 'combine.add_files', current: '', multiple: true });
    expect(open.mock.calls[0][0].defaultPath).toBe('D:\\Clips');
  });

  it('nothing at all: the field default', async () => {
    const hl = 'C:\\Steam\\steamapps\\common\\Half-Life\\hl.exe';
    const { p, open } = harness({
      dirs: ['C:\\Steam\\steamapps\\common', 'C:\\Steam\\steamapps\\common\\Half-Life\\dod', 'C:\\Users\\me\\Videos'],
      context: { hlPath: hl, gameFolder: 'dod' },
    });
    await p.pick({ field: 'queue.add_folder', directory: true });
    await p.pick({ field: 'capture.output_folder', directory: true });
    await p.pick({ field: 'config.hl', current: '' });
    expect(open.mock.calls.map(([o]) => o.defaultPath)).toEqual([
      'C:\\Steam\\steamapps\\common\\Half-Life\\dod',
      'C:\\Users\\me\\Videos',
      'C:\\Steam\\steamapps\\common',
    ]);
  });

  it('no default that exists: no defaultPath, so Windows chooses', async () => {
    const { p, open } = harness({ dirs: [] });
    await p.pick({ field: 'hd.python', title: 'Python' });
    expect(open).toHaveBeenCalledWith({ title: 'Python' });
  });

  it('a fallback the call site gives goes ahead of the field default', async () => {
    const { p, open } = harness({ dirs: ['C:\\Users\\me\\Documents', 'C:\\Users\\me\\Documents\\dod-studio\\projects'] });
    await p.pick({ field: 'project.open', fallback: 'C:\\Users\\me\\Documents\\dod-studio\\projects' });
    expect(open.mock.calls[0][0].defaultPath).toBe('C:\\Users\\me\\Documents\\dod-studio\\projects');
  });

  it('a successful pick updates the remembered folder and saves it', async () => {
    const { p, persist } = harness({ dirs: [], picked: 'F:\\Downloads\\ffmpeg\\bin\\ffmpeg.exe' });
    p.setFolders({ 'config.hlae': 'C:\\HLAE' });
    await expect(p.pick({ field: 'config.ffmpeg' })).resolves.toBe('F:\\Downloads\\ffmpeg\\bin\\ffmpeg.exe');
    expect(p.getFolders()).toEqual({ 'config.hlae': 'C:\\HLAE', 'config.ffmpeg': 'F:\\Downloads\\ffmpeg\\bin' });
    expect(persist).toHaveBeenCalledTimes(1);
  });

  it('a cancelled pick or the same folder again saves nothing', async () => {
    const { p, persist, open } = harness({ dirs: [] });
    await p.pick({ field: 'config.ffmpeg' });
    p.setFolders({ 'queue.add_files': 'D:\\Demos' });
    open.mockResolvedValueOnce(['D:\\Demos\\a.dem']);
    await p.pick({ field: 'queue.add_files', multiple: true });
    expect(persist).not.toHaveBeenCalled();
  });

  it('a backend that fails still opens the dialog', async () => {
    const open = vi.fn(async () => null);
    const p = createPickPath({ open, pathStates: async () => { throw new Error('gone'); }, userFolders: async () => null });
    const err = vi.spyOn(console, 'error').mockImplementation(() => {});
    await p.pick({ field: 'config.hl', current: 'C:\\HL\\hl.exe', title: 'hl' });
    expect(open).toHaveBeenCalledWith({ title: 'hl' });
    err.mockRestore();
  });

  it('setFolders keeps only folder strings', () => {
    const { p } = harness();
    p.setFolders({ a: 'C:\\A', b: 3, c: '' });
    expect(p.getFolders()).toEqual({ a: 'C:\\A' });
    p.setFolders(null);
    expect(p.getFolders()).toEqual({});
  });
});
