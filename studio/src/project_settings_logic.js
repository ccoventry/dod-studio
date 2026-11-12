// project_settings_logic.js
// A project's own settings (#1085, build plan step 4): the `settings` block
// of the project file, and the one rule that says which value a capture uses.
// Pure, so it is tested on its own; project_settings.js holds the open
// project's block and wires it to the page.
//
// The Configuration form is the app's store (settings_sync.js saves whatever
// it shows), so a project's value never goes into the form: readers take the
// form's value and pass it through resolveSettings, which puts the project's
// over it. A project with no block (none open, or a file not opened in a
// window since #1085) resolves to the form, exactly as before.
//
// Keys are the settings file's own names (`AppSettings`, which the capture
// payload shares), so a reader passes what it already reads.

import { killGapsFrom } from './kill_gaps_setting.js';
import { samePath, fileNameOf, folderOf } from './path_display.js';
import { isUsableFov } from './clip_settings_logic.js';

const CAPTURE_MODES = ['frame_sequence', 'direct_to_video', 'obs', 'agr'];

/** Each kind of value a block may hold: the value when it is a usable one,
 *  else undefined, so a hand-edited or damaged field falls back to the form
 *  rather than reaching a capture. */
const KINDS = {
  mode: (v) => (CAPTURE_MODES.includes(v) ? v : undefined),
  size: (v) => (Number.isInteger(v) && v > 0 ? v : undefined),
  // AGR's 0 is "the same as Capture FPS".
  rateOrZero: (v) => (Number.isInteger(v) && v >= 0 ? v : undefined),
  seconds: (v) => (typeof v === 'number' && Number.isFinite(v) && v >= 0 ? v : undefined),
  flag: (v) => (typeof v === 'boolean' ? v : undefined),
  text: (v) => (typeof v === 'string' ? v : undefined),
  textOrNone: (v) => (typeof v === 'string' || v === null ? v : undefined),
  texts: (v) => (Array.isArray(v) && v.every((s) => typeof s === 'string') ? [...v] : undefined),
  killGaps: (v) => (v && typeof v === 'object' ? killGapsFrom(v) : undefined),
  fov: (v) => (isUsableFov(v) ? v : undefined),
  scheduled: (v) => (Array.isArray(v) && v.every((c) => c && typeof c.command === 'string')
    ? v.map((c) => ({ command: c.command, relation: c.relation === 'After' ? 'After' : 'Before', offset_seconds: c.offset_seconds }))
    : undefined),
};

/**
 * The settings a project owns, each as `{ key, path, kind }`: `key` the
 * settings file's name, `path` where it sits in the block (the data model in
 * docs/ui_facelift/build-plan.md). Pre-roll and post-roll stay the app's
 * until the roll check is tested (plan step 6).
 */
export const PROJECT_FIELDS = Object.freeze([
  { key: 'capture_mode', path: ['picture', 'captureMode'], kind: 'mode' },
  { key: 'resolution_width', path: ['picture', 'width'], kind: 'size' },
  { key: 'resolution_height', path: ['picture', 'height'], kind: 'size' },
  { key: 'capture_fps', path: ['picture', 'captureFps'], kind: 'size' },
  { key: 'obs_capture_fps', path: ['picture', 'obsCaptureFps'], kind: 'size' },
  { key: 'agr_fps', path: ['picture', 'agrFps'], kind: 'rateOrZero' },
  // Not a settings-file key: Configuration has no field of view of its own,
  // so only the Project settings page sets it (build plan step 8).
  { key: 'capture_fov', path: ['picture', 'fov'], kind: 'fov' },
  { key: 'finish_clips_after_batch', path: ['picture', 'finished', 'enabled'], kind: 'flag' },
  { key: 'finish_codec_obs', path: ['picture', 'finished', 'codecObs'], kind: 'text' },
  { key: 'finish_codec_video', path: ['picture', 'finished', 'codecVideo'], kind: 'text' },
  { key: 'finish_codec_frames', path: ['picture', 'finished', 'codecFrames'], kind: 'text' },
  { key: 'record_start_lead', path: ['clipLength', 'beforeSeconds'], kind: 'seconds' },
  { key: 'record_stop_trail', path: ['clipLength', 'afterSeconds'], kind: 'seconds' },
  { key: 'kill_gaps', path: ['clipLength', 'killGaps'], kind: 'killGaps' },
  { key: 'command_profile_active', path: ['clipSettings', 'preset'], kind: 'textOrNone' },
  { key: 'init_commands', path: ['clipSettings', 'initCommands'], kind: 'texts' },
  { key: 'custom_commands', path: ['clipSettings', 'customCommands'], kind: 'scheduled' },
  { key: 'clip_name_template', path: ['clipNames', 'template'], kind: 'text' },
]);

