// review_hltv_pick.js
// The HLTV box's "Players I pick" (#828), the pure half: everyone who has a
// highlight in the queue's HLTV demos, a team's players, and the pick the
// project file keeps. review_hltv_picker.js draws it.
//
// A player is one SteamID with every name they used (player_filter.js's
// groupPlayers). A highlight from an older project has no SteamID, only a
// name, so it lists under that name, unless a SteamID'd player in the queue
// used it: review_queue.js's hltvPlayerChosen matches by name too, so that
// player's entry already covers it.

import { groupPlayers } from './player_filter.js';
import { listedStreaks } from './queue_filters.js';
import { isHltvHighlight, HLTV_PLAYERS } from './review_queue.js';
import { buildTeamsList, rootTag, sideTag } from './project_teams.js';

/** The id of a player known only by name. */
const nameId = (name) => `name:${name.trim().toLowerCase()}`;

/** Each HLTV highlight in the queue, with the id its player goes by. */
function hltvEntries(demos) {
  const entries = [];
  for (const demo of demos || []) {
    for (const streak of listedStreaks(demo)) {
      if (!isHltvHighlight(demo, streak)) continue;
      const name = String(streak.target_player || '').trim();
      const id = streak.steam_id || (name ? nameId(name) : null);
      if (id) entries.push({ id, name, demo, streak });
    }
  }
  return entries;
}

/**
 * Everyone with a highlight in the queue's HLTV demos, one entry per
 * SteamID (`{ id, names, label }`, sorted as the Player filter is). Players
 * with no highlight there are left out: there is nothing of theirs to review.
 */
export function hltvDemoPlayers(demos) {
  const grouped = groupPlayers(hltvEntries(demos).map(({ id, name }) => ({ id, name })));
  const steamNames = new Set(
    grouped.filter((p) => !p.id.startsWith('name:')).flatMap((p) => p.names.map((n) => n.toLowerCase())),
  );
  return grouped.filter((p) => !p.id.startsWith('name:') || !steamNames.has(p.names[0]?.toLowerCase()));
}

/** Whether `name` carries the clan tag `tag` at its start or end, set off
 *  from the rest by a space or a symbol (`DICE | stealth`, `[dicE]m00cat`,
 *  `m00cat.dicE`), so the tag `over` doesn't take `overlord`. */
export function nameCarriesTag(name, tag) {
  const n = String(name || '').trim().toLowerCase().replace(/^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu, '');
  const t = String(tag || '').toLowerCase();
  if (!n || !t) return false;
  if (n === t) return true;
  const boundary = (ch) => ch !== undefined && !/[\p{L}\p{N}]/u.test(ch);
  return (n.startsWith(t) && boundary(n[t.length])) || (n.endsWith(t) && boundary(n[n.length - t.length - 1]));
}

/**
 * The players of the Teams list's team `tag` (#445) among `players`
 * (hltvDemoPlayers): whoever played a highlight on the side that team had in
 * that HLTV demo, or whose name carries one of the team's tags (merged ones
 * too). The side counts for a teammate who left the tag off.
 */
export function teamPlayers(demos, projectTeams, tag, players = hltvDemoPlayers(demos)) {
  const team = rootTag(projectTeams, tag);
  const tags = Object.keys(projectTeams?.merged || {}).filter((t) => rootTag(projectTeams, t) === team);
  tags.push(team);
  const onSide = new Set();
  for (const { id, name, demo, streak } of hltvEntries(demos)) {
    const sideOf = streak.faction ? sideTag(demo, streak.faction) : null;
    if (sideOf && rootTag(projectTeams, sideOf) === team) {
      onSide.add(id);
      if (name) onSide.add(nameId(name));
    }
  }
  return players.filter((p) => onSide.has(p.id)
    || p.names.some((n) => onSide.has(nameId(n)) || tags.some((t) => nameCarriesTag(n, t))));
}

/** The Teams list's teams with at least one player in `players`, each with
 *  its players: `[{ tag, name, players }]`, as the Teams list orders them. */
export function hltvTeams(demos, projectTeams, players = hltvDemoPlayers(demos)) {
  return buildTeamsList(demos, projectTeams).rows
    .map((row) => ({ tag: row.tag, name: row.name, players: teamPlayers(demos, projectTeams, row.tag, players) }))
    .filter((t) => t.players.length > 0);
}

/** A fresh pick: the HLTV box on its default, nobody picked. */
export function emptyHltvPick() {
  return { choice: HLTV_PLAYERS.MISSING, players: [] };
}

/**
 * The pick from a project file's `review` (`{ hltv, hltv_players }`).
 * Tolerant: a project saved before #828 has none, and anything malformed is
 * dropped rather than failing the load.
 */
export function normalizeHltvPick(raw) {
  const pick = emptyHltvPick();
  if (typeof raw?.hltv === 'string' && raw.hltv) pick.choice = raw.hltv;
  const seen = new Set();
  for (const p of Array.isArray(raw?.hltv_players) ? raw.hltv_players : []) {
    if (!p || typeof p.id !== 'string' || !p.id || seen.has(p.id)) continue;
    seen.add(p.id);
    const names = (Array.isArray(p.names) ? p.names : []).filter((n) => typeof n === 'string' && n.trim());
    pick.players.push(withLabel({ id: p.id, names }));
  }
  return pick;
}

/** What the project file keeps of a pick. */
export function savedHltvPick(pick) {
  return { hltv: pick.choice, hltv_players: pick.players.map(({ id, names }) => ({ id, names })) };
}

/** A picked player as the Player filter would label them. */
function withLabel({ id, names }) {
  const [entry] = groupPlayers(names.length ? names.map((name) => ({ id, name })) : [{ id, name: '' }]);
  return { id, names, label: id.startsWith('name:') ? names.join(' / ') || id.slice(5) : entry.label };
}

/** `players` with `more` added, each id once; a player picked again takes
 *  the names the queue has for them now. Returns a new list. */
export function addPickedPlayers(players, more) {
  const list = [...players];
  for (const p of more || []) {
    const at = list.findIndex((q) => q.id === p.id);
    const entry = { id: p.id, names: [...p.names], label: p.label };
    if (at >= 0) list[at] = entry;
    else list.push(entry);
  }
  return list;
}

/** `players` without the one with `id`. Returns a new list. */
export function removePickedPlayer(players, id) {
  return players.filter((p) => p.id !== id);
}
