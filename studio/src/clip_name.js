// clip_name.js
// A highlight's clip name (#441): built from a user-set template, or typed on
// its Highlight Details row, and used to name the finished file. Pure, so it
// can be tested on its own.
//
// Template syntax: literal text plus `{placeholder}`, `{placeholder:lower}`
// or `{placeholder:upper}`. The two team placeholders also take a fallback
// word after `|`: `{opponent:lower|mix}`.

import { streakUid, resolveTake } from './take_index.js';
import { STRINGS } from './strings.js';

/** Keep in step with `default_clip_name_template` in settings_manager.rs. */
export const DEFAULT_TEMPLATE = '{map}_{player}_{kills}k_{weapons}_{time}';

export const PLACEHOLDERS = [
  'player', 'faction', 'enemy_faction', 'map', 'kills', 'weapons', 'first_weapon',
  'victims', 'first_victim', 'row', 'time', 'demo', 'date', 'team_name', 'opponent',
];
export const MODIFIERS = ['lower', 'upper'];

/** The only placeholders that can be missing, so the only ones with a fallback. */
const WITH_FALLBACK = new Set(['team_name', 'opponent']);
/** What tells two highlights apart. */
const DISTINGUISHING = ['row', 'time', 'demo'];
/** Cut first, in this order, when a name is too long for its path. */
const TRIM_ORDER = ['victims', 'demo', 'weapons'];
/** Never cut: they keep names unique and readable. */
const NEVER_TRIM = new Set(['row', 'time']);

