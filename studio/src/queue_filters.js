// queue_filters.js
// The Master Queue's quick filters (#54), and the highlights they and the
// queue's counts read. Pure, so it can be tested on its own.

/**
 * The demo's highlights. Whose they are is decided once, by the scan
 * (`analysis::highlights`, the in-game Highlights tab's rule too: a POV
 * demo's recorder's, an HLTV demo's everyone's; #247 D18, #573), so the
 * frontend lists `demo.streaks` as it is and only narrows it by Min Kills and
 * the Player filter.
 */
export function listedStreaks(demo) {
  return demo.streaks || [];
}

/**
 * A project saved before #573 holds every player's highlights for each demo,
 * and the frontend kept the recorder's (`local_player_index`). Drops the
 * others once, at load, so the project lists what a new scan would. A demo
 * scanned since has `recorded_this_demo` on each highlight and is left alone.
 * Returns whether anything was dropped.
 */
export function dropOtherPlayersSavedStreaks(demo) {
  const streaks = demo?.streaks;
  const recorder = demo?.local_player_index;
  if (!Array.isArray(streaks) || recorder === null || recorder === undefined) return false;
  if (streaks.some((s) => s.recorded_this_demo !== undefined)) return false;
  const kept = streaks.filter((s) => s.player_index === recorder);
  kept.forEach((s) => { s.recorded_this_demo = true; });
  demo.streaks = kept;
  return kept.length !== streaks.length;
}

/** The Kills quick filter's values. */
export const KILLS_FILTER = {
  ALL: 'all',
  /** At least one kill by the recording player. */
  WITH_KILLS: 'with_kills',
  /** At least one highlight of two or more kills. */
  MULTI_KILL: 'multi_kill',
};

/**
 * Whether any of `demo`'s highlights is ticked for capture. Every ticked one
 * counts, even one Min Kills hides, since the capture batch takes it too.
 */
export function hasTickedHighlight(demo) {
  return (demo?.streaks || []).some((s) => s.selected === true);
}

/**
 * Whether `demo` passes the quick filters: `kills` (a KILLS_FILTER value),
 * `ownerOnly` (hide demos with no resolvable recording player) and
 * `tickedOnly` (hide demos with no highlight ticked, #127). `keepPath` is the
 * demo open in Highlight Details: Ticked only keeps it listed, so unticking
 * its last highlight doesn't pull the row out from under the user.
 */
export function matchesQuickFilters(demo, { kills = KILLS_FILTER.ALL, ownerOnly = false, tickedOnly = false, keepPath = null } = {}) {
  const hasOwner = demo.local_player_index !== null && demo.local_player_index !== undefined;
  if (ownerOnly && !hasOwner) return false;
  if (tickedOnly && !hasTickedHighlight(demo) && !(keepPath && demo.path === keepPath)) return false;
  if (kills === KILLS_FILTER.ALL) return true;
  const own = listedStreaks(demo);
  if (kills === KILLS_FILTER.WITH_KILLS) return own.some((s) => (s.kill_count || 0) > 0);
  if (kills === KILLS_FILTER.MULTI_KILL) return own.some((s) => (s.kill_count || 0) >= 2);
  return true;
}
