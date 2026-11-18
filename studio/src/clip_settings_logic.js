// clip_settings_logic.js
// The rules behind Project settings › Clip settings (#1085, build plan
// step 8), with no DOM, so they are tested on their own. The tab is an
// editor over the project's Initial Commands lines (`init_commands`): a row
// reads the last line that sets its cvar, as the game does (the last one
// wins), and an edit rewrites only that line, so a list nobody edits stays
// exactly as it was. Presets stay command profiles (#442) and are copied in
// (decision 9): picking one replaces the lists, with no link back.
//
// Field of view is the project's own, on the Project tab's Picture
// (`settings.picture.fov`, project_fov.js). The batch sends it after these
// lines and leaves their `mirv_fov <n>` out (native picture_fov.rs), so
// such a line is no row: it shows in Your own commands with its refused
// note. `r_decals` stays a line.
//
// Use Clip settings (`clip_rows_on`, the block's `clipSettings.useRows`)
// decides whether the rows count at all. On, every row shows a value (its
// line's, else the game's own, ROW_DEFAULTS) and a batch sends every row
// after the user's own lines (linesWithRows), so the page is what a capture
// gets. Off, the tab shows only Your own commands, every line in it, and a
// batch sends the lines as they are, as before the rows existed. A block
// without the field (a project made before it) reads as Off; a new project
// starts On (project_settings.js).

import { ROW_CVARS, CLIP_SETTINGS_ROWS, ROW_DEFAULTS, SENT_ROWS } from './clip_settings_rows.js';
import { profileLists } from './command_profiles.js';
import { STRINGS } from './strings.js';

/** The settings the Clip settings tab edits: the settings-file names of the
 *  project block's `clipSettings` lists, and Use Clip settings
 *  (`clip_rows_on`), which only the page sets: Configuration has none. */
export const CLIP_TAB_KEYS = Object.freeze(['clip_rows_on', 'command_profile_active', 'init_commands', 'custom_commands']);

/** Whether `pending` (or a block's resolved values) has Use Clip settings
 *  on. Only `true` is on, so a project without the field reads as Off. */
export const rowsAreOn = (values) => values?.clip_rows_on === true;

/** Field of view's cvar, which the project owns on the Project tab. */
export const FOV_CVAR = 'mirv_fov';

/** The game's own field of view: a project's when nothing says another. */
export const DEFAULT_FOV = 90;

const FOV_MIN = 1;
const FOV_MAX = 179;

/** Whether `v` is a field of view a capture can use: a number from 1 to 179. */
export function isUsableFov(v) {
  return typeof v === 'number' && Number.isFinite(v) && v >= FOV_MIN && v <= FOV_MAX;
}

/** `text` without one pair of surrounding double quotes: a .cfg's `"105"`. */
function unquote(text) {
  const t = text.trim();
  return t.length >= 2 && t.startsWith('"') && t.endsWith('"') ? t.slice(1, -1).trim() : t;
}

/**
 * The setting one Initial Commands line makes, as `{ name, value }` with
 * `name` lower case, or null for a line that sets nothing a row can show:
 * blank, a comment, several commands joined by `;`, or a name with no value.
 */
export function parseSetting(line) {
  const text = String(line ?? '').trim();
  if (!text || text.startsWith('//') || text.includes(';')) return null;
  const match = /^([A-Za-z_][\w.]*)\s+(.+)$/.exec(text);
  if (!match) return null;
  return { name: match[1].toLowerCase(), value: unquote(match[2]) };
}

/** Whether `line` sets one of the rows' cvars. */
function isRowLine(line) {
  const setting = parseSetting(line);
  return !!setting && ROW_CVARS.includes(setting.name);
}

/** The index of the last line of `lines` that sets `cvar`, or -1. */
function lastIndexFor(lines, cvar) {
  const name = cvar.toLowerCase();
  for (let i = lines.length - 1; i >= 0; i -= 1) {
    if (parseSetting(lines[i])?.name === name) return i;
  }
  return -1;
}

/** The value the last line setting `cvar` gives it, or null when no line
 *  does. */
export function rowValue(lines, cvar) {
  const i = lastIndexFor(lines || [], cvar);
  return i < 0 ? null : parseSetting(lines[i]).value;
}

/** The value a row shows, and a batch sends, with Use Clip settings on: its
 *  line's, else the game's own (ROW_DEFAULTS). */
export function shownRowValue(lines, cvar) {
  return rowValue(lines, cvar) ?? ROW_DEFAULTS[cvar];
}

/**
 * The Initial Commands a batch sends with Use Clip settings on: the lines no
 * row shows, in their order, then one line for every row (SENT_ROWS, page
 * order) with its shown value. The rows go last, as Which wins says, so a cfg
 * an own line `exec`s never has the last word over a row.
 */
export function linesWithRows(lines) {
  const own = (lines || []).filter((line) => !isRowLine(line));
  return [...own, ...SENT_ROWS.map((r) => `${r.cvar} ${shownRowValue(lines, r.cvar)}`)];
}

/** Whether a config's `configValue` for a row differs from the row's
 *  `shown` value: numbers compared as numbers (`1` and `1.0` agree), else
 *  any case, as the native value rules do (cfg_scan.rs same_value). */
