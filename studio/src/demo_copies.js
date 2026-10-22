// demo_copies.js
// Identical copies of a demo under another name (#21). Two rows for one
// demo would capture every highlight twice, so a scan adds only the first.

/**
 * Splits freshly scanned demos into the ones to add to the queue and the
 * identical copies to skip. A copy is a demo whose file key (size + first
 * 64 KB) matches a queued demo at another path, or one earlier in this scan.
 * A rescan of a queued path is never a copy, and a demo with no key can't be
 * checked, so it's kept.
 *
 * @returns {{ keep: object[], copies: { demo: object, sameAs: object }[] }}
 */
export function splitIdenticalCopies(queued, scanned) {
  const queuedPaths = new Set(queued.map((d) => d.path));
  const byKey = new Map();
  queued.forEach((d) => {
    if (d.file_key && !byKey.has(d.file_key)) byKey.set(d.file_key, d);
  });
  const keep = [];
  const copies = [];
  scanned.forEach((demo) => {
    const original = demo.file_key ? byKey.get(demo.file_key) : undefined;
    if (!queuedPaths.has(demo.path) && original && original.path !== demo.path) {
      copies.push({ demo, sameAs: original });
      return;
    }
    if (demo.file_key && !byKey.has(demo.file_key)) byKey.set(demo.file_key, demo);
    keep.push(demo);
  });
  return { keep, copies };
}