/** The folder lists a project owns per PC (decision 4): full paths, under
 *  `folders.<pcId>`, so a project carried to another PC uses that PC's. */
export const PC_FIELDS = Object.freeze([
  { key: 'target_drives', name: 'destinations' },
  { key: 'render_export_dirs', name: 'exports' },
]);

const pcPath = (field, pcId) => ['folders', pcId, field.name];

/** Where a block keeps its field of view (`capture_fov`'s path). */
const PICTURE_FOV_PATH = PROJECT_FIELDS.find((f) => f.key === 'capture_fov').path;

function readPath(block, path) {
  return path.reduce((at, part) => (at && typeof at === 'object' ? at[part] : undefined), block);
}

/** `block` with `value` at `path`, every object on the way copied, so the
 *  block it came from, and any field this build doesn't know, are untouched. */
function writePath(block, path, value) {
  const [part, ...rest] = path;
  const base = block && typeof block === 'object' ? block : {};
  return { ...base, [part]: rest.length === 0 ? value : writePath(base[part], rest, value) };
}

const copyOf = (value) => (value === undefined ? undefined : JSON.parse(JSON.stringify(value)));

/** Every owned value of `block` as `[key, value]`, the bad ones left out. */
function blockValues(block, pcId) {
  if (!block || typeof block !== 'object') return [];
  const values = PROJECT_FIELDS.map((f) => [f.key, KINDS[f.kind](readPath(block, f.path))]);
  if (pcId) {
    PC_FIELDS.forEach((f) => values.push([f.key, KINDS.texts(readPath(block, pcPath(f, pcId)))]));
  }
  return values.filter(([, value]) => value !== undefined);
}

/**
 * `form` (any of the keys above, as the form or the settings file holds
 * them) with the project's value over each one `block` sets: what a capture,
 * a launch or a render uses. Keys `form` doesn't have stay out, so a reader
 * gets back just what it asked about. `block` null (no project, or one that
 * sets nothing) returns `form` as it is. `pcId` picks this PC's folders.
 * Returns a new object; neither argument is changed.
 */
export function resolveSettings(form, block, pcId = '') {
  const resolved = { ...form };
  for (const [key, value] of blockValues(block, pcId)) {
    if (key in resolved) resolved[key] = value;
  }
  return resolved;
}

/** The field of view `block` sets (Picture), or null when it sets none. */
export function blockFov(block) {
  const fov = readPath(block, PICTURE_FOV_PATH);
  return isUsableFov(fov) ? fov : null;
}

/** `block` with its field of view set to `fov` (project_fov.js's fill). */
export function blockWithFov(block, fov) {
  return writePath(block, PICTURE_FOV_PATH, fov);
}

/**
 * resolveSettings for what a batch sends: the same values, and with
 * `pictureFov` (the New UI switch: the page that sets it exists) the
 * block's field of view as `picture_fov`. The backend sends it after the
 * Initial Commands and leaves their own `mirv_fov <n>` out (native
 * picture_fov.rs), so it wins over every line and every cfg they exec.
 * Without it, or with a block that sets none, the batch sends what it did
 * before step 8.
 */
export function resolveCaptureSettings(form, block, pcId = '', { pictureFov = false } = {}) {
  const resolved = resolveSettings(form, block, pcId);
  const fov = pictureFov ? blockFov(block) : null;
  return fov === null ? resolved : { ...resolved, picture_fov: fov };
}

