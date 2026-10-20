// command_players.js
// The Players a Scheduled Command is limited to (a crosshair per player in a
// team movie). Pure, so it can be tested on its own; command_players_ui.js is
// the row's chips and picker. The batch itself decides which clips a command
// runs around (`native::patch::command_players`): by SteamID when both sides
// carry one, else by name, ignoring case. An empty list is everyone.
//
// A player is stored as `{ id, name }`: `id` a SteamID64 or null, `name` what
// the chip shows and what a clip without a SteamID is matched by.

import { groupPlayers, steamIdText } from './player_filter.js';

const STEAM64_BASE = 76561197960265728n;

/** SteamID64 for a SteamID64 or `STEAM_X:Y:Z` text, else null. */
export function steamId64(text) {
  const t = String(text ?? '').trim();
  if (/^\d{17}$/.test(t)) return steamIdText(t) ? t : null;
  const m = /^STEAM_[0-5]:([01]):(\d{1,10})$/i.exec(t);
  if (!m) return null;
  return String(STEAM64_BASE + BigInt(m[2]) * 2n + BigInt(m[1]));
}

/** `list` cleaned up for saving: trimmed, entries with neither id nor name
 *  dropped, one entry per player (by id, else by name). */
export function normalisePlayers(list) {
  if (!Array.isArray(list)) return [];
  const seen = new Set();
  const out = [];
  for (const p of list) {
    const id = steamId64(p?.id);
    const name = String(p?.name ?? '').trim();
    if (!id && !name) continue;
    const key = id || `name:${name.toLowerCase()}`;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push({ id, name });
  }
  return out;
}

/** What a chip says: the name, or the SteamID for an entry without one. */
export function playerChipLabel(player) {
  return player?.name || steamIdText(player?.id) || player?.id || '';
}

/**
 * The picker's options: everyone the queue's clips can be of. A POV demo's
 * clips are its recorder's, an HLTV demo's are the players its highlights
 * follow; `recorderOf(demo)` covers a POV demo with no highlights listed yet.
 * Grouped like the queue's player filter (`groupPlayers`), each option
 * carrying the `{ id, name }` to store.
 */
export function queuePlayerOptions(demos, recorderOf) {
  const seen = [];
  for (const demo of demos || []) {
    const recorder = recorderOf?.(demo);
    if (recorder) seen.push({ id: recorder.id, name: recorder.name });
    for (const s of demo?.streaks || []) {
      if (s?.target_player || s?.steam_id) seen.push({ id: s.steam_id, name: s.target_player });
    }
  }
  // groupPlayers keys on an id; a player with no SteamID is keyed by name so
  // the same name from two demos is one option.
  const keyed = seen.map((p) => {
    const id = steamId64(p.id);
    const name = String(p.name || '').trim();
    return { id: id || `name:${name.toLowerCase()}`, name };
  }).filter((p) => p.id !== 'name:');
  return groupPlayers(keyed).map((o) => ({
    ...o,
    player: { id: steamId64(o.id), name: o.names[0] || '' },
  }));
}

/** The player typed or picked: an option by its label, a SteamID (named
 *  after the option with that id, if any), else a name. Null when empty. */
export function playerFromText(text, options) {
  const t = String(text ?? '').trim();
  if (!t) return null;
  const list = options || [];
  const exact = list.find((o) => o.label === t);
  if (exact) return { ...exact.player };
  const id = steamId64(t);
  if (id) {
    const known = list.find((o) => o.player.id === id);
    return { id, name: known?.player.name || '' };
  }
  const byName = list.find((o) => o.names.some((n) => n.toLowerCase() === t.toLowerCase()));
  if (byName) return { ...byName.player };
  return { id: null, name: t };
}

/** `players` with `player` added, unless it is already there. */
export function addPlayer(players, player) {
  return normalisePlayers([...(players || []), player]);
}
