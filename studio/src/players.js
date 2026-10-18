// players.js
// Configuration > Players (#428), the pure half: the table's shape, who a
// typed or picked player is, adding, editing and removing rows, and the
// crosshair grid. players_ui.js draws it; the Rust side
// (native::player_overrides) saves it and mirrors it for the hook DLL, which
// applies it in demos (goldsrc-hooks/src/player_table.rs).
//
// A player is a SteamID64 when one is known, else a name (bots, players a
// demo carries no SteamID for, or one typed by name). The hook matches by
// SteamID first, then by name ignoring case. A row's empty spray or crosshair
// leaves that one to the fallback.

import { groupPlayers, steamIdText } from './player_filter.js';
import { steamIdForms } from './steam_ids.js';

/** Spray fallbacks offered for POV demos, and HLTV's: an HLTV demo records
 *  that a player sprayed, never the image, so "their own" isn't one. */
export const POV_FALLBACKS = ['own', 'png', 'none'];
export const HLTV_FALLBACKS = ['png', 'none'];
export const CROSSHAIR_FALLBACKS = ['game', 'sprite'];

/** The sprite DoD ships, whose tile `cl_xhair_style` picks. */
export const DEFAULT_CROSSHAIR_SPRITE = 'customXHair.spr';
/** DoD's crosshair grid: 4x4 tiles of 64x64 (docs/goldsrc_crosshair.md). */
export const TILE_SIZE = 64;
export const GRID = 4;
export const TILES = GRID * GRID;
/** A row's spray value for "From the demo" (native::player_overrides::OWN_SPRAY). */
export const OWN_SPRAY = 'own';
/** The spray sizes offered: the longer side of a spray built from a PNG. */
export const SPRAY_SIZES = [64, 96, 128];
export const DEFAULT_SPRAY_SIZE = 64;

/** The table as a settings file without one gets it (matches
 *  `PlayerTable::default()`). */
export function defaultPlayers() {
  return {
    use_in_game: true,
    players: [],
    pov_fallback: 'own',
    pov_fallback_png: '',
    hltv_fallback: 'png',
    hltv_fallback_png: '',
    crosshair_fallback: 'game',
    crosshair_fallback_sprite: DEFAULT_CROSSHAIR_SPRITE,
    crosshair_fallback_tile: 1,
    spray_size: DEFAULT_SPRAY_SIZE,
  };
}

function isFileName(name, extension) {
  const n = String(name || '').trim();
  return !!n && !/[\\/:\t]/.test(n) && n.toLowerCase().endsWith(extension);
}

/** A PNG name the hook accepts: a file in `logos\`, never a path. */
export function isPngName(name) {
  return isFileName(name, '.png');
}

/** A sprite name the hook accepts: a file in `sprites\`, never a path. */
export function isSpriteName(name) {
  return isFileName(name, '.spr');
}

const text = (v) => (typeof v === 'string' ? v.trim() : '');

/** A tile number 1..16, or 0 for none. */
export function cleanTile(v) {
  const n = Number(v);
  return Number.isInteger(n) && n >= 1 && n <= TILES ? n : 0;
}

/** Any saved value as a well-formed table; unknown choices fall back to
 *  the defaults, rows with nobody to match are dropped. */
