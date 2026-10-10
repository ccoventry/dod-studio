// demo_rename.js
// The Demo Auditor's renamer (#469): each demo's new name, from one template
// for POV demos and one for HLTV demos, on the clip names' template engine
// (#441, clip_name.js). Team names are the project's Teams list's (#445).
// Pure, so it can be tested on its own.
//
// The facts come from native::demo_rename::DemoFacts:
// `{ path, file_name, map, modified_unix_secs, demo_type, name, kills,
//    deaths, side, teams: [{ side, tag }], error }`.

import { parseTemplate, buildName } from './clip_name.js';
import { sideTag, teamName } from './project_teams.js';
import { STRINGS } from './strings.js';

/** Keep in step with the defaults in settings_manager.rs. */
export const DEFAULT_POV_TEMPLATE = '{name}_{kills}k_v_{opponent}_{map}';
export const DEFAULT_HLTV_TEMPLATE = '{allies}_v_{axis}_{map}_{date}';

export const DEMO_PLACEHOLDERS = [
  'name', 'full_name', 'kills', 'deaths', 'faction', 'enemy_faction', 'map', 'date', 'demo_type',
  'team_name', 'opponent', 'allies', 'axis', 'team1', 'team2', 'demo',
];

/** About the recording player, so only a POV demo has them: an HLTV
 *  template that uses one is refused, and its chips leave them out. */
export const POV_ONLY = new Set([
  'name', 'full_name', 'kills', 'deaths', 'faction', 'enemy_faction', 'team_name', 'opponent',
]);

/** The chip rows, by what each placeholder holds: the recording player,
 *  the sides (Allies / Axis), the teams' names, and the demo itself. Every
 *  placeholder is in exactly one. */
export const DEMO_PLACEHOLDER_GROUPS = [
  { key: 'player', names: ['name', 'full_name', 'kills', 'deaths'] },
  { key: 'sides', names: ['faction', 'enemy_faction'] },
  { key: 'teams', names: ['team_name', 'opponent', 'allies', 'axis', 'team1', 'team2'] },
  { key: 'demo', names: ['map', 'date', 'demo_type', 'demo'] },
];

/** The placeholders a demo type has, in chip order. */
export function placeholdersFor(demoType) {
  return demoType === 'hltv' ? DEMO_PLACEHOLDERS.filter((p) => !POV_ONLY.has(p)) : DEMO_PLACEHOLDERS;
}

/** The ones a demo can be without, so the ones that take `|fallback`. */
export const WITH_FALLBACK = new Set([
  'name', 'full_name', 'faction', 'enemy_faction', 'team_name', 'opponent', 'allies', 'axis', 'team1', 'team2',
]);

const DEMO_RULES = {
  placeholders: DEMO_PLACEHOLDERS,
  withFallback: WITH_FALLBACK,
  // Clashes get `_2`, `_3`, ... in the preview, where they can be seen.
  distinguishing: [],
  noDistinguishing: () => '',
  fallbackNotAllowed: (raw) => STRINGS.DEMO_RENAME.fallbackNotAllowed(raw),
};

/** A template checked against the placeholders its demo type has. */
export function parseDemoTemplate(template, demoType = 'pov') {
  const parsed = parseTemplate(template, DEMO_RULES);
  if (demoType === 'hltv') {
    for (const part of parsed.parts) {
      if (part.name && POV_ONLY.has(part.name)) parsed.errors.push(STRINGS.DEMO_RENAME.povOnly(part.raw));
    }
  }
  return parsed;
}

/** `name` without the clan tag its side's players share (#445), and
 *  without the punctuation that framed the tag: `dicE[: :]m00cat :D`
 *  with tag `dicE` gives `m00cat :D`. The name as it is when there's no
 *  tag, or nothing would be left. */
export function nameWithoutTag(name, tag) {
  if (!name || !tag) return name || null;
  const lower = name.toLowerCase();
  const t = tag.toLowerCase();
  // Past any bracket or symbol the tag sits in: `[dicE] m00cat`.
  const start = lower.match(/^[^\p{L}\p{N}]*/u)[0].length;
  const end = lower.length - lower.match(/[^\p{L}\p{N}]*$/u)[0].length;
  let rest;
  if (lower.startsWith(t, start)) rest = name.slice(start + tag.length);
  else if (lower.slice(0, end).endsWith(t)) rest = name.slice(0, end - tag.length);
  else return name;
  rest = rest.replace(/^[^\p{L}\p{N}]+/u, '').replace(/[^\p{L}\p{N}]+$/u, '');
  return rest || name;
}

/**
 * List Demos' progress, from a `demo_rename_progress` event
 * (`{ done, total, cached, parsed }`: the cached demos are read first, then
 * the rest are parsed). `parseMs` is how long the parses have been running.
 * Returns `{ pct, text }`; the text adds a time left once a parse has
 * finished to measure the pace by.
 */
export function listProgressView({ done = 0, total = 0, cached = 0, parsed = 0 } = {}, parseMs = 0) {
  const pct = total ? Math.min(100, Math.round((done / total) * 100)) : 0;
  const D = STRINGS.DEMO_RENAME;
  let text = D.reading(done, total);
  if (cached > 0) text += D.readingCached(cached);
  const left = total - cached - parsed;
  if (parsed > 0 && left > 0 && parseMs > 0) text += D.timeLeft(Math.round((parseMs / parsed) * left / 1000));
  return { pct, text };
}

