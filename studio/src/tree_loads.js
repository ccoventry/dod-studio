// tree_loads.js — which open Explorer folders still need reading (#572).
//
// The Demo Analyzer's Explorer keeps which folders are open apart from what
// it has read of each. An open folder with nothing read renders "Loading…",
// and two paths used to leave it that way for good: Refresh clears every
// listing but keeps every open folder, then reloads only the current one's
// ancestors; and a failed read while auto-opening ancestors stored nothing.

/**
 * The open folders that have no listing and no read under way.
 * @param {Iterable<string>} openNodes
 * @param {Map<string, unknown>} cache  path -> listing
 * @param {Set<string>} pending  paths being read now
 * @returns {string[]}
 */
export function unloadedOpenNodes(openNodes, cache, pending) {
  return [...openNodes].filter((path) => !cache.has(path) && !pending.has(path));
}
