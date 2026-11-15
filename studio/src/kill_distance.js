// kill_distance.js
//
// Engagement distance per kill and its averages (#448). The analysis records
// `kill_positions` with the straight-line distance in world units, null when
// either player's position was not in the demo (a POV demo only knows the
// enemies its recorder could see). Pure, no imports, so the web analyzer
// copies this file as it is and Vitest pins it (kill_distance.test.js).

// World units to metres. GoldSrc's unit is conventionally an inch.
export const METRES_PER_UNIT = 0.0254;

export function unitsToMetres(units) {
  return units * METRES_PER_UNIT;
}

function killKey(killer, victim, time) {
  const t = time?.viewdemo_offset;
  return `${killer}|${victim}|${t?.secs ?? 0}.${t?.nanos ?? 0}`;
}

/**
 * A lookup from a kill in a player's `kill_streaks` ([time, weapon, victim]) to
 * its distance in world units: `(killerId, victimId, time) => number | null`.
 * Streak kills and `kill_positions` entries are made from the same death
 * message, so killer, victim and demo time identify one kill.
 */
export function killDistanceLookup(kills) {
  const byKill = new Map();
  for (const k of kills || []) {
    if (k.teamkill || !k.killer || !k.victim || typeof k.distance !== 'number') continue;
    byKill.set(killKey(k.killer, k.victim, k.time), k.distance);
  }
  return (killer, victim, time) => byKill.get(killKey(killer, victim, time)) ?? null;
}

/**
 * Over a list of distances (a number, or null for an unknown position):
 * `{ measured, total, average }`, the average in world units over the known
 * ones, or null with none.
 */
export function distanceSummary(distances) {
  const known = distances.filter((d) => typeof d === 'number');
  return {
    measured: known.length,
    total: distances.length,
    average: known.length ? known.reduce((a, b) => a + b, 0) / known.length : null,
  };
}

/**
 * One player's kills of other players (team kills left out) from
 * `kill_positions`: `{ all, byWeapon }`, each a `distanceSummary`; `byWeapon`
 * is a Map from weapon to its summary.
 */
export function playerDistanceSummaries(kills, playerId) {
  const mine = (kills || []).filter((k) => k.killer === playerId && !k.teamkill);
  const perWeapon = new Map();
  for (const k of mine) {
    const list = perWeapon.get(k.weapon) || [];
    list.push(typeof k.distance === 'number' ? k.distance : null);
    perWeapon.set(k.weapon, list);
  }
  return {
    all: distanceSummary(mine.map((k) => (typeof k.distance === 'number' ? k.distance : null))),
    byWeapon: new Map([...perWeapon].map(([w, list]) => [w, distanceSummary(list)])),
  };
}
