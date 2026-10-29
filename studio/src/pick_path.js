// pick_path.js — every Browse button's file/folder dialog (#936). Windows
// remembers one last folder per app, shared by every picker, so without a
// start folder picking FFmpeg opened wherever the last demo was picked. Each
// dialog now starts, in order:
//   1. at the field's own path: a file's folder with the file selected (the
//      dialog plugin splits a file path into folder + file name), a folder
//      itself;
//   2. else in the folder this same field last picked from (settings'
//      `pick_folders`, keyed by the field id below);
//   3. else, when the field's path or remembered folder has moved, in its
//      nearest folder that still exists (never a bare drive root);
//   4. else in the field's default (PICK_FIELDS);
//   5. else wherever Windows puts it.
// Call sites keep their own `open()` options: `pickPath({ field, current,
// ...options })` takes the same options and returns what `open()` returns.
//
// One dialog at a time, app-wide: a pick asked for while another is being
// prepared or is open returns null without opening, as on Cancel. Getting a
// dialog up can take seconds, and every Ctrl+O or Browse click in that wait
// used to queue its own dialog, all appearing at once. dialog_wait.js shows
// the wait from the click until the dialog returns; a `fallback` the caller
// still has to ask the backend for is passed as a function, so that wait
// covers it too.

import { open as dialogOpen } from '@tauri-apps/plugin-dialog';
import { pickPathStates, pickUserFolders } from './ipc_bridge.js';
import { setDialogWait } from './dialog_wait.js';

/** Steam's usual library, for the game pickers before hl.exe is set. */
export const STEAM_COMMON = 'C:\\Program Files (x86)\\Steam\\steamapps\\common';

/**
 * Each Browse button's id and the default it starts in when it has no path
 * and has never picked: `steam` Steam's steamapps\common (the install's
 * parent once hl.exe is set), `game` the folder hl.exe is in, `demos` the
 * game folder beside hl.exe (dod\), `videos` / `documents` / `programs` the
 * user's folders and Program Files, null Windows' own choice. Two buttons
 * that fill the same setting (Configuration's and Setup's hl.exe) share an
 * id, so they share a remembered folder.
 */
export const PICK_FIELDS = {
  'config.hl': 'steam',
  'config.hlae': 'programs',
  'config.ffmpeg': 'programs',
  'config.obs': 'programs',
  'config.hook_dll': 'documents',
  'capture.import_cfg': 'demos',
  'capture.output_folder': 'videos',
  'render.export_folder': 'videos',
  'combine.add_files': 'videos',
  'queue.add_files': 'demos',
  'queue.add_folder': 'demos',
  'project.open': 'documents',
  'project.locate_demo': 'demos',
  'analyzer.demo': 'demos',
  'obs_test.demo': 'demos',
  'analyzer.pin_folder': 'demos',
  'auditor.target_folder': 'demos',
  'auditor.add_folder': 'demos',
  'auditor.keep_folder': 'demos',
  'auditor.consolidate_folder': 'demos',
  'demo_links.folder': 'documents',
  'players.crosshair': 'documents',
  'blender.exe': 'programs',
  'blender.agr': 'videos',
  'blender.assets': 'documents',
  'hd.python': null,
  'hd.upscaler': 'documents',
};

const SEP = /[\\/]/;

/** The folder a path is in; a drive root keeps its backslash (`C:\`). */
export function parentOf(path) {
  const trimmed = String(path || '').replace(/[\\/]+$/, '');
  const cut = Math.max(trimmed.lastIndexOf('\\'), trimmed.lastIndexOf('/'));
  if (cut < 0) return '';
  const parent = trimmed.slice(0, cut);
  return /^[A-Za-z]:$/.test(parent) ? `${parent}\\` : parent;
}

function isDriveRoot(path) {
  return /^[A-Za-z]:[\\/]?$/.test(String(path || ''));
}

function join(dir, name) {
  return SEP.test(dir.slice(-1)) ? `${dir}${name}` : `${dir}\\${name}`;
}

/**
 * The default's candidate paths, best first (the first that exists wins).
 * @param {string|null} kind  a PICK_FIELDS value
 * @param {object} ctx  { hlPath, gameFolder, userFolders: { videos, documents, program_files } }
 */
export function defaultCandidates(kind, { hlPath = '', gameFolder = '', userFolders = {} } = {}) {
  const install = hlPath ? parentOf(hlPath) : '';
  const user = userFolders || {};
  switch (kind) {
    case 'steam': return [install && parentOf(install), STEAM_COMMON].filter(Boolean);
    case 'game': return [install].filter(Boolean);
    case 'demos': return install ? [join(install, gameFolder || 'dod'), install] : [];
    case 'videos': return [user.videos, user.documents].filter(Boolean);
    case 'documents': return [user.documents].filter(Boolean);
    case 'programs': return [user.program_files].filter(Boolean);
    default: return [];
  }
}