export function normalizePlayers(raw) {
  const d = defaultPlayers();
  if (!raw || typeof raw !== 'object') return d;
  const players = (Array.isArray(raw.players) ? raw.players : [])
    .map((p) => ({
      steam_id: steamIdForms(text(p?.steam_id)) ? text(p.steam_id) : '',
      name: text(p?.name),
      png: text(p?.png),
      crosshair_sprite: text(p?.crosshair_sprite),
      crosshair_tile: cleanTile(p?.crosshair_tile),
    }))
    .filter((p) => p.steam_id || p.name);
  return {
    use_in_game: raw.use_in_game !== false,
    players,
    pov_fallback: POV_FALLBACKS.includes(raw.pov_fallback) ? raw.pov_fallback : d.pov_fallback,
    pov_fallback_png: text(raw.pov_fallback_png),
    // `own` from a hand-edited file reads as the HLTV default.
    hltv_fallback: HLTV_FALLBACKS.includes(raw.hltv_fallback) ? raw.hltv_fallback : d.hltv_fallback,
    hltv_fallback_png: text(raw.hltv_fallback_png),
    crosshair_fallback: CROSSHAIR_FALLBACKS.includes(raw.crosshair_fallback)
      ? raw.crosshair_fallback : d.crosshair_fallback,
    crosshair_fallback_sprite: text(raw.crosshair_fallback_sprite) || d.crosshair_fallback_sprite,
    crosshair_fallback_tile: cleanTile(raw.crosshair_fallback_tile) || d.crosshair_fallback_tile,
    spray_size: SPRAY_SIZES.includes(Number(raw.spray_size)) ? Number(raw.spray_size) : d.spray_size,
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
 * player, every name they were seen with in the label, like the queue's
 * player filter (player_filter.js).
 */
export function playerOptions(players) {
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

/** The table with `player` in it, and their row's index: their existing row
 *  (which keeps the name it was added with, or picks one up), or a new
 *  empty one at the end. */
export function addPlayer(table, player) {
  const t = normalizePlayers(table);
  const row = { steam_id: player?.steam_id || '', name: text(player?.name) };
  if (!row.steam_id && !row.name) return { table: t, index: -1 };
  const at = t.players.findIndex((p) => samePlayer(p, row));
  if (at >= 0) {
    t.players[at] = { ...t.players[at], name: t.players[at].name || row.name };
    return { table: t, index: at };
  }
  t.players.push({ ...row, png: '', crosshair_sprite: '', crosshair_tile: 0 });
  return { table: t, index: t.players.length - 1 };
}

/** The table with row `index`'s fields replaced by `patch`'s. */
export function updatePlayer(table, index, patch) {
  const t = normalizePlayers(table);
  if (!t.players[index]) return t;
  t.players[index] = normalizePlayers({ players: [{ ...t.players[index], ...patch }] }).players[0]
    || t.players[index];
  return t;
}

export function removePlayer(table, index) {
  const t = normalizePlayers(table);
  t.players.splice(index, 1);
  return t;
}

/** What a row shows: the name and the SteamID when there is one. */
export function playerLabel(row) {
  const steam = row.steam_id ? steamIdText(row.steam_id) : null;
  if (row.name && steam) return `${row.name} (${steam})`;
  return row.name || steam || row.steam_id;
}

/** Where tile `tile` (1..16) sits in the sprite, as DoD's own crosshair code
 *  works it out (`spectator_crosshair::tile_rect`): left to right, then top
 *  to bottom, 64x64 each. Null for anything else. */
export function tileRect(tile) {
  const t = cleanTile(tile);
  if (!t) return null;
  const i = t - 1;
  return { x: (i % GRID) * TILE_SIZE, y: Math.floor(i / GRID) * TILE_SIZE, w: TILE_SIZE, h: TILE_SIZE };
}

/** The PNGs the table names that `listed` (the files in `logos\`) lacks,
 *  each once. */
export function missingPngs(table, listed) {
  const have = new Set((listed || []).map((n) => n.toLowerCase()));
  const named = [
    ...table.players.map((p) => p.png).filter((p) => p !== OWN_SPRAY),
    table.pov_fallback === 'png' ? table.pov_fallback_png : '',
    table.hltv_fallback === 'png' ? table.hltv_fallback_png : '',
  ].filter(Boolean);
  return [...new Set(named)].filter((n) => !have.has(n.toLowerCase()));
}

/** The sprites the table names that `listed` (the files in `sprites\`)
 *  lacks, each once. */
export function missingSprites(table, listed) {
  const have = new Set((listed || []).map((n) => n.toLowerCase()));
  const named = [
    ...table.players.filter((p) => p.crosshair_tile).map((p) => p.crosshair_sprite),
    table.crosshair_fallback === 'sprite' ? table.crosshair_fallback_sprite : '',
  ].filter(Boolean);
  return [...new Set(named)].filter((n) => !have.has(n.toLowerCase()));
}