/** Characters Windows refuses in a file name. */
const INVALID_CHARS = /[\\/:*?"<>|]/;
const INVALID_CHARS_ALL = /[\\/:*?"<>|\x00-\x1f]/g;

/** A file name past this is flagged in the preview. */
export const NAME_WARN_LENGTH = 120;
/** The full output path is kept under this, below Windows' 260. */
export const PATH_LIMIT = 250;
/** Room for what the renderer adds: `_hud`, a wav suffix, `_NN`, extension. */
const RENDER_SUFFIX_MARGIN = 20;

/**
 * Splits a template into literal text and placeholders, with everything
 * wrong with it. `errors` stop it being used; `warnings` don't.
 */
export function parseTemplate(template) {
  const text = String(template ?? '');
  const parts = [];
  const errors = [];
  const warnings = [];
  let literal = '';
  const flushLiteral = () => {
    if (literal) parts.push({ text: literal });
    literal = '';
  };

  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (c === '}') {
      errors.push(STRINGS.CLIP_NAME.strayBrace(i + 1));
      continue;
    }
    if (c !== '{') {
      literal += c;
      continue;
    }
    const close = text.indexOf('}', i + 1);
    const nextOpen = text.indexOf('{', i + 1);
    if (close === -1 || (nextOpen !== -1 && nextOpen < close)) {
      errors.push(STRINGS.CLIP_NAME.unclosedBrace(i + 1));
      literal += c;
      continue;
    }
    flushLiteral();
    const raw = text.slice(i, close + 1);
    const inner = text.slice(i + 1, close);
    const [spec, fallback] = inner.split('|', 2);
    const [name, modifier] = spec.split(':', 2);
    const part = { raw, name: name.trim(), modifier: modifier?.trim() || null, fallback: fallback ?? null };
    if (!part.name) errors.push(STRINGS.CLIP_NAME.EMPTY_PLACEHOLDER);
    else if (!PLACEHOLDERS.includes(part.name)) errors.push(STRINGS.CLIP_NAME.unknownPlaceholder(raw));
    else if (part.modifier !== null && !MODIFIERS.includes(part.modifier)) errors.push(STRINGS.CLIP_NAME.unknownModifier(raw));
    else if (part.fallback !== null && !WITH_FALLBACK.has(part.name)) errors.push(STRINGS.CLIP_NAME.fallbackNotAllowed(raw));
    else if (part.fallback !== null && INVALID_CHARS.test(part.fallback)) errors.push(STRINGS.CLIP_NAME.invalidCharacters(raw));
    parts.push(part);
    i = close;
  }
  flushLiteral();

  const bad = parts.filter((p) => p.text !== undefined && INVALID_CHARS.test(p.text));
  if (bad.length) errors.push(STRINGS.CLIP_NAME.invalidCharacters(bad.map((p) => p.text).join(' ')));
  if (!parts.some((p) => p.name || (p.text && p.text.trim()))) errors.push(STRINGS.CLIP_NAME.EMPTY_TEMPLATE);
  else if (!parts.some((p) => DISTINGUISHING.includes(p.name))) warnings.push(STRINGS.CLIP_NAME.NO_DISTINGUISHING);

  return { parts, errors, warnings };
}

/** A value made safe for a file name. */
export function cleanValue(value) {
  return String(value ?? '').replace(INVALID_CHARS_ALL, '_').trim();
}

function weaponWord(weapon) {
  return String(weapon || '').toLowerCase().replace(/\s+/g, '');
}

function distinct(values) {
  return [...new Set(values.filter(Boolean))];
}

function mostCommon(values) {
  const counts = new Map();
  values.forEach((v) => counts.set(v, (counts.get(v) || 0) + 1));
  return [...counts.entries()].sort((a, b) => b[1] - a[1])[0]?.[0] || null;
}

/** Seconds into the demo as `12m34s`. */
export function timeWord(seconds) {
  const total = Math.max(0, Math.floor(Number(seconds) || 0));
  return `${Math.floor(total / 60)}m${String(total % 60).padStart(2, '0')}s`;
}

function localDate(unixSecs) {
  if (!unixSecs) return null;
  const d = new Date(unixSecs * 1000);
  const pad = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** The recording player's highlights, the rows Highlight Details numbers
 *  with Min Kills at 1. */
function ownStreaks(demo) {
  const own = demo?.local_player_index;
  const streaks = demo?.streaks || [];
  if (own === null || own === undefined) return streaks;
  return streaks.filter((s) => s.player_index === own);
}

/** The highlight's row number, 1-based, stable whatever Min Kills is set to. */
export function highlightRow(demo, streak) {
  const index = ownStreaks(demo).indexOf(streak);
  return index === -1 ? 1 : index + 1;
}

/**
 * Every placeholder's value for one highlight, over its chosen Kill Range.
 * `null` means missing (only the team placeholders, or data a project saved
 * before #441 doesn't have until the demo is scanned again).
 */
export function highlightValues(demo, streak, context = {}) {
  const kills = streak?.kills || [];
  const start = Math.min(streak?.start_index ?? 0, Math.max(kills.length - 1, 0));
  const end = Math.max(start, Math.min(streak?.end_index ?? kills.length - 1, kills.length - 1));
  const range = kills.slice(start, end + 1);
  const victims = (streak?.victims || []).slice(start, end + 1);
  const victimSides = (streak?.victim_factions || []).slice(start, end + 1).filter((f) => f && f !== 'Unknown');
  const seconds = streak?.viewdemo_times?.[start] ?? range[0]?.[1] ?? 0;
  const map = demo?.map_name ? demo.map_name.replace(/^dod_/i, '') : null;
  const row = context.row ?? highlightRow(demo, streak);
  const teams = context.teams || {};
  return {
    player: streak?.target_player || null,
    faction: streak?.faction || null,
    enemy_faction: mostCommon(victimSides),
    map,
    kills: String(range.length || streak?.kill_count || 0),
    weapons: distinct(range.map((k) => weaponWord(k[2]))).join('-') || null,
    first_weapon: weaponWord(range[0]?.[2]) || null,
    victims: distinct(victims).join('-') || null,
    first_victim: victims[0] || null,
    row: String(row).padStart(2, '0'),
    time: timeWord(seconds),
    demo: demo?.name ? demo.name.replace(/\.dem$/i, '') : null,
    date: localDate(demo?.modified_unix_secs),
    team_name: teams.team_name || null,
    opponent: teams.opponent || null,
  };
}

function applyModifier(value, modifier) {
  if (modifier === 'lower') return value.toLowerCase();
  if (modifier === 'upper') return value.toUpperCase();
  return value;
}

/**
 * Builds a name from a parsed template. Each placeholder's value is cleaned
 * for a file name; a missing one gives its fallback word, else `unknown`.
 * With `maxLength`, long parts are cut (victims, demo, weapons first) until
 * it fits; `{row}`, `{time}` and literal text are never cut.
 */
export function buildName(parsed, values, { maxLength } = {}) {
  const pieces = parsed.parts.map((p) => {
    if (p.text !== undefined) return { text: p.text, name: null };
    // Shown as typed, so the preview points at it (the error says why).
    if (!PLACEHOLDERS.includes(p.name)) return { text: p.raw, name: null };
    const value = values[p.name];
    const text = value === null || value === undefined || value === ''
      ? cleanValue(p.fallback ?? STRINGS.CLIP_NAME.MISSING_VALUE)
      : cleanValue(value);
    return { text: applyModifier(text, p.modifier), name: p.name };
  });
  const joined = () => pieces.map((p) => p.text).join('').replace(/[. ]+$/, '');
  let trimmed = false;
  if (maxLength) {
    const cutOrder = [
      ...TRIM_ORDER,
      ...pieces.filter((p) => p.name && !NEVER_TRIM.has(p.name) && !TRIM_ORDER.includes(p.name))
        .sort((a, b) => b.text.length - a.text.length).map((p) => p.name),
    ];
    for (const name of distinct(cutOrder)) {
      for (const piece of pieces.filter((p) => p.name === name)) {
        const over = joined().length - maxLength;
        if (over <= 0) break;
        const keep = Math.max(1, piece.text.length - over);
        if (keep < piece.text.length) {
          piece.text = piece.text.slice(0, keep).replace(/[-_ ]+$/, '');
          trimmed = true;
        }
      }
    }
  }
  return { name: joined(), trimmed };
}

/** The longest file name that keeps every export folder's path under the limit. */
export function maxNameLength(exportDirs) {
  const longest = Math.max(0, ...(exportDirs || []).map((d) => String(d).replace(/[\\/]+$/, '').length));
  return PATH_LIMIT - longest - 1 - RENDER_SUFFIX_MARGIN;
}

/**
 * A highlight's clip name: the one typed on its row, else the template's.
 * `{ name, typed, trimmed }`.
 */
export function clipNameFor(demo, streak, template, options = {}) {
  const typed = cleanValue(streak?.clipName).replace(/[. ]+$/, '');
  if (typed) return { name: typed, typed: true, trimmed: false };
  return automaticClipName(demo, streak, template, options);
}

/** The template's name for a highlight, ignoring any typed one. A template
 *  with errors gives the default template's name. */
export function automaticClipName(demo, streak, template, { maxLength, teams } = {}) {
  const parsed = parseTemplate(template);
  const source = parsed.errors.length ? parseTemplate(DEFAULT_TEMPLATE) : parsed;
  return { ...buildName(source, highlightValues(demo, streak, { teams }), { maxLength }), typed: false };
}

/** Makes names unique (ignoring case) by adding `_2`, `_3`, ... to repeats. */
export function uniqueNames(names) {
  const seen = new Set();
  return names.map((name) => {
    let candidate = name;
    for (let n = 2; seen.has(candidate.toLowerCase()); n++) candidate = `${name}_${n}`;
    seen.add(candidate.toLowerCase());
    return candidate;
  });
}

/**
 * Clip names for every recorded take the project knows (`take key -> name`),
 * for Render Studio's queue. A take that covers several highlights (an
 * overlap merge) is named after its first one. Names come out unique.
 */
export function clipNamesForTakes(takeIndex, demos, template, options = {}) {
  const byUid = new Map();
  (demos || []).forEach((demo) => (demo.streaks || []).forEach((streak) => {
    byUid.set(streakUid(demo.path, streak), { demo, streak });
  }));
  const keys = [];
  const names = [];
  Object.keys(takeIndex || {}).sort().forEach((key) => {
    const highlights = resolveTake(takeIndex, key).map((uid) => byUid.get(uid)).filter(Boolean);
    if (!highlights.length) return;
    highlights.sort((a, b) => a.streak.start_tick - b.streak.start_tick);
    const { demo, streak } = highlights[0];
    const { name } = clipNameFor(demo, streak, template, options);
    if (!name) return;
    keys.push(key);
    names.push(name);
  });
  const unique = uniqueNames(names);
  return Object.fromEntries(keys.map((key, i) => [key, unique[i]]));
}