export function differsFromConfig(shown, configValue) {
  const a = Number(shown);
  const b = Number(configValue);
  if (String(shown).trim() !== '' && String(configValue).trim() !== '' && Number.isFinite(a) && Number.isFinite(b)) return a !== b;
  return String(shown).toLowerCase() !== String(configValue).toLowerCase();
}

/**
 * `lines` with `cvar` set to `value`: the last line that sets it rewritten
 * in place, earlier ones dropped (they did nothing), or a line added at the
 * end when none did. `value` null drops every line for it. Other lines keep
 * their text and order.
 */
export function withRowValue(lines, cvar, value) {
  const list = [...(lines || [])];
  const last = lastIndexFor(list, cvar);
  const name = cvar.toLowerCase();
  const next = [];
  list.forEach((line, i) => {
    if (parseSetting(line)?.name !== name) next.push(line);
    else if (i === last && value !== null) next.push(`${cvar} ${value}`);
  });
  if (last < 0 && value !== null) next.push(`${cvar} ${value}`);
  return next;
}

/** What Your own commands lists, as `{ index, text }` with `index` into
 *  `lines`: with `rowsOn` the lines no row shows, else every line (no row
 *  shows then, and every line is sent as it is). */
export function ownEntries(lines, { rowsOn = true } = {}) {
  const entries = (lines || []).map((text, index) => ({ index, text }));
  return rowsOn ? entries.filter((e) => !isRowLine(e.text)) : entries;
}

/** `lines` with the line at `index` replaced by `text`. */
export function withLineAt(lines, index, text) {
  return (lines || []).map((line, i) => (i === index ? text : line));
}

/** `lines` without the line at `index`. */
export function withoutLineAt(lines, index) {
  return (lines || []).filter((_, i) => i !== index);
}

/**
 * `lines` with each row cvar set by one line only, the last (it is the one
 * that counts): run after Your own commands is edited, so a row setting
 * typed there moves into its row. Returns the same array when nothing
 * repeats.
 */
export function withRowLinesMerged(lines) {
  const list = lines || [];
  const keep = new Set();
  const seen = new Set();
  for (let i = list.length - 1; i >= 0; i -= 1) {
    const setting = parseSetting(list[i]);
    if (!setting || !ROW_CVARS.includes(setting.name)) keep.add(i);
    else if (!seen.has(setting.name)) {
      seen.add(setting.name);
      keep.add(i);
    }
  }
  return keep.size === list.length ? list : list.filter((_, i) => keep.has(i));
}

/** The field of view the lines give, the last `mirv_fov <n>` line's, or
 *  null. A sub-command (`mirv_fov handleZoom enabled 1`) sets none. */
export function fovFromLines(lines) {
  const list = lines || [];
  for (let i = list.length - 1; i >= 0; i -= 1) {
    const setting = parseSetting(list[i]);
    if (setting?.name !== FOV_CVAR || /\s/.test(setting.value)) continue;
    const fov = Number(setting.value);
    if (isUsableFov(fov)) return fov;
  }
  return null;
}

/**
 * What picking `profile` (a command profile, built in or the user's) puts
 * into the tab's pending values: a copy of its lists, and its name. Null
 * (No preset) empties both lists, so every row goes back to the game's own
 * value.
 */
export function presetPick(profile) {
  if (!profile) return { command_profile_active: '', init_commands: [], custom_commands: [] };
  const lists = profileLists(profile);
  return { command_profile_active: profile.name, ...lists };
}

/** Whether a number row's `value` is usable: a number at least the row's
 *  `min`, whole when the row says so. */
export function isUsableNumber(row, value) {
  const n = Number(value);
  if (String(value ?? '').trim() === '' || !Number.isFinite(n)) return false;
  if (row.integer && !Number.isInteger(n)) return false;
  return row.min === undefined || n >= row.min;
}

/** A value for a row's line from a number box: blank is null (no line, so
 *  the row is back at the game's own value). */
export function numberRowValue(text) {
  const t = String(text ?? '').trim();
  return t === '' ? null : t;
}

/** The `data-row` of a Clip settings row, for its problem mark. */
export const clipRowKey = (row) => `clip-${row.id}`;

/**
 * What is wrong with the Clip settings tab's pending values, as `{ key:
 * message }` (pendingProblems' shape): with Use Clip settings on, a number
 * row whose line holds no usable number (off, that line is the user's own,
 * sent as typed, and no row shows), and an At each clip command with no
 * usable seconds. Refused or warned commands are not problems: they get a
 * note, and Start Capture Batch checks them as it always has.
 */
export function clipProblems(pending) {
  const S = STRINGS.CLIP_SETTINGS;
  const problems = {};
  const lines = pending.init_commands || [];
  for (const row of CLIP_SETTINGS_ROWS) {
    if (row.kind !== 'number' || !rowsAreOn(pending)) continue;
    const value = rowValue(lines, row.cvar);
    if (value !== null && !isUsableNumber(row, value)) problems[clipRowKey(row)] = S.badNumber(row.min);
  }
  const badOffset = (pending.custom_commands || []).some((c) => String(c?.command ?? '').trim()
    && !(typeof c.offset_seconds === 'number' && Number.isFinite(c.offset_seconds) && c.offset_seconds >= 0));
  if (badOffset) problems.custom_commands = S.BAD_OFFSET;
  return problems;
}
