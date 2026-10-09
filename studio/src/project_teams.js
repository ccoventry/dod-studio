// project_teams.js
//
// The project's Teams list (#445): every clan tag the scan found across the
// queue's demos, with the display name the user gave it and which tags they
// merged into one team. Pure, no DOM.
//
// A scanned demo carries `teams: [{ side, tag }]` (analysis::team_tags, one
// entry per playing side, `tag` null when the side's names share none). A
// demo from a project saved before that existed has no `teams` at all.
//
// The user's part is project state, not demo state, so a re-scan can't touch
// it: `{ names: { tag: displayName }, merged: { tag: intoTag } }`. Plain
// objects, so it round-trips through the project file's JSON as it is. The
// detected tag stays the key; a name or a merge on a tag no demo has any
// more is kept, so it comes back if that demo does.

/** A fresh, empty Teams state. */
export function emptyProjectTeams() {
  return { names: {}, merged: {} };
}

/**
 * The Teams state from a loaded project file. Tolerant: a project saved
 * before the Teams list existed has none, and anything malformed is dropped
 * rather than failing the load.
 */
export function normalizeProjectTeams(raw) {
  const teams = emptyProjectTeams();
  for (const key of ['names', 'merged']) {
    const source = raw?.[key];
    if (!source || typeof source !== 'object' || Array.isArray(source)) continue;
    for (const [tag, value] of Object.entries(source)) {
      if (tag && typeof value === 'string' && value) teams[key][tag] = value;
    }
  }
  return teams;
}

/** Whether this demo's teams were read (false for one scanned before #445). */
export function demoHasTeams(demo) {
  return Array.isArray(demo?.teams);
}

/**
 * The tag a tag counts as once merges are followed: itself unless it was
 * merged into another. Stops on a loop instead of spinning.
 */
export function rootTag(projectTeams, tag) {
  const seen = new Set();
  let current = tag;
  while (projectTeams?.merged?.[current] && !seen.has(current)) {
    seen.add(current);
    current = projectTeams.merged[current];
  }
  return current;
}

/** The name a tag goes by: its team's display name, else the team's tag. */
export function teamName(projectTeams, tag) {
  if (!tag) return null;
  const root = rootTag(projectTeams, tag);
  return projectTeams?.names?.[root] || root;
}

/**
 * The Teams list: one row per team (a tag and whatever was merged into it),
 * most demos first. `unread` counts demos scanned before teams were read.
 *
 * Row: `{ tag, name, customName, demoCount, demoNames, merged }`, where
 * `name` is what the team goes by and `merged` the other tags folded in.
 */
export function buildTeamsList(demos, projectTeams) {
  const rows = new Map();
  let unread = 0;
  for (const demo of demos || []) {
    if (!demoHasTeams(demo)) {
      unread += 1;
      continue;
    }
    for (const { tag } of demo.teams) {
      if (!tag) continue;
      const root = rootTag(projectTeams, tag);
      if (!rows.has(root)) rows.set(root, { tag: root, merged: new Set(), demos: new Set() });
      const row = rows.get(root);
      if (tag !== root) row.merged.add(tag);
      row.demos.add(demo.name || demo.path);
    }
  }
  const list = [...rows.values()].map((row) => ({
    tag: row.tag,
    name: teamName(projectTeams, row.tag),
    customName: !!projectTeams?.names?.[row.tag],
    demoCount: row.demos.size,
    demoNames: [...row.demos].sort(),
    merged: [...row.merged].sort(),
  }));
  list.sort((a, b) => b.demoCount - a.demoCount || a.tag.localeCompare(b.tag));
  return { rows: list, unread };
}

/**
 * Gives a team a display name. A blank name, or one that is just the tag,
 * goes back to showing the tag. Returns whether anything changed.
 */
export function renameTeam(projectTeams, tag, name) {
  const root = rootTag(projectTeams, tag);
  const trimmed = (name || '').trim();
  const before = projectTeams.names[root];
  if (!trimmed || trimmed === root) delete projectTeams.names[root];
  else projectTeams.names[root] = trimmed;
  return projectTeams.names[root] !== before;
}

/**
 * Folds `tag`'s team into `intoTag`'s (a spelling change, a re-tag). The
 * merged team keeps `intoTag`'s name; `tag`'s own name is kept for if it is
 * split out again. Returns whether anything changed.
 */
export function mergeTeam(projectTeams, tag, intoTag) {
  const target = rootTag(projectTeams, intoTag);
  if (!tag || !target || rootTag(projectTeams, tag) === target || target === tag) return false;
  projectTeams.merged[tag] = target;
  return true;
}

/** Splits a merged tag back out into a team of its own. */
export function unmergeTeam(projectTeams, tag) {
  if (!projectTeams.merged[tag]) return false;
  delete projectTeams.merged[tag];
  return true;
}

// DoD's Allied side is the Allies or the British, never both in one demo.
function sideKey(side) {
  if (side === 'Axis') return 'Axis';
  if (side === 'Allies' || side === 'British') return 'Allied';
  return null;
}

/** The tag detected for `side` ("Allies", "British" or "Axis") in a demo. */
export function sideTag(demo, side) {
  const key = sideKey(side);
  if (!key || !demoHasTeams(demo)) return null;
  return demo.teams.find((t) => sideKey(t.side) === key)?.tag || null;
}

/**
 * The team placeholders of a highlight's clip name (#441): `team_name` is
 * the display name of the highlight player's team, `opponent` that of the
 * first victim's team (in the chosen Kill Range). `null` when a side has no
 * tag, so the clip name gives `unknown` or the template's own fallback word,
 * never the faction.
 *
 * Sides come from the streak's `faction` and `victim_factions` (#487). With
 * no victim sides, the opponent is the other side of the demo.
 */
export function teamsForHighlight(demo, streak, projectTeams) {
  const own = streak?.faction;
  const victims = streak?.victim_factions || [];
  const start = Math.max(0, streak?.start_index ?? 0);
  let enemy = victims.slice(start).find((f) => sideKey(f)) || null;
  if (!enemy && sideKey(own)) {
    enemy = (demo?.teams || []).map((t) => t.side).find((s) => sideKey(s) && sideKey(s) !== sideKey(own)) || null;
  }
  return {
    team_name: teamName(projectTeams, sideTag(demo, own)),
    opponent: teamName(projectTeams, sideTag(demo, enemy)),
  };
}
