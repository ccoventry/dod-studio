// project_settings_snapshot.js
// The one snapshot of Configuration every project made before #1085 starts
// from (build plan decision 6). It is taken at the first start of a build
// with project settings, from Configuration as it is then, and kept for good
// (project_settings_snapshot.rs, a file of its own beside settings.json).
// An older project opened later takes this snapshot, not Configuration as it
// is that day, so setting Configuration up for another movie never reaches a
// project that wasn't saved in this build yet, and the order older projects
// are opened in doesn't matter. Once saved, a project owns its settings.
//
// Shape: `{ schema, takenAt, appVersion, settings, folders }`. `settings` is a
// project `settings` block without `folders`; `folders` is this PC's capture
// and export folders, `{ destinations, exports }`, unkeyed, because the file
// itself belongs to this PC. A project taking the snapshot gets them under
// this PC's id (blockFromSnapshot), as #1085's per-PC rule wants, so they are
// never filed under an id a later settings.json might not have.

import { keepProjectSettingsSnapshot, getAppVersion, logFrontendEvent } from './ipc_bridge.js';
import { blockFromForm, madeWithOf, PC_FIELDS } from './project_settings_logic.js';
import { projectSnapshotFailedLine } from './project_log.js';

const copyOf = (value) => JSON.parse(JSON.stringify(value));
const textsOrNone = (v) => (Array.isArray(v) ? v.filter((s) => typeof s === 'string') : []);

// project_migrations.js is imported where it is used, not here: it reaches
// the take and render modules, and every module that reads a setting in
// effect imports this one through project_settings.js.
const loadMigrations = () => import('./project_migrations.js');

/**
 * The snapshot of `form` (the settings file's values, as persistAppSettings
 * would save them), stamped with `takenAt` (an ISO time), `appVersion` and
 * `schema`, the project schema its block follows (CURRENT_SCHEMA).
 */
export function snapshotFromForm(form, { takenAt, appVersion = null, schema = null }) {
  // No PC id: blockFromForm then leaves the folders out of the block.
  const settings = blockFromForm(form, { pcId: '', madeWith: madeWithOf(form.hl_path) });
  const folders = Object.fromEntries(PC_FIELDS.map((f) => [f.name, textsOrNone(form[f.key])]));
  return { schema, takenAt, appVersion, settings, folders };
}

/**
 * `raw` (the kept file) as a snapshot this build can use, or null for one it
 * can't. A snapshot from an older schema goes through the same steps a
 * project file of that schema does (project_migrations.js), so it and a
 * project saved the day it was taken stay alike. It is migrated, never taken
 * again: a new one would hold Configuration as it is now, which is what the
 * snapshot is there to avoid. A newer one is used as it is: values this build
 * can't use fall back to Configuration's (resolveSettings), and fields it
 * doesn't know are kept in the projects that take it. `migrations` is
 * project_migrations.js (`CURRENT_SCHEMA`, `migrateProject`).
 */
export function usableSnapshot(raw, { CURRENT_SCHEMA, migrateProject }) {
  if (!raw || typeof raw !== 'object' || !raw.settings || typeof raw.settings !== 'object') return null;
  const schema = Number.isInteger(raw.schema) ? raw.schema : CURRENT_SCHEMA;
  const data = { settings: copyOf(raw.settings) };
  try {
    migrateProject(data, schema);
  } catch {
    return null;
  }
  return { ...raw, schema: Math.max(schema, CURRENT_SCHEMA), settings: data.settings };
}

/** The `settings` block an older project takes from `snapshot`, its own
 *  copy, with the snapshot's folders as this PC's (`pcId`). */
export function blockFromSnapshot(snapshot, pcId) {
  const block = copyOf(snapshot.settings);
  delete block.folders;
  if (pcId) {
    const folders = Object.fromEntries(PC_FIELDS.map((f) => [f.name, textsOrNone(snapshot.folders?.[f.name])]));
    block.folders = { [pcId]: folders };
  }
  return block;
}

/** The day `takenAt` names, for the notice: "November 11, 2026". */
export function snapshotDay(takenAt) {
  const d = new Date(takenAt);
  if (Number.isNaN(d.getTime())) return String(takenAt || '');
  return d.toLocaleDateString('en-US', { month: 'long', day: 'numeric', year: 'numeric' });
}

// The snapshot in effect, as a promise of a usable snapshot or null, once
// startSnapshot has run.
let inEffect = null;

/**
 * Reads the form now (`readForm()`, settings_sync.js's reader) and keeps it
 * as the snapshot unless one was taken before; the backend answers with the
 * one in effect, which an older project then takes. main.js calls this once,
 * right after the settings are loaded into the form, before anything can open
 * a project. The form is read before any await, so a later edit can't get in.
 * A failure is logged; older projects then take Configuration as it is when
 * they open, as before.
 */
export function startSnapshot(readForm, { keep = keepProjectSettingsSnapshot, now = () => new Date() } = {}) {
  const form = readForm();
  if (!form) {
    inEffect = Promise.resolve(null);
    return inEffect;
  }
  const candidate = snapshotFromForm(form, { takenAt: now().toISOString() });
  inEffect = (async () => {
    const migrations = await loadMigrations();
    candidate.schema = migrations.CURRENT_SCHEMA;
    candidate.appVersion = await getAppVersion();
    const kept = await keep(candidate);
    const snapshot = usableSnapshot(kept?.snapshot, migrations);
    if (!snapshot) throw new Error('the kept snapshot has no usable settings block');
    return snapshot;
  })().catch((err) => {
    logFrontendEvent(projectSnapshotFailedLine(err));
    return null;
  });
  return inEffect;
}

/** The snapshot an older project takes: null when none was started (no
 *  settings loaded) or it failed. Waits for one still being kept. */
export function snapshotInEffect() {
  return inEffect || Promise.resolve(null);
}

/** Forgets the snapshot in effect, for tests. */
export function resetSnapshotForTests() {
  inEffect = null;
}
