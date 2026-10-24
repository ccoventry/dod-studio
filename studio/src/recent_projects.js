// recent_projects.js
// File → Open Recent (#935): the last project files opened or saved, newest
// first, kept in the settings file as `recent_projects`. Opening one goes
// through project_session.js's openProject, the same path as Load Session
// (unsaved-changes prompt, then the missing-demo check and rescan offer).

import { filesExist } from './ipc_bridge.js';
import { themedConfirm } from './themed_confirm.js';
import { fileNameOf, folderOf, shortFolder, samePath } from './path_display.js';
import { STRINGS } from './strings.js';

export const RECENT_PROJECTS_CAP = 10;

/** `list` with `path` at the top, listed once, at most `cap` long. */
export function addRecent(list, path, cap = RECENT_PROJECTS_CAP) {
  if (!path) return [...(list || [])];
  return [path, ...(list || []).filter((p) => !samePath(p, path))].slice(0, cap);
}

/** `list` without `path` (either slash, any case). */
export function removeRecent(list, path) {
  return (list || []).filter((p) => !samePath(p, path));
}

/** A settings file's `recent_projects` made safe to use: strings only, no
 *  repeats, capped. An older file has none. */
export function normalizeRecent(raw, cap = RECENT_PROJECTS_CAP) {
  if (!Array.isArray(raw)) return [];
  const out = [];
  raw.forEach((p) => {
    if (typeof p === 'string' && p && !out.some((q) => samePath(q, p))) out.push(p);
  });
  return out.slice(0, cap);
}

/**
 * @param {object} app  What this needs from main.js: openProject(path) from
 *   project_session.js, which asks about unsaved changes first and resolves
 *   whether it opened, and persistAppSettings(), which reads list() back
 *   through settings_sync.js's getState().
 *   `exists(paths)` resolves one boolean per path (the files_exist command);
 *   `confirm` is themedConfirm. Both are swappable for tests.
 */
export function createRecentProjects({
  openProject,
  persistAppSettings,
  exists = filesExist,
  confirm = themedConfirm,
}) {
  let recent = [];
  // Paths last found missing, so the menu can dim them; refreshed every time
  // the File menu opens.
  let missing = new Set();
  const panel = document.querySelector('#recent-projects-panel');
  const row = document.querySelector('#recent-projects-menu');

  function render() {
    if (!panel) return;
    panel.replaceChildren();
    if (recent.length === 0) {
      const empty = document.createElement('button');
      empty.className = 'menu-dropdown-item';
      empty.disabled = true;
      empty.textContent = STRINGS.NAV.OPEN_RECENT_EMPTY;
      panel.appendChild(empty);
    }
    recent.forEach((path) => {
      const item = document.createElement('button');
      item.className = 'menu-dropdown-item recent-project-item';
      item.dataset.path = path;
      item.title = path;
      const isMissing = missing.has(path);
      item.classList.toggle('missing', isMissing);
      const name = document.createElement('span');
      name.className = 'recent-project-name';
      name.textContent = fileNameOf(path) + (isMissing ? ` ${STRINGS.NAV.OPEN_RECENT_MISSING}` : '');
      const folder = document.createElement('span');
      folder.className = 'recent-project-folder';
      folder.textContent = shortFolder(folderOf(path));
      item.append(name, folder);
      item.addEventListener('click', () => openRecent(path));
      panel.appendChild(item);
    });
    const sep = document.createElement('div');
    sep.className = 'menu-separator';
    panel.appendChild(sep);
    const clear = document.createElement('button');
    clear.className = 'menu-dropdown-item';
    clear.id = 'clear-recent-projects-btn';
    clear.textContent = STRINGS.NAV.CLEAR_RECENT;
    clear.disabled = recent.length === 0;
    clear.title = recent.length === 0 ? STRINGS.NAV.CLEAR_RECENT_EMPTY_TITLE : '';
    clear.addEventListener('click', () => clearRecent());
    panel.appendChild(clear);
  }

  /** Looks up which entries are gone and dims them. */
  async function refreshMissing() {
    if (recent.length === 0) return;
    const paths = [...recent];
    const found = await exists(paths);
    if (!Array.isArray(found)) return;
    missing = new Set(paths.filter((_, i) => found[i] === false));
    render();
  }

  async function commit(next) {
    recent = next;
    render();
    await persistAppSettings();
  }

  /** Puts `path` at the top: called whenever a project is opened or saved. */
  async function remember(path) {
    missing.delete(path);
    await commit(addRecent(recent, path));
  }

  async function forget(path) {
    missing.delete(path);
    await commit(removeRecent(recent, path));
  }

  /** A project renamed on disk (switched to .dodstudio, #528): its entry
   *  follows the new name, at the top. */
  async function rename(oldPath, newPath) {
    missing.delete(oldPath);
    await commit(addRecent(removeRecent(recent, oldPath), newPath));
  }

  async function clearRecent() {
    missing = new Set();
    await commit([]);
  }

  // A file that's gone offers to drop it, before the unsaved-changes prompt:
  // there's nothing to open, so nothing to save first for.
  async function openRecent(path) {
    const found = await exists([path]);
    if (Array.isArray(found) && found[0] === false) {
      missing.add(path);
      render();
      const remove = await confirm(STRINGS.NAV.RECENT_MISSING_MESSAGE, {
        title: STRINGS.NAV.RECENT_MISSING_TITLE,
        details: [{ primary: fileNameOf(path), secondary: folderOf(path), title: path }],
        confirmLabel: STRINGS.NAV.RECENT_MISSING_REMOVE,
        cancelLabel: STRINGS.NAV.RECENT_MISSING_KEEP,
      });
      if (remove) await forget(path);
      return false;
    }
    return openProject(path);
  }

  // The submenu's own row isn't an item: clicking it keeps the File menu open
  // (app_menu.js closes it on any other click inside the panel). The event's
  // path, not panel.contains(): Clear Recent re-renders the list, taking the
  // clicked button out of the page before the click gets here.
  row?.addEventListener('click', (e) => {
    if (!e.composedPath().includes(panel)) e.stopPropagation();
  });
  document.querySelector('#file-menu-btn')?.addEventListener('click', () => {
    refreshMissing();
  });

  render();

  return {
    /** The list as saved: settings_sync.js reads it through getState(). */
    list: () => [...recent],
    /** The saved list from the settings file, at startup. */
    load(raw) {
      recent = normalizeRecent(raw);
      missing = new Set();
      render();
    },
    remember,
    forget,
    rename,
    clearRecent,
    openRecent,
  };
}
