// project_file.js
// What every writer of a project file follows (#1085): the format's version
// number, what this build must hold on to for a newer build, and the backup
// before a save raises the format. project_session.js, batch_catch_up.js and
// rescan_recent.js (through the session) all read and write through here, so
// the three can't drift. Older builds can't be fixed, so the backup is the one
// cover they have.
//
// `schema` goes up one for each new kind of data; a file with none is schema 1.
// `saveNeeds` is the lowest build level that may save over the file: a lower
// one offers Save a copy instead. Nothing raises it yet. `version` stays
// "0.12.0": nothing reads it, and older builds expect it.

import { filesExist, readProjectFile, writeProjectFile, logFrontendEvent } from './ipc_bridge.js';
import { CURRENT_SCHEMA, migrateProject } from './project_migrations.js';
import { PROJECT_EXTENSION } from './project_file_type.js';
import { projectBackupStartedLine, projectBackedUpLine } from './project_log.js';
import { fileNameOf, folderOf } from './path_display.js';
import { STRINGS } from './strings.js';

/** This build's level: it may save over a file whose `saveNeeds` is this or lower. */
export const SAVE_LEVEL = 1;

const FILE_VERSION = '0.12.0';

/** Top-level keys this build reads and writes itself. */
const KNOWN_KEYS = new Set([
  'version', 'schema', 'saveNeeds', 'scanPaths', 'demos', 'takeIndex', 'takeCaptures',
  'takeRenders', 'teams', 'review', 'mode',
]);

/** Keys older builds wrote and this one drops on purpose: those paths are this
 *  PC's settings, and a project carrying them switched them on every open. */
const RETIRED_KEYS = new Set(['hlaePath', 'hlPath']);

/** Backups tried per file per day: `-2`, `-3`… after the first. A save stops
 *  when none is free, so this is far past what a day of moving a project
 *  between builds can use. */
const BACKUP_TRIES = 99;

/** A save or backup that didn't happen, with the sentence to show for it. */
export class ProjectFileError extends Error {}

/** A file's whole-number field, or `fallback` when it has none or a bad one. */
function wholeNumber(value, fallback) {
  return Number.isInteger(value) && value >= fallback ? value : fallback;
}

/** The schema of a parsed project file (no `schema` = 1). */
export function schemaOf(data) {
  return wholeNumber(data?.schema, 1);
}

/** The `saveNeeds` of a parsed project file (none = 1). */
export function saveNeedsOf(data) {
  return wholeNumber(data?.saveNeeds, 1);
}

/** The top-level keys of `data` this build doesn't know, to write back on save. */
function unknownKeysOf(data) {
  const entries = Object.entries(data || {}).filter(([key]) => !KNOWN_KEYS.has(key) && !RETIRED_KEYS.has(key));
  return Object.fromEntries(entries);
}

/**
 * A project file's text, migrated up to this build: `{ data, schema, saveNeeds,
 * unknownKeys }`, `schema` and `saveNeeds` being what the file said. Throws on
 * text that isn't JSON. `data` is null for a file holding just `null`.
 */
export function openProjectText(text) {
  const data = JSON.parse(text);
  const schema = schemaOf(data);
  if (data && typeof data === 'object') migrateProject(data, schema);
  return { data, schema, saveNeeds: saveNeedsOf(data), unknownKeys: unknownKeysOf(data) };
}

/** Whether `opened` (from openProjectText) came from a newer build's format. */
export function isNewerProject(opened) {
  return !!opened && opened.schema > CURRENT_SCHEMA;
}

/** Whether this build may save over the file `opened` came from. */
export function canSaveOver(opened) {
  return !opened || opened.saveNeeds <= SAVE_LEVEL;
}

/**
 * The text to write for a project: `fields`, the keys this build owns, plus
 * what `opened` (null for a project not opened from a file) held for a newer
 * build. A newer file keeps its own `schema` and `saveNeeds`: writing this
 * build's lower number would have the newer build run its steps a second time
 * over data that already has them. Returns `{ json, schema }`.
 */
export function buildProjectFile(fields, opened = null) {
  const schema = Math.max(opened?.schema ?? 1, CURRENT_SCHEMA);
  const saveNeeds = Math.max(opened?.saveNeeds ?? 1, 1);
  const stamp = { version: FILE_VERSION, schema, saveNeeds };
  // The stamp twice: first for its place at the top of the file, last so that
  // `fields` from an edited file (batch_catch_up.js) can't bring back old values.
  const json = JSON.stringify({ ...stamp, ...fields, ...opened?.unknownKeys, ...stamp }, null, 2);
  return { json, schema };
}

