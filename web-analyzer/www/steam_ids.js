// The three ways a SteamID is written (#536), from a player's SteamID64.
// A copy of studio/src/steam_ids.js (#238).
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
  return `ds_hud_deathmsg block !${forms.id64}`;
}

const NOT_A_STEAM_ID = Object.freeze({ ok: false, reason: 'not_a_steam_id' });

/** The 32-bit account number a pasted SteamID names, or null. Accepts the
 *  classic form (`STEAM_0:1:6155141`, universe 0 or 1: older games print 0
 *  for the public universe, newer ones 1), SteamID3 (`[U:1:12310283]`, the
 *  brackets optional) and SteamID64, with whitespace around it. Any other
 *  universe or account type is not a player to add. */
function accountFromText(input) {
  if (typeof input !== 'string') return null;
  const t = input.trim();
  const classic = /^STEAM_[01]:([01]):(\d{1,10})$/i.exec(t);
  if (classic) return BigInt(classic[2]) * 2n + BigInt(classic[1]);
  const id3 = /^(?:\[U:1:(\d{1,10})\]|U:1:(\d{1,10}))$/i.exec(t);
  if (id3) return BigInt(id3[1] ?? id3[2]);
  if (/^\d{15,20}$/.test(t)) return BigInt(t) - INDIVIDUAL_BASE;
  return null;
}

/** What a person pasted as a SteamID, in any of the three forms: `{ ok: true,
 *  id64, classic, id3 }`, or `{ ok: false, reason: 'not_a_steam_id' }`.
 *  Never throws. `id64` is the canonical form the Players table is keyed
 *  by. Account 0 and anything past 32 bits are refused, as in
 *  `steamIdForms`. */
export function parseSteamIdInput(input) {
  const account = accountFromText(input);
  if (account === null || account <= 0n || account >= ACCOUNT_LIMIT) return NOT_A_STEAM_ID;
  const forms = steamIdForms(String(INDIVIDUAL_BASE + account));
  return forms ? { ok: true, ...forms } : NOT_A_STEAM_ID;
}

/** Converts a pasted SteamID to one form (`'id64'`, `'classic'` or `'id3'`),
 *  or null when it isn't a SteamID. */
export function convertSteamId(input, form) {
  const parsed = parseSteamIdInput(input);
  return parsed.ok && form !== 'ok' && Object.hasOwn(parsed, form) ? parsed[form] : null;
}
