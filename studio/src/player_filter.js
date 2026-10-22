// player_filter.js
// Filtering demos by player (#437 in the Demo Analyzer, #174 in the Master
// Queue). Pure, so it can be tested on its own.
//
// A player is identified by the analyzer's global id: a SteamID64 when the
// demo carries one (`*sid`), otherwise `PLAYER_<fid>` or a per-demo
// `CONNECTION_<n>`. Names are not identity -- 15 of 55 players changed name
// during one event (#429) -- so options group every name an id was seen with.

/** SteamID64 -> STEAM_0:Y:Z, or null for anything that isn't one. */
export function steamIdText(id) {
  if (!/^\d{17}$/.test(String(id || ''))) return null;
  const account = BigInt(id) - 76561197960265728n;
  if (account < 0n) return null;
  return `STEAM_0:${account % 2n}:${account / 2n}`;
}

/**
 * The options for a player picker: one per id, every name it was seen with
 * (most-used first), sorted by that first name. `players` is a flat list of
 * `{ id, name }` from any number of demos.
 */
export function groupPlayers(players) {
  const byId = new Map();
  for (const p of players || []) {
    if (!p || !p.id) continue;
    let entry = byId.get(p.id);
    if (!entry) {
      entry = { id: p.id, counts: new Map() };
      byId.set(p.id, entry);
    }
    const name = String(p.name || '').trim();
    if (name) entry.counts.set(name, (entry.counts.get(name) || 0) + 1);
  }
  return [...byId.values()]
    .map(({ id, counts }) => {
      const names = [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).map(([n]) => n);
      const steam = steamIdText(id);
      const shown = names.length ? names.join(' / ') : id;
      return { id, names, label: steam ? `${shown} (${steam})` : shown };
    })
    .sort((a, b) => a.label.toLowerCase().localeCompare(b.label.toLowerCase()));
}

/**
 * What the player box asks for: an exact option (by its label) means that
 * id; anything else is a case-insensitive search over names and SteamIDs.
 * Empty text means no player filter.
 */
export function parsePlayerQuery(text, options) {
  const trimmed = String(text || '').trim();
  if (!trimmed) return null;
  const exact = (options || []).find((o) => o.label === trimmed);
  if (exact) return { id: exact.id };
  return { text: trimmed.toLowerCase() };
}

/** The player in `players` that `query` picks out, or null. */
export function findPlayer(players, query) {
  if (!query) return null;
  const list = players || [];
  if (query.id) return list.find((p) => p.id === query.id) || null;
  return list.find((p) => {
    const steam = steamIdText(p.id) || '';
    return String(p.name || '').toLowerCase().includes(query.text)
      || String(p.id).toLowerCase() === query.text
      || steam.toLowerCase() === query.text;
  }) || null;
}
