// placeholder_catalogue.js
// The one list of name placeholders (#751): which rows they sit in and
// which of the two templates has each, the clip name's (clip_name.js) and
// Rename Demos' (demo_rename.js). Both lay their chips out from here, so the
// two can't drift apart again. What each one means is in strings.js
// (STRINGS.PLACEHOLDERS). Pure, no DOM.

import { STRINGS } from './strings.js';

/**
 * The chip rows in order, by what each placeholder holds. `both` are in
 * both templates with the same meaning; `clip` and `demo` only in that one.
 * `{kills}` is in both but means different things (the highlight's kills,
 * the player's kills in the whole demo), so each template lists it where it
 * belongs.
 */
const ROWS = [
  { key: 'player', both: ['name', 'full_name'], demo: ['kills', 'deaths'] },
  { key: 'sides', both: ['faction', 'enemy_faction'] },
  { key: 'teams', both: ['team_name', 'opponent', 'allies', 'axis', 'team1', 'team2'] },
  { key: 'demo', both: ['map', 'date', 'demo_type', 'demo', 'half', 'match'] },
  { key: 'highlight', clip: ['kills', 'headshots', 'weapons', 'first_weapon', 'victims', 'first_victim', 'row', 'time'] },
];

/** The chip rows of one template (`'clip'` or `'demo'`): `[{ key, names }]`,
 *  rows it has nothing in left out. */
export function placeholderGroups(kind) {
  return ROWS
    .map((r) => ({ key: r.key, names: [...(r.both || []), ...(r[kind] || [])] }))
    .filter((g) => g.names.length);
}

/** Every placeholder of one template, in chip order. */
export function placeholderNames(kind) {
  return placeholderGroups(kind).flatMap((g) => g.names);
}

/** Old names that still work. A clip name's `{player}` was the player's
 *  name as it is, tag included, so saved templates keep their names. */
export const CLIP_ALIASES = { player: 'full_name' };

/** The placeholders a demo or highlight can be without, so the ones that
 *  take a `|fallback` word, in both templates. */
export const WITH_FALLBACK = new Set([
  'name', 'full_name', 'faction', 'enemy_faction', 'team_name', 'opponent', 'allies', 'axis', 'team1', 'team2',
  'half', 'match',
]);

export const MODIFIERS = ['lower', 'upper', 'first'];

/** What a placeholder means in one template: that template's own
 *  description where it has one, else the shared one. */
export function placeholderDescription(kind, name) {
  const own = kind === 'clip' ? STRINGS.CLIP_NAME.DESCRIPTIONS : STRINGS.DEMO_RENAME.DESCRIPTIONS;
  return own[name] ?? STRINGS.PLACEHOLDERS.DESCRIPTIONS[name] ?? '';
}
