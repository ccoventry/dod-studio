// sprays.js
// Configuration > Sprays (#428), the pure half: the table's shape, who a
// typed or picked player is, and adding and removing rows. sprays_ui.js
// draws it; the Rust side (native::sprays) saves it and mirrors it for the
// hook DLL, which applies it in demos (goldsrc-hooks/src/spray_table.rs).
//
// A player is a SteamID64 when one is known, else a name (bots, players a
// demo carries no SteamID for, or one typed by name). The hook matches by
// SteamID first, then by name ignoring case.

import { groupPlayers, steamIdText } from './player_filter.js';
import { steamIdForms } from './steam_ids.js';

/** POV fallbacks offered, and HLTV's: an HLTV demo records that a player
 *  sprayed, never the image, so "their own" isn't one. */
export const POV_FALLBACKS = ['own', 'png', 'none'];
export const HLTV_FALLBACKS = ['png', 'none'];

/** The table as a settings file without one gets it (matches
 *  `SprayTable::default()`). */
export function defaultSprays() {
  return {
    players: [],
    pov_fallback: 'own',
    pov_fallback_png: '',
    hltv_fallback: 'png',
    hltv_fallback_png: '',
  };
}

/** A PNG name the hook accepts: a file in `logos\`, never a path. */
export function isPngName(name) {
  const n = String(name || '').trim();
  return !!n && !/[\\/:\t]/.test(n) && n.toLowerCase().endsWith('.png');
}

const text = (v) => (typeof v === 'string' ? v.trim() : '');

/** Any saved value as a well-formed table; unknown choices fall back to
 *  the defaults, rows with nobody to match are dropped. */
export function normalizeSprays(raw) {
  const d = defaultSprays();
  if (!raw || typeof raw !== 'object') return d;
  const players = (Array.isArray(raw.players) ? raw.players : [])
    .map((p) => ({
      steam_id: steamIdForms(text(p?.steam_id)) ? text(p.steam_id) : '',
      name: text(p?.name),
      png: text(p?.png),
    }))
    .filter((p) => p.steam_id || p.name);
  return {
    players,
    pov_fallback: POV_FALLBACKS.includes(raw.pov_fallback) ? raw.pov_fallback : d.pov_fallback,
    pov_fallback_png: text(raw.pov_fallback_png),
    // `own` from a hand-edited file reads as the HLTV default.
    hltv_fallback: HLTV_FALLBACKS.includes(raw.hltv_fallback) ? raw.hltv_fallback : d.hltv_fallback,
    hltv_fallback_png: text(raw.hltv_fallback_png),
  };
}

/** A typed SteamID in any of its three spellings, as the SteamID64 string,
 *  or null. */
export function parseSteamId(input) {
  const t = text(input);
  if (steamIdForms(t)) return t;
  const classic = /^STEAM_[0-5]:([01]):(\d+)$/i.exec(t);
  const id3 = /^\[U:1:(\d+)\]$/i.exec(t);
  let account = null;
  if (classic) account = BigInt(classic[2]) * 2n + BigInt(classic[1]);
  else if (id3) account = BigInt(id3[1]);
  if (account === null) return null;
  const id = String(76561197960265728n + account);
  return steamIdForms(id) ? id : null;
}

/**
 * The player picker's options from the players of the queue's demos
 * (`{ id, name }` from the player index, any number of demos): one per
 * player, every name he was seen with in the label, like the queue's
 * player filter (player_filter.js).
 */
export function sprayPlayerOptions(players) {
  return groupPlayers(players).map((o) => ({
    steam_id: steamIdText(o.id) ? o.id : '',
    name: o.names[0] || '',
    label: o.label,
  })).filter((o) => o.steam_id || o.name);
}

/** Who the player box means: an option by its exact label, a SteamID, or
 *  else a name. Null for empty text. */
export function playerFromInput(input, options) {
  const t = text(input);
  if (!t) return null;
  const option = (options || []).find((o) => o.label === t);
  if (option) return { steam_id: option.steam_id, name: option.name };
  const steam = parseSteamId(t);
  if (steam) return { steam_id: steam, name: '' };
  return { steam_id: '', name: t };
}

/** Whether two rows are the same player: same SteamID, or both without
 *  one and the same name ignoring case. */
export function samePlayer(a, b) {
  if (a.steam_id || b.steam_id) return !!a.steam_id && a.steam_id === b.steam_id;
  return a.name.toLowerCase() === b.name.toLowerCase();
}

/** The table with `player` given `png`: his row updated if he has one, or
 *  a new row at the end. */
export function setPlayerSpray(table, player, png) {
  const t = normalizeSprays(table);
  const row = { steam_id: player.steam_id || '', name: text(player.name), png: text(png) };
  if (!row.steam_id && !row.name) return t;
  const at = t.players.findIndex((p) => samePlayer(p, row));
  if (at >= 0) {
    // The row keeps the name it was added with; a SteamID row picks one up
    // if it had none.
    t.players[at] = { ...t.players[at], png: row.png, name: t.players[at].name || row.name };
  } else {
    t.players.push(row);
  }
  return t;
}

export function removePlayer(table, index) {
  const t = normalizeSprays(table);
  t.players.splice(index, 1);
  return t;
}

/** What a row shows: the name and the SteamID when there is one. */
export function playerLabel(row) {
  const steam = row.steam_id ? steamIdText(row.steam_id) : null;
  if (row.name && steam) return `${row.name} (${steam})`;
  return row.name || steam || row.steam_id;
}

/** The PNGs the table names that `listed` (the files in `logos\`) lacks,
 *  each once. */
export function missingPngs(table, listed) {
  const have = new Set((listed || []).map((n) => n.toLowerCase()));
  const named = [
    ...table.players.map((p) => p.png),
    table.pov_fallback === 'png' ? table.pov_fallback_png : '',
    table.hltv_fallback === 'png' ? table.hltv_fallback_png : '',
  ].filter(Boolean);
  return [...new Set(named)].filter((n) => !have.has(n.toLowerCase()));
}
