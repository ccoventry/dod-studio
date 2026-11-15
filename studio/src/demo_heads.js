// demo_heads.js — fills in the map and type of a listed folder's demos.
//
// A Demos-list folder listing is file names only. Reading the start of every
// demo held the whole list back until the last one was read, and on a cloud
// drive (Google Drive) each read can fetch the file over the network. The
// list shows from the names, and this reads the heads a few at a time, so
// the map column fills in while you look.

/** Demos per `read_demo_heads` call: each call's answers show at once. */
export const HEADS_PER_READ = 8;

/**
 * Reads the heads of `demos` in order, `size` at a time, writing each one's
 * `map_name` and `demo_type` into its entry. Stops when `stillWanted()` turns
 * false (another folder was opened).
 * @param {{ path: string, map_name?: string|null, demo_type?: string }[]} demos
 * @param {object} io
 * @param {(paths: string[]) => Promise<{ path: string, map_name: string, demo_type: string }[]>} io.read
 * @param {() => boolean} io.stillWanted
 * @param {() => void} io.onRead  after each call's answers are in
 * @returns {Promise<boolean>} whether every demo was read
 */
export async function readDemoHeadsInto(demos, { read, stillWanted, onRead, size = HEADS_PER_READ }) {
  const byPath = new Map(demos.map((d) => [d.path, d]));
  for (let i = 0; i < demos.length; i += size) {
    if (!stillWanted()) return false;
    const chunk = demos.slice(i, i + size);
    const heads = await read(chunk.map((d) => d.path));
    if (!stillWanted()) return false;
    for (const head of Array.isArray(heads) ? heads : []) {
      const entry = byPath.get(head?.path);
      if (!entry) continue;
      entry.map_name = head.map_name;
      if (head.demo_type) entry.demo_type = head.demo_type;
    }
    // A failed read leaves the map unknown, not "still reading" for good.
    for (const entry of chunk) if (entry.map_name == null) entry.map_name = '';
    onRead();
  }
  return true;
}

/** The map column's text: `…` until the head is read, then the map. */
export function mapCellText(entry, { reading, dash }) {
  if (entry.map_name == null) return reading;
  return entry.map_name || dash;
}