/**
 * Where the dialog starts, from the candidates' states on disk (pure).
 * @param {object} args
 *   directory  the dialog picks folders;
 *   states  pick_path_states' answer for [current, remembered, ...defaults],
 *     each { path, is_file, exact, is_root } or null.
 * @returns {string|undefined}  the dialog's defaultPath
 */
export function chooseStartPath({ directory = false, states = [] }) {
  const [current, remembered, ...defaults] = states;
  if (current?.exact) {
    if (!current.is_file) return current.path;
    return directory ? parentOf(current.path) : current.path;
  }
  if (remembered?.exact && !remembered.is_file) return remembered.path;
  if (current && !current.is_root) return current.path;
  if (remembered && !remembered.is_root) return remembered.path;
  const start = defaults.find((d) => d?.exact && !d.is_file);
  return start?.path;
}

/** The folder a pick was made from: a file's folder, a folder's parent (a
 *  drive root itself), so the next pick starts beside the last one. */
export function folderToRemember(picked) {
  const first = Array.isArray(picked) ? picked[0] : picked;
  if (!first || typeof first !== 'string') return null;
  if (isDriveRoot(first)) return first;
  return parentOf(first) || null;
}

/**
 * @param {object} deps
 *   open(options) the dialog; pathStates(paths) and userFolders() the
 *   backend's answers; getContext() { hlPath, gameFolder }; persist() saves
 *   settings after a remembered folder changes; setBusy(on) shows the wait
 *   (dialog_wait.js).
 */
export function createPickPath({
  // Looked up at call time, so a test mocking ipc_bridge.js without these
  // still loads every module that imports this one.
  open = (options) => dialogOpen(options),
  pathStates = (paths) => pickPathStates(paths),
  userFolders = () => pickUserFolders(),
  getContext = () => ({}),
  persist = async () => {},
  setBusy = setDialogWait,
} = {}) {
  const deps = { getContext, persist };
  let folders = {};
  let picking = false;
  let userFoldersPromise = null;

  function loadUserFolders() {
    if (!userFoldersPromise) {
      userFoldersPromise = Promise.resolve(userFolders()).then((f) => {
        // Asked again next time when the backend gave nothing.
        if (!f) userFoldersPromise = null;
        return f || {};
      }, () => { userFoldersPromise = null; return {}; });
    }
    return userFoldersPromise;
  }

  async function startPathFor({ field, current, directory, fallback }) {
    const kind = PICK_FIELDS[field] ?? null;
    const ctx = { ...(deps.getContext() || {}), userFolders: await loadUserFolders() };
    const first = typeof fallback === 'function' ? await fallback() : fallback;
    const defaults = [first, ...defaultCandidates(kind, ctx)].filter(Boolean);
    const paths = [current || '', (field && folders[field]) || '', ...defaults];
    const states = await pathStates(paths);
    return chooseStartPath({ directory, states: Array.isArray(states) ? states : [] });
  }

  async function remember(field, picked) {
    const folder = folderToRemember(picked);
    if (!field || !folder || folders[field] === folder) return;
    folders = { ...folders, [field]: folder };
    try {
      await deps.persist();
    } catch (err) {
      console.error('Saving the Browse folder failed:', err);
    }
  }

  /**
   * `open(options)` started in the right folder.
   * @param {object} options  `field` the PICK_FIELDS id, `current` the
   *   field's path now, `fallback` a default ahead of the field's own (or
   *   an async function returning it), and
   *   the dialog's options as they were. Null without opening while
   *   another pick is under way.
   */
  async function pick({ field, current, fallback, ...options }) {
    if (picking) return null;
    picking = true;
    setBusy(true);
    try {
      let defaultPath;
      try {
        defaultPath = await startPathFor({ field, current, directory: !!options.directory, fallback });
      } catch (err) {
        console.error('Choosing where the dialog starts failed:', err);
      }
      const picked = await open(defaultPath ? { ...options, defaultPath } : options);
      if (picked) await remember(field, picked);
      return picked;
    } finally {
      picking = false;
      setBusy(false);
    }
  }

  return {
    pick,
    isPicking: () => picking,
    startPathFor,
    getFolders: () => ({ ...folders }),
    setFolders: (map) => {
      folders = map && typeof map === 'object' && !Array.isArray(map)
        ? Object.fromEntries(Object.entries(map).filter(([, v]) => typeof v === 'string' && v))
        : {};
    },
    configure: ({ getContext: g, persist: p } = {}) => {
      if (g) deps.getContext = g;
      if (p) deps.persist = p;
    },
  };
}

// The app's one instance: the call sites use these, main.js configures it
// and settings_sync.js loads and saves its folders.
const shared = createPickPath();
export const pickPath = (options) => shared.pick(options);
export const configurePickPath = (deps) => shared.configure(deps);
export const getPickFolders = () => shared.getFolders();
export const setPickFolders = (map) => shared.setFolders(map);
