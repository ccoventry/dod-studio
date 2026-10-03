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
  'name', 'kills', 'deaths', 'map', 'date', 'demo_type',
  'team_name', 'opponent', 'allies', 'axis', 'team1', 'team2', 'demo',
];

/** The ones a demo can be without, so the ones that take `|fallback`. */
const WITH_FALLBACK = new Set(['name', 'team_name', 'opponent', 'allies', 'axis', 'team1', 'team2']);

const DEMO_RULES = {
  placeholders: DEMO_PLACEHOLDERS,
  withFallback: WITH_FALLBACK,
  // Clashes get `_2`, `_3`, ... in the preview, where they can be seen.
  distinguishing: [],
  noDistinguishing: () => '',
  fallbackNotAllowed: (raw) => STRINGS.DEMO_RENAME.fallbackNotAllowed(raw),
};

/** A template checked against the demo placeholders. */
export function parseDemoTemplate(template) {
  return parseTemplate(template, DEMO_RULES);
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
  return {
    name: facts?.name || null,
    kills: number(facts?.kills),
    deaths: number(facts?.deaths),
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
export function planRenames(factsList, { povTemplate, hltvTemplate, projectTeams, selected }) {
  const templates = {
    pov: parseDemoTemplate(povTemplate),
    hltv: parseDemoTemplate(hltvTemplate),
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

    const base = buildName(parsed, demoValues(facts, projectTeams)).name;
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
