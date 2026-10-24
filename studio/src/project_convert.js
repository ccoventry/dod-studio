// project_convert.js
// After a `.json` project loads (#528), offers once to switch it to the
// `.dodstudio` file type, so it opens in DoD Studio when double-clicked.
//
// - Switch renames `<name>.json` to `<name>.dodstudio` in the same folder
//   (project_convert.rs): no copy is left behind for Open Recent to list.
//   project_session.js then points the session and Open Recent at the new
//   name, and saves there when the project has unsaved changes.
// - Keep as .json remembers that path in the settings file
//   (`kept_json_projects`), so the same project never asks again.
// - Never offered for a `.autosave.json`, a file Studio may not rename (a
//   read-only file, a folder it can't write to), or when `<name>.dodstudio`
//   is already there: the backend answers no offer.

import { projectConvertOffer, convertProjectFile } from './ipc_bridge.js';
import { themedConfirm } from './themed_confirm.js';
import { fileNameOf, folderOf, samePath } from './path_display.js';
import { showToast } from './toast.js';
import { STRINGS } from './strings.js';

/** At most this many "Keep as .json" answers are remembered, newest first. */
export const KEPT_JSON_CAP = 200;

let kept = [];

/** The settings file's `kept_json_projects`, as saved. */
export function getKeptJsonProjects() {
  return [...kept];
}

/** Loads the saved list: strings only, no repeats, capped. */
export function setKeptJsonProjects(raw) {
  kept = [];
  (Array.isArray(raw) ? raw : []).forEach((p) => {
    if (typeof p === 'string' && p && !kept.some((q) => samePath(q, p))) kept.push(p);
  });
  kept = kept.slice(0, KEPT_JSON_CAP);
}

/** A `.json` project the offer could be about (not a `.autosave.json`). */
export function isConvertibleProject(path) {
  const name = (fileNameOf(path) || '').toLowerCase();
  return name.endsWith('.json') && !name.endsWith('.autosave.json') && name !== '.json';
}

/**
 * @param {object} deps  persist() saves the settings file; offer(path),
 *   convert(path), confirm and toast are the backend, the themed prompt
 *   and showToast, swappable for tests.
 * @returns {{ offerConvert(path): Promise<string|null> }} the new path when
 *   the project was switched, else null.
 */
export function createProjectConvert({
  persist = async () => {},
  offer = projectConvertOffer,
  convert = convertProjectFile,
  confirm = themedConfirm,
  toast = showToast,
} = {}) {
  const S = () => STRINGS.PROJECT_CONVERT;

  async function offerConvert(path) {
    if (!isConvertibleProject(path) || kept.some((p) => samePath(p, path))) return null;
    const target = await offer(path);
    if (!target) return null;
    const yes = await confirm(S().MESSAGE, {
      title: S().TITLE,
      details: [{ primary: `${fileNameOf(path)} → ${fileNameOf(target)}`, secondary: folderOf(path), title: path }],
      confirmLabel: S().SWITCH,
      cancelLabel: S().KEEP,
    });
    if (!yes) {
      kept = [path, ...kept.filter((p) => !samePath(p, path))].slice(0, KEPT_JSON_CAP);
      try {
        await persist();
      } catch (err) {
        console.error('Saving the Keep as .json answer failed:', err);
      }
      return null;
    }
    try {
      const renamed = await convert(path);
      toast(S().switchedToast(fileNameOf(renamed)), 'success');
      return renamed;
    } catch (err) {
      toast(S().failedToast(err), 'error');
      return null;
    }
  }

  return { offerConvert };
}
