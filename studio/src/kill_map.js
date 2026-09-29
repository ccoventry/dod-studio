// kill_map.js
//
// The arithmetic behind the Demo Analyzer's Kill Map tab (#448): placing a
// world position on a map's overview image, and summing engagement distances.
// Pure, so it is tested without a DOM (kill_map.test.js).

// World units to metres. GoldSrc's unit is conventionally an inch.
export const METRES_PER_UNIT = 0.0254;

export function unitsToMetres(units) {
  return units * METRES_PER_UNIT;
}

// DoD's client lays the overview out with a fixed 1.33 aspect, whatever the
// screen (CHudDoDMap::DrawOverviewLayer); stock HL uses the screen's.
const OVERVIEW_ASPECT = 1.33;

// Where a world position lands on the overview image, as fractions of its
// width (u) and height (v); 0..1 is on the image.
//
// From DoD's own client (dod_map.cpp, DrawOverviewLayer/DrawOverviewEntities):
// the image spans 8192/zoom world units, centred on ORIGIN, with the image's
// top edge at +X and its left edge at +Y -- up is +X, right is -Y. A ROTATED
// map first turns the position 90 degrees about ORIGIN:
//   x' = ox + (y - oy),  y' = oy - (x - ox)
// Checked against real deaths on dod_anzio (not rotated) and dod_saints2_b2
// (rotated): every one lands in a street or a room, none inside a wall.
export function worldToOverview(origin, placement) {
  const [ox, oy] = placement.origin;
  const zoom = placement.zoom || 1;
  let [x, y] = origin;
  if (placement.rotated) {
    [x, y] = [ox + (y - oy), oy - (x - ox)];
  }
  const span = 8192 / zoom;
  return {
    u: 0.5 + (oy - y) / span,
    v: 0.5 + ((ox - x) * OVERVIEW_ASPECT) / span,
  };
}

// Per weapon: how many kills have a measured distance, the average, and the
// longest -- teamkills and kills without both positions left out. Sorted by
// count, then name.
export function engagementByWeapon(kills) {
  const byWeapon = new Map();
  for (const k of kills || []) {
    if (k.teamkill || typeof k.distance !== 'number') continue;
    const e = byWeapon.get(k.weapon) || { weapon: k.weapon, count: 0, total: 0, longest: 0 };
    e.count += 1;
    e.total += k.distance;
    e.longest = Math.max(e.longest, k.distance);
    byWeapon.set(k.weapon, e);
  }
  return [...byWeapon.values()]
    .map((e) => ({ weapon: e.weapon, count: e.count, average: e.total / e.count, longest: e.longest }))
    .sort((a, b) => b.count - a.count || String(a.weapon).localeCompare(String(b.weapon)));
}

// The same over every weapon: { count, average, longest }, or null with none.
export function engagementOverall(kills) {
  const rows = engagementByWeapon(kills);
  const count = rows.reduce((s, r) => s + r.count, 0);
  if (count === 0) return null;
  return {
    count,
    average: rows.reduce((s, r) => s + r.average * r.count, 0) / count,
    longest: Math.max(...rows.map((r) => r.longest)),
  };
}
