// detail_player_filter.js
// Highlight Details' Player filter (#247): a demo with no single recorder (an
// HLTV demo) lists every player's highlights, 150 or more for a half, too many
// to scroll through. The filter keeps one player's. Pure, so it can be tested
// on its own.

/** Every player with a highlight in `demo`: `[{ name, count }]`, most
 *  highlights first, then by name. */
export function playersOf(demo) {
  const counts = new Map();
  for (const s of demo?.streaks || []) {
    const name = s.target_player || '';
    counts.set(name, (counts.get(name) || 0) + 1);
  }
  return [...counts.entries()]
    .map(([name, count]) => ({ name, count }))
    .sort((a, b) => b.count - a.count || a.name.localeCompare(b.name));
}

/** Whether `streak` passes the filter `name` ('' keeps everyone). A name
 *  this demo doesn't have keeps everyone too, so a pick made on one demo
 *  never empties the table of another. */
export function matchesPlayer(demo, streak, name) {
  if (!name) return true;
  const inDemo = (demo?.streaks || []).some((s) => s.target_player === name);
  return !inDemo || streak.target_player === name;
}
