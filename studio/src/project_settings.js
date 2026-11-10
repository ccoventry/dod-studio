// project_settings.js
// The open project's own settings (#1085, build plan step 4): its `settings`
// block, kept here while the project is open, and what every reader asks for
// the value a capture, launch or render uses (effectiveSettings). The rules
// are project_settings_logic.js's; this holds the state.
//
// The Project settings page (project_settings_page.js) changes them, through
// blockForPage and applyBlockFromPage. Configuration still does too, until
// it shows a read-only "This project sets" line (build plan): an edit there
// is saved as the app's value, as always, and copied into the open project
// too (noteFormSaved). The project's values themselves never go into the
// form, which would make them the app's.
//
// One project is open per window, so the block lives at module level, like
// clip_name_ui.js's template; project_session.js opens, saves and clears it
// through createProjectSettings.

import { logFrontendEvent } from './ipc_bridge.js';
import {
  resolveSettings, blockFromForm, followFormEdits, keysDifferingFromForm, madeWithOf,
} from './project_settings_logic.js';
import { projectSettingsCopiedLine, projectSettingsDifferLine, projectSettingsFollowedLine } from './project_log.js';

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

// Told when the open project's block is replaced from outside the Project
// settings page (a project opened, New Session), so the page drops edits
// made against the block that was there.
let onReplaced = () => {};

/** Registers `listener()`, called when a project opens or closes. One
 *  listener: the Project settings page. */
export function onProjectBlockReplaced(listener) {
  onReplaced = listener;
}

// The Project settings page's leave(), registered when the page is made.
let leavePage = async () => true;

/** Registers the Project settings page's `leave(how)`. */
export function setProjectSettingsPageLeave(leave) {
  leavePage = leave;
}

/**
 * Asks the Project settings page to close before something leaves it
 * (closing the window, opening a project, New Session). With pending edits
 * it asks Apply, Discard or Stay. Resolves true when the page is closed (or
 * was never open), false when the user chose Stay. Lives here rather than in
 * the page's module so the session and the close check need not load the
 * page.
 */
export function leaveProjectSettingsPage(how) {
  return leavePage(how);
}

/**
 * What the Project settings page edits: `form` the settings file's values
 * now (null before settings load), `block` the open project's block, or one
 * made from `form` for a session not saved yet, and `pcId`.
 */
export function blockForPage() {
  const form = formNow();
  const base = block || (form ? blockFromForm(form, { pcId, madeWith: madeWithOf(form.hl_path) }) : null);
  return { form, block: base, pcId };
}

/**
 * Makes `next` the open project's block (the Project settings page's Apply)
 * and marks the project unsaved. A session not saved yet keeps it and
 * writes it with its first save.
 */
export function applyBlockFromPage(next) {
  block = next;
  markDirty();
}

/**
 * The open session's side: `markDirty()` marks the project unsaved.
 * @returns {{
 *   open: (raw: object|undefined, path: string) => { copied: boolean },
 *   toSave: () => object,
 *   clear: () => void,
 *   block: () => object|null,
 *   setBlock: (next: object) => void,
 * }}
 *   `open` takes a loaded file's `settings`: a file without one (made before
 *   #1085) gets today's Configuration copied in once (decision 6), `copied`
 *   then true; it is written with the next save. `toSave` is the block to
 *   write, made from the form for a project saved the first time. `clear`
 *   is New Session.
 */
export function createProjectSettings({ markDirty: mark }) {
  markDirty = mark;

  function open(raw, path) {
    onReplaced();
    const form = formNow();
    formAtLastLook = form;
    if (raw && typeof raw === 'object') {
      block = raw;
      const differs = form ? keysDifferingFromForm(form, block, pcId) : [];
      if (differs.length > 0) logFrontendEvent(projectSettingsDifferLine(path, differs));
      return { copied: false };
    }
    block = form ? blockFromForm(form, { pcId, madeWith: madeWithOf(form.hl_path) }) : null;
    if (block) logFrontendEvent(projectSettingsCopiedLine(path));
    return { copied: !!block };
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
    onReplaced();
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
