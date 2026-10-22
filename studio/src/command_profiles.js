// command_profiles.js
// Named sets of Initial and Scheduled Commands (#442), saved under a name and
// applied in one pick. Pure, so it can be tested on its own;
// command_profiles_ui.js wires it to Configuration > Commands.
//
// The lists use the settings file's shape (`getCommandsState()`):
// `init_commands` is a list of strings, `custom_commands` a list of
// `{ command, relation, offset_seconds }`.

/** Both lists in a normalised shape: trimmed, blank rows dropped. Order is
 *  kept, since the commands run in it. */
export function profileLists(lists) {
  return {
    init_commands: (Array.isArray(lists?.init_commands) ? lists.init_commands : [])
      .map((c) => String(c ?? '').trim())
      .filter((c) => c.length > 0),
    custom_commands: (Array.isArray(lists?.custom_commands) ? lists.custom_commands : [])
      .map((c) => ({
        command: String(c?.command ?? '').trim(),
        relation: c?.relation === 'After' ? 'After' : 'Before',
        // Same fallback as hydrateCommandsState, and never NaN: the settings
        // file cannot hold one.
        offset_seconds: Number.isFinite(c?.offset_seconds) ? c.offset_seconds : 2.0,
      }))
      .filter((c) => c.command.length > 0),
  };
}

/** Whether two sets of lists hold the same commands in the same order. */
export function sameLists(a, b) {
  return JSON.stringify(profileLists(a)) === JSON.stringify(profileLists(b));
}

/** Saved profiles, cleaned up: unnamed entries dropped, lists normalised,
 *  sorted by name. */
export function normaliseProfiles(list) {
  if (!Array.isArray(list)) return [];
  return list
    .map((p) => ({ name: String(p?.name ?? '').trim(), ...profileLists(p) }))
    .filter((p) => p.name.length > 0)
    .sort(byName);
}

/** The first profile whose lists equal `lists`, or null. */
export function matchingProfile(profiles, lists) {
  return (profiles || []).find((p) => sameLists(p, lists)) || null;
}

/**
 * Which profile the Commands tab is on, and whether its lists have been
 * changed since. `activeName` is the profile last applied or saved; when it no
 * longer exists, the first profile the lists match stands in for it.
 * Returns `{ name, edited }`, with `name` empty when there is no profile.
 */
export function profileState(profiles, activeName, lists) {
  const active = (profiles || []).find((p) => p.name === activeName);
  if (active) return { name: active.name, edited: !sameLists(active, lists) };
  return { name: matchingProfile(profiles, lists)?.name || '', edited: false };
}

function byName(a, b) {
  return a.name.toLowerCase().localeCompare(b.name.toLowerCase());
}

/** `profiles` with `name` saved as `lists`, replacing a profile of the same
 *  name (ignoring case), sorted by name. Empty names are refused. */
export function saveProfile(profiles, name, lists) {
  const trimmed = String(name || '').trim();
  if (!trimmed) return profiles || [];
  const kept = (profiles || []).filter((p) => p.name.toLowerCase() !== trimmed.toLowerCase());
  return [...kept, { name: trimmed, ...profileLists(lists) }].sort(byName);
}

/** `profiles` with `from` renamed to `to`, sorted by name. Null when `to` is
 *  empty, `from` does not exist, or another profile already has the name
 *  (ignoring case). */
export function renameProfile(profiles, from, to) {
  const trimmed = String(to || '').trim();
  const list = profiles || [];
  if (!trimmed || !list.some((p) => p.name === from)) return null;
  if (list.some((p) => p.name !== from && p.name.toLowerCase() === trimmed.toLowerCase())) return null;
  return list.map((p) => (p.name === from ? { ...p, name: trimmed } : p)).sort(byName);
}

/** `profiles` without the one named `name`. */
export function deleteProfile(profiles, name) {
  return (profiles || []).filter((p) => p.name !== name);
}
