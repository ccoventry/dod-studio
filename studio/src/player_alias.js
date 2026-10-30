// player_alias.js
// The "Use as" name (#686): what {name} and {player} show for a player in a
// clip or demo name, set on the Players tab and keyed by SteamID64 so a player
// who changes names keeps one alias.
//
// Every template site asks `useAsName(steamId)` and nothing else. The answer
// comes from the table `setUseAsTable` was last given (the app's Players
// table); when projects get tables of their own (#1085) they sit in front of
// it by changing what `setUseAsTable` is fed, with no template code touched.

/** A saved Use as, trimmed; empty means unset. */
export function cleanUseAs(value) {
  return typeof value === 'string' ? value.trim() : '';
}

/** `steamId`'s Use as in `table` (a Players table), or null when unset or
 *  when the player has no SteamID. Rows without a SteamID are matched by name
 *  in the game, but an alias follows the SteamID only. */
export function useAsFromTable(table, steamId) {
  const id = String(steamId ?? '').trim();
  if (!id) return null;
  const row = (table?.players || []).find((p) => p.steam_id === id);
  return cleanUseAs(row?.use_as) || null;
}

let source = null;
const listeners = new Set();

/** Gives the lookup the table to read, and tells whoever shows a name built
 *  from it (the previews) to redraw. */
export function setUseAsTable(table) {
  source = table || null;
  listeners.forEach((fn) => fn());
}

/** The Use as name for `steamId`, or null. The one lookup templates use. */
export function useAsName(steamId) {
  return useAsFromTable(source, steamId);
}

/** Calls `fn` whenever the table changes. */
export function onUseAsChange(fn) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}
