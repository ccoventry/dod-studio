// queue_filters.js
// The Master Queue's quick filters (#54), and the recording-player rule they
// share with the queue's counts. Pure, so it can be tested on its own.

/**
 * Streaks belong to whichever player got the kills, not just the demo's
 * recording player — `demo.streaks` covers every player in the match. The
 * Highlight Details table (detail_pane.js) filters down to the recording
 * player's own streaks before displaying rows; the queue's counts mirror
 * that same filter so they agree with what the table actually shows.
 *
 * Gate on whether local_player_index actually resolved, not on demo.is_pov
 * — is_pov reflects any SvcHltv/SvcDirector message anywhere in the file,
 * which also fires on an ordinary player-recorded demo whenever an HLTV
 * caster was merely spectating the live match (server-broadcast messages
 * every connected client picks up), so it's not a reliable "no single
 * owner" signal. True HLTV proxy files are already rejected earlier in the
 * pipeline (scan_demo_for_highlights), so None here means "no resolvable
 * owner", not "is_pov".
 */
export function recordingPlayerStreaks(demo) {
  const streaks = demo.streaks || [];
  const recPlayer = demo.local_player_index;
  if (recPlayer === null || recPlayer === undefined) return streaks;

  return streaks.filter((s) => s.player_index === recPlayer);
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
 * Whether `demo` passes the quick filters: `kills` (a KILLS_FILTER value)
 * and `ownerOnly` (hide demos with no resolvable recording player).
 */
export function matchesQuickFilters(demo, { kills = KILLS_FILTER.ALL, ownerOnly = false } = {}) {
  const hasOwner = demo.local_player_index !== null && demo.local_player_index !== undefined;
  if (ownerOnly && !hasOwner) return false;
  if (kills === KILLS_FILTER.ALL) return true;
  const own = recordingPlayerStreaks(demo);
  if (kills === KILLS_FILTER.WITH_KILLS) return own.some((s) => (s.kill_count || 0) > 0);
  if (kills === KILLS_FILTER.MULTI_KILL) return own.some((s) => (s.kill_count || 0) >= 2);
  return true;
}
