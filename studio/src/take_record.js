// take_record.js
//
// One entry of the project's take index (#439). Projects saved before it hold
// a bare uid array per take key; a take recorded since holds
// `{ uids, session_id, captured_at, capture_mode, capture_fps, take_folder }`.
// Both shapes are read everywhere, and an entry keeps the shape it has until
// a capture records into it, so opening and saving an old project loses
// nothing and rewrites nothing.

/** The capture facts a record keeps beside its uids. */
const CAPTURE_FIELDS = ['session_id', 'captured_at', 'capture_mode', 'capture_fps', 'take_folder'];

/** The highlight uids of an entry in either shape. */
export function uidsOf(entry) {
  if (Array.isArray(entry)) return entry;
  return Array.isArray(entry?.uids) ? entry.uids : [];
}

/** The same entry with `change` applied to its uids, in the shape it had. */
export function mapUids(entry, change) {
  if (Array.isArray(entry)) return change(entry);
  return { ...entry, uids: change(uidsOf(entry)) };
}

/**
 * The entry after a capture records `uids` into it: uids unioned, and each
 * capture fact in `meta` that is known written over the old one (a re-capture
 * into the same key is the newer capture). Always a record.
 */
export function recordInto(entry, uids, meta) {
  const record = Array.isArray(entry) || !entry ? {} : { ...entry };
  record.uids = Array.from(new Set([...uidsOf(entry), ...uids]));
  CAPTURE_FIELDS.forEach((field) => {
    const value = meta?.[field];
    if (value !== undefined && value !== null && value !== '') record[field] = value;
  });
  return record;
}