/**
 * A whole block from the settings file's values (`form`, persistAppSettings'
 * payload): the snapshot older projects take (decision 6,
 * project_settings_snapshot.js, which builds it with no `pcId`), and what a
 * project saved for the first time starts with. The folders go under `pcId`;
 * `madeWith` is the install it was made with (madeWithOf), or null.
 */
export function blockFromForm(form, { pcId, madeWith = null }) {
  let block = {};
  for (const f of PROJECT_FIELDS) {
    const value = KINDS[f.kind](copyOf(form[f.key]));
    if (value !== undefined) block = writePath(block, f.path, value);
  }
  if (pcId) {
    for (const f of PC_FIELDS) block = writePath(block, pcPath(f, pcId), KINDS.texts(form[f.key]) || []);
  }
  return writePath(block, ['install', 'madeWith'], madeWith);
}

/**
 * An edit's values into a block, for the Project settings page's Apply and
 * for Configuration, which changes the open project too until it shows a
 * read-only "This project sets" line (build plan): each owned value that differs
 * between `before` and `after` (two settings payloads) goes into `block`,
 * and a new `hl_path` becomes its `install.madeWith`. Returns `{ block, changed }`, `changed` the keys copied, `block` the same
 * object when none were.
 */
export function followFormEdits(block, before, after, pcId = '') {
  const fields = [
    ...PROJECT_FIELDS.map((f) => ({ key: f.key, path: f.path })),
    ...(pcId ? PC_FIELDS.map((f) => ({ key: f.key, path: pcPath(f, pcId) })) : []),
  ];
  const changed = [];
  let next = block;
  for (const { key, path } of fields) {
    if (!(key in after) || JSON.stringify(before?.[key]) === JSON.stringify(after[key])) continue;
    next = writePath(next, path, copyOf(after[key]));
    changed.push(key);
  }
  // An install picked with the project open is the one it uses now, so the
  // next open doesn't offer to switch back.
  const picked = after.hl_path;
  if (picked && !samePath(before?.hl_path || '', picked) && !samePath(readPath(next, ['install', 'madeWith', 'hlExe']) || '', picked)) {
    next = withMadeWith(next, picked);
    changed.push('hl_path');
  }
  return { block: next, changed };
}

/** `block` saying it is made with the install at `hlExe` from now on. */
export function withMadeWith(block, hlExe) {
  return writePath(block, ['install', 'madeWith'], madeWithOf(hlExe));
}

/** The keys whose value `block` sets to something other than `form`'s. */
export function keysDifferingFromForm(form, block, pcId = '') {
  return blockValues(block, pcId)
    .filter(([key, value]) => key in form && JSON.stringify(value) !== JSON.stringify(form[key]))
    .map(([key]) => key);
}

/** What a block remembers of the install at `hlExe`: the hl.exe, and the
 *  install's folder name ("Half-Life - POST-Anniversary for Movies"), which
 *  is how another PC finds its own copy. Null for no hl.exe. */
export function madeWithOf(hlExe) {
  const exe = String(hlExe || '').trim();
  if (!exe) return null;
  return { name: fileNameOf(folderOf(exe)), hlExe: exe };
}

/** The install row of `installs` for `hlExe`, or null. */
export function installFor(installs, hlExe) {
  return (installs || []).find((i) => samePath(i.hl_exe, hlExe)) || null;
}

/**
 * The install this PC should offer to switch to for a project made with
 * `madeWith`, while `currentHlExe` is picked: the same hl.exe, else an
 * install of the same folder name (the project came from another PC). Null
 * when the current one is it, when the block names none, or when this PC
 * has no such install.
 */
export function installToOffer(madeWith, currentHlExe, installs) {
  if (!madeWith || (!madeWith.hlExe && !madeWith.name)) return null;
  const current = installFor(installs, currentHlExe);
  const isMadeWith = (i) => (madeWith.hlExe && samePath(i.hl_exe, madeWith.hlExe))
    || (!!madeWith.name && i.name === madeWith.name);
  if (samePath(currentHlExe, madeWith.hlExe) || (current && isMadeWith(current))) return null;
  return installFor(installs, madeWith.hlExe)
    || (installs || []).find((i) => !!madeWith.name && i.name === madeWith.name) || null;
}