/** "2026-10-23", in local time. */
export function localDate(d = new Date()) {
  const pad = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** Where try `n` (1 first) of a project's backup goes: beside it, named
 *  `<name>.before-<label>-<date>.<extension>`, then `…-<date>-2.<extension>`
 *  and on, so a second backup the same day keeps the first. */
export function backupPath(path, { label, date, extension, n = 1 }) {
  const name = fileNameOf(path);
  const stem = name.replace(/\.(json|dodstudio)$/i, '');
  const folder = name === path ? '' : folderOf(path);
  const sep = path.includes('\\') ? '\\' : '/';
  const file = `${stem}.before-${label}-${date}${n > 1 ? `-${n}` : ''}.${extension}`;
  return folder ? `${folder}${sep}${file}` : file;
}

/** The first of `tries` that `taken` (one boolean per name, or null when the
 *  check failed) says is free. Null when none is, or when it can't tell: the
 *  caller then fails rather than overwrite an earlier backup, which may be
 *  the only copy of how the file was. */
export function freeBackupPath(tries, taken) {
  if (!Array.isArray(taken)) return null;
  return tries.find((_, k) => taken[k] === false) ?? null;
}

/**
 * Writes `contents` to the first free backup name for `path`, `name` being
 * backupPath's `{ label, extension }`, and resolves its path. Never overwrites
 * a file, and Studio never deletes one. Throws when no name is free or the
 * check fails.
 */
export async function writeProjectBackup(path, contents, name, { date, exists, write }) {
  const tries = Array.from({ length: BACKUP_TRIES }, (_, k) => backupPath(path, { ...name, date, n: k + 1 }));
  const target = freeBackupPath(tries, await exists(tries));
  if (!target) throw new Error(STRINGS.PROJECT_FILE.NO_BACKUP_NAME);
  await write(target, contents);
  return target;
}

const defaultIo = {
  exists: filesExist,
  read: readProjectFile,
  write: writeProjectFile,
  log: logFrontendEvent,
  now: Date.now,
};

/** What a file on disk says of itself. A file that won't parse reads as the
 *  oldest format, so it's backed up rather than lost under the new save. */
function headerOf(text) {
  try {
    const data = JSON.parse(text);
    return { schema: schemaOf(data), saveNeeds: saveNeedsOf(data) };
  } catch (_) {
    return { schema: 1, saveNeeds: 1 };
  }
}

/** Backs up the file at `path` when saving `schema` over it raises its format.
 *  Resolves `{ backup, fromSchema }`, or null when no backup was needed. */
async function backupBeforeRaising(path, schema, io) {
  const S = STRINGS.PROJECT_FILE;
  const startedAt = io.now();
  let onDisk;
  try {
    onDisk = await io.read(path);
  } catch (err) {
    throw new ProjectFileError(S.backupFailed(path, err));
  }
  const header = headerOf(onDisk);
  if (header.saveNeeds > SAVE_LEVEL) throw new ProjectFileError(S.needsNewerBuild(path));
  if (header.schema >= schema) return null;
  io.log(projectBackupStartedLine(path, header.schema, schema));
  try {
    const date = localDate(new Date(startedAt));
    const name = { label: `v${schema}`, extension: PROJECT_EXTENSION };
    const backup = await writeProjectBackup(path, onDisk, name, { ...io, date });
    io.log(projectBackedUpLine(backup, io.now() - startedAt));
    return { backup, fromSchema: header.schema };
  } catch (err) {
    throw new ProjectFileError(S.backupFailed(path, err?.message || err));
  }
}

/**
 * Writes `built` (from buildProjectFile) to `path`: the one way a project file
 * is saved. The file already there is backed up first when this raises its
 * `schema`, and left alone with an error when it needs a newer build to save
 * over it. Resolves `{ backup, fromSchema }` when it raised the file, else
 * null. Throws ProjectFileError when the file couldn't be checked or backed
 * up: nothing is written then. A failed write throws what the write threw.
 * `io` swaps the disk out in tests.
 */
export async function saveProjectFile(path, built, overrides = {}) {
  const io = { ...defaultIo, ...overrides };
  const found = await io.exists([path]);
  if (!Array.isArray(found)) throw new ProjectFileError(STRINGS.PROJECT_FILE.backupFailed(path, STRINGS.PROJECT_FILE.CANT_CHECK));
  const raised = found[0] ? await backupBeforeRaising(path, built.schema, io) : null;
  await io.write(path, built.json);
  return raised;
}
