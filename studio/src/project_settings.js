// project_settings.js
// The open project's own settings (#1085, build plan step 4): its `settings`
// block, kept here while the project is open, and what every reader asks for
// the value a capture, launch or render uses (effectiveSettings). The rules
// are project_settings_logic.js's; this holds the state.
//
// Until the Project settings page exists (plan step 6), Configuration still
// changes a project's settings: an edit there is saved as the app's value, as
// always, and copied into the open project too (noteFormSaved). The project's
// values themselves never go into the form, which would make them the app's.
//
// One project is open per window, so the block lives at module level, like
// clip_name_ui.js's template; project_session.js opens, saves and clears it
// through createProjectSettings.

import { logFrontendEvent } from './ipc_bridge.js';
import {
  resolveSettings, blockFromForm, followFormEdits, keysDifferingFromForm, madeWithOf,
} from './project_settings_logic.js';
import {
  projectSettingsCopiedLine, projectSettingsDifferLine, projectSettingsFollowedLine, projectSettingsFromSnapshotLine,
} from './project_log.js';
import { snapshotInEffect, blockFromSnapshot, startSnapshot } from './project_settings_snapshot.js';

// The open project's block, or null when no project is open or a new one
// hasn't been saved yet: everything then resolves to the form, as before #1085.
let block = null;
// This PC's id (settings.json's `pc_id`), the key of the block's folders.
let pcId = '';
// The settings payload the form last held, to tell what an edit changed.
let formAtLastLook = null;
// settings_sync.js's reader of the form, as the settings file's values.
let readForm = null;
// The open session's markProjectDirty.
let markDirty = () => {};

/** Generates an id for a PC that has none. */
const newPcId = () => globalThis.crypto?.randomUUID?.() || `pc-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;

/**
 * Takes this PC's id from the loaded settings file, or makes one for a file
 * that has none. Returns true when it made one: the caller saves the settings
 * then, so the id is the same next time.
 */
export function loadPcId(saved) {
  const id = typeof saved === 'string' ? saved.trim() : '';
  pcId = id || newPcId();
  return !id;
}

/** This PC's id, for the settings file. */
export function getPcId() {
  return pcId;
}

/** settings_sync.js hands over its reader of the form: a function returning
 *  the settings payload persistAppSettings would save. */
export function setFormReader(reader) {
  readForm = reader;
}

/** `form` (settings-file keys) with the open project's values over it: what
 *  a capture, a launch or a render uses. See resolveSettings. */
export function effectiveSettings(form) {
  return resolveSettings(form, block, pcId);
}

/** The value in effect for one setting whose form value is `formValue`. */
export function effectiveSetting(key, formValue) {
  return effectiveSettings({ [key]: formValue })[key];
}

/** A getter of the value in effect, from `getFormValue`, a getter of the
 *  form's: for the folder lists main.js hands to the Capture and Render tabs. */
export function inEffect(key, getFormValue) {
  return () => effectiveSetting(key, getFormValue());
}

/**
 * `request` (a launch request, or null) with the open project's window size
 * in, so Launch Game, Launch Preview, Review and the running-game check start
 * or keep the game at the size a batch would use. Unchanged with no project
 * block; the backend then takes the saved settings' size, as before.
 */
export function withProjectLaunchSize(request) {
  if (!block) return request;
  const size = effectiveSettings({ resolution_width: undefined, resolution_height: undefined });
  const sized = { ...(request || {}) };
  if (size.resolution_width !== undefined && sized.resolution_width == null) sized.resolution_width = size.resolution_width;
  if (size.resolution_height !== undefined && sized.resolution_height == null) sized.resolution_height = size.resolution_height;
  return sized;
}

/**
 * settings_sync.js calls this with each settings payload it saves. An owned
 * value that changed since the form was last read goes into the open
 * project too, which is then unsaved (followFormEdits).
 */
export function noteFormSaved(payload) {
  const before = formAtLastLook;
  formAtLastLook = payload;
  if (!block || !before) return;
  const { block: next, changed } = followFormEdits(block, before, payload, pcId);
  if (changed.length === 0) return;
  block = next;
  logFrontendEvent(projectSettingsFollowedLine(changed));
  markDirty();
}

/** The form now, as the settings file's values, or null before settings_sync
 *  has handed its reader over. */
function formNow() {
  return readForm ? readForm() : null;
}

/** Keeps the form as it is now as the snapshot older projects start from,
 *  unless one was taken before (project_settings_snapshot.js). main.js calls
 *  it once, as soon as the settings are in the form. */
export function startProjectSettingsSnapshot() {
  return startSnapshot(formNow);
}

/**
 * The open session's side: `markDirty()` marks the project unsaved.
 * @returns {{
 *   open: (raw: object|undefined, path: string) => Promise<{ copied: boolean, snapshotTakenAt: string|null }>,
 *   toSave: () => object,
 *   clear: () => void,
 *   block: () => object|null,
 *   setBlock: (next: object) => void,
 * }}
 *   `open` takes a loaded file's `settings`: a file without one (made before
 *   #1085) takes the one snapshot of Configuration every older project
 *   starts from (decision 6, project_settings_snapshot.js), `copied` then
 *   true and `snapshotTakenAt` its time; with no snapshot (it failed), it
 *   takes Configuration as it is now. It is written with the next save. `toSave` is the block to
 *   write, made from the form for a project saved the first time. `clear`
 *   is New Session.
 */
export function createProjectSettings({ markDirty: mark }) {
  markDirty = mark;

  async function open(raw, path) {
    // Waited for before anything else, so the form read below and the block
    // set here belong to the same moment.
    const snapshot = raw && typeof raw === 'object' ? null : await snapshotInEffect();
    const form = formNow();
    formAtLastLook = form;
    if (raw && typeof raw === 'object') {
      block = raw;
      logDiffering(form, path);
      return { copied: false, snapshotTakenAt: null };
    }
    if (snapshot) {
      block = blockFromSnapshot(snapshot, pcId);
      logFrontendEvent(projectSettingsFromSnapshotLine(path, snapshot));
      logDiffering(form, path);
      return { copied: true, snapshotTakenAt: snapshot.takenAt };
    }
    block = form ? blockFromForm(form, { pcId, madeWith: madeWithOf(form.hl_path) }) : null;
    if (block) logFrontendEvent(projectSettingsCopiedLine(path));
    return { copied: !!block, snapshotTakenAt: null };
  }

  /** Names, in the activity log, the settings the open block sets other
   *  than Configuration's: a capture uses the block's. */
  function logDiffering(form, path) {
    const differs = form ? keysDifferingFromForm(form, block, pcId) : [];
    if (differs.length > 0) logFrontendEvent(projectSettingsDifferLine(path, differs));
  }

  function toSave() {
    if (!block) {
      const form = formNow();
      formAtLastLook = form;
      block = form ? blockFromForm(form, { pcId, madeWith: madeWithOf(form.hl_path) }) : null;
    }
    return block || undefined;
  }

  function clear() {
    block = null;
    formAtLastLook = null;
  }

  return {
    open,
    toSave,
    clear,
    block: () => block,
    setBlock: (next) => { block = next; },
  };
}