/** A file name made plain: letters, digits, `-` and `_` only. Anything
 *  else (spaces, brackets, `#`, emoji) becomes `_`, accents are dropped,
 *  runs of `_` collapse, and none is left at either end. */
export function plainName(name) {
  return String(name ?? '')
    .normalize('NFKD')
    .replace(/\p{M}/gu, '')
    .replace(/[^A-Za-z0-9_-]+/g, '_')
    .replace(/_+/g, '_')
    .replace(/^[_-]+|[_-]+$/g, '');
}

function localDate(unixSecs) {
  if (!unixSecs) return null;
  const d = new Date(unixSecs * 1000);
  const pad = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

const isAxis = (side) => side === 'Axis';

/** Two names in a fixed order whichever side each played: alphabetical,
 *  a missing one last. */
function fixedOrder(a, b) {
  if (!a || !b) return a ? [a, b] : [b, a];
  return a.localeCompare(b, undefined, { sensitivity: 'base' }) <= 0 ? [a, b] : [b, a];
}

/** Every placeholder's value for one demo. `null` means missing. */
export function demoValues(facts, projectTeams) {
  const allies = teamName(projectTeams, sideTag(facts, 'Allies'));
  const axis = teamName(projectTeams, sideTag(facts, 'Axis'));
  const known = facts?.side === 'Allies' || facts?.side === 'British' || isAxis(facts?.side);
  const [team1, team2] = fixedOrder(allies, axis);
  const number = (n) => (n === null || n === undefined ? null : String(n));
  // The other side: Axis against Allies or British, whichever the demo has.
  const enemy = !known ? null
    : isAxis(facts.side)
      ? ((facts.teams || []).find((t) => t.side === 'British') ? 'British' : 'Allies')
      : 'Axis';
  return {
    name: nameWithoutTag(facts?.name || null, sideTag(facts, facts?.side)),
    full_name: facts?.name || null,
    kills: number(facts?.kills),
    deaths: number(facts?.deaths),
    faction: known ? facts.side : null,
    enemy_faction: enemy,
    map: facts?.map ? facts.map.replace(/^dod_/i, '') : null,
    date: localDate(facts?.modified_unix_secs),
    demo_type: facts?.demo_type || null,
    team_name: known ? (isAxis(facts.side) ? axis : allies) : null,
    opponent: known ? (isAxis(facts.side) ? allies : axis) : null,
    allies,
    axis,
    team1,
    team2,
    demo: facts?.file_name ? facts.file_name.replace(/\.dem$/i, '') : null,
  };
}

function folderOf(facts) {
  return facts.path.slice(0, facts.path.length - facts.file_name.length);
}

/**
 * The preview: one row per demo, in the order given.
 *
 * Row: `{ path, folder, from, to, demoType, status, numbered, error }`, where
 * `status` is `rename`, `same` (the template gives its current name),
 * `skipped` (not ticked), `unreadable` or `template` (its type's template has
 * errors). `to` is the name it ends up with.
 *
 * A new name never takes one already in its folder: every demo's current
 * name there is kept free, except the demo's own (so a change of case alone
 * goes through). A clash gets `_2`, `_3`, ... and `numbered`.
 */
export function planRenames(factsList, { povTemplate, hltvTemplate, lowercase = false, projectTeams, selected }) {
  const templates = {
    pov: parseDemoTemplate(povTemplate, 'pov'),
    hltv: parseDemoTemplate(hltvTemplate, 'hltv'),
  };
  const taken = new Map();
  const takenIn = (folder) => {
    if (!taken.has(folder)) taken.set(folder, new Set());
    return taken.get(folder);
  };
  for (const facts of factsList) takenIn(folderOf(facts)).add(facts.file_name.toLowerCase());

  return factsList.map((facts) => {
    const folder = folderOf(facts);
    const demoType = facts.demo_type === 'hltv' ? 'hltv' : 'pov';
    const row = {
      path: facts.path, folder, from: facts.file_name, to: facts.file_name,
      demoType, status: 'rename', numbered: false, error: facts.error || null,
    };
    if (!selected.has(facts.path)) return { ...row, status: 'skipped' };
    if (facts.error) return { ...row, status: 'unreadable' };
    const parsed = templates[demoType];
    if (parsed.errors.length) return { ...row, status: 'template' };

    const built = plainName(buildName(parsed, demoValues(facts, projectTeams)).name);
    const base = lowercase ? built.toLowerCase() : built;
    if (!base) return { ...row, status: 'template' };
    const used = takenIn(folder);
    const own = facts.file_name.toLowerCase();
    let to = `${base}.dem`;
    for (let n = 2; to.toLowerCase() !== own && used.has(to.toLowerCase()); n++) {
      to = `${base}_${n}.dem`;
      row.numbered = true;
    }
    if (to === facts.file_name) return { ...row, status: 'same', numbered: false };
    used.add(to.toLowerCase());
    return { ...row, to };
  });
}

/** The pairs to send to `demo_rename_apply`: full paths, renames only. */
export function renamePairs(rows) {
  return rows.filter((r) => r.status === 'rename').map((r) => ({ from: r.path, to: r.folder + r.to }));
}
