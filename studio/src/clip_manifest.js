// clip_manifest.js
// The clip manifest (#429): a JSON file saved beside the VEGAS EDL (#110)
// with the same clips plus whose highlight each one is and that player's
// nameplate picture. `vegas/DoD Studio Nameplates.cs` reads it to put each
// nameplate on its own track above its clip. The EDL itself is unchanged:
// CMX 3600 has nowhere to put these fields that every editor would ignore,
// so they live in a file of their own. Pure, so it can be tested on its own.
//
// The format only grows: a reader ignores fields it doesn't know, and a new
// field never changes what an old one means. `version` goes up only when a
// field changes meaning.

import { streakUid } from './take_index.js';

/** What `format` says, so a reader can tell the file is one of these. */
export const MANIFEST_FORMAT = 'dodstudio-clip-manifest';
export const MANIFEST_VERSION = 1;
/** The end of a manifest's file name: `<edl name>.dodstudio.json`. */
export const MANIFEST_SUFFIX = '.dodstudio.json';

/** The manifest's path for an EDL saved at `edlPath`: beside it, the same
 *  name with `.edl` swapped for MANIFEST_SUFFIX. */
export function manifestPathFor(edlPath) {
  return String(edlPath).replace(/\.edl$/i, '') + MANIFEST_SUFFIX;
}

/** Each highlight of `demos` by its uid, for looking takes up. */
function streaksByUid(demos) {
  const byUid = new Map();
  (demos || []).forEach((demo) => {
    (demo.streaks || []).forEach((streak) => byUid.set(streakUid(demo.path, streak), streak));
  });
  return byUid;
}

/**
 * Whose highlight the take `takeKey` holds: `{ steam_id, in_game_name }`,
 * from the take index and the loaded demos. Null when the take was never
 * recorded, its highlights aren't loaded, or it holds more than one
 * player's (an overlap merge of two players' highlights), since one clip
 * gets one nameplate.
 */
export function takePlayer(takeKey, takeIndex, byUid) {
  const uids = (takeKey && takeIndex?.[takeKey]) || [];
  const players = new Map();
  uids.forEach((uid) => {
    const streak = byUid.get(uid);
    if (!streak) return;
    const steamId = String(streak.steam_id ?? '').trim();
    const name = String(streak.target_player ?? '').trim();
    const key = steamId || `name:${name}`;
    if (steamId || name) players.set(key, { steam_id: steamId, in_game_name: name });
  });
  return players.size === 1 ? [...players.values()][0] : null;
}

/**
 * The manifest as an object, ready for JSON.
 * @param {Array<{name, path, seconds, take_key}>} clips  `edlClips`' clips,
 *   in timeline order
 * @param {object} ctx  `edlName` the EDL's file name; `takeIndex` and
 *   `demos` the project's; `useAsName(steamId)` and `nameplatePath(steamId)`
 *   the Players tab's lookups (player_alias.js)
 * @returns {object}  `{ format, version, edl, clips: [{ name, path, seconds,
 *   player: { steam_id, name, in_game_name } | null, nameplate: path | null }] }`
 */
export function clipManifest(clips, { edlName = '', takeIndex = {}, demos = [], useAsName, nameplatePath }) {
  const byUid = streaksByUid(demos);
  return {
    format: MANIFEST_FORMAT,
    version: MANIFEST_VERSION,
    edl: edlName,
    clips: (clips || []).map((clip) => {
      const who = takePlayer(clip.take_key, takeIndex, byUid);
      const player = who ? {
        steam_id: who.steam_id,
        // The Use as name when set, as clip names show it (#686).
        name: (who.steam_id && useAsName?.(who.steam_id)) || who.in_game_name,
        in_game_name: who.in_game_name,
      } : null;
      return {
        name: clip.name,
        path: clip.path,
        seconds: clip.seconds,
        player,
        nameplate: (who?.steam_id && nameplatePath?.(who.steam_id)) || null,
      };
    }),
  };
}

/** How many of the manifest's clips have a nameplate, for the toast. */
export function nameplateCount(manifest) {
  return (manifest?.clips || []).filter((c) => c.nameplate).length;
}
