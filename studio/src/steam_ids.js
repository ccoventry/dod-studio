// The three ways a SteamID is written (#536), from a player's SteamID64.
// Pure arithmetic, no DOM, so the web analyzer can copy it as is.

// SteamID64 of account 0 in the public universe: individual type, desktop
// instance. A real player's id is this plus their 32-bit account number.
const INDIVIDUAL_BASE = 76561197960265728n;
const ACCOUNT_LIMIT = 1n << 32n;

/** `{ id64, classic, id3 }` for a real player's SteamID64, or `null` for
 *  anything else: bots and `PLAYER_<n>` stand-ins (not numeric), and ids
 *  that are numbers but not a user account, such as the HLTV proxy's
 *  `90071996842377216`. Those get no made-up classic or SteamID3 form. */
export function steamIdForms(id) {
  if (typeof id !== 'string' || !/^\d{15,20}$/.test(id)) return null;
  let account;
  try {
    account = BigInt(id) - INDIVIDUAL_BASE;
  } catch {
    return null;
  }
  if (account <= 0n || account >= ACCOUNT_LIMIT) return null;
  return {
    id64: id,
    classic: `STEAM_0:${account % 2n}:${account / 2n}`,
    id3: `[U:1:${account}]`,
  };
}

/** The console line that hides every frag in the kill feed except this
 *  player's. The SteamID64 has no `:`, so it reaches the hook unsplit. */
export function deathmsgShowOnlyLine(forms) {
  return `dodstudio_deathmsg block !${forms.id64}`;
}
